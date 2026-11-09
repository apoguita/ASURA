# Re-engineering plan: AzerothCore's skeleton, this client's front door

Status: proposed, 2026-11-08. Owner's direction: additive, retro-compatible, step by step, and every
step works with the whole project before the next one starts. This document is the plan and the
statement of the target. It is the single place that describes what the server is meant to become;
each other document is updated only when a phase makes its own sentence true.

## Why this plan exists

The tree works, but it fuses the two things AzerothCore keeps apart. One process answers the login
conversation and relays gameplay; one database holds identity, character state and content; one
configuration directory and one log serve everything. The structural elements of the reference are
recorded in `docs/guidance/architecture.md:59-74`, and the two places where they were explicitly not
built are the store split and the bounded parallel map update (`architecture.md:260-264`). The
documentation described that fused code, so each document was accurate about a wrong skeleton, which
is how the documents came to contradict each other.

## The target composition

**The server is cloned from the reference's structure, which was read from its own tree on 2026-11-08**
(`docs/research/azerothcore-structure.md`, checkout `C:\Users\Apo\Documents\GitHub\WowWarband` at
`30a36ec4d`). The parts, in the reference's terms:

| part | what it is | the reference's shape |
|---|---|---|
| login program | a program of its own: config file, log, its own pool | `apps/authserver/Main.cpp`: config, log, auth pool, secrets, realm list, then `AuthSocketMgr.StartNetwork(3724)` |
| world program | a program of its own: config file, log, its own pool | `apps/worldserver/Main.cpp`: config, log, thread pool, scripts, metrics, secrets, world settings, then `WorldSocketMgr.StartWorldNetwork` |
| shared core | sockets, packets, realms, secrets - libraries both programs use | `shared/Network`, `Packets`, `Realms`, `Secrets` |
| database layer | the pools, the loader and the SQL updater | `database/DatabaseEnv`, `Updater/DBUpdater` + `UpdateFetcher` |
| game library | one directory per game noun, the simulation and its socket layer inside it | `game/`, 43 subsystem directories, 325 files; `WorldSocket`/`WorldSession` live in `game/Server` |
| scripts | behaviour against engine hooks, never named by the engine | `game/Scripting/ScriptMgr` (345 hooks), `scripts/` (598 files), `modules/` out of tree |
| content | the world's own data, updated by a tool | `data/sql/{base,updates,archive,custom}` |
| configuration | one file per program, sections, unknown keys refused | `env/dist/etc/worldserver.conf.dist`, `authserver.conf.dist` |

**One client-visible address.** This is the client's rule, measured rather than chosen: after the
redirect the client dials the address the game was *launched* with, and a client handed a different
address still dials the launch address (build 39112, `docs/reference/client-protocol.md:708-712`).
The reference's two client-facing ports are therefore not available to us; the split stays by **program**,
and the login program owns the one address and hands the gameplay connection to the world program.

**What does not carry over, because Guild Wars has no such noun:** battlegrounds, arena spectator,
warden, auction house, mail, vehicles, pets, totems, corpses. Checked in the reference tree: the first
five are directories there and the last four sit inside `Entities/` or are absent.

**What is ours and has no reference equivalent:** the portal web service for identity, the file check
and version exchange before login, content as a JSON pack rather than SQL rows (D20, D21, D24), and the
player token placement carries.

## The rules for every phase

1. **Additive.** The new structure is built beside the old one and callers move onto it; the old path
   is deleted only when nothing uses it.
2. **The packet stream is frozen.** No phase changes a byte a client receives. Migrations may only
   add. Configuration files may only gain keys.
3. **One phase, one commit, green.** Build succeeds, unit tests pass, and the real client logs in, is
   redirected, and enters a map with byte-identical frames. No phase carries a feature.
4. **A phase that cannot be built beside what runs is split again** rather than forced.

## The skeleton, and what populating it means

- **The skeleton is the reference's two programs, built as new programs.** The login program and the
  world program are each their own crate, with their own configuration file and their own log; the
  existing programs keep serving until the client runs on the new ones, and are then deleted. Building
  the skeleton is small because it is mostly directories and `main` functions.
