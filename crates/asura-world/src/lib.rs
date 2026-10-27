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
//! * one agent, the character the transfer named, standing where the map's spawn says â€” or where it
//!   stood when it last left, which is saved and read back (`asura_db::game::record_position`);
//! * the character's inventory, read from rows and built into the reference's messages, including the
//!   nine items `UPDATE_AGENT_VISUAL_EQUIPMENT` puts on the model (`asura_db::inventory`);
//! * `GAME_CMSG_DROP_ITEM`, which puts an item agent on the ground â€” and, as in the reference, does not
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

use crate::agent::{Adopted, Agents};
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
/// ASURA has one character in a map, so the id table is a constant Ã¢â‚¬â€ and the party message carries
/// these as `u16`, which is the only real constraint. It becomes a real allocator when there is more
/// than one player to allocate for.
pub const PLAYER_ID: u32 = 1;
/// See [`PLAYER_ID`].
pub const AGENT_ID: u32 = 1;

/// The party a lone player is put in.
pub const PARTY_ID: u16 = 1;

/// `MapType_CharacterCreation` (`GmMap.h`): the instance a new character is made in. The reference hosts it
/// as a map of its own, which is why `main.rs` accepts it alongside a main town.
pub const MAP_TYPE_CHARACTER_CREATION: i32 = 11;

/// `GM_ERROR_CHARACTER_NAME_ALREADY_EXIST` (`GmError.h`): the code the reference answers a taken name with
/// (`GameSrv.c:1939`).
pub const GM_ERROR_CHARACTER_NAME_ALREADY_EXIST: u32 = 0x0106;

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
    /// What the map is called, for `/maps` and the log. Empty is allowed: a map without a name prints
    /// its id, which is a worse answer than the reference's and not a broken one.
    pub name: String,
    /// How many players this world will place in it. `/maps` reports it because a player asking "where
    /// can I go" wants to know what is running, not only what exists.
    pub capacity: i32,
}

impl MapEntry {
    /// How the map is named in an answer or a log line: its name when it has one, its id when it does
    /// not, and never an empty string.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("map {}", self.map_id)
        } else {
            format!("{} ({})", self.name, self.map_id)
        }
    }

    /// The alias `/tp` would take for this map: the reference's aliases are lower-case names with single
    /// spaces ("lakeside county"), so this is the name lower-cased. An unnamed map has no alias to type and
    /// says so, rather than inventing one.
    pub fn alias(&self) -> String {
        let alias = self.name.to_lowercase();
        let collapsed = alias.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            format!("map {}", self.map_id)
        } else {
            collapsed
        }
    }

    /// `0` and `1` are the reference's own `MapType` values; every other value it can carry is a kind of
    /// instance this world does not host.
    pub fn kind(&self) -> &'static str {
        match self.map_type {
            0 => "outpost",
            1 => "explorable",
            _ => "other",
        }
    }
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
    /// Everyone standing in this world right now. The reference keeps the same list
    /// (`srv->agents`, `srv->players`) and broadcasts through it; a server that has nobody to tell has
    /// no use for it, which is why this is new rather than recently-added: without a second player the
    /// list had nothing in it but the one client being served.
    pub peers: Peers,
    /// The address to advertise to a client that is being redirected here (`--public`). A redirect
    /// carries a `sockaddr_in`, and a world that hands out its *bind* address hands out the wrong one
    /// whenever the two differ — which is the normal case behind a realmlist.
    pub public: std::net::SocketAddr,
}

/// One player in the world, plus the channel that reaches its connection.
///
/// The two connections are separate sockets owned by separate tasks, so a "broadcast" is a push into
/// each peer's outbox. `Mutex` is the std one: the critical sections are a `Vec` scan, and await points
/// inside them would be the only reason to want an async lock.
pub struct Peer {
    /// What `GAME_SMSG_PLAYER_CREATE` calls the player, and what the client uses to refer to it.
    pub player_id: u32,
    /// What `GAME_SMSG_WORLD_CREATE_AGENT` calls the agent.
    pub agent_id: u32,
    pub name: String,
    /// Where this peer stands, as the server last believed it. Kept here so a joining player can be
    /// told about everyone already in the map without reaching into another connection's state.
    pub standing: Standing,
    /// The nine equipped item ids, for the same reason as `standing`.
    pub equipped: [u32; 9],
    outbox: tokio::sync::mpsc::UnboundedSender<Vec<Frame>>,
}

/// Everyone in the world, keyed by nothing in particular: the list is at most a handful of players.
#[derive(Default, Clone)]
pub struct Peers(std::sync::Arc<std::sync::Mutex<Vec<Peer>>>);

impl Peers {
    /// Take the lowest free ids and put the peer in the list.
    ///
    /// The reference allocates both from id tables (`GmIdAllocate`); with a list this short, the lowest
    /// free id is the same thing, and it keeps the single-player case at id 1 exactly as it was.
    pub fn join(&self, mut peer: Peer) -> (u32, u32) {
        let mut peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        peer.player_id = (1..)
            .find(|id| !peers.iter().any(|other| other.player_id == *id))
            .unwrap_or(1);
        peer.agent_id = (1..)
            .find(|id| !peers.iter().any(|other| other.agent_id == *id))
            .unwrap_or(1);
        let ids = (peer.player_id, peer.agent_id);
        peers.push(peer);
        ids
    }

    /// Remove a peer, and say whether it was there.
    pub fn leave(&self, player_id: u32) -> bool {
        let mut peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        let before = peers.len();
        peers.retain(|peer| peer.player_id != player_id);
        before != peers.len()
    }

    /// Everyone except one player, as plain data (no outbox, so the caller cannot hold the lock).
    pub fn others(&self, player_id: u32) -> Vec<PeerView> {
        let peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        peers
            .iter()
            .filter(|peer| peer.player_id != player_id)
            .map(PeerView::from)
            .collect()
    }

    /// Push frames to everyone except `except`. A peer whose channel is closed is gone; its own task is
    /// responsible for leaving the list, so a failed send is not an error here.
    pub fn broadcast(&self, except: u32, frames: &[Frame]) {
        let peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        for peer in peers.iter().filter(|peer| peer.player_id != except) {
            let _ = peer.outbox.send(frames.to_vec());
        }
    }

    /// Push frames to everyone in the world.
    pub fn broadcast_all(&self, frames: &[Frame]) {
        let peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        for peer in peers.iter() {
            let _ = peer.outbox.send(frames.to_vec());
        }
    }

    /// What the server believes about one peer, for the position broadcasts.
    pub fn update(&self, player_id: u32, standing: Standing) {
        let mut peers = self.0.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(peer) = peers.iter_mut().find(|peer| peer.player_id == player_id) {
            peer.standing = standing;
        }
    }
}

/// A peer as it is copied out of the list: everything a joining player needs to be told.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerView {
    pub player_id: u32,
    pub agent_id: u32,
    pub name: String,
    pub standing: Standing,
    pub equipped: [u32; 9],
}

impl From<&Peer> for PeerView {
    fn from(peer: &Peer) -> Self {
        PeerView {
            player_id: peer.player_id,
            agent_id: peer.agent_id,
            name: peer.name.clone(),
            standing: peer.standing,
            equipped: peer.equipped,
        }
    }
}

/// The frames that introduce one player to another: `GameSrv_SendWorldAgents`'s order for a player
/// agent (`GmAgent.c:310-331`) â€” the player, their party size, their party, then the agent itself.
///
/// `GmAgent.c` also sends `BroadcastAddPlayerToPlayerParty` rather than the unicast sender, because the
/// party of a joining player is news to everyone already there. Here it is the same frame either way.
pub fn introduction(peer: &PeerView) -> Vec<Frame> {
    vec![
        game::player_create(
            peer.player_id,
            peer.agent_id,
            game::Appearance::REFERENCE_DEFAULT.pack(),
            &peer.name,
        ),
        game::player_update_party_size(peer.player_id, 1),
        game::player_update_party(peer.player_id, peer.player_id),
        game::world_create_agent(&game::CreateAgent {
            agent_id: peer.agent_id,
            model_id: game::CHAR_CLASS_PLAYER_BASE | peer.player_id,
            agent_type: game::AGENT_TYPE_LIVING,
            position: peer.standing.position,
            plane: peer.standing.plane,
            direction: (1.0, 0.0),
            speed_base: game::PLAYER_SPEED_BASE,
            player_team_token: PLAYER_TEAM_TOKEN,
        }),
    ]
}

