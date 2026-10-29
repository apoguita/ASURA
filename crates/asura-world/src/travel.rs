//! Travel: leaving one instance for another.
//!
//! The reference's request/redirect cycle (`docs/02-architecture.md`, "Travel, end to end"):
//! reserve a slot, write a transfer row, send `TRANSFER_GAME_SERVER_INFO`, then
//! `MAP_UPDATE_CURRENT`. [`teleport`] is the `/tp` half; [`load_map_on_demand`] is the
//! documented fallback that loads a map this world was not started with (`docs/03-protocol.md`
//! #5c, measured 0-2 ms per map).
//!
//! The client-driven half of travel - `AUTH_CMSG_REQUEST_GAME_INSTANCE` and the front door - is
//! in `asura-auth`; a world only decides where a player *already here* goes next.

use crate::*;

/// The district number the reference always answers with (`AuthSrv_HandleRequestGameInstance`).
pub const DISTRICT_NUMBER: u32 = 1;

/// Load a catalogue map this world was **not** started with - the `--on-demand` experiment.
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
pub async fn load_map_on_demand(
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
/// A teleport is not a position change - it is a **server transfer**, the same cycle a portal starts.
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
pub async fn teleport(
    stream: &mut TcpStream,
    session: &mut Session,
    world: &World,
    player: &mut Player,
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
        // with the gate off is a log line - the same shape as every other GM answer here.
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
            // `--on-demand`: the destination was not preloaded. Load it here, and measure the cost -
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
            // can only say so - unless `--on-demand` loaded it (handled above).
            eprintln!(
                "world: {} ({}) is not hosted by this world; start it with --maps {} (or --on-demand \
                 to load destinations at travel time)",
                target.label, target.map_id, target.map_id
            );
            return Ok(true);
        }
    };
    if target.map_id == player.instance.map.map_id {
        // `GmCommands.c:198-202`.
        eprintln!("world: You are already here.");
        return Ok(true);
    }

    // The seat on the destination. `reserve` gives up this session's current lease in the same
    // transaction, which is exactly a transfer's accounting: one seat, moved.
    let Some(world_id) = placement::reserve(
        &world.pool,
        world.world_id,
        target.map_id,
        player.session_id,
    )
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
        player.session_id,
        player.account_id,
        player.character,
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
    player.moved = true;
    eprintln!(
        "world: {:?} teleporting: map {} -> map {} '{}', player token {player_token:#010X}, \
         redirect to {public}",
        player.name,
        player.instance.map.map_id,
        target.map_id,
        target.label()
    );
    Ok(true)
}
