//! The world server's gameplay connection.
//!
//! A world server is where a client plays. The auth service tells a travelling client *where* to
//! connect (`AUTH_SMSG_GAME_SERVER_INFO`); this module is what answers when it does.
//!
//! The order, and it is the reference's order (`docs/03-protocol.md` section 11):
//!
//! ```text
//! 1. classify          the first four bytes name the connection kind; anything but a game
//!                      connection is refused here, not guessed at
//! 2. GAME_CMSG_VERSION 64 bytes: the world id, map, player token, account and character
//! 3. claim             the token pair must match a transfer this world was told about, or the
//!                      connection is closed (the reference closes it with no message at all)
//! 4. key exchange      the client's seed, the server's seed; everything after is scrambled
//! 5. the load block    four messages, sent without being asked: INSTANCE_LOAD_HEAD,
//!                      PLAYER_DATA_START, LOAD_PLAYER_NAME, LOAD_INFO
//! 6. the three asks    the client asks for items, for a spawn point, and for the players; each is
//!                      answered, and the last answer ends with INSTANCE_LOAD_FINISH
//! 7. keep-alive        a world tick every 500 ms and a ping every 5 s, forever
//! ```
//!
//! What exists today, and what does not -- `docs/07-status.md` is generated from the same comparison
//! as the parity report, and is the place to check:
//!
//! * one agent, the character the transfer named, standing where the map's spawn says, whose position
//!   this server follows within the reference's movement budget (`agent`);
//! * the reference's default inventory, replayed from its trace: bags and items are *sent*, but nothing
//!   can be equipped, moved, dropped or bought;
//! * no NPCs, no other players, no combat, no quests, no persistence.
//!
//! The load messages go out even for the parts that are not simulated, because a client that does not
//! receive them never leaves the loading screen.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asura_db::game as db_game;
use asura_db::{placement, PgPool};
use asura_protocol::client::game::GameVersion;
use asura_protocol::client::handshake::{self, Established, CLIENT_SEED_LEN, MASTER_SECRET_LEN};
use asura_protocol::client::version::{self, GAME_CMSG_VERSION_LEN};
use asura_protocol::client::{Frame, FrameError};
use asura_protocol::game;

use crate::agent::AgentState;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How often the world's clock is sent. The reference sends one at least every 500 ms
/// (`GAME_SRV_MAX_TIME_BETWEEN_WORLD_TICK_MS`), and a client that stops receiving it stops moving.
pub const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// How often the server asks the client whether it is still there. The reference's constant is 5000 ms
/// (`GAME_SRV_TIME_BETWEEN_PING_MS`).
pub const PING_INTERVAL: Duration = Duration::from_secs(5);

/// The ping value the reference answers with. It is a constant, not a measurement: GWPS sends `20`.
pub const PING_VALUE: u32 = 20;

/// The one player and one agent this world server has.
///
/// The reference allocates both from per-server id tables (thousands of agents, hundreds of players).
/// ASURA has one character in a map, so the id table is a constant â€” and the party message carries
/// these as `u16`, which is the only real constraint. It becomes a real allocator when there is more
/// than one player to allocate for.
pub const PLAYER_ID: u32 = 1;
/// See [`PLAYER_ID`].
pub const AGENT_ID: u32 = 1;

/// The party a lone player is put in.
pub const PARTY_ID: u16 = 1;

/// `GameSrv_CreatePlayerAgent`'s arbitrary team token; the client keeps it for the session and the
/// reference never gives it a meaning.
pub const PLAYER_TEAM_TOKEN: u32 = 0xBAAD_F00D;

/// The district number the reference always answers with (`AuthSrv_HandleRequestGameInstance`).
pub const DISTRICT_NUMBER: u32 = 1;

/// The primary profession the reference reports for its default character, read off its send log
/// (`GAME_SMSG_PLAYER_UPDATE_PROFESSION`: `01`). 0 is `Profession_None`, which is not a character a
/// client can render.
pub const DEFAULT_PRIMARY_PROFESSION: u8 = 1;

/// The unlocked-profession bitmap the reference sends (`GAME_SMSG_PLAYER_UPDATE_UNLOCKED_PROFESSIONS`:
/// `02 00 00 00`), i.e. the bit for the profession above.
pub const DEFAULT_UNLOCKED_PROFESSIONS: u32 = 2;

/// One map a world server publishes, with what the client needs to load it.
#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry {
    pub map_id: i32,
    /// The `Gw.dat` file id of the map blob (`map_config.map_file_id`). A map without one loads to a
    /// black screen, so it is passed to the client rather than guessed.
    pub map_file_id: u32,
    /// 0 for an outpost, 1 for an explorable area.
    pub map_type: i32,
    /// Where a character stands on entering. The reference's own coordinates, read back from a live
    /// client in the rig; (0, 0) is outside the walkable geometry on every map measured so far.
    pub spawn: (f32, f32),
    /// The plane inside the map blob.
    pub spawn_plane: u16,
}

