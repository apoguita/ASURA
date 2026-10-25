# Start here (session handoff)

State of the project as of the last commit, so a new session can begin without re-reading everything.

## Where this stands

- **M2 delivered: placement is safe, and it is proven against the real database.** The `asura-db`
  crate now owns registration, the atomic reservation, the lease ledger, the reaper and the
  reconciler; `asura-fakeworld` runs the whole sequence as a harness; the acceptance suite passes
  repeatedly (`.\tools\m2-verify.ps1`). No game client was involved — that was the point.
- **M3 is nearly done, and the service runs.** `asura-auth` is a real program now: it reads its
  configuration, connects, verifies the schema version, binds a socket, classifies each connection,
  runs the key exchange with real randomness, scrambles both directions with ARC4, takes the login
  message, validates the identity pair against the database, and answers with the character list —
  with a per-message trace behind `ASURA_TRACE=1` that prints direction, header, name, size and the
  first bytes, the way GWPS does. A reference client (`asura-protocol::client::peer`) and
  `cargo run -p asura-auth --example login` drive it without the game.
  **91 tests pass** (`cargo test --workspace`), `.\tools\m3-verify.ps1` runs the whole path in one
  command (five steps, ending with a client logging in over a socket against the live database), and
  clippy plus `cargo fmt --check` are clean.
- **What the pieces are:** classification from the first four bytes, framing by the table's fixed size
  per header with no length field, the key exchange (ARC4, the fixed-constant Diffie–Hellman, the
  protocol's own key derivation, pinned by fixtures **compiled from the reference's own C**), typed
  message layouts for the login path, the reference client, the login decision in `asura-db::login`,
  the migration record, and a seed for the three identity pairs the rig's portal DLL hands out.
- **Two rules the wire taught us, both now documented:** a login does **not** spend the session row
  (GWPS never writes its session table, and the rig presents the same fixed pair every time), and the
  login batch has a fixed order the client expects (`docs/03-protocol.md` section 4).
- **Still missing for M3's acceptance:** the `AUTH_SMSG_ACCOUNT_SETTINGS` and `AUTH_SMSG_ACCOUNT_INFO`
  messages GWPS interleaves into that batch, and the packet-trace diff against a real client.
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
| **Rust** | installed, 1.99.1 | `cargo test --workspace` → **91 tests, 0 failures**; `cargo clippy --workspace --all-targets` → clean |
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

## Next session: M3's acceptance — the real client, and two missing messages

The service answers a client end to end. What remains is the part only the real client can settle.

1. **Verify the baseline** (2 minutes): `cargo test --workspace --offline` → **91 passing** (7 of
   them end-to-end over a socket, 6 live-database login tests, 4 live-database placement tests). Then
   `.\tools\m3-verify.ps1`. If a count changed, read `git log` before assuming it is broken.
2. **Capture the reference trace.** Run GWPS with `GWPS_TRACE=1`, log in to the character list, and
   keep that trace as the reference side of the diff. Then run this service with `ASURA_TRACE=1` and
   diff the two. This is M3's acceptance criterion (`docs/roadmap.md`).
3. **Add the two messages the batch is missing** — `AUTH_SMSG_ACCOUNT_SETTINGS` (`0x0016`, a fixed
   ~500-byte blob that GWPS ships verbatim in `AuthSrv_SendAccountSettings`) and
   `AUTH_SMSG_ACCOUNT_INFO` (`0x0011`, whose fields `AuthSrv_SendAccountInfo` fills). The trace diff
   says whether the client needs them to reach the character list; the reference sends them, so
   assume it does. Both are fixed layouts, so both belong in `client::messages` with a test that sizes
   them against the table — and the settings blob is a copied fact, so generate it from the C rather
   than retyping it (`tools/gen-handshake-fixtures.py` is the pattern).
4. **Answer the settings uploads.** `AUTH_CMSG_SETTING_UPDATE_SIZE` and `..._CONTENT` are already
   acknowledged; if the capture shows others, acknowledge those too. An unacknowledged upload is a
   hang, and it is the trap the reference's own history is built on (`docs/03-protocol.md` §6).
5. **M4 — end-to-end travel** stays blocked on M3 and on a world server that accepts its own
   gameplay connections (`docs/04-from-gwps.md`, step 1).

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

- Two messages of the login batch: `AUTH_SMSG_ACCOUNT_SETTINGS` and `AUTH_SMSG_ACCOUNT_INFO`. The
  service sends the characters, the friend-stream end and the request response; the reference
  interleaves two more, and the trace diff will say whether the client insists on them.
- No world server work at all: no gameplay connection, no travel, no simulation. The auth service
  answers a login and nothing else (`docs/roadmap.md`, M4 onwards).
- No CI. Tests run locally first, and `tools/m3-verify.ps1` is the one command that stands in for it.
- No session bookkeeping in memory: the service resolves a login per connection and keeps no registry
  of who is connected. That registry is what "the newest connection wins" will need at M4.

## Reading order for a fresh session

`docs/HANDOFF.md` (this file) → `docs/01-scope.md` → `docs/02-architecture.md` →
`docs/03-protocol.md` → `docs/roadmap.md` → the ADR that matches the question you have.
`docs/00-glossary.md` defines every term if something reads as jargon.