/// The frames that take one player back out of another's world: `GameSrv_BroadcastPlayerDestroy`
/// (`GmPlayer.c:314`) and `GameSrv_BroadcastAgentRemove` (`GmAgent.c:60`), in that order.
pub fn farewell(player_id: u32, agent_id: u32) -> Vec<Frame> {
    vec![
        game::player_destroy(player_id),
        game::world_remove_agent(agent_id),
    ]
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

/// The reference's own load stream, as `(header, size)` pairs, generated by `tools/parity-load.py` from
/// its `GWPS_TRACE=1` log. `the_load_stream_is_the_references_message_for_message` compares this server's
/// blocks against it, which is what makes "the load block matches the reference" a measurement rather
/// than a claim.
pub mod load_fixture;

/// What a new character wears, per campaign and profession, generated by `tools/gen-default-equipment.py`
/// from the reference's `GmDefaultArmors.c`. `GameSrv_HandleCharCreationChangeProf` answers a profession
/// change out of that same table.
pub mod default_equipment;

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
        let connection =
            tokio::spawn(async move { handle_connection(stream, &world, trace).await });
        // The connection is reaped rather than detached. A task that dies of a *panic* leaves this
        // process alive, so from outside "the server crashed" and "one connection panicked" look the
        // same â€” and the panic's own message is the only thing that says which it was.
        tokio::spawn(async move {
            match connection.await {
                Ok(Ok(())) => {
                    if trace {
                        eprintln!("world: {peer} left");
                    }
                }
                Ok(Err(err)) => eprintln!("world: {peer}: {err}"),
                Err(err) => {
                    if err.is_panic() {
                        eprintln!(
                            "world: {peer}: connection task PANICKED: {}",
                            panic_message(err)
                        );
                    } else {
                        eprintln!("world: {peer}: connection task ended: {err}");
                    }
                }
            }
        });
    }
}

/// The message a panicking task carried, if it carried one a human can read.
fn panic_message(err: tokio::task::JoinError) -> String {
    let payload = err.into_panic();
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_string())
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
            eprintln!(
                "world: seeded the default inventory for {name:?} ({} bags, {} items)",
                inventory.bags.len(),
                inventory.items.len()
            );
        }
    }

    // Where the character stands: where it left, if it left from this map.
    let saved = db_game::position(&world.pool, transfer.character_id)
        .await
        .map_err(GameError::Database)?;
    let standing = Standing::resume(&map, saved);

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
    // has created. The ids are allocated here rather than being the constants they used to be: two
    // players in one map need two of each, and everything the reference sends a player about itself
    // carries them.
    let equipped = inventory.equipped_item_ids();
    let (outbox, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let (player_id, agent_id) = world.peers.join(Peer {
        player_id: 0,
        agent_id: 0,
        name: name.clone(),
        standing,
        equipped,
        outbox,
    });
    let map_type = map.map_type;
    let mut play = Play {
        character: transfer.character_id,
        player_id,
        map,
        name,
        inventory,
        agents: Agents::new(
            agent_id,
            standing.position,
            standing.plane,
            game::PLAYER_SPEED_BASE,
        ),
        session_id: transfer.session_id,
        account_id: transfer.account_id,
        moved: false,
        // A creation instance is its own map type (`MapType_CharacterCreation`, 11): a connection that
        // arrived in one is there to make a character, and one that arrived in anything else cannot.
        creation: if map_type == MAP_TYPE_CHARACTER_CREATION {
            Some(Creation::start())
        } else {
            None
        },
        started: Instant::now(),
    };

    // ---- 5. the load block ------------------------------------------------------------------
    // The reference sends these four the moment it has the connection, without waiting to be asked.
    let block = match &world.replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Load) => replay.load.clone(),
        _ => load_block(&play.map, &play.name, play.agents.player.agent_id),
    };
    send(&mut stream, &mut session, block, trace).await?;

    // ---- 6/7. the asks, then the session ----------------------------------------------------
    // Run to its end however it ends. A session that dies of a framing error is still a player that left,
    // and both things that happen on the way out â€” the position and the pool slot â€” belong to the leaving
    // and not to the ending: skipping them for the untidy exits would leak a slot until the reaper and
    // lose where the character stood.
    let outcome = run(
        &mut stream,
        &mut session,
        world,
        &mut play,
        &mut inbox,
        trace,
    )
    .await;

    // The player left: everyone else's world loses the agent before the slot goes back, so a client that
    // is being told about the departure never sees an agent whose seat is already free.
    if world.peers.leave(play.player_id) {
        world
            .peers
            .broadcast_all(&farewell(play.player_id, play.agents.player.agent_id));
    }

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
    //
    // **Unless it was moved.** `/tp` reserves the *destination*'s seat for this session in the same
    // transaction that gives up this one, so the only lease the session holds by the time this runs is
    // the one the client is about to arrive on. Releasing it would free a seat a player is standing in —
    // the reference's own mirror of this race is recorded in `PreSearing/docs/08-gm-commands.md`.
    if play.moved {
        eprintln!(
            "world: {:?} was moved to map {}; the seat it holds is the destination's",
            play.name, play.map.map_id
        );
    } else if let Err(err) = placement::release(&world.pool, transfer.session_id).await {
        eprintln!("world: could not release the lease of the leaving player: {err}");
    }
    eprintln!(
        "world: {:?} left map {} from ({:.1}, {:.1}) plane {left_plane}, saved",
        play.name, play.map.map_id, left.0, left.1
    );
    outcome
}

