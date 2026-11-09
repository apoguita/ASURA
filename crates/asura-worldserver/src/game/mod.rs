//! The game library: one directory per game noun, and the socket layer that serves them.
//!
//! Copied from the reference's `src/server/game` arrangement (see `docs/research/azerothcore-structure.md`)
//! and filled by moving `asura-world` into it cut by cut (`docs/work/re-engineering-plan.md`, phase 4).
//! The directories are the reference's where it has an equivalent; `content` and `support` are this
//! project's own because the reference keeps content in SQL and the archive reader has no peer there.
//!
//! The crate root re-exports every module below, so a file that names `crate::player` keeps working
//! while the tree is grouped: the grouping is the map, the alias is the road.
pub mod content;
pub mod entities;
pub mod maps;
pub mod mechanics;
pub mod server;
pub mod support;
pub mod world;
