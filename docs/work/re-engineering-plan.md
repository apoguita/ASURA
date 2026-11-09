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

**One client-visible address, several programs behind it.** This is the client's rule, measured rather
than chosen: after the redirect the client dials the address the game was *launched* with, and a client
handed a different address still dials the launch address (build 39112,
`docs/reference/client-protocol.md:708-712`). AzerothCore's two client-facing ports are therefore not
available to us, and the door split is by **program**, not by port: the login program owns the one
address and hands the gameplay connection to a separate world program, which is what the reference does
in the same position (`crates/asura-gateway/src/relay.rs:1-17`).

The client walks in through the login address, is told where to play, and walks in again at that same
address; the login program recognises the second connection and copies it to the world it names.

| part | what it is | owns |
|---|---|---|
| login program | `asura-gateway` plus `asura-auth` | the client's one address, file check, version, key exchange, account, character list, select, redirect, and the handoff of a gameplay connection |
| world program | `asura-world`'s listener and everything behind it | the gameplay connection, maps, instances, players |
| registry | the database row per world | which world serves a map, its capacity, its freshness |
| identity store | accounts, credentials, sessions | who this is |
| state store | characters, items, leases, transfers | what happened to whom |
| content | `config/World/<pack>/` files | what the world is made of |

Deliberate deviations from AzerothCore, each already decided: the client asks a portal web service who
it is before login; it begins with a file check and a version exchange; content is JSON files rather
than SQL (D20, D21, D24); placement carries a player token. Unresolved and not decided here: whether
the token survives, whether the login door keeps a relay switch, and how a script dispatcher reaches
mechanics.

## The rules for every phase

1. **Additive.** The new structure is built beside the old one and callers move onto it; the old path
   is deleted only when nothing uses it.
2. **The packet stream is frozen.** No phase changes a byte a client receives. Migrations may only
   add. Configuration files may only gain keys.
3. **One phase, one commit, green.** Build succeeds, unit tests pass, and the real client logs in, is
   redirected, and enters a map with byte-identical frames. No phase carries a feature.
4. **A phase that cannot be built beside what runs is split again** rather than forced.

## The skeleton, and what populating it means

- **The skeleton is two programs and the registry they share.** The login door is its own program,
  with its own configuration file and its own log; the world door is the existing world program, which
  stops being fed by a relay and accepts the client itself. Building the skeleton is therefore small.
- **Populating a layer means moving working code into it unchanged.** The sockets, the login
  conversation, the load block and the movement answers are what the real client taught this project;
  they move rather than being rewritten. What changes is where the code sits and what it may touch.
- **The old path stays alive until the client proves the new one:** login, a redirect to the world's
  own address, a character in a map, and byte-identical frames. Only then is the old door deleted.

## Where the work stands

Read this table first when resuming. A phase is `done` only when its row in "The phases" was proven
and its documents were updated in the same commit.

| phase | state | evidence |
|---|---|---|
| 0 | done | this file, `d7d229d`; the database-holds-state ruling and the action table, `01250ff` |
| 1 | done by measurement | the client dials its launch address, so one client-visible address is forced (`docs/reference/client-protocol.md:708-712`); the world already sits behind it as a separate program (`crates/asura-gateway/src/relay.rs:9-17`) |
| 2 | in progress: cut 2a done | `crates/asura-store` exists with the identity store written fresh (`Identity::account`, `record_eula`, `portal_for_email`); workspace builds, its two live tests pass; callers still on `asura-db`, which is next (2b) |
| 3 | not started | |
| 4 | not started | |
| 5 | not started | |
| 6 | not started | |

Last recorded green, before this plan began: `7e45201` (the simulation reads its own cadence),
`d140f37` (the agent's numbers are the pack's), `4f4b8cf` (the audit record). `asura-protocol` 90
tests, `asura-world` 90 unit tests, `cargo check --workspace` exits 0.

## Known red, before any phase starts and not caused by it

- `cargo fmt --all --check` flags 15 files this plan has not touched (`asura-auth/src/travel.rs`,
  `asura-config`, `asura-db`, and eight world files). CI's format job is red for that reason alone.
- The `end_to_end` test and the whole `load` suite fail on `outpost 146 has no configured random spawn
  list`: the tests build map 146 as an outpost with no spawns, while the pack has it as an explorable
  with no `spawns.json`. The refusal landed in `301a97a` (2026-11-06), after the tests were written.

Neither is fixed by this plan. A phase that changes either of them says so in its commit.

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
| 4 | the world's inside split: listener, sessions, simulation, mechanics as separate files with private data; the reference's bounded parallel map update replaces one task per map | real client in-world; ping and tick cadence measured; two clients see each other | `work/layer-rework-checklist.md`, `work/world-simulation-plan.md` |
| 5 | hosting: per-daemon configuration and logs; the supervisor owns start, stop, deploy, health and nothing about players | supervisor start path; a draining deploy with a player present | `operations/running-the-server.md`, `operations/running-on-linux.md`, `config/*/README.md` |
| 6 | game sealed on content: mechanics read values from files only; behaviour reaches them through a dispatcher in AzerothCore's script-manager position | a JSON edit changes behaviour after restart, with no rebuild | `guidance/scripting-and-behavior.md`, `work/world-simulation-plan.md`, `guidance/game-packs.md` |

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
| 2e | `asura-db` is deleted; `config` and the migration runner move to where their remaining users live | build green with the crate gone |

A cut that cannot be done without changing a caller's path is not done: the old path is re-exported
until the caller moves in a later cut.

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