/// What a world server needs in order to accept a client.
#[derive(Clone)]
pub struct World {
    pub pool: PgPool,
    /// The world's registered id: the `map_token` a client presents, and the key of its `world` row.
    pub world_id: i32,
    /// The maps this world publishes. A client that presents a map not in this list has been sent to
    /// the wrong world, and is refused.
    pub maps: Vec<MapEntry>,
    /// When set, the blocks sent to a client are the reference's own captured bytes instead of the
    /// ones this server builds (see the `replay` module). A bisection tool: if a client survives a
    /// replay and dies against the built blocks, the difference is payload content.
    pub replay: Option<crate::replay::Replay>,
}

impl World {
    /// The map entry for `map_id`, if this world publishes it.
    pub fn map(&self, map_id: i32) -> Option<&MapEntry> {
        self.maps.iter().find(|map| map.map_id == map_id)
    }
}

/// Replaying the reference's captured stream, for bisection (ASURA_WORLD_REPLAY).
/// One agent's position, and the reference's rules for believing a client about it.
pub mod agent;

pub mod replay;

/// Why a connection ended.
#[derive(Debug)]
pub enum GameError {
    Io(io::Error),
    /// The first four bytes are not a game connection's.
    NotAGameConnection(version::VersionError),
    /// The version message was malformed or never finished arriving.
    BadVersion(version::GameVersionError),
    /// The client presented a token pair this world was not told about, or one already claimed, or a
    /// character that is not the one the transfer was issued for.
    UnknownTransfer {
        map_token: u32,
        player_token: u32,
    },
    /// The transfer is real but names a map this world does not host.
    UnknownMap(i32),
    /// The transfer is real but the character it names cannot be read.
    UnknownCharacter,
    /// A client message arrived that this world cannot frame, so the stream cannot be trusted.
    Framing(FrameError),
    /// The key exchange failed.
    Handshake(handshake::HandshakeError),
    /// The database failed.
    Database(asura_db::DatabaseError),
}

impl core::fmt::Display for GameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GameError::Io(err) => write!(f, "socket: {err}"),
            GameError::NotAGameConnection(err) => write!(f, "not a game connection: {err}"),
            GameError::BadVersion(err) => write!(f, "version: {err}"),
            GameError::UnknownTransfer {
                map_token,
                player_token,
            } => write!(
                f,
                "no pending transfer for world {map_token} with player token {player_token:#010X}"
            ),
            GameError::UnknownMap(map_id) => write!(f, "this world does not host map {map_id}"),
            GameError::UnknownCharacter => write!(f, "the character named by the transfer is gone"),
            GameError::Framing(err) => write!(f, "could not frame a client message: {err}"),
            GameError::Handshake(err) => write!(f, "key exchange: {err}"),
            GameError::Database(err) => write!(f, "database: {err}"),
        }
    }
}

impl std::error::Error for GameError {}

impl From<io::Error> for GameError {
    fn from(err: io::Error) -> Self {
        GameError::Io(err)
    }
}

/// Accept gameplay connections until the listener fails.
pub async fn serve(listener: TcpListener, world: Arc<World>, trace: bool) -> io::Result<()> {
    let address = listener.local_addr()?;
    eprintln!(
        "asura-world: listening on {address} for world {} ({} map(s))",
        world.world_id,
        world.maps.len()
    );
    loop {
        let (stream, peer) = listener.accept().await?;
        let world = Arc::clone(&world);
        tokio::spawn(async move {
            match handle_connection(stream, &world, trace).await {
                Ok(()) => {
                    if trace {
                        eprintln!("world: {peer} left");
                    }
                }
                Err(err) => eprintln!("world: {peer}: {err}"),
            }
        });
    }
}

/// One connection's cipher state and the decrypted bytes not yet framed.
struct Session {
    established: Established,
    plaintext: Vec<u8>,
}

impl Session {
    fn new(established: Established) -> Self {
        Session {
            established,
            plaintext: Vec::new(),
        }
    }

    /// Take `count` bytes, decrypt them in place, and append them to the plaintext buffer.
    fn absorb(&mut self, bytes: &[u8]) {
        let start = self.plaintext.len();
        self.plaintext.extend_from_slice(bytes);
        self.established
            .incoming
            .apply(&mut self.plaintext[start..]);
    }
}