/// The conversation itself: answer what arrives until the client goes away, a message cannot be framed,
/// or the socket fails.
async fn run(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<Frame>>,
    trace: bool,
) -> Result<(), GameError> {
    let mut tick = tokio::time::interval(TICK_INTERVAL);
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut buffer = [0u8; 8192];

    'session: loop {
        tokio::select! {
            biased;

            // Frames another player's connection pushed at us. This is the whole of the broadcast
            // mechanism: the reference writes into every other connection's buffer, and here every
            // other connection's task writes into this channel.
            Some(frames) = inbox.recv() => {
                send(stream, session, frames, trace).await?;
            }

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
/// need â€” whose character this is, where it stands, what it carries â€” is decided in one place rather than
/// read again in the middle of a conversation.
struct Play {
    /// The character the transfer named. Every write on the way out is scoped to it.
    character: Uuid,
    /// This connection's player id, allocated when it joined. The reference allocates the same number
    /// from `srv->players` (`GmIdAllocate`) and puts it in `GAME_SMSG_PLAYER_CREATE`, every agent's
    /// `model_id`, the party messages and the chat speaker field.
    player_id: u32,
    map: MapEntry,
    name: String,
    inventory: Inventory,
    agents: Agents,
    /// The login session and account the transfer named.
    ///
    /// The reference keeps both on its player (`GmPlayer.account_id`, `.char_id`) and uses them for every
    /// bag and item query and for its transfer request (`GameSrv.c:1450-1472`). This server scoped its
    /// writes by character alone, which is enough for everything it did until a *world* had to start a
    /// travel: the transfer row is keyed by session and needs the account as well (`docs/09` §9).
    session_id: Uuid,
    account_id: Uuid,
    /// Set when this connection has been redirected to another map. Its teardown must then leave the
    /// lease alone: the seat it holds now belongs to the connection the client is about to open.
    moved: bool,
    /// What a character-creation connection has chosen so far: `Some` when this connection is in a
    /// creation instance (map type 11), which is the reference's `MapType_CharacterCreation`.
    creation: Option<Creation>,
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
/// exactly what a client would receive, byte for byte, without a socket â€” which is how this stream is
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
/// * **bags, by model id** â€” its `GmBagArray` is walked in declaration order, which is backpack, belt
///   pouch, bags, equipment pack, material storage, unclaimed items, storage, equipped items. That is
///   not the order the bag ids were handed out in (the capture's bags are 1, 2, 6, 3, 4, 5), so a server
///   that sorts by id sends a different stream;
/// * **a bag's own item before the bag** â€” `GameSrv_SendInventory` sends `bag_item_id` first, because the
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
pub fn load_block(map: &MapEntry, name: &str, agent_id: u32) -> Vec<Frame> {
    vec![
        game::instance_load_head_default(),
        game::instance_player_data_start(),
        game::instance_load_player_name(name),
        game::instance_load_info(
            agent_id,
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
        //
        // Everyone already in the map goes after it, in the reference's own order for a joining client
        // (`GameSrv_SendWorldAgents`, `GmAgent.c:310-331`), and the same frames go out to *them* so they
        // learn about this one. The reference broadcasts the player create there and unicasts the rest;
        // the order is what matters here, and it is the same in both directions.
        0x8090 => {
            let mut block = match replay {
                Some(replay) if replay.pick.wants(crate::replay::BlockName::Players) => {
                    replay.players.clone()
                }
                _ => player_block(
                    &play.name,
                    play.standing(),
                    &play.inventory,
                    play.started,
                    play.player_id,
                    play.agents.player.agent_id,
                ),
            };
            let others = world.peers.others(play.player_id);
            for peer in &others {
                block.extend(introduction(peer));
            }
            let me = PeerView {
                player_id: play.player_id,
                agent_id: play.agents.player.agent_id,
                name: play.name.clone(),
                standing: play.standing(),
                equipped: play.inventory.equipped_item_ids(),
            };
            if !others.is_empty() {
                world.peers.broadcast(play.player_id, &introduction(&me));
            }
            send(stream, session, block, trace).await?;
            Ok(true)
        }

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
            let walk = game::agent_move_to_point(
                play.agents.player.agent_id,
                destination,
                plane,
                play.agents.player.plane,
            );
            // Anyone else in the map is told where this agent is walking: the reference broadcasts the
            // same message (`GameSrv_BroadcastAgentMoveToPoint`), and a client that is not told keeps
            // drawing the walker where it last stood.
            world
                .peers
                .broadcast(play.player_id, std::slice::from_ref(&walk));
            send(stream, session, vec![walk], trace).await?;
            Ok(true)
        }

        // The client reports where it is (`GameSrv_HandlePositionUpdate`). The position is adopted when
        // the budget allows it, clamped when it does not, and refused across a plane change that is too
        // far. Nothing goes back to the reporter â€” it already knows where it is â€” but everyone else is
        // told, which is what the reference's `GameSrv_BroadcastAgentPosition` does and why a second
        // client sees the first one move.
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
            let standing = play.standing();
            world.peers.update(play.player_id, standing);
            if adopted != Adopted::Refused {
                world.peers.broadcast(
                    play.player_id,
                    &[game::agent_update_position(
                        play.agents.player.agent_id,
                        standing.position,
                        standing.plane,
                    )],
                );
            }
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
            let stop = game::agent_stop_moving(play.agents.player.agent_id);
            world
                .peers
                .broadcast(play.player_id, std::slice::from_ref(&stop));
            send(stream, session, vec![stop], trace).await?;
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
        // `!` is local/general, `#` group, `$` trade, `/` a command, and the reference answers anything
        // else with bad user data.
        //
        // **A player's message is sent ungated**, exactly as the reference sends it: its
        // `GameSrv_ChatSendingEnabled` gate covers the text the *server* generates (GM answers, "unknown
        // command"), not a line a player typed. This port once had a switch on everything, which is why
        // a `!` line went nowhere; the switch is gone (`docs/03` Â§11), so nothing here is gated and only
        // the messages the reference never sends are withheld.
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
                // `Channel_All`, `Channel_Group`, `Channel_Trade`: the three channels the reference
                // speaks on. Sent ungated, as the reference sends them. None of the three chat messages
                // appears in any capture, and no chat text has ever rendered on screen in either server
                // -- so what this path matches is the reference's *bytes*, not its visible result.
                speaker @ ('!' | '#' | '$') => {
                    let channel = match speaker {
                        '#' => game::CHANNEL_GROUP,
                        '$' => game::CHANNEL_TRADE,
                        _ => game::CHANNEL_ALL,
                    };
                    if trace {
                        eprintln!("world: {speaker} {spoken:?} on channel {channel}");
                    }
                    send(
                        stream,
                        session,
                        vec![
                            game::chat_message_core(&spoken),
                            game::chat_message_local(play.player_id, channel),
                        ],
                        trace,
                    )
                    .await?;
                    Ok(true)
                }
                '/' => {
                    // `/stuck` is the command that needs no other machinery. `/maps` is the one the
                    // reference's GM file has that this world can answer honestly, because it knows
                    // exactly which maps it hosts. `/tp` is not here: the reference teleports by having
                    // the auth service place the player on another map server and redirecting the
                    // client, and **a world cannot start that cycle today** -- the redirect itself is
                    // known (46 bytes at `game::TRANSFER_GAME_SERVER_INFO`, then `MAP_UPDATE_CURRENT`,
                    // `docs/03-protocol.md` Â§5), so what is missing is the allocation, not the bytes.
                    let command = spoken.trim();
                    if command == "stuck" {
                        // `GameSrv_HandleChatCommand`'s `stuck`: cancel the movement and broadcast where
                        // the agent is now. **Not** `AGENT_STOP_MOVING` -- that is what
                        // `HandleCancelMovement` (`0x8028`) answers, and the reference sends a position
                        // update here instead. It also broadcasts a world tick first; the 500 ms ticker
                        // covers that, so no second clock is invented.
                        let player = &play.agents.player;
                        if trace {
                            eprintln!(
                                "world: /stuck -- agent {} stands at ({:.1}, {:.1}) plane {}",
                                player.agent_id, player.position.0, player.position.1, player.plane
                            );
                        }
                        send(
                            stream,
                            session,
                            vec![game::agent_update_position(
                                player.agent_id,
                                player.position,
                                player.plane,
                            )],
                            trace,
                        )
                        .await?;
                        Ok(true)
                    } else if command == "maps" {
                        // `GmCommands_ListTargets`, as it behaves with the reference's own defaults: the
                        // table goes to the log, and nothing goes on the wire. That is parity, not a
                        // gate -- GWPS's chat gate is what leaves its chat half in its log too, and no
                        // chat text has ever reached a screen in either server. Sending it here crashed a
                        // live client, which is why the answer is printed and not sent.
                        let answer = maps_answer(&world.maps);
                        for line in &answer.table {
                            eprintln!("world: {line}");
                        }
                        if trace {
                            for line in &answer.chat {
                                eprintln!("world: /maps (chat, not sent): {line}");
                            }
                        }
                        Ok(true)
                    } else if let Some(argument) = command
                        .strip_prefix("tp ")
                        .or_else(|| command.strip_prefix("teleport "))
                    {
                        // `GameSrv_HandleTeleportCommand`: a *server transfer*, which is what the game
                        // itself does when a portal is walked through. Every byte of the redirect is
                        // measured (`docs/03-protocol.md` §5) and the reference's refusals are here
                        // verbatim, so this is a port rather than an invention.
                        teleport(stream, session, world, play, argument.trim(), trace).await
                    } else if command == "tp" || command == "teleport" {
                        // `GmCommand_ToAscii` fails on an empty argument, and the reference's answer is
                        // its "unknown command" line.
                        eprintln!("world: Unknown command");
                        Ok(true)
                    } else {
                        // `GameSrv_SendInvalidCommand`: the reference's text is "Unknown command", and with
                        // its chat gate off that is a log line and nothing else. Same here.
                        eprintln!("world: Unknown command");
                        Ok(true)
                    }
                }
                other => {
                    // `@`, `%` and `"` are ignored by the reference; anything else it refuses.
                    if matches!(other, '@' | '%' | '"') {
                        if trace {
                            eprintln!(
                                "world: channel {other:?} ignored, as the reference ignores it"
                            );
                        }
                        return Ok(true);
                    }
                    eprintln!(
                        "world: chat on channel {other:?} refused, as the reference refuses it"
                    );
                    Ok(true)
                }
            }
        }

        // The client is going away.
        0x8008 => Ok(false),

        // ---- character creation (a connection in a `MapType_CharacterCreation` instance) -------------
        //
        // The four messages `GameSrv_ProcessPlayerMessage` dispatches to `GameSrv_HandleCharCreation*`.
        // The block each answers with is the reference's own order; the equipment is memory, not rows,
        // until the confirm writes it (`GameSrv_CreatePlayerBags` runs there and nowhere else).
        0x8089 => {
            let Some(creation) = play.creation else {
                return Ok(true);
            };
            let agent_id = play.agents.player.agent_id;
            let inventory = creation_inventory(creation);
            let mut frames = vec![game::instance_player_data_start()];
            frames.push(game::item_stream_create(ITEM_STREAM, 0));
            frames.extend(inventory_block(&inventory));
            frames.push(game::item_set_active_weapon_set(ITEM_STREAM, 0));
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
                frames.push(game::faction_max(faction, 10_000));
            }
            frames.push(game::agent_update_attribute_points(agent_id, 50, 50));
            frames.push(game::player_update_profession(
                agent_id,
                creation.profession,
                0,
                0,
            ));
            frames.push(game::player_update_unlocked_professions(
                agent_id,
                DEFAULT_UNLOCKED_PROFESSIONS,
            ));
            frames.push(game::skillbar_update(agent_id, &[0; 8], &[0; 8], 1));
            frames.push(game::update_unlocked_skills(&[]));
            frames.push(game::character_update_factions(
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ));
            frames.push(game::agent_update_attributes(agent_id, &[0; 42]));
            frames.push(game::agent_property_update_int(
                game::AGENT_PROPERTY_ENERGY,
                agent_id,
                game::PLAYER_ENERGY_MAX,
            ));
            frames.push(game::agent_property_update_int(
                game::AGENT_PROPERTY_HEALTH,
                agent_id,
                game::PLAYER_HEALTH_MAX,
            ));
            frames.push(game::agent_property_update_float(
                game::AGENT_PROPERTY_ENERGY_REGEN,
                agent_id,
                game::PLAYER_ENERGY_REGEN,
            ));
            frames.push(game::instance_player_data_done());
            send(stream, session, frames, trace).await?;
            Ok(true)
        }

        // `GameSrv_HandleCharCreationRequestArmors`: the PvP half. This account has no unlocked skills,
        // no heroes and no PvP items, which is what an empty stream says.
        0x808A => {
            let frames = vec![
                game::pvp_update_unlocked_skills(&[]),
                game::pvp_update_unlocked_heroes(&[]),
                game::pvp_item_stream_end(),
            ];
            send(stream, session, frames, trace).await?;
            Ok(true)
        }

        // `GameSrv_HandleCharCreationChangeProf` (`0x8060`: campaign, profession): swap the equipped set
        // and tell the client, which is the whole of what the reference does here.
        0x8060 => {
            let Some(mut creation) = play.creation else {
                return Ok(true);
            };
            if frame.body.len() < 2 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 4 }));
            }
            creation.campaign = frame.body[0];
            creation.profession = frame.body[1];
            play.creation = Some(creation);

            let agent_id = play.agents.player.agent_id;
            let inventory = creation_inventory(creation);
            let mut frames = vec![
                game::update_unlocked_skills(&[]),
                game::player_update_profession(agent_id, creation.profession, 0, 0),
            ];
            // `GameSrv_SendBagItems` for the equipped bag: each item, then where it sits.
            let equipped_bag = inventory
                .bags
                .iter()
                .find(|bag| bag.bag_type == 2)
                .map(|bag| bag.bag_id);
            for item in inventory
                .items
                .iter()
                .filter(|item| item.bag_id == equipped_bag)
            {
                frames.push(game::create_named_item(&named_item(item)));
                if let (Some(bag), Some(slot)) = (item.bag_id, item.slot) {
                    frames.push(game::item_move_to_location(
                        ITEM_STREAM,
                        item.id as u32,
                        bag as u16,
                        slot as u8,
                    ));
                }
            }
            send(stream, session, frames, trace).await?;
            Ok(true)
        }

        // `GameSrv_HandleCharCreationConfirm` (`0x808B`: name, appearance, flags): write the rows and say
        // so. The reference answers `CHAR_CREATION_ERROR` when the name is taken; this server has the same
        // unique-per-account rule to enforce, and answers the same way.
        0x808B => {
            let Some(creation) = play.creation else {
                return Ok(true);
            };
            let body = &frame.body;
            if body.len() < 2 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 4 }));
            }
            let units = u16::from_le_bytes([body[0], body[1]]) as usize;
            let name = String::from_utf16_lossy(
                &(0..units.min(20))
                    .map(|index| {
                        let at = 2 + index * 2;
                        u16::from_le_bytes([
                            body.get(at).copied().unwrap_or(0),
                            body.get(at + 1).copied().unwrap_or(0),
                        ])
                    })
                    .collect::<Vec<u16>>(),
            );
            if name.trim().is_empty() {
                send(
                    stream,
                    session,
                    vec![game::char_creation_error(
                        GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
                    )],
                    trace,
                )
                .await?;
                return Ok(true);
            }

            let equipped = creation_equipment_rows(creation);
            match db_inventory::create_character(
                &world.pool,
                play.account_id,
                &name,
                play.map.map_id,
                &equipped,
            )
            .await
            {
                Ok(character) => {
                    eprintln!(
                        "world: created character {name:?} ({} items, campaign {} profession {}) in map {}",
                        equipped.len(),
                        creation.campaign,
                        creation.profession,
                        play.map.map_id
                    );
                    send(
                        stream,
                        session,
                        vec![game::char_creation_success(
                            character.to_bytes_le(),
                            &name,
                            &[],
                        )],
                        trace,
                    )
                    .await?;
                }
                Err(err) => {
                    eprintln!("world: could not create {name:?}: {err}");
                    send(
                        stream,
                        session,
                        vec![game::char_creation_error(
                            GM_ERROR_CHARACTER_NAME_ALREADY_EXIST,
                        )],
                        trace,
                    )
                    .await?;
                }
            }
            Ok(true)
        }

        // The client drops an item (`GameSrv_HandleDropItem`, `0x802C`: `{item_id: u32, count: u8}`).
        //
        // **This is where this port leaves the reference behind, deliberately.** The reference's handler
        // creates an agent for the item and returns: it never removes the item from its bag, never
        // frees its id, never sends `ITEM_REMOVE`, and bails out entirely when the count is smaller than
        // the stack (`TODO: we must still implement this`) -- so dropping a stacked item there does
        // nothing at all, and the client always sends `count = 1` for a stack. Its item agent also
        // carries the item's *id* as its model id, so the thing on the ground is drawn as nothing. A
        // measured live session (`docs/08-next-inventory-model.md`) showed all three in one log.
        //
        // What this does instead: take `count` off the stack for real, write it down, and say so in the
        // log. It sends nothing -- both messages it could send (`ITEM_REMOVE` and an item agent) crashed a
        // live client, and neither is in any capture. `drop_messages` carries the whole argument.
        0x802C => {
            let body = &frame.body;
            if body.len() < 5 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 7 }));
            }
            let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let count = i32::from(body[4]);
            drop_item(world, play, item_id, count).await?;
            Ok(true)
        }

        // The client moves an item to a slot (`GAME_CMSG_ITEM_MOVE`, `0x8072`: `{item_id: u32,
        // bag_id: u16, slot: u8}` -- the exact inverse of the `ITEM_MOVE_TO_LOCATION` we send). Measured
        // from a live client: dragging an item produced two of these and nothing else.
        //
        // The client has already moved the item in its own model by the time it asks, so a refusal is
        // answered by telling it where the item really is rather than by silence.
        0x8072 => {
            let body = &frame.body;
            if body.len() < 7 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 9 }));
            }
            let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let bag_id = i32::from(u16::from_le_bytes([body[4], body[5]]));
            let slot = i32::from(body[6]);
            move_item(
                stream,
                session,
                world,
                play,
                Placement {
                    item_id,
                    bag_id,
                    slot,
                },
                trace,
            )
            .await?;
            Ok(true)
        }

        // The client equips an item (`GAME_CMSG_EQUIP_ITEM`, `0x8030`: `{item_id: u32}`).
        //
        // The reference has no handler for this at all, so the behaviour here is this project's: the
        // item moves into the equipped-items bag slot its type belongs to, whatever was worn there goes
        // into the first free carrying slot, and the client is told what it is now wearing. The
        // type-to-slot table is `game::equipped_slot`.
        0x8030 => {
            let body = &frame.body;
            if body.len() < 4 {
                return Err(GameError::Framing(FrameError::Incomplete { needed: 6 }));
            }
            let item_id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            equip_item(stream, session, world, play, item_id, trace).await?;
            Ok(true)
        }

        // `GAME_CMSG_UNEQUIP_ITEM` (`0x804F`, 6 bytes: `{u8, u16, u8}`). Its layout carries no item id,
        // and nothing here has seen a live one, so this does not guess: unequipping by dragging an item
        // out of the equipped-items bag arrives as `ITEM_MOVE` above and is handled there.
        0x804F => {
            eprintln!(
                "world: 0x804F (unequip) is not implemented: it carries no item id and no live capture \
                 has shown what it means; dragging the item out of the equipped bag works instead"
            );
            Ok(true)
        }

        // The client answered one of our pings (`GAME_CMSG_PING_REPLY`, `0x8009`, 10 bytes). Nothing to
        // do: the reference discards it. Named rather than ignored, because a trace that says "ignored"
        // twenty-nine times reads like a bug.
        0x8009 => Ok(true),

        // `GAME_CMSG_HEARTBEAT` (`0x800C`, 2 bytes). The other keep-alive, likewise discarded.
        0x800C => Ok(true),

        // A message this world can frame but does not act on. Framing already happened, so ignoring it
        // is safe and is what the reference does for most of them. The name is in the trace when the
        // generated table has one: "0x8072 ignored" is a riddle, "GAME_CMSG_ITEM_MOVE ignored" is a
        // to-do.
        other => {
            if trace {
                let name = asura_protocol::client::game_tables::lookup(other, true)
                    .and_then(|entry| entry.name)
                    .unwrap_or("unnamed");
                eprintln!("world: {other:#06X} {name} ignored");
            }
            Ok(true)
        }
    }
}

