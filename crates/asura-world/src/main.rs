//! A world server ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â AzerothCore calls this a *realm*.
//!
//! Owns: the gameplay connection to the client, the instances it hosts, and (from M5) character
//! persistence. The auth service owns placement; this service publishes what it can host and answers
//! the clients that are sent to it.
//!
//! The startup order is enforced here rather than described (`docs/02-architecture.md`), and it is
//! the sequence a restart has to follow:
//!
//! 1. read the configuration: world id, bind address, the address to advertise, and the maps;
//! 2. open the database pool;
//! 3. **refuse to run against a database older than this build**;
//! 4. load each map's config (a map without a `map_file_id` cannot be loaded by a client);
//! 5. register in `world` and publish pool capacity, start the heartbeat, then accept connections.
//!
//! Step 3 first, and step 5 last: a world that is in the registry is a world that is ready. That is
//! what makes a travel arriving two seconds after boot get a correct answer instead of a race.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use asura_db::migrations::{self, EnsureError};
use asura_db::{game as db_game, placement};
use asura_protocol::Trace;
use asura_world::{
    serve, MapEntry, World, MAP_TYPE_CHARACTER_CREATION, MAP_TYPE_MAIN_EXPLORABLE,
    MAP_TYPE_MAIN_TOWN,
};

/// How often the world tells the database it is alive. Placement stops offering a world whose
/// heartbeat is older than a minute, so this is comfortably inside that window.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

