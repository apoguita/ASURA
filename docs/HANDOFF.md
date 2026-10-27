# Start here (session handoff)

State of the project as of the last commit, so a new session can begin without re-reading everything.

## Where this stands

- **Items work on a live client.** Dropping takes the stack down (`count = 1` of a hundred is what a
  client sends, and the reference refuses exactly that), the ground agent wears the item's *model*, moving
  an item between slots is answered and persisted, and wearing one moves it into the equipped bag and
  re-sends `UPDATE_AGENT_VISUAL_EQUIPMENT`. Chat speaks on `!`/`#`/`$` ungated, as the reference does, and
  `/stuck` and `/maps` answer. `docs/08-next-inventory-model.md` records what the reference got wrong and
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
- **The parity audit exists: `docs/09-parity-audit.md`.** Every GWPS behaviour and ASURA's counterpart,
  row by row, with `file:line` on both sides and a verdict (1:1 / AHEAD / DEVIATION / BEHIND / STUB).
  51 of 78 are indistinguishable, 10 are ASURA's own, 5 differ on purpose, 11 are behind but collapse
  into three work items, and 1 is a stub in both. Read it before deciding that something is "missing".
- **M2 delivered: placement is safe, and it is proven against the real database.** The `asura-db`
  crate now owns registration, the atomic reservation, the lease ledger, the reaper and the
  reconciler; `asura-fakeworld` runs the whole sequence as a harness; the acceptance suite passes
  repeatedly (`.\tools\m2-verify.ps1`). No game client was involved — that was the point.
- **M3 is delivered: the patched client reaches the character list against this service.** Measured
  2026-10-26: build 39112 logged in as `user1@example.com`, took the EULA, and reached character select.
  `docs/06-reference-session.md` holds the whole session — both directions, every header and size — and
  `asura-auth` reproduced it line for line up to the client's travel request, which it refused while no
  world server existed yet. The service runs: it reads its configuration, refuses a stale schema,
  binds a socket, classifies each connection, performs the key exchange with real randomness, answers
  the file check, the computer-info preamble and the session salt, validates the identity pair, and
  sends the reference's five-message login batch. `ASURA_TRACE=1` logs every message both ways.
  `.\tools\m3-verify.ps1` runs the whole path in one command (it was 105 tests when M3 landed; the
  suite is 175 now).
- **What the pieces are:** classification from the first four bytes, framing by the table's fixed size
  per header with no length field, the key exchange (ARC4, the fixed-constant Diffie–Hellman, the
  protocol's own key derivation, pinned by fixtures **compiled from the reference's own C**), typed
  message layouts for the login path, the reference client, the login decision in `asura-db::login`,
  the migration record, and a seed for the three identity pairs the rig's portal DLL hands out.
- **Two rules the wire taught us, both now documented:** a login does **not** spend the session row
  (GWPS never writes its session table, and the rig presents the same fixed pair every time), and the
  login batch has a fixed order the client expects (`docs/03-protocol.md` section 4).
- **The real client has been pointed at this service, and the risky half is confirmed.** Client build
  39112, on this machine, 2026-10-26: it classified as an auth connection, completed the key exchange,
  and **its subsequent traffic decrypted correctly in our ARC4** — which is the first proof that the
  fixed-constant Diffie–Hellman and the protocol's own key derivation match the real client rather
  than just the reference's C. Its opening messages were captured byte for byte: `0x8001`
  `SEND_COMPUTER_INFO` (**42 bytes, variable-length, `u16` counts — not the table's 138**) and `0x8002`
  `SEND_COMPUTER_HASH` (22 bytes), 64 bytes in total. Both are now decoded, and the capture is a
  regression test.
- **The portal is a real HTTP service after all, and this project has to serve it**
  (`docs/03-protocol.md` §4, corrected): with `-portal 127.0.0.1` the client calls four
  `/Spawned/WebGate/…` endpoints on `localhost:6601` and takes its `user_id`/`session_id` from them.
  The reference web gate (`PreSearing/tools/webgate.py`) answered all four during a working run.
  **Not yet implemented here** — a run against this service used the reference's web gate.
- **Where it stops, and it is now the top of the queue:** after that web-gate login the client opened a
  second auth connection, sent the same preamble, and reset it instead of sending the portal login.
  Against GWPS the login did arrive, nine seconds later, and worked. The reference's own log shows it
  *mis-framing* that preamble (`Unhandled AuthSrv packet with header 35`), so it is not that GWPS
  handles the message better — something else differs. `docs/03-protocol.md` §10 records the two cheap
  pieces of evidence that would settle it: the client's on-screen error text, and a trace with
  timestamps and connection ids.

- **M1's scaffold still holds:** the workspace compiles, `cargo test --workspace` is green (the M1/M2
  part of it: 12 unit + 4 live-database placement tests that skip cleanly without a database), and
  `cargo clippy --workspace --all-targets` is clean.
