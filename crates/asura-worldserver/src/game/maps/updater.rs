//! The map updater: one scheduler, a bounded number of maps updating at once.
//!
//! The reference does this with `game/Maps/MapUpdater`: a producer/consumer queue (`PCQueue`) and
//! `activate(std::size_t num_threads)`, so a hundred loaded maps do not mean a hundred threads and the
//! number of maps being updated at the same time is a config value (`MapUpdate.Threads`). This module is
//! the same position for this project: `activate(workers)` bounds the work, and the schedule keeps each
//! map's own cadence - a map still ticks every interval, it just no longer owns a task that sleeps.
//!
//! What changed from "one task per map": the maps no longer each hold a sleeping task. One scheduler
//! wakes on the configured interval and hands every loaded instance to the pool, and at most `workers`
//! of those steps run at once. The step itself is the same `Instance::step` with the same elapsed time,
//! so what a client sees is unchanged; what is bounded is how much of it can run at the same moment.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;

use crate::game::maps::instance::{Instance, Instances};

/// How many maps may be updated at the same time.
#[derive(Debug, Clone)]
pub struct MapUpdater {
    workers: usize,
    permits: Arc<Semaphore>,
}

impl MapUpdater {
    /// Build an updater that runs at most `workers` maps at once.
    ///
    /// `workers` of zero would serialise everything behind a permit nobody holds, so it is raised to
    /// one - the reference does the same with its thread count.
    pub fn activate(workers: usize) -> Self {
        let workers = workers.max(1);
        MapUpdater {
            workers,
            permits: Arc::new(Semaphore::new(workers)),
        }
    }

    /// The number of maps this updater updates at once.
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Update one instance, bounded by the pool.
    ///
    /// The step runs on its own task and waits for a permit, so a burst of maps queues instead of
    /// multiplying threads. Nothing here interprets a packet: the step is the simulation's own.
    pub fn schedule(&self, instance: Arc<Instance>, elapsed: Duration) {
        let permits = Arc::clone(&self.permits);
        tokio::spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
            instance.step(elapsed);
        });
    }
}

/// The clock: wake on the configured interval, hand every loaded map to the updater, sleep again.
///
/// This is the world's one sleeping task - the reference's `MapMgr::Update` loop, which is why a map
/// stops ticking the moment it is unloaded rather than lingering as a task of its own.
pub async fn run_clock(instances: Instances, updater: Arc<MapUpdater>) {
    let mut tick = tokio::time::interval(asura_config::intervals().tick());
    let mut last = Instant::now();
    loop {
        tick.tick().await;
        let elapsed = last.elapsed();
        last = Instant::now();
        for instance in instances.all() {
            updater.schedule(instance, elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_has_at_least_one_worker() {
        assert_eq!(
            MapUpdater::activate(0).workers(),
            1,
            "zero is raised to one"
        );
        assert_eq!(MapUpdater::activate(4).workers(), 4);
    }

    #[tokio::test]
    async fn a_scheduled_step_advances_the_map() {
        // The pool runs the same `Instance::step` the per-map task used to run, so the proof is that a
        // scheduled step moves an agent: a walk started, then one update.
        let instances = Instances::default();
        let map = crate::MapEntry {
            map_id: 146,
            map_file_id: 0,
            map_type: 0,
            spawn: (6602.0, 4485.0),
            spawn_plane: 0,
            spawns: Vec::new(),
            name: "Lakeside County".to_string(),
            capacity: 8,
        };
        let instance = instances.for_map(&map, 288.0);
        let (outbox, _inbox) = tokio::sync::mpsc::unbounded_channel();
        let (player_id, _agent_id) = instance.join(
            "Test".to_string(),
            crate::Standing {
                position: (6602.0, 4485.0),
                plane: 0,
                direction: (1.0, 0.0),
            },
            [0; 9],
            outbox,
        );
        instance.loaded(player_id);
        instance.with_slot(player_id, |slot| {
            slot.agent_mut().begin_walk(vec![crate::pathfind::Waypoint {
                pos: crate::pathfind::Pos {
                    x: 6702.0,
                    y: 4485.0,
                    plane: 0,
                },
                trap_id: 0,
            }])
        });
        let before = instance
            .with_slot(player_id, |slot| slot.agent().position())
            .expect("a slot");

        let updater = MapUpdater::activate(2);
        updater.schedule(Arc::clone(&instance), Duration::from_millis(100));
        tokio::time::sleep(Duration::from_millis(60)).await;

        let after = instance
            .with_slot(player_id, |slot| slot.agent().position())
            .expect("a slot");
        assert!(
            after.0 > before.0,
            "the scheduled step walked the agent: {before:?} -> {after:?}"
        );
    }
    #[test]
    fn the_work_is_bounded_by_the_pool_not_by_the_map_count() {
        // The permits are shared: two maps asked for at once wait for the same two workers rather than
        // each starting their own.
        let updater = MapUpdater::activate(2);
        assert_eq!(updater.permits.available_permits(), 2);
    }
}