/// `GameSrv_HandleDropItem`: take the items off the stack for real, and reserve the agent id a ground
/// item will use once a capture shows what a drop actually sends.
///
/// The shape that agent will have is the reference's own, recorded here so it is not re-derived later:
/// the player's position plus (30, 30), the same plane, facing x, everything `GameSrv_CreateAgent`'s
/// `memset` leaves at zero (speed, team token), and the item's *model* where the reference puts the
/// item's *id*. Nothing is sent from here today: `drop_messages` carries the whole argument.
async fn drop_item(
    world: &World,
    play: &mut Play,
    item_id: i32,
    count: i32,
) -> Result<(), GameError> {
    if item_id <= 0 || count <= 0 {
        eprintln!("world: a drop of {count} of item {item_id} names something that is not an item");
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
    if count > item.quantity {
        eprintln!(
            "world: a drop of {count} of item {item_id} is more than the {} it holds; ignored",
            item.quantity
        );
        return Ok(());
    }

    let Some(taken) = db_inventory::take_from_stack(&world.pool, play.character, item_id, count)
        .await
        .map_err(GameError::Database)?
    else {
        eprintln!("world: item {item_id} could not be taken out of its stack; ignored");
        return Ok(());
    };

    let agent_id = play.agents.allocate();
    eprintln!(
        "world: {} dropped {count} of item {item_id} at ({:.1}, {:.1}), agent {agent_id} reserved, {} \
         left in the bag -- and nothing is sent: both messages a drop could send have crashed a client",
        play.name,
        play.agents.player.position.0 + DROP_OFFSET,
        play.agents.player.position.1 + DROP_OFFSET,
        taken.remaining
    );
    reload(world, play).await
}

/// The messages a drop puts on the wire: **none**.
///
/// This is not caution and it is not a switch any more -- it is the whole of what has been measured. Both
/// candidates put a live client on the floor:
///
/// * `ITEM_REMOVE` (`0x014D`) with the item re-created at what was left: the client crashed, and in
///   another build cloned the item;
/// * the ground agent (`WORLD_CREATE_AGENT`, `0x0020`, `AgentType_Item`): the client crashed, and the
///   log's last line was that message.
///
/// Neither message appears in any capture, and an *item* agent never can: the only `0x0020` a capture
/// carries is a player's (type 1), and the one code path that would have produced an item agent is the
/// reference's own unfinished drop handler. So a drop deducts the stack, writes it down, says so in the
/// log -- and sends nothing, because a client that is running is worth more than an item on the ground.
/// When a capture of a working drop exists, this function grows the message it shows.
pub fn drop_messages() -> Vec<Frame> {
    Vec::new()
}

/// An item and where it is being put: the three fields `GAME_CMSG_ITEM_MOVE` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    item_id: i32,
    bag_id: i32,
    slot: i32,
}