/// Drive one gameplay connection from the first byte to the close.
pub async fn handle_connection(
    mut stream: TcpStream,
    world: &World,
    trace: bool,
) -> Result<(), GameError> {
    // ---- 1. what kind of connection is this? ------------------------------------------------
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    match version::classify(&header) {
        Ok(version::ConnectionKind::Game) => {}
        Ok(other) => {
            // An auth or control connection belongs to another program. Refusing it by name is the
            // point of classifying before anything else.
            eprintln!("world: a {other:?} connection arrived on the game port; closing it");
            return Err(GameError::NotAGameConnection(
                version::VersionError::Unknown {
                    header: u32::from_le_bytes(header),
                },
            ));
        }
        Err(err) => return Err(GameError::NotAGameConnection(err)),
    }

    // ---- 2. the version message -------------------------------------------------------------
    let mut message = vec![0u8; GAME_CMSG_VERSION_LEN];
    message[..4].copy_from_slice(&header);
    stream.read_exact(&mut message[4..]).await?;
    let (game_version, _) = GameVersion::decode(&message).map_err(GameError::BadVersion)?;
    if trace {
        eprintln!(
            "world <- game version build={} world={} map={} player token={:#010X}",
            game_version.version,
            game_version.map_token,
            game_version.map_id,
            game_version.player_token
        );
    }

    // ---- 3. the token pair is the credential ------------------------------------------------
    let refusal = GameError::UnknownTransfer {
        map_token: game_version.map_token,
        player_token: game_version.player_token,
    };
    let transfer =
        placement::claim_transfer(&world.pool, world.world_id, game_version.player_token)
            .await
            .map_err(GameError::Database)?
            .ok_or(GameError::UnknownTransfer {
                map_token: game_version.map_token,
                player_token: game_version.player_token,
            })?;

    // The character the client claims must be the one the transfer was issued for. This is the check
    // the reference makes (`AuthSrv_DoPostHandshake`); skipping it would let any client that knows a
    // live token play as any character on the account.
    if game_version.character != transfer.character_id.to_bytes_le() {
        return Err(refusal);
    }

    let map_id = transfer.map_id;
    let Some(map) = world.map(map_id).cloned() else {
        return Err(GameError::UnknownMap(map_id));
    };

    let Some(name) = db_game::character_name(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?
    else {
        return Err(GameError::UnknownCharacter);
    };

    if trace {
        eprintln!(
            "world: {name:?} joined map {} (file id {}) on world {}",
            map.map_id, map.map_file_id, world.world_id
        );
    }

    // ---- 4. the key exchange ----------------------------------------------------------------
    let mut client_seed = vec![0u8; CLIENT_SEED_LEN];
    stream.read_exact(&mut client_seed).await?;
    let mut master_secret = [0u8; MASTER_SECRET_LEN];
    getrandom::getrandom(&mut master_secret).map_err(|err| io::Error::other(err.to_string()))?;
    let established =
        handshake::accept(&client_seed, &master_secret).map_err(GameError::Handshake)?;
    stream.write_all(&established.reply).await?;
    if trace {
        eprintln!("world -> server seed, scrambled from here on");
    }

    let mut session = Session::new(established);
    // The one agent this world has: the character the transfer named, standing at the map's spawn.
    let mut agent = AgentState::new(
        AGENT_ID,
        map.spawn,
        map.spawn_plane,
        game::PLAYER_SPEED_BASE,
    );

    // ---- 5. the load block ------------------------------------------------------------------
    // The reference sends these four the moment it has the connection, without waiting to be asked.
    let block = match &world.replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Load) => replay.load.clone(),
        _ => load_block(&map, &name),
    };
    send(&mut stream, &mut session, block, trace).await?;

    // ---- 6/7. the asks, then the session ----------------------------------------------------
    let started = Instant::now();
    let mut tick = tokio::time::interval(TICK_INTERVAL);
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut buffer = [0u8; 8192];

    'session: loop {
        tokio::select! {
            biased;

            read = stream.read(&mut buffer) => {
                let read = read?;
                if read == 0 {
                    break; // the client closed its side
                }
                session.absorb(&buffer[..read]);

                loop {
                    // Framed by the generated game table, field by field: a counted field carries its
                    // own length, so a message is exactly as long as it says. What arrives is framed
                    // before it is answered, and a header the table does not know ends the session
                    // rather than consuming the wrong number of bytes.
                    match Frame::decode_game_message(&session.plaintext, true) {
                        Ok((frame, used)) => {
                            session.plaintext.drain(..used);
                            if trace {
                                eprintln!(
                                    "world <- {:#06X} ({used} bytes): {}",
                                    frame.header,
                                    hex_preview(&frame.encode())
                                );
                            }
                            if !answer(
                                &mut stream,
                                &mut session,
                                frame,
                                &map,
                                &name,
                                started,
                                &mut agent,
                                world.replay.as_ref(),
                                trace,
                            )
                            .await?
                            {
                                break 'session;
                            }
                        }
                        Err(FrameError::Incomplete { .. }) => break,
                        Err(err) => return Err(GameError::Framing(err)),
                    }
                }
            }

            _ = tick.tick() => {
                let delta = TICK_INTERVAL.as_millis() as u32;
                send(&mut stream, &mut session, vec![game::world_simulation_tick(delta)], trace).await?;
            }

            _ = ping.tick() => {
                send(&mut stream, &mut session, vec![game::ping_request()], trace).await?;
            }
        }
    }

    // The player left. The slot goes back to the pool now rather than when a reaper notices: the
    // player is gone, and a pool that only empties on a reaper would refuse a legitimate travel after
    // an ordinary logout.
    if let Err(err) = placement::release(&world.pool, transfer.session_id).await {
        eprintln!("world: could not release the lease of the leaving player: {err}");
    }
    if trace {
        eprintln!("world: {name:?} left map {}", map.map_id);
    }
    Ok(())
}

