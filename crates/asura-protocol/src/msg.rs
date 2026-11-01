//! The packets this tree names in its own code, as constants rather than bare headers.
//!
//! One constant per message the world and the auth service actually dispatch or build, named exactly
//! as the catalogue names it (`crate::packets`, the `name` column), so a reader of a `match` arm sees
//! the message and not a number. The value is the wire header, direction bit included: client-to-server
//! is `0x8000 | n`, server-to-client is `n`.
//!
//! Provenance of every name: `crates/asura-protocol/src/packets.rs`, where each row carries the
//! evidence for its name beside it. A name that the catalogue does not have yet would not appear here;
//! every constant below resolves to a named row.
//!
//! This module is a vocabulary, not a protocol: it holds numbers that already exist in the catalogue,
//! and adding one changes nothing on the wire. A message whose header is wrong here is wrong there too,
//! which is the point of having one place to read it.

/// `0x8091`, 4 bytes: the client asks for its item stream, which opens the inventory part of a load.
pub const GAME_CMSG_INSTANCE_LOAD_REQUEST_ITEMS: u16 = 0x8091;

/// `0x8088`, 2 bytes: the client asks for the spawn point it stands at.
pub const GAME_CMSG_INSTANCE_LOAD_REQUEST_SPAWN: u16 = 0x8088;

/// `0x8090`, 19 bytes: the client asks for the players in the instance it is entering.
pub const GAME_CMSG_INSTANCE_LOAD_REQUEST_PLAYERS: u16 = 0x8090;

/// `0x800D`, 2 bytes: the client's ping, which a server answers so the latency readout moves.
pub const GAME_CMSG_PING_REQUEST: u16 = 0x800D;

/// `0x803E`, 14 bytes: a click on the ground, as a destination only.
pub const GAME_CMSG_MOVE_TO_COORD: u16 = 0x803E;

/// `0x803D`, 26 bytes: the client's own position report, sent while it walks.
pub const GAME_CMSG_POSITION_UPDATE: u16 = 0x803D;

/// `0x8028`, 2 bytes: the client's movement was cancelled.
pub const GAME_CMSG_CANCEL_MOVEMENT: u16 = 0x8028;

/// `0x8047`, 14 bytes: the last position before a movement cancel, which is where the walk stopped.
pub const GAME_CMSG_LAST_POS_BEFORE_MOVE_CANCELED: u16 = 0x8047;

/// `0x80C1`, 10 bytes: the client touched a travel object - a portal of the instance it stands in.
pub const GAME_CMSG_TRAVEL_INIT: u16 = 0x80C1;

/// `0x80B1`, 9 bytes: the client chose an instance on the world map.
pub const GAME_CMSG_PARTY_TRAVEL: u16 = 0x80B1;

/// `0x8064`, a counted string: a chat line the player typed.
pub const GAME_CMSG_SEND_CHAT_MESSAGE: u16 = 0x8064;

/// `0x8008`, 2 bytes: the client is going away.
pub const GAME_CMSG_DISCONNECT: u16 = 0x8008;

/// `0x8089`, 2 bytes: the character-creation screen asks for a character to model.
pub const GAME_CMSG_CHAR_CREATION_REQUEST_PLAYER: u16 = 0x8089;

/// `0x808A`, 2 bytes: the character-creation screen asks for the starting armours.
pub const GAME_CMSG_CHAR_CREATION_REQUEST_ARMORS: u16 = 0x808A;

/// `0x8060`, 4 bytes: the profession picked on the character-creation screen.
pub const GAME_CMSG_CHAR_CREATION_CHANGE_PROF: u16 = 0x8060;

/// `0x808B`, 58 bytes: the character-creation screen's confirmation, which is what a creation costs.
pub const GAME_CMSG_CHAR_CREATION_CONFIRM: u16 = 0x808B;

/// `0x802C`, 7 bytes: the player dropped an item. The reference has no handler for it.
pub const GAME_CMSG_DROP_ITEM: u16 = 0x802C;

/// `0x8072`, 9 bytes: an item moved between two places. The reference has no handler for it either.
pub const GAME_CMSG_ITEM_MOVE: u16 = 0x8072;

/// `0x8030`, 6 bytes: an item was equipped.
pub const GAME_CMSG_EQUIP_ITEM: u16 = 0x8030;

