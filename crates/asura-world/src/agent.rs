//! One agent's position, and the rules the reference uses to believe a client about it.
//!
//! A client moves itself: it walks where it wants and reports its position about twice a second
//! (`GAME_CMSG_POSITION_UPDATE`), and it can ask to stop (`CANCEL_MOVEMENT`) or to correct where it was
//! when a move was cancelled. The server's job is to keep its own idea of where the agent is, and to
//! refuse the parts of that which cannot be true.
//!
//! The rules are the reference's (`GmAgent.c`, `GameSrv_HandlePositionUpdate` and friends), and they are
//! copied rather than invented because they are what a real client's movement is shaped for:
//!
//! * with 50 units of slack, a client may be as far ahead as its own speed allows since the last report,
//!   and no further. Between two reports 2 seconds apart at 288 units a second that is a generous
//!   budget; a report after a five-minute stall is not a teleport, it is a client that resumed;
//! * an out-of-budget report is not dropped. The agent is moved as far along the reported direction as
//!   the budget allows — dropping it leaves the two views further apart than accepting part of it;
//! * a plane change is allowed only when it is short (stairs, a small portal);
//! * a correction after a cancelled move is accepted only within 100 units and only on the same plane.
//!
//! ## Walking
//!
//! The other half is the server's own idea of a walk, and it is the reference's too: a `MOVE_TO_COORD`
//! becomes a path (`pathfind.rs`, from `GmPaths.c`), the agent walks the first segment at its own
//! `speed_base` (288 units a second, `GameSrv.c:1088`), and every world tick moves it `speed_base * delta`
//! further along. When a segment ends the next waypoint becomes the destination and the world is told;
//! when the last one ends the walk is over and the agent stands still. That state is [`Motion`], which is
//! the reference's `GmAgent.destination`, `.direction`, `.speed`, `.waypoints` and `.waypoint_idx`.

use std::time::{Duration, Instant};

use crate::pathfind::{Pos, Waypoint};

/// The reference's `POSITION_UPDATE_TOLERANCE`: how far a client may be from where the server thought,
/// before distance is even considered.
pub const POSITION_UPDATE_TOLERANCE: f32 = 50.0;

/// The reference's `POSITION_UPDATE_MAX_ELAPSED_MS`: the longest gap between reports that counts as
/// elapsed time. A longer gap is treated as this, so a stalled client does not get an unbounded budget.
pub const POSITION_UPDATE_MAX_ELAPSED_MS: u32 = 2_000;

/// The reference's `MAXIMUM_ALLOWED_CORRECTION`: how far a client may move itself when it cancels a walk.
pub const MAXIMUM_ALLOWED_CORRECTION: f32 = 100.0;

/// What the server did with a reported position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adopted {
    /// The report was inside the budget and is now the agent's position.
    Accepted,
    /// The report was too far ahead; the agent moved as far as the budget allows, along the reported
    /// direction.
    Clamped,
    /// The report was refused (a plane change that is too far).
    Refused,
}

/// The agents one gameplay connection has: the player's own, and whatever a drop put on the ground.
///
/// The reference allocates agent ids from a per-server table (`GmIdAllocate`) and frees them when the
/// agent goes away; a dropped item's agent is never freed, because there is nothing in the reference that
/// picks an item up. With one player and a handful of dropped items the table is a counter — but the ids
/// still have to be distinct, because a client told that two agents share an id draws one of them.
#[derive(Debug, Clone)]
pub struct Agents {
    /// The character this connection drives.
    pub player: AgentState,
    next_id: u32,
}

impl Agents {
    /// A session with one agent: the player, at `agent_id`, standing where the map or the saved position
    /// says.
    pub fn new(agent_id: u32, position: (f32, f32), plane: u16, speed_base: f32) -> Agents {
        Agents {
            player: AgentState::new(agent_id, position, plane, speed_base),
            next_id: agent_id.saturating_add(1),
        }
    }

