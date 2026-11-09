//! The world program's entry point: configuration, logging, the listener.
//!
//! The reference's `apps/worldserver/Main.cpp` is this and nothing more: it reads its own configuration
//! file, starts logging, constructs the managers, and hands the socket manager the bind address. The
//! game lives in the library behind it. This `main` keeps that line: it parses this program's
//! arguments, opens the database, checks the schema, and accepts connections; everything a connection
//! then does is `game`'s business and arrives cut by cut.
//!
//! Cut 4a: the program's shell and its socket half. Until cut 4b moves sessions in, this program accepts
//! a connection, reads the version message, and closes it - which proves the shell and the socket, and
//! nothing about a game yet. The old world program keeps serving the client meanwhile.

use std::net::SocketAddr;
use std::process::ExitCode;

use asura_worldserver::database;
use asura_worldserver::shared::socket::{accept_loop, Connection};

/// The bind address when nothing says otherwise: the same default the old program uses, so a test rig
/// does not have to relearn where a world listens.
const DEFAULT_BIND: &str = "127.0.0.1:6113";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("asura-worldd {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    let bind = match argument(&args, "--bind").or_else(|| std::env::var("ASURA_WORLD_BIND").ok()) {
        Some(value) => match value.trim().parse::<SocketAddr>() {
            Ok(address) => address,
            Err(err) => {
                eprintln!("asura-worldd: --bind {value:?} is not an address: {err}");
                return ExitCode::from(2);
            }
        },
        None => DEFAULT_BIND.parse().expect("the default address parses"),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("asura-worldd: no runtime: {err}");
            return ExitCode::from(3);
        }
    };

    runtime.block_on(async move { run(&args, bind).await })
}

async fn run(_args: &[String], bind: SocketAddr) -> ExitCode {
    eprintln!("asura-worldd {} starting", env!("CARGO_PKG_VERSION"));

    let Some(url) = database::database_url() else {
        eprintln!(
            "asura-worldd: no {}. Set it, or copy .env.example to .env and run tools/db-setup.ps1.",
            database::DATABASE_URL_VARIABLE
        );
        return ExitCode::from(2);
    };
    let pool = match database::connect(&url, 4).await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("asura-worldd: the database did not answer: {err}");
            return ExitCode::from(3);
        }
    };
    if let Err(err) = database::ensure_schema_is_current(&pool).await {
        eprintln!("asura-worldd: {err}");
        return ExitCode::from(4);
    }

    let trace = asura_protocol::Trace::from_environment().on();
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("asura-worldd: cannot listen on {bind}: {err}");
            return ExitCode::from(3);
        }
    };
    eprintln!("asura-worldd: listening on {bind}");

    // Cut 4a's handler: read the message a connection opens with, then close. Cut 4b replaces this with
    // a session that claims the transfer and runs the load.
    let outcome = accept_loop(
        listener,
        |mut connection: Connection| {
            Box::pin(async move {
                let version = connection.read_game_version().await?;
                eprintln!(
                    "worldd: {} opened a game connection for world {} map {} (cut 4a: no session yet)",
                    connection.peer(),
                    version.map_token,
                    version.map_id
                );
                Ok(())
            }) as asura_worldserver::shared::socket::ConnectionFuture
        },
        trace,
    )
    .await;

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("asura-worldd: the listener failed: {err}");
            ExitCode::from(3)
        }
    }
}

/// The value of `--name value`, if the program was given it.
fn argument(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter.next().cloned();
        }
    }
    None
}

fn print_help() {
    println!(
        "asura-worldd {} - the world program, built in the reference server's shape",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE:");
    println!("    asura-worldd [--bind ADDRESS]");
    println!();
    println!("OPTIONS:");
    println!("    --bind ADDRESS   listen here (default {DEFAULT_BIND}, or ASURA_WORLD_BIND)");
    println!();
    println!("Cut 4a: this program accepts a connection and reads its version message. Sessions,");
    println!("the simulation and the mechanics arrive with the following cuts; the old world");
    println!("program serves the client meanwhile (docs/work/re-engineering-plan.md, phase 4).");
}
