//! The dispatch table: one header in, one domain's handler out.
//!
//! The reference's `GameSrv.c` is one switch over headers and so is this; a client cannot be told
//! about a command seam it does not speak. What this file is *not*: a home for domain logic. Each
//! arm validates, then calls into `item`, `chat`, `travel`, `creation`, `player` or `map`; the
//! only state it touches is the one the message is about.
//!
//! A header with no arm is framed and ignored - named in the trace, never guessed at.

use asura_protocol::msg;

use crate::*;

/// Answer one client message. `Ok(false)` finishes the session.
pub async fn answer(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    match frame.header {
        msg::GAME_CMSG_INSTANCE_LOAD_REQUEST_ITEMS => item::handle_request_items(stream, session, world, player, trace).await,

        msg::GAME_CMSG_INSTANCE_LOAD_REQUEST_SPAWN => map::handle_request_spawn(stream, session, world, player, trace).await,

        msg::GAME_CMSG_INSTANCE_LOAD_REQUEST_PLAYERS => world::handle_request_players(stream, session, world, player, trace).await,

        msg::GAME_CMSG_PING_REQUEST => connection::handle_ping_request(stream, session, trace).await,

        msg::GAME_CMSG_MOVE_TO_COORD => agent::handle_move_to_coord(stream, session, &frame, world, player, trace).await,

        msg::GAME_CMSG_POSITION_UPDATE => {
            let answer = agent::handle_position_update(&frame, player, trace);

            travel::portal_reached(stream, session, world, player, trace).await?;
            answer
        }

        msg::GAME_CMSG_CANCEL_MOVEMENT => {
            agent::handle_cancel_movement(stream, session, &frame, world, player, trace).await
        }

        msg::GAME_CMSG_LAST_POS_BEFORE_MOVE_CANCELED => {
            let answer = agent::handle_last_position(&frame, player, trace);
            travel::portal_reached(stream, session, world, player, trace).await?;
            answer
        }

        // The client's own travel announcement: the player touched a travel object. Named rather than
        // left to the fallthrough, because the line it prints is the measurement a portal table is
        // built from - see `travel::handle_travel_init` for what it carries and what is still missing.
        msg::GAME_CMSG_TRAVEL_INIT => travel::handle_travel_init(&frame, player, trace),

        // The world map's request. The client has already decided which instance it wants; this carries
        // the decision out with the same cycle `/tp` uses.
        msg::GAME_CMSG_PARTY_TRAVEL => travel::handle_party_travel(stream, session, &frame, world, player, trace).await,

        msg::GAME_CMSG_SEND_CHAT_MESSAGE => chat::handle_chat_message(stream, session, &frame, world, player, trace).await,

        // The client is going away.
        msg::GAME_CMSG_DISCONNECT => Ok(false),

        msg::GAME_CMSG_CHAR_CREATION_REQUEST_PLAYER => creation::handle_request_player(stream, session, player, trace).await,

        msg::GAME_CMSG_CHAR_CREATION_REQUEST_ARMORS => creation::handle_request_armors(stream, session, trace).await,

        msg::GAME_CMSG_CHAR_CREATION_CHANGE_PROF => creation::handle_change_profession(stream, session, &frame, player, trace).await,

        msg::GAME_CMSG_CHAR_CREATION_CONFIRM => creation::handle_confirm(stream, session, &frame, world, player, trace).await,

        msg::GAME_CMSG_DROP_ITEM => item::handle_drop_item(&frame, world, player).await,

        msg::GAME_CMSG_ITEM_MOVE => item::handle_item_move(stream, session, &frame, world, player, trace).await,

        msg::GAME_CMSG_EQUIP_ITEM => item::handle_equip_item(stream, session, &frame, world, player, trace).await,

        msg::GAME_CMSG_UNEQUIP_ITEM => item::handle_unequip_item(),

        // The client answered one of our pings (`GAME_CMSG_PING_REPLY`, `0x8009`, 10 bytes). Nothing to
        // do: the reference discards it. Named rather than ignored, because a trace that says "ignored"
        // twenty-nine times reads like a bug.
        msg::GAME_CMSG_PING_REPLY => Ok(true),

        // `GAME_CMSG_HEARTBEAT` (`0x800C`, 2 bytes). The other keep-alive, likewise discarded.
        msg::GAME_CMSG_HEARTBEAT => Ok(true),

        // A message this world can frame but does not act on. Framing already happened, so ignoring it
        // is safe and is what the reference does for most of them. The name is in the trace when the
        // generated table has one: "0x8072 ignored" is a riddle, "GAME_CMSG_ITEM_MOVE ignored" is a
        // to-do.
        other => {
            if trace {
                let name = asura_protocol::client::game_tables::lookup(other, true)
                    .and_then(|entry| entry.name)
                    .unwrap_or("unnamed");
                eprintln!("world: {other:#06X} {name} ignored");
            }
            Ok(true)
        }
    }
}
