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
//! What this does *not* do is pathfinding. The reference walks the map's navmesh and hands the client
//! one waypoint at a time; with no navmesh here, the client's own destination is what comes back, and a
//! straight line can cut a corner. That is a limitation of this server, stated rather than hidden.

use std::time::{Duration, Instant};

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
        }
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
        let dx = other.0 - self.position.0;
        let dy = other.1 - self.position.1;
        (dx * dx + dy * dy).sqrt()
    }
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
}