/// `GAME_CMSG_ITEM_MOVE`: put an item in a slot, and tell the client where it ended up.
async fn move_item(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    placement: Placement,
    trace: bool,
) -> Result<(), GameError> {
    let Placement {
        item_id,
        bag_id,
        slot,
    } = placement;
    let before = play.inventory.equipped_item_ids();
    let outcome = db_inventory::move_item(&world.pool, play.character, item_id, bag_id, slot)
        .await
        .map_err(GameError::Database)?;

    let mut frames = Vec::with_capacity(2);
    match outcome {
        db_inventory::MoveOutcome::At { bag_id, slot, .. } => {
            eprintln!("world: item {item_id} moved to bag {bag_id} slot {slot}");
            // Whether the client wants this confirmed is not settled, and both reports are real: with
            // the echo a live client moved the item (that same session also reported a clone, but it was
            // a build that re-created items on a drop too), and with silence the item sprang back and the
            // move did nothing. `MoveEcho` keeps both, and defaults to the echo â€” the one a client was
            // seen to act on.
            frames.push(game::item_move_to_location(
                ITEM_STREAM,
                wire_u32(item_id),
                wire_u16(bag_id),
                wire_u8(slot),
            ));
            reload(world, play).await?;
        }
        db_inventory::MoveOutcome::Unchanged { .. } => return Ok(()),
        db_inventory::MoveOutcome::Refused { item, reason } => {
            eprintln!("world: item {item_id} to bag {bag_id} slot {slot} refused: {reason:?}");
            // The client moved it in its own model before asking, so put it back where it really is.
            if let Some(item) = item {
                if let (Some(bag), Some(slot)) = (item.bag_id, item.slot) {
                    frames.push(game::item_move_to_location(
                        ITEM_STREAM,
                        wire_u32(item_id),
                        wire_u16(bag),
                        wire_u8(slot),
                    ));
                }
            }
        }
    }

    if play.inventory.equipped_item_ids() != before {
        frames.push(equipment_frame(play));
    }
    send(stream, session, frames, trace).await
}

/// `GAME_CMSG_EQUIP_ITEM`: wear an item, evicting whatever the equipped slot already held.
async fn equip_item(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    item_id: i32,
    trace: bool,
) -> Result<(), GameError> {
    let Some(item) = play.inventory.item(item_id).cloned() else {
        eprintln!(
            "world: an equip names item {item_id}, which {0:?} does not have",
            play.name
        );
        return Ok(());
    };
    let Some(slot) = game::equipped_slot(wire_u8(item.item_type)) else {
        eprintln!(
            "world: item {item_id} (type {}) is not something a character wears; equip ignored",
            item.item_type
        );
        return Ok(());
    };
    let Some(bag) = play
        .inventory
        .bags
        .iter()
        .find(|bag| bag.bag_model_id == db_inventory::EQUIPPED_ITEMS_MODEL)
        .copied()
    else {
        eprintln!(
            "world: {0:?} has no equipped-items bag; equip ignored",
            play.name
        );
        return Ok(());
    };

    let slot = i32::from(slot);
    let before = play.inventory.equipped_item_ids();
    let mut frames = Vec::with_capacity(3);

    // Whatever is worn in that slot has to come off first, and there has to be somewhere to put it.
    let occupant = play
        .inventory
        .items_in(bag.bag_id)
        .into_iter()
        .find(|item| item.slot == Some(slot) && item.id != item_id)
        .cloned();
    if let Some(occupant) = occupant {
        let Some((to_bag, to_slot)) =
            db_inventory::first_free_carrying_slot(&world.pool, play.character)
                .await
                .map_err(GameError::Database)?
        else {
            eprintln!(
                "world: no free carrying slot to take item {} off, so item {item_id} cannot be worn",
                occupant.id
            );
            return Ok(());
        };
        match db_inventory::move_item(&world.pool, play.character, occupant.id, to_bag, to_slot)
            .await
            .map_err(GameError::Database)?
        {
            db_inventory::MoveOutcome::At { bag_id, slot, .. } => {
                eprintln!(
                    "world: item {} taken off to bag {bag_id} slot {slot}",
                    occupant.id
                );
                frames.push(game::item_move_to_location(
                    ITEM_STREAM,
                    wire_u32(occupant.id),
                    wire_u16(bag_id),
                    wire_u8(slot),
                ));
            }
            outcome => {
                eprintln!(
                    "world: could not take item {} off to make room for {item_id}: {outcome:?}",
                    occupant.id
                );
                return Ok(());
            }
        }
    }

    match db_inventory::move_item(&world.pool, play.character, item_id, bag.bag_id, slot)
        .await
        .map_err(GameError::Database)?
    {
        db_inventory::MoveOutcome::At { bag_id, slot, .. } => {
            eprintln!("world: item {item_id} worn in equipped slot {slot}");
            // The same switch as a plain move, and the same open question. What the client cannot work
            // out by itself is what the agent now wears, and that is sent below either way.
            frames.push(game::item_move_to_location(
                ITEM_STREAM,
                wire_u32(item_id),
                wire_u16(bag_id),
                wire_u8(slot),
            ));
        }
        db_inventory::MoveOutcome::Unchanged { .. } => {}
        db_inventory::MoveOutcome::Refused { item, reason } => {
            eprintln!("world: equipping item {item_id} refused: {reason:?}");
            if let Some(item) = item {
                if let (Some(bag), Some(slot)) = (item.bag_id, item.slot) {
                    frames.push(game::item_move_to_location(
                        ITEM_STREAM,
                        wire_u32(item_id),
                        wire_u16(bag),
                        wire_u8(slot),
                    ));
                }
            }
        }
    }

    reload(world, play).await?;
    if play.inventory.equipped_item_ids() != before {
        frames.push(equipment_frame(play));
    }
    send(stream, session, frames, trace).await
}

