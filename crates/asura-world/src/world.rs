//! The world: the registry row, the maps it publishes, and everyone standing in it.
//!
//! [`World`] is what a world server registers as and what `serve` hands each connection. [`Peer`]
//! and [`Peers`] are the players in it: the reference's `srv->agents`/`srv->players`, kept here
//! because a connection's tick has to tell everyone else what it is doing.
//!
//! This is the noun the party and the instance both hang off; `docs/02-architecture.md` describes
//! the instance as a per-map update thread with affinity, and that object is still missing
//! (`docs/HANDOFF.md`, the world server's domains).

use crate::*;

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
    /// whenever the two differ - which is the normal case behind a realmlist.
    pub public: std::net::SocketAddr,
    /// The maps' path graphs, imported from `Gw.dat` at startup, and one finder per map. This is the
    /// reference's `srv->map_context`: what a `MOVE_TO_COORD` is answered from.
    pub graphs: PathGraphs,
    /// Maps loaded **after** startup (`--on-demand`): the destination of a travel this world was not
    /// started with. Kept apart from `maps` so the preloaded set - what the world registered for and
    /// what `/maps` lists - stays exactly what the operator chose, while the demand-loaded map is
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
    pub outbox: tokio::sync::mpsc::UnboundedSender<Vec<Frame>>,
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
/// agent (`GmAgent.c:310-331`) - the player, their party size, their party, then the agent itself.
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

impl World {
    /// The map entry for `map_id`, if this world serves it - preloaded at startup, or loaded on
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

/// The client asks who else is here. This is the big block, and its last message
/// (`INSTANCE_LOAD_FINISH`) is what takes the client off the loading screen.
///
/// Everyone already in the map goes after it, in the reference's own order for a joining client
/// (`GameSrv_SendWorldAgents`, `GmAgent.c:310-331`), and the same frames go out to *them* so they
/// learn about this one. The reference broadcasts the player create there and unicasts the rest;
/// the order is what matters here, and it is the same in both directions.
pub async fn handle_request_players(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let replay = world.replay.as_ref();
    let mut block = match replay {
        Some(replay) if replay.pick.wants(crate::replay::BlockName::Players) => {
            replay.players.clone()
        }
        _ => player_block(
            &player.name,
            player.standing(),
            &player.inventory,
            player.started,
            player.player_id,
            player.agents.player.agent_id,
            player.traits,
        ),
    };
    let others = world.peers.others(player.player_id);
    for peer in &others {
        block.extend(introduction(peer));
    }
    let me = PeerView {
        player_id: player.player_id,
        agent_id: player.agents.player.agent_id,
        name: player.name.clone(),
        standing: player.standing(),
        equipped: player.inventory.equipped_item_ids(),
    };
    if !others.is_empty() {
        // The introduction, then what the others need in order to draw this player:
        // `GameSrv_BroadcastPlayerEquippedItems` (`GmItem.c:129-160`, the *unnamed* item form),
        // then `GameSrv_BroadcastUpdateAgentVisualEquipment` and `GameSrv_BroadcastAgentDisplayCape`
        // - the reference's own order in the same handler (`GameSrv.c:1643-1650`).
        let mut mine = introduction(&me);
        let equipped_bag = player
            .inventory
            .bags
            .iter()
            .find(|bag| bag.bag_type == 2)
            .map(|bag| bag.bag_id);
        for item in player
            .inventory
            .items
            .iter()
            .filter(|item| item.bag_id == equipped_bag)
        {
            mine.push(game::create_unnamed_item(&named_item(item)));
        }
        mine.push(equipment_frame(player));
        mine.push(game::agent_display_cape(player.agents.player.agent_id, 1));
        world.peers.broadcast(player.player_id, &mine);
    }
    send(stream, session, block, trace).await?;
    Ok(true)
}
