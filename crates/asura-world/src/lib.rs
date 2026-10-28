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

/// How often the world's clock is sent.
///
/// Measured from the live capture (`captures/live-move.jsonl`): a `0x001E` every ~100 ms, carrying the
/// real elapsed milliseconds since the previous one (median 81 ms). The frame is the reference's own
/// 6-byte `GameSrv_UpdateWorldSimulationTick` (`GameMsg.h:521`); only the cadence is the measurement, and
/// the reference's floor is 500 ms.
pub const TICK_INTERVAL: Duration = Duration::from_millis(100);

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

/// MapType_MainTown (0) and MapType_MainExplorable (1), the GmMap.h names for the two kinds of place a
/// client walks around in.
pub const MAP_TYPE_MAIN_TOWN: i32 = 0;
pub const MAP_TYPE_MAIN_EXPLORABLE: i32 = 1;

/// The energy and health a character-creation screen's player-data block reports. **Not** the load block's
/// numbers: `GameSrv_SendPlayerHealthEnergy` (`GmPlayer.c:178-199`) sends 20 and 100, while the load path
/// sends 25 and 400 (measured from the reference's own send log). Two paths, two sets of numbers, and mixing
/// them -- which this server did until the log was read -- is a mismatched block.
pub const CREATION_ENERGY: u32 = 20;
/// See [`CREATION_ENERGY`].
pub const CREATION_HEALTH: u32 = 100;

/// `AgentProperty_Value64` (`GmAgentProperties.h`): the third property the creation path sends, always 0.
pub const AGENT_PROPERTY_VALUE64: u32 = 64;

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
    /// The maps' path graphs, imported from `Gw.dat` at startup, and one finder per map. This is the
    /// reference's `srv->map_context`: what a `MOVE_TO_COORD` is answered from.
    pub graphs: PathGraphs,
    /// Maps loaded **after** startup (`--on-demand`): the destination of a travel this world was not
    /// started with. Kept apart from `maps` so the preloaded set — what the world registered for and
    /// what `/maps` lists — stays exactly what the operator chose, while the demand-loaded map is
    /// still found by the reconnect.
    pub lazy_maps: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<i32, MapEntry>>>,
    /// Whether `/tp` may load a catalogue map this world was not started with. Off by default: the
    /// reference and this server both pre-spawn/preload their destinations, and this switch exists to
    /// measure whether that is a requirement for ASURA or only a property of OpenTyria's design.
    pub load_on_demand: bool,
    /// Slots per map a demand-loaded map publishes when its pool row is created.
    pub capacity: i32,
    /// The client archive path, kept so a demand-loaded map can import its path graph.
    pub archive: String,
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

    /// How many players are in the world. What a leak check needs: an idle pair must stay a pair, and a
    /// departed player must actually leave.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).len()
    }

    /// Whether the world is empty. Paired with [`len`](Self::len) because clippy insists the two go
    /// together, and because "no players" is a real state a test asserts.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
    /// The map entry for `map_id`, if this world serves it — preloaded at startup, or loaded on
    /// demand since (`--on-demand`; see [`World::load_on_demand`]).
    pub fn map(&self, map_id: i32) -> Option<MapEntry> {
        if let Some(map) = self.maps.iter().find(|map| map.map_id == map_id) {
            return Some(map.clone());
        }
        self.lazy_maps
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&map_id)
            .cloned()
    }

    /// Every map this world serves right now: the preloaded list, then any loaded on demand. The
    /// order is the preloaded one first, so `/maps` keeps the reference's table order for the maps
    /// that were there at startup.
    pub fn all_maps(&self) -> Vec<MapEntry> {
        let mut all = self.maps.clone();
        let mut lazy: Vec<MapEntry> = self
            .lazy_maps
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .values()
            .cloned()
            .collect();
        lazy.sort_by_key(|map| map.map_id);
        all.extend(lazy);
        all
    }

    /// Record a map that was loaded after startup (`--on-demand`).
    pub fn insert_lazy_map(&self, map: MapEntry) {
        self.lazy_maps
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(map.map_id, map);
    }
}

/// What the finder said about a walk.
#[derive(Debug, Clone)]
pub enum Walk {
    /// A path, in the reference's reduced form. It may hold a single waypoint naming the destination.
    Found(Vec<pathfind::Waypoint>),
    /// The finder ran and refused: the destination is not reachable from where the agent stands.
    NoPath,
    /// This world has no graph for the map, so nothing can be said about reachability. That happens when
    /// the map's blob could not be imported from `Gw.dat` -- a server whose archive is missing or older
    /// than the catalogue refuses walks rather than inventing them.
    NoGraph,
}

/// The path graphs of the maps this world hosts, and one finder per map.
///
/// The reference keeps both inside its server (`srv->map_context`, `GameSrv_LoadMap`), because it runs one
/// server per map; this hosts several maps in one process, so it keeps a graph per map and one finder per
/// map, built on first use. Cheap to clone (it is shared), because `World` is cloned per connection.
///
/// A graph is keyed by the **`Gw.dat` blob id**, not by map id, because the reference's catalogue gives
/// several maps the same blob (146/148, 160/163, 161/165 and 162/166): one import serves both. A map is
/// *bound* to its blob only when the world knows both — by [`PathGraphs::bind`] at startup, or by
/// [`PathGraphs::import`] when `--on-demand` loads a map. The graphs themselves can come from a `Gw.dat`
/// ([`PathGraphs::load`]) or from a dump made once ([`PathGraphs::from_dump`]), which is what lets a
/// server run without the 3.6 GB archive.
#[derive(Clone, Default)]
pub struct PathGraphs(std::sync::Arc<PathGraphsInner>);

#[derive(Default)]
struct PathGraphsInner {
    /// One graph per `map_file_id`, behind a lock because a demand-loaded map (`--on-demand`) adds its
    /// graph after startup. Reads clone the `Arc` out and drop the lock, so a search never holds it.
    data:
        std::sync::Mutex<std::collections::HashMap<u32, std::sync::Arc<map_data::PathStaticData>>>,
    /// map id -> the blob id its graph is stored under, for the maps this world can path in.
    bound: std::sync::Mutex<std::collections::HashMap<i32, u32>>,
    /// One finder per map, behind one lock. The finder keeps scratch arrays between searches, so sharing
    /// it is the point; the searches themselves are short and a world serves a handful of players.
    finders: std::sync::Mutex<std::collections::HashMap<i32, pathfind::PathContext>>,
}

impl PathGraphs {
    /// Import every map's graph out of a `Gw.dat`, which is what `GameSrv_LoadMap` does with the blob its
    /// map config names.
    ///
    /// A map whose blob is missing or does not import is left out, with a line saying so: the world still
    /// serves that map (a client can load it from its own copy of the archive), it just cannot say
    /// anything about where an agent in it may walk. Only an archive that cannot be opened at all is an
    /// error, and the caller decides what to do about that.
    pub fn load(archive_path: &str, maps: &[MapEntry]) -> io::Result<PathGraphs> {
        let mut archive = gwdat::GwDat::open(archive_path)?;
        let mut data = std::collections::HashMap::new();

        for map in maps {
            if let Some(graph) = import_graph(&mut archive, archive_path, map) {
                data.entry(map.map_file_id).or_insert(graph);
            }
        }

        let graphs = PathGraphs(std::sync::Arc::new(PathGraphsInner {
            data: std::sync::Mutex::new(data),
            bound: std::sync::Mutex::new(std::collections::HashMap::new()),
            finders: std::sync::Mutex::new(std::collections::HashMap::new()),
        }));
        graphs.bind(maps);
        Ok(graphs)
    }

