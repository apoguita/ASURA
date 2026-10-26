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
//! * one agent, the character the transfer named, standing where the map's spawn says — or where it
//!   stood when it last left, which is saved and read back (`asura_db::game::record_position`);
//! * the character's inventory, read from rows and built into the reference's messages, including the
//!   nine items `UPDATE_AGENT_VISUAL_EQUIPMENT` puts on the model (`asura_db::inventory`);
//! * `GAME_CMSG_DROP_ITEM`, which puts an item agent on the ground — and, as in the reference, does not
//!   take the item out of the bag;
//! * no NPCs, no other players, no combat, no quests, no picking anything up.
//!
//! The load messages go out even for the parts that are not simulated, because a client that does not
//! receive them never leaves the loading screen.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asura_db::game as db_game;
use asura_db::inventory::{self as db_inventory, Inventory};
use asura_db::{placement, PgPool, Uuid};
use asura_protocol::client::game::GameVersion;
use asura_protocol::client::handshake::{self, Established, CLIENT_SEED_LEN, MASTER_SECRET_LEN};
use asura_protocol::client::version::{self, GAME_CMSG_VERSION_LEN};
use asura_protocol::client::{Frame, FrameError};
use asura_protocol::game;

use crate::agent::Agents;
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

/// Where a character stands as it enters a map.
///
/// Two sources, and which one applies is the whole of the persistence rule: the position the character
/// was saved at when it last left **if it was saved in this map**, and otherwise the map's own spawn
/// point. A character that has never been played has no saved position, and one that was saved in a
/// different map is arriving here for the first time (`asura_db::game::position`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Standing {
    pub position: (f32, f32),
    pub plane: u16,
}

impl Standing {
    /// The map's own spawn point: where the map row says a character arrives.
    pub fn from_map(map: &MapEntry) -> Standing {
        Standing {
            position: map.spawn,
            plane: map.spawn_plane,
        }
    }

    /// This character's standing, from what the database remembers about it.
    ///
    /// The saved position is used only when it is a position *in this map*: a character saved in another
    /// map has no coordinate here, and using one would put it somewhere the map's geometry does not
    /// contain. `None` is a character that has never been saved, which is every character's first visit.
    pub fn resume(map: &MapEntry, saved: Option<db_game::Position>) -> Standing {
        match saved {
            Some(saved) if saved.map_id == map.map_id => Standing {
                position: (saved.x, saved.y),
                plane: saved.plane.max(0) as u16,
            },
            _ => Standing::from_map(map),
        }
    }
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