/// `0x804F`, 6 bytes: an item was unequipped; its layout is still unmeasured.
pub const GAME_CMSG_UNEQUIP_ITEM: u16 = 0x804F;

/// `0x8009`, 10 bytes: the client's answer to a ping.
pub const GAME_CMSG_PING_REPLY: u16 = 0x8009;

/// `0x800C`, 2 bytes: the client's keep-alive, one of the two heartbeats.
pub const GAME_CMSG_HEARTBEAT: u16 = 0x800C;

#[cfg(test)]
mod tests {
    /// The constant and the catalogue row must name the same message at the same header. This is the
    /// one thing a rename can get wrong, and it is cheap to check.
    #[test]
    fn every_constant_is_the_header_the_catalogue_names() {
        use crate::packets::{Conn, Status};
        let pairs: &[(&str, u16)] = &[
            ("GAME_CMSG_INSTANCE_LOAD_REQUEST_ITEMS", super::GAME_CMSG_INSTANCE_LOAD_REQUEST_ITEMS),
            ("GAME_CMSG_INSTANCE_LOAD_REQUEST_SPAWN", super::GAME_CMSG_INSTANCE_LOAD_REQUEST_SPAWN),
            ("GAME_CMSG_INSTANCE_LOAD_REQUEST_PLAYERS", super::GAME_CMSG_INSTANCE_LOAD_REQUEST_PLAYERS),
            ("GAME_CMSG_PING_REQUEST", super::GAME_CMSG_PING_REQUEST),
            ("GAME_CMSG_MOVE_TO_COORD", super::GAME_CMSG_MOVE_TO_COORD),
            ("GAME_CMSG_POSITION_UPDATE", super::GAME_CMSG_POSITION_UPDATE),
            ("GAME_CMSG_CANCEL_MOVEMENT", super::GAME_CMSG_CANCEL_MOVEMENT),
            ("GAME_CMSG_LAST_POS_BEFORE_MOVE_CANCELED", super::GAME_CMSG_LAST_POS_BEFORE_MOVE_CANCELED),
            ("GAME_CMSG_TRAVEL_INIT", super::GAME_CMSG_TRAVEL_INIT),
            ("GAME_CMSG_PARTY_TRAVEL", super::GAME_CMSG_PARTY_TRAVEL),
            ("GAME_CMSG_SEND_CHAT_MESSAGE", super::GAME_CMSG_SEND_CHAT_MESSAGE),
            ("GAME_CMSG_DISCONNECT", super::GAME_CMSG_DISCONNECT),
            ("GAME_CMSG_CHAR_CREATION_REQUEST_PLAYER", super::GAME_CMSG_CHAR_CREATION_REQUEST_PLAYER),
            ("GAME_CMSG_CHAR_CREATION_REQUEST_ARMORS", super::GAME_CMSG_CHAR_CREATION_REQUEST_ARMORS),
            ("GAME_CMSG_CHAR_CREATION_CHANGE_PROF", super::GAME_CMSG_CHAR_CREATION_CHANGE_PROF),
            ("GAME_CMSG_CHAR_CREATION_CONFIRM", super::GAME_CMSG_CHAR_CREATION_CONFIRM),
            ("GAME_CMSG_DROP_ITEM", super::GAME_CMSG_DROP_ITEM),
            ("GAME_CMSG_ITEM_MOVE", super::GAME_CMSG_ITEM_MOVE),
            ("GAME_CMSG_EQUIP_ITEM", super::GAME_CMSG_EQUIP_ITEM),
            ("GAME_CMSG_UNEQUIP_ITEM", super::GAME_CMSG_UNEQUIP_ITEM),
            ("GAME_CMSG_PING_REPLY", super::GAME_CMSG_PING_REPLY),
            ("GAME_CMSG_HEARTBEAT", super::GAME_CMSG_HEARTBEAT),
        ];
        for (name, header) in pairs {
            let entry = crate::packets::lookup(Conn::Game, *header, true)
                .unwrap_or_else(|| panic!("{name} ({header:#06X}) is not a game client-to-server row"));
            assert_eq!(
                entry.name,
                Some(*name),
                "{header:#06X} is named {:?} in the catalogue, not {name}",
                entry.name
            );
            assert_eq!(entry.from_client, true, "{name} must be client-to-server");
            assert_ne!(entry.status, Status::Unnamed, "{name} must be a named row");
        }
    }
}