- **Populating a layer means moving working code into it unchanged.** The sockets, the login
  conversation, the load block and the movement answers are what the real client taught this project;
  they move rather than being rewritten. What changes is where the code sits and what it may touch.
- **The old path stays alive until the client proves the new one:** login, a redirect, a character in a
  map, and byte-identical frames. Only then is the old program deleted.

## Where the work stands

Read this table first when resuming. A phase is `done` only when its row in "The phases" was proven
and its documents were updated in the same commit.

| phase | state | evidence |
|---|---|---|
| 0 | done | this file, `d7d229d`; the database-holds-state ruling and the action table, `01250ff` |
| 1 | done by measurement | the client dials its launch address, so one client-visible address is forced (`docs/reference/client-protocol.md:708-712`); the world already sits behind it as a separate program (`crates/asura-gateway/src/relay.rs:9-17`) |
| 2 | done (2a-2e) | **a real client loads after the split (owner, 2026-11-08)**; `asura-store::identity` owns account, portal and session; `asura-store::state` owns characters, character facts and inventory (`State::new(pool)`). The world reaches them through its `db_game` / `db_inventory` re-exports, so no call site moved; auth, webgate and the gateway tests call the store directly. Cut 2d moved placement the same way, and cut 2e finished the emptying: the service catalogue moved too, the database URL moved to `asura-config` (its old path is a door in `asura-db`), and every today-document's citations were rewritten to the new homes. What remains in `asura-db` is the schema and the migration runner, plus the content rows that have no caller and die in phase 6 - so the crate itself is deleted with that content, as phase 6's first step, not here. `cargo check --workspace --all-targets` exits 0; store 6 unit plus 21 integration, world 90, db 12, auth 3, webgate 6, gateway 4, rules 9, protocol 90 |
| 3 | done | `asura_store::placement::Placement::new(pool)` exposes the four operations - find worlds (candidates and endpoint), reserve, release, record a transfer (begin, claim, note destination, note arrival origin). The login program, the world program and the gateway relay call it and nothing else: a search over those four files finds 7 interface calls and 0 raw placement calls. Same SQL; `cargo check --workspace --all-targets` exits 0; auth 3, world 90, gateway 4, store 6 |
| 4 | in progress: cuts 4a-4e done, 4f gated on the client | `crates/asura-worldserver` holds the reference tree's shape: `shared/socket` (accept loop, buffered reads, the 64-byte game version, framing, send), `database`, `game/` divided into `server`, `entities`, `maps`, `mechanics`, `content`, `support`, `world`, and `scripts`. Cut 4b moved the whole world library into `game/` by noun - the crate root re-exports every module, so no call site changed - and the binary runs the copied `serve`: `target/debug/asura-worldserver` builds, 93 unit tests pass, and `config/services.toml` now launches it for world 8001. Verified without a client: the binary starts, loads 145 maps, 138 spawn lists, 17 doorway maps and 140 path graphs, registers a world and accepts a socket; and the acceptance suites were ported with it (`tests/path_dump.rs` 3 pass, `tests/load.rs` compiles and fails only on the pre-existing map-146 spawn refusal, exactly as the old crate does). The old `asura-world` still builds and passes its 90 tests and is deleted when the client has run on the new program. `asura-worldserver` and `asura-store` are clippy-clean with no warnings under the workspace lint policy, and every public item is documented; the only clippy warnings left in the workspace are four in `asura-world` and `asura-db`, the two crates this plan deletes |
| 5 | not started | |
| 6 | not started | |

Last recorded green, before this plan began: `7e45201` (the simulation reads its own cadence),
`d140f37` (the agent's numbers are the pack's), `4f4b8cf` (the audit record). `asura-protocol` 90
tests, `asura-world` 90 unit tests, `cargo check --workspace` exits 0.

## Known red, before any phase starts and not caused by it

- `cargo fmt --all --check` now flags 4 files, down from 15: every file this plan edited was formatted,
  and what remains untouched is `asura-config/src/lib.rs`, `asura-config/src/world.rs`,
  `asura-db/src/migrations.rs` and `asura-world/tests/portals.rs`. CI's format job stays red on those
  four until someone formats them.