    /// Load the graphs from a dump written by `asura-world --dump-path-graphs`, so no `Gw.dat` is
    /// needed at runtime. Nothing is bound yet: call [`PathGraphs::bind`] with the maps this world
    /// serves, and an on-demand load binds its own map through [`PathGraphs::import`].
    pub fn from_dump(path: &str) -> io::Result<PathGraphs> {
        let bytes = std::fs::read(path)?;
        let dumped = graph_dump::decode(&bytes)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let mut data = std::collections::HashMap::new();
        for graph in dumped {
            data.insert(graph.map_file_id, std::sync::Arc::new(graph.data));
        }
        Ok(PathGraphs(std::sync::Arc::new(PathGraphsInner {
            data: std::sync::Mutex::new(data),
            bound: std::sync::Mutex::new(std::collections::HashMap::new()),
            finders: std::sync::Mutex::new(std::collections::HashMap::new()),
        })))
    }

    /// Bind every map whose blob is among the loaded graphs. Returns how many were bound.
    pub fn bind(&self, maps: &[MapEntry]) -> usize {
        let data = self.0.data.lock().unwrap_or_else(|err| err.into_inner());
        let mut bound = self.0.bound.lock().unwrap_or_else(|err| err.into_inner());
        let mut count = 0;
        for map in maps {
            if data.contains_key(&map.map_file_id) {
                bound.insert(map.map_id, map.map_file_id);
                count += 1;
            }
        }
        count
    }

    /// Make one map pathable: bind it to a graph already loaded (a dump), or import its blob from the
    /// archive. The `--on-demand` half of [`PathGraphs::load`]: an archive that will not open is the
    /// only error; a blob that is missing or does not import is logged and leaves the map without
    /// pathfinding, exactly as at startup.
    pub fn import(&self, map: &MapEntry, archive_path: &str) -> io::Result<()> {
        let already_loaded = self
            .0
            .data
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(&map.map_file_id);
        if already_loaded {
            self.0
                .bound
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .insert(map.map_id, map.map_file_id);
            return Ok(());
        }
        let mut archive = gwdat::GwDat::open(archive_path)?;
        if let Some(graph) = import_graph(&mut archive, archive_path, map) {
            self.0
                .data
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .entry(map.map_file_id)
                .or_insert(graph);
        }
        self.0
            .bound
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(map.map_id, map.map_file_id);
        Ok(())
    }

    /// A world with no graphs: every walk is refused with [`Walk::NoGraph`].
    pub fn empty() -> PathGraphs {
        PathGraphs::default()
    }

    /// How many graphs are loaded — from an archive or a dump — whether or not a map is bound to
    /// them yet. A startup line and tests use it; `/maps` uses [`PathGraphs::maps`] instead.
    pub fn imported(&self) -> usize {
        self.0
            .data
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    /// The maps this world can path in, for a startup line and for tests.
    pub fn maps(&self) -> Vec<i32> {
        let mut maps: Vec<i32> = self
            .0
            .bound
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .keys()
            .copied()
            .collect();
        maps.sort_unstable();
        maps
    }

    /// Ask the finder for a walk from `from` to `to` on `map_id`.
    pub fn walk(&self, map_id: i32, from: pathfind::Pos, to: pathfind::Pos) -> Walk {
        let file_id = self
            .0
            .bound
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&map_id)
            .copied();
        let data = file_id.and_then(|file_id| {
            self.0
                .data
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .get(&file_id)
                .map(std::sync::Arc::clone)
        });
        let Some(data) = data else {
            return Walk::NoGraph;
        };

        let mut finders = self.0.finders.lock().unwrap_or_else(|err| err.into_inner());
        let finder = finders
            .entry(map_id)
            .or_insert_with(|| pathfind::PathContext::new(data));

        let mut waypoints = Vec::new();
        if finder.path_finding(from, to, &mut waypoints) {
            Walk::Found(waypoints)
        } else {
            Walk::NoPath
        }
    }
}

/// Read one map's blob out of an open archive and import it, or log why it is absent.
fn import_graph(
    archive: &mut gwdat::GwDat,
    archive_path: &str,
    map: &MapEntry,
) -> Option<std::sync::Arc<map_data::PathStaticData>> {
    match find_path_blob(archive, map.map_file_id, map.map_id) {
        Ok(Some((blob_id, content))) => {
            if blob_id != map.map_file_id {
                eprintln!(
                    "world: map {} ({}) path graph comes from blob {blob_id:#X} rather than \
                     {:#X}: the archive binds the plain logical id (the bit-31 replacement flag, \
                     docs/03-roadmap.md)",
                    map.map_id, map.name, map.map_file_id
                );
            }
            match map_data::PathStaticData::import(&content) {
                Ok(imported) => Some(std::sync::Arc::new(imported)),
                Err(err) => {
                    eprintln!(
                        "world: map {} ({}) has no path graph: blob {blob_id:#X} did not import: {err}",
                        map.map_id, map.name
                    );
                    None
                }
            }
        }
        Ok(None) => {
            eprintln!(
                "world: map {} ({}) has no path graph: blob {:#X} is not in {archive_path}",
                map.map_id, map.name, map.map_file_id
            );
            None
        }
        Err(err) => {
            eprintln!(
                "world: map {} ({}) has no path graph: blob {:#X} could not be read: {err}",
                map.map_id, map.name, map.map_file_id
            );
            None
        }
    }
}