- **The login identities exist.** The client does not ask a web service who it is — the rig's portal
  DLL hands it a fixed pair (`PreSearing/portal/portal.c`) — so `tools/seed-dev.ps1` writes the rows
  those pairs must resolve to: three accounts with a character each. Idempotent; re-running reopens
  the sessions.
- **M1's scaffold still holds:** the workspace compiles, `cargo test --workspace` is green (the M1/M2
  part of it: 12 unit + 4 live-database placement tests that skip cleanly without a database), and
  `cargo clippy --workspace --all-targets` is clean.
- **A protocol document was wrong, and is now corrected.** `docs/03-protocol.md` section 1 claimed a
  `u16 length` after every message header. The reference has no length field at all: `pack_msg`
  writes the format's fields and nothing else, so a message's size is a property of its header.
  Writing the length would have produced the client's opaque `007`. The evidence is recorded in the
  document, along with the related trap that `sizeof(struct)` is not the wire size
  (`AuthSrv_SessionInfo`: 14 bytes in C, 10 on the wire).
- **Fifteen documents and eight ADRs** describe what is built, what is refused, and why.
- **GWPS is research material** (`docs/adr/0005-gwps-reference-only.md`). ASURA is written from
  scratch; nothing is ported or patched there.
- **AzerothCore is the design authority** for structure (`docs/adr/0006-azerothcore-is-the-reference.md`).
  A deviation has to be recorded before it is implemented — see ADR 0008 for how that is done well and
  how it is done badly.

## What M2 delivered

| piece | where |
|---|---|
| lease ledger migration (`lease` table: one row per reserved slot, unique per session) | `crates/asura-db/migrations/0002_leases.sql` |
| typed placement queries — register, heartbeat, reserve, release, reap, reconcile, remove | `crates/asura-db/src/placement.rs` |
| synchronous, idempotent migrations and the schema tests that keep them honest | `crates/asura-db/src/lib.rs` |
| the concurrency acceptance test (capacity N-1, N concurrent reservations, exactly N-1 win; full-pool refusal; reaper; reconcile) | `crates/asura-db/tests/placement.rs` |
| the fake-world harness (`asura-fakeworld scenario`) | `crates/asura-fakeworld/` |
| one command: integration tests + harness scenario | `tools/m2-verify.ps1` |
| dependencies, added with the code that needs them: `sqlx` 0.8 (PostgreSQL), `tokio` 1, `uuid` | `Cargo.toml` |

The acceptance result, verbatim from a run: capacity 4 with 5 concurrent reservations → 4 won, 1
refused; `leased` ends at capacity; the ledger matches; the reaper freed a silent world's leases and
left a fresh world's alone; reconcile rebuilt the counter from the ledger. Repeated 5×, no flakiness.

**Why a ledger beside the counter:** `world_pool.leased` makes the reservation atomic; the `lease`
rows make a lease *identifiable*, which is what the reaper needs to free a dead world's slots and the
reconciler needs to rebuild `leased` from the worlds instead of trusting the table
(`docs/05-gw2-lessons.md`, rule 2). They are inserted, released and reaped together in one
transaction, so `count(lease) <= leased <= capacity` holds at every commit and the
`world_pool_never_oversold` CHECK never fires.

