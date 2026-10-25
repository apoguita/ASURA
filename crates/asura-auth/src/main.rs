//! The auth service.
//!
//! Owns: the login endpoint (key exchange, login, character list, character select), sessions, and
//! reading the registry to answer "where does this client connect?".
//!
//! Does NOT own: placement, character state, instances, the gameplay connection. Those belong to the
//! world servers (`docs/01-scope.md`).
//!
//! AzerothCore parallel: `authserver` — realmlist, session key, and nothing else.
//!
//! Nothing here listens on a socket yet. The startup order below is the part worth having early,
//! because it is the sequence a restart has to follow (`docs/02-architecture.md`, "Startup order").

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("asura-auth {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("asura-auth {} — the ASURA auth service", env!("CARGO_PKG_VERSION"));
        println!();
        println!("Next milestone (M2): bind the login endpoint, read the registry, allocate nothing.");
        return;
    }

    for (step, description) in startup_order().iter().enumerate() {
        eprintln!("startup {}/{}: {}", step + 1, startup_order().len(), description);
    }
    eprintln!("asura-auth: reachable configuration and networking are milestone M2; exiting cleanly");
}

/// The steps this service must complete, in order, before it accepts a client.
pub fn startup_order() -> [&'static str; 4] {
    [
        "open the auth and game database pools; verify the schema version",
        "bind the login endpoint",
        "read the world registry (do not cache it as truth: the table is the truth)",
        "accept logins",
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_order_starts_with_the_database_and_ends_with_players() {
        let steps = super::startup_order();
        assert!(steps[0].contains("schema version"));
        assert_eq!(steps.len(), 4);
        assert!(steps[3].contains("accept"));
    }
}
