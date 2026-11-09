# Architecture

The structure is AzerothCore's (`docs/reference/decisions/0006-structural-design-authority.md`): it is the design
authority, and this document describes the structure we take from it. The instance-creation experiment and its recorded outcome are described in (`docs/reference/decisions/0005-gwps-reference-boundary.md`).

## Components

This is the component inventory for the project. The process/library split and connection routing
were checked against the entry points, workspace manifests and D25/D26 in
`docs/reference/decisions/layer-decisions.md`. The implementation descriptions below were checked on 2026-11-08. Intended behavior and
unresolved requirements are identified separately; this is not client acceptance.

| Component | Kind | Responsibility |
|---|---|---|
| Socket layer (`asura-gateway`) | Program | Owns the client-facing listener, file check, connection classification and gameplay relay. |
| Authentication (`asura-auth`) | Library called by gateway | Runs the login conversation: key exchange, identity lookup, character list/select, EULA and auth requests, including account-scoped character deletion through asura-db. |
| Rules (`asura-rules`) | Library | Supplies pure decisions over caller-provided values; auth currently calls its placement rule. |
| World server (`asura-world`) | Program | Hosts maps and running instances, claims gameplay transfers, and runs player movement, inventory, chat and gameplay persistence. |
| Web gate (`asura-webgate`) | Program | Answers the four portal HTTP endpoints and its health endpoint; the client obtains its portal identity here before auth checks it. |
| Supervisor (`asura-supervisor`) | Program | Starts, stops, restarts, builds and reports services configured in `config/services.toml`. |
| Stores (`asura-store`) | Library | The stores by owner: identity (accounts, sessions, portal pairs), state (characters, position, unlocks, inventory), placement (world registry, pools, leases, transfers) and the service catalogue. One interface per store; no decisions. |
| Schema (`asura-db`) | Library | The migration runner and the SQL that defines the schema, plus the content rows that are leaving for the pack (phase 6). It holds no store: every query an owner needs moved to `asura-store` in phase 2 of the re-engineering plan. |
| Protocol (`asura-protocol`) | Library | Defines client messages, codecs, connection classification and cryptographic exchange operations. |
| Configuration (`asura-config`) | Library | Reads server settings and the selected pack's shared map catalogue. |
| Fake world (`asura-fakeworld`) | Development harness program | Exercises registration, capacity and lease behavior; it is not a normal client-serving service. |
| PostgreSQL | Database service, managed outside the supervisor | Stores persistent operational/game state and the coordination rows shared by the programs. |
| Content/file service | Proposed external service; not implemented in this workspace | Would deliver game assets to the client. It is not the web gate or the world pack loader. |

The manifests in `Cargo.toml` identify the workspace members. `config/services.toml:29` lists the
web gate, `:36` names the gateway service `auth`, and `:43` starts the world. The service name
`auth` and its log filename do not mean that the `asura-auth` crate owns a listener.

### Connections, placement and routing

The client obtains a portal identity from the web gate, then opens its auth connection at the
gateway address. Gateway classifies the socket and calls `asura_auth::converse`
(`crates/asura-gateway/src/lib.rs:199`). The auth library speaks the login protocol; it has no
listener or independent service executable.

At login travel, auth passes character/request data to `asura_rules::placement`, then uses
`asura-store`'s placement store to reserve capacity and record the pending transfer
(`crates/asura-auth/src/travel.rs:165`, `:234`, `:251`). A travel started while playing is
handled by the current world, using the same database coordination mechanism. A placement decision,
a database reservation and socket routing are different responsibilities.

The client's separate gameplay connection also arrives at gateway. Gateway's game branch calls
`relay::route` (`crates/asura-gateway/src/lib.rs:195`), which finds the world's registered
endpoint, forwards the connection prefix and copies bytes between sockets
(`crates/asura-gateway/src/relay.rs:128`, `:135`). The world validates the pending transfer and
runs the gameplay conversation. Gateway does not become the world's identity authority by relaying it.