## Environment status on this machine

| what | state | evidence |
|---|---|---|
| **Rust** | installed, 1.99.1 | `cargo test --workspace` → **175 tests, 0 failures**; `cargo clippy --workspace --all-targets` → clean; `cargo fmt --all -- --check` → clean |
| cargo on PATH | **not** on PATH in a fresh shell | use `C:\Users\Apo\.cargo\bin\cargo.exe`, or add that directory to PATH permanently |
| **PostgreSQL** | installed, 16.15, service **Running**, and **provisioned** | role `asura`, database `asura`, **6 migrations applied / 11 tables** (`account`, `bag`, `characters`, `item`, `lease`, `map_config`, `schema_version`, `session`, `transfer`, `world`, `world_pool`); credentials in `.env` (gitignored) |
| psql | `C:\Program Files\PostgreSQL\16\bin\psql.exe` | not on PATH |
| client rig (Reforged + stealth hooks) | present from the PreSearing work | `tools/start-server.ps1`, then the client; the trace is the verdict |

**Database is ready.** `ASURA_DATABASE_URL` is in `.env` (gitignored, never committed). Every migration
is applied; `crates/asura-db/tests/placement.rs` verified the pool invariants against the live database —
at capacity the reservation `UPDATE ... WHERE leased < capacity` returns no row (the refusal signal), and
an over-capacity row is rejected by `world_pool_never_oversold` — and `tests/inventory.rs` verifies that a
character is seeded once, that its items read back identical to what was written, and that a position
survives.

If the superuser password is ever lost again, `tools/pg-recover.ps1` (run elevated) does the standard
recovery. It restores secure auth before it finishes, and it writes the new credentials to `.env`.

Setup in full:

```powershell
$env:PGPASSWORD = "<postgres superuser password>"
.\tools\db-setup.ps1            # creates role + database, applies every migration, lists the tables
```

## The ten-minute idle run (T13's remaining clause)

The cadence and the accounting are covered by `an_idle_pair_keeps_its_rhythm_and_its_count`. What a test
cannot see is the process's working set, so that half stays a hand run:

```powershell
# watch the server's memory while the long variant holds one connection for ten minutes
Get-Process asura-world | Select-Object -ExpandProperty WorkingSet64
cargo test -p asura-world --test load a_ten_minute_idle_pair -- --ignored --nocapture
```

The test prints the tick and ping counts and the world's player count when it finishes; a crash inside the
window fails it, and a working set that climbs without settling is the thing to look at.

## Run it: two commands

```powershell
.\tools\start-server.ps1        # database -> web gate :6601 -> auth :6112 -> world :6113, logs tailed
.\tools\stop-server.ps1         # the same, backwards, and it says what is still listening
```

**It is quiet by default, and that is a rule now**: `-Trace` prints every packet, and the packet log is
off unless asked for, because a world tick every 500 ms is three lines a second forever and it buries the
lines that matter. What is *always* printed is the events — `auth: login ok for …`, `auth: travel to map
146 placed on world 8001 …`, `world: … joined map 146 … with 6 bag(s) and 9 item(s)`, a saved position, a
dropped item, a moved item, a worn item, a refused move, a chat that was not sent. Those were inside the
trace block until a live session pointed out that the console was unreadable; per-packet lines are the
only ones behind the flag now, and `-Trace` still exists for exactly the case it was built for.

