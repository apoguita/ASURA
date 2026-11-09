//! Message layouts and framing for the client protocol.
//!
//! This crate holds what the game client speaks. Every constant in it was observed in GWPS and is
//! recorded in `docs/reference/client-protocol.md`. It is not ours to design, only to reproduce.
//!
//! An internal service-to-service protocol was defined here once - magic `ASUR`, a 12-byte header
//! and four message kinds - and nothing outside this crate's tests ever called it; state and
//! placement go through the database, not through messages (`docs/guidance/architecture.md`). It was
//! removed on 2026-11-08 rather than left as a second, unused path through the wire layer
//! (`docs/work/layer-rework-checklist.md`, layer 1 audit).
//!
//! AzerothCore parallel: their `Packet`/`WorldPacket` layer with `ByteBuffer` readers. Same shape:
//! explicit little-endian writers and readers, and a fixed layout per header.

/// The client-facing protocol: connection classification, framing, and the auth message table.
///
/// See the module documentation for the order a connection follows and for what is deliberately
/// not here yet (the game/world family).
pub mod client;

/// The game protocol: what a world server sends a client that has just been placed in a map, and the
/// table of client messages it must read. Separate from [`client`] because the two families collide on
/// header values: `0x8000 | n` names a different message in each.
pub mod game;

/// **Every packet, one table.** Both protocol families, one row per header, with the fact columns
/// (header, index, size, name, layout) and the two columns a session maintains (status, evidence).
/// The single source for what a message is: nothing else in the tree lists a header.
pub mod packets;

/// The per-message trace switch both services read (`ASURA_TRACE`).
pub mod trace;

pub use trace::Trace;
