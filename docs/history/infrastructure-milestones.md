# Infrastructure milestones and earlier session records

Historical record. The completed roadmap milestones and older handoff sections below retain their original measurements, assumptions and instructions as dated project history. Current operation is documented in [Running the server](../operations/running-the-server.md); current session notes are in [Session handoff](../work/session-handoff.md).

## Completed infrastructure roadmap

## M0 - Specification

This repository's documents, and the decisions they depend on.

**Done when:** `docs/guidance/architecture.md`, `docs/reference/client-protocol.md`,
`docs/history/gwps-project-origins.md`, the ADRs, and this roadmap are complete enough to answer two questions
without asking the author: *what does the auth service do?* and *what does a world server decide?*

**Test:** a reader who has never seen GWPS can describe the allocation flow and the one rule.

## M1 - Cargo workspace, no networking

The skeleton follows AzerothCore's shape (auth service, world server, database domains, registry),
not GWPS's single-process layout.

The workspace is now seven members (`Cargo.toml`): `asura-auth`, `asura-world`, `asura-webgate`,
`asura-protocol`, `asura-db`, `asura-fakeworld`, `asura-supervisor`. M1's skeleton was smaller and this
list has grown with each milestone; there is no `asura` binary.

**Done when:** every world-link message round-trips byte-for-byte in a unit test, the protocol
version mismatch path is tested, and a service binary starts and exits cleanly against `--help` or the
real entry point.

**Test:** `cargo test`. Nothing depends on a client.

## M2 - Schema, registry, and reservations against a fake world

Schema first, then coordination through it: world servers register in `world`, publish pool capacity
in `world_pool`, and travel decisions reserve slots atomically.

**Done when:** a test harness (a "fake world" binary in the workspace) can register, heartbeat, be
marked stale when it goes silent, and have its leases reaped. Reservation is proven atomic under
concurrency: with a pool of capacity N-1, N parallel reservations succeed exactly N-1 times.

**Test:** harness-only, fully automated, no game client involved. The concurrency case runs the
reservation from many tasks at once.

**Delivered.** `asura-fakeworld` is the harness; `crates/asura-db/src/placement.rs` holds the queries;
`crates/asura-db/tests/placement.rs` proves the race, the refusal, the reaper and the reconciler
against the live database; `tools/m2-verify.ps1` runs both in one command. A `lease` ledger sits
beside `world_pool` so a lease can be identified, reaped and reconciled (`docs/guidance/architecture.md`).
The suite runs repeatedly without flakiness - it is the first thing to re-run after any change to
placement.

## M3 - The auth endpoint (the hard one)

Port the handshake and login: framing, ARC4, DH with the fixed constants, login against the web
gate, character list, character select.

**Done when:** the patched `Gw.custom.exe` reaches the character list against the auth service, and the
handshake produces traffic that diffs clean against the GWPS reference for the same actions.

**Test:** real client, login only - no worlds needed yet. Diff with
`PreSearing/tools/diff-transitions.py` and the `GWPS_TRACE` logs as the reference side.

**Risk:** the highest in the project. Failure is an opaque client error (`007`), so keep the packet
trace and compare every message. This is why the reference implementation stays runnable.

**Delivered 2026-10-26.** The patched client (build 39112) logged in against this service, took the
EULA and reached character select. `docs/reference/captures/login-session-2026-10-26.md` records the whole session from both
servers - every direction, header and size - and `asura-auth` reproduced it line for line up to the
client's travel request, which at that point was refused because no world server existed (it is answered`r
now, `docs/reference/game-world-model.md`): `REQUEST_GAME_INSTANCE`
(`0x8029`) needs `GAME_SERVER_INFO` (`0x0009`, 42 bytes: address, port, tokens) and then a second
connection speaking the game protocol, which is M4.

The pieces behind that: connection classification from the first four bytes; framing by walking each
message's field layout (u16 counts, no padding - the wire is variable-length in **both** directions,
which the table's sizes do not describe; see `docs/reference/client-protocol.md` #1); the full auth message table
generated out of the GWPS reference; the key exchange (ARC4, the fixed-constant Diffie-Hellman, the
protocol's own key derivation, pinned by fixtures **compiled from the reference's own C**,
`docs/reference/decisions/0009-protocol-cryptography.md`); the file check; typed layouts for every message on the login
path, including the 591-byte settings blob and the 62-byte character settings block copied from a live
reply; the login decision against the database; and the listener that puts them together with a
per-message trace behind `ASURA_TRACE=1`. `crates/asura-auth/tests/login.rs` proves it over a socket
without a game, and `tools/m3-verify.ps1` runs the whole path in one command. `tools/inspect-reply.py`
is still in the tree; the comparison tools it was used with (`probe`, `tools/compare-replies.py`) are
retired with the parity tooling (`docs/history/service-supervisor-delivery-plan.md`) - they are what settled the shapes when reading the
reference was not enough.

## M4 - End-to-end travel

The auth service allocates, a world grants an instance, the client is redirected to that world's own address
and reconnects with the token.

**Depends on:** GWPS step 1 - a world server that accepts and authenticates its own gameplay
connections (`docs/history/gwps-project-origins.md`, step 1).

**Done when:** one C world server, one the auth service, one client: log in -> enter a map -> travel to another
map, and the client's world rebuilds (the in-client recorder should show a tear-down followed by a
rebuild, as it does on GWPS: 100-204 ms).

**Test:** real client, plus the client-state recorder to prove the client actually settled.

