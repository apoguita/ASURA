# Project status

Feature implementation and client acceptance. Dated results apply to the tested behavior and build;
they do not establish acceptance of later changes or full packet parity.

`[x]` a real client did the named check. `[o]` an automated check is recorded, no client confirmation.
`[~]` implementation is recorded, but verification or owner acceptance is incomplete. `[ ]` not done.

## Client

- [x] A stock `Gw.exe` is patched into `Gw.custom.exe` (`client-patcher/patch-gw.py`; rebuild is byte identical, 2026-10-28)
- [x] The patched client runs beside another copy, mutex check removed
- [x] The client asks the web gate who it is, four `/Spawned/WebGate` endpoints (`docs/reference/client-protocol.md` #4)
- [x] Login: key exchange, session, character list (`docs/reference/captures/login-session-2026-10-26.md`)
- [x] EULA handling: accepted once per account, not on every login (2026-10-28, second login asked nothing; the cited log has rolled). Current write path: `crates/asura-auth/src/lib.rs`, `AUTH_CMSG_ACCEPT_EULA`; the code location is not a new client test
- [x] The client reaches character select and enters an outpost (build 39112)

## Character

- [x] Position persistence was recorded; `crates/asura-store/tests/inventory.rs::a_position_is_saved_and_read_back_and_a_new_character_has_none` checks storage. Current entry selection is described in [Architecture](../guidance/architecture.md#inside-a-world-server) and the [pending placement repair](#pending-client-verification), not a promise to resume saved coordinates on every entry
- [x] Items: block built from rows; drops, moves and equipment persist
- [x] Chat on `!`, `#`, `$`; `/stuck` and `/maps` answer
- [o] Character creation: name, appearance, profession, starting equipment (`crates/asura-world/tests/load.rs::a_character_is_created_in_a_creation_instance`)
- [~] Character deletion by name, scoped to the owning account (`crates/asura-auth/src/lib.rs:405-424`); no client confirmation recorded
- [x] Character selection message `AUTH_CMSG_CHANGE_PLAY_CHARACTER` (`crates/asura-auth/src/lib.rs`, matching handler; a real client selected "Asura One", `logs/auth.log` 2026-10-30)

## Maps and movement

- [x] A world hosts several maps in one process
- [x] Travel: request, redirect, reconnect into the destination spawn
- [x] Destination loaded on demand under `--on-demand`, measured at 0-2 ms per map against the real client; without the flag a world builds its hosted catalogue at start-up (current source: [World pack files](../../config/World/README.md)), which is the shipped default (`config/services.toml` passes no flag)
- [x] Click-to-walk answers with pathfound waypoints
- [x] The reported Ascalon City bridge alternation test passed on 2026-11-01; this is dated route-specific evidence, not acceptance of subsequent movement changes ([recorded test](../history/infrastructure-milestones.md), "Alternating movement, client-confirmed fixed")
- [~] Manual movement remains provisional and is not accepted: on 2026-11-06 the owner reported "movement is better but not perfect" and "this feature implementation is not of my liking". The directional walker is unfinished; further functional work is paused ([session details](session-handoff.md#latest-gameplay-session-notes))
- [~] Refused click-to-walk sends nothing (`crates/asura-world/src/agent.rs`, `handle_move_to_coord`, `Walk::NoPath | Walk::NoGraph`). Its comment records earlier client failures after position broadcasts. The contradictory `crates/asura-world/tests/load.rs::a_walk_the_finder_refuses_is_answered_with_the_agents_position` remains ignored; it does not prove this behavior. The owner's ruling is still pending
- [x] Tick and movement cadence were checked against `recordings/client/feature/movement/movement-sniff.log`; this check does not establish movement or synchronization parity
- [o] Two clients in one map see each other (`crates/asura-world/tests/load.rs::a_second_player_is_introduced_to_the_first_and_withdrawn_again`)

## Services

- [x] Auth answers login, keeps sessions, refuses a travel cleanly
- [x] Gateway relays the game connection to the selected world ([connection ownership](../guidance/architecture.md#connections-placement-and-routing))
- [o] Placement reads the registry, heartbeats decide freshness, a stale world is not offered
- [o] Pool capacity refuses a full world without disturbing a lease (`crates/asura-store/tests/placement.rs::a_full_pool_refuses_without_disturbing_the_holder`)
- [o] The `reap_stale` database helper frees a silent world's leases in tests/harness scenarios; no serving-program loop calls it ([runtime limits](../guidance/architecture.md#the-lease-ledger); `crates/asura-store/tests/placement.rs::the_reaper_frees_a_silent_world_but_not_a_fresh_one`)
- [o] The front door dials a world registered at a second address, not its bind address
- [o] The newest build is the only one offered; a full newest build refuses instead of falling back (`crates/asura-store/tests/placement.rs::the_newest_build_is_the_only_one_offered`)

## Pending client verification

- [~] Outpost login placement repair, 2026-11-06: implemented; awaiting client confirmation ([session evidence](session-handoff.md#latest-gameplay-session-notes))
- [~] Northlands map-file and path-graph repair, 2026-11-06: implemented; awaiting client confirmation ([session evidence](session-handoff.md#latest-gameplay-session-notes))
- [~] Portal wall crossing repair, 2026-11-06: implemented; awaiting client confirmation ([session evidence](session-handoff.md#latest-gameplay-session-notes))

## Remaining feature and evidence gaps

- [ ] Client confirmation of starting-map selection for every campaign. Creation reads the selected pack's `character_spawn.json` through `spawns::current` and `for_campaign` (`crates/asura-world/src/creation.rs:433-466`); configured entries alone are not evidence that the client path is correct
- [ ] Verify creation-screen inventory parity against its capture. Current `creation::creation_inventory` combines non-equipped seed items, profession equipment and seed bags (`crates/asura-world/src/creation.rs:52-130`); the earlier "equipped set only" description is obsolete. This review does not re-establish the older live count of 60 items in 7 bags

- [ ] Pick up a dropped item (`0x803F`)
- [ ] Split a stack by hand (`0x8075`)
- [ ] Buy and sell (`0x804D`)
- [ ] Swap two items in one move
- [ ] Dedicated unequip message `0x804F`: parsed meaning and client behavior remain unverified; `crates/asura-world/src/item.rs::handle_unequip_item` logs and returns. Dragging equipment into a bag uses `ITEM_MOVE` and is a separate supported path (`crates/asura-world/tests/load.rs::an_item_can_be_taken_off_and_worn_again`)
- [ ] A player standing in an instance during a rolling deploy (real client)
- [ ] A travel to the second address (real client, `docs/research/multi-host-routing-experiment.md`)

## Operations

- [o] The supervisor starts, stops, restarts, builds, applies the schema and seeds (`docs/operations/running-the-server.md`)
- [o] Service lifecycle is recorded in the `t_service` registry (`crates/asura-supervisor/tests/supervisor.rs::a_healthy_child_is_ready_listed_stopped_and_its_port_freed`); world placement uses a separate registry ([ownership](../guidance/architecture.md#the-registry))
- [~] Rolling deploy: a second world runs beside the first and the old one drains when it empties - `asura-supervisor deploy-world --old 8001 --new 8002` (`crates/asura-supervisor/src/supervise.rs:525`); nothing in `crates/asura-supervisor/tests/` drives it
- [o] The stack runs as two containers (`docker/`, optional)
- [o] CI is configured to run format and lint on pushes and pull requests (`.github/workflows/ci.yml`); no new CI run was checked in this documentation pass

Recordings of a client proof live in `recordings/`, one row per file in `recordings/index.jsonl`.
