//! The messages the login path actually exchanges, as typed values instead of offsets.
//!
//! Every layout here is copied from GWPS `AuthMsg.h` (packed structs) and checked against the size
//! table: a body of the wrong length cannot be constructed through these functions, because
//! [`super::frame::Frame::encode_validated`] refuses it.
//!
//! Only what the login path needs is here. The rest of the 100 auth messages arrive with the code
//! that needs them, for the reason `docs/adr/0007-rust-translation.md` gives: an unused message
//! layout is a fact nobody has verified.
//!
//! **UUIDs travel in the RFC 4122 little-endian variant** — the first three fields are little
//! endian, the clock sequence and node are not (`uuid_enc_le` in GWPS `GmUuid.h`). That is what
//! `uuid::Uuid::to_bytes_le`/`from_bytes_le` do, and it is *not* the same as the canonical byte
//! order: reading it the obvious way swaps the halves of every id, so every login would report a
//! pair that does not resolve. This module keeps UUIDs as raw `[u8; 16]` and leaves the conversion
//! to the caller, so the choice is made once, where the database is read.

use super::frame::Frame;
use super::tables;

/// The status field of an `AUTH_SMSG_REQUEST_RESPONSE`: success (`GM_ERROR_NONE`).
pub const STATUS_OK: u32 = 0;
/// The status field of a refusal: the pair did not resolve (`GM_ERROR_AUTH_ERROR`, GWPS `GmErrors.h`).
pub const STATUS_AUTH_ERROR: u32 = 11;

/// Why a message body could not be read or built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
    /// The frame carries a different header than this message expects.
    UnexpectedHeader { got: u16, expected: u16 },
    /// The body ended early: the reference's layout needs more bytes.
    Truncated { field: &'static str },
    /// A counted string or array was longer than the field that must hold it.
    TooLong {
        field: &'static str,
        capacity: usize,
        got: usize,
    },
}

impl core::fmt::Display for MessageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MessageError::UnexpectedHeader { got, expected } => {
                write!(f, "message has header {got:#06X}, expected {expected:#06X}")
            }
            MessageError::Truncated { field } => write!(f, "body ends inside `{field}`"),
            MessageError::TooLong {
                field,
                capacity,
                got,
            } => write!(f, "`{field}` holds at most {capacity}, got {got}"),
        }
    }
}

impl std::error::Error for MessageError {}

/// A cursor over a message body. Every read moves forward; there is no seeking, so a layout cannot
/// be read in a different order from the one it is written in.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, position: 0 }
    }

    fn take(&mut self, count: usize, field: &'static str) -> Result<&'a [u8], MessageError> {
        let end = self.position + count;
        if end > self.bytes.len() {
            return Err(MessageError::Truncated { field });
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, MessageError> {
        let bytes = self.take(4, field)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn array16(&mut self, count: usize, field: &'static str) -> Result<[u8; 16], MessageError> {
        let bytes = self.take(count, field)?;
        let mut out = [0u8; 16];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    /// A counted UTF-16 string: a `u32` length, then that many `u16` code units, then padding to the
    /// field's capacity. The length is the number of code units, not bytes.
    fn utf16(&mut self, capacity: usize, field: &'static str) -> Result<String, MessageError> {
        let count = self.u32(field)? as usize;
        if count > capacity {
            return Err(MessageError::TooLong {
                field,
                capacity,
                got: count,
            });
        }
        let bytes = self.take(capacity * 2, field)?;
        let units: Vec<u16> = (0..count)
            .map(|index| u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]))
            .collect();
        Ok(char::decode_utf16(units)
            .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect())
    }
}

/// A message body under construction.
#[derive(Debug)]
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Writer { bytes: Vec::new() }
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn array16(&mut self, value: [u8; 16]) -> &mut Self {
        self.bytes.extend_from_slice(&value);
        self
    }

    /// A counted UTF-16 string padded to `capacity` code units, which is what the fixed-size layout
    /// requires: the field always occupies its full width whatever the name's length.
    fn utf16(
        &mut self,
        text: &str,
        capacity: usize,
        field: &'static str,
    ) -> Result<&mut Self, MessageError> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.len() > capacity {
            return Err(MessageError::TooLong {
                field,
                capacity,
                got: units.len(),
            });
        }
        self.u32(units.len() as u32);
        for unit in &units {
            self.bytes.extend_from_slice(&unit.to_le_bytes());
        }
        self.bytes
            .resize(self.bytes.len() + (capacity - units.len()) * 2, 0);
        Ok(self)
    }

    fn into_body(self) -> Vec<u8> {
        self.bytes
    }
}