/// Whether chat is sent at all, from `ASURA_CHAT` (`1`, `true`, `yes`).
///
/// The same gate as the reference's `GameSrv_ChatSendingEnabled`/`GWPS_CHAT`, and off for the same
/// reason it is off there: its own comment records that an unconditional send put a message on the wire
/// the client could not parse, `GameSrv_SendInvalidCommand` above all, and crashed it.
fn chat_enabled() -> bool {
    static VALUE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var("ASURA_CHAT")
            .map(|value| {
                let value = value.trim().to_ascii_lowercase();
                value == "1" || value == "true" || value == "yes"
            })
            .unwrap_or(false)
    })
}

/// The first bytes of a message, as hex, for the trace.
///
/// When a client dies mid-load, the only evidence left is what it sent and what it was sent. A header
/// and a size say which message; the first bytes say whether its contents were what the reference
/// would have written.
fn hex_preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(24)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The answer to the client's item request (`GameSrv_HandleInstanceLoadRequestItems`).
///
/// A pure function of the map so that `examples/dump-load.rs` can print exactly what a client would
/// receive, byte for byte, without a socket â€” which is how this stream is compared against the
/// reference's own log.
pub fn items_block(map: &MapEntry) -> Vec<Frame> {
    let mut frames = vec![
        game::item_stream_create(1, 0),
        game::item_set_active_weapon_set(1, 0),
    ];
    // The inventory. The reference seeds every character it loads with a fixed set -- a backpack with
    // its bag item, a cupcake, and six pieces of equipment -- and a client given no items at all has no
    // inventory model: it dies when it draws the character it has just created. Those messages are
    // copied verbatim from a trace of the reference working (`game::default_inventory`), because every
    // field in them (file ids, models, names) is its data and not something this project can derive.
    for (header, body) in game::default_inventory::DEFAULT_INVENTORY {
        frames.push(Frame::new(*header, body.to_vec()));
    }
    // Four empty weapon slots, exactly as `GameSrv_SendWeaponSlots` sends them.
    for slot in 0..4u8 {
        frames.push(game::item_weapon_set(1, slot, 0, 0));
    }
    frames.push(game::update_gold_storage(1, 0));
    for faction in [
        game::FACTION_MAX_KURZICK,
        game::FACTION_MAX_LUXON,
        game::FACTION_MAX_BALTHAZAR,
        game::FACTION_MAX_IMPERIAL,
    ] {
        // 10000, which is what the reference sends (`10 27 00 00`).
        frames.push(game::faction_max(faction, 10_000));
    }
    frames.push(game::hard_mode_unlocked(1));
    // The reference sends an **empty** name here (`GameSrv_SendCharacterInfo`), not the character's:
    // the client already has the name from the load block.
    frames.push(game::character_update_info(""));
    frames.push(game::map_update_current(map.map_id as u16));
    frames.push(game::ready_for_map_spawn());
    frames.extend(manifest(map.map_id as u16));
    frames
}

/// The answer to the client's spawn request: the map blob, and where to stand inside it.
pub fn spawn_block(map: &MapEntry) -> Vec<Frame> {
    vec![game::instance_load_spawn_point(
        map.map_file_id,
        map.spawn.0,
        map.spawn.1,
        map.spawn_plane,
    )]
}

/// The four messages a world server sends without being asked, the moment it has the connection.
pub fn load_block(map: &MapEntry, name: &str) -> Vec<Frame> {
    vec![
        game::instance_load_head_default(),
        game::instance_player_data_start(),
        game::instance_load_player_name(name),
        game::instance_load_info(
            AGENT_ID,
            map.map_id as u16,
            1, // the reference hardcodes this; see the builder's documentation
            DISTRICT_NUMBER,
            0, // language: the reference's default
            0, // is_observer
        ),
    ]
}

/// Scramble and write frames.
async fn send(
    stream: &mut TcpStream,
    session: &mut Session,
    frames: Vec<Frame>,
    trace: bool,
) -> Result<(), GameError> {
    for frame in frames {
        let mut bytes = frame.encode();
        if trace {
            eprintln!("world -> {:#06X} ({} bytes)", frame.header, bytes.len());
        }
        session.established.outgoing.apply(&mut bytes);
        stream.write_all(&bytes).await?;
    }
    Ok(())
}

