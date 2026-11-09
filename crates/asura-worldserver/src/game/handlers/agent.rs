//! The agent handlers: the messages a client sends about movement.
//!
//! The reference keeps these in `src/server/game/Handlers/MovementHandler.cpp`. Two of them were
//! reachable from here only after their rules moved into the noun (cut 4d): `cancel_walk` and
//! `adopt_correction_point`. The other three - move-to-coord, position update, spawn request - still
//! carry rules of their own and stay in `game/entities/agent` until those rules move too.

use super::*;

/// The keyboard stop includes its stopping point; an accepted point ends the old
/// path and answers with type 9 and that point (alternating-live.jsonl 482-486).
///
/// A refused correction sends **nothing at all**. The reference refuses a long or wrong-plane correction
/// and stops there (`GmAgent.c:547-563`, `docs/history/gwps-parity/behavior-audit.md` row 17); the frame it must not send is
/// the destination, because the only position it could name is its own stale guess -- and a client
/// obeys that message, so the character walks backwards to where it stood a report ago. Measured live:
/// `logs/world-8001.log` 6230-6246 is one refusal ordering the client 196.7 units back and the client
/// complying. The server's own copy still stops walking.
pub async fn handle_last_position(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    if frame.body().len() < 12 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 16 }));
    }
    let position = (
        f32::from_le_bytes([
            frame.body()[0],
            frame.body()[1],
            frame.body()[2],
            frame.body()[3],
        ]),
        f32::from_le_bytes([
            frame.body()[4],
            frame.body()[5],
            frame.body()[6],
            frame.body()[7],
        ]),
    );
    let plane = u32::from_le_bytes([
        frame.body()[8],
        frame.body()[9],
        frame.body()[10],
        frame.body()[11],
    ]) as u16;
    let instance = Arc::clone(&player.instance());
    let Some(correction) = instance.with_slot(player.player_id(), |slot| {
        slot.agent_mut().adopt_correction_point(position, plane)
    }) else {
        return Ok(true);
    };
    let taken = correction.taken;
    let reply = correction.reply;
    let believed = correction.believed;
    let distance = correction.distance;
    if !reply.is_empty() {
        instance.broadcast(player.player_id(), &reply);
        send(stream, session, reply, trace).await?;
    }
    if position.0.is_finite() && position.1.is_finite() {
        player.record_portal_step(position);
    } else {
        let _ = player.take_last_step();
    }
    if move_trace_on() {
        // The distance is what decides and the ceiling is configuration, so the trace names both: a
        // refusal is otherwise indistinguishable from a plane mismatch without it.
        eprintln!(
            "move[stop] correction to (x={:.1}, y={:.1}, plane={plane}) against the server's \
             (x={:.1}, y={:.1}, plane={}) - d={distance:.1}, ceiling={:.1}: {}",
            position.0,
            position.1,
            believed.position.0,
            believed.position.1,
            believed.plane,
            max_allowed_correction(),
            if taken {
                "taken - type 9 and a destination at that point [0x802B, 0x8029]"
            } else if plane != believed.plane {
                "refused (the plane differs) - the agent stops and nothing is sent"
            } else {
                "refused (farther than the ceiling) - the agent stops and nothing is sent"
            }
        );
    }
    if trace {
        eprintln!(
            "world: agent {} correction to ({:.1}, {:.1}) plane {plane}: {}",
            player.GetAgentID(),
            position.0,
            position.1,
            if taken {
                "taken - type 9 and a destination at that point"
            } else {
                "refused - the agent stops and nothing is sent"
            }
        );
    }
    Ok(true)
}

/// The client stopped walking (`GameSrv_HandleCancelMovement`): cancel the agent's own walk --
/// `GameSrv_CancelAgentMovement` sets its destination to where it stands and its speed to zero --
/// and then tell everyone it stopped. Cancelling is the load-bearing half: without it the server
/// keeps advancing its copy along the abandoned path, and the next click is answered from a
/// position the client walked away from minutes ago (observed live: a click to x4 started from
/// the old x2 the client had already left, pulling the agent back to it).
pub async fn handle_cancel_movement(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    let _ = world;
    let instance = Arc::clone(&player.instance());
    let Some((cancelled, stop)) = instance.with_slot(player.player_id(), |slot| {
        let cancelled = slot.agent_mut().cancel_walk();
        let stop = builders::agent_stop_moving(slot.agent().agent_id());
        (cancelled, stop)
    }) else {
        return Ok(true);
    };
    let was_walking = cancelled.was_walking;
    let stood = cancelled.stood;
    if move_trace_on() {
        eprintln!(
            "move[stop] client cancelled walking at (x={:.1}, y={:.1}, plane={}){} \
             [0x8028, {} bytes]",
            stood.position.0,
            stood.position.1,
            stood.plane,
            if was_walking {
                " - a server-side walk WAS running: cancelled (speed -> 0, destination cleared)"
            } else {
                " (no server-side walk was running)"
            },
            frame.body().len() + 2
        );
    }
    instance.broadcast(player.player_id(), std::slice::from_ref(&stop));
    send(stream, session, vec![stop], trace).await?;
    Ok(true)
}