That one listener is the client's rule, not a design choice: after the redirect it dials the address the
game was launched with, and a client handed a different address still dials the launch address (build
39112, `docs/reference/client-protocol.md:708-712`). AzerothCore's two client-facing ports are therefore
not available here; the separation the reference has between its auth and world programs is kept by the
world being a separate program behind that address.

Auth and world coordinate state and placement through the database; the relay carries client traffic,
not a separate auth/world state protocol. World processes can be replaced independently of the auth
conversation, but stopping gateway drops the gameplay sockets it is carrying.

The structural correspondences and their reasons are recorded in
`docs/reference/decisions/0006-structural-design-authority.md`; D25/D26 record the rule and socket
boundaries. The correspondence is reference material, not a second component inventory here.

## The five structural elements

Structural direction from ADR 0006, with current implementation limits:

1. **Login and world simulation run in separate programs**, with no live state channel between them
   for placement or character state. The socket layer hosts the auth library.
2. **File-owned content and database-owned operational state.** The earlier three-database
   proposal included a SQL content domain; D20/D21/D24 replaced that content ownership. The
   implemented pack readers and layout are described once in
   [World packages](../../config/World/README.md). The world opens one PostgreSQL pool
   (`crates/asura-world/src/main.rs:157`); separate auth/game pools are not established here.
3. **A registry table** that world servers write themselves into; its implemented use is described below.
4. **Gameplay state written by world servers**, through database calls. The earlier asynchronous
   write-queue proposal is not an implemented worker: see Inside a world server below.
5. **Instance management inside the world server.** One task runs each used map today; the bounded
   update-thread pool remains planned.

### Storage ownership

**Owner clarification, 2026-11-08: auth must not write data.** The current persistent-write calls
listed below are implementation gaps, not exceptions granted because the code already uses them.
Calling a write operation through `asura-store` still makes auth initiate that mutation. Fixing or
reassigning these operations is outside the current documentation work; their eventual owner is
not selected here. Required recorded packet behavior must be preserved when that work is assigned.

| Write initiated by auth | Persistent effect | Current auth call site |
|---|---|---|
| Select a character | Updates `t_session.char_id`, during selection and login initialization | `crates/asura-auth/src/lib.rs:335`, `:648` |
| Delete a character | Clears session references and deletes the account-owned `t_characters` row | `crates/asura-auth/src/lib.rs:424`; storage operation `crates/asura-store/src/state/characters.rs:73-90` |
| Record EULA acceptance | Updates `t_account.eula_accepted` | `crates/asura-auth/src/lib.rs:482` |
| Reserve/release capacity | Changes `t_lease` and the pool's leased counters, including failure cleanup | `crates/asura-auth/src/travel.rs:234`, `:263` |
| Create a pending transfer | Inserts `t_transfer` with the issued player token and destination | `crates/asura-auth/src/travel.rs:251` |
| Record session destination | Updates `t_session.world_id` and `map_id` | `crates/asura-auth/src/travel.rs:268` |

`asura-store` owns the stores - identity, state, placement and the service catalogue - one interface
each, and it decides nothing. `asura-db` owns the schema and the migration runner, and its supervisor
path applies that SQL for setup; its seed command applies `tools/seed-dev.sql`
(`crates/asura-supervisor/src/main.rs:393`). Map definitions are read from the selected world pack,
not installed by that seed command. File-owned content and database-owned operational state are the
recorded D20/D21/D24 decisions in `docs/reference/decisions/layer-decisions.md` and AGENTS.md section 4a.
Those later decisions replace the earlier SQL-content proposal; legacy migrations remain schema history.

## The registry

World startup calls `register_world` before binding its gameplay listener
(`crates/asura-world/src/main.rs:259`, `:316`). Registration updates `t_world`, clears that
world's old lease rows, resets the published counters and removes obsolete map-capacity rows in
one transaction (`crates/asura-store/src/placement.rs:41-96`). Capacity advertises seats, not
prebuilt running instances.