    // ---- the character's inventory ------------------------------------------------------------
    // A character with no bags has never been played, and the reference gives it the default set before
    // it sends anything (`GameSrv_CreateDefaultBags`). A client with no inventory model dies when it draws
    // the character it was just given, so this happens before the load block and not lazily.
    let mut inventory = db_inventory::load(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?;
    if inventory.bags.is_empty() {
        let seeded = db_inventory::seed_default(&world.pool, transfer.character_id)
            .await
            .map_err(GameError::Database)?;
        if seeded {
            inventory = db_inventory::load(&world.pool, transfer.character_id)
                .await
                .map_err(GameError::Database)?;
            if trace {
                eprintln!(
                    "world: seeded the default inventory for {name:?} ({} bags, {} items)",
                    inventory.bags.len(),
                    inventory.items.len()
                );
            }
        }
    }

    // Where the character stands: where it left, if it left from this map.
    let saved = db_game::position(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?;
    let standing = Standing::resume(&map, saved);

    if trace {
        eprintln!(
            "world: {name:?} joined map {} (file id {}) on world {}, standing at ({:.1}, {:.1}) plane \
             {} with {} bag(s) and {} item(s)",
            map.map_id,
            map.map_file_id,
            world.world_id,
            standing.position.0,
            standing.position.1,
            standing.plane,
            inventory.bags.len(),
            inventory.items.len()
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
    // What this connection knows about itself: whose character it is, where it stands, and the agents it
    // has created.
    let mut play = Play {
        character: transfer.character_id,
        map,
        name,
        inventory,
        agents: Agents::new(
            AGENT_ID,
            standing.position,
            standing.plane,
            game::PLAYER_SPEED_BASE,
        ),
        started: Instant::now(),
    };

    // ---- 5. the load block ------------------------------------------------------------------
    // The reference sends these four the moment it has the connection, without waiting to be asked.
    let block = match &world.replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Load) => replay.load.clone(),
        _ => load_block(&play.map, &play.name),
    };
    send(&mut stream, &mut session, block, trace).await?;

    // ---- 6/7. the asks, then the session ----------------------------------------------------
    // Run to its end however it ends. A session that dies of a framing error is still a player that left,
    // and both things that happen on the way out — the position and the pool slot — belong to the leaving
    // and not to the ending: skipping them for the untidy exits would leak a slot until the reaper and
    // lose where the character stood.
    let outcome = run(&mut stream, &mut session, world, &mut play, trace).await;

    // Where the player was when it left is what it comes back to. Written *before* the lease goes back,
    // and best-effort: a failed save must not turn an ordinary logout into an error, and losing a
    // position is cheaper than refusing a player its way out.
    let left = play.agents.player.position;
    let left_plane = play.agents.player.plane;
    if let Err(err) = db_game::record_position(
        &world.pool,
        play.character,
        play.map.map_id,
        left.0,
        left.1,
        left_plane as i16,
    )
    .await
    {
        eprintln!("world: could not save the position of the leaving player: {err}");
    }

    // The player left. The slot goes back to the pool now rather than when a reaper notices: the
    // player is gone, and a pool that only empties on a reaper would refuse a legitimate travel after
    // an ordinary logout.
    if let Err(err) = placement::release(&world.pool, transfer.session_id).await {
        eprintln!("world: could not release the lease of the leaving player: {err}");
    }
    if trace {
        eprintln!(
            "world: {:?} left map {} from ({:.1}, {:.1}) plane {left_plane}, saved",
            play.name, play.map.map_id, left.0, left.1
        );
    }
    outcome
}

/// The conversation itself: answer what arrives until the client goes away, a message cannot be framed,
/// or the socket fails.
async fn run(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    trace: bool,
) -> Result<(), GameError> {
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
                            if !answer(stream, session, frame, world, play, trace).await? {
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
                send(stream, session, vec![game::world_simulation_tick(delta)], trace).await?;
            }

            _ = ping.tick() => {
                send(stream, session, vec![game::ping_request()], trace).await?;
            }
        }
    }
    Ok(())
}

/// What one gameplay connection knows about itself.
///
/// Assembled after the token pair is believed and before the load block, so that everything the blocks
/// need — whose character this is, where it stands, what it carries — is decided in one place rather than
/// read again in the middle of a conversation.
struct Play {
    /// The character the transfer named. Every write on the way out is scoped to it.
    character: Uuid,
    map: MapEntry,
    name: String,
    inventory: Inventory,
    agents: Agents,
    /// When the connection began, for the load time the reference reports.
    started: Instant,
}