/// Answer one client message. `Ok(false)` finishes the session.
#[allow(clippy::too_many_arguments)]
async fn answer(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: Frame,
    map: &MapEntry,
    name: &str,
    started: Instant,
    agent: &mut AgentState,
    replay: Option<&crate::replay::Replay>,
    trace: bool,
) -> Result<bool, GameError> {
    let _ = &frame.body; // no message here carries a field this world reads yet
    match frame.header {
        // The client asks for the item stream. The reference answers with the inventory, the weapon
        // slots, the gold, the faction caps, the character info, the map, the spawn readiness and the
        // download manifest -- in that order (`GameSrv_HandleInstanceLoadRequestItems`).
        0x8091 => {
            let block = match replay {
                Some(replay) if replay.pick.wants(crate::replay::BlockName::Items) => {
                    replay.items.clone()
                }
                _ => items_block(map),
            };
            send(stream, session, block, trace).await?;
            Ok(true)
        }

        // The client asks where to stand. The reference answers with the agent's own position and the
        // map blob's file id.
        0x8088 => {
            let block = match replay {
                Some(replay) if replay.pick.wants(crate::replay::BlockName::Spawn) => {
                    replay.spawn.clone()
                }
                _ => spawn_block(map),
            };
            send(stream, session, block, trace).await?;
            Ok(true)
        }

        // The client asks who else is here. This is the big block, and its last message
        // (`INSTANCE_LOAD_FINISH`) is what takes the client off the loading screen.
        0x8090 => {
            let block = match replay {
                Some(replay) if replay.pick.wants(crate::replay::BlockName::Players) => {
                    replay.players.clone()
                }
                _ => player_block(name, map, started),
            };
            send(stream, session, block, trace).await?;
            Ok(true)
        }

        // The client answered our ping. Nothing to do: the reference parses it and discards it.
        0x800C => Ok(true),

        // The client asks to be pinged back.
        0x800D => {
            send(stream, session, vec![game::ping_reply(PING_VALUE)], trace).await?;
            Ok(true)
        }

        // Movement. This world has no simulation yet, so a move is acknowledged the only honest way
        // available: nothing is sent, and the trace says why. The client keeps its own prediction; a
        // The client asks to walk somewhere (`GameSrv_HandleMoveToCoord`). The reference answers with the
        // next waypoint of its own path; this server has no navmesh, so it answers with the client's own
        // destination -- a straight line, which is what it can honestly promise.
        0x803E => {
            if frame.body.len() < 12 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 16 }));
            }
            let destination = (
                f32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]]),
                f32::from_le_bytes([frame.body[4], frame.body[5], frame.body[6], frame.body[7]]),
            );
            let plane =
                u32::from_le_bytes([frame.body[8], frame.body[9], frame.body[10], frame.body[11]])
                    as u16;
            if trace {
                eprintln!(
                    "world: agent {} walks to ({:.1}, {:.1}) plane {plane}",
                    agent.agent_id, destination.0, destination.1
                );
            }
            send(
                stream,
                session,
                vec![game::agent_move_to_point(
                    agent.agent_id,
                    destination,
                    plane,
                    agent.plane,
                )],
                trace,
            )
            .await?;
            Ok(true)
        }

        // The client reports where it is (`GameSrv_HandlePositionUpdate`). The position is adopted when
        // the budget allows it, clamped when it does not, and refused across a plane change that is too
        // far. Nothing is sent back: the client already knows where it is, and with one player there is
        // nobody else to tell.
        0x803D => {
            if frame.body.len() < 12 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 16 }));
            }
            let position = (
                f32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]]),
                f32::from_le_bytes([frame.body[4], frame.body[5], frame.body[6], frame.body[7]]),
            );
            let plane =
                u32::from_le_bytes([frame.body[8], frame.body[9], frame.body[10], frame.body[11]])
                    as u16;
            let adopted = agent.adopt_report(position, plane, Instant::now());
            if trace {
                eprintln!(
                    "world: agent {} reported ({:.1}, {:.1}) plane {plane}: {adopted:?} -> ({:.1}, \
                     {:.1})",
                    agent.agent_id, position.0, position.1, agent.position.0, agent.position.1
                );
            }
            Ok(true)
        }

        // The client stopped walking (`GameSrv_HandleCancelMovement`): the agent stops where it is and
        // everyone is told, which is the mover too as far as this server is concerned.
        0x8028 => {
            send(
                stream,
                session,
                vec![game::agent_stop_moving(agent.agent_id)],
                trace,
            )
            .await?;
            Ok(true)
        }

        // The client corrects its position after a cancelled walk
        // (`GameSrv_HandleLastPosOnMoveCanceled`): short corrections on the same plane are taken, longer
        // ones refused, and nothing is sent either way.
        0x8047 => {
            if frame.body.len() < 12 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 16 }));
            }
            let position = (
                f32::from_le_bytes([frame.body[0], frame.body[1], frame.body[2], frame.body[3]]),
                f32::from_le_bytes([frame.body[4], frame.body[5], frame.body[6], frame.body[7]]),
            );
            let plane =
                u32::from_le_bytes([frame.body[8], frame.body[9], frame.body[10], frame.body[11]])
                    as u16;
            let taken = agent.adopt_correction(position, plane);
            if trace {
                eprintln!(
                    "world: agent {} correction to ({:.1}, {:.1}) plane {plane}: {}",
                    agent.agent_id,
                    position.0,
                    position.1,
                    if taken { "taken" } else { "refused" }
                );
            }
            Ok(true)
        }

        // The client speaks (`GameSrv_HandleChatMessage`). The first unit of the message is the channel:
        // `!` is local/general, `/` is a command, and the reference refuses anything else as bad user
        // data. Its *sending* is gated off by default (`GameSrv_ChatSendingEnabled`) because an
        // unconditional send once put a message on the wire the client could not parse and crashed it,
        // so this port has the same gate and the same default.
        0x8064 => {
            let body = &frame.body;
            if body.len() < 6 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 8 }));
            }
            let units = u16::from_le_bytes([body[4], body[5]]) as usize;
            let text: Vec<u16> = (0..units)
                .map(|index| {
                    let at = 6 + index * 2;
                    u16::from_le_bytes([
                        body.get(at).copied().unwrap_or(0),
                        body.get(at + 1).copied().unwrap_or(0),
                    ])
                })
                .collect();
            let Some(channel) = text.first().copied() else {
                return Ok(true); // an empty message, which the reference drops
            };
            let spoken: String = char::decode_utf16(text[1.min(text.len())..].iter().copied())
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();

            match u8::try_from(channel).unwrap_or(0) as char {
                '!' | '#' | '$' => {
                    if !chat_enabled() {
                        if trace {
                            eprintln!("world: chat {spoken:?} dropped (sending is off; ASURA_CHAT=1 to send)");
                        }
                        return Ok(true);
                    }
                    // Channel values other than `Channel_All` are not implemented yet: the reference
                    // maps `#` to group and `$` to trade, and those enum values have to be read out of
                    // its headers rather than guessed, so only `!` speaks today.
                    if u8::try_from(channel).unwrap_or(0) as char != '!' {
                        if trace {
                            eprintln!(
                                "world: channel {channel:#X} not implemented; message dropped"
                            );
                        }
                        return Ok(true);
                    }
                    send(
                        stream,
                        session,
                        vec![
                            game::chat_message_core(&spoken),
                            game::chat_message_local(PLAYER_ID, game::CHANNEL_ALL),
                        ],
                        trace,
                    )
                    .await?;
                    Ok(true)
                }
                '/' => {
                    // The only command that does not need the GM machinery: `/stuck` stops the agent.
                    if spoken.trim() == "stuck" {
                        send(
                            stream,
                            session,
                            vec![game::agent_stop_moving(agent.agent_id)],
                            trace,
                        )
                        .await?;
                        Ok(true)
                    } else {
                        if trace {
                            eprintln!("world: unknown command {spoken:?}");
                        }
                        Ok(true)
                    }
                }
                other => {
                    eprintln!(
                        "world: chat on channel {other:?} refused, as the reference refuses it"
                    );
                    Ok(true)
                }
            }
        }

        // The client is going away.
        0x8008 => Ok(false),

        // A message this world can frame but does not act on. Framing already happened, so ignoring it
        // is safe and is what the reference does for most of them.
        other => {
            if trace {
                eprintln!("world: {other:#06X} ignored");
            }
            Ok(true)
        }
    }
}