/// The blob a map's path graph actually lives at, tolerating the archive's bit-31 replacement rule.
///
/// The reference's catalogue names some blobs with bit 31 set (`0x8005EA96` for The Northlands)
/// while a client archive may bind the plain logical id (`0x5EA96`) — the bit is the archive's
/// "replacement pending" flag, not part of the map's identity (`docs/03-roadmap.md`,
/// `docs/10-rurik-comparison.md`). So: the configured id, then the plain one, then the flagged one,
/// and the caller logs when a substitution happened.
///
/// Returns the id that answered and its bytes. `Ok(None)` means none of the three is in the
/// archive; the caller decides whether that is a skip or an error.
pub fn find_path_blob(
    archive: &mut gwdat::GwDat,
    wanted: u32,
    map_id: i32,
) -> io::Result<Option<(u32, Vec<u8>)>> {
    let mut candidates = vec![wanted];
    let plain = wanted & 0x7FFF_FFFF;
    if plain != wanted {
        candidates.push(plain);
    }
    let flagged = wanted | 0x8000_0000;
    if flagged != wanted {
        candidates.push(flagged);
    }
    let mut first_error = None;
    for candidate in candidates {
        match archive.file(candidate) {
            Ok(Some(content)) => {
                if candidate != wanted {
                    eprintln!(
                        "world: map {map_id} path graph found at blob {candidate:#X} \
                         (the catalogue names {wanted:#X})"
                    );
                }
                return Ok(Some((candidate, content)));
            }
            Ok(None) => {}
            Err(err) => {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(None),
    }
}

/// Replaying the reference's captured stream, for bisection (ASURA_WORLD_REPLAY).
/// One agent's position, and the reference's rules for believing a client about it.
pub mod agent;

/// The path-graph dump: `PathStaticData` written once and loaded without a `Gw.dat`, so a server
/// machine needs a few megabytes instead of the client's 3.6 GB archive (`asura-world
/// --dump-path-graphs` / `--path-graphs`).
pub mod graph_dump;

/// A server-side packet capture: every game message both ways, plaintext JSONL, for diagnosing a
/// live session without an injected client (`--capture` / `ASURA_CAPTURE`).
pub mod capture;

pub mod replay;

/// The reference's own load stream, as `(header, size)` pairs, generated by `tools/parity-load.py` from
/// its `GWPS_TRACE=1` log. `the_load_stream_is_the_references_message_for_message` compares this server's
/// blocks against it, which is what makes "the load block matches the reference" a measurement rather
/// than a claim.
pub mod load_fixture;

/// The `Gw.dat` archive's index: the reference's `FaArchive.c`, ported far enough to find a file id and
/// say where its bytes are. The decompressor is the other half and is not here yet (docs/10-parity-queue.md,
/// item D).
pub mod gwdat;

/// The reference's decompressor (`FaCompress.c`), ported: a `Gw.dat` entry is compressed with an LZ +
/// Huffman scheme and this is the only thing that turns it into bytes.
pub mod fa_compress;

/// The decompressor's own tables, generated from the reference by `tools/gen-fa-compress-tables.py`.
pub mod fa_compress_tables;

/// A map blob: the reference's container (`GmMapDataImport.c`) and the path graph inside it, which is what
/// answers "which trapezoid is this position in" -- the first half of the reference's path finder.
pub mod map_data;

/// The reference's path finder (`GmPaths.c` + `GmPathHeap.c`): A* over the trapezoid graph, and the
/// waypoint reduction that turns the chosen chain into a walk. `tools/parity-path.py --check` compares it
/// against the reference's own code, case by case and bit for bit.
pub mod pathfind;

/// The path finder's fixed-point lookup tables, generated from `GmPaths.c` by `tools/gen-path-tables.py`.
pub mod path_tables;

/// The reference's `/tp` alias table (`s_teleport_targets`), generated from `GmCommands.c` by
/// `tools/gen-map-config.py`. Every alias it accepts, with the map it names -- which is not the same set as
/// "the labels of the maps this world hosts": `/tp lakeside` is an alias, "Lakeside County" is the label.
pub mod travel_targets;

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
    // A client whose character stands in a map this world was **not** started with: with `--on-demand`
    // the map is loaded here and the connection goes on, instead of the client being kicked with
    // "this world does not host map X". That is the whole point of the switch — a login into an
    // unhosted map and a /tp into one are the same refusal when it is off, and the same demand-load
    // when it is on.
    let map = match world.map(map_id) {
        Some(map) => map,
        None if world.load_on_demand => {
            match load_map_on_demand(world, map_id)
                .await
                .map_err(GameError::Database)?
            {
                Some(map) => map,
                None => return Err(GameError::UnknownMap(map_id)),
            }
        }
        None => return Err(GameError::UnknownMap(map_id)),
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
        traits: db_game::traits(&world.pool, transfer.character_id)
            .await
            .map_err(GameError::Database)?
            .unwrap_or_default(),
        started: Instant::now(),
        move_stats: MoveStats::default(),
    };
    if move_trace_on() {
        eprintln!(
            "move[spawn] agent {} starts on map {} at (x={:.1}, y={:.1}, plane={}) with speed {} \
             (the client will walk from here)",
            play.agents.player.agent_id,
            play.map.map_id,
            play.agents.player.position.0,
            play.agents.player.position.1,
            play.agents.player.plane,
            game::PLAYER_SPEED_BASE
        );
    }

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
    if move_trace_on() {
        let stats = play.move_stats;
        eprintln!(
            "move[summary] {} walk request(s) (pathfinding), {} position report(s) (manual walk): \
             {} accepted, {} clamped, {} refused, {} plane change(s); {} tick(s) walked, \
             largest client/server gap {:.1} units",
            stats.walk_requests,
            stats.reports,
            stats.accepted,
            stats.clamped,
            stats.refused,
            stats.plane_changes,
            stats.ticks,
            stats.max_drift
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
    inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<Frame>>,
    trace: bool,
) -> Result<(), GameError> {
    let mut tick = tokio::time::interval(TICK_INTERVAL);
    // `GameSrv_WorldTick` announces the real elapsed time between frames, so the clock starts here.
    let mut last_tick = Instant::now();
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
                            crate::capture::record("CToS", frame.header, &frame.encode());
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
                // `GameSrv_WorldTick`: announce the elapsed time, then move every agent along its own
                // walk. The announcement goes to this connection only -- each connection keeps its own
                // ticker here, where the reference has one loop for the whole server, so every client
                // still hears one tick per interval.
                // The announced number is the **real** elapsed time since the previous tick, which is
                // what `GameSrv_WorldTick` sends (`srv->current_frame_time - srv->last_world_tick`).
                let elapsed = last_tick.elapsed();
                last_tick = Instant::now();
                let delta_ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
                let delta = elapsed.as_secs_f32();
                let was_moving = 0.0 < play.agents.player.motion.speed;
                let before = play.agents.player.position;
                let before_plane = play.agents.player.plane;
                let crossed = play.agents.player.advance(delta);
                if move_trace_on() {
                    play.move_stats.ticks += 1;
                    if was_moving {
                        let moved = {
                            let dx = play.agents.player.position.0 - before.0;
                            let dy = play.agents.player.position.1 - before.1;
                            (dx * dx + dy * dy).sqrt()
                        };
                        let expected = game::PLAYER_SPEED_BASE * delta;
                        eprintln!(
                            "move[tick] walked {moved:.1} units (expected {expected:.1} at \
                             {:.0}/s), agent (x={:.1}, y={:.1}, plane={})",
                            game::PLAYER_SPEED_BASE,
                            play.agents.player.position.0,
                            play.agents.player.position.1,
                            play.agents.player.plane
                        );
                        if play.agents.player.plane != before_plane {
                            play.move_stats.plane_changes += 1;
                            eprintln!(
                                "move[jump] the walk changed the agent's plane {before_plane} -> {} \
                                 at (x={:.1}, y={:.1}) — the client will place it on that plane now",
                                play.agents.player.plane,
                                play.agents.player.position.0,
                                play.agents.player.position.1
                            );
                        }
                    }
                    if let Some(waypoint) = &crossed {
                        eprintln!(
                            "move[tick] crossed waypoint {}/{} (x={:.1}, y={:.1}, plane={}, trap={}); \
                             agent now (x={:.1}, y={:.1}, plane={})",
                            play.agents.player.motion.waypoint_idx,
                            play.agents.player.motion.waypoints.len(),
                            waypoint.pos.x,
                            waypoint.pos.y,
                            waypoint.pos.plane,
                            waypoint.trap_id,
                            play.agents.player.position.0,
                            play.agents.player.position.1,
                            play.agents.player.plane
                        );
                    } else if was_moving && play.agents.player.motion.speed == 0.0 {
                        eprintln!(
                            "move[tick] arrival at (x={:.1}, y={:.1}, plane={}); the walk is over",
                            play.agents.player.position.0,
                            play.agents.player.position.1,
                            play.agents.player.plane
                        );
                    }
                }
                let mut frames = vec![game::world_simulation_tick(delta_ms)];
                if let Some(waypoint) = crossed {
                    // A crossed waypoint is a new destination, and the reference broadcasts it: `every`
                    // client is drawing this agent from the last destination it was told
                    // (`GameSrv_UpdateAgentDestination` -> `GameSrv_BroadcastAgentMoveToPoint`).
                    let walk = game::agent_move_to_point(
                        play.agents.player.agent_id,
                        (waypoint.pos.x, waypoint.pos.y),
                        waypoint.pos.plane,
                        play.agents.player.plane,
                    );
                    world
                        .peers
                        .broadcast(play.player_id, std::slice::from_ref(&walk));
                    frames.push(walk);
                }
                send(stream, session, frames, trace).await?;
                // What the world believes about this player advances with it, so a joining client is told
                // where a walker is now rather than where it was when it asked.
                world.peers.update(play.player_id, play.standing());
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
    /// The character's stored traits — what the load block sends instead of constants.
    traits: db_game::Traits,
    /// When the connection began, for the load time the reference reports.
    started: Instant,
    /// Movement counters, for the trace's closing summary.
    move_stats: MoveStats,
}

/// Counters the movement trace keeps for its session summary: what the client asked for, what the
/// server decided about each report, and every point where the agent's plane changed — which is what
/// a client shows as a terrain jump.
#[derive(Debug, Default, Clone, Copy)]
struct MoveStats {
    walk_requests: u32,
    reports: u32,
    accepted: u32,
    clamped: u32,
    refused: u32,
    plane_changes: u32,
    ticks: u32,
    /// The largest gap seen between a reported position and the server's own, before adoption.
    max_drift: f32,
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
pub fn items_block(map: &MapEntry, inventory: &Inventory, gold: u32) -> Vec<Frame> {
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
    frames.push(game::update_gold_storage(ITEM_STREAM, gold));
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

/// Whether the movement trace is on (`ASURA_MOVE_TRACE=1`; `play.ps1 -MoveTrace` sets it).
///
/// Movement is the one part of the wire a capture cannot diagnose after the fact: drift and terrain
/// jumps are a sequence of requests, answers and ticks, and the question is always *what did the
/// server think the agent's position and plane were* at each step. This prints that sequence — every
/// move request with its parsed destination, the path the finder produced (each waypoint's position,
/// plane and trapezoid), the echoed waypoint, every tick that crosses one, and every position report
/// with the budget decision. It prints no packet log, so a session stays readable.
fn move_trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("ASURA_MOVE_TRACE").ok().as_deref(),
            Some("1" | "true" | "yes" | "on")
        )
    })
}

/// A path as the trace prints it: the first few waypoints with their planes and trapezoids.
fn waypoints_preview(waypoints: &[pathfind::Waypoint]) -> String {
    let shown: Vec<String> = waypoints
        .iter()
        .take(10)
        .map(|waypoint| {
            format!(
                "(x={:.1}, y={:.1}, plane={}, trap={})",
                waypoint.pos.x, waypoint.pos.y, waypoint.pos.plane, waypoint.trap_id
            )
        })
        .collect();
    let more = if waypoints.len() > 10 {
        format!(" ... {} more", waypoints.len() - 10)
    } else {
        String::new()
    };
    format!("[{}]{}", shown.join(" "), more)
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
        crate::capture::record("StoC", frame.header, &bytes);
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
                _ => items_block(&play.map, &play.inventory, play.traits.gold),
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
                    play.traits,
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
                // The introduction, then what the others need in order to draw this player:
                // `GameSrv_BroadcastPlayerEquippedItems` (`GmItem.c:129-160`, the *unnamed* item form),
                // then `GameSrv_BroadcastUpdateAgentVisualEquipment` and `GameSrv_BroadcastAgentDisplayCape`
                // — the reference's own order in the same handler (`GameSrv.c:1643-1650`).
                let mut mine = introduction(&me);
                let equipped_bag = play
                    .inventory
                    .bags
                    .iter()
                    .find(|bag| bag.bag_type == 2)
                    .map(|bag| bag.bag_id);
                for item in play
                    .inventory
                    .items
                    .iter()
                    .filter(|item| item.bag_id == equipped_bag)
                {
                    mine.push(game::create_unnamed_item(&named_item(item)));
                }
                mine.push(equipment_frame(play));
                mine.push(game::agent_display_cape(play.agents.player.agent_id, 1));
                world.peers.broadcast(play.player_id, &mine);
            }
            send(stream, session, block, trace).await?;
            Ok(true)
        }

        // The client asks to be pinged back.
        0x800D => {
            send(stream, session, vec![game::ping_reply(PING_VALUE)], trace).await?;
            Ok(true)
        }

        // The client asks to walk somewhere (`GameSrv_HandleMoveToCoord`, `GmAgent.c:371-398`).
        //
        // The reference asks its path finder for the walk and then answers with **the first waypoint of
        // its own path**, not with the client's destination: the client predicts a straight line, and a
        // straight line through a wall is exactly what this message corrects. When the finder refuses, the
        // reference sends no refusal of its own -- it cancels the walk and broadcasts where the agent
        // really is, which is what makes the client snap back.
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
            let agent_id = play.agents.player.agent_id;
            let from = play.agents.player.here();
            let to = pathfind::Pos {
                x: destination.0,
                y: destination.1,
                plane,
            };
            let walk_active = 0.0 < play.agents.player.motion.speed;
            if move_trace_on() {
                play.move_stats.walk_requests += 1;
                eprintln!(
                    "move[pathfind] agent {agent_id} asked to walk to (x={:.1}, y={:.1}, plane={plane}) \
                     while the server thinks it stands at (x={:.1}, y={:.1}, plane={}){} \
                     [0x803E, {} bytes]",
                    destination.0,
                    destination.1,
                    from.x,
                    from.y,
                    from.plane,
                    if walk_active {
                        " — a server-side walk was still running (client has walked since its last report)"
                    } else {
                        ""
                    },
                    frame.body.len() + 2
                );
            }

            let walked = world.graphs.walk(play.map.map_id, from, to);
            if move_trace_on() {
                match &walked {
                    Walk::Found(waypoints) => eprintln!(
                        "move[pathfind] finder produced {} waypoint(s): {}",
                        waypoints.len(),
                        waypoints_preview(waypoints)
                    ),
                    Walk::NoPath => eprintln!("move[pathfind] finder refused: no path"),
                    Walk::NoGraph => eprintln!(
                        "move[pathfind] finder refused: this world has no path graph for map {}",
                        play.map.map_id
                    ),
                }
            }
            if trace {
                let said = match &walked {
                    Walk::Found(waypoints) => format!("{} waypoint(s)", waypoints.len()),
                    Walk::NoPath => "no path".to_string(),
                    Walk::NoGraph => "no path graph for this map".to_string(),
                };
                eprintln!(
                    "world: agent {agent_id} walks from ({:.1}, {:.1}) plane {} to ({:.1}, {:.1}) \
                     plane {plane}: {said}",
                    from.x, from.y, from.plane, destination.0, destination.1
                );
            }

            match walked {
                // An empty path sends nothing: the reference checks `0 < agent->waypoints.len`.
                Walk::Found(waypoints) => {
                    if let Some(first) = play.agents.player.begin_walk(waypoints) {
                        let walk = game::agent_move_to_point(
                            agent_id,
                            (first.pos.x, first.pos.y),
                            first.pos.plane,
                            play.agents.player.plane,
                        );
                        if move_trace_on() {
                            eprintln!(
                                "move[pathfind] answered with waypoint 0 (x={:.1}, y={:.1}, plane={}) \
                                 current_plane={} [0x0029, {} bytes]; {} waypoint(s) left to walk, \
                                 agent stays at (x={:.1}, y={:.1}, plane={})",
                                first.pos.x,
                                first.pos.y,
                                first.pos.plane,
                                play.agents.player.plane,
                                walk.len(),
                                play.agents.player.motion.waypoints.len(),
                                play.agents.player.position.0,
                                play.agents.player.position.1,
                                play.agents.player.plane
                            );
                        }
                        // Everyone else is told where this agent is walking: the reference broadcasts the
                        // same message (`GameSrv_BroadcastAgentMoveToPoint`), and a client that is not
                        // told keeps drawing the walker where it last stood.
                        world
                            .peers
                            .broadcast(play.player_id, std::slice::from_ref(&walk));
                        send(stream, session, vec![walk], trace).await?;
                    }
                }
                Walk::NoPath | Walk::NoGraph => {
                    if trace {
                        eprintln!(
                            "world: path finding failed for agent {agent_id}; standing still"
                        );
                    }
                    play.agents.player.cancel_movement();
                    let standing = play.standing();
                    let position =
                        game::agent_update_position(agent_id, standing.position, standing.plane);
                    if move_trace_on() {
                        eprintln!(
                            "move[pathfind] walk refused; snapped to (x={:.1}, y={:.1}, plane={}) \
                             [0x002D, {} bytes]",
                            standing.position.0,
                            standing.position.1,
                            standing.plane,
                            position.len()
                        );
                    }
                    world.peers.update(play.player_id, standing);
                    world
                        .peers
                        .broadcast(play.player_id, std::slice::from_ref(&position));
                    send(stream, session, vec![position], trace).await?;
                }
            }
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
            let now = Instant::now();
            let before = play.standing();
            let since = play.agents.player.since_last_report(now);
            let walk_was_running = 0.0 < play.agents.player.motion.speed;
            let adopted = play.agents.player.adopt_report(position, plane, now);
            let standing = play.standing();
            if move_trace_on() {
                play.move_stats.reports += 1;
                match adopted {
                    Adopted::Accepted => play.move_stats.accepted += 1,
                    Adopted::Clamped => play.move_stats.clamped += 1,
                    Adopted::Refused => play.move_stats.refused += 1,
                }
                let distance = {
                    let dx = position.0 - before.position.0;
                    let dy = position.1 - before.position.1;
                    (dx * dx + dy * dy).sqrt()
                };
                play.move_stats.max_drift = play.move_stats.max_drift.max(distance);
                eprintln!(
                    "move[manual] report (x={:.1}, y={:.1}, plane={plane}) against the server's \
                     (x={:.1}, y={:.1}, plane={}) — d={distance:.1}, {} ms since the last report, \
                     server walk running={walk_was_running}: {adopted:?}, agent now \
                     (x={:.1}, y={:.1}, plane={}) [0x803D, {} bytes]",
                    position.0,
                    position.1,
                    before.position.0,
                    before.position.1,
                    before.plane,
                    since.map(|elapsed| elapsed.as_millis()).unwrap_or(0),
                    standing.position.0,
                    standing.position.1,
                    standing.plane,
                    frame.body.len() + 2
                );
                if adopted == Adopted::Accepted && plane != before.plane {
                    play.move_stats.plane_changes += 1;
                    eprintln!(
                        "move[jump] the client's report changed the plane {before_plane} -> {plane} \
                         at (x={:.1}, y={:.1}){note}",
                        position.0,
                        position.1,
                        before_plane = before.plane,
                        note = if distance > 50.0 {
                            " — more than the tolerance away, so this is a real plane change, not stairs"
                        } else {
                            ""
                        }
                    );
                }
                if adopted != Adopted::Accepted && distance > 50.0 {
                    eprintln!(
                        "move[jump] report rejected or clamped {adopted:?}: client at (x={:.1}, \
                         y={:.1}, plane={plane}), server at (x={:.1}, y={:.1}, plane={}), d={distance:.1}",
                        position.0,
                        position.1,
                        before.position.0,
                        before.position.1,
                        before.plane
                    );
                }
            }
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
        // The client stopped walking (`GameSrv_HandleCancelMovement`): cancel the agent's own walk --
        // `GameSrv_CancelAgentMovement` sets its destination to where it stands and its speed to zero --
        // and then tell everyone it stopped. Cancelling is the load-bearing half: without it the server
        // keeps advancing its copy along the abandoned path, and the next click is answered from a
        // position the client walked away from minutes ago (observed live: a click to x4 started from
        // the old x2 the client had already left, pulling the agent back to it).
        0x8028 => {
            let was_walking = 0.0 < play.agents.player.motion.speed;
            play.agents.player.cancel_movement();
            let stop = game::agent_stop_moving(play.agents.player.agent_id);
            if move_trace_on() {
                eprintln!(
                    "move[stop] client cancelled walking at (x={:.1}, y={:.1}, plane={}){} \
                     [0x8028, {} bytes]",
                    play.agents.player.position.0,
                    play.agents.player.position.1,
                    play.agents.player.plane,
                    if was_walking {
                        " — a server-side walk WAS running: cancelled (speed -> 0, destination cleared)"
                    } else {
                        " (no server-side walk was running)"
                    },
                    frame.body.len() + 2
                );
            }
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
                    // The command surface, from the reference's own two places: `GmCommands.c`'s
                    // `GameSrv_TryHandleGmCommand` (`/tp`, `/teleport`, `/maps`) and `GmChat.c`'s
                    // `GameSrv_HandleChatCommand` (`/stuck`). `COMMANDS` names them once, and
                    // `tools/parity-commands.py` compares that list with the reference's so the surface
                    // cannot drift without a check failing.
                    let spoken = spoken.trim();
                    let (name, argument) = match spoken.split_once(' ') {
                        Some((name, rest)) => (name, rest.trim_start()),
                        None => (spoken, ""),
                    };
                    match command(name) {
                        Some(named) if named.names.contains(&"stuck") && argument.is_empty() => {
                            // `GameSrv_HandleChatCommand`'s `stuck`: cancel the movement and broadcast where
                            // the agent is now. **Not** `AGENT_STOP_MOVING` -- that is what
                            // `HandleCancelMovement` (`0x8028`) answers, and the reference sends a position
                            // update here instead. It also broadcasts a world tick first; the 500 ms ticker
                            // covers that, so no second clock is invented.
                            let player = &play.agents.player;
                            if trace {
                                eprintln!(
                                    "world: /stuck -- agent {} stands at ({:.1}, {:.1}) plane {}",
                                    player.agent_id,
                                    player.position.0,
                                    player.position.1,
                                    player.plane
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
                        }
                        Some(named) if named.names.contains(&"maps") && argument.is_empty() => {
                            // `GmCommands_ListTargets`, as it behaves with the reference's own defaults: the
                            // table goes to the log, and nothing goes on the wire. That is parity, not a
                            // gate -- GWPS's chat gate is what leaves its chat half in its log too, and no
                            // chat text has ever reached a screen in either server. Sending it here crashed
                            // a live client, which is why the answer is printed and not sent.
                            let mut listed = world.all_maps();
                            if world.load_on_demand {
                                // On-demand: every catalogue map is one this world will serve
                                // (it loads it on first use), so /maps answers the reference's
                                // table even before a map has been used. One query per map that
                                // has not been loaded yet; /maps is a console command, not a hot
                                // path.
                                if let Ok(ids) = db_game::configured_map_ids(&world.pool).await {
                                    for map_id in ids {
                                        if listed.iter().any(|map| map.map_id == map_id) {
                                            continue;
                                        }
                                        if let Ok(Some(configured)) =
                                            db_game::spawn_map(&world.pool, map_id).await
                                        {
                                            listed.push(MapEntry {
                                                map_id,
                                                map_file_id: u32::try_from(configured.map_file_id)
                                                    .unwrap_or(0),
                                                map_type: configured.map_type,
                                                spawn: (configured.spawn_x, configured.spawn_y),
                                                spawn_plane: configured.spawn_plane as u16,
                                                name: configured.name.clone(),
                                                capacity: world.capacity,
                                            });
                                        }
                                    }
                                }
                            }
                            let answer = maps_answer(&listed);
                            for line in &answer.table {
                                eprintln!("world: {line}");
                            }
                            if trace {
                                for line in &answer.chat {
                                    eprintln!("world: /maps (chat, not sent): {line}");
                                }
                            }
                            Ok(true)
                        }
                        Some(named) if named.names.contains(&"tp") && !argument.is_empty() => {
                            // `GameSrv_HandleTeleportCommand`: a *server transfer*, which is what the game
                            // itself does when a portal is walked through. Every byte of the redirect is
                            // measured (`docs/03-protocol.md` §5) and the reference's refusals are here
                            // verbatim, so this is a port rather than an invention.
                            teleport(stream, session, world, play, argument, trace).await
                        }
                        _ => {
                            // `GameSrv_SendInvalidCommand`: the reference's text is "Unknown command". That
                            // covers `/tp` with no map name (`GmCommand_ToAscii` refuses the empty
                            // argument) and every name that is not in the table; with its chat gate off
                            // it is a log line and nothing else, and so it is here.
                            eprintln!("world: Unknown command");
                            Ok(true)
                        }
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
            // `GameSrv_SendPlayerHealthEnergy` (`GmPlayer.c:178-199`): three **int** properties with the
            // creation path's own numbers -- energy 20, health 100, property 64 zero. The load block's numbers
            // differ (25, 400, a regen float and a level), because the reference sends different ones there.
            frames.push(game::agent_property_update_int(
                game::AGENT_PROPERTY_ENERGY,
                agent_id,
                CREATION_ENERGY,
            ));
            frames.push(game::agent_property_update_int(
                game::AGENT_PROPERTY_HEALTH,
                agent_id,
                CREATION_HEALTH,
            ));
            frames.push(game::agent_property_update_int(
                AGENT_PROPERTY_VALUE64,
                agent_id,
                0,
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
                // GameSrv_SendItemsInBag sends the *unnamed* form here, not the named one the
                // inventory stream uses (GmItem.c:111-127).
                frames.push(game::create_unnamed_item(&named_item(item)));
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
            // The traits the confirm implies: the packed appearance the client sent (`msg->config`, four
            // bytes of `Appearance`), the profession the screen last chose, and the one unlocked-profession
            // bit the reference sets (`character.unlocked_professions = 1 << app.primary_profession`).
            let config_at = 2 + units * 2;
            let appearance = u32::from_le_bytes([
                body.get(config_at).copied().unwrap_or(0),
                body.get(config_at + 1).copied().unwrap_or(0),
                body.get(config_at + 2).copied().unwrap_or(0),
                body.get(config_at + 3).copied().unwrap_or(0),
            ]);
            let traits = db_game::Traits {
                appearance,
                profession: creation.profession,
                unlocked_professions: 1 << creation.profession,
                gold: 0,
                campaign: creation.campaign,
                // The creation flow has no secondary profession: the reference sets one only through
                // `CHAR_CREATION_CHANGE_PROF`, and its confirm leaves the field zero.
                secondary_profession: 0,
            };
            match db_inventory::create_character(
                &world.pool,
                play.account_id,
                &name,
                play.map.map_id,
                traits,
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

/// The reference's `/maps`, line for line (`GmCommand_ListTargets`).
///
/// It prints two things and they are **not the same text**, which is the part I had wrong until a client
/// tester pointed out that the list no longer showed every map:
///
/// * the **chat** side is one alias per map -- the first in the table, because that is the one to type --
///   comma-separated and wrapped inside one chat fragment, then the reference's fixed
///   "`Use: /tp <name>, e.g. /tp lakeside county`";
/// * the **log** side is **every alias**, one row each, with the map it names, in the reference's own column
///   widths and in its table order:
///
/// ```text
/// Teleport targets (20 aliases, every one with a map config):
///   /tp ascalon          -> map 148  Ascalon City (pre-Searing)
///   /tp ascalon city     -> map 148  Ascalon City (pre-Searing)
/// ```
///
/// The header's count is the **alias** count, not the map count, and the loop below prints a row per alias:
/// collapsing it to one row per map (which is what the chat half does) is how `/maps` stopped naming the
/// aliases a tester has to type.
///
/// The table is what a person reads, and it is the half that has always worked: no chat text has ever
/// reached a client's screen, in the reference or here, and GWPS's own chat gate leaves the chat half in its
/// log with the rest. So this returns the log lines, in that format, and `/maps` prints them. The chat
/// fragments are built too, because they are the answer's other half and a caller may one day have a channel
/// that renders them -- but `/maps` does not send them: sending chat crashed a live client here.
///
/// Only the aliases of maps **this world hosts** are listed, so a narrowed `--maps` does not advertise a
/// target `/tp` would refuse. With the default (the whole catalogue) that is the reference's whole table.
/// A map the reference's table does not name gets one row of its own, named by its id, rather than vanishing.
pub fn maps_answer(maps: &[MapEntry]) -> MappedMaps {
    let hosted: Vec<i32> = maps.iter().map(|map| map.map_id).collect();

    // The rows, in the reference's table order: one per alias.
    let mut rows: Vec<(String, i32, String)> = Vec::new();
    let mut chat: Vec<String> = Vec::new();
    let mut seen: Vec<i32> = Vec::new();
    for target in travel_targets::TRAVEL_TARGETS {
        if !hosted.contains(&target.map_id) {
            continue;
        }
        rows.push((
            target.alias.to_string(),
            target.map_id,
            target.label.to_string(),
        ));
        if !seen.contains(&target.map_id) {
            seen.push(target.map_id);
            chat.push(target.alias.to_string());
        }
    }
    for map in maps {
        if seen.contains(&map.map_id) {
            continue;
        }
        seen.push(map.map_id);
        let alias = map.alias();
        chat.push(alias.clone());
        rows.push((alias, map.map_id, map.name.clone()));
    }

    // The chat line, split the way the reference splits it: `100 <= used + needed` starts a continuation
    // line indented by six spaces, each entry followed by a comma.
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::from("Maps:");
    for alias in &chat {
        if 100 <= current.len() + alias.len() + 3 {
            lines.push(current);
            current = String::from("      ");
        }
        current.push_str(&format!(" {alias},"));
    }
    lines.push(current);
    // The reference's own sentence, literal: it names `lakeside county` whatever is in the table.
    lines.push("Use: /tp <name>, e.g. /tp lakeside county".to_string());

    // The table the log gets: the alias count in the header, then every alias.
    let mut table = vec![format!(
        "Teleport targets ({} aliases, every one with a map config):",
        rows.len()
    )];
    for (alias, map_id, label) in &rows {
        table.push(format!("  /tp {alias:<15} -> map {map_id:>3}  {label}"));
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
/// `/tp` (`GmCommand_ToAscii`).
fn alias_key(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One chat command this world answers.
///
/// The table is the single place the command surface is written down, and the handler dispatches through
/// it, so `tools/parity-commands.py` comparing it with the reference cannot drift from what the code does.
/// The reference's own surface is two functions: `GameSrv_TryHandleGmCommand` (`GmCommands.c`, which owns
/// `/tp`, `/teleport` and `/maps`) and `GameSrv_HandleChatCommand` (`GmChat.c`, which owns `/stuck`).
#[derive(Debug, Clone, Copy)]
pub struct Command {
    /// What a player types, without the slash. `GmCommands.c` accepts two names for one command.
    pub names: &'static [&'static str],
    /// Where the reference's own dispatch handles it, for a reader who wants to check.
    pub reference: &'static str,
    /// Whether the command takes an argument (`GameSrv_TryHandleGmCommand` splits on the first space).
    pub takes_argument: bool,
    /// What answering it does.
    pub answer: &'static str,
}

/// Every chat command this world answers, in the reference's order.
pub const COMMANDS: &[Command] = &[
    Command {
        names: &["tp", "teleport"],
        reference: "GmCommands.c:184 GameSrv_TryHandleGmCommand -> GameSrv_HandleTeleportCommand",
        takes_argument: true,
        answer: "a server transfer, or one of the reference's three refusals (unknown map, no config, already here)",
    },
    Command {
        names: &["maps"],
        reference: "GmCommands.c:206 GameSrv_TryHandleGmCommand -> GmCommand_ListTargets",
        takes_argument: false,
        answer: "the alias table, printed: the reference's chat half is gated off, so nothing goes on the wire",
    },
    Command {
        names: &["stuck"],
        reference: "GmChat.c:243 GameSrv_HandleChatCommand",
        takes_argument: false,
        answer: "AGENT_UPDATE_POSITION for the agent -- a position, not AGENT_STOP_MOVING",
    },
];

/// The command a player typed, if this world answers it.
pub fn command(name: &str) -> Option<&'static Command> {
    COMMANDS
        .iter()
        .find(|candidate| candidate.names.contains(&name))
}

/// The reference's `/tp` lookup: normalise the argument, find it in its table.
///
/// `GmCommand_ToAscii` refuses anything that is not printable ASCII, which is why a non-ASCII argument is
/// an unknown command rather than a fuzzy match; and the table is exact, so `lakesidecounty` is not
/// `lakeside county` even though the reference's own comment says otherwise.
fn travel_target(argument: &str) -> Option<&'static travel_targets::TravelTarget> {
    let wanted = alias_key(argument);
    if wanted.is_empty() {
        return None;
    }
    travel_targets::TRAVEL_TARGETS
        .iter()
        .find(|target| target.alias == wanted)
}

/// A fresh `u32` for a player token. The auth service mints the same kind
/// (`AuthSrv_HandleRequestGameInstance`; `crates/asura-auth/src/travel.rs:178` sends the field.
fn random_u32() -> Result<u32, GameError> {
    let mut bytes = [0u8; 4];
    getrandom::getrandom(&mut bytes)
        .map_err(|err| GameError::Io(io::Error::other(err.to_string())))?;
    Ok(u32::from_le_bytes(bytes))
}

/// Load a catalogue map this world was **not** started with — the `--on-demand` experiment.
///
/// Off unless `--on-demand` (`World::load_on_demand`): the reference pre-spawns one server per
/// configured map so that a travel never constructs anything, and this server preloads every hosted
/// map for the same reason. This switch answers the open question of whether that readiness is a hard
/// requirement for ASURA or only a property of OpenTyria's design (`docs/03-protocol.md`).
///
/// The three steps are done here, in this order, because they are what a preload would have done at
/// startup: read the `map_config` row, publish the seat pool (what `reserve` counts and what the auth
/// front door's world lookup sees), and import the path graph out of `Gw.dat`. Each step's cost is
/// logged; a client load that hangs while one of these runs is the measurement the switch exists for.
///
/// `None` means "no usable config": no `map_file_id`, or a map type this world does not serve. That is
/// the same refusal a preloaded map with bad config would get, so the client sees one refusal shape.
async fn load_map_on_demand(
    world: &World,
    map_id: i32,
) -> Result<Option<MapEntry>, asura_db::DatabaseError> {
    let started = Instant::now();
    let Some(configured) = db_game::spawn_map(&world.pool, map_id).await? else {
        return Ok(None);
    };
    let map_file_id = u32::try_from(configured.map_file_id).unwrap_or(0);
    if map_file_id == 0 {
        return Ok(None);
    }
    if !matches!(
        configured.map_type,
        MAP_TYPE_MAIN_TOWN | MAP_TYPE_MAIN_EXPLORABLE | MAP_TYPE_CHARACTER_CREATION
    ) {
        return Ok(None);
    }
    let entry = MapEntry {
        map_id,
        map_file_id,
        map_type: configured.map_type,
        spawn: (configured.spawn_x, configured.spawn_y),
        spawn_plane: configured.spawn_plane as u16,
        name: configured.name.clone(),
        capacity: world.capacity,
    };
    let row_done = started.elapsed();

    let pool = Instant::now();
    placement::publish_pool(&world.pool, world.world_id, map_id, world.capacity).await?;
    let pool_done = pool.elapsed();

    let graph = Instant::now();
    if let Err(err) = world.graphs.import(&entry, &world.archive) {
        eprintln!(
            "world: map {map_id} '{}' loaded on demand without a path graph: {err}",
            entry.name
        );
    }
    let graph_done = graph.elapsed();
    world.insert_lazy_map(entry.clone());

    eprintln!(
        "world: map {} '{}' loaded on demand in {} ms (row {} ms, pool {} ms, path graph {} ms); \
         the travel goes on with this destination",
        map_id,
        entry.name,
        started.elapsed().as_millis(),
        row_done.as_millis(),
        pool_done.as_millis(),
        graph_done.as_millis()
    );
    Ok(Some(entry))
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
    // The alias table, not the map names: the reference accepts `lakeside` as well as `lakeside county`
    // (`s_teleport_targets` lists both), so matching against `MapEntry::alias()` answered "unknown map
    // name" for half the aliases the reference's own `/maps` prints.
    let Some(target) = travel_target(argument) else {
        // `GmCommands.c:172-177`: "Unknown map name. Type /maps." through the gated chat sender, which
        // with the gate off is a log line — the same shape as every other GM answer here.
        eprintln!("world: Unknown map name. Type /maps.");
        return Ok(true);
    };
    if !target.hosted {
        // `GmCommands.c:180-190`: the reference refuses a target with no `GmMapConfig` rather than letting
        // `GameSrv_LoadMap` fall back to Kamadan. Here the equivalent is a map this world has no row for.
        eprintln!(
            "world: That map has no config; the server would fall back, so the teleport is refused ({}).",
            target.label
        );
        return Ok(true);
    }
    let target = match world.map(target.map_id) {
        Some(map) => map,
        None if world.load_on_demand => {
            // `--on-demand`: the destination was not preloaded. Load it here, and measure the cost —
            // this is the experiment the switch exists for (docs/03-protocol.md, "precache").
            match load_map_on_demand(world, target.map_id)
                .await
                .map_err(GameError::Database)?
            {
                Some(loaded) => loaded,
                None => {
                    eprintln!(
                        "world: {} ({}) has no usable config (no map file id, or a map type this \
                         world does not serve); the teleport is refused",
                        target.label, target.map_id
                    );
                    return Ok(true);
                }
            }
        }
        None => {
            // The table knows the map; this world was not started with `--maps` for it. The
            // reference pre-spawns one server per map, so its answer would be a transfer; this one
            // can only say so — unless `--on-demand` loaded it (handled above).
            eprintln!(
                "world: {} ({}) is not hosted by this world; start it with --maps {} (or --on-demand \
                 to load destinations at travel time)",
                target.label, target.map_id, target.map_id
            );
            return Ok(true);
        }
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
    traits: db_game::Traits,
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
        traits.profession,
        0,
        0,
    ));
    frames.push(game::player_update_unlocked_professions(
        agent_id,
        traits.unlocked_professions,
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
    // The appearance the client chose, or the reference's default for a character with no bits recorded:
    // all-zero bits say `Profession_None`, which is not a character the client can draw.
    let appearance = if traits.appearance == 0 {
        game::Appearance::REFERENCE_DEFAULT.pack()
    } else {
        traits.appearance
    };
    frames.push(game::player_create(player_id, agent_id, appearance, name));
    // The profession goes out **twice** in the reference: once with the skills and attributes, and
    // again here, immediately after the player is created and before the agent exists
    // (`GameSrv_HandleInstanceLoadRequestPlayers`: `BroadcastPlayerCreate` then
    // `SendPlayerProfession`). It is the second one that gives the freshly created player the
    // profession its model is built from â€” without it the client draws an agent it has no model for.
    frames.push(game::player_update_profession(
        agent_id,
        traits.profession,
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
        // The ping is the reference's own constant; the tick cadence is the measured 100 ms (the frame is
        // the reference's 6-byte form either way).
        assert_eq!(TICK_INTERVAL, Duration::from_millis(100));
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
        frames.extend(items_block(&map, &inventory, 0));
        frames.extend(spawn_block(&map, standing));
        frames.extend(player_block(
            name,
            standing,
            &inventory,
            Instant::now(),
            PLAYER_ID,
            AGENT_ID,
            db_game::Traits::default(),
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
    fn the_block_sends_the_characters_own_appearance_not_the_default() {
        // The reference's load block carries whatever `HandleCharCreationConfirm` stored
        // (`GameSrv.c:1869-1886`). Before the traits existed this server sent
        // `Appearance::REFERENCE_DEFAULT` to everyone, so every character looked the same.
        let map = test_map();
        let standing = Standing::from_map(&map);
        let inventory = db_inventory::default_inventory();
        let traits = db_game::Traits {
            appearance: 0x1234_5678,
            profession: 9,
            unlocked_professions: 0x40,
            gold: 0,
            campaign: 1,
            secondary_profession: 0,
        };
        let frames = player_block(
            "Asura",
            standing,
            &inventory,
            Instant::now(),
            PLAYER_ID,
            AGENT_ID,
            traits,
        );
        let created = frames
            .iter()
            .find(|frame| frame.header == game::PLAYER_CREATE)
            .expect("a player create");
        assert_eq!(
            u32::from_le_bytes([
                created.body[8],
                created.body[9],
                created.body[10],
                created.body[11]
            ]),
            traits.appearance,
            "the appearance the character was created with"
        );

        // A character with no bits recorded still gets the reference's default, because all-zero bits say
        // `Profession_None` and the client cannot draw that.
        let fallback = player_block(
            "Asura",
            standing,
            &inventory,
            Instant::now(),
            PLAYER_ID,
            AGENT_ID,
            db_game::Traits::default(),
        );
        let created = fallback
            .iter()
            .find(|frame| frame.header == game::PLAYER_CREATE)
            .expect("a player create");
        assert_eq!(
            u32::from_le_bytes([
                created.body[8],
                created.body[9],
                created.body[10],
                created.body[11]
            ]),
            game::Appearance::REFERENCE_DEFAULT.pack(),
            "an unrecorded appearance falls back to the reference's default"
        );
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
            db_game::Traits::default(),
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
            db_game::Traits::default(),
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
        let frames = items_block(&test_map(), &reference_inventory(), 0);
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
            db_game::Traits::default(),
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
        let frames = items_block(&test_map(), &Inventory::default(), 0);
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
        // `GmCommand_ListTargets` prints two things, and they are *not* the same list:
        //
        //   the chat half     one alias per map -- the first in the table, the one to type -- comma-separated
        //                     and wrapped, then the reference's fixed "Use: /tp <name>, e.g. /tp lakeside
        //                     county";
        //   the log table     **every alias**, one row each, header counting aliases. `%-15s` for the alias
        //                     and `%3d` for the map id, because a format that drifts is a format nobody can
        //                     compare.
        //
        // Collapsing the table to one row per map (a mistake this test used to pin) is exactly what made
        // `/maps` stop naming the aliases a tester has to type: map 146 has two, and both are answers.
        let answer = maps_answer(&[test_map()]);
        assert_eq!(
            answer.table,
            vec![
                "Teleport targets (2 aliases, every one with a map config):".to_string(),
                "  /tp lakeside        -> map 146  Lakeside County".to_string(),
                "  /tp lakeside county -> map 146  Lakeside County".to_string(),
            ],
            "the reference's header (counting aliases) and one row per alias"
        );
        assert_eq!(
            answer.chat,
            vec![
                "Maps: lakeside,".to_string(),
                "Use: /tp <name>, e.g. /tp lakeside county".to_string(),
            ],
            "and its chat half: the first alias of each map, then the reference's own sentence"
        );

        // Two maps: every alias of both, in the reference's table order (148 comes first in its table).
        let second = MapEntry {
            map_id: 148,
            name: "Ascalon City".to_string(),
            ..test_map()
        };
        let both = maps_answer(&[test_map(), second]);
        assert_eq!(
            both.table,
            vec![
                "Teleport targets (4 aliases, every one with a map config):".to_string(),
                "  /tp ascalon         -> map 148  Ascalon City (pre-Searing)".to_string(),
                "  /tp ascalon city    -> map 148  Ascalon City (pre-Searing)".to_string(),
                "  /tp lakeside        -> map 146  Lakeside County".to_string(),
                "  /tp lakeside county -> map 146  Lakeside County".to_string(),
            ],
            "table order, and the alias column is 15 wide"
        );
        assert_eq!(
            both.chat[0], "Maps: ascalon, lakeside,",
            "one chat line, one alias per map, comma-separated"
        );

        // A map with no name and no row in the reference's table has no alias to type; it prints its id
        // instead of inventing one.
        let unnamed = MapEntry {
            map_id: 50_146,
            name: String::new(),
            ..test_map()
        };
        let odd = maps_answer(&[unnamed]);
        assert_eq!(
            odd.table,
            vec![
                "Teleport targets (1 aliases, every one with a map config):".to_string(),
                "  /tp map 50146       -> map 50146  ".to_string(),
            ],
            "an unnamed, uncatalogued map is named by its id"
        );
    }

    #[test]
    fn every_alias_the_reference_accepts_resolves_to_its_map() {
        // The command layer's half of the map catalogue: `s_teleport_targets` has twenty aliases over
        // twelve maps, and `/tp` has to accept all twenty. Matching on the map's *label* accepted eight
        // (`lakeside county`, `the catacombs`, `ashford abbey`, `foibles fair`, `fort ranik`, ...) and
        // answered "unknown map name" for the reference's own short forms.
        assert_eq!(
            travel_targets::TRAVEL_TARGETS.len(),
            20,
            "the reference's table, generated"
        );

        for alias in [
            "lakeside",
            "lakeside county",
            "ascalon",
            "ascalon city",
            "ashford",
            "ashford abbey",
            "catacombs",
            "the catacombs",
            "northlands",
            "the northlands",
            "green hills",
            "barradin",
            "barradin estate",
            "wizards folly",
            "foibles",
            "foibles fair",
            "regent",
            "regent valley",
            "fort ranik",
            "kamadan",
        ] {
            let target = travel_target(alias).unwrap_or_else(|| panic!("/tp {alias} is an alias"));
            assert!(target.hosted, "{alias} names a map this server hosts");
        }

        // The rule is the reference's (`GmCommand_ToAscii`): case-insensitive, runs of spaces collapsed.
        assert_eq!(
            travel_target("  Lakeside   County ").map(|target| target.map_id),
            Some(146)
        );
        // And nothing else resolves. The reference's own comment claims "/tp LakesideCounty both work";
        // its code lower-cases and collapses spaces without splitting camel case, so it does not, and
        // neither does this.
        assert!(travel_target("LakesideCounty").is_none());
        assert!(travel_target("lakeside countyy").is_none());
        assert!(travel_target("").is_none());
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
