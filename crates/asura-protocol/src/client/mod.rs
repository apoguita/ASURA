//! The client-facing protocol.
//!
//! The game client speaks this; ASURA does not design it, only reproduces it
//! (`docs/adr/0005-gwps-reference-only.md`). Every constant here is a copied fact from the GWPS
//! reference and is recorded in `docs/03-protocol.md`.
//!
//! The order of events on any connection, and therefore the order of the modules below:
//!
//! 1. [`version`] — the first message is not a normal frame. It is a bare `u32` "version" header
//!    that says what kind of connection this is: auth, game, or control. Get this wrong and every
//!    later decision is wrong.
//! 2. [`handshake`] — the key exchange: the client's seed, the server's seed, and the ARC4 key both
//!    sides derive from them ([`arc4`]). Everything after the server seed is scrambled.
//! 3. [`frame`] — after that, both directions are a `u16 header` and a body whose size the header
//!    alone fixes ([`tables`]). A size disagreement is what produces the client's opaque `007`
//!    error, so it is an error here, not a warning.
//!
//! Names are GWPS's, verbatim, so a Rust message can be compared against a C one by name
//! (`tools/gen-auth-tables.py` generates them from `opcodes.h`).
//!
//! The game (world) family is **not** in this module yet. The auth family (60 client headers, 40
//! server headers) is what M3 needs; the game table is 488 server headers and arrives with the
//! travel milestone, which is when the client first needs it.

pub mod arc4;
pub mod frame;
pub mod handshake;
pub mod tables;
pub mod version;

/// Numbers generated from the reference implementation by `tools/gen-handshake-fixtures.py`. Test
/// data, so it is not compiled into a release build.
#[cfg(test)]
mod handshake_fixtures;

pub use arc4::Arc4;
pub use frame::{Frame, FrameError};
pub use handshake::{ClientSeed, Established, HandshakeError};
pub use tables::Entry;

/// Server → client: where to go. `{host[24], port, map_token, region, player_token,
/// is_explorable, map_id}`. GWPS sends 46 bytes; live captures show 52. Field order matters.
///
/// This is a *game* header, not an auth one, so it has no row in [`tables::AUTH_TABLE`] yet; the
/// two constants below are the recorded evidence that it must not be treated as one size.
pub const TRANSFER_GAME_SERVER_INFO: u16 = 0x01A6;
/// See [`TRANSFER_GAME_SERVER_INFO`]; the size GWPS sends.
pub const TRANSFER_GAME_SERVER_INFO_LEN: usize = 46;

/// Server → client: switch to this map now. `{header, 0, map_id, 0}`.
pub const MAP_UPDATE_CURRENT: u16 = 0x0099;
/// In a travel redirect this is 12 bytes.
pub const MAP_UPDATE_CURRENT_LEN: usize = 12;
/// In the login batch it must be 5 bytes, or the client fails with error 007.
pub const MAP_UPDATE_CURRENT_LOGIN_LEN: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_constants_match_the_recorded_protocol() {
        assert_eq!(TRANSFER_GAME_SERVER_INFO, 0x01A6);
        assert_eq!(MAP_UPDATE_CURRENT, 0x0099);
        assert_ne!(
            MAP_UPDATE_CURRENT_LEN, MAP_UPDATE_CURRENT_LOGIN_LEN,
            "the map-update size trap must stay visible"
        );
    }

    #[test]
    fn the_game_headers_are_not_in_the_auth_table() {
        // If someone adds the game table to the auth lookup, these assertions fail first.
        assert!(tables::expected_size(TRANSFER_GAME_SERVER_INFO).is_none());
        assert!(tables::expected_size(MAP_UPDATE_CURRENT).is_none());
    }
}