impl Play {
    /// Where the agent is **now**, which is what the reference answers the spawn and players requests
    /// with (`agent->position`): after the client has walked, that is no longer where it entered.
    fn standing(&self) -> Standing {
        Standing {
            position: self.agents.player.position,
            plane: self.agents.player.plane,
        }
    }
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
/// A pure function of the map and the character's inventory so that `examples/dump-load.rs` can print
/// exactly what a client would receive, byte for byte, without a socket — which is how this stream is
/// compared against the reference's own log.
pub fn items_block(map: &MapEntry, inventory: &Inventory) -> Vec<Frame> {
    let mut frames = vec![
        game::item_stream_create(ITEM_STREAM, 0),
        game::item_set_active_weapon_set(ITEM_STREAM, 0),
    ];
    // The inventory itself, from the character's rows. It is the reference's own walk: for every bag, the
    // item that *is* the bag, the bag, and then its items with their placements.
    frames.extend(inventory_block(inventory));
    // Four empty weapon slots, exactly as `GameSrv_SendWeaponSlots` sends them.
    for slot in 0..4u8 {
        frames.push(game::item_weapon_set(ITEM_STREAM, slot, 0, 0));
    }
    frames.push(game::update_gold_storage(ITEM_STREAM, 0));
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

/// The item stream's id. The reference hardcodes `1` in every message that carries one
/// (`GameSrv_SendInventory`, `GameSrv_SendBagItems`), and a second stream is a thing it never opens.
pub const ITEM_STREAM: u16 = 1;

/// The inventory itself (`GameSrv_SendInventory`), built from rows instead of replayed bytes.
///
/// The order is the reference's and it matters twice over:
///
/// * **bags, by model id** — its `GmBagArray` is walked in declaration order, which is backpack, belt
///   pouch, bags, equipment pack, material storage, unclaimed items, storage, equipped items. That is
///   not the order the bag ids were handed out in (the capture's bags are 1, 2, 6, 3, 4, 5), so a server
///   that sorts by id sends a different stream;
/// * **a bag's own item before the bag** — `GameSrv_SendInventory` sends `bag_item_id` first, because the
///   bag message names it.
///
/// Within a bag, items go out by slot, each followed by where it sits and, if it was made for a
/// profession, what profession (`GameSrv_SendBagItems`). A missing row for a bag's own item is logged and
/// skipped rather than fatal, exactly as the reference's `GameSrv_SendItemById` logs and returns.
pub fn inventory_block(inventory: &Inventory) -> Vec<Frame> {
    let mut frames = Vec::with_capacity(inventory.bags.len() * 2 + inventory.items.len() * 3);
    for bag in &inventory.bags {
        if bag.assoc_item_id != 0 {
            match inventory.item(bag.assoc_item_id) {
                Some(item) => frames.push(game::create_named_item(&named_item(item))),
                None => eprintln!(
                    "world: bag {} names item {} as its own, and no such item exists",
                    bag.bag_id, bag.assoc_item_id
                ),
            }
        }

        frames.push(game::inventory_create_bag(
            ITEM_STREAM,
            wire_u8(bag.bag_type),
            wire_u8(bag.bag_model_id),
            wire_u16(bag.bag_id),
            wire_u8(bag.slot_count),
            wire_u32(bag.assoc_item_id),
        ));

        for item in inventory.items_in(bag.bag_id) {
            frames.push(game::create_named_item(&named_item(item)));
            if item.profession != 0 {
                frames.push(game::item_set_profession(
                    wire_u32(item.id),
                    wire_u8(item.profession),
                ));
            }
            frames.push(game::item_move_to_location(
                ITEM_STREAM,
                wire_u32(item.id),
                wire_u16(bag.bag_id),
                wire_u8(item.slot.unwrap_or(0)),
            ));
        }
    }
    frames
}

/// One row as the wire's field widths. Every value here comes from a row this schema wrote, so a
/// narrowing that cannot fail is the only kind that can occur; a value that somehow does is clamped
/// rather than panicking a live connection.
fn wire_u8(value: i32) -> u8 {
    u8::try_from(value).unwrap_or(0)
}

fn wire_u16(value: i32) -> u16 {
    u16::try_from(value).unwrap_or(0)
}

fn wire_u32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

/// A row as `CREATE_NAMED_ITEM` carries it.
fn named_item(item: &db_inventory::Item) -> game::NamedItem {
    game::NamedItem {
        id: wire_u32(item.id),
        file_id: u32::try_from(item.file_id).unwrap_or(0),
        item_type: wire_u8(item.item_type),
        dye_tint: wire_u8(item.dye_tint),
        dye_colors: u16::try_from(item.dye_colors).unwrap_or(0),
        materials: u16::try_from(item.materials).unwrap_or(0),
        unk1: wire_u8(item.unk1),
        flags: u32::try_from(item.flags).unwrap_or(0),
        value: wire_u32(item.value),
        model_id: wire_u32(item.model_id),
        quantity: wire_u32(item.quantity),
        name: item.name_units(),
        modifiers: item.modifier_dwords(),
    }
}

/// The answer to the client's spawn request: the map blob, and where to stand inside it.
pub fn spawn_block(map: &MapEntry, standing: Standing) -> Vec<Frame> {
    vec![game::instance_load_spawn_point(
        map.map_file_id,
        standing.position.0,
        standing.position.1,
        standing.plane,
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
async fn answer(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: Frame,
    world: &World,
    play: &mut Play,
    trace: bool,
) -> Result<bool, GameError> {
    let replay = world.replay.as_ref();
    match frame.header {
        // The client asks for the item stream. The reference answers with the inventory, the weapon
        // slots, the gold, the faction caps, the character info, the map, the spawn readiness and the
        // download manifest -- in that order (`GameSrv_HandleInstanceLoadRequestItems`).
        0x8091 => {
            let block = match replay {
                Some(replay) if replay.pick.wants(crate::replay::BlockName::Items) => {
                    replay.items.clone()
                }
                _ => items_block(&play.map, &play.inventory),
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
                _ => spawn_block(&play.map, play.standing()),
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
                _ => player_block(&play.name, play.standing(), &play.inventory, play.started),
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
                    play.agents.player.agent_id, destination.0, destination.1
                );
            }
            send(
                stream,
                session,
                vec![game::agent_move_to_point(
                    play.agents.player.agent_id,
                    destination,
                    plane,
                    play.agents.player.plane,
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
            let adopted = play
                .agents
                .player
                .adopt_report(position, plane, Instant::now());
            if trace {
                eprintln!(
                    "world: agent {} reported ({:.1}, {:.1}) plane {plane}: {adopted:?} -> ({:.1}, \
                     {:.1})",
                    play.agents.player.agent_id, position.0, position.1, play.agents.player.position.0, play.agents.player.position.1
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
                vec![game::agent_stop_moving(play.agents.player.agent_id)],
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
            let taken = play.agents.player.adopt_correction(position, plane);
            if trace {
                eprintln!(
                    "world: agent {} correction to ({:.1}, {:.1}) plane {plane}: {}",
                    play.agents.player.agent_id,
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
                            vec![game::agent_stop_moving(play.agents.player.agent_id)],
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

        // The client drops an item (`GameSrv_HandleDropItem`, `0x802C`: `{item_id: u32, count: u8}`).
        //
        // Copied from the reference rule for rule, including the two things it does *not* do, because a
        // port that quietly fixes them is a port that no longer agrees with the thing it is measured
        // against (`docs/07-status.md`):
        //
        // * **the item stays in its bag.** The reference finds it, checks it, and then creates an agent
        //   for it -- without removing it from the slot, without freeing the item id, and without sending
        //   `ITEM_REMOVE`. Dropping a cupcake therefore puts a cupcake on the ground and leaves the
        //   original in the backpack. Its handler is plainly unfinished: the partial-stack branch below is
        //   a `TODO` in its own source;
        // * **the item agent's model id is the item's id**, not the item's model (`item_agent->model_id =
        //   item->item_id`), which is not the model the dropped thing is drawn with. Copied for the same
        //   reason, and noted here so nobody spends an afternoon finding it again.
        //
        // What it does do, and this does too: item id 0, an item in no bag, an item that is not in the
        // bags it searches (backpack to bag 2), a count larger than the stack and a count smaller than it
        // are all refused -- logged and then ignored, with nothing sent to the client, which is what the
        // reference's `ERR_OK` returns amount to.
        0x802C => {
            let body = &frame.body;
            if body.len() < 5 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 7 }));
            }
            let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let count = i32::from(body[4]);
            drop_item(stream, session, world, play, item_id, count, trace).await?;
            Ok(true)
        }

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

/// `GameSrv_HandleDropItem`: put an item agent on the ground, and tell everyone in the instance.
///
/// See the match arm above for what the reference does *not* do. The agent's fields are the reference's
/// own choices, unaltered: the player's position offset by (30, 30), the same plane, facing x, an agent
/// left at zero for everything `GameSrv_CreateAgent`'s `memset` covers -- which is its speed and its team
/// token. The broadcast includes the dropping player, because `GameSrv_BroadcastMessage` walks every
/// connection without filtering the sender; with one connection in the world that is this one, which is
/// why this writes to the socket it was given instead of keeping a list of connections.
async fn drop_item(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    item_id: i32,
    count: i32,
    trace: bool,
) -> Result<(), GameError> {
    if item_id == 0 {
        eprintln!(
            "world: a drop names item 0, which is not an item; ignored as the reference ignores it"
        );
        return Ok(());
    }

    let Some(item) = db_inventory::find_in_carrying_bags(&world.pool, play.character, item_id)
        .await
        .map_err(GameError::Database)?
    else {
        eprintln!(
            "world: a drop names item {item_id}, which {0:?} does not carry; ignored",
            play.name
        );
        return Ok(());
    };

    if item.quantity < count {
        eprintln!(
            "world: a drop of {count} of item {item_id} is more than the {} it holds; ignored",
            item.quantity
        );
        return Ok(());
    }
    if count < item.quantity {
        // The reference's own `TODO`: a partial stack is not implemented, and its handler returns before
        // doing anything at all.
        eprintln!(
            "world: a partial drop ({count} of item {item_id} of {}) is not implemented, in the reference \
             or here; ignored",
            item.quantity
        );
        return Ok(());
    }

    let agent_id = play.agents.allocate();
    let player = &play.agents.player;
    let agent = game::CreateAgent {
        agent_id,
        // The reference's own line: `item_agent->model_id = item->item_id`. The item's *model* is the
        // field it should have used; see the note on the handler.
        model_id: wire_u32(item.id),
        agent_type: game::AGENT_TYPE_ITEM,
        position: (
            player.position.0 + DROP_OFFSET,
            player.position.1 + DROP_OFFSET,
        ),
        plane: player.plane,
        direction: (1.0, 0.0),
        speed_base: 0.0,
        player_team_token: 0,
    };
    if trace {
        eprintln!(
            "world: {} dropped item {item_id} at ({:.1}, {:.1}), agent {agent_id}",
            play.name, agent.position.0, agent.position.1
        );
    }
    send(
        stream,
        session,
        vec![game::world_create_agent(&agent)],
        trace,
    )
    .await
}

/// How far from the player the reference puts an item it drops: `Vec2fAdd(position, (30, 30))`.
const DROP_OFFSET: f32 = 30.0;

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
///
/// Two parameters are there for the two things this block no longer takes from a constant: `standing`,
/// which places the agent where the character actually is (`WORLD_CREATE_AGENT`), and `inventory`, which
/// says what the agent is wearing (`UPDATE_AGENT_VISUAL_EQUIPMENT`) — the nine ids read out of the
/// equipped-items bag, because they belong to *this* character's items.
pub fn player_block(
    name: &str,
    standing: Standing,
    inventory: &Inventory,
    started: Instant,
) -> Vec<Frame> {
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
    frames.push(game::world_create_agent(&game::CreateAgent {
        agent_id: AGENT_ID,
        model_id: game::CHAR_CLASS_PLAYER_BASE | PLAYER_ID,
        agent_type: game::AGENT_TYPE_LIVING,
        position: standing.position,
        plane: standing.plane,
        direction: (1.0, 0.0),
        speed_base: game::PLAYER_SPEED_BASE,
        player_team_token: PLAYER_TEAM_TOKEN,
    }));
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
        inventory.equipped_item_ids(),
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
        let frames = player_block(
            "Asura",
            Standing::from_map(&test_map()),
            &db_inventory::default_inventory(),
            Instant::now(),
        );
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
        let frames = player_block(
            "Asura",
            Standing::from_map(&test_map()),
            &db_inventory::default_inventory(),
            Instant::now(),
        );
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

    /// The inventory the reference's capture describes, as rows.
    fn reference_inventory() -> Inventory {
        db_inventory::default_inventory()
    }

    /// The whole point of building the item block from rows: from rows, it is the reference's stream.
    ///
    /// The capture (`asura_protocol::game::default_inventory::REFERENCE_ITEM_MESSAGES`) is GWPS answering
    /// the same request for its default character. Every message in it, in its order, byte for byte, is
    /// what this server now writes out of six bag rows and nine item rows — including the ids, which come
    /// out of the rows rather than out of a byte table.
    #[test]
    fn the_item_block_from_rows_is_the_capture_byte_for_byte() {
        let built: Vec<Vec<u8>> = inventory_block(&reference_inventory())
            .iter()
            .map(Frame::encode)
            .collect();

        let captured: Vec<Vec<u8>> = game::default_inventory::REFERENCE_ITEM_MESSAGES
            .iter()
            .map(|(header, body)| {
                let mut whole = header.to_le_bytes().to_vec();
                whole.extend_from_slice(body);
                whole
            })
            .collect();

        assert_eq!(
            built.len(),
            captured.len(),
            "the reference sent {} messages for its inventory, the rows produce {}",
            captured.len(),
            built.len()
        );
        for (index, (ours, theirs)) in built.iter().zip(captured.iter()).enumerate() {
            assert_eq!(
                ours,
                theirs,
                "message {index} of the item block ({:#06X}) is not what the reference sent",
                u16::from_le_bytes([theirs[0], theirs[1]])
            );
        }
    }

    #[test]
    fn the_item_block_opens_with_the_stream_and_carries_the_capture() {
        let frames = items_block(&test_map(), &reference_inventory());
        assert_eq!(frames[0].header, game::ITEM_STREAM_CREATE);
        assert_eq!(frames[1].header, game::ITEM_SET_ACTIVE_WEAPON_SET);
        assert_eq!(
            frames.last().expect("a block").header,
            game::INSTANCE_MANIFEST_DONE,
            "the answer ends with the download manifest, as the reference's does"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.header == game::CREATE_NAMED_ITEM),
            "the items are in it"
        );
        assert!(
            frames
                .iter()
                .any(|frame| frame.header == game::ITEM_MOVE_TO_LOCATION),
            "and where they sit"
        );
    }

    #[test]
    fn the_bags_go_out_in_model_order_not_id_order() {
        // The trap the rows are read in the wrong order by: the capture's bags are ids 1, 2, 6, 3, 4, 5,
        // because the reference walks its bag *array*. Sorting by id sends a different stream, and a
        // client that reads the equipped-items bag before the backpack has nowhere to put the armour.
        let frames = inventory_block(&reference_inventory());
        let bags: Vec<u16> = frames
            .iter()
            .filter(|frame| frame.header == game::INVENTORY_CREATE_BAG)
            .map(|frame| u16::from_le_bytes([frame.body[4], frame.body[5]]))
            .collect();
        assert_eq!(bags, vec![1, 2, 6, 3, 4, 5]);
    }

    #[test]
    fn the_equipment_on_the_model_is_what_the_equipped_bag_holds() {
        let frames = player_block(
            "Asura",
            Standing::from_map(&test_map()),
            &reference_inventory(),
            Instant::now(),
        );
        let worn = frames
            .iter()
            .find(|frame| frame.header == game::UPDATE_AGENT_VISUAL_EQUIPMENT)
            .expect("the visual equipment");
        let ids: Vec<u32> = (0..9)
            .map(|slot| {
                let at = 4 + slot * 4;
                u32::from_le_bytes([
                    worn.body[at],
                    worn.body[at + 1],
                    worn.body[at + 2],
                    worn.body[at + 3],
                ])
            })
            .collect();
        assert_eq!(
            ids,
            game::default_inventory::REFERENCE_EQUIPPED_ITEM_IDS.to_vec(),
            "the nine ids the capture carries, read out of the equipped-items bag"
        );
    }

    #[test]
    fn an_agent_with_no_inventory_still_gets_a_well_formed_block() {
        // A character whose bags have not been written yet: the client must not be told about items that
        // do not exist, and the block must still end where the loading screen ends.
        let frames = items_block(&test_map(), &Inventory::default());
        assert_eq!(frames[0].header, game::ITEM_STREAM_CREATE);
        assert!(!frames
            .iter()
            .any(|frame| frame.header == game::CREATE_NAMED_ITEM));
        assert_eq!(
            frames.last().expect("a block").header,
            game::INSTANCE_MANIFEST_DONE
        );
    }

    #[test]
    fn a_character_stands_where_it_left_and_nowhere_else() {
        let map = test_map();
        // Never saved: the map's own spawn.
        assert_eq!(Standing::resume(&map, None), Standing::from_map(&map));

        // Saved here: that position, on that plane.
        let saved = db_game::Position {
            map_id: map.map_id,
            x: 6700.0,
            y: 4400.0,
            plane: 3,
        };
        assert_eq!(
            Standing::resume(&map, Some(saved)),
            Standing {
                position: (6700.0, 4400.0),
                plane: 3
            }
        );

        // Saved in another map: this one's spawn, because that coordinate is not in this map.
        let elsewhere = db_game::Position {
            map_id: map.map_id + 1,
            x: 6700.0,
            y: 4400.0,
            plane: 3,
        };
        assert_eq!(
            Standing::resume(&map, Some(elsewhere)),
            Standing::from_map(&map),
            "a position is only usable on the map it was taken in"
        );
    }

    #[test]
    fn the_spawn_point_is_the_standing_position() {
        let map = test_map();
        let standing = Standing {
            position: (6700.0, 4400.0),
            plane: 2,
        };
        let frames = spawn_block(&map, standing);
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].header,
            game::INSTANCE_LOAD_SPAWN_POINT,
            "the client is told the same coordinate the load info and the agent were built from"
        );
        assert_eq!(
            f32::from_le_bytes([
                frames[0].body[4],
                frames[0].body[5],
                frames[0].body[6],
                frames[0].body[7]
            ]),
            6700.0
        );
    }

    #[test]
    fn a_dropped_item_becomes_an_item_agent_a_hundred_and_eighty_units_away() {
        // `GameSrv_HandleDropItem`: an agent of type 4 at the player's position plus (30, 30), facing x,
        // with the item's *id* as its model (the reference's own line, which is noted in the handler).
        let player =
            crate::agent::AgentState::new(AGENT_ID, (6602.0, 4485.0), 0, game::PLAYER_SPEED_BASE);
        let agent = game::CreateAgent {
            agent_id: 2,
            model_id: 2,
            agent_type: game::AGENT_TYPE_ITEM,
            position: (
                player.position.0 + DROP_OFFSET,
                player.position.1 + DROP_OFFSET,
            ),
            plane: player.plane,
            direction: (1.0, 0.0),
            speed_base: 0.0,
            player_team_token: 0,
        };
        let bytes = game::world_create_agent(&agent).encode();
        assert_eq!(bytes.len(), 99, "the reference's fixed agent message");
        // header(2) agent_id(4) model_id(4) agent_type(1) h000B(1) position(8) plane(2) ...
        assert_eq!(
            u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            2,
            "the item id is the model id, as the reference writes it"
        );
        assert_eq!(bytes[10], game::AGENT_TYPE_ITEM, "AgentType_Item");
        assert_eq!(
            f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            6632.0,
            "the player's x plus 30"
        );
        assert_eq!(
            f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            4515.0,
            "and the player's y plus 30"
        );
    }

    #[test]
    fn agent_ids_do_not_collide() {
        // A player's agent and a dropped item's are told apart: two agents sharing an id is one agent the
        // client draws and one it does not.
        let mut agents = Agents::new(AGENT_ID, (0.0, 0.0), 0, game::PLAYER_SPEED_BASE);
        let first = agents.allocate();
        let second = agents.allocate();
        assert_eq!(first, AGENT_ID + 1, "the player's id is taken");
        assert_ne!(first, second);
        assert_ne!(first, agents.player.agent_id);
    }
}