`start-server.ps1` exists because `play.ps1` alone is not enough for a client and the missing half is
invisible: with `-portal 127.0.0.1` the client asks the **reference web gate on 6601** who it is before
it sends a login, and with nothing there it opens the auth connection, does the key exchange, sends its
preamble, and closes. That is what "I cannot connect to the server" looked like on this machine — with
the server running perfectly and `AUTH_CMSG_PORTAL_ACCOUNT_LOGIN` never arriving. `start-server.ps1`
starts PostgreSQL (if it is stopped), the web gate, and then hands over to `play.ps1`; `stop-server.ps1`
kills the services and the gate, verifies the ports are free, and notes what a hard stop leaves behind
(a connected player's pool slot, which only the unscheduled reaper frees).

Then prove M2 at any time:

```powershell
.\tools\m2-verify.ps1           # integration tests + the fake-world scenario, live database
```

(No network is needed for either once dependencies are cached. If `cargo` cannot reach crates.io,
the sandbox token may be blocking TLS — run it from a normal shell, or with full access.)

Scripts in this repository stay ASCII-only: Windows PowerShell 5.1 reads a BOM-less file as ANSI,
where a UTF-8 em dash decodes to a smart quote and breaks the parse at a line far from the cause.

## Decisions already made (do not re-litigate without reading these)

| decision | where |
|---|---|
| **Rust** for the whole stack, translated from AzerothCore's structure | `docs/adr/0001-rust.md`, `docs/adr/0007-rust-translation.md` |
| **PostgreSQL**, a justified deviation from AzerothCore's MySQL; mechanisms unchanged | `docs/adr/0008-database-vendor.md` |
| Separate project from GWPS | `docs/adr/0002-separate-project.md` |
| Simulation is in scope, deferred behind the base structure | `docs/adr/0003-simulation-in-scope.md` |
| AzerothCore's server structure, adopted as a whole | `docs/adr/0004-port-azerothcore-structure.md` |
| GWPS is reference only; instance creation is settled by an experiment in this stack | `docs/adr/0005-gwps-reference-only.md` |
| AzerothCore is the design authority (structure); deviations are recorded | `docs/adr/0006-azerothcore-is-the-reference.md` |
| The one rule: never send a client to something that is not already answering | `docs/02-architecture.md` |

## Where the next session starts

M4 is delivered — a real client logs in, travels, and stands in a map (`git log`, `4f610a8`) — and it now
carries an inventory and comes back where it logged out.

1. **Verify the baseline** (3 minutes): `cargo test --workspace --offline` → **175 passing**, of which 16
   are live-database tests in `asura-db` (login, placement, inventory, migrations) and 6 drive real
   sockets in `asura-world` (the load, a drop, a return). Then `.\tools\test-battery.ps1`. If a count
   changed, read `git log` before assuming it is broken.
2. **What is left of the reference's dispatch is character creation**: the four
   `GAME_CMSG_CHAR_CREATION_*` handlers (`docs/07-status.md`). They are blocked on a
   *character-creation* instance (`MapType_CharacterCreation`, which this world does not host) and on
   porting the default-equipment table `GetDefaultEquipments` answers with (`GmDefaultArmors.c`, 2820
   lines of it). The second is now generation work rather than design: `tools/gen-pvp-items.py` and
   `tools/gen-default-inventory.py` are the two shapes to copy, and equipment is item rows.
3. **Then the things the reference never wrote**: picking a dropped item back up
   (`GAME_CMSG_INTERACT_ITEM`, `0x803F` — the last piece that makes a drop reversible), splitting a
   stack by hand (`0x8075`), buying and selling (`GAME_CMSG_TRANSACT_ITEMS`, `0x804D`), swapping two
   items in one move, and `UNEQUIP_ITEM` (`0x804F`), whose layout is still unmeasured. Each starts with
   a *measurement* of what the client sends; the reference's C has now been insufficient four times, and
   a live trace sufficient every time.
4. **Use the reference session as the spec.** `docs/06-reference-session.md` is the whole working login,
   both directions; `tools/patch-gwps-auth-trace.py` reproduces the logging that produced it,
   `tools/compare-replies.py` diffs two servers' replies field by field, and `tools/compare-load.py`
   diffs the world's load stream against a capture of the reference working.
5. **The real client is the last word on anything the wire does.** Everything above is harness-only, and
   three messages that no capture contains have now crashed a live client (`ITEM_REMOVE`, an item agent,
   chat). `tools/start-server.ps1` plus the trace in that console is the verdict — and if a client
   crashes, the log's last `world ->` line names the message that did it.

**Decided, and not to be re-opened without a reason:**

- The portal user id and the session id are separate columns, not one reused column
  (`crates/asura-db/migrations/0003_portal_identity.sql`, `docs/02-architecture.md`). The migration
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
  was nothing to copy — swapping is a decision nobody has made yet.
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
| the seed: the reference's own six bags and nine items, generated from the capture | `crates/asura-db/src/inventory/seed.rs` (generated) |
| the capture itself, kept as the fixture the rows must reproduce | `crates/asura-protocol/src/game/default_inventory.rs` (generated) |
| one generator for both halves: the seed rows and the captured bytes | `tools/gen-default-inventory.py` |
| `CREATE_NAMED_ITEM`, `ITEM_MOVE_TO_LOCATION`, `ITEM_SET_PROFESSION` | `crates/asura-protocol/src/game/mod.rs` |
| the item block from rows, the bags in model order, the worn ids from the equipped bag | `crates/asura-world/src/lib.rs` (`inventory_block`, `items_block`, `player_block`) |
| `DROP_ITEM`, finished rather than copied: the stack is taken down, the agent wears the item's model | `crates/asura-world/src/lib.rs` (`0x802C`, `drop_item`, `drop_messages`) |
| `ITEM_MOVE` and `EQUIP_ITEM`, which the reference does not dispatch at all | same file (`0x8072`, `0x8030`) + `asura_db::inventory::{move_item, first_free_carrying_slot, take_from_stack}` |
| the position saved on leaving and resumed on entering | `asura_db::game::record_position`/`position`, `Standing::resume` |
| the live-database proof: seeding, round trip, drop scoping, position | `crates/asura-db/tests/inventory.rs` |

## The pieces the map data added (D1-D3)

A map's geometry is one `Gw.dat` entry, compressed, and the reference reads it in three steps: the
archive's index, its decompressor, and `GmMapDataImport.c`'s chunk reader. All three are here.

| piece | where |
|---|---|
| the archive index: header, file table, TOC hash, one entry's bytes | `crates/asura-world/src/gwdat.rs` |
| `FaCompress.c`'s decoder: the bit window, the node tree, the block loop | `crates/asura-world/src/fa_compress.rs` |
| its six static tables, generated from the reference and `--check`ed | `tools/gen-fa-compress-tables.py` -> `src/fa_compress_tables.rs` |
| the container and the path graph: planes, trapezoids, x/y/sink nodes, portals | `crates/asura-world/src/map_data.rs` |
| "which trapezoid is this position in": `PathFindTrapezoid` | same file (`PathStaticData::trapezoid_at`) |
| every catalogue map imported out of a real `Gw.dat`, with its spawn point | `crates/asura-world/examples/map-census.rs` |
| the reference's own fingerprints of the same blobs, as a fixture | `crates/asura-world/tests/fixtures/gwdat-pathing.csv` |
| the test that compares this repo's import to them, column by column | `crates/asura-world/tests/map_data.rs` |
| the three-way check: reference binary, fixture, this repo | `tools/parity-pathing.py --check` |
| the numbers, and what the reference does that a tidier reader would not | `docs/10-parity-queue.md`, item D |
| the A*: neighbour and portal visits, the heap, the waypoint builder | `crates/asura-world/src/pathfind.rs` |
| its fixed-point reciprocal and square-root tables, generated from `GmPaths.c` | `tools/gen-path-tables.py` -> `src/path_tables.rs` |
| the walks both sides run: every catalogue spawn point and eight offsets from it | `tools/gen-path-cases.py` -> `tests/fixtures/path-cases.txt` |
| the reference's own answers for them, waypoint bits included | `tools/parity-path.py --write` -> `tests/fixtures/path-results.csv` |
| the reference's path finder as a runnable oracle (its translation unit, unmodified) | `tools/path-oracle.c`, built by `tools/build-path-oracle.ps1` |
| the case-by-case comparison, and the four in-module tests | `crates/asura-world/tests/pathfind.rs`, `src/pathfind.rs` |
| the graphs a world serves with, imported at startup from `Gw.dat` | `crates/asura-world/src/lib.rs` (`PathGraphs`), `src/main.rs` (`--gwdat`, `ASURA_GW_DAT`) |
| the walk itself: destination, direction, speed, waypoints, and the tick that advances them | `crates/asura-world/src/agent.rs` (`Motion`, `begin_walk`, `advance`) |
| `MOVE_TO_COORD`'s answer: waypoint 0, or the refusal that is a position broadcast | `crates/asura-world/src/lib.rs` (`0x803E`) |

The grep that matters is `python tools/parity-pathing.py --check`: it runs
`..\PreSearing\build\RelWithDebInfo\gwdat_pathing.exe` over the catalogue's blobs, compares its output to
the fixture, and compares both to this repo's importer. Any of the three drifting fails it. It skips (exit
0) when `Gw.dat` or the built reference tool is absent, so it is safe on a machine without them.

