//! Message layouts and framing.
//!
//! Two protocol families live here:
//!
//! * the **client protocol** — what the game client speaks. Every constant in this file was observed
//!   in GWPS and is recorded in `docs/03-protocol.md`. It is not ours to design, only to reproduce.
//! * the **internal protocol** — ours, versioned, and small by policy: state and placement go through
//!   the database, not through messages (`docs/02-architecture.md`).
//!
//! AzerothCore parallel: their `Packet`/`WorldPacket` layer with `ByteBuffer` readers. Same shape:
//! explicit little-endian writers and readers, and a fixed layout per header.

/// Frame header for the internal (service-to-service) protocol.
///
/// ```text
/// u32 magic          "ASUR"
/// u16 version        protocol version; a mismatch is fatal, never a warning
/// u16 kind           message kind
/// u32 length         body length
/// u8  body[length]
/// ```
pub const INTERNAL_MAGIC: [u8; 4] = *b"ASUR";
pub const INTERNAL_VERSION: u16 = 1;
pub const INTERNAL_HEADER_LEN: usize = 12;

/// Message kinds this project defines. Deliberately short: placement and state go through the
/// database, so only events that must not wait for a heartbeat belong here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    /// A world server announcing itself: id, address, port, build, capacity.
    WorldHello = 1,
    /// A world server reporting liveness and load.
    Heartbeat = 2,
    /// A world server reporting that an instance is released and back in its pool.
    InstanceReleased = 3,
    /// Ask a world server to flush state and exit.
    Shutdown = 4,
}

impl Kind {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Kind::WorldHello),
            2 => Some(Kind::Heartbeat),
            3 => Some(Kind::InstanceReleased),
            4 => Some(Kind::Shutdown),
            _ => None,
        }
    }
}

/// A framed internal message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: Kind,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    BadMagic,
    VersionMismatch { got: u16, expected: u16 },
    UnknownKind(u16),
    LengthMismatch { declared: usize, actual: usize },
}

impl Frame {
    pub fn new(kind: Kind, body: Vec<u8>) -> Self {
        Frame { kind, body }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(INTERNAL_HEADER_LEN + self.body.len());
        out.extend_from_slice(&INTERNAL_MAGIC);
        out.extend_from_slice(&INTERNAL_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.kind as u16).to_le_bytes());
        out.extend_from_slice(&(self.body.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<(Frame, usize), FrameError> {
        if bytes.len() < INTERNAL_HEADER_LEN {
            return Err(FrameError::TooShort);
        }
        if bytes[0..4] != INTERNAL_MAGIC {
            return Err(FrameError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != INTERNAL_VERSION {
            return Err(FrameError::VersionMismatch {
                got: version,
                expected: INTERNAL_VERSION,
            });
        }
        let kind_raw = u16::from_le_bytes([bytes[6], bytes[7]]);
        let kind = Kind::from_u16(kind_raw).ok_or(FrameError::UnknownKind(kind_raw))?;
        let declared = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if bytes.len() < INTERNAL_HEADER_LEN + declared {
            return Err(FrameError::LengthMismatch {
                declared,
                actual: bytes.len() - INTERNAL_HEADER_LEN,
            });
        }
        let body = bytes[INTERNAL_HEADER_LEN..INTERNAL_HEADER_LEN + declared].to_vec();
        Ok((Frame { kind, body }, INTERNAL_HEADER_LEN + declared))
    }
}

/// Client-facing message headers that ASURA has to reproduce.
///
/// Sources: GWPS `msgdefs.c` and the captures in `PreSearing/captures/`. Sizes are context
/// dependent for `MAP_UPDATE_CURRENT`, which is a trap recorded in `docs/03-protocol.md`.
pub mod client {
    /// Server → client: where to go. `{host[24], port, map_token, region, player_token,
    /// is_explorable, map_id}`. GWPS sends 46 bytes; live captures show 52. Field order matters.
    pub const TRANSFER_GAME_SERVER_INFO: u16 = 0x01A6;
    pub const TRANSFER_GAME_SERVER_INFO_LEN: usize = 46;

    /// Server → client: switch to this map now. `{header, 0, map_id, 0}`.
    pub const MAP_UPDATE_CURRENT: u16 = 0x0099;
    /// In a travel redirect this is 12 bytes.
    pub const MAP_UPDATE_CURRENT_LEN: usize = 12;
    /// In the login batch it must be 5 bytes, or the client fails with error 007.
    pub const MAP_UPDATE_CURRENT_LOGIN_LEN: usize = 5;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trips() {
        let frame = Frame::new(Kind::Heartbeat, vec![1, 2, 3, 4]);
        let bytes = frame.encode();
        assert_eq!(bytes.len(), INTERNAL_HEADER_LEN + 4);
        let (decoded, consumed) = Frame::decode(&bytes).expect("decodes");
        assert_eq!(decoded, frame);
        assert_eq!(consumed, bytes.len());
    }

    #[test]
    fn frame_rejects_a_version_mismatch_loudly() {
        let mut bytes = Frame::new(Kind::Shutdown, vec![]).encode();
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        assert_eq!(
            Frame::decode(&bytes),
            Err(FrameError::VersionMismatch { got: 0xFFFF, expected: INTERNAL_VERSION })
        );
    }

    #[test]
    fn frame_rejects_truncated_bodies() {
        let bytes = Frame::new(Kind::Heartbeat, vec![1, 2, 3]).encode();
        let truncated = &bytes[..bytes.len() - 1];
        assert!(matches!(Frame::decode(truncated), Err(FrameError::LengthMismatch { .. })));
    }

    #[test]
    fn client_constants_match_the_recorded_protocol() {
        assert_eq!(client::TRANSFER_GAME_SERVER_INFO, 0x01A6);
        assert_eq!(client::MAP_UPDATE_CURRENT, 0x0099);
        assert_ne!(
            client::MAP_UPDATE_CURRENT_LEN,
            client::MAP_UPDATE_CURRENT_LOGIN_LEN,
            "the map-update size trap must stay visible"
        );
    }
}
