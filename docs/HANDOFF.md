# Start here (session handoff)

State of the project as of the last commit, so a new session can begin without re-reading everything.

## Where this stands

- **M2 delivered: placement is safe, and it is proven against the real database.** The `asura-db`
  crate now owns registration, the atomic reservation, the lease ledger, the reaper and the
  reconciler; `asura-fakeworld` runs the whole sequence as a harness; the acceptance suite passes
  repeatedly (`.\tools\m2-verify.ps1`). No game client was involved — that was the point.
- **M3 is delivered: the patched client reaches the character list against this service.** Measured
  2026-10-26: build 39112 logged in as `user1@example.com`, took the EULA, and reached character select.
  `docs/06-reference-session.md` holds the whole session — both directions, every header and size — and
  `asura-auth` reproduced it line for line up to the client's travel request, which it refuses because
  no world server exists yet. The service runs: it reads its configuration, refuses a stale schema,
  binds a socket, classifies each connection, performs the key exchange with real randomness, answers
  the file check, the computer-info preamble and the session salt, validates the identity pair, and
  sends the reference's five-message login batch. `ASURA_TRACE=1` logs every message both ways.
  **105 tests pass** (`cargo test --workspace`), `.\tools\m3-verify.ps1` runs the whole path in one
  command, and clippy plus `cargo fmt --check` are clean.
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
| **Rust** | installed, 1.99.1 | `cargo test --workspace` → **93 tests, 0 failures**; `cargo clippy --workspace --all-targets` → clean |
| cargo on PATH | **not** on PATH in a fresh shell | use `C:\Users\Apo\.cargo\bin\cargo.exe`, or add that directory to PATH permanently |
| **PostgreSQL** | installed, 16.15, service **Running**, and **provisioned** | role `asura`, database `asura`, **8 tables** applied (the 7 from 0001 plus `lease`), and 0003 added the `session.portal_user` column; credentials in `.env` (gitignored) |
| psql | `C:\Program Files\PostgreSQL\16\bin\psql.exe` | not on PATH |
| client rig (Reforged + stealth hooks) | present from the PreSearing work | not needed until M4 |

**Database is ready.** `ASURA_DATABASE_URL` is in `.env` (gitignored, never committed). Both
migrations are applied; `crates/asura-db/tests/placement.rs` verified the pool invariants against the
live database — at capacity the reservation `UPDATE ... WHERE leased < capacity` returns no row (the
refusal signal), and an over-capacity row is rejected by `world_pool_never_oversold`.

If the superuser password is ever lost again, `tools/pg-recover.ps1` (run elevated) does the standard
recovery. It restores secure auth before it finishes, and it writes the new credentials to `.env`.

Setup in full:

```powershell
$env:PGPASSWORD = "<postgres superuser password>"
.\tools\db-setup.ps1            # creates role + database, applies every migration, lists the tables
```

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

## Next session: M4 — the game connection

M3 is delivered. The client logs in against this service, takes the EULA, and reaches character select.
What it asks for next is a **world**, and there is none.

1. **Verify the baseline** (2 minutes): `cargo test --workspace --offline` → **105 passing** (9 of
   them end-to-end over a socket, 6 live-database login tests, 4 live-database placement tests). Then
   `.\tools\m3-verify.ps1`. If a count changed, read `git log` before assuming it is broken.
2. **Answer the travel request.** `REQUEST_GAME_INSTANCE` (`0x8029`, 26 bytes: request, map type, map
   id, region, district) must be answered with `GAME_SERVER_INFO` (`0x0009`, **42 bytes**: request id,
   map token, map id, `host[24]` holding family/port/address, player token) and then a request
   response — that is step 21 of `docs/06-reference-session.md`, with GWPS's own bytes for reference.
   This is `AUTH_SMSG_GAME_SERVER_INFO`, and the message table already carries its layout.
3. **Then something has to be listening at that address** and able to accept a `GAME_CMSG_VERSION`
   connection — the second connection in the reference session (step 23). That is the world server,
   and it is the boundary this project drew on purpose (`docs/adr/0003-simulation-in-scope.md`): the
   auth service's job ends at the redirect.
4. **Use the reference session as the spec.** `docs/06-reference-session.md` is the whole working
   login, both directions; `tools/patch-gwps-auth-trace.py` reproduces the logging that produced it,
   and `tools/compare-replies.py` diffs two servers' replies field by field. Reading the reference's C
   was not enough to get the message shapes right twice; the measurement was.
5. **M4 proper — end-to-end travel** stays blocked on a world server that accepts its own gameplay
   connections (`docs/04-from-gwps.md`, step 1).

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

- The acceptance proof itself: no real game client has been pointed at this service. Everything that
  can be checked without one is checked, and the packet trace is built for the moment it is.
- No world server work at all: no gameplay connection, no travel, no simulation. The auth service
  answers a login and nothing else (`docs/roadmap.md`, M4 onwards).
- No CI. Tests run locally first, and `tools/m3-verify.ps1` is the one command that stands in for it.
- No session bookkeeping in memory: the service resolves a login per connection and keeps no registry
  of who is connected. That registry is what "the newest connection wins" will need at M4.
- No character selection yet: the client's `AUTH_CMSG_CHANGE_PLAY_CHARACTER` is logged as unhandled,
  and `asura_db::login::select_character` exists but nothing calls it. The login batch reports the
  character the session already points at; choosing one is the next login-path message.

## Reading order for a fresh session

`docs/HANDOFF.md` (this file) → `docs/01-scope.md` → `docs/02-architecture.md` →
`docs/03-protocol.md` → `docs/roadmap.md` → the ADR that matches the question you have.
`docs/00-glossary.md` defines every term if something reads as jargon.