`python tools/parity-path.py --check` is the same shape for the search, with one difference: the reference
has no tool of its own for `PathFinding`, so `tools/path-oracle.c` supplies one by compiling the reference's
own translation unit and calling it. That needs MSVC and the libraries the reference's CMake build already
produced, which is why `tools/build-path-oracle.ps1` exists and why the binary is gitignored — the fixture
it produced is committed, and the tests run from the fixture without a compiler.

One corner of that comparison is a reference defect rather than a port difference:
`PathBuildAddWaypointAndReduce` reads its intersection parameters after `FindIntersectionPoint` has
declined to write them (parallel lines), so the reference answers one of the 96 cases with a different
waypoint list on different runs. `tools/parity-path.py` detects that by running the oracle three times,
compares those cases on the answer alone, and lists them in the fixture's header. If a future case starts
flapping, that is how it will show up.

## The pieces M3 added

| piece | where |
|---|---|
| connection classification: the three version headers, their sizes, and the refusal to guess | `crates/asura-protocol/src/client/version.rs` |
| framing: header plus table-sized body, no length field, with encode-side size validation | `crates/asura-protocol/src/client/frame.rs` |
| the auth message table: 100 rows with names, sizes and field layouts | `crates/asura-protocol/src/client/tables.rs` (generated) |
| the generator that reads it out of the reference, and checks its own output | `tools/gen-auth-tables.py` |
| the portal identity column, and why it is not the account id | `crates/asura-db/migrations/0003_portal_identity.sql` |
| the login decision: find the session, close the others, read the characters | `crates/asura-db/src/login.rs` |
| the refusal rules under test against the live database | `crates/asura-db/tests/login.rs` |
| the three identity pairs the rig's portal DLL presents, seeded idempotently | `tools/seed-dev.ps1` |
| one command for the whole login path, client not needed yet | `tools/m3-verify.ps1` |
| the database's record of its own migrations, and the refusal to run against a stale schema | `crates/asura-db/src/migrations.rs` (+ `tests/migrations.rs`) |
| ARC4, ported from `arc4.c`, checked against published vectors and the reference's keystream | `crates/asura-protocol/src/client/arc4.rs` |
| the key exchange: seeds, fixed-constant DH, and the protocol's own key derivation | `crates/asura-protocol/src/client/handshake.rs` |
| reference outputs for the handshake, compiled out of GWPS's own C | `tools/gen-handshake-fixtures.py` -> `client/handshake_fixtures.rs` |
| the login batch's fixed 591-byte settings blob, parsed out of the reference | same generator -> `client/login_fixtures.rs` |
| typed layouts for the messages the login path uses, with sizes checked against the table | `crates/asura-protocol/src/client/messages.rs` |
| a reference client: the same protocol from the other end, so the service is testable without the game | `crates/asura-protocol/src/client/peer.rs` |
| the service itself: listener, key exchange, ARC4 both ways, login, trace | `crates/asura-auth/src/lib.rs` (+ `src/main.rs`) |
| a client anyone can run against a live service | `crates/asura-auth/examples/login.rs` |
| the end-to-end proof: a real socket, the real database, the character list | `crates/asura-auth/tests/login.rs` |