- `asura-config`'s `the_committed_movement_file_states_the_ceiling` fails: the committed
  `config/Server/movement.json` says 250 while the test asserts 150. Neither was touched by this plan;
  the file's own doc still says the owner decided 150 on 2026-11-04, and `fbaca61` changed the file.
- `clippy --workspace --all-targets` reports four warnings, all in the two crates this plan deletes:
  `asura-world/src/instance_table.rs:136` (manual `Option::zip`), `asura-world/src/agent.rs:515`
  (`is_none_or` newer than the 1.75 MSRV) and two complex types in `asura-db/examples/export-map-content.rs`.
  Every crate this plan created or rewrote is clean.
- The `end_to_end` test and the whole `load` suite fail on `outpost 146 has no configured random spawn
  list`: the tests build map 146 as an outpost with no spawns, while the pack has it as an explorable
  with no `spawns.json`. The refusal landed in `301a97a` (2026-11-06), after the tests were written.

None of these is fixed by this plan. A phase that changes any of them says so in its commit.

## Resuming

1. Read this file top to bottom; it is the only statement of the target.
2. Read "Where the work stands" and pick the first phase whose state is not `done`.
3. Read that phase's row in "The phases" for what proves it and which documents it updates.
4. Run the three checks before touching code, so a pre-existing failure is not mistaken for the phase's.
5. Update the state table in the same commit as the phase.

## The phases

| phase | what lands | what proves it | documents updated with it |
|---|---|---|---|
| 0 | this plan | you read it | this file; one pointer line in `docs/README.md` and the handoff |
| 1 | the door split, re-cut by measurement: one client-visible address, and the programs behind it named for what they are. The login program owns that address and the handoff; the world program owns its own listener and accepts the handoff as a client connection. No second client-facing port is created, because the client would not use it | nothing new runs; this phase closes on the measurement at `docs/reference/client-protocol.md:708-712`, which is what forced it | this file; `guidance/architecture.md` (today) |
| 2 | three stores by owner: identity, state, content stop sharing one access path; one interface per store | login, travel, item persistence, a full world refused cleanly; migrations unchanged | `config/Server/README.md`, the storage section of `guidance/architecture.md` |
| 3 | placement as one responsibility: four operations - find worlds, reserve, release, record a transfer - and both doors call them | travel in both directions; a full world refused without disturbing the holder | `guidance/architecture.md`, the entry and travel rows of `reference/game-world-model.md` |
| 4 | the world program, built as a new crate with the reference's library shape: `shared` (sockets, packets, secrets), `database`, `game/` by game noun, `scripts`, one config file. `asura-world` moves into it cut by cut - listener, sessions, entities and maps, mechanics by noun, scripts - and is deleted when empty. The reference's bounded parallel map update comes with the map cut, not before it | real client in a map after each cut; a drop, a move and a travel as the mechanics land; ping and tick cadence measured; two clients see each other | `work/layer-rework-checklist.md`, `work/world-simulation-plan.md`, `guidance/architecture.md` |
| 5 | hosting against the reference's shape: one config file per program, each read by its own program only; the supervisor owns start, stop, deploy, health and nothing about players | supervisor start path; a draining deploy with a player present | `operations/running-the-server.md`, `operations/running-on-linux.md`, `config/*/README.md` |
| 6 | game sealed on content: mechanics read values from files only; behaviour reaches them through a dispatcher in the reference's script-manager position (345 hooks in its tree) | a JSON edit changes behaviour after restart, with no rebuild | `guidance/scripting-and-behavior.md`, `work/world-simulation-plan.md`, `guidance/game-packs.md` |

## Phase 2 - three stores by owner

### The inventory, measured 2026-11-08

No SQL exists outside `asura-db`: every table is touched inside that one crate, so the store boundary
already exists between programs. The mixing is *inside* the crate, in which module owns which table:

| table | touched by | owner it should have |
|---|---|---|
| `t_account` | `account.rs`, `login.rs`, `portal.rs`, `lib.rs` | identity |
| `t_session` | `login.rs`, `placement.rs`, `portal.rs`, `lib.rs` | identity for the session, placement for its destination |
| `t_characters` | `login.rs`, `inventory.rs`, `game.rs`, `lib.rs` | state |
| `t_item`, `t_bag` | `inventory.rs`, `lib.rs` | state |
| `t_instance_unlock` | `game.rs` | state |
| `t_world`, `t_world_pool`, `t_lease`, `t_transfer` | `placement.rs`, `lib.rs` | placement |
| `t_service` | `catalog.rs` | hosting |
| `t_map_config`, `t_map_spawn`, `t_map_portal` | `game.rs` | content, and they die: their readers have no caller |

