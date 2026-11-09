//! Behaviour against hooks: the engine publishes, content answers.
//!
//! The reference puts this at `game/Scripting/ScriptMgr`: a registry of scripts (`ScriptObject`
//! subclasses, registered by an `AddScripts` function the program calls at start-up) and 345 `On...`
//! methods that answer events - `OnPlayerLogin`, `OnMapUpdate`, `OnCreatureAddWorld`,
//! `OnSpellCheckCast` and the rest - without the engine ever naming a quest, a dungeon or a boss.
//!
//! This module is that position for this project, in the same shape:
//!
//! * [`Script`] is what content implements: one hook per event, each with an empty default, so a script
//!   answers only what it owns.
//! * [`ScriptMgr`] is the registry the program holds: it is filled by [`add_scripts`] at start-up and
//!   asked by the engine at the points below.
//! * [`add_scripts`] is the `AddScripts` equivalent: the one place that names what this build carries.
//!
//! What is wired today, and what that means: two hooks - a player entering a map, and a map's tick -
//! are called from the session and the clock. No script answers them yet, so each event costs one empty
//! loop. That is the position, deliberately cheap; hooks that reach mechanics arrive as the mechanics
//! they serve are published.

use std::time::Duration;

use crate::Uuid;

/// What a script may answer. Every hook has an empty body, so a script implements only what it owns.
pub trait Script: Send + Sync {
    /// A name for the logs, so a misbehaving script can be named.
    fn name(&self) -> &str {
        "script"
    }

    /// A player joined a map: the moment a script would welcome, arm or place it.
    fn on_player_enter(&self, _event: &PlayerEnter) {}

    /// A map's clock advanced: the moment a script would drive an encounter.
    fn on_map_tick(&self, _event: &MapTick) {}
}

/// A player entering a map: which character, where, and in which world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerEnter {
    /// The world the player entered.
    pub world_id: i32,
    /// The map it entered.
    pub map_id: i32,
    /// The connection's player id inside that map.
    pub player_id: u32,
    /// The character behind it.
    pub character: Uuid,
}

/// A map's clock advancing: which map, how long since the last tick, and how many players it holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTick {
    /// The map that advanced.
    pub map_id: i32,
    /// Time since its previous tick.
    pub elapsed: Duration,
    /// How many players are in it.
    pub players: usize,
}

/// The registry the program holds: what this build carries, and the way the engine asks it.
#[derive(Default)]
pub struct ScriptMgr {
    scripts: Vec<Box<dyn Script>>,
}

impl ScriptMgr {
    /// A registry with nothing registered.
    pub fn new() -> Self {
        ScriptMgr::default()
    }

    /// Register one script. The reference's `ScriptMgr::AddScript`.
    pub fn add(&mut self, script: Box<dyn Script>) {
        self.scripts.push(script);
    }

    /// How many scripts this build carries.
    pub fn count(&self) -> usize {
        self.scripts.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    /// The names of the registered scripts, for the start-up line.
    pub fn names(&self) -> Vec<&str> {
        self.scripts.iter().map(|script| script.name()).collect()
    }

    /// Ask every script about a player entering a map.
    pub fn on_player_enter(&self, event: PlayerEnter) {
        for script in &self.scripts {
            script.on_player_enter(&event);
        }
    }

    /// Ask every script about a map's tick.
    pub fn on_map_tick(&self, event: MapTick) {
        for script in &self.scripts {
            script.on_map_tick(&event);
        }
    }
}

impl core::fmt::Debug for ScriptMgr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScriptMgr")
            .field("scripts", &self.names())
            .finish()
    }
}

/// What this build carries. The reference's `AddScripts`.
///
/// Empty today: the world carries no content behaviour yet, and a script that answered a hook would be
/// behaviour this phase does not add. When the first one lands - a quest, an encounter, a custom skill -
/// it is added here and nowhere else.
pub fn add_scripts(_mgr: &mut ScriptMgr) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A script that records what it was asked, so a hook can be proven to reach content.
    struct Recorder {
        enters: Arc<AtomicUsize>,
        ticks: Arc<AtomicUsize>,
    }

    impl Script for Recorder {
        fn name(&self) -> &str {
            "recorder"
        }
        fn on_player_enter(&self, _event: &PlayerEnter) {
            self.enters.fetch_add(1, Ordering::Relaxed);
        }
        fn on_map_tick(&self, _event: &MapTick) {
            self.ticks.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn a_registered_script_is_asked_about_both_events() {
        let enters = Arc::new(AtomicUsize::new(0));
        let ticks = Arc::new(AtomicUsize::new(0));
        let mut mgr = ScriptMgr::new();
        add_scripts(&mut mgr);
        assert_eq!(
            mgr.count(),
            0,
            "this build carries no content behaviour yet"
        );
        mgr.add(Box::new(Recorder {
            enters: Arc::clone(&enters),
            ticks: Arc::clone(&ticks),
        }));
        assert_eq!(mgr.names(), vec!["recorder"]);

        mgr.on_player_enter(PlayerEnter {
            world_id: 8001,
            map_id: 146,
            player_id: 1,
            character: Uuid::nil(),
        });
        mgr.on_map_tick(MapTick {
            map_id: 146,
            elapsed: Duration::from_millis(100),
            players: 1,
        });

        assert_eq!(
            enters.load(Ordering::Relaxed),
            1,
            "the entry hook reached it"
        );
        assert_eq!(ticks.load(Ordering::Relaxed), 1, "the tick hook reached it");
    }
}