The generated files carry `#[rustfmt::skip]` and are checked by `python tools/gen-auth-tables.py
--check` and `python tools/gen-handshake-fixtures.py --check`; if either disagrees with the reference,
that command fails rather than letting the port drift. `--gwps` points them at a checkout other than
`..\PreSearing`. The fixture generator needs a C compiler (it looks for `cl.exe` under Visual Studio,
then gcc) and refuses to write fixtures it could not compile.

Two PowerShell traps are fixed in the scripts and worth not re-introducing:

- A native program's stderr (psql's `NOTICE`, cargo's progress) becomes an error record that
  `$ErrorActionPreference = "Stop"` treats as fatal, so repeated or idempotent runs aborted. Each
  script runs native programs with the preference relaxed and judges them by exit code alone; psql
  takes its options before the database URI.
- **Never rewrite a UTF-8 markdown file with `Get-Content`/`Set-Content`.** Windows PowerShell reads it
  as ANSI and writes back a different encoding, which turned every em dash in this file into mojibake
  once already. Use the file tools, or `-Encoding utf8` with `[System.IO.File]::ReadAllText`.

## Definition of done for the first session (met)

A `world` registered in the database, capacity published, reservations proven atomic under
concurrency, and the reaper proven to free a dead world's leases — all against the live database,
repeatedly, with no client, no protocol and no simulation. That is M2, and it is done.

