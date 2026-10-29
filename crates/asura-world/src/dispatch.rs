//! The dispatch table: one header in, one domain's handler out.
//!
//! The reference's `GameSrv.c` is one switch over headers and so is this; a client cannot be told
//! about a command seam it does not speak. What this file is *not*: a home for domain logic. Each
//! arm validates, then calls into `item`, `chat`, `travel`, `creation`, `player` or `map`; the
//! only state it touches is the one the message is about.
//!
//! A header with no arm is framed and ignored - named in the trace, never guessed at.

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
        0x8091 => item::handle_request_items(stream, session, world, player, trace).await,

        0x8088 => map::handle_request_spawn(stream, session, world, player, trace).await,

        0x8090 => world::handle_request_players(stream, session, world, player, trace).await,

        0x800D => connection::handle_ping_request(stream, session, trace).await,

        0x803E => agent::handle_move_to_coord(stream, session, &frame, world, player, trace).await,

        0x803D => agent::handle_position_update(&frame, player, trace),

        0x8028 => {
            agent::handle_cancel_movement(stream, session, &frame, world, player, trace).await
        }

        0x8047 => agent::handle_last_position(&frame, player, trace),

        0x8064 => chat::handle_chat_message(stream, session, &frame, world, player, trace).await,

        // The client is going away.
        0x8008 => Ok(false),

        0x8089 => creation::handle_request_player(stream, session, player, trace).await,

        0x808A => creation::handle_request_armors(stream, session, trace).await,

        0x8060 => creation::handle_change_profession(stream, session, &frame, player, trace).await,

        0x808B => creation::handle_confirm(stream, session, &frame, world, player, trace).await,

        0x802C => item::handle_drop_item(&frame, world, player).await,

        0x8072 => item::handle_item_move(stream, session, &frame, world, player, trace).await,

        0x8030 => item::handle_equip_item(stream, session, &frame, world, player, trace).await,

        0x804F => item::handle_unequip_item(),

        // The client answered one of our pings (`GAME_CMSG_PING_REPLY`, `0x8009`, 10 bytes). Nothing to
        // do: the reference discards it. Named rather than ignored, because a trace that says "ignored"
        // twenty-nine times reads like a bug.
        0x8009 => Ok(true),

        // `GAME_CMSG_HEARTBEAT` (`0x800C`, 2 bytes). The other keep-alive, likewise discarded.
        0x800C => Ok(true),

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
