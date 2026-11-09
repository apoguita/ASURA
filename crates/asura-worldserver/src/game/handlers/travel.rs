//! The travel handlers: the messages a client sends about going somewhere.
//!
//! The reference keeps these in `src/server/game/Handlers/` beside the others. `handle_travel_init` is
//! a measurement - it names the travel object the client touched and starts nothing, because no portal
//! row names a non-player agent yet. `handle_party_travel` is still in `game/mechanics/travel` because
//! the teleport cycle it drives is a rule of that noun, not a message shape.

use super::*;

/// A travel the **client** announced: `GAME_CMSG_TRAVEL_INIT` (`0x80C1`), the portal touch.
///
/// Ten bytes on the wire, two agent ids and nothing else (the row's fields; the capture in
/// `recordings/client/feature/movement/movement-sniff.log` seq 1651 shows the same message with a
/// four-byte header, `3a 00 00 00 | 00 00 00 00` after it, and seq 1652 shows the client sending
/// `INTERACT_LIVING` for that same agent 0x3A in the same millisecond - so the first id is the travel
/// object the player used, and the second was zero).
///
/// **It names an agent, not a map**, so the destination is this server's knowledge, not the client's:
/// the object it names leads somewhere only because a row says so. This world has no portal table yet
/// and no non-player agent to touch (it sends exactly one agent, the player), so this arm names the
/// touch and starts no transfer - it is the measurement that has to come first. The line below is
/// what the next live touch writes: which agent, in which map, for which player. A portal row keyed
/// by that map and agent is what turns this into the same redirect `/tp` already sends
/// (`docs/reference/client-protocol.md` 5e).
pub fn handle_travel_init(frame: &Frame, player: &Player, _trace: bool) -> Result<bool, GameError> {
    if frame.body().len() < 8 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 10 }));
    }
    let agent = u32::from_le_bytes([
        frame.body()[0],
        frame.body()[1],
        frame.body()[2],
        frame.body()[3],
    ]);
    let other = u32::from_le_bytes([
        frame.body()[4],
        frame.body()[5],
        frame.body()[6],
        frame.body()[7],
    ]);
    eprintln!(
        "world: {:?} touched a travel object in map {} '{}': TRAVEL_INIT agent {agent:#010X}, \
         second {other:#010X}; no portal row names that agent, so no travel starts",
        player.name(),
        player.instance().map().map_id,
        player.instance().map().name,
    );
    Ok(true)
}

/// The client's own request, from the world map: `GAME_CMSG_PARTY_TRAVEL` (`0x80B1`).
///
/// **The client decides where it may go; this server carries the decision out.** That is the whole
/// policy (`docs/reference/game-world-model.md`): the unlocked-instance bitmap is what a client uses to offer a
/// destination, and a server that second-guesses it blocks legitimate travel - an outpost and the
/// explorable it exits to can share one map blob, so a rule about "which kinds of instance you may
/// travel to" refuses travel the game itself allows.
///
/// The wire, measured from this client's own sends (`logs/world-8001.log`, `world <- 0x80B1 (9 bytes)`):
///
/// ```text
/// B1 80 | 94 00 | 02 | 00 00 | 00 | 01      destination 148
/// B1 80 | A6 00 | 02 | 00 00 | 00 | 01      destination 166
/// ```
///
/// so the destination instance is a `u16` first, then three bytes this server does not interpret. The
/// retail client sends the same request with a 32-bit instance id
/// (`captures/zone-ref2-012225.jsonl`: `b1 00 00 00 | a4 00 00 00 | ... | 01 00 00 00`), which is the
/// other build of the protocol (`docs/reference/client-protocol.md` 5e) - the field is the same one either way.
pub async fn handle_party_travel(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    if frame.body().len() < 2 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 9 }));
    }
    let map_id = i32::from(u16::from_le_bytes([frame.body()[0], frame.body()[1]]));
    eprintln!(
        "world: {:?} asked for instance {map_id} from the world map in map {}",
        player.name(),
        player.instance().map().map_id
    );
    let Some(destination) = resolve_instance(world, map_id).await? else {
        // A statement about this server, not about the client's rights: it has no row, no blob, or a
        // type this world does not host.
        eprintln!("world: instance {map_id} cannot be served by this world; the travel is refused");
        return Ok(true);
    };
    begin_transfer(stream, session, world, player, destination, None, trace).await
}
