//! A world server — AzerothCore calls this a *realm*.
//!
//! Owns: instance lifecycle (prebuilt pools), placement (reserving a destination slot in
//! `world_pool`), character persistence, and the gameplay connection from the client.
//!
//! The instance-creation policy is the project's one open architectural question
//! (`docs/adr/0005-gwps-reference-only.md`, milestone M6.5): prebuilt pools are the default, and
//! on-demand creation is an experiment to be run here, against the real client, not argued about.
//!
//! Nothing here listens on a socket yet.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("asura-world {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "asura-world {} — one ASURA world server (a realm)",
            env!("CARGO_PKG_VERSION")
        );
        println!();
        println!(
            "Next milestone (M2): register in `world`, publish pool capacity, reserve on travel."
        );
        return;
    }

    for (step, description) in startup_order().iter().enumerate() {
        eprintln!(
            "startup {}/{}: {}",
            step + 1,
            startup_order().len(),
            description
        );
    }
    eprintln!("asura-world: instance pools and networking are milestone M2; exiting cleanly");
}

/// The steps this service must complete, in order, before it may be advertised as a candidate for
/// placement. A world that has not finished these is simply not in the registry yet, which is what
/// makes a request arriving two seconds after boot get a correct "no room" instead of a race.
pub fn startup_order() -> [&'static str; 5] {
    [
        "open the game database pool; verify the schema version",
        "load map configuration and static map data",
        "prebuild outpost instances, one per map per district",
        "prebuild explorable pools",
        "register in `world`, publish `world_pool` capacity, then accept connections",
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_world_registers_last() {
        let steps = super::startup_order();
        assert!(steps.last().unwrap().contains("register"));
        assert!(steps[0].contains("schema version"));
    }
}