/// The reference's `/maps`, line for line (`GmCommands_ListTargets`).
///
/// It prints two things and they are not the same text. The **chat** side is a list of aliases to type
/// ("Maps: ascalon, lakeside, ...", wrapped inside one chat fragment, then "Use: /tp <name>, e.g. /tp
/// lakeside county"). The **log** side is a table of every alias with the map it names, in the reference's
/// own column widths:
///
/// ```text
/// Teleport targets (20 aliases, every one with a map config):
///   /tp ascalon          -> map 146  Ascalon City (pre-Searing)
/// ```
///
/// The table is what a person reads, and it is the half that has always worked: no chat text has ever
/// reached a client's screen, in the reference or here, and GWPS's own chat gate leaves the chat half in
/// its log with the rest. So this returns the log lines, in that format, and `/maps` prints them. The chat
/// fragments are built too, because they are the answer's other half and a caller may one day have a
/// channel that renders them -- but `/maps` does not send them: sending chat crashed a live client here.
///
/// An alias is the map's name, lower-cased, as the reference's aliases are (`lakeside county`). A map with
/// no name has no alias to type, so it prints its id and says so rather than inventing a name for it.
pub fn maps_answer(maps: &[MapEntry]) -> MappedMaps {
    let aliases: Vec<String> = maps.iter().map(MapEntry::alias).collect();

    // The chat line, split the way the reference splits it: one line per 100 characters, continuation
    // lines indented by six spaces, each entry followed by a comma.
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::from("Maps:");
    for alias in &aliases {
        if current.len() + alias.len() + 3 > 100 {
            lines.push(current);
            current = String::from("      ");
        }
        current.push_str(&format!(" {alias},"));
    }
    lines.push(current);
    lines.push(format!(
        "Use: /tp <name>, e.g. /tp {}",
        aliases.first().map(String::as_str).unwrap_or("<name>")
    ));

    // The table the log gets.
    let mut table = vec![format!(
        "Teleport targets ({} aliases, every one with a map config):",
        maps.len()
    )];
    for (map, alias) in maps.iter().zip(&aliases) {
        table.push(format!(
            "  /tp {:<15} -> map {:>3}  {}",
            alias, map.map_id, map.name
        ));
    }

    MappedMaps { chat: lines, table }
}

/// What `/maps` produces: the lines the reference would put on the chat channel, and the table it puts in
/// its log. Two shapes, because the reference has two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedMaps {
    pub chat: Vec<String>,
    pub table: Vec<String>,
}

/// `UPDATE_AGENT_VISUAL_EQUIPMENT`: what the agent is wearing, read out of the equipped-items bag.
/// How a map alias is compared: lower case, runs of spaces collapsed. The reference's own rule for
/// `/tp` (`GmCommands.c:31`: "`/tp lakeside county` and `/tp LakesideCounty` both work").
fn alias_key(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A fresh `u32` for a player token. The auth service mints the same kind
/// (`AuthSrv_HandleRequestGameInstance`; `crates/asura-auth/src/travel.rs:178` sends the field.
fn random_u32() -> Result<u32, GameError> {
    let mut bytes = [0u8; 4];
    getrandom::getrandom(&mut bytes)
        .map_err(|err| GameError::Io(io::Error::other(err.to_string())))?;
    Ok(u32::from_le_bytes(bytes))
}

/// `/tp <alias>` and `/teleport <alias>`: `GameSrv_HandleTeleportCommand` (`GmCommands.c:155-216`).
///
/// A teleport is not a position change — it is a **server transfer**, the same cycle a portal starts.
/// The reference asks its auth service for a game server on the target district; the auth service mints
/// a player token and records the identity the client's *next* connection must present; the old server
/// sends `TRANSFER_GAME_SERVER_INFO` and the client reconnects to be handed on
/// (`PreSearing/docs/08-gm-commands.md`, "What a teleport actually does").
///
/// This is that cycle with this server's pieces: the seat is reserved for the destination (giving up the
/// current one in the same transaction, `placement::reserve`), the transfer row is written *before* the
/// redirect so a client that reconnects the instant it is told finds it, and the two messages are the
/// five fields the capture shows. The limit is stated rather than hidden: this server never constructs
/// another world, so the alias has to name a map it already hosts.
async fn teleport(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    play: &mut Play,
    argument: &str,
    trace: bool,
) -> Result<bool, GameError> {
    let wanted = alias_key(argument);
    if wanted.is_empty() {
        // `GmCommand_ToAscii` refuses an empty argument, and the answer is the invalid-command line.
        eprintln!("world: /tp without a map name");
        return Ok(true);
    }
    let Some(target) = world
        .maps
        .iter()
        .find(|map| alias_key(&map.alias()) == wanted)
        .cloned()
    else {
        // `GmCommands.c:172-177`: "Unknown map name. Type /maps." through the gated chat sender, which
        // with the gate off is a log line — the same shape as every other GM answer here.
        eprintln!("world: Unknown map name. Type /maps.");
        return Ok(true);
    };
    if target.map_id == play.map.map_id {
        // `GmCommands.c:198-202`.
        eprintln!("world: You are already here.");
        return Ok(true);
    }

    // The seat on the destination. `reserve` gives up this session's current lease in the same
    // transaction, which is exactly a transfer's accounting: one seat, moved.
    let Some(world_id) =
        placement::reserve(&world.pool, world.world_id, target.map_id, play.session_id)
            .await
            .map_err(GameError::Database)?
    else {
        eprintln!("world: {} has no free seat; /tp refused", target.label());
        return Ok(true);
    };

    let player_token = random_u32()?;
    placement::begin_transfer(
        &world.pool,
        player_token,
        world_id,
        target.map_id,
        play.session_id,
        play.account_id,
        play.character,
    )
    .await
    .map_err(GameError::Database)?;

    let std::net::SocketAddr::V4(public) = world.public else {
        eprintln!("world: /tp needs an IPv4 advertised address; refused");
        return Ok(true);
    };
    // `GameSrv.c:1394-1403`: the flag describes the **destination**. 1 for an explorable area, 0 for an
    // outpost, and getting it from the source map produced client error 005.
    let is_explorable = match target.map_type {
        1 | 4 | 6 => 1,
        _ => 0,
    };
    let frames = vec![
        game::transfer_game_server_info(
            public.ip().octets(),
            public.port(),
            world_id as u32,
            0, // region: the reference sends its district's region, and this server has no regions
            target.map_id as u32,
            is_explorable,
            player_token,
        ),
        game::map_update_current_redirect(target.map_id as u32),
    ];
    send(stream, session, frames, trace).await?;
    // From here the session's seat belongs to the connection the client is about to open, so its own
    // teardown must not hand it back (see the release at the end of `handle_connection`).
    play.moved = true;
    eprintln!(
        "world: {:?} teleporting: map {} -> map {} '{}', player token {player_token:#010X}, \
         redirect to {public}",
        play.name,
        play.map.map_id,
        target.map_id,
        target.label()
    );
    Ok(true)
}

fn equipment_frame(play: &Play) -> Frame {
    game::update_agent_visual_equipment(
        play.agents.player.agent_id,
        play.inventory.equipped_item_ids(),
    )
}

/// What a character-creation connection has chosen so far.
///
/// The reference keeps exactly this on its player (`char_creation_campaign`, `primary_profession`) and
/// nothing else: the equipment is not stored either — it is re-derived from the generated table every time
/// the profession changes (`GetDefaultEquipments`) — and only becomes rows at confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Creation {
    campaign: u8,
    profession: u8,
}