## What is deliberately not here yet

- **What a drop puts on the wire.** Dropping deducts the stack, prints it, and sends nothing: both candidate
  messages crashed a live client and neither is in any capture. There is **no switch** for that, deliberately
  — a knob nobody can use is worse than the plain behaviour. The ground item waits for a capture of GWPS
  actually dropping something, and its handler is the unfinished one, so that capture may never exist.
- **`/tp` and `/teleport`**: they need a *world-initiated* travel (reserve a slot, write a transfer row,
  send `TRANSFER_GAME_SERVER_INFO`, then `MAP_UPDATE_CURRENT`). The redirect is **not** the unknown part:
  the reference sends it on every `/tp`, its 46-byte layout is the one `game::TRANSFER_GAME_SERVER_INFO`
  already carries, and the cycle is written up in `docs/03-protocol.md` §5. What is missing is a way for a
  world to start it — today only the client can, through the auth service. `/maps`, `/stuck` and every chat
  channel are implemented and **not gated**: a spoken line is sent, a generated answer is printed, because
  printing is the half that has always worked — the reference's `/maps` answered into its own log and never
  onto a screen.
- **No trade, no salvage, no pick-up**: an item can be dropped (silently), moved and worn, and nothing
  else. Nothing picks a dropped item up, because the reference has no handler for that either
  (`docs/07-status.md`).
- **No character creation**: the four `CHAR_CREATION_*` handlers need a creation instance and the default
  equipment table.
- **One agent, one player**: no NPCs, no other players, no combat, no quests, and no session registry in
  memory — the service resolves a connection per connection and keeps no record of who is connected.
- **No character selection yet**: the client's `AUTH_CMSG_CHANGE_PLAY_CHARACTER` is logged as unhandled,
  and `asura_db::login::select_character` exists but nothing calls it. The login batch reports the
  character the session already points at; choosing one is the next login-path message.
- **No CI.** Tests run locally first, and `tools/test-battery.ps1` is the one command that stands in for
  it (10 steps, PASS/FAIL each, and the last one is a human with a game client).

## Reading order for a fresh session

`docs/HANDOFF.md` (this file) → `docs/01-scope.md` → `docs/02-architecture.md` →
`docs/03-protocol.md` → `docs/roadmap.md` → the ADR that matches the question you have.
`docs/00-glossary.md` defines every term if something reads as jargon.