    /// The next free agent id, for something the world creates (`GAME_CMSG_DROP_ITEM`).
    pub fn allocate(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

/// Where one agent is, and when it last said so.
#[derive(Debug, Clone)]
pub struct AgentState {
    pub agent_id: u32,
    pub position: (f32, f32),
    pub plane: u16,
    /// The reference's `last_position_update`, in milliseconds since the connection started.
    last_report: Option<Instant>,
    /// The agent's walking speed, which is what the budget is computed from.
    speed_base: f32,
    /// The walk in progress, if any (`GmAgent.destination`, `.direction`, `.speed`, `.waypoints`,
    /// `.waypoint_idx`).
    pub motion: Motion,
}

/// The reference's `GmAgent` walking state: one segment being walked, and the waypoints after it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Motion {
    /// `GmAgent.waypoints`: the path the server chose. The segment being walked has already been taken
    /// off the front of it, exactly as `waypoint_idx` does in the reference.
    pub waypoints: Vec<Waypoint>,
    /// `GmAgent.waypoint_idx`: the next waypoint to take when the current segment ends.
    pub waypoint_idx: usize,
    /// `GmAgent.destination`: where the current segment ends, plane included.
    pub destination: Pos,
    /// `GmAgent.direction`: a unit vector from the agent toward `destination`.
    pub direction: (f32, f32),
    /// `GmAgent.speed`: zero when the agent is standing still.
    pub speed: f32,
}

impl AgentState {
    /// A new agent, standing where the map says a character arrives.
    pub fn new(agent_id: u32, position: (f32, f32), plane: u16, speed_base: f32) -> AgentState {
        AgentState {
            agent_id,
            position,
            plane,
            last_report: None,
            speed_base,
            motion: Motion::default(),
        }
    }

    /// The agent's position as a point on the map's graph.
    pub fn here(&self) -> Pos {
        Pos {
            x: self.position.0,
            y: self.position.1,
            plane: self.plane,
        }
    }

    /// Begin walking a path the finder produced (`GameSrv_HandleMoveToCoord`'s success half).
    ///
    /// The reference takes the first waypoint as the destination and leaves the rest in the list; this is
    /// that, and it returns the waypoint so the caller can broadcast it. `None` when the path is empty,
    /// which the reference also ignores (`if (0 < agent->waypoints.len)`).
    pub fn begin_walk(&mut self, waypoints: Vec<Waypoint>) -> Option<Waypoint> {
        self.motion.waypoints = waypoints;
        self.motion.waypoint_idx = 0;
        let first = *self.motion.waypoints.first()?;
        self.motion.waypoint_idx = 1;
        self.set_destination(first.pos);
        Some(first)
    }

    /// `GameSrv_UpdateAgentDestination`, without the broadcast: point at `destination` and start moving.
    ///
    /// The reference divides by the distance without checking it, so a destination equal to the current
    /// position produces a NaN direction and then a NaN position -- which would be saved to the database
    /// and reloaded on the next visit. This keeps the direction it had instead, which makes such a walk a
    /// no-op that the arrival test below clears on the next tick. That is a deliberate difference, and
    /// the only one in this file.
    fn set_destination(&mut self, destination: Pos) {
        self.motion.speed = self.speed_base;
        self.motion.destination = destination;
        let dx = destination.x - self.position.0;
        let dy = destination.y - self.position.1;
        let norm = ((dx * dx) + (dy * dy)).sqrt();
        if norm > 0.0 {
            self.motion.direction = (dx / norm, dy / norm);
        }
    }

    /// `GameSrv_CancelAgentMovement`: stand where you are -- the destination collapses to the position and
    /// the speed to zero. The waypoint list is the caller's business, as it is in the reference.
    pub fn cancel_movement(&mut self) {
        self.motion.destination = self.here();
        self.motion.speed = 0.0;
    }