fn expect(frame: &Frame, expected: u16) -> Result<(), MessageError> {
    if frame.header != expected {
        return Err(MessageError::UnexpectedHeader {
            got: frame.header,
            expected,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------- client to server

/// `AUTH_CMSG_PORTAL_ACCOUNT_LOGIN` (`0x8038`): the client presents its identity pair.
///
/// Layout (`AuthSrv_PortalAccountLogin`): `req_id`, `user_id[16]`, `session_id[16]`, a counted
/// character name of up to 20 UTF-16 units, and a counted `type` string of up to 32.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalAccountLogin {
    /// Echoed back in every reply to this request.
    pub request: u32,
    /// The portal user id, in the little-endian UUID variant (see the module documentation).
    pub user_id: [u8; 16],
    /// The session id, in the little-endian UUID variant.
    pub session_id: [u8; 16],
    /// The character the client last played, empty on a first login. Informational: the pair is what
    /// authorises, and the client picks a character later.
    pub character_name: String,
    /// A client-side description of the account type. Unused; carried so the trace matches.
    pub kind: String,
}

impl PortalAccountLogin {
    /// Read the message out of a received frame.
    pub fn decode(frame: &Frame) -> Result<Self, MessageError> {
        expect(frame, tables::AUTH_CMSG_PORTAL_ACCOUNT_LOGIN)?;
        let mut reader = Reader::new(&frame.body);
        let request = reader.u32("req_id")?;
        let user_id = reader.array16(16, "user_id")?;
        let session_id = reader.array16(16, "session_id")?;
        let character_name = reader.utf16(20, "charname")?;
        let kind = reader.utf16(32, "type")?;
        Ok(PortalAccountLogin {
            request,
            user_id,
            session_id,
            character_name,
            kind,
        })
    }

    /// Build the message, which is what the reference client in [`super::peer`] does.
    pub fn frame(&self) -> Result<Frame, MessageError> {
        let mut writer = Writer::new();
        writer
            .u32(self.request)
            .array16(self.user_id)
            .array16(self.session_id)
            .utf16(&self.character_name, 20, "charname")?
            .utf16(&self.kind, 32, "type")?;
        Ok(Frame::new(
            tables::AUTH_CMSG_PORTAL_ACCOUNT_LOGIN,
            writer.into_body(),
        ))
    }
}

// ---------------------------------------------------------------- server to client

/// `AUTH_SMSG_REQUEST_RESPONSE` (`0x0003`): the answer to any request, success or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestResponse {
    pub request: u32,
    pub status: u32,
}

impl RequestResponse {
    pub fn frame(&self) -> Frame {
        let mut writer = Writer::new();
        writer.u32(self.request).u32(self.status);
        Frame::new(tables::AUTH_SMSG_REQUEST_RESPONSE, writer.into_body())
    }

    pub fn decode(frame: &Frame) -> Result<Self, MessageError> {
        expect(frame, tables::AUTH_SMSG_REQUEST_RESPONSE)?;
        let mut reader = Reader::new(&frame.body);
        Ok(RequestResponse {
            request: reader.u32("req_id")?,
            status: reader.u32("status")?,
        })
    }

    /// Whether the server accepted the request.
    pub fn is_ok(&self) -> bool {
        self.status == STATUS_OK
    }
}

/// `AUTH_SMSG_CHARACTER_INFO` (`0x0007`): one character in the list, sent once per character.
///
/// Layout (`AuthSrv_CharacterInfo`): `req_id`, `uuid[16]`, `unk0`, a counted name of up to 20 UTF-16
/// units, a counted 64-byte `extended` block (character settings).
pub fn character_info(
    request: u32,
    character: [u8; 16],
    name: &str,
) -> Result<Frame, MessageError> {
    let mut writer = Writer::new();
    writer.u32(request).array16(character).u32(0); // unk0: the reference leaves it zero
    writer.utf16(name, 20, "name")?;
    writer.u32(0); // n_extended: no settings yet; the reference sends its character-settings block
    writer.bytes.resize(writer.bytes.len() + 64, 0);
    Ok(Frame::new(
        tables::AUTH_SMSG_CHARACTER_INFO,
        writer.into_body(),
    ))
}

/// `AUTH_SMSG_FRIEND_STREAM_END` (`0x0014`): "the friend list has finished". GWPS sends the request id
/// in both fields, and the client needs it before the login completes.
pub fn friend_stream_end(request: u32) -> Frame {
    let mut writer = Writer::new();
    writer.u32(request).u32(request);
    Frame::new(tables::AUTH_SMSG_FRIEND_STREAM_END, writer.into_body())
}

/// `AUTH_SMSG_SESSION_INFO` (`0x0001`): the server salt. GWPS sends this in answer to the *account*
/// login, not the portal one; both fields are echoed into the log for the trace.
pub fn session_info(server_salt: u32, unknown: u32) -> Frame {
    let mut writer = Writer::new();
    writer.u32(server_salt).u32(unknown);
    Frame::new(tables::AUTH_SMSG_SESSION_INFO, writer.into_body())
}

/// `AUTH_SMSG_ACCOUNT_SETTINGS` (`0x0016`): the client's stored settings, as a fixed blob.
///
/// Layout (`AuthSrv_AccountSettings`): `req_id`, `n_data`, then 2048 bytes of data of which GWPS fills
/// the first 591. The blob is copied from the reference's own literal
/// ([`super::login_fixtures::ACCOUNT_SETTINGS_DATA`]) because nothing here knows what most of it
/// means — it is what the reference sends, which is the only reason a client will accept it.
pub fn account_settings(request: u32) -> Frame {
    let data = super::login_fixtures::ACCOUNT_SETTINGS_DATA;
    let mut writer = Writer::new();
    writer.u32(request).u32(data.len() as u32);
    writer.bytes.extend_from_slice(data);
    writer
        .bytes
        .resize(writer.bytes.len() + (2048 - data.len()), 0);
    Frame::new(tables::AUTH_SMSG_ACCOUNT_SETTINGS, writer.into_body())
}

/// `AUTH_SMSG_ACCOUNT_INFO` (`0x0011`): the account, the selected character, and the account's feature
/// list.
///
/// Layout and values from GWPS `AuthSrv_SendAccountInfo`. The two 8-byte blobs are its literals:
/// `h000E` (`3F 00 00 00 00 00 00 00`) and `h0016` (`80 3F 42 00 83 00 0C 02`, commented in the
/// reference as controlling which campaigns are owned). `features` is the third (`01 00 04 00 57 00
/// 01 00`), whose third `u16` is the account's number of character slots.
///
/// `character` is the session's last selection; the reference sends all zeroes when the account has
/// no characters at all.
pub fn account_info(request: u32, account: [u8; 16], character: Option<[u8; 16]>) -> Frame {
    /// `h000E` in `AuthSrv_SendAccountInfo`.
    const H000E: [u8; 8] = [0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    /// `h0016`: which campaigns the account owns, per the reference's own comment.
    const H0016: [u8; 8] = [0x80, 0x3F, 0x42, 0x00, 0x83, 0x00, 0x0C, 0x02];
    /// `features`: the third `u16` is the number of character slots (the reference notes that two are
    /// unaccounted for).
    const FEATURES: [u8; 8] = [0x01, 0x00, 0x04, 0x00, 0x57, 0x00, 0x01, 0x00];

    let mut writer = Writer::new();
    writer.u32(request);
    writer.u32(0); // h0006
    writer.u32(4); // h000A
    writer.bytes.extend_from_slice(&H000E);
    writer.bytes.extend_from_slice(&H0016);
    writer.array16(account);
    writer.array16(character.unwrap_or([0u8; 16]));
    writer.u32(8); // h003E
                   // `unk4` is an ARRAY_8 of 200 slots: a four-byte count, then the whole field whatever the count
                   // says. The reference fills eight of them and leaves the rest zero, and the message size depends
                   // on the field's capacity, not on the count.
    writer.u32(FEATURES.len() as u32); // n_unk4
    writer.bytes.extend_from_slice(&FEATURES);
    writer
        .bytes
        .resize(writer.bytes.len() + (200 - FEATURES.len()), 0);
    writer.bytes.push(24); // eula_accepted
    writer.u32(3); // unk5
    Frame::new(tables::AUTH_SMSG_ACCOUNT_INFO, writer.into_body())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_this_module_builds_is_the_size_the_table_requires() {
        // The table is the authority on size; a body built here that disagrees cannot be sent.
        let login = PortalAccountLogin {
            request: 7,
            user_id: [0x11; 16],
            session_id: [0x22; 16],
            character_name: "Asura".to_string(),
            kind: "Gw1".to_string(),
        };
        let frame = login.frame().expect("builds");
        assert_eq!(frame.len(), 150);
        assert!(frame.size_is_expected());

        let response = RequestResponse {
            request: 7,
            status: STATUS_OK,
        };
        assert_eq!(response.frame().len(), 10);

        let character = character_info(7, [0x33; 16], "Asura").expect("builds");
        assert_eq!(character.len(), 138);

        // The two the batch carries in the middle: 2058 and 275 bytes, per the reference's layouts.
        let settings = account_settings(7);
        assert_eq!(settings.len(), 2058);
        let info = account_info(7, [0x44; 16], Some([0x55; 16]));
        assert_eq!(info.len(), 275);

        assert_eq!(friend_stream_end(7).len(), 10);
        assert_eq!(session_info(1, 0).len(), 10);

        for frame in [
            frame,
            response.frame(),
            character,
            settings,
            info,
            friend_stream_end(7),
            session_info(1, 0),
        ] {
            frame
                .encode_validated()
                .unwrap_or_else(|err| panic!("header {:#06X}: {err}", frame.header));
        }
    }

    #[test]
    fn the_account_settings_carry_the_reference_blob_and_no_more() {
        // The data length is part of the message: the client reads `n_data` bytes of a 2048-byte
        // field, so both the blob and the count have to match the reference.
        let frame = account_settings(0x1234);
        let body = &frame.body;
        assert_eq!(
            u32::from_le_bytes([body[0], body[1], body[2], body[3]]),
            0x1234
        );
        let declared = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        assert_eq!(
            declared,
            super::super::login_fixtures::ACCOUNT_SETTINGS_DATA.len()
        );
        assert_eq!(declared, 591, "the reference's blob is 591 bytes");
        assert_eq!(
            &body[8..8 + declared],
            super::super::login_fixtures::ACCOUNT_SETTINGS_DATA,
            "the blob is copied verbatim"
        );
        assert!(
            body[8 + declared..].iter().all(|byte| *byte == 0),
            "the rest of the 2048-byte field is zero, as the reference leaves it"
        );
    }

    #[test]
    fn the_account_info_carries_the_account_and_the_selected_character() {
        let account = [0xAAu8; 16];
        let character = [0xBBu8; 16];
        let frame = account_info(9, account, Some(character));

        assert_eq!(
            u32::from_le_bytes(frame.body[0..4].try_into().expect("4")),
            9
        );
        // req_id, h0006, h000A, then h000E and h0016 (8 each), then the two UUIDs.
        assert_eq!(&frame.body[12..20], &[0x3F, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &frame.body[20..28],
            &[0x80, 0x3F, 0x42, 0x00, 0x83, 0x00, 0x0C, 0x02]
        );
        assert_eq!(&frame.body[28..44], &account);
        assert_eq!(&frame.body[44..60], &character);

        // With no character selected the reference sends zeroes rather than omitting the field.
        let empty = account_info(9, account, None);
        assert_eq!(empty.len(), frame.len());
        assert!(empty.body[44..60].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_login_message_round_trips_through_the_wire() {
        let login = PortalAccountLogin {
            request: 0xDEAD_BEEF,
            user_id: core::array::from_fn(|index| index as u8),
            session_id: core::array::from_fn(|index| (index as u8).wrapping_mul(3)),
            character_name: "Presearing Hero".to_string(),
            kind: "Gw1".to_string(),
        };

        let bytes = login.frame().expect("builds").encode();
        let (frame, used) = Frame::decode(&bytes).expect("frames");
        assert_eq!(used, bytes.len());

        let decoded = PortalAccountLogin::decode(&frame).expect("decodes");
        assert_eq!(decoded, login);
    }

    #[test]
    fn a_name_that_does_not_fit_is_refused_rather_than_truncated() {
        // A truncated name is a character nobody can log in with, and the client's only symptom
        // would be a name it does not recognise.
        let long = "x".repeat(21);
        let mut writer = Writer::new();
        assert_eq!(
            writer.utf16(&long, 20, "charname").map(|_| ()),
            Err(MessageError::TooLong {
                field: "charname",
                capacity: 20,
                got: 21
            })
        );

        let login = PortalAccountLogin {
            request: 1,
            user_id: [0; 16],
            session_id: [0; 16],
            character_name: long,
            kind: String::new(),
        };
        assert!(matches!(
            login.frame(),
            Err(MessageError::TooLong {
                field: "charname",
                ..
            })
        ));
    }

    #[test]
    fn a_counted_string_always_occupies_its_full_capacity() {
        // The size is fixed per header, so a shorter name must be padded, not shortened.
        let short = character_info(0, [0; 16], "A").expect("builds");
        let long = character_info(0, [0; 16], "AAAAAAAAAAAAAAAAAAAA").expect("builds");
        assert_eq!(short.len(), long.len());
        assert_eq!(short.len(), 138);
    }

    #[test]
    fn a_body_that_ends_early_is_refused() {
        let login = PortalAccountLogin {
            request: 1,
            user_id: [0; 16],
            session_id: [0; 16],
            character_name: "A".to_string(),
            kind: String::new(),
        };
        let mut frame = login.frame().expect("builds");
        frame.body.truncate(frame.body.len() - 8);
        assert!(matches!(
            PortalAccountLogin::decode(&frame),
            Err(MessageError::Truncated { .. })
        ));
    }

    #[test]
    fn the_wrong_header_is_refused() {
        let frame = friend_stream_end(1);
        assert_eq!(
            RequestResponse::decode(&frame),
            Err(MessageError::UnexpectedHeader {
                got: tables::AUTH_SMSG_FRIEND_STREAM_END,
                expected: tables::AUTH_SMSG_REQUEST_RESPONSE
            })
        );
    }

    #[test]
    fn a_count_longer_than_its_field_is_refused() {
        // A client can send any count it likes; one larger than the field is a malformed message,
        // not an instruction to read past the body.
        let mut frame = PortalAccountLogin {
            request: 1,
            user_id: [0; 16],
            session_id: [0; 16],
            character_name: "A".to_string(),
            kind: String::new(),
        }
        .frame()
        .expect("builds");

        // The charname count sits after req_id, user_id and session_id.
        frame.body[36..40].copy_from_slice(&21u32.to_le_bytes());
        assert_eq!(
            PortalAccountLogin::decode(&frame),
            Err(MessageError::TooLong {
                field: "charname",
                capacity: 20,
                got: 21
            })
        );
    }

    #[test]
    fn an_empty_character_name_is_legal() {
        // A first login presents no name at all, and that is not an error.
        let login = PortalAccountLogin {
            request: 1,
            user_id: [0; 16],
            session_id: [0; 16],
            character_name: String::new(),
            kind: String::new(),
        };
        let bytes = login.frame().expect("builds").encode();
        let (frame, _) = Frame::decode(&bytes).expect("frames");
        let decoded = PortalAccountLogin::decode(&frame).expect("decodes");
        assert_eq!(decoded.character_name, "");
    }
}
