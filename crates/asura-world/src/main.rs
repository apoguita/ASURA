//! A world server — AzerothCore calls this a *realm*.
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
use asura_world::{serve, MapEntry, World};

/// How often the world tells the database it is alive. Placement stops offering a world whose
/// heartbeat is older than a minute, so this is comfortably inside that window.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

const DEFAULT_WORLD_ID: i32 = 8001;
const DEFAULT_BIND: &str = "127.0.0.1:6113";
const DEFAULT_CAPACITY: i32 = 8;
/// The map a fresh development database starts in: the reference rig's outpost.
const DEFAULT_MAP: i32 = 146;

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

    let maps_arg: Vec<i32> = match argument(&args, "--maps") {
        Some(value) => value
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .filter_map(|part| match part.trim().parse::<i32>() {
                Ok(map_id) => Some(map_id),
                Err(_) => {
                    eprintln!("asura-world: --maps {value:?} is not a comma-separated list of ids");
                    None
                }
            })
            .collect(),
        None => vec![DEFAULT_MAP],
    };
    if maps_arg.is_empty() {
        eprintln!("asura-world: --maps named no map");
        return ExitCode::from(2);
    }

    let trace = Trace::from_environment();

    eprintln!("asura-world {} starting", env!("CARGO_PKG_VERSION"));
    eprintln!("asura-world: database ready, connecting");

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
    let mut maps = Vec::new();
    for map_id in &maps_arg {
        match db_game::spawn_map(&pool, *map_id).await {
            Ok(Some(configured)) => {
                let map_file_id = u32::try_from(configured.map_file_id).unwrap_or(0);
                if map_file_id == 0 {
                    eprintln!(
                        "asura-world: map {map_id} has no map_file_id; a client cannot load it"
                    );
                    return ExitCode::from(4);
                }
                if configured.map_type != 0 {
                    // Explorable areas are a later milestone: an outpost is where a client lands,
                    // and claiming to host an explorable would put a player in a map this server
                    // cannot spawn anything into.
                    eprintln!(
                        "asura-world: map {map_id} is an explorable area (type {}); only outposts \
                         are hosted so far, skipping it",
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
    let capacities: Vec<(i32, i32)> = maps.iter().map(|map| (map.map_id, capacity)).collect();
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
        "asura-world: registered world {world_id} at {public} for map(s) {}, {capacity} slot(s) each",
        maps.iter()
            .map(|map| map.map_id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
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

    let world = Arc::new(World {
        pool,
        world_id,
        maps,
        replay,
    });
    if let Err(err) = serve(listener, world, trace.on()).await {
        eprintln!("asura-world: the listener stopped: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
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
    println!("    --maps LIST       comma-separated map ids to host (default {DEFAULT_MAP})");
    println!("    --capacity N      slots per map (default {DEFAULT_CAPACITY})");
    println!("    -V, --version     print the version and exit");
    println!("    -h, --help        print this and exit");
    println!();
    println!("ENVIRONMENT:");
    println!("    ASURA_DATABASE_URL    the PostgreSQL URL (falls back to the repository .env)");
    println!("    ASURA_TRACE=1         log every message: direction, header, size");
}