    /// `GameSrv_UpdateAgentMovement`: walk `delta` seconds of the current segment.
    ///
    /// Returns the waypoint that became the new destination, if the agent crossed one during the tick --
    /// the caller broadcasts it, because every client is drawing this agent from the last destination it
    /// was told. Arrival clears the walk, as the reference does.
    pub fn advance(&mut self, delta: f32) -> Option<Waypoint> {
        if self.motion.speed == 0.0 {
            return None;
        }

        let mut distance = delta * self.motion.speed;
        let destination = self.motion.destination;
        let to_destination = distance_between(self.position, (destination.x, destination.y));

        let mut crossed = None;
        if to_destination <= distance {
            distance -= to_destination;

            self.position = (destination.x, destination.y);
            self.plane = destination.plane;

            if self.motion.waypoint_idx < self.motion.waypoints.len() {
                let next = self.motion.waypoints[self.motion.waypoint_idx];
                self.motion.waypoint_idx += 1;
                self.set_destination(next.pos);
                crossed = Some(next);
            } else {
                distance = 0.0;
            }
        }

        if 0.0 < distance {
            let (dx, dy) = self.motion.direction;
            self.position.0 += dx * distance;
            self.position.1 += dy * distance;
        }

        if self.plane == destination.plane
            && self.position.0 == destination.x
            && self.position.1 == destination.y
        {
            self.motion.destination = Pos::default();
            self.motion.speed = 0.0;
            self.motion.waypoint_idx = 0;
            self.motion.waypoints.clear();
        }

        crossed
    }

    /// Adopt a position the client reported (`GAME_CMSG_POSITION_UPDATE`).
    pub fn adopt_report(&mut self, position: (f32, f32), plane: u16, now: Instant) -> Adopted {
        let elapsed_ms = match self.last_report {
            // The first report of a session gets the full budget rather than none: the client has been
            // standing still while it loaded, and the reference makes the same choice.
            None => POSITION_UPDATE_MAX_ELAPSED_MS,
            Some(last) => {
                let elapsed = now.saturating_duration_since(last).as_millis() as u32;
                if elapsed == 0 || elapsed > POSITION_UPDATE_MAX_ELAPSED_MS {
                    POSITION_UPDATE_MAX_ELAPSED_MS
                } else {
                    elapsed
                }
            }
        };
        self.last_report = Some(now);

        let allowed = self.speed_base * (elapsed_ms as f32 / 1000.0) + POSITION_UPDATE_TOLERANCE;
        let distance = self.distance_to(position);

        if plane != self.plane {
            // A plane change is legitimate when it is close by and suspicious when it is not.
            return if allowed < distance {
                Adopted::Refused
            } else {
                self.plane = plane;
                self.position = position;
                Adopted::Accepted
            };
        }

        if distance <= allowed {
            self.position = position;
            return Adopted::Accepted;
        }

        // Out of budget: move as far as the budget allows, along the reported direction.
        let step = if distance > 0.0 {
            allowed / distance
        } else {
            0.0
        };
        self.position = (
            self.position.0 + (position.0 - self.position.0) * step,
            self.position.1 + (position.1 - self.position.1) * step,
        );
        Adopted::Clamped
    }

    /// Adopt a correction the client sends after cancelling a walk
    /// (`GAME_CMSG_LAST_POS_BEFORE_MOVE_CANCELED`). Only a short correction on the same plane counts.
    pub fn adopt_correction(&mut self, position: (f32, f32), plane: u16) -> bool {
        if plane != self.plane || MAXIMUM_ALLOWED_CORRECTION < self.distance_to(position) {
            return false;
        }
        self.position = position;
        true
    }

    /// How long ago the client last reported, for the trace.
    pub fn since_last_report(&self, now: Instant) -> Option<Duration> {
        self.last_report
            .map(|last| now.saturating_duration_since(last))
    }

