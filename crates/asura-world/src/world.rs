//! The world: the registry row, the maps it publishes, and everyone standing in it.
//!
//! [`World`] is what a world server registers as and what `serve` hands each connection. Who is
//! standing in which map belongs to the maps themselves: [`Instances`] keeps one running
//! [`Instance`] per hosted map, and that instance owns its players, their agents and its clock
//! (`docs/02-architecture.md`, "Inside a world server").
//!
//! What stays here is the world's own: the registry row, the map catalogue, the path graphs, and
//! the demand-loaded maps.

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
    /// The maps this world is running: one instance per hosted map, created on first use, each
    /// owning its players, their agents and its own clock.
    pub instances: Instances,
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
            player.agent_id,
            player.traits,
        ),
    };
    let others = player.instance.others(player.player_id);
    for peer in &others {
        block.extend(introduction(peer));
    }
    let me = PeerView {
        player_id: player.player_id,
        agent_id: player.agent_id,
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
        mine.push(game::agent_display_cape(player.agent_id, 1));
        player.instance.broadcast(player.player_id, &mine);
    }
    send(stream, session, block, trace).await?;
    Ok(true)
}
