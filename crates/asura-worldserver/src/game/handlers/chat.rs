//! The chat handlers: what a client says, and what this server answers.
//!
//! The reference keeps these in `src/server/game/Handlers/ChatHandler.cpp`. The commands themselves -
//! `/maps`, `/stuck`, `/tp`, the four channel prefixes - live in `game/mechanics/chat`; this file only
//! carries the message in and the answers out.

use super::*;
/// The client speaks (`GameSrv_HandleChatMessage`). The first unit of the message is the channel:
/// `!` is local/general, `#` group, `$` trade, `/` a command, and the reference answers anything
/// else with bad user data.
///
/// **A player's message is sent ungated**, exactly as the reference sends it: its
/// `GameSrv_ChatSendingEnabled` gate covers the text the *server* generates (GM answers, "unknown
/// command"), not a line a player typed. This port once had a switch on everything, which is why
/// a `!` line went nowhere; the switch is gone (`docs/reference/client-protocol.md` #11), so nothing here is gated and only
/// the messages the reference never sends are withheld.
pub async fn handle_chat_message(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let body = &frame.body();
    if body.len() < 6 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 8 }));
    }
    let units = u16::from_le_bytes([body[4], body[5]]) as usize;
    let text: Vec<u16> = (0..units)
        .map(|index| {
            let at = 6 + index * 2;
            u16::from_le_bytes([
                body.get(at).copied().unwrap_or(0),
                body.get(at + 1).copied().unwrap_or(0),
            ])
        })
        .collect();
    let Some(channel) = text.first().copied() else {
        return Ok(true); // an empty message, which the reference drops
    };
    let spoken: String = char::decode_utf16(text[1.min(text.len())..].iter().copied())
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();

    match u8::try_from(channel).unwrap_or(0) as char {
        // `Channel_All`, `Channel_Group`, `Channel_Trade`: the three channels the reference
        // speaks on. Sent ungated, as the reference sends them. None of the three chat messages
        // appears in any capture, and no chat text has ever rendered on screen in either server
        // -- so what this path matches is the reference's *bytes*, not its visible result.
        speaker @ ('!' | '#' | '$') => {
            let channel = match speaker {
                '#' => builders::CHANNEL_GROUP,
                '$' => builders::CHANNEL_TRADE,
                _ => builders::CHANNEL_ALL,
            };
            if trace {
                eprintln!("world: {speaker} {spoken:?} on channel {channel}");
            }
            send(
                stream,
                session,
                vec![
                    builders::chat_message_core(&spoken),
                    builders::chat_message_local(player.player_id(), channel),
                ],
                trace,
            )
            .await?;
            Ok(true)
        }
        '/' => {
            // The command surface, from the reference's own two places: `GmCommands.c`'s
            // `GameSrv_TryHandleGmCommand` (`/tp`, `/teleport`, `/maps`) and `GmChat.c`'s
            // `GameSrv_HandleChatCommand` (`/stuck`). `COMMANDS` names them once, and
            // `tools/parity-commands.py` compares that list with the reference's so the surface
            // cannot drift without a check failing.
            let spoken = spoken.trim();
            let (name, argument) = match spoken.split_once(' ') {
                Some((name, rest)) => (name, rest.trim_start()),
                None => (spoken, ""),
            };
            match command(name) {
                Some(named) if named.names.contains(&"stuck") && argument.is_empty() => {
                    // `GameSrv_HandleChatCommand`'s `stuck`: cancel the movement and broadcast where
                    // the agent is now. **Not** `AGENT_STOP_MOVING` -- that is what
                    // `HandleCancelMovement` (`0x8028`) answers, and the reference sends a position
                    // update here instead. It also broadcasts a world tick first; the 500 ms ticker
                    // covers that, so no second clock is invented.
                    let Some(agent) = player.GetAgent() else {
                        return Ok(true);
                    };
                    if trace {
                        eprintln!(
                            "world: /stuck -- agent {} stands at ({:.1}, {:.1}) plane {}",
                            agent.agent_id(),
                            agent.position().0,
                            agent.position().1,
                            agent.plane()
                        );
                    }
                    send(
                        stream,
                        session,
                        vec![builders::agent_update_position(
                            agent.agent_id(),
                            agent.position(),
                            agent.plane(),
                        )],
                        trace,
                    )
                    .await?;
                    Ok(true)
                }
                Some(named) if named.names.contains(&"maps") && argument.is_empty() => {
                    // `GmCommands_ListTargets`, as it behaves with the reference's own defaults: the
                    // table goes to the log, and nothing goes on the wire. That is parity, not a
                    // gate -- GWPS's chat gate is what leaves its chat half in its log too, and no
                    // chat text has ever reached a screen in either server. Sending it here crashed
                    // a live client, which is why the answer is printed and not sent.
                    let mut listed = world.all_maps();
                    if world.load_on_demand {
                        // On-demand: every catalogue map is one this world will serve
                        // (it loads it on first use), so /maps answers the reference's
                        // table even before a map has been used. One query per map that
                        // has not been loaded yet; /maps is a console command, not a hot
                        // path.
                        if let Ok(maps) = asura_config::world::catalogue() {
                            for map_id in maps.keys() {
                                if listed.iter().any(|map| map.map_id == *map_id) {
                                    continue;
                                }
                                if let Ok(Some(map)) =
                                    crate::catalogue::entry(*map_id, world.capacity)
                                {
                                    listed.push(map);
                                }
                            }
                        }
                    }
                    let answer = maps_answer(&listed);
                    for line in &answer.table {
                        eprintln!("world: {line}");
                    }
                    if trace {
                        for line in &answer.chat {
                            eprintln!("world: /maps (chat, not sent): {line}");
                        }
                    }
                    Ok(true)
                }
                Some(named) if named.names.contains(&"tp") && !argument.is_empty() => {
                    // `GameSrv_HandleTeleportCommand`: a *server transfer*, which is what the game
                    // itself does when a portal is walked through. Every byte of the redirect is
                    // measured (`docs/reference/client-protocol.md` #5) and the reference's refusals are here
                    // verbatim, so this is a port rather than an invention.
                    teleport(stream, session, world, player, argument, trace).await
                }
                _ => {
                    // `GameSrv_SendInvalidCommand`: the reference's text is "Unknown command". That
                    // covers `/tp` with no map name (`GmCommand_ToAscii` refuses the empty
                    // argument) and every name that is not in the table; with its chat gate off
                    // it is a log line and nothing else, and so it is here.
                    eprintln!("world: Unknown command");
                    Ok(true)
                }
            }
        }
        other => {
            // `@`, `%` and `"` are ignored by the reference; anything else it refuses.
            if matches!(other, '@' | '%' | '"') {
                if trace {
                    eprintln!("world: channel {other:?} ignored, as the reference ignores it");
                }
                return Ok(true);
            }
            eprintln!("world: chat on channel {other:?} refused, as the reference refuses it");
            Ok(true)
        }
    }
}