const DEFAULT_WORLD_ID: i32 = 8001;
const DEFAULT_BIND: &str = "127.0.0.1:6113";
const DEFAULT_CAPACITY: i32 = 8;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("asura-world {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    let world_id = match argument(&args, "--world") {
        Some(value) => match value.parse::<i32>() {
            Ok(id) => id,
            Err(err) => {
                eprintln!("asura-world: --world {value:?} is not an id: {err}");
                return ExitCode::from(2);
            }
        },
        None => DEFAULT_WORLD_ID,
    };

    let bind = match argument(&args, "--bind").or_else(|| std::env::var("ASURA_WORLD_BIND").ok()) {
        Some(value) => match value.trim().parse::<SocketAddr>() {
            Ok(address) => address,
            Err(err) => {
                eprintln!("asura-world: --bind {value:?} is not an address: {err}");
                return ExitCode::from(2);
            }
        },
        None => DEFAULT_BIND.parse().expect("the default address parses"),
    };

    // The address a *client* dials. It is separate from `--bind` on purpose: a server behind a NAT
    // listens on a private address and must publish the public one, exactly as a realmlist does.
    let public: SocketAddr =
        match argument(&args, "--public").or_else(|| std::env::var("ASURA_WORLD_PUBLIC").ok()) {
            Some(value) => match value.trim().parse::<SocketAddr>() {
                Ok(address) => address,
                Err(err) => {
                    eprintln!("asura-world: --public {value:?} is not an address: {err}");
                    return ExitCode::from(2);
                }
            },
            None => bind,
        };

    let capacity = argument(&args, "--capacity")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(DEFAULT_CAPACITY);

    // `--maps` names a subset; without it the world hosts **every** map the catalogue has, which is what
    // the reference does (one server per map, all of them pre-spawned at startup). Pinning this to a single
    // map made `/maps` show one alias and `/tp` answer "not hosted" for the other eleven -- a map list is a
    // deployment detail, not something a player should have to know.
    let maps_arg: Option<Vec<i32>> = argument(&args, "--maps").map(|value| {
        value
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .filter_map(|part| match part.trim().parse::<i32>() {
                Ok(map_id) => Some(map_id),
                Err(_) => {
                    eprintln!("asura-world: --maps {value:?} is not a comma-separated list of ids");
                    None
                }
            })
            .collect::<Vec<i32>>()
    });
    if maps_arg.as_ref().is_some_and(|maps| maps.is_empty()) {
        eprintln!("asura-world: --maps named no map");
        return ExitCode::from(2);
    }

    let trace = Trace::from_environment();

    eprintln!("asura-world {} starting", env!("CARGO_PKG_VERSION"));
    eprintln!("asura-world: database ready, connecting");

    // Said once, and never gated. Chat commands answer **here**, in this console, which is the half that
    // has always worked: the reference's `/maps` printed its table to its own log, and no chat text has
    // ever reached a client's screen in either server. Command answers are not sent -- sending them crashed
    // a live client -- while a line a player *speaks* is sent, as the reference sends it.
    eprintln!(
        "asura-world: /maps, /stuck and unknown commands answer in this console; a spoken chat line is sent"
    );

    let Some(database_url) = asura_db::config::database_url() else {
        eprintln!(
            "asura-world: no {}. Set it, or copy .env.example to .env and run tools/db-setup.ps1.",
            asura_db::config::DATABASE_URL_VARIABLE
        );
        return ExitCode::from(2);
    };
    let pool = match placement::connect(&database_url, 8).await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("asura-world: could not connect to the database: {err}");
            return ExitCode::FAILURE;
        }
    };

    match migrations::ensure_current(&pool).await {
        Ok(()) => {}
        Err(EnsureError::TooOld(too_old)) => {
            eprintln!("asura-world: {too_old}");
            eprintln!("asura-world: refusing to start against a stale schema");
            return ExitCode::from(3);
        }
        Err(EnsureError::Database(err)) => {
            eprintln!("asura-world: could not read the schema version: {err}");
            return ExitCode::FAILURE;
        }
    }

    // The maps, with what a client needs to load them. A map named on the command line that the
    // database does not describe is a map a client would load forever, so it is fatal here.
    let map_ids = match maps_arg {
        Some(explicit) => explicit,
        None => match db_game::configured_map_ids(&pool).await {
            Ok(ids) if !ids.is_empty() => {
                eprintln!(
                    "asura-world: no --maps given, so every map in the catalogue is hosted ({} of them, \
                     the reference's own list)",
                    ids.len()
                );
                ids
            }
            Ok(_) => {
                eprintln!(
                    "asura-world: map_config is empty, so there is no map to host; run \
                     tools/seed-map.ps1 to write the reference's catalogue"
                );
                return ExitCode::from(4);
            }
            Err(err) => {
                eprintln!("asura-world: could not read the map catalogue: {err}");
                return ExitCode::FAILURE;
            }
        },
    };

    let mut maps = Vec::new();
    for map_id in &map_ids {
        match db_game::spawn_map(&pool, *map_id).await {
            Ok(Some(configured)) => {
                let map_file_id = u32::try_from(configured.map_file_id).unwrap_or(0);
                if map_file_id == 0 {
                    eprintln!(
                        "asura-world: map {map_id} has no map_file_id; a client cannot load it"
                    );
                    return ExitCode::from(4);
                }
                if !matches!(
                    configured.map_type,
                    MAP_TYPE_MAIN_TOWN | MAP_TYPE_MAIN_EXPLORABLE | MAP_TYPE_CHARACTER_CREATION
                ) {
                    // A world hosts what the reference hosts: main towns (0), explorable areas (1) and the
                    // character-creation instance (11). Explorable hosting used to be refused here on the
                    // theory that this server "cannot spawn anything into one" — but a world server does not
                    // spawn content, it hands the client the blob and the spawn point, which is exactly what
                    // the reference does with Lakeside County (146), its verified explorable. Refusing it made
                    // this server unable to host the reference's flagship map.
                    eprintln!(
                        "asura-world: map {map_id} is type {} (main towns, explorable areas and the \
                         character-creation instance are hosted), skipping it",
                        configured.map_type
                    );
                    continue;
                }
                maps.push(MapEntry {
                    map_id: *map_id,
                    map_file_id,
                    map_type: configured.map_type,
                    spawn: (configured.spawn_x, configured.spawn_y),
                    spawn_plane: configured.spawn_plane as u16,
                    name: configured.name.clone(),
                    capacity,
                });
            }
            Ok(None) => {
                eprintln!(
                    "asura-world: map {map_id} is not in map_config; run tools/seed-map.ps1 to add it"
                );
                return ExitCode::from(4);
            }
            Err(err) => {
                eprintln!("asura-world: could not read map {map_id}: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    if maps.is_empty() {
        eprintln!(
            "asura-world: no map can be hosted; refusing to register a world that cannot serve"
        );
        return ExitCode::from(4);
    }

    // ---- step 5: register, publish, heartbeat, then accept -----------------------------------
    // `--on-demand`: load a catalogue map this world was not started with, instead of refusing it —
    // for a `/tp` into it, and for a login whose character stands in it (both are the same refusal
    // when this is off). Off by default, because preloading every destination is what the reference
    // does (one pre-spawned server per map) and what this world does at startup; the switch exists
    // to measure whether the client actually needs the destination ready before the hop.
    let load_on_demand = args.iter().any(|arg| arg == "--on-demand")
        || matches!(
            std::env::var("ASURA_WORLD_ON_DEMAND").ok().as_deref(),
            Some("1" | "true" | "yes" | "on")
        );
    // The pool rows this world publishes. Normally one per map it preloads; with `--on-demand`, one
    // per catalogue map, because placement asks "who publishes this map?" and an on-demand world can
    // serve any of them — a login into a map it did not preload is loaded there and then, not refused.
    let capacities: Vec<(i32, i32)> = if load_on_demand {
        match db_game::configured_map_ids(&pool).await {
            Ok(ids) if !ids.is_empty() => ids.iter().map(|map_id| (*map_id, capacity)).collect(),
            _ => maps.iter().map(|map| (map.map_id, capacity)).collect(),
        }
    } else {
        maps.iter().map(|map| (map.map_id, capacity)).collect()
    };
    let build = format!("asura-world/{}", env!("CARGO_PKG_VERSION"));
    if let Err(err) = placement::register_world(
        &pool,
        world_id,
        "asura-dev",
        &public.ip().to_string(),
        i32::from(public.port()),
        &build,
        &capacities,
    )
    .await
    {
        eprintln!("asura-world: could not register the world: {err}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "asura-world: registered world {world_id} at {public} for map(s) {} ({} slot(s) each){}",
        maps.iter()
            .map(|map| map.map_id.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        capacity,
        if load_on_demand {
            format!(
                "; on-demand serves any of the {} catalogue map(s)",
                capacities.len()
            )
        } else {
            String::new()
        }
    );

    // The heartbeat. A world that stops sending it stops being offered, which is the point: a dead
    // world that still accepts TCP connections is worse than one that is plainly gone.
    let heartbeat_pool = pool.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            ticker.tick().await;
            if let Err(err) = placement::heartbeat(&heartbeat_pool, world_id, 0).await {
                eprintln!("asura-world: heartbeat failed: {err}");
            }
        }
    });

    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("asura-world: could not bind {bind}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let replay = match asura_world::replay::Replay::from_environment() {
        Ok(replay) => replay,
        Err(err) => {
            eprintln!("asura-world: could not read the replay capture: {err}");
            return ExitCode::from(5);
        }
    };
    if let Some(replay) = &replay {
        eprintln!(
            "asura-world: replaying the reference's captured stream ({} / {} / {} / {} messages)",
            replay.load.len(),
            replay.items.len(),
            replay.spawn.len(),
            replay.players.len()
        );
    }

    // The path graphs, which is what a `MOVE_TO_COORD` is answered from (`GameSrv_LoadMap` reads the same
    // blob). A map whose blob cannot be imported is served without one, and every walk in it is refused.
    let archive = archive_path(&args);
    let graphs = match asura_world::PathGraphs::load(&archive, &maps) {
        Ok(graphs) => {
            let with_graphs = graphs.maps();
            eprintln!(
                "asura-world: path graphs from {archive}: {} of {} map(s) ({})",
                with_graphs.len(),
                maps.len(),
                with_graphs
                    .iter()
                    .map(|map_id| map_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            graphs
        }
        Err(err) => {
            eprintln!(
                "asura-world: no path graphs, so every walk will be refused: could not open {archive}: \
                 {err}. Set ASURA_GW_DAT or pass --gwdat."
            );
            asura_world::PathGraphs::empty()
        }
    };

    // `--on-demand` was read above, before the pool rows were published, so that an on-demand world
    // lists every catalogue map as servable while it preloads only the maps it was started with.

    let world = Arc::new(World {
        pool,
        world_id,
        maps,
        replay,
        peers: Default::default(),
        public,
        graphs,
        lazy_maps: Default::default(),
        load_on_demand,
        capacity,
        archive,
    });
    if load_on_demand {
        eprintln!(
            "asura-world: on-demand destination loading is ON: a login or a /tp that names a \
             catalogue map this world was not started with loads it there and then and measures \
             the cost"
        );
    }
    if let Err(err) = serve(listener, world, trace.on()).await {
        eprintln!("asura-world: the listener stopped: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Where the client's archive is, for the path graphs: `--gwdat`, then `ASURA_GW_DAT`, then the two
/// places a local copy lives (the working directory, and `data/Gw.dat` in this repository).
///
/// The archive is the game's own asset file and is not distributed with this repository; copy a
/// `Gw.dat` here (or point `ASURA_GW_DAT` at one) and the world imports every map's graph at startup.
/// GWPS reads the same file beside its own binary (`FaArchive.c`), so this is the reference's rule.
/// A world that cannot find it still serves (a client loads a map from its own copy of the same
/// archive), but it refuses every walk instead of inventing one, and says so at startup.
fn archive_path(args: &[String]) -> String {
    if let Some(given) = argument(args, "--gwdat") {
        return given;
    }
    if let Ok(from_env) = std::env::var("ASURA_GW_DAT") {
        return from_env;
    }
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    for candidate in [
        std::path::PathBuf::from("Gw.dat"),
        repository.join("data").join("Gw.dat"),
    ] {
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    "Gw.dat".to_string()
}

/// The value after `name`, if the flag was given.
fn argument(args: &[String], name: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == name)?;
    args.get(index + 1).cloned()
}

fn print_help() {
    println!(
        "asura-world {} -- one ASURA world server",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("    asura-world [--world ID] [--bind ADDRESS] [--public ADDRESS] [--maps LIST] [--capacity N]");
    println!();
    println!("OPTIONS:");
    println!(
        "    --world ID        the id this world registers under (default {DEFAULT_WORLD_ID})"
    );
    println!("    --bind ADDRESS    listen here (default {DEFAULT_BIND}, or ASURA_WORLD_BIND)");
    println!(
        "    --public ADDRESS  the address to advertise to clients (default: the bind address,"
    );
    println!("                      or ASURA_WORLD_PUBLIC)");
    println!(
        "    --maps LIST       comma-separated map ids, to host a subset instead of the whole"
    );
    println!(
        "                      catalogue (default: every map in map_config -- the reference starts"
    );
    println!("                      a server per configured map, so /maps lists them all and /tp reaches them)");
    println!(
        "    --on-demand       load a catalogue map this world was not started with, instead of"
    );
    println!(
        "                      refusing it: for a /tp into it, or a login whose character stands"
    );
    println!("                      in it. Off by default; the switch is the precache experiment");
    println!("    --capacity N      slots per map (default {DEFAULT_CAPACITY})");
    println!("    --gwdat PATH      the client archive the path graphs come from (default: ASURA_GW_DAT,");
    println!("                      then Gw.dat, then data\\Gw.dat in this repository)");
    println!("    -V, --version     print the version and exit");
    println!("    -h, --help        print this and exit");
    println!();
    println!("ENVIRONMENT:");
    println!("    ASURA_DATABASE_URL    the PostgreSQL URL (falls back to the repository .env)");
    println!("    ASURA_GW_DAT          the client archive, for the path graphs");
    println!("    ASURA_TRACE=1         log every message: direction, header, size");
}