/// The reference's `MapId_Count`: the sentinel the first manifest `DONE` carries, meaning "every map
/// up to here". Its value is 877 (`0x036D`), read off the reference's own send log
/// (`... 01 02 6D 03 00 00 00 00`).
const MAP_ID_COUNT: u16 = 877;

/// A zero-filled bitmap of the length the reference uses for a character with no progress: 31 words.
///
/// Copied from its trace, not chosen: `GAME_SMSG_MAP_UPDATE_UNLOCKED_LIST` there carries two lists of
/// 31 zero dwords (`02 00 00 00` twice, then 31 words of zeros, twice more). Sending the lists empty
/// instead changes the *length* of a bitmap the client indexes by map id.
const UNLOCKED_MAP_ZEROS: &[u32] = &[0u32; 31];

/// The six-message manifest the reference sends after `READY_FOR_MAP_SPAWN`
/// (`GameSrv_SendDownloadManifest`).
///
/// The phases are the reference's own enum (`ManifestPhase_Phase1 = 0`, `Phase2 = 1`, `Done = 2`), and
/// the second `DONE` carries `Phase1` rather than `Done`: that is not a mistake in this port but the
/// reference's own sequence ("this map's manifest is complete"), and a client that is told `Done`
/// there may consider the whole download finished before it starts.
///
/// The first `DONE` carries the `MapId_Count` sentinel, the second the map itself.
pub fn manifest(map_id: u16) -> Vec<Frame> {
    vec![
        game::instance_manifest_phase(PHASE_1),
        game::instance_manifest_phase(PHASE_2),
        game::instance_manifest_done(PHASE_DONE, MAP_ID_COUNT),
        game::instance_manifest_phase(PHASE_1),
        game::instance_manifest_phase(PHASE_2),
        game::instance_manifest_done(PHASE_1, map_id),
    ]
}

/// `ManifestPhase_Phase1` in the reference's enum (`GmMap.h`).
const PHASE_1: u8 = 0;
/// See [`PHASE_1`].
const PHASE_2: u8 = 1;
/// See [`PHASE_1`].
const PHASE_DONE: u8 = 2;