    fn distance_to(&self, other: (f32, f32)) -> f32 {
        distance_between(self.position, other)
    }
}

/// `Vec2f_Dist2`: the real square root, which is what the reference's movement uses (its *path* finder
/// uses the table-driven one, and that is `pathfind::path_sqrt`).
fn distance_between(left: (f32, f32), right: (f32, f32)) -> f32 {
    let dx = right.0 - left.0;
    let dy = right.1 - left.1;
    ((dx * dx) + (dy * dy)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentState {
        AgentState::new(1, (6602.0, 4485.0), 0, 288.0)
    }

    #[test]
    fn a_report_inside_the_budget_is_believed() {
        let now = Instant::now();
        let mut agent = agent();
        let later = now + Duration::from_millis(500);
        assert_eq!(
            agent.adopt_report((6602.0 + 100.0, 4485.0), 0, later),
            Adopted::Accepted
        );
        assert_eq!(agent.position, (6702.0, 4485.0));
    }

    #[test]
    fn a_report_ahead_of_the_budget_is_clamped_not_dropped() {
        // The first report of a session gets the full budget, so the budget only bites from the second
        // one: 5000 units in 500 ms is not something the agent could have walked, and it is moved as far
        // as 500 ms of its own speed plus the tolerance allows, along the same line.
        let now = Instant::now();
        let mut agent = agent();
        agent.adopt_report((6602.0, 4485.0), 0, now); // the first report sets the clock
        let later = now + Duration::from_millis(500);
        assert_eq!(
            agent.adopt_report((6602.0 + 5000.0, 4485.0), 0, later),
            Adopted::Clamped
        );
        let travelled = agent.position.0 - 6602.0;
        let allowed = 288.0 * 0.5 + POSITION_UPDATE_TOLERANCE;
        assert!(
            (travelled - allowed).abs() < 0.01,
            "moved {travelled}, budget {allowed}"
        );
        assert!(agent.position.0 < 6602.0 + 5000.0, "and not the whole jump");
    }

    #[test]
    fn a_far_plane_change_is_refused_and_a_near_one_is_taken() {
        let now = Instant::now();
        let mut near = agent();
        let later = now + Duration::from_millis(100);
        assert_eq!(
            near.adopt_report((6602.0, 4485.0), 4, later),
            Adopted::Accepted,
            "a plane change in place is stairs or a small portal"
        );
        assert_eq!(near.plane, 4);

        let mut far = agent();
        assert_eq!(
            far.adopt_report((6602.0 + 900.0, 4485.0), 4, later),
            Adopted::Refused,
            "and a plane change far away is not"
        );
        assert_eq!(far.plane, 0, "the refusal leaves the agent where it was");
    }

    #[test]
    fn a_correction_is_short_on_the_same_plane() {
        let mut fresh = agent();
        assert!(fresh.adopt_correction((6602.0 + 50.0, 4485.0), 0));
        assert_eq!(fresh.position, (6652.0, 4485.0));

        let mut other = agent();
        assert!(
            !other.adopt_correction((6602.0 + 400.0, 4485.0), 0),
            "too far"
        );
        assert!(!other.adopt_correction((6602.0, 4485.0), 3), "wrong plane");
        assert_eq!(other.position, (6602.0, 4485.0));
    }

    #[test]
    fn the_first_report_of_a_session_gets_the_full_budget() {
        // A client that stood still through the load then reports a position is not cheating: the
        // reference gives the first report the maximum elapsed time, and so does this.
        let mut agent = agent();
        assert_eq!(
            agent.adopt_report((6602.0 + 500.0, 4485.0), 0, Instant::now()),
            Adopted::Accepted
        );
    }

    fn waypoint(x: f32, y: f32, trap_id: u32) -> Waypoint {
        Waypoint {
            pos: Pos { x, y, plane: 0 },
            trap_id,
        }
    }

    #[test]
    fn a_walk_starts_at_the_paths_first_waypoint() {
        let mut walker = agent();
        let started = walker
            .begin_walk(vec![
                waypoint(6702.0, 4485.0, 1),
                waypoint(6802.0, 4485.0, 2),
            ])
            .expect("a walk with waypoints starts");
        assert_eq!(started.trap_id, 1, "the reference takes waypoint 0");
        assert_eq!(walker.motion.destination.x, 6702.0);
        assert_eq!(
            walker.motion.speed, 288.0,
            "and starts moving at speed_base"
        );
        assert_eq!(walker.motion.direction, (1.0, 0.0), "pointing at it");
        assert_eq!(walker.motion.waypoint_idx, 1, "with the rest still to walk");

        // An empty path is the reference's `0 < agent->waypoints.len` check: nothing starts.
        let mut empty = agent();
        assert!(empty.begin_walk(Vec::new()).is_none());
    }

    #[test]
    fn the_tick_walks_the_segment_and_hands_over_to_the_next_waypoint() {
        // 288 units a second, half a second at a time: 144 units a tick, and what is left of a tick after
        // a waypoint is spent on the next segment -- which is what the reference's `dist -=` does.
        let mut walker = agent();
        walker.begin_walk(vec![
            waypoint(6602.0 + 200.0, 4485.0, 1),
            waypoint(6602.0 + 500.0, 4485.0, 2),
        ]);

        assert_eq!(walker.advance(0.5), None, "still on the first segment");
        assert!((walker.position.0 - (6602.0 + 144.0)).abs() < 0.01);

        // The second tick covers the 56 units left to the first waypoint and spends the other 88 on the
        // segment after it, taking that waypoint as the new destination as it goes.
        let next = walker.advance(0.5).expect("a waypoint was crossed");
        assert_eq!(next.trap_id, 2);
        assert_eq!(walker.motion.destination.x, 6602.0 + 500.0);
        assert!(
            (walker.position.0 - (6602.0 + 200.0 + 88.0)).abs() < 0.01,
            "position {} after the overshoot",
            walker.position.0
        );
    }

    #[test]
    fn arriving_ends_the_walk() {
        let mut agent = agent();
        agent.begin_walk(vec![waypoint(6602.0 + 100.0, 4485.0, 1)]);
        // 100 units at 288 a second is inside one tick.
        assert_eq!(
            agent.advance(0.5),
            None,
            "the last waypoint is the destination"
        );
        assert_eq!(agent.position, (6602.0 + 100.0, 4485.0));
        assert_eq!(agent.motion.speed, 0.0, "and the walk is over");
        assert!(agent.motion.waypoints.is_empty());
        assert_eq!(agent.motion.destination, Pos::default());
        assert_eq!(agent.advance(0.5), None, "a standing agent does not move");
    }

    #[test]
    fn a_walk_to_where_the_agent_stands_does_not_poison_its_direction() {
        // The reference divides by the distance here without checking it, so this exact case gives it a
        // NaN direction and then a NaN position. This keeps the direction it had and ends the walk on the
        // next tick instead -- the one deliberate difference in the movement code.
        let mut walker = agent();
        walker.begin_walk(vec![waypoint(6602.0, 4485.0, 1)]);
        assert_eq!(
            walker.motion.direction,
            (0.0, 0.0),
            "unchanged (nothing to point at yet), not NaN"
        );
        walker.advance(0.5);
        assert_eq!(walker.position, (6602.0, 4485.0));
        assert_eq!(
            (walker.motion.speed, walker.motion.destination),
            (0.0, Pos::default()),
            "the walk is over immediately"
        );
        assert!(walker.position.0.is_finite() && walker.position.1.is_finite());
    }

    #[test]
    fn cancelling_stands_still_without_forgetting_the_path() {
        // `GameSrv_CancelAgentMovement` sets the destination to the position and the speed to zero; the
        // waypoint list is the caller's business, and `MOVE_TO_COORD` clears it first.
        let mut agent = agent();
        agent.begin_walk(vec![
            waypoint(6802.0, 4485.0, 1),
            waypoint(6902.0, 4485.0, 2),
        ]);
        agent.cancel_movement();
        assert_eq!(agent.motion.speed, 0.0);
        assert_eq!(agent.motion.destination.x, 6602.0);
        assert_eq!(agent.advance(0.5), None, "and nothing moves");
        assert_eq!(agent.position, (6602.0, 4485.0));
    }
}
