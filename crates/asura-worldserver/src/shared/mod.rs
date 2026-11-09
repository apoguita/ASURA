//! The shared core: what both programs need and neither owns.
//!
//! The reference keeps this in `src/server/shared` - `Network/` (async accept, threads, socket
//! managers), `Packets/`, `Realms/`, `Secrets/`, `DataStores/`. This is the same idea for this project:
//! the world program's socket machinery, the wire types it speaks, and the registry rows it reads.
//!
//! Cut 4a fills the socket half. The rest arrives with the cut that needs it.

pub mod socket;

use asura_protocol::client::Frame;

/// What a socket hands up to a session: one complete frame, or the bytes that name a connection.
///
/// This exists as a type rather than a comment because the two are easy to confuse in code that reads
/// several messages from one buffer: a frame is validated against the table, a raw prefix is not.
pub enum Incoming {
    /// One message, already framed and validated by the packet table.
    Frame(Frame),
    /// Bytes that are not a whole message yet.
    Partial(Vec<u8>),
}