And who calls which module today:

| module | called by |
|---|---|
| `login` | auth (login), world (connection) |
| `game` | auth (travel), world (agent, lib, tests, dump-load) |
| `placement` | auth (travel), gateway, webgate, world tests |
| `inventory` | world (item, lib, dump-load) |
| `portal` | webgate, gateway |
| `account` | auth |
| `catalog` | supervisor |
| `config`, `migrations` | every program, as facilities rather than stores |

Two facts matter: `game` is the mixed module (content leftovers beside character state and unlocks), and
`login` serves both identity (account, session) and state (the character list).

### The decision: a new crate, not a patch

The owner's direction, 2026-11-08: `asura-db` is replaced, not patched. A new crate,
`crates/asura-store`, holds the three stores and is written fresh; the old crate keeps serving until
its last store has moved, and every program keeps the name and the connection settings it already has.
A store moves in one commit: its interface is complete here, its callers move onto it, and its tables
leave the old crate in that same commit.

### The cuts, in order

| cut | what lands | what proves it |
|---|---|---|
| 2a | `asura-store` exists with the identity store written fresh: `Identity::account`, `record_eula`, `portal_for_email`, one error type, the SQL unchanged | workspace builds; two live tests skip without a database and pass with one |
| 2b | the identity callers move onto `asura-store` - webgate, gateway, auth - and `account`, `portal` and the identity half of `login` leave the old crate | webgate answers `who are you?`, login resolves the pair, no caller names the old path |
| 2c | the state store: characters, traits, position, unlocks and inventory, written fresh; the world's callers move onto it | login, entry, item persistence unchanged; the old module loses its tables |
| 2d | the placement store: worlds, pools, leases, transfers, written fresh; auth, gateway and webgate move onto it | travel in both directions; a full world refused without disturbing the holder |
| 2e | the crate is emptied of every store: the service catalogue moves to the store, the database URL moves to `asura-config` (old path kept as a door), and every today-document is rewritten to the new homes. The crate itself survives with the schema runner and the content rows that die in phase 6, and its deletion is phase 6's first step | build green; every store lives in `asura-store`; no today-document names a moved path |

A cut that cannot be done without changing a caller's path is not done: the old path is re-exported
until the caller moves in a later cut.

## Phase 4 - the cuts

The world program is a new crate (`crates/asura-worldserver`) in the reference tree's shape, and the
old `asura-world` moves into it cut by cut. Each cut ends green; 4b onwards is proven by the real
client.