Auth's `find_worlds` filters by map, heartbeat freshness, draining flag, selected build and
available capacity (`placement.rs:459-478`). Auth then probes the candidate's TCP port before
reserving (`crates/asura-auth/src/travel.rs:209-242`). A fresh registry row alone does not
establish that the listener is ready. Gateway reads the registered endpoint for relay routing;
its current route does not reject on the returned `fresh` field
(`crates/asura-gateway/src/relay.rs:80-103`).

The separate `t_service` catalogue is published from configured services and supervisor state
(`crates/asura-supervisor/src/registry.rs:49-80`), every five seconds while the supervisor runs
(`main.rs:138-150`). That heartbeat describes the supervisor's publication, not an independent
health report from each child. Gameplay placement and relay lookup use `t_world` instead.

### The lease ledger

`reserve` increments a non-full `t_world_pool` counter, removes the same session's previous
lease and inserts its replacement in one transaction (`placement.rs:192-236`).
`release` deletes whichever lease the supplied session id currently owns and decrements its
counter (`placement.rs:242-253`). It has no connection-generation argument; an older socket
using the same session id is not automatically a harmless release.

`reap_stale` deletes leases for worlds with old heartbeats and reduces their pool counters;
it does not remove the world registry row (`placement.rs:262-292`). `reconcile_world` counts
`t_lease` rows, not reports from running instances (`placement.rs:310-323`). Their callers
are database tests and `asura-fakeworld` scenarios, not the serving programs
(`crates/asura-store/tests/placement.rs:263`, `:346`,
`crates/asura-fakeworld/src/main.rs:225`, `:258`). Automatic runtime lease reaping and
reconstruction from world reports are therefore not implemented guarantees.

## Travel, end to end

1. At login, auth obtains candidates ordered by leased count and world id, probes them and
   attempts a reservation; it moves to the next candidate when a probe fails or capacity fills
   (`crates/asura-store/src/placement.rs:468`, `crates/asura-auth/src/travel.rs:195-242`).
   The query has no preference for the character's previous world.
2. A successful reservation precedes the pending transfer row and redirect. Auth releases the
   reservation if writing that transfer fails (`crates/asura-auth/src/travel.rs:245-264`).
   The advertised client endpoint is the gateway; see Connections, placement and routing.
3. In-world travel resolves a map on the current world and reserves there
   (`crates/asura-world/src/travel.rs:300`, `:414`). It is not the login candidate-selection
   loop and does not itself move a player to another build's world process.
4. The receiving world claims the transfer, reads character state and joins the map's running
   instance (`crates/asura-world/src/connection.rs:156`, `:243-268`, `:306-307`). It
   does not clear a private instance for each arrival.
5. Ordinary disconnect cleanup saves position where applicable and releases the session's lease;
   a redirected connection skips that release (`connection.rs:413-442`). A stopped world's
   abandoned leases are not automatically reaped by a serving-program task; see The lease ledger.

## Identity and reconnection

