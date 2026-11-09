//! The world program, built in the reference server's shape.
//!
//! This crate exists because the reference server (`docs/research/azerothcore-structure.md`, read from
//! its own tree) separates a program's *shell* from its *library*: `apps/worldserver/Main.cpp` only
//! wires configuration, logging and the managers, and everything else lives under `src/server/shared`,
//! `src/server/database` and `src/server/game`. This crate copies that shape for this project, and
//! `asura-world` moves into it cut by cut (`docs/work/re-engineering-plan.md`, phase 4) until the old
//! program is empty and deleted.
//!
//! The directories are the reference's, and each arrives with the cut that fills it:
//!
//! * [`shared`] - sockets, packets, framing and the secrets both programs use (`shared/Network`,
//!   `Packets`, `Secrets` in the reference). Cut 4a fills the socket half.
//! * [`database`] - the pool and the schema check (`database/DatabaseEnv`, `Updater` in the reference).
//! * `game` - one directory per game noun, the simulation and its socket layer inside it. Cut 4b
//!   begins it with sessions.
//! * `scripts` - behaviour against hooks the engine publishes (`game/Scripting/ScriptMgr` in the
//!   reference). Cut 4e begins it.
//!
//! Nothing in this crate decides anything about a game rule: it is the program's shell and its
//! machinery, and the rules arrive with the nouns.

pub mod database;
pub mod game;
pub mod scripts;
pub mod shared;
