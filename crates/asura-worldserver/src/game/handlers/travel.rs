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