**Progress.** The auth half is in (`crates/asura-auth/src/travel.rs`): `0x8029` is decoded,
`asura_db::placement::find_worlds` looks for a world that publishes the requested map and is fresh,
not draining and not full, the candidate's port is **dialled before it is offered** (a world can be
fresh in the database and dead on the socket, and the one rule is that a client is never sent to a
port nothing answers on), the slot is taken with the atomic `reserve`, and the answer is
`GAME_SERVER_INFO` (42 bytes, host blob in the reference's own asymmetry) plus a status-0 response. A
refusal is status 2 with no info message, and it leaves the counter untouched. `tests/travel.rs`
proves all of that over a socket, including the refusal for a registered-but-silent world.

**Progress (world half).** `asura-world` listens, and the whole path exists
(`crates/asura-world/src/lib.rs`):

* it classifies the connection, reads `GAME_CMSG_VERSION` (64 bytes: world id, map, player token, both
  UUIDs), and **claims** the pending transfer - a token nobody issued, a token already claimed, a token
  for another world, or the wrong character closes the socket with no message, which is what the
  reference does;
* it completes the key exchange, sends the reference's four-message load block unasked, answers the
  item/spawn/players requests (the last ending with `INSTANCE_LOAD_FINISH`), and keeps the world tick
  and ping running;
* the spawn point comes from `map_config` (migration 0005) and holds the coordinate the reference rig
  verified against a live client, not `(0,0)`;
* leaving releases the pool slot immediately rather than waiting for the reaper;
* the game message table is the one packet table (`crates/asura-protocol/src/packets.rs`, 682 rows), so
  client messages are framed by the reference's own field layouts rather than by a hand-written list.
**DELIVERED 2026-10-26 with the real client (build 39112): log in, choose the character, travel, and
stand in Lakeside County with its armour on, with the world server keeping the clock and answering
pings for as long as the client stays.**

The auth service is also the game connection's front door: a real client dials the auth address
whatever `GAME_SERVER_INFO` names (measured - it ignored the port in that message), so the auth routes
the socket to the world it named (`crates/asura-gateway/src/relay.rs`).

`crates/asura-world/tests/load.rs` proves the load over a socket - a placed client is loaded into its
map, a token nobody issued is closed without a word, a transfer naming a different character is
refused - and `tests/end_to_end.rs` drives login -> travel -> the routed game connection ->
`INSTANCE_LOAD_FINISH` in one process. `tools/m4-verify.ps1` runs all of it, and
`asura-supervisor start` starts the stack for the real client (`docs/operations/running-the-server.md`).

**How the last bugs were found, because the method is the durable part.** Three tools did it, and all
three are retired with the parity tooling (`docs/history/service-supervisor-delivery-plan.md`), so this is the record and not a recipe:
`tools/patch-gwps-full-trace.py`
made the reference dump whole messages (its own trace cut at 24 bytes, which hid every field that
mattered); `tools/compare-load.py` diffed our stream against its trace by size, bytes **and order**;
`ASURA_WORLD_REPLAY` + `ASURA_WORLD_REPLAY_PICK` serve the reference's captured bytes per block, which
is what separated "our payloads differ" from "our connection is wrong". The three bugs that survived
every earlier guess were, in order:

1. **a missing second `PLAYER_UPDATE_PROFESSION`** after `PLAYER_CREATE` and before the agent exists -
   an *order* bug, invisible to a diff that matched messages by header;
2. **an all-zero appearance word** (`PROFESSION_None`, hair style 0) - a model the client cannot build;
3. **no inventory at all** - bags and nothing else, and a client with no items has nothing to draw the
   character from. The reference's own default inventory was generated from its trace by
   `tools/gen-default-inventory.py` (retired with the parity tooling, `docs/history/service-supervisor-delivery-plan.md`) - now as **rows**
   rather than as replayed bytes, and the block rebuilt from them is compared to that capture byte for
   byte (`docs/reference/inventory-model.md`).

## M5 - Capacity, refusal, and persistence

The auth service sizes the world, refuses when full, and owns player state.

**Done when:** with one explorable slot configured, a second character travelling there is refused
*while still standing in the outpost* - the client never enters a loading screen. Player state is
written on leaving an instance and read back on entering.

**Test:** two clients, or one client and a harness that holds the slot.

**Progress (persistence half).** A character's *position* is saved when it leaves and read back when it
enters (`asura_db::game::record_position`/`position`, `Standing::resume`), and it is used only in the map
it was taken in; a character that has never been played spawns where `map_config` says. The inventory is
rows too (`crates/asura-db/migrations/0006_inventory.sql`), so an item survives a session - which is what
"player state" meant and did not exist before. What is still *not* saved is everything else about a
character: level, profession, gold, equipment changes.

**Progress (capacity half).** Placement already publishes per-map capacity, reserves atomically and
refuses a full map (`docs/work/session-handoff.md`, M2), and `docs/guidance/architecture.md` records the one rule: a client
is never sent to a port nothing answers on. What is missing is the *explorable* instance this milestone's
"refused while still standing in the outpost" needs - the world hosts outposts only - so a refusal at
travel time is proven (`asura-auth/tests/travel.rs`) but never with a client in a loading screen.

## M6 - More than one machine

A world on a second host, reachable because step 1 made the client-facing address real.

**Done when:** a hop takes a character from a world on host A to a world on host B, and back, with
nothing but a longer loading screen to show for it.

**Test:** real client, two hosts.

## M6.5 - The instance-creation experiment (answered 2026-10-27)

The one question we cannot answer from GWPS without more work there, so it is answered here
(`docs/reference/decisions/0005-gwps-reference-boundary.md`):

- Default: a travel is answered only with an already-running instance (prebuilt pool).
- Test: create the instance during the travel, redirect only after the world reports the instance
  ready, and watch the real client with the recorder. Rebuilt world = on-demand creation is
  acceptable; torn down and never rebuilt = the pool requirement is real, and we have a reproduction
  in our own code instead of an inference from a confounded one.

**Done when:** the answer is recorded with the client state that proves it, either way.

**Answered 2026-10-27: on-demand creation works with the client; the prebuilt pool is not a client
requirement.** The world ran with nothing preloaded (the retired `start-server.ps1 -OnDemand`) and the
real client took a loop of teleports - 148 -> 163 -> 164 -> 165 -> 166 -> 148 - with every destination
loaded on demand in 0-2 ms. The hang that forced the reference to pre-spawn one server per map
(`AuthSrv_PreSpawnServers`) was OpenTyria building the instance inside the hop, not a property of the
client. The runbook and numbers are `docs/reference/client-protocol.md` #5c; the server half is pinned by
`a_travel_to_an_unhosted_map_is_loaded_on_demand` and
`a_connection_to_an_unhosted_map_is_loaded_on_demand` (`crates/asura-world/tests/load.rs`). The
prebuilt pool stays the normal path; on-demand creation is the proven fallback.

## The tests that must exist before each milestone is called done

| milestone | automated | real client | measurement |
|---|---|---|---|
| M1 | unit round-trips, version mismatch | - | - |
| M2 | harness register/heartbeat/stale/refuse | - | - |
| M3 | every packet row recomputes to its recorded size (`crates/asura-protocol/src/packets.rs` tests), key exchange matches fixtures compiled from the reference's C, framing round-trips, identity-pair refusals, and a client logging in over a socket (`asura-auth/tests/login.rs`) | login to character list | packet diff vs GWPS |
| M4 | - | hop, and client state settles | tear-down/rebuilt, memory |
| M5 | the inventory rebuilt from rows equals the reference's capture byte for byte; seeding, drop scoping and the position round trip against the live database (`asura-db/tests/inventory.rs`); a client dropping an item and returning where it left over a socket (`asura-world/tests/load.rs`) | refusal while in outpost | saved state round-trip |
| M6 | - | cross-host hop | latency |

## Earlier status entries from the handoff

- **`/tp` works on a live client, ported from GWPS.** A real client in a map types `/tp ascalon`, receives
  GWPS's own `0x01A6` struct (46 bytes, `map_token, region, player_token, is_explorable, map_id`) and
  the 12-byte `0x0099`, reconnects, and lands in the destination's spawn. `docs/reference/client-protocol.md` #5
  records the port target and the two failed attempts that proved it (`007` from a wrong field order,
  `005` from a wrong frame size). `/maps` prints the reference's own table, one row per alias.
- **A travel into an explorable waits 4.3 s, and the field that decides it is `is_explorable`** (measured
  2026-10-31 on the patched client, one field moved and nothing else): 5138 ms with the field carrying the
  destination's own kind against 849 ms with it forced to 0, over the same walk through the same portal; the
  client's own clock agrees, 7.0 s against 0.90 s
  (`recordings/client/feature/travel/explorable-flag-on-20261031-223636.jsonl`), and live spends 0.76 s on
  that step with the same flag at 1 because its explorable is populated. **What the 4.3 s is spent on is an
  open question**, and the list of what it is *not* is measured: the four withdrawer messages (restored, timed),
  the world clock inside the load (gated, 0 ticks, same wait), the exploration grid and the instance table
  (removed entirely with `ASURA_LOAD_EXTRA=off`, same wait), and the database (no stall in a 400-iteration
  probe). The client asks for the players block - the one that carries a population - only *after* the wait, so
  a population is what an explorable is missing before the flag can honestly say 1 (row 9 of `docs/reference/instance-initialization.md`), not
  what the wait waits for. The measurement that names it is `tools/context-dump.py` taken inside the wait in
  both modes. **The default is now `is_explorable` 0 for every travel** so that a crossing costs 849 ms
  instead of 5138, and it announces an explorable as an outpost until the cause is found.
  **Two wrong readings of this bug came from timing the client with the sniffer's `t` field, which is the
  drain clock, not the packet's**: the record's own clock is its `tick`, and a gap read from `t` is a gap in
  the tool's loop.
- **Everything a run needs is in this repository.** The web gate is `crates/asura-webgate` (the
  reference's `tools/webgate.py`, ported, database-backed); the path graphs are **bundled** as
  `data/path-graphs.bin` (142 maps, 87,336,403 bytes, about 83 MiB; 11.1 MB and twelve maps was the
  2026-10-27 reading) and it is loaded automatically, so a fresh clone paths without finding a client
  archive; a local `Gw.dat` (`ASURA_GW_DAT`, `./Gw.dat`,
  or `data/Gw.dat`) is only the re-dump source, and the tests skip cleanly without one; the
  reference capture the offline checks read is committed as a recording under
  `recordings/gwps/world/` (`recordings/README.md`). No
  reference checkout is consulted at runtime, and `asura-supervisor start` builds and starts all of it
  (`docs/reference/client-protocol.md` #5d has the dump runbook).
- **Diagnosing a live session has its own tools, in this repository.** `-MoveTrace`
  (`ASURA_MOVE_TRACE=1`) logs every movement event - `move[pathfind]`/`[manual]`/`[tick]`/`[stop]`,
  a `move[jump]` wherever the agent's plane changes, and a `move[summary]` at teardown; `-Capture`
  (`--capture PATH` / `ASURA_CAPTURE`) writes **every game message both ways, decrypted**, as JSONL
  into `recordings/asura/world/` (the committed research archive) - no client hook, no elevation. `tools/sniff-capture.py` is the
  client-side counterpart for capturing the **live client on live servers**; it needs an elevated
  shell, the Py4GW_Stealth injection, and a **32-bit** Python (the tool re-executes itself under it,
  because the rig reads the instruction pointer through the x86 CONTEXT). A capture plus the movement log is how a drift or a jump is pinned to a
  message rather than guessed at.
- **Alternating movement, client-confirmed fixed (2026-11-01).** The owner reproduced the Ascalon City
  bridge fault on ASURA and performed the same route successfully on live servers. The captures are
  `recordings/client/feature/movement/alternating-asura.jsonl` and `alternating-live.jsonl` in the same
  directory. All 14 manual movement orders and 4 stops in the ASURA capture received no movement reply;
  the live capture replies with destinations and direction/type updates, and answers stops with type 9
  then the stopping destination (records 319-322 and 482-486). The server already held the correct
  position before the bridge click (`logs/world-8001.log:279818`); the client's sync copy being stale
  explains the return to the old click destination, but that copy was not directly measured.
  `agent.rs` now replaces the old path on manual movement, sends the replies to the player and peers,
  clears the path on stop, and restores forward speed on the next click. The reply builders use the
  packed wire layouts, not sniffer struct sizes. After rebuilding and restarting world 8001, the owner
  confirmed: "the issues are not reproduceable anymore". This proves the reported route, not complete
  movement parity. Validation: 98 protocol and 61 world unit tests passed, world built, ASCII check passed.
- **Answered (2026-10-27): precaching the destination is NOT a requirement here.** The reference
  pre-spawns one server per map so a travel constructs nothing; this world used to preload every map
  at startup. Measured on the real client with nothing preloaded (the retired `start-server.ps1 -OnDemand`): a
  loop of teleports (148 -> 163 -> 164 -> 165 -> 166 -> 148 ...) loaded each destination on demand in
  0-2 ms and the client took every hop without an issue. The hang that forced the reference's
  pre-spawn was OpenTyria's construction inside the hop, not a client requirement. The switch stays
  for testing; the runbook and the numbers are in `docs/reference/client-protocol.md` #5c, and the server half is
  pinned by the two `*_loaded_on_demand` tests.
- **Items work on a live client.** Dropping takes the stack down (`count = 1` of a hundred is what a
  client sends, and the reference refuses exactly that), the ground agent wears the item's *model*, moving
  an item between slots is answered and persisted, and wearing one moves it into the equipped bag and
  re-sends `UPDATE_AGENT_VISUAL_EQUIPMENT`. Chat speaks on `!`/`#`/`$` ungated, as the reference does, and
  `/stuck` and `/maps` answer. `docs/reference/inventory-model.md` records what the reference got wrong and
  which parts of this are therefore this project's own.
- **What the live session taught, in one line each**: the reference's `count < quantity` bail-out means a
  stacked item cannot be dropped there at all; its item agent carries the item's *id* where a model id
  belongs; the client sends `ITEM_MOVE` when an item is dragged and the reference cannot answer it; and the
  client's ping reply is `0x8009`, not `0x800C` (which is the heartbeat). The trace used to call all of
  those "ignored".
- **M5's inventory is otherwise as it was**: the item block is built from rows and a test compares it to
  the capture **byte for byte** (`the_item_block_from_rows_is_the_capture_byte_for_byte`), the default
  inventory is the reference's own six bags and nine items generated into
  `crates/asura-db/src/inventory/seed.rs`, and a character's position survives a session
  (`Standing::resume`).
- **What is left of the reference's dispatch is the four `CHAR_CREATION_*` handlers**, and they are
  blocked on two things: a character-creation *instance* (its `MapType_CharacterCreation`, which this
  world does not host) and the default-equipment table `GetDefaultEquipments` answers with. The second
  is now cheap: equipment is item rows.
- **The parity audit exists: `docs/history/gwps-parity/behavior-audit.md`.** Every GWPS behaviour and ASURA's counterpart,
  row by row, with `file:line` on both sides and a verdict (1:1 / AHEAD / DEVIATION / BEHIND / STUB).
  51 of 78 are indistinguishable, 10 are ASURA's own, 5 differ on purpose, 11 are behind but collapse
  into three work items, and 1 is a stub in both. Read it before deciding that something is "missing". The
  tooling behind it - the oracle, the `parity-*` and `gen-*` scripts - was retired when parity closed
  (docs/history/service-supervisor-delivery-plan.md); the audit and the parity documents are history, and the tree no longer references the
  reference project.
- **M2 delivered: placement is safe, and it is proven against the real database.** The `asura-db`
  crate now owns registration, the atomic reservation, the lease ledger, the reaper and the
  reconciler; `asura-fakeworld` runs the whole sequence as a harness; the acceptance suite passes
  repeatedly (`.\tools\m2-verify.ps1`). No game client was involved - that was the point.
- **M3 is delivered: the patched client reaches the character list against this service.** Measured
  2026-10-26: build 39112 logged in as `user1@example.com`, took the EULA, and reached character select.
  `docs/reference/captures/login-session-2026-10-26.md` holds the whole session - both directions, every header and size - and
  `asura-auth` reproduced it line for line up to the client's travel request, which it refused while no
  world server existed yet. The service runs: it reads its configuration, refuses a stale schema,
  binds a socket, classifies each connection, performs the key exchange with real randomness, answers
  the file check, the computer-info preamble and the session salt, validates the identity pair, and
  sends the reference's five-message login batch. `ASURA_TRACE=1` logs every message both ways.
  `.\tools\m3-verify.ps1` runs the whole path in one command (it was 105 tests when M3 landed; the
  suite is 175 now).
- **What the pieces are:** classification from the first four bytes, framing by the table's fixed size
  per header with no length field, the key exchange (ARC4, the fixed-constant Diffie-Hellman, the
  protocol's own key derivation, pinned by fixtures **compiled from the reference's own C**), typed
  message layouts for the login path, the reference client, the login decision in `asura-db::login`,
  the migration record, and a seed for the three identity pairs the rig's portal DLL hands out.
- **Two rules the wire taught us, both now documented:** a login does **not** spend the session row
  (GWPS never writes its session table, and the rig presents the same fixed pair every time), and the
  login batch has a fixed order the client expects (`docs/reference/client-protocol.md` section 4).
- **The real client has been pointed at this service, and the risky half is confirmed.** Client build
  39112, on this machine, 2026-10-26: it classified as an auth connection, completed the key exchange,
  and **its subsequent traffic decrypted correctly in our ARC4** - which is the first proof that the
  fixed-constant Diffie-Hellman and the protocol's own key derivation match the real client rather
  than just the reference's C. Its opening messages were captured byte for byte: `0x8001`
  `SEND_COMPUTER_INFO` (**42 bytes, variable-length, `u16` counts - not the table's 138**) and `0x8002`
  `SEND_COMPUTER_HASH` (22 bytes), 64 bytes in total. Both are now decoded, and the capture is a
  regression test.
- **The portal is a real HTTP service, and this project now serves it itself**
  (`docs/reference/client-protocol.md` #4, corrected): with `-portal 127.0.0.1` the client calls four
  `/Spawned/WebGate/...` endpoints on `localhost:6601` and takes its `user_id`/`session_id` from them.
  The reference answered them with `tools/webgate.py`; **`crates/asura-webgate` is now the port** -
  same endpoints, same XML and status codes, answered from the rows `tools/seed-dev.ps1` writes
  (`asura_db::portal`). `asura-supervisor start` builds and starts it; no Python, no reference
  checkout.
- **Where it stops, and it is now the top of the queue:** after that web-gate login the client opened a
  second auth connection, sent the same preamble, and reset it instead of sending the portal login.
  Against GWPS the login did arrive, nine seconds later, and worked. The reference's own log shows it
  *mis-framing* that preamble (`Unhandled AuthSrv packet with header 35`), so it is not that GWPS
  handles the message better - something else differs. `docs/reference/client-protocol.md` #10 records the two cheap
  pieces of evidence that would settle it: the client's on-screen error text, and a trace with
  timestamps and connection ids.

- **M1's scaffold still holds:** the workspace compiles, `cargo test --workspace` is green (the M1/M2
  part of it: 12 unit + 4 live-database placement tests that skip cleanly without a database), and
  `cargo clippy --workspace --all-targets` is clean.
- **The login identities exist.** The client does not ask a web service who it is - the rig's portal
  DLL hands it a fixed pair (`PreSearing/portal/portal.c`) - so `tools/seed-dev.ps1` writes the rows
  those pairs must resolve to: three accounts with a character each. Idempotent; re-running reopens
  the sessions.
- **M1's scaffold still holds:** the workspace compiles, `cargo test --workspace` is green (the M1/M2
  part of it: 12 unit + 4 live-database placement tests that skip cleanly without a database), and
  `cargo clippy --workspace --all-targets` is clean.
- **A protocol document was wrong, and is now corrected.** `docs/reference/client-protocol.md` section 1 claimed a
  `u16 length` after every message header. The reference has no length field at all: `pack_msg`
  writes the format's fields and nothing else, so a message's size is a property of its header.
  Writing the length would have produced the client's opaque `007`. The evidence is recorded in the
  document, along with the related trap that `sizeof(struct)` is not the wire size
  (`AuthSrv_SessionInfo`: 14 bytes in C, 10 on the wire).
- **Fifteen documents and eight ADRs** describe what is built, what is refused, and why.
- **GWPS is research material** (`docs/reference/decisions/0005-gwps-reference-boundary.md`). ASURA is written from
  scratch; nothing is ported or patched there.
- **AzerothCore is the design authority** for structure (`docs/reference/decisions/0006-structural-design-authority.md`).
  A deviation has to be recorded before it is implemented - see ADR 0008 for how that is done well and
  how it is done badly.

## Delivery and operations records from the handoff

## What M2 delivered

| piece | where |
|---|---|
| lease ledger migration (`lease` table: one row per reserved slot, unique per session) | `crates/asura-db/migrations/0002_leases.sql` |
| typed placement queries - register, heartbeat, reserve, release, reap, reconcile, remove | `crates/asura-db/src/placement.rs` |
| synchronous, idempotent migrations and the schema tests that keep them honest | `crates/asura-db/src/lib.rs` |
| the concurrency acceptance test (capacity N-1, N concurrent reservations, exactly N-1 win; full-pool refusal; reaper; reconcile) | `crates/asura-db/tests/placement.rs` |
| the fake-world harness (`asura-fakeworld scenario`) | `crates/asura-fakeworld/` |
| one command: integration tests + harness scenario | `tools/m2-verify.ps1` |
| dependencies, added with the code that needs them: `sqlx` 0.8 (PostgreSQL), `tokio` 1, `uuid` | `Cargo.toml` |

The acceptance result, verbatim from a run: capacity 4 with 5 concurrent reservations -> 4 won, 1
refused; `leased` ends at capacity; the ledger matches; the reaper freed a silent world's leases and
left a fresh world's alone; reconcile rebuilt the counter from the ledger. Repeated 5x, no flakiness.

**Why a ledger beside the counter:** `world_pool.leased` makes the reservation atomic; the `lease`
rows make a lease *identifiable*, which is what the reaper needs to free a dead world's slots and the
reconciler needs to rebuild `leased` from the worlds instead of trusting the table
(`docs/research/gw2-service-design-lessons.md`, rule 2). They are inserted, released and reaped together in one
transaction, so `count(lease) <= leased <= capacity` holds at every commit and the
`world_pool_never_oversold` CHECK never fires.

## Environment status on this machine

| what | state | evidence |
|---|---|---|
| **Rust** | installed, 1.99.1 | `cargo test --workspace` -> **175 tests, 0 failures**; `cargo clippy --workspace --all-targets` -> clean; `cargo fmt --all -- --check` -> clean |
| cargo on PATH | **not** on PATH in a fresh shell | use `C:\Users\Apo\.cargo\bin\cargo.exe`, or add that directory to PATH permanently |
| **PostgreSQL** | installed, 16.15, service **Running**, and **provisioned** | role `asura`, database `asura`, **9 migrations applied / 11 tables** (`account`, `bag`, `characters`, `item`, `lease`, `map_config`, `schema_version`, `session`, `transfer`, `world`, `world_pool`); credentials in `.env` (gitignored) |
| psql | `C:\Program Files\PostgreSQL\16\bin\psql.exe` | not on PATH |
| client rig (Reforged + stealth hooks) | present from the PreSearing work | `asura-supervisor start`, then the client; the trace is the verdict. The client is patched by `client-patcher/patch-gw.py` with `client-patcher/keys.txt` (the server's own DH constants) |

**Database is ready.** `ASURA_DATABASE_URL` is in `.env` (gitignored, never committed). Every migration
is applied; `crates/asura-db/tests/placement.rs` verified the pool invariants against the live database -
at capacity the reservation `UPDATE ... WHERE leased < capacity` returns no row (the refusal signal), and
an over-capacity row is rejected by `world_pool_never_oversold` - and `tests/inventory.rs` verifies that a
character is seeded once, that its items read back identical to what was written, and that a position
survives.

If the superuser password is ever lost again, `tools/pg-recover.ps1` (run elevated) does the standard
recovery. It restores secure auth before it finishes, and it writes the new credentials to `.env`.

Setup in full:

```powershell
$env:PGPASSWORD = "<postgres superuser password>"
.\tools\db-setup.ps1            # creates role + database, applies every migration, lists the tables
```

## The ten-minute idle run (T13)

All three clauses of the reference's T13 are now measured by one command:

```powershell
cargo test -p asura-world --test load a_ten_minute_idle_pair -- --ignored --nocapture
```

It holds **two** connections idle for ten minutes, counts the ticks and pings, watches the world's peer
count, and samples the process's working set every fifteen seconds through
`K32GetProcessMemoryInfo` (kernel32, no crate). The verdict is asserted, not narrated: after a tenth of the
window to let the allocator settle, a working set that grows by more than 8 MiB fails the test.

Two details worth knowing before trusting a "flat" line. The sampler **checks itself first**: it allocates
16 MiB, writes one byte per page -- a zeroed allocation is demand-zero pages and does not move the working
set, which the first version of this check proved the hard way -- and fails if the sample does not see it.
And the window is `ASURA_SOAK_SECONDS` (default 600), so the mechanism can be exercised in a minute:

```powershell
$env:ASURA_SOAK_SECONDS = 45
cargo test -p asura-world --test load a_ten_minute_idle_pair -- --ignored --nocapture
```

The world runs as a task in the test process here, so the sample is everything the world allocated plus the
harness -- the reference runs a process per map and watches that process. Same question, one process fewer.

## Run it: one document

The "how do I run this" page is **`docs/operations/running-the-server.md`** - what you need once, the one command, what
success looks like, the everyday supervisor commands, the rolling deploy, the client, CI, and a
troubleshooting table. It is the only place those instructions live now; this section keeps only the
operational facts behind them:

- **The project runs natively on this machine**: `asura-supervisor`, host path, and it is the one start
  path on both operating systems (`AGENTS.md` #7). `docker/` exists as an offer and does not drive the
  project.

- **The run is quiet by default**: `-Trace` prints every packet, and the packet log is
  off unless asked for, because a world tick every 500 ms is three lines a second forever and it buries the
  lines that matter. What is *always* printed is the events - `auth: login ok for ...`, `auth: travel to map
  146 placed on world 8001 ...`, `world: ... joined map 146 ... with 6 bag(s) and 9 item(s)`, a saved position, a
  dropped item, a moved item, a worn item, a refused move, a chat that was not sent.
- **The web gate is not optional for a client attempt.** With `-portal 127.0.0.1` the client asks the gate
  on `6601` who it is before it sends a login; with nothing there it opens the auth connection, does the key
  exchange, sends its preamble, and closes - which looks like "I cannot connect to the server" while the
  server is running perfectly (`AUTH_CMSG_PORTAL_ACCOUNT_LOGIN` never arrives). The gate is this
  project's own (`crates/asura-webgate`).
- **The launcher scripts are retired** (2026-10-28, S1's separate step): `tools/start-server.ps1`, `tools/play.ps1` and `tools/stop-server.ps1` are gone - no more PowerShell in the run path, and the supervisor is the one start path on Windows and Linux (`docs/operations/running-the-server.md`). What they did that the supervisor does not: `start-server.ps1` started PostgreSQL (elevation); the database is "managed elsewhere", so start it yourself - `Start-Service 'postgresql-x64-<version>'` on Windows, `sudo systemctl start postgresql` on Linux. A hard stop no longer leaves a visible "what is still leased" note: the supervisor's stop is clean, and a hard kill still leaves a connected player's pool slot for the unscheduled reaper. M2 is provable at any time with `tools/m2-verify.ps1` (integration tests + the fake-world scenario, live database).

## Run it: the supervisor (cross-platform)

`crates/asura-supervisor` starts this machine's services from `config/services.toml` and watches them
(`docs/history/service-supervisor-delivery-plan.md`, S1; the how is `docs/operations/running-the-server.md`). It is the one start path on
both operating systems; the PowerShell launchers it replaced are retired. **Starting the server is one command**
(`start`); it builds what is missing, seeds the rows a fresh database needs (the same steps the old
start script ran), starts everything, and watches until Ctrl+C or `stop`. It also carries the two setup
steps as cross-platform commands: `schema` (the migrations) and `seed` (the dev identities and the map
catalogue) - the SQL lives in `tools/seed-dev.sql` and `tools/map-config.sql`, applied through `psql`
the same way on Windows and Linux. Every service it starts is registered in the `service` table (S2):
kind, address, port, build, state and a heartbeat; `asura-supervisor status` prints that registry with
a STALE marker, and a stopped supervisor leaves the rows at `stopped`. A world's build is stamped with
`git describe` (`--build` overrides it), and `deploy-world --old N --new M` starts the new build,
proves it is registered, and only then marks the old world draining - the old world keeps its players
and leaves once its last one reconnects, with nobody kicked (`docs/guidance/architecture.md`, Deployment and
draining). On a Linux server host, the runbook is `docs/operations/running-on-linux.md` (same commands; the
game client stays Windows-only, and Linux has not had a live run recorded yet). The command list lives
in `docs/operations/running-the-server.md`; it is not repeated here.

Two operational rules the supervisor encodes: a crash is retried at most `restart_max` times
(`config/services.toml`), then the service is reported FAILED - never restarted forever; and the
services run in their own process group, so Ctrl+C stops them *through* the supervisor (closing the
console window still ends everything attached to it, and a hard kill of the supervisor orphans the
services - there is no code that can run on SIGKILL).

(No network is needed for either once dependencies are cached. If `cargo` cannot reach crates.io,
the sandbox token may be blocking TLS - run it from a normal shell, or with full access.)

## Earlier next-session instructions and implementation accounts

## Where the next session starts

For the world-simulation implementation assignment, use `docs/work/world-simulation-plan.md`.
For control-panel design, use `docs/work/control-panel-plan.md`: the owner expanded the manager
to server start/stop and operations, Py4GW Stealth capture-session launching/management and in-game
data reading, guided tests, and configuration/content management on 2026-11-02. Stealth is a core
research capability of the panel, not just an attachment to captures; the normal server remains
independent of it. Interface/deployment choices and initial reader coverage remain open.
It records the owner's intent, the early map/portal content manager, seven ordered phases,
completion checks, and the unresolved choices the implementer must ask about before dependent
work. Lua and the BehaviorTree.CPP execution reference are chosen; manager technology and JSON
import/export are not. This roadmap does not authorize unrelated gameplay changes.

M4 is delivered - a real client logs in, travels, and stands in a map (`git log`, `4f610a8`) - and it now
carries an inventory and comes back where it logged out.

### The staging, and where this tree stands in it

Owner, 2026-11-01 (the decision row above). Four stages, in order, and only the first is closed:

- **Stage 1 - GWPS parity. Closed.** Auth dispatch 15/15, game dispatch 18/18, and one login's stream is
  reproduced message for message.
- **Stage 2 - 1:1 with the game.** The rule is written (`docs/reference/client-protocol.md` 5e): nothing between the wire
  and a handler is shaped by GWPS's arrangement. This is held, not reached.
- **Stage 3 - naming.** Every header named with its evidence, so no row is `Unnamed`. Fifteen of the 32
  headers a real session used are named (`23de10e`, `d17a60a`, then `0x8093`
  `GAME_CMSG_INSTANCE_QUERY` from a world-map capture); the seventeen that remain, with the shape
  each capture proves and the one action that names it, are `docs/work/unnamed-packets.md`. That page also
  records why the rest cannot be finished offline: the catalogue of this lineage leaves them unnamed and
  the reference never sends them. **The initialization phase is the first target** and it is two names
  wide: one load carries 54 server-to-client headers, 32 of them already sent here, 20 named but never
  sent, and only `0x011F` and `0x012F` unnamed. Whether the 20 can be filled depends on what the client's
  structures hold after a load, which is `docs/operations/capturing-instance-initialization.md`: one sniffer capture plus
  `tools/context-dump.py` at four moments. After the 17, the 306 game rows no capture contains need a
  stimulus instead, one opcode at a time. The table as it stands now: 782 rows, 391 `Unnamed` (323 of
  them `Conn::Game`), 255 `Known`, 87 `Emitted`, 36 `Handled`, 7 `Inferred`, 6 `Observed`.
- **Stage 4 - stream parity.** Every message of a flow answered, or ignored with a written reason. The
  ledgers are `docs/reference/instance-initialization.md` (the set and order an instance sends) and
  `docs/research/client-session-packet-analysis.md` (what a live stream carries and this tree drops). Today 78 of the 148
  headers a real session used are neither sent nor acted on.

The last client run on `main` is the 2026-11-01 movement confirmation above: the owner walked the
Ascalon City bridge on ASURA, reproduced the same route on live servers, and reported "the issues are
not reproduceable anymore" after `agent.rs` began answering manual movement orders. The attempt's
`007` predates that run (`docs/history/packet-parity-attempt-2026-10-31.md`), so a client run today is a measurement,
not a confirmation.

1. **Verify the baseline** (3 minutes): `cargo test --workspace --offline`. The last full run recorded
   here was **175 passing** (2026-10-31), of which 16 are live-database tests in `asura-db` (login,
   placement, inventory, migrations) and 6 drive real sockets in `asura-world` (the load, a drop, a
   return). That figure is a dated reading, not a target: the workspace declares 261
   `#[test]`/`#[tokio::test]` attributes today, 2 of them `#[ignore]`d. Then `.\tools\test-battery.ps1`.
   If a count changed, read `git log` before assuming it is broken.
2. **Nothing of the reference's dispatch is left.** Character creation, the last gap, is implemented:
   the creation instance is hosted (`crates/asura-world/src/main.rs:237-251`), the four
   `GAME_CMSG_CHAR_CREATION_*` handlers are dispatched (`crates/asura-world/src/dispatch.rs:65-71`),
   and the default-equipment table is carried as data from `GmDefaultArmors.c`
   (`crates/asura-world/src/default_equipment.rs`).
3. **Then the things the reference never wrote**: picking a dropped item back up
   (`GAME_CMSG_INTERACT_ITEM`, `0x803F` - the last piece that makes a drop reversible), splitting a
   stack by hand (`0x8075`), buying and selling (`GAME_CMSG_TRANSACT_ITEMS`, `0x804D`), swapping two
   items in one move, and `UNEQUIP_ITEM` (`0x804F`), whose layout is still unmeasured. Each starts with
   a *measurement* of what the client sends; the reference's C has now been insufficient four times, and
   a live trace sufficient every time.
4. **Use the reference session as the spec.** `docs/reference/captures/login-session-2026-10-26.md` is the whole working login,
   both directions; the parity tooling that produced and diffed it (trace patches, reply and load comparators)
   was retired when parity closed (docs/history/service-supervisor-delivery-plan.md); the session document and the recordings under `recordings/`
   are the record.
5. **The real client is the last word on anything the wire does.** Everything above is harness-only, and
   three messages that no capture contains have now crashed a live client (`ITEM_REMOVE`, an item agent,
   chat). `asura-supervisor start` with `ASURA_TRACE=1` set in the shell is the verdict - and if a client
   crashes, the log's last `world ->` line names the message that did it.

**Decided, and not to be re-opened without a reason:**

- The portal user id and the session id are separate columns, not one reused column
  (`crates/asura-db/migrations/0003_portal_identity.sql`, `docs/guidance/architecture.md`). The migration
  refuses to invent an identity for a pre-existing session row, because a login path that silently
  accepts a pair nobody issued is the one failure mode worth failing loudly for.
- **The login path never writes `ended_at`.** GWPS only reads its session table, and the rig presents
  the same fixed pair every login. `close_session`/`close_other_sessions` are for a deliberate logout
  or a stale sweep, not for a disconnect.
- **A connection this service does not understand is closed, not guessed at**, and a pair that does
  not resolve is answered with the client's auth error (`11`) rather than silence.
- **A dropped item becomes litter.** It is off the stack and on the ground for the rest of the session:
  the reference has no pick-up handler and this one has none either. `GAME_CMSG_INTERACT_ITEM` (`0x803F`)
  exists in the generated table, and what it means is one measurement away (drop one, interact with it,
  read the header out of the trace).
- **A move that would swap two items is refused**, not swapped: an occupied slot is `Refused::Occupied`
  and the client is told where the item really is. The reference has no move handler at all, so there
  was nothing to copy - swapping is a decision nobody has made yet.
- **`UNEQUIP_ITEM` is not implemented.** Its 6 bytes (`u8, u16, u8`) carry no item id and no capture has
  shown what it means; taking an item off by dragging it out of the equipped bag arrives as `ITEM_MOVE`
  and works.
- **The item block is built from rows and compared to the capture byte for byte.** Either side changing
  alone fails `the_item_block_from_rows_is_the_capture_byte_for_byte`, which is the point: the rows and
  the capture are two descriptions of one thing, and the test is what keeps them one thing.

## The pieces the inventory added (M5)

| piece | where |
|---|---|
| `bag`, `item`, the item-id sequence, and `characters.pos_plane` | `crates/asura-db/migrations/0006_inventory.sql` |
| reading a character's bags and items, seeding the default set, finding the item a drop names | `crates/asura-db/src/inventory.rs` |
| the seed: the reference's own six bags and nine items, generated from the capture | `crates/asura-db/src/inventory/seed.rs` |
| the capture itself, kept as the fixture the rows must reproduce | `crates/asura-db/src/inventory.rs:243` (`default_inventory()`) |
| the generator for both halves: the seed rows and the captured bytes | `tools/gen-default-inventory.py`, retired with the parity tooling (`docs/history/service-supervisor-delivery-plan.md`); the two files above are its committed output |
| `CREATE_NAMED_ITEM`, `ITEM_MOVE_TO_LOCATION`, `ITEM_SET_PROFESSION` | `crates/asura-protocol/src/game/mod.rs` |
| the item block from rows, the bags in model order, the worn ids from the equipped bag | `crates/asura-world/src/item.rs:70` (`items_block`), `:120` (`inventory_block`), `crates/asura-world/src/player.rs:145` (`player_block`) |
| `DROP_ITEM`, finished rather than copied: the stack is taken down, and nothing goes on the wire | `crates/asura-world/src/item.rs:187` (`drop_item`), `:243-249` (why it sends nothing); dispatched at `dispatch.rs:73` |
| `ITEM_MOVE` and `EQUIP_ITEM`, which the reference does not dispatch at all | `crates/asura-world/src/item.rs` + `asura_db::inventory::{move_item, first_free_carrying_slot, take_from_stack}`, dispatched at `dispatch.rs:73-79` |
| the position saved on leaving and resumed on entering | `asura_db::game::record_position`/`position`, `Standing::resume` |
| the live-database proof: seeding, round trip, drop scoping, position | `crates/asura-db/tests/inventory.rs` |

## The pieces the map data added (D1-D3)

A map's geometry is one `Gw.dat` entry, compressed, and the reference reads it in three steps: the
archive's index, its decompressor, and `GmMapDataImport.c`'s chunk reader. All three are here.

| piece | where |
|---|---|
| the archive index: header, file table, TOC hash, one entry's bytes | `crates/asura-world/src/gwdat.rs` |
| `FaCompress.c`'s decoder: the bit window, the node tree, the block loop | `crates/asura-world/src/fa_compress.rs` |
| its six static tables | `crates/asura-world/src/fa_compress_tables.rs` |
| the container and the path graph: planes, trapezoids, x/y/sink nodes, portals | `crates/asura-world/src/map_data.rs` |
| "which trapezoid is this position in": `PathFindTrapezoid` | same file (`PathStaticData::trapezoid_at`) |
| every catalogue map imported out of a real `Gw.dat`, with its spawn point | `crates/asura-world/examples/map-census.rs` |
| the test that a dumped graph answers exactly the walks the `Gw.dat` import answers | `crates/asura-world/tests/path_dump.rs` |
| the A*: neighbour and portal visits, the heap, the waypoint builder | `crates/asura-world/src/pathfind.rs` |
| its fixed-point reciprocal and square-root tables | `crates/asura-world/src/path_tables.rs` |
| the in-module pathfinding tests: the visits, the heap, the waypoint builder | `crates/asura-world/src/pathfind.rs` |
| the graphs a world serves with, imported at startup from `Gw.dat` | `crates/asura-world/src/lib.rs` (`PathGraphs`), `src/main.rs` (`--gwdat`, `ASURA_GW_DAT`) |
| the walk itself: destination, direction, speed, waypoints, and the tick that advances them | `crates/asura-world/src/agent.rs` (`Motion`, `begin_walk`, `advance`) |
| `MOVE_TO_COORD`'s answer: waypoint 0, or the refusal that is a position broadcast | `crates/asura-world/src/lib.rs` (`0x803E`) |

The pathing proof is data-driven now: `tests/path_dump.rs` asserts a graph written with
`--dump-path-graphs` answers exactly the walks the `Gw.dat` import answers, and the in-module tests in
`src/pathfind.rs` pin the visits and the waypoint builder. They skip cleanly without a local `.dat`.

The parity/regeneration apparatus that used to sit beside them (`parity-pathing.py`, `path-oracle.c`,
`gen-path-cases.py`, the walk fixtures) was retired when parity closed (docs/history/service-supervisor-delivery-plan.md); the measurements it
produced are described in `docs/history/gwps-parity/completed-work-queue.md`, item D, and are history. One of those
measurements is worth keeping as a note rather than a command: `PathBuildAddWaypointAndReduce` reads
its intersection parameters after `FindIntersectionPoint` has declined to write them (parallel lines),
so one of the catalogue cases had a different waypoint list on different runs. That was a property of
the reference, not of this port, and `path_dump.rs` is the guard that a waypoint list is stable now.

## The pieces the character settings added (B5)

The login reply's character list carries a 62-byte settings block per character, and this server used to send
the same captured one for every character. It is now built from the row.

| piece | where |
|---|---|
| the five columns the blob needs (level, campaign, is-pvp, secondary profession, helmet status) | `crates/asura-db/migrations/0009_character_settings.sql` |
| reading them back, plus the armour pieces derived from the equipped bag | `asura_db::game::settings` |
| `CharacterSettings`'s packing, byte for byte, and the reference's own blob kept as the expected output | `crates/asura-protocol/src/client/messages.rs` (`character_settings`, `CHARACTER_SETTINGS_EXTENDED`) |
| the login reply building it per character instead of sending the constant | `crates/asura-auth/src/lib.rs` |
| the creation defaults the reference writes (`level = 1`, `is_pvp` when the campaign is none, helmet shown) | `asura_db::inventory::create_character` |
| the proof: the built block equals the reference's, the seeded character's does not, and the row round-trips | `messages.rs` tests, `crates/asura-auth/tests/login.rs`, `crates/asura-db/tests/inventory.rs` |
| the schema diff, as a historical record: 95 reference columns, each stored, derived or explained | `docs/history/gwps-parity/database-schema-comparison.md` (history) |

Two schema details worth not re-learning: `smallint` and `integer` are *different Rust types* to sqlx, so a
query that reads a mixed set of them needs `::int` casts or it fails at runtime; and the settings block is
always 62 bytes even for a character wearing nothing, because the reference sends `sizeof(CharacterSettings)`
- the five piece slots are blank, not absent. The comparison tools that produced `docs/history/gwps-parity/database-schema-comparison.md` and the load
allowances were retired with the rest of the parity tooling (docs/history/service-supervisor-delivery-plan.md).

## The pieces M3 added

| piece | where |
|---|---|
| connection classification: the three version headers, their sizes, and the refusal to guess | `crates/asura-protocol/src/client/version.rs` |
| framing: header plus table-sized body, no length field, with encode-side size validation | `crates/asura-protocol/src/client/frame.rs` |
| the packet table: every header of both families, 100 auth rows and 682 game rows, with names, sizes and field layouts | `crates/asura-protocol/src/packets.rs` |
| the portal identity column, and why it is not the account id | `crates/asura-db/migrations/0003_portal_identity.sql` |
| the login decision: find the session, close the others, read the characters | `crates/asura-db/src/login.rs` |
| the refusal rules under test against the live database | `crates/asura-db/tests/login.rs` |
| the three identity pairs the rig's portal DLL presents, seeded idempotently | `tools/seed-dev.ps1` |
| one command for the whole login path, client not needed yet | `tools/m3-verify.ps1` |
| the database's record of its own migrations, and the refusal to run against a stale schema | `crates/asura-db/src/migrations.rs` (+ `tests/migrations.rs`) |
| ARC4, ported from `arc4.c`, checked against published vectors and round trips | `crates/asura-protocol/src/client/arc4.rs` |
| the key exchange: seeds, fixed-constant DH, and the protocol's own key derivation | `crates/asura-protocol/src/client/handshake.rs` |
| the login batch's fixed 591-byte settings blob | `crates/asura-protocol/src/client/login_fixtures.rs` |
| typed layouts for the messages the login path uses, with sizes checked against the table | `crates/asura-protocol/src/client/messages.rs` |
| a test client: the same protocol from the other end, so the service is testable without the game | `crates/asura-protocol/src/client/peer.rs` |
| the service itself: listener, key exchange, ARC4 both ways, login, trace | `crates/asura-auth/src/lib.rs` (+ `src/main.rs`) |
| a client anyone can run against a live service | `crates/asura-auth/examples/login.rs` |
| the end-to-end proof: a real socket, the real database, the character list | `crates/asura-auth/tests/login.rs` |

The tables and the fixture blobs carry `#[rustfmt::skip]` and are the implementation now. Their
generators and checks (the `gen-*` scripts, the `parity-*` scripts, the oracle binaries) were retired
when parity closed (docs/history/service-supervisor-delivery-plan.md); if a table ever disagrees with the client, the client is the test.

Two PowerShell traps are fixed in the scripts and worth not re-introducing:

- A native program's stderr (psql's `NOTICE`, cargo's progress) becomes an error record that
  `$ErrorActionPreference = "Stop"` treats as fatal, so repeated or idempotent runs aborted. Each
  script runs native programs with the preference relaxed and judges them by exit code alone; psql
  takes its options before the database URI.
- A `Get-Content`/`Set-Content` round trip on a UTF-8 markdown file reads it as ANSI and writes back a
  different encoding, which turned every em dash in this file into mojibake once already. The rule for
  that is `AGENTS.md` #5.

## Definition of done for the first session (met)

A `world` registered in the database, capacity published, reservations proven atomic under
concurrency, and the reaper proven to free a dead world's leases - all against the live database,
repeatedly, with no client, no protocol and no simulation. That is M2, and it is done.

## What is deliberately not here yet

- **What a drop puts on the wire.** Dropping deducts the stack, prints it, and sends nothing: both candidate
  messages crashed a live client and neither is in any capture. There is **no switch** for that, deliberately
  - a knob nobody can use is worse than the plain behaviour. The ground item waits for a capture of GWPS
  actually dropping something, and its handler is the unfinished one, so that capture may never exist.
- **`/tp` and `/teleport` are implemented.** The world starts the cycle itself: the seat reserved for
  the destination, the transfer row written, `TRANSFER_GAME_SERVER_INFO` then `MAP_UPDATE_CURRENT` sent
  (`crates/asura-world/src/travel.rs:181`, reached from `crates/asura-world/src/chat.rs:339`). `/maps`,
  `/stuck` and every chat channel are implemented and **not gated**: a spoken line is sent, a generated
  answer is printed, because printing is the half that has always worked - the reference's `/maps`
  answered into its own log and never onto a screen.
- **No trade, no salvage, no pick-up**: an item can be dropped (silently), moved and worn, and nothing
  else. Nothing picks a dropped item up, because the reference has no handler for that either
  (`docs/history/gwps-parity/handler-status.md`).
- **No NPCs, no combat, no quests**: an instance holds the clients playing in it and nothing else - the
  players are introduced to each other and told when one leaves
  (`crates/asura-world/src/instance.rs`, `PeerView`), and no session registry is kept in memory: the
  service resolves a connection per connection.
- **Character selection is handled**: `AUTH_CMSG_CHANGE_PLAY_CHARACTER` is answered
  (`crates/asura-auth/src/lib.rs:416-462`, which calls `asura_db::login::select_character`), and a real
  client selected a character (`docs/work/project-status.md`). The login batch reports the character the session
  points at; the message that chooses a different one is answered by rewriting the session's character.
- **No CI.** Tests run locally first, and `tools/test-battery.ps1` is the one command that stands in for
  it (10 steps, PASS/FAIL each, and the last one is a human with a game client).