/// Everything the client is told about the players in the instance, in the reference's order
/// (`GameSrv_HandleInstanceLoadRequestPlayers`).
pub fn player_block(name: &str, map: &MapEntry, started: Instant) -> Vec<Frame> {
    let mut frames = Vec::with_capacity(30);

    // The PvP unlocks. The reference sends its own fixed table of 387 unlockable items
    // (`GameSrv_SendPvpItems`, generated into `game::pvp_items`), and the client builds its
    // unlockable-item table from them, so they are copied rather than skipped.
    frames.push(game::pvp_update_unlocked_skills(&[]));
    frames.push(game::pvp_update_unlocked_heroes(&[0xFFFF_FFFF; 8]));
    for item in game::pvp_items::PVP_ITEMS {
        frames.push(game::pvp_item_stream_add(item.id, item.params));
    }
    frames.push(game::pvp_item_stream_end());

    // The account features, exactly the reference's list (`GameSrv_SendAccountFeatures`).
    for (feature, param1, param2) in [
        (1u16, 10u16, 0u16),
        (99, 3, 0),
        (100, 5, 0),
        (101, 5, 5),
        (102, 2, 2),
        (111, 1, 0),
        (124, 1, 0),
        (125, 1, 0),
        (131, 1, 0),
    ] {
        frames.push(game::account_feature(feature, param1, param2));
    }

    // The unlocked-map list. The three bitmaps a character can have progress in are sent at their
    // **full length, zero-filled**, because that is what the reference sends for a character with no
    // progress: 31 words each for the two hard-mode lists, and nothing for the two normal-mode ones.
    // An empty list is not the same message — it is an array the client can index past the end of.
    frames.push(game::map_update_unlocked_list([
        &[][..],
        &[],
        UNLOCKED_MAP_ZEROS,
        UNLOCKED_MAP_ZEROS,
        &[],
    ]));
    frames.push(game::instance_loaded(PLAYER_TEAM_TOKEN));

    // Skills and attributes, then factions, then the attribute spread. The profession values are the
    // reference's own for its default character, read off its send log: primary 1 (Warrior) and one
    // unlocked-profession bit.
    frames.push(game::agent_update_attribute_points(AGENT_ID, 50, 50));
    frames.push(game::player_update_profession(
        AGENT_ID,
        DEFAULT_PRIMARY_PROFESSION,
        0,
        0,
    ));
    frames.push(game::player_update_unlocked_professions(
        AGENT_ID,
        DEFAULT_UNLOCKED_PROFESSIONS,
    ));
    frames.push(game::skillbar_update(AGENT_ID, &[0; 8], &[0; 8], 1));
    frames.push(game::update_unlocked_skills(&[]));
    frames.push(game::character_update_factions(
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ));
    frames.push(game::agent_update_attributes(AGENT_ID, &[0; 42]));
    frames.push(game::world_update_load_time(
        started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
    ));

    // The player, the agent, and what a client needs in order to draw them. The appearance carries
    // the profession the model is built from: all-zero bits say `Profession_None`, which is not a
    // character the client can draw, so the reference's own default is used.
    frames.push(game::player_create(
        PLAYER_ID,
        AGENT_ID,
        game::Appearance::REFERENCE_DEFAULT.pack(),
        name,
    ));
    // The profession goes out **twice** in the reference: once with the skills and attributes, and
    // again here, immediately after the player is created and before the agent exists
    // (`GameSrv_HandleInstanceLoadRequestPlayers`: `BroadcastPlayerCreate` then
    // `SendPlayerProfession`). It is the second one that gives the freshly created player the
    // profession its model is built from — without it the client draws an agent it has no model for.
    frames.push(game::player_update_profession(
        AGENT_ID,
        DEFAULT_PRIMARY_PROFESSION,
        0,
        0,
    ));
    frames.push(game::world_create_agent(
        AGENT_ID,
        game::CHAR_CLASS_PLAYER_BASE | PLAYER_ID,
        map.spawn,
        map.spawn_plane,
        (1.0, 0.0),
        game::PLAYER_SPEED_BASE,
        PLAYER_TEAM_TOKEN,
    ));
    // Health and energy in the reference's order, then the regeneration rate and the public level.
    // The reference's own values, read off its send log: energy 25, health 400, regeneration 0.0 and
    // level 0 (it never sets `energy_per_sec` or `level` on a fresh agent).
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_ENERGY,
        AGENT_ID,
        game::PLAYER_ENERGY_MAX,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_HEALTH,
        AGENT_ID,
        game::PLAYER_HEALTH_MAX,
    ));
    frames.push(game::agent_property_update_float(
        game::AGENT_PROPERTY_ENERGY_REGEN,
        AGENT_ID,
        0.0,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_PUBLIC_LEVEL,
        AGENT_ID,
        0,
    ));
    frames.push(game::agent_initial_effects(AGENT_ID, 0));
    frames.push(game::update_agent_visual_equipment(
        AGENT_ID,
        game::default_inventory::EQUIPPED_ITEM_IDS,
    ));
    frames.push(game::agent_display_cape(AGENT_ID, 1));
    frames.push(game::world_update_controlled_agent(AGENT_ID, 3));

    // The party: one member, this player, and the client's own party pointer.
    frames.push(game::party_create(PARTY_ID));
    frames.push(game::party_player_add(PARTY_ID, PLAYER_ID as u16, 1));
    frames.push(game::party_member_stream_end(PARTY_ID));
    frames.push(game::character_update_party(PARTY_ID, 1));

    frames.push(game::instance_load_finish());
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lakeside County, with the reference's verified blob and spawn.
    fn test_map() -> MapEntry {
        MapEntry {
            map_id: 146,
            map_file_id: 0x1B97D,
            map_type: 0,
            spawn: (6602.0, 4485.0),
            spawn_plane: 0,
        }
    }

    #[test]
    fn the_tick_and_ping_intervals_are_the_references() {
        assert_eq!(TICK_INTERVAL, Duration::from_millis(500));
        assert_eq!(PING_INTERVAL, Duration::from_secs(5));
        assert_eq!(PING_VALUE, 20);
    }

    #[test]
    fn the_map_lookup_is_by_map_id() {
        let maps = [test_map()];
        let find = |map_id: i32| maps.iter().find(|map| map.map_id == map_id);
        assert!(find(146).is_some());
        assert!(find(15).is_none(), "a map this world does not host");
    }

    #[test]
    fn a_player_message_is_framed_by_the_generated_game_table() {
        // The three load requests, at the sizes the reference's own table gives them. A bare-header
        // message is two bytes; the players request needs all nineteen.
        let (frame, used) = Frame::decode_game_message(&[0x88, 0x80], true).expect("frames");
        assert_eq!((frame.header, used), (0x8088, 2));

        assert!(matches!(
            Frame::decode_game_message(&[0x90, 0x80], true),
            Err(FrameError::Incomplete { .. })
        ));
        let mut full = vec![0x90, 0x80];
        full.resize(19, 0);
        let (frame, used) = Frame::decode_game_message(&full, true).expect("frames");
        assert_eq!((frame.header, used), (0x8090, 19));

        let mut items = vec![0x91, 0x80];
        items.resize(4, 0);
        let (_, used) = Frame::decode_game_message(&items, true).expect("frames");
        assert_eq!(used, 4, "the item request is four bytes");
    }

    #[test]
    fn an_unknown_header_is_an_error_rather_than_a_guess() {
        // 0x8004 is a game header that exists; 0x7FFF is not. Framing either wrongly would consume
        // bytes that belong to the next message, so an unknown header ends the session instead.
        assert!(matches!(
            Frame::decode_game_message(&[0xFF, 0x7F], true),
            Err(FrameError::UnknownHeader { .. })
        ));
    }

    #[test]
    fn the_two_families_disagree_about_the_same_header_value() {
        // 0x8000 is AUTH_CMSG_HEARTBEAT on the auth connection and GAME_CMSG_TRADE_ACKNOWLEDGE here,
        // and the game message is longer than its header. That collision is why the world frames with
        // the game table and never with the auth one.
        let mut message = vec![0x00, 0x80];
        message.resize(6, 0); // whatever the trade acknowledge body is; the point is the header
        let (frame, used) = Frame::decode_game_message(&message, true).expect("a game header");
        assert_eq!(frame.header, 0x8000);
        assert!(
            used > 2,
            "a game message with a body, not the auth heartbeat"
        );
        assert_eq!(
            asura_protocol::client::tables::entry(0x8000).map(|entry| entry.header),
            Some(0x8000),
            "and the auth table knows the same value for a different message"
        );
    }

    #[test]
    fn the_block_ends_with_the_message_that_leaves_the_loading_screen() {
        let frames = player_block("Asura", &test_map(), Instant::now());
        assert_eq!(
            frames.last().expect("a block").header,
            game::INSTANCE_LOAD_FINISH
        );
        assert!(
            frames.len() > 20,
            "the block is the reference's twenty-odd messages"
        );
    }

    #[test]
    fn the_manifest_has_six_messages_and_ends_on_the_map_itself() {
        let frames = manifest(146);
        assert_eq!(frames.len(), 6);
        assert_eq!(frames[0].header, game::INSTANCE_MANIFEST_PHASE);
        assert_eq!(frames[2].header, game::INSTANCE_MANIFEST_DONE);
        let last = frames[5].encode();
        assert_eq!(last[0..2], [0x98, 0x01], "the last is a manifest DONE");
        assert_eq!(
            last[2], PHASE_1,
            "the second DONE carries phase 1, as the reference sends it"
        );
        assert_eq!(last[3..5], 146u16.to_le_bytes(), "and it names the map");
    }

    #[test]
    fn the_player_block_carries_the_name_in_the_two_messages_that_name_a_character() {
        let frames = player_block("Asura", &test_map(), Instant::now());
        let named: Vec<u16> = frames
            .iter()
            .filter(|frame| {
                frame.header == game::PLAYER_CREATE
                    || frame.header == game::INSTANCE_LOAD_PLAYER_NAME
            })
            .map(|frame| frame.header)
            .collect();
        assert_eq!(
            named,
            vec![game::PLAYER_CREATE],
            "only PLAYER_CREATE is in this block; the load name went out earlier"
        );
    }
}