impl Creation {
    /// The reference's starting state: `HandleCharCreationRequestPlayer` sets Warrior, and nothing sets the
    /// campaign until the client says so — so a zeroed `Campaign` (`None`, the PvP table) is what a fresh
    /// creation screen is built from.
    fn start() -> Creation {
        Creation {
            campaign: 0,
            profession: DEFAULT_PRIMARY_PROFESSION,
        }
    }
}

/// The bags and items a creation screen shows: the reference's own default set, with the equipped bag
/// holding the chosen profession's starting equipment.
///
/// Built in memory because that is where the reference keeps it too — `HandleCharCreationChangeProf`
/// frees and re-adds items in the player's `GmBag`s, and `HandleCharCreationConfirm` is the only place they
/// become database rows.
fn creation_inventory(creation: Creation) -> Inventory {
    use std::collections::HashMap;

    let equipped_bag = db_inventory::seed::BAGS
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);

    let mut items: Vec<db_inventory::Item> = Vec::new();
    // The capture's item ids mean nothing once rows are renumbered, so the mapping is kept explicitly:
    // a bag names its own item by seed id, and only this map knows which row that became.
    let mut by_seed: HashMap<i32, i32> = HashMap::new();
    let mut next_id = 1i32;

    // Everything the reference's default inventory holds outside the equipped bag -- the backpack's own
    // item and the belt pouch's -- so what the creation screen sees is what the confirm writes.
    for seed_item in db_inventory::seed::ITEMS
        .iter()
        .filter(|item| item.bag_id != equipped_bag)
    {
        by_seed.insert(seed_item.id, next_id);
        items.push(db_inventory::Item {
            id: next_id,
            bag_id: seed_item.bag_id,
            slot: seed_item.slot,
            file_id: seed_item.file_id,
            item_type: seed_item.item_type,
            model_id: seed_item.model_id,
            dye_tint: seed_item.dye_tint,
            dye_colors: seed_item.dye_colors,
            materials: seed_item.materials,
            unk1: seed_item.unk1,
            flags: seed_item.flags,
            value: seed_item.value,
            quantity: seed_item.quantity,
            profession: seed_item.profession,
            name: seed_item.name.to_vec(),
            modifiers: seed_item.modifiers.to_vec(),
        });
        next_id += 1;
    }

    for spec in crate::default_equipment::default_equipment(creation.campaign, creation.profession)
    {
        // `GetBagSlotForItemType`'s mapping, which is what `equip_item` uses too.
        let slot = game::equipped_slot(spec.item_type).map(i32::from);
        items.push(db_inventory::Item {
            id: next_id,
            bag_id: equipped_bag,
            slot,
            file_id: i64::from(spec.file_id),
            item_type: i32::from(spec.item_type),
            model_id: spec.model_id as i32,
            dye_tint: i32::from(spec.dye_tint),
            dye_colors: i32::from(spec.dye_colors),
            materials: i32::from(spec.materials),
            unk1: i32::from(spec.unk1),
            flags: i64::from(spec.flags),
            value: spec.value as i32,
            quantity: spec.quantity as i32,
            profession: i32::from(spec.profession),
            name: spec.name.iter().map(|unit| i32::from(*unit)).collect(),
            modifiers: spec.modifiers.iter().map(|word| i64::from(*word)).collect(),
        });
        next_id += 1;
    }

    let bags = db_inventory::seed::BAGS
        .iter()
        .map(|seed_bag| db_inventory::Bag {
            bag_id: seed_bag.bag_id,
            bag_model_id: seed_bag.bag_model_id,
            bag_type: seed_bag.bag_type,
            slot_count: seed_bag.slot_count,
            assoc_item_id: by_seed.get(&seed_bag.assoc_item).copied().unwrap_or(0),
        })
        .collect();

    Inventory { bags, items }
}

/// The equipped set as rows the confirm writes, owning their slices.
fn creation_equipment_rows(creation: Creation) -> Vec<db_inventory::NewItem> {
    let equipped_bag = db_inventory::seed::BAGS
        .iter()
        .find(|bag| bag.bag_type == 2)
        .map(|bag| bag.bag_id);
    crate::default_equipment::default_equipment(creation.campaign, creation.profession)
        .iter()
        .map(|spec| db_inventory::NewItem {
            bag_id: equipped_bag,
            slot: game::equipped_slot(spec.item_type).map(i32::from),
            file_id: i64::from(spec.file_id),
            item_type: i32::from(spec.item_type),
            model_id: spec.model_id as i32,
            dye_tint: i32::from(spec.dye_tint),
            dye_colors: i32::from(spec.dye_colors),
            materials: i32::from(spec.materials),
            unk1: i32::from(spec.unk1),
            flags: i64::from(spec.flags),
            value: spec.value as i32,
            quantity: spec.quantity as i32,
            profession: i32::from(spec.profession),
            name: spec.name.iter().map(|unit| i32::from(*unit)).collect(),
            modifiers: spec.modifiers.iter().map(|word| i64::from(*word)).collect(),
        })
        .collect()
}