The intended newest-connection rule is recorded in
[the service-design research](../research/gw2-service-design-lessons.md#1-trust-only-the-most-recent-connection-slides-49-50).
The current implementation does not establish the full rule:

- `claim_transfer` accepts the same world/token again and updates `claimed_at`, even after a
  prior claim (`crates/asura-store/src/placement.rs:578-604`). This permits reconnects; it does
  not close or invalidate the older gameplay socket.
- Auth creates a local `Session` for each conversation
  (`crates/asura-auth/src/lib.rs:204`). No connection-replacement registry is established by
  that path. Session-id-only lease release has the limit described under The lease ledger.
- Login resolves both portal-user id and session id against an open `t_session` row
  (`crates/asura-store/src/identity/session.rs:32-52`). The credential row is separate from a live socket;
  ending the auth conversation does not consume it (`crates/asura-auth/src/lib.rs:198-203`).

Account and character identifiers are UUIDs in transfer records; world ids and player tokens
select the routing/transfer record (`crates/asura-store/src/placement.rs:516-530`). The research's
party-identity direction is not a claim that party identity is implemented.

## What is authoritative

The intended restart model is recorded in
[the service-design research](../research/gw2-service-design-lessons.md#2-restartable-means-rebuild-by-asking-not-reload-from-disk-slides-39-46).
The current storage and recovery paths are:

| State | Current home | Implemented recovery or limit |
|---|---|---|
| Live players and agents | World instance memory | First join constructs an instance; no persisted membership snapshot is restored (`crates/asura-world/src/instance.rs:427-443`). |
| Capacity and leases | `t_world_pool`, `t_lease` | World registration clears its old leases and resets counters; the standalone reconciler counts lease rows (`placement.rs:73-105`, `:310`). |
| Saved characters and items | PostgreSQL through `asura-store`'s state store | Entry reads saved state; durability depends on completed writes, not an unimplemented periodic-save guarantee (`connection.rs:243-268`, `:413`). |
| Login credentials and transfers | `t_session`, `t_transfer` | Database rows outlive sockets; transfer claims remain reusable (`login.rs:49`, `placement.rs:587`). |
| World endpoints | `t_world` | Re-registration refreshes a row; auth excludes stale rows. Staleness does not itself delete them (`placement.rs:53`, `:474`). |

References without a crate prefix in this table are under `crates/asura-store/src/` (state) or `crates/asura-store/src/placement.rs`, except
`connection.rs`, which is under `crates/asura-world/src/`. No path here rebuilds live membership
by asking another world for its players.

## Deployment and draining

The supervisor's `deploy-world` starts the new configured service if needed, waits for a fresh
registry row, then marks the old world draining
(`crates/asura-supervisor/src/supervise.rs:525-578`). This is distinct from ordinary stop.

Login candidate selection uses the build of the most recently started fresh world hosting the
requested map, then excludes draining/full candidates of that build
(`crates/asura-store/src/placement.rs:450-469`). It compares start time, not version ordering;
a full selected build has no fallback to an older build in this query.

The old world's heartbeat loop checks its population and removes its registry row before exiting
once draining and empty (`crates/asura-world/src/main.rs:390-410`). In-world travel still
reserves on the current process, as described under Travel, end to end. The login query's drain
filter is not a blanket prohibition enforced by `reserve` itself
(`crates/asura-store/src/placement.rs:191-194`). A real-client rolling-deploy acceptance remains
open in [project status](../work/project-status.md).

## Retry policy

The timeout-only, four-to-six-attempt direction belongs to
[the service-design research](../research/gw2-service-design-lessons.md#4-retry-policy-retry-timeouts-fail-everything-else-fast-slides-47-48).
It is not a shared retry facility implemented across these paths:

| Path | Current behavior | Evidence |
|---|---|---|
| Login placement | Try the next candidate after an invalid endpoint, failed probe or full reservation; database errors propagate. | `crates/asura-auth/src/travel.rs:195-242` |
| Gameplay relay | One timed TCP connection attempt, then return an error on failure. | `crates/asura-gateway/src/relay.rs:103-118` |
| Supervisor database startup | Retry connection errors and timeouts until its startup deadline. | `crates/asura-supervisor/src/supervise.rs:93-117` |
| Child process restart | Apply the configured restart policy, delay and retry cap. | `crates/asura-supervisor/src/supervise.rs:320-415` |

Process restart, candidate selection and retrying a database operation are separate mechanisms.

## Observability

The intended shape, not yet implemented: each world server and the auth service should expose a small
fixed set of counters - instances, leases, players, refused travels, database queue depth, tick
latency - and a **rate** form for anything cumulative, because a counter that resets on restart hides
the rate it is supposed to show (`docs/research/gw2-service-design-lessons.md`, rule 5). Nothing exports a counter today
and nothing reads one. Counters are cheap and always on; aggregation is a separate task, not
something a simulation thread does.

## Instances

| map type | lifecycle | count |
|---|---|---|
| outpost / town | one running instance created on first use, keyed by map id | one per used map |
| explorable | the same today; independent copies per party remain planned | one per used map |

Map descriptions and capacity advertisements are not running instances. Startup selects pack map
records and publishes capacity (`crates/asura-world/src/main.rs:178-247`). The first join calls
`Instances::for_map`, which constructs the instance and spawns its clock
(`crates/asura-world/src/connection.rs:306`, `instance.rs:435-442`).

`--on-demand` controls loading map descriptions and path data for maps outside the preloaded set;
it does not switch between startup-created and first-use instances
(`crates/asura-world/src/travel.rs:90-100`). Independent copies and their lifecycle remain in
[the simulation plan](../work/world-simulation-plan.md), phase 2.

The running instance collection is an in-memory map (`crates/asura-world/src/instance.rs:427`).
It is not a persisted instance snapshot. Restarting gateway drops its relayed sockets; durable
session/transfer rows and saved character state are separate from live instance membership.

## Inside a world server

- **Gameplay IO uses Tokio tasks.** Gateway relays sockets to the world; the map clock is a
  separate task. This does not imply a dedicated single OS reactor thread.
- **An update task per map.** The first join to a map starts its clock on its own task
  (`Instance::run`, `tokio::spawn` at `instance.rs:441`), and its loop continues indefinitely (`instance.rs:345-353`). The
  bounded pool of update threads with map affinity that AzerothCore uses is **not built**: nothing
  in `crates/asura-world` pins a map to a worker. It is the shape `docs/work/world-simulation-plan.md` phase 3 and
  `docs/reference/decisions/0006-structural-design-authority.md` name for the simulation stage, where per-instance state stops being lock-free.
- **Database access through the world's PostgreSQL pool.** Connection cleanup awaits
  `db_game::record_position` directly (`crates/asura-world/src/connection.rs:413-425`). This
  path does not enqueue a write to a separate database worker.
- **Character persistence has separate write paths.** Entry reads position/inventory; disconnect
  cleanup saves position except after a redirect or in character creation
  (`connection.rs:243-268`, `:413`). This does not establish a periodic save or a one-minute
  crash-loss guarantee. Those earlier claims are not verified implementation facts.

## Failure behaviour

| Event | Code-supported consequence | Evidence |
|---|---|---|
| World crashes | Gameplay sockets end; auth stops offering it after heartbeat expiry or a failed probe. Automatic runtime lease cleanup is absent. | `placement.rs:474`; `auth/travel.rs:222`; The lease ledger above |
| World restarts | Re-registers from configured map capacity; clears its old leases and resets counters. Live instances are not restored. | `placement.rs:73-105`; Instances above |
| Gateway stops | Its auth conversations and relayed gameplay sockets end. Credential rows are separate. | Connections, placement and routing; Identity and reconnection above |
| Database operation fails | The caller's error path decides the result: entry/travel operations propagate errors; disconnect position saving logs failure and continues. Continued play through an outage has not been established. | `crates/asura-world/src/connection.rs:156`, `:243`, `:413-425`; `travel.rs:414-436` |
| World registry row becomes stale | Auth excludes it; gateway still attempts the endpoint it resolves. Staleness does not mark or delete the row. | `placement.rs:474`; `crates/asura-gateway/src/relay.rs:80-103` |

Here `placement.rs` is under `crates/asura-store/src/` and `auth/travel.rs` is under
`crates/asura-auth/src/`. These are code-path descriptions, not failure-injection or client
acceptance results. They establish neither a crash-loss bound nor a general database-integrity
guarantee.

## Startup order

Operator commands belong in [Running the server](../operations/running-the-server.md).
The relevant lifecycle ownership is:

1. Supervisor connects to PostgreSQL, applies setup and starts configured children in dependency
   order (`crates/asura-supervisor/src/main.rs:128-152`, `supervise.rs:122-137`). The
   shipped dependency-free list sorts alphabetically by service name (`config.rs:192`).
2. World checks the schema, selects pack map records and registers capacity before validating
   further content and binding the listener (`crates/asura-world/src/main.rs:165`, `:178`,
   `:259`, `:293-316`). Therefore registration is not a completed-prebuild readiness barrier.
   Auth's subsequent port probe is described under The registry.
3. Ordinary supervisor stop signals each watcher, which calls `child.kill`; it is not a world
   drain or a character-flush protocol (`crates/asura-supervisor/src/supervise.rs:413-423`,
   `:505`). Connection cleanup saves only when that cleanup path runs.
4. Deployment draining is a separate path; see Deployment and draining. Supervisor shutdown also
   publishes stopped service-catalogue state (`crates/asura-supervisor/src/main.rs:174-176`).

Schema compatibility is checked by `asura_db::migrations::ensure_current`
(`crates/asura-world/src/main.rs:165-175`); a stale schema is refused at startup.

## Cost model

Earlier sizing comparison, retained as an unverified projection rather than measured ASURA capacity.
This table names no recording or reproducible measurement for its GWPS numbers. Its ASURA column
is a target estimate; the thread pool and memory-mapped archive entries are not implementation claims:

| item | GWPS today | ASURA target |
|---|---|---|
| private memory per instance | 11.9 MB | ~5 MB (shared `Gw.dat` index, database pool per world not per instance) |
| OS threads | 1 per instance | bounded pool, constant with instance count |
| database connections | 1 per instance | a small pool per world server |
| `Gw.dat` index | private copy per instance (4.7 MB) | one memory-mapped copy per world server |
| SQLite page cache | 2 MB per instance | gone - pooled PostgreSQL connections |

The earlier 500-instance / 2.5 GB figure is arithmetic on the 5 MB estimate, not a measured
capacity result. Deployment routing is described above; this estimate does not verify it.

## Deliberate non-goals

- Orchestration and autoscaling. Adding a world is a config entry and a process.
- Distributed consensus. The database is the arbiter; there is no second source of truth.
- Multi-region. One region; districts are capacity, not geography.

## Scope and boundary statements awaiting review

Simulation is in the project's scope (`docs/reference/decisions/0003-simulation-scope.md`). Its
implementation work is tracked in `docs/work/world-simulation-plan.md`; feature acceptance belongs
in `docs/work/project-status.md`. Archive/geometry loading belongs to the world. Client asset
streaming is the external content-service concern identified in the component inventory.

The following requirements remain unresolved against the implementation; documenting their code
paths does not approve or reject the requirements.

| Recorded statement | Conflict or implementation limit | Evidence |
|---|---|---|
| Worlds cannot mint tokens. | In-world travel generates a player token. The intended token-authority boundary remains unresolved. | `crates/asura-world/src/travel.rs:425` |
| The newest connection wins; older cleanup cannot release its lease. | Reusable transfer claims do not replace a live socket, and release is keyed only by session id. | Identity and reconnection; The lease ledger above |
| Rebuild leases from world reports after restart. | The helper counts database lease rows; world registration instead clears its previous leases. No serving-program report/reconciliation loop is established. | What is authoritative above |
| Timeout-only retry direction applies to database work. | Supervisor database startup retries connection errors as well as timeouts. | Retry policy above |
| Independent restarts preserve correctness. | Gateway carries gameplay sockets; no live-membership recovery protocol or general failure guarantee has been established. | Connections, placement and routing; Failure behaviour above |

The former scope also listed simulation design, game balance and content as deferred/out of scope.
Those blanket exclusions need review against the later simulation and pack plans. Its one-region /
one-language limitation is retained under deliberate non-goals above. Cross-world chat was excluded:
chat delivery was scoped to the world hosting the player. These statements are preserved for the
content review; this merge introduces no new scope decision.
