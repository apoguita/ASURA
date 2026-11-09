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

/// The client asks to walk somewhere (`GameSrv_HandleMoveToCoord`, `GmAgent.c:371-398`).
///
/// The reference asks its path finder for the walk and then answers with **the first waypoint of
/// its own path**, not with the client's destination: the client predicts a straight line, and a
/// straight line through a wall is exactly what this message corrects. When the finder refuses, the
/// reference sends no refusal of its own -- it cancels the walk and broadcasts where the agent
/// really is, which is what makes the client snap back.
pub async fn handle_move_to_coord(
    stream: &mut TcpStream,
    session: &mut Session,
    frame: &Frame,
    world: &World,
    player: &mut Player,
    trace: bool,
) -> Result<bool, GameError> {
    if frame.body().len() < 12 {
        return Err(GameError::Framing(FrameError::Incomplete { needed: 16 }));
    }
    let destination = (
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
    let agent_id = player.GetAgentID();
    let instance = Arc::clone(&player.instance());
    let map_id = instance.map().map_id;
    let Some((from, walk_active)) = instance.with_slot(player.player_id(), |slot| {
        (slot.agent().here(), 0.0 < slot.agent().motion_speed())
    }) else {
        eprintln!("world: agent {agent_id} asked to walk but has no slot in its instance");
        return Ok(true);
    };
    let to = pathfind::Pos {
        x: destination.0,
        y: destination.1,
        plane,
    };
    if move_trace_on() {
        instance.with_slot(player.player_id(), |slot| {
            slot.move_stats_mut().walk_requests += 1
        });
        eprintln!(
            "move[pathfind] agent {agent_id} asked to walk to (x={:.1}, y={:.1}, plane={plane}) \
             while the server thinks it stands at (x={:.1}, y={:.1}, plane={}){} \
             [0x803E, {} bytes]",
            destination.0,
            destination.1,
            from.x,
            from.y,
            from.plane,
            if walk_active {
                " - a server-side walk was still running (client has walked since its last report)"
            } else {
                ""
            },
            frame.body().len() + 2
        );
    }

    let walked = world.graphs.walk(map_id, from, to);
    if move_trace_on() {
        match &walked {
            Walk::Found(waypoints) => eprintln!(
                "move[pathfind] finder produced {} waypoint(s): {}",
                waypoints.len(),
                waypoints_preview(waypoints)
            ),
            Walk::NoPath => eprintln!("move[pathfind] finder refused: no path"),
            Walk::NoGraph => eprintln!(
                "move[pathfind] finder refused: this world has no path graph for map {map_id}"
            ),
        }
    }
    if trace {
        let said = match &walked {
            Walk::Found(waypoints) => format!("{} waypoint(s)", waypoints.len()),
            Walk::NoPath => "no path".to_string(),
            Walk::NoGraph => "no path graph for this map".to_string(),
        };
        eprintln!(
            "world: agent {agent_id} walks from ({:.1}, {:.1}) plane {} to ({:.1}, {:.1}) \
             plane {plane}: {said}",
            from.x, from.y, from.plane, destination.0, destination.1
        );
    }

    match walked {
        // An empty path sends nothing: the reference checks `0 < agent->waypoints.len`.
        Walk::Found(waypoints) => {
            // The path as the live servers hand it out: a destination about every 145 units,
            // which at the character's 288 u/s is the ~0.5 s cadence the live capture shows.
            let waypoints = densify(waypoints, from, live_cadence_step());
            // The reference's own point, as it is: no re-writing of its plane, no dropping of its
            // points. The plane id is whatever the map's pmaps call the surface the path is on
            // (floor is 0; slopes and bridges have their own ids), and that is the reference's
            // business, not this server's.
            let begun = instance
                .with_slot(player.player_id(), |slot| {
                    slot.agent_mut().begin_walk_announcing(waypoints)
                })
                .flatten();
            if let Some(begun) = begun {
                let first = begun.first;
                let current_plane = begun.current_plane;
                let left = begun.left;
                let stood = begun.stood;
                let speed = begun
                    .announcement
                    .then(|| builders::agent_update_speed(agent_id, 1.0, 1));
                let walk = builders::agent_move_to_point(
                    agent_id,
                    (first.pos.x, first.pos.y),
                    first.pos.plane,
                    current_plane,
                );
                if move_trace_on() {
                    eprintln!(
                        "move[pathfind] answered with waypoint 0 (x={:.1}, y={:.1}, \
                             plane={}) current_plane={current_plane} [0x0029, {} bytes]; {left} \
                             waypoint(s) left to walk, agent stays at (x={:.1}, y={:.1}, plane={})",
                        first.pos.x,
                        first.pos.y,
                        first.pos.plane,
                        walk.len(),
                        stood.0,
                        stood.1,
                        current_plane
                    );
                }
                // Everyone else is told where this agent is walking: the reference broadcasts the
                // same message (`GameSrv_BroadcastAgentMoveToPoint`), and a client that is not told
                // keeps drawing the walker where it last stood.
                let mut reply: Vec<_> = speed.into_iter().collect();
                reply.push(walk);
                instance.broadcast(player.player_id(), &reply);
                send(stream, session, reply, trace).await?;
            }
        }
        Walk::NoPath | Walk::NoGraph => {
            if trace {
                eprintln!("world: path finding failed for agent {agent_id}; standing still");
            }
            instance.with_slot(player.player_id(), |slot| slot.agent_mut().cancel_walk());
            let standing = player.standing();
            // **Nothing goes on the wire.** The reference answers a refused walk with
            // `AGENT_UPDATE_POSITION` (`0x002C`, `GmAgent.c:391-394`), and that message carries
            // the server's copy position: measured, it disconnected the client with 007 once
            // (capture-8001-20261027-221113 seq 519) and teleported it into a wall once
            // (capture of 22:16: `snapped to (9438.1, 7118.5)` after the clamp had dragged the
            // copy off the navmesh). Silence is what the reference sends in its other correction
            // path (`GameSrv_HandleLastPosOnMoveCanceled`), and the client's own reports settle
            // the truth.
            if move_trace_on() {
                eprintln!(
                    "move[pathfind] walk refused; the agent stands at (x={:.1}, y={:.1}, \
                     plane={}) and nothing is sent",
                    standing.position.0, standing.position.1, standing.plane
                );
            }
        }
    }
    Ok(true)
}