/// Re-read the inventory after a change, so the next message built from it is the new truth.
async fn reload(world: &World, play: &mut Play) -> Result<(), GameError> {
    play.inventory = db_inventory::load(&world.pool, play.character)
        .await
        .map_err(GameError::Database)?;
    Ok(())
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
/// says what the agent is wearing (`UPDATE_AGENT_VISUAL_EQUIPMENT`) â€” the nine ids read out of the
/// equipped-items bag, because they belong to *this* character's items.
pub fn player_block(
    name: &str,
    standing: Standing,
    inventory: &Inventory,
    started: Instant,
    player_id: u32,
    agent_id: u32,
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
    // An empty list is not the same message â€” it is an array the client can index past the end of.
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
    frames.push(game::agent_update_attribute_points(agent_id, 50, 50));
    frames.push(game::player_update_profession(
        agent_id,
        DEFAULT_PRIMARY_PROFESSION,
        0,
        0,
    ));
    frames.push(game::player_update_unlocked_professions(
        agent_id,
        DEFAULT_UNLOCKED_PROFESSIONS,
    ));
    frames.push(game::skillbar_update(agent_id, &[0; 8], &[0; 8], 1));
    frames.push(game::update_unlocked_skills(&[]));
    frames.push(game::character_update_factions(
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ));
    frames.push(game::agent_update_attributes(agent_id, &[0; 42]));
    frames.push(game::world_update_load_time(
        started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
    ));

    // The player, the agent, and what a client needs in order to draw them. The appearance carries
    // the profession the model is built from: all-zero bits say `Profession_None`, which is not a
    // character the client can draw, so the reference's own default is used.
    frames.push(game::player_create(
        player_id,
        agent_id,
        game::Appearance::REFERENCE_DEFAULT.pack(),
        name,
    ));
    // The profession goes out **twice** in the reference: once with the skills and attributes, and
    // again here, immediately after the player is created and before the agent exists
    // (`GameSrv_HandleInstanceLoadRequestPlayers`: `BroadcastPlayerCreate` then
    // `SendPlayerProfession`). It is the second one that gives the freshly created player the
    // profession its model is built from â€” without it the client draws an agent it has no model for.
    frames.push(game::player_update_profession(
        agent_id,
        DEFAULT_PRIMARY_PROFESSION,
        0,
        0,
    ));
    frames.push(game::world_create_agent(&game::CreateAgent {
        agent_id,
        model_id: game::CHAR_CLASS_PLAYER_BASE | player_id,
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
        agent_id,
        game::PLAYER_ENERGY_MAX,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_HEALTH,
        agent_id,
        game::PLAYER_HEALTH_MAX,
    ));
    frames.push(game::agent_property_update_float(
        game::AGENT_PROPERTY_ENERGY_REGEN,
        agent_id,
        0.0,
    ));
    frames.push(game::agent_property_update_int(
        game::AGENT_PROPERTY_PUBLIC_LEVEL,
        agent_id,
        0,
    ));
    frames.push(game::agent_initial_effects(agent_id, 0));
    frames.push(game::update_agent_visual_equipment(
        agent_id,
        inventory.equipped_item_ids(),
    ));
    frames.push(game::agent_display_cape(agent_id, 1));
    frames.push(game::world_update_controlled_agent(agent_id, 3));

    // The party: one member, this player, and the client's own party pointer.
    frames.push(game::party_create(PARTY_ID));
    frames.push(game::party_player_add(PARTY_ID, player_id as u16, 1));
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
            name: "Lakeside County".to_string(),
            capacity: 8,
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
    fn the_load_stream_is_the_references_message_for_message() {
        // Measured, not asserted from memory: `tools/parity-load.py` reads the reference's own
        // GWPS_TRACE=1 log, drops the periodic traffic (pings and simulation ticks, which that log has
        // and a block dump does not), and writes the ordered (header, size) pairs into
        // `load_fixture.rs`. This test builds the same four blocks the server sends and compares.
        //
        // The one content that leaks into a *size* is the character name, because
        // `INSTANCE_LOAD_PLAYER_NAME` is counted: five units, the length of the reference's own name in
        // that log.
        let map = test_map();
        let standing = Standing::from_map(&map);
        let inventory = db_inventory::default_inventory();
        let name = "Appok";

        let mut frames = load_block(&map, name, AGENT_ID);
        frames.extend(items_block(&map, &inventory));
        frames.extend(spawn_block(&map, standing));
        frames.extend(player_block(
            name,
            standing,
            &inventory,
            Instant::now(),
            PLAYER_ID,
            AGENT_ID,
        ));
        let ours: Vec<(u16, usize)> = frames
            .iter()
            .map(|frame| (frame.header, frame.len()))
            .collect();

        assert_eq!(
            ours.len(),
            crate::load_fixture::REFERENCE_LOAD.len(),
            "the same number of messages as the reference sends"
        );
        for (index, (ours_message, reference_message)) in ours
            .iter()
            .zip(crate::load_fixture::REFERENCE_LOAD.iter())
            .enumerate()
        {
            assert_eq!(
                ours_message, reference_message,
                "message {index} disagrees with the reference: header and size must both match"
            );
        }
    }

    #[test]
    fn every_profession_starts_with_the_references_own_equipment() {
        use crate::default_equipment::{default_equipment, EQUIPMENT};

        // Four campaigns x eight professions, which is what `GetDefaultEquipments`'s switch covers.
        assert_eq!(EQUIPMENT.len(), 32, "four campaigns by eight professions");
        for (campaign, profession, items) in EQUIPMENT {
            assert!(
                !items.is_empty(),
                "campaign {campaign} profession {profession} has no items"
            );
        }

        // The first item of a Prophecies Warrior, verbatim from `g_DefWarriorPropheciesPveEquipments`:
        // the chest piece, grey, model 823, a four-unit name and three modifiers.
        let warrior = default_equipment(1, 1);
        assert_eq!(
            warrior.len(),
            6,
            "a Warrior's starting set: five armour pieces and a weapon"
        );
        let chest = warrior[0];
        assert_eq!(chest.file_id, 0x8000_005B);
        assert_eq!(chest.item_type, 7, "ItemType_Body");
        assert_eq!(chest.dye_colors, 11, "DyeColor_Gray");
        assert_eq!(chest.model_id, 823);
        assert_eq!(chest.name, &[0x22A3, 0xFB75, 0xA867, 0x466D]);
        assert_eq!(chest.modifiers, &[0xA3C8_1900, 0x8040_0000, 0xA0F8_1400]);

        // A pair the reference has no table for answers with nothing, rather than a guess.
        assert!(default_equipment(9, 1).is_empty());
    }

    #[test]
    fn the_block_ends_with_the_message_that_leaves_the_loading_screen() {
        let frames = player_block(
            "Asura",
            Standing::from_map(&test_map()),
            &db_inventory::default_inventory(),
            Instant::now(),
            PLAYER_ID,
            AGENT_ID,
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
            PLAYER_ID,
            AGENT_ID,
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
    /// what this server now writes out of six bag rows and nine item rows â€” including the ids, which come
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
            PLAYER_ID,
            AGENT_ID,
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
    fn a_drop_sends_nothing_because_both_candidates_crashed_a_client() {
        // Not a placeholder: a tripwire. `ITEM_REMOVE` (`0x014D`) and an item agent (`0x0020`, type 4)
        // each crashed a live client, and neither appears in any capture. If a message is ever added to
        // this path, it has to arrive with a capture that shows it working -- so this test failing is the
        // reminder, and the reason is in `drop_messages`' documentation.
        assert!(
            drop_messages().is_empty(),
            "a drop must not put an unmeasured message on the wire"
        );
    }

    #[test]
    fn the_chat_messages_a_line_produces_are_the_references_pair() {
        // The reference answers a spoken line with the text and then the speaker (`GameSrv_HandleLocal
        // Message`); it gates the text *it* generates, not the text a player types, which is why the gate
        // is not consulted for this pair.
        let frames = [
            game::chat_message_core("hello"),
            game::chat_message_local(PLAYER_ID, game::CHANNEL_ALL),
        ];
        assert_eq!(frames[0].header, game::CHAT_MESSAGE_CORE);
        assert_eq!(
            frames[0].len(),
            2 + 2 + 2 * 5,
            "five code units: a count, then the text"
        );
        assert_eq!(frames[1].len(), 10);
        assert_eq!(
            u32::from_le_bytes([
                frames[1].body[4],
                frames[1].body[5],
                frames[1].body[6],
                frames[1].body[7]
            ]),
            game::CHANNEL_ALL
        );
        assert_eq!(game::CHANNEL_GROUP, 11, "Channel_Group");
        assert_eq!(game::CHANNEL_TRADE, 12, "Channel_Trade");
    }

    #[test]
    fn maps_prints_the_references_table_and_nothing_else() {
        // `GmCommands_ListTargets` prints two things: chat aliases to type, and a table in its log. The
        // table is the half a person reads, and it is asserted here field for field -- `%-15s` for the
        // alias and `%3d` for the map id, because a format that drifts is a format nobody can compare.
        let answer = maps_answer(&[test_map()]);
        assert_eq!(
            answer.table,
            vec![
                "Teleport targets (1 aliases, every one with a map config):".to_string(),
                "  /tp lakeside county -> map 146  Lakeside County".to_string(),
            ],
            "the reference's header and row shape"
        );
        assert_eq!(
            answer.chat,
            vec![
                "Maps: lakeside county,".to_string(),
                "Use: /tp <name>, e.g. /tp lakeside county".to_string(),
            ],
            "and its chat half: aliases to type, then how to use them"
        );

        // Two maps: the rows line up in the reference's columns, and the aliases are lower-case names.
        let second = MapEntry {
            map_id: 148,
            name: "Ascalon City".to_string(),
            ..test_map()
        };
        let both = maps_answer(&[test_map(), second]);
        assert_eq!(
            both.table[1..],
            [
                "  /tp lakeside county -> map 146  Lakeside County".to_string(),
                "  /tp ascalon city    -> map 148  Ascalon City".to_string(),
            ],
            "the alias column is 15 wide, so 148's row is padded differently to 146's"
        );
        assert_eq!(
            both.chat[0], "Maps: lakeside county, ascalon city,",
            "one chat line, aliases comma-separated"
        );

        // A map with no name has no alias to type; it prints its id instead of inventing one.
        let unnamed = MapEntry {
            name: String::new(),
            ..test_map()
        };
        assert_eq!(
            maps_answer(&[unnamed]).table[1],
            "  /tp map 146         -> map 146  ",
            "an unnamed map is named by its id, and the label is empty because there is no name"
        );
    }

    #[test]
    fn stopping_and_stucking_are_different_messages() {
        // Two paths that look alike in a client and are not alike in the reference:
        //   `GAME_CMSG_CANCEL_MOVEMENT` -> `GameSrv_BroadcastAgentStopMoving`  (0x0028)
        //   `/stuck`                    -> `GameSrv_BroadcastAgentPosition`    (0x002C)
        // Sending the wrong one is the sort of parity bug that looks fine until someone compares traces.
        let stop = game::agent_stop_moving(1);
        assert_eq!(stop.header, game::AGENT_STOP_MOVING);
        assert_eq!(stop.len(), 6, "header, then the agent id");

        let position = game::agent_update_position(1, (6602.0, 4485.0), 0);
        assert_eq!(position.header, game::AGENT_UPDATE_POSITION);
        assert_eq!(position.len(), 16, "header, agent id, two floats, plane");
        assert_eq!(
            f32::from_le_bytes([
                position.body[4],
                position.body[5],
                position.body[6],
                position.body[7]
            ]),
            6602.0,
            "the x coordinate follows the agent id"
        );
        assert_eq!(
            u16::from_le_bytes([position.body[12], position.body[13]]),
            0,
            "and the plane is the last field"
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