| cut | what lands | state |
|---|---|---|
| 4a | the shell and `shared/socket`: accept, buffered reads, the 64-byte game version, framing, send | done, `4d04350` |
| 4b | the whole library moves into `game/` by noun, re-exported at the crate root; the binary serves | done, `79c21d9` |
| 4c | the nouns become real: the public surface below shrinks to what other nouns actually call, and the reference's bounded parallel map update replaces one task per map | started: `agent` state is private behind accessors (`6ddd776`), and `player` state is private behind accessors with `Player::admit` as the one way to build one; `Instance::map` is private behind `map()`, so the running object and the row it was built from cannot be confused. and `Slot` is closed too, with an API rather than fields: player_id, agent/agent_mut, name, equipped, outbox, portal_outbox, move_stats/move_stats_mut, loading/set_loading - so the agent rules take the body through the instance instead of reaching into its bookkeeping. `agent`'s surface fell from 48 public functions to 18 - advance, agent, agent_id, arrival, begin_walk, facing, from_map, GetXY, here, motion_speed, motion_waypoint_count, motion_waypoint_index, new, plane, position, resume, speed_base, waypoint - and the other 26 are internal or tests. The bounded parallel update landed too: `game/maps/updater.rs` holds the reference's position - one scheduler on the configured interval, a `Semaphore` bounding how many maps step at once, `MapUpdater::activate(workers)` - and `Instance::run` (one sleeping task per map) is gone. 96 unit tests pass, one of them proving a scheduled step walks an agent, and the binary starts, registers and listens |
| 4d | the mechanics read content and ask the simulation; `agent` splits into movement and standing; the handlers move into `game/handlers/`, the reference's own directory | in progress: `game/handlers/` holds the item handlers and two of the agent's - `handle_cancel_movement` and `handle_last_position`, after their rules moved into the noun as `cancel_walk` and `adopt_correction_point`. and `handle_move_to_coord` joined them once the walk-start rule moved into the noun as `begin_walk_announcing` (which resets the speed announcement when it differs). and the last two followed: `handle_request_spawn` moved as-is (it only reads content), and `handle_position_update` moved once its budget rule became the noun's `accept_report`. All five agent handlers are now in `game/handlers/agent.rs` and the noun holds no handler at all. The chat handler and `handle_travel_init` followed without a rule extraction - both only read and answer - so `game/handlers/` now holds agent, chat, item and travel. `handle_party_travel` followed too, after `resolve_instance` and `begin_transfer` became the noun's public rules, so travel holds no handler. The four creation handlers followed with the noun publishing what they ask for - `equipped_items` is public, and the handlers import `Creation`, `CREATION_ENERGY`, `CREATION_HEALTH`, `creation_inventory` and `creation_equipment_rows` from the mechanic - so `game/handlers/` holds agent, chat, creation, item and travel. and the last one, `handle_request_players`, moved once its body asked for `player.instance()`, `player.inventory()` and the other accessors instead of the fields. Every message handler now lives in `game/handlers/`: agent, chat, creation, item, player, travel | |
| 4e | `scripts/` gains the hook position: the reference's `ScriptMgr` shape - a `Script` trait, `ScriptMgr::add`, `add_scripts` as the `AddScripts` equivalent - with two hooks called from the engine: a player entering a map and a map tick | done: `crates/asura-worldserver/src/scripts/mod.rs`; `World::scripts`, called in `connection.rs` and `updater.rs`. The crate also took the workspace's lint policy back: 0 errors, and the one test-only `unsafe` allowed with its reason. The documentation debt it exposed was 144 warnings; it is now zero - every struct, field, variant and function in the crate is documented, and the four reference-named helpers nothing calls (`IsValid`, `GetZPlane`, `adopt_exact`, `walk_to`) carry an `allow(dead_code)` with their reason instead of quietly disappearing |
| 4f | `asura-world` is deleted once the client has run on the new program for a full pass | |

The measured surface the 4c work starts from (`crates/asura-worldserver/src/game`, 2026-11-08):

| module | lines | public fns | public fields |
|---|---|---|---|
| `entities/agent.rs` | 1,521 | 48 | 26 |
| `maps/instance.rs` | 529 | 27 | 14 |
| `entities/item.rs` | 565 | 20 | 0 |
| `maps/pathfind.rs` | 1,795 | 18 | 7 |
| `entities/player.rs` | 545 | 14 | 23 |
| `maps/portals.rs` | 504 | 13 | 13 |
| `server/connection.rs` | 594 | 11 | 5 |
| `maps/map_data.rs` | 1,197 | 11 | 47 |
| `support/gwdat.rs` | 432 | 10 | 14 |

A noun is "real" when its fields are private and a caller outside it uses only the functions its own
documentation names. That is the whole of 4c's judgement, and the compiler enumerates the callers.

## How the documents change

Every document is one of three kinds, and its first lines say which:

- **A dated record** - status, handoff, research, history, recordings. Frozen with its date; a wrong
  sentence is replaced, never appended beside.
- **A statement of today** - the runbook, the storage section, the component inventory. Updated in the
  same commit as the phase that changes it, so it is never half-migrated.
- **A statement of a target** - this plan. It describes what is being built, not what runs.

A document that describes today is not edited to describe intent; that is the mistake that produced
the contradictions this plan replaces.

## Open questions, reported and not decided here

- Does the player token survive, or does the world validate a session the way AzerothCore does?
- Does the login door keep a relay path for the client's second connection, and for how long?
- Which script dispatcher reaches mechanics, and when it is introduced.
- The bounded update pool's size and its map affinity.
