# Start here (session handoff)

State of the project as of the last commit, so a new session can begin without re-reading everything.

## Where this stands

- **No implementation yet.** The repository is a specification plus a scaffold that **compiles and
  passes its tests**. That order was deliberate: the architecture was argued and recorded before any
  feature code existed.
- **Fifteen documents and eight ADRs** describe what is built, what is refused, and why.
- **GWPS is research material** (`docs/adr/0005-gwps-reference-only.md`). ASURA is written from
  scratch; nothing is ported or patched there.
- **AzerothCore is the design authority** for structure (`docs/adr/0006-azerothcore-is-the-reference.md`).
  A deviation has to be recorded before it is implemented — see ADR 0008 for how that is done well and
  how it is done badly.

## Environment status on this machine

| what | state | evidence |
|---|---|---|
| **Rust** | installed, 1.99.1 | `cargo test` → **11 tests, 0 failures**; `cargo clippy --all-targets` → clean |
| cargo on PATH | **not** on PATH in a fresh shell | use `C:\Users\Apo\.cargo\bin\cargo.exe`, or add that directory to PATH permanently |
| **PostgreSQL** | installed, 16.15, service `postgresql-x64-16` **Running** | accepts connections on 5432; **credentials not yet configured** |
| psql | `C:\Program Files\PostgreSQL\16\bin\psql.exe` | not on PATH |
| client rig (Reforged + stealth hooks) | present from the PreSearing work | not needed until M4 |

**The one thing blocking M2** is a database role and schema. Either run the two commands below, or
export `PGPASSWORD` and say so, and the setup plus the M2 acceptance test can be done in one pass.

```sql
CREATE ROLE asura LOGIN PASSWORD '<choose one>';
CREATE DATABASE asura OWNER asura;
```

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

## First session tasks, in order

1. **Verify the baseline** (2 minutes):
   `cargo test --offline` → 11 passing. If the count changed, something was added since this handoff;
   read `git log` before assuming it is broken.
2. **M2 — make placement safe, no client needed.** This is the milestone that justifies the whole
   architecture, so it comes before anything client-facing:
   - create the role and database, apply `crates/asura-db/migrations/0001_init.sql`;
   - add a `world` row and `world_pool` capacity rows for a fake world;
   - implement the reservation: `UPDATE world_pool SET leased = leased + 1 WHERE world_id = $1 AND
     map_id = $2 AND leased < capacity RETURNING world_id`;
   - **acceptance test:** with capacity N-1 and N reservations issued concurrently from separate
     tasks, exactly N-1 succeed, one is refused, `leased` ends at capacity, and the `CHECK` constraint
     never fires. Run it repeatedly (it must not be flaky).
   - also test the reaper path: a lease whose world stops heartbeating is released.
3. **M3 — the auth endpoint.** Handshake, ARC4, DH with the fixed constants, login, character list.
   The hard one, and the first that needs the real client. GWPS is the oracle for every byte
   (`docs/03-protocol.md`), and `asura-protocol` already carries the message identities with the
   `0x0099` size trap as a test.

## Definition of done for the first session

A `world` registered in the database, capacity published, reservations proven atomic under
concurrency, and the reaper proven to free a dead world's leases. No client, no protocol, no
simulation — and placement is then trustworthy for everything built on top of it.

## What is deliberately not here yet

- No networking, no `tokio`, no `sqlx`. Dependencies get added when the code that needs them exists,
  and the scaffold is dependency-free so the toolchain stays provable offline.
- No CI. Tests run locally first.
- No simulation. It arrives in phase 2 (`docs/roadmap.md`).

## Reading order for a fresh session

`docs/HANDOFF.md` (this file) → `docs/01-scope.md` → `docs/02-architecture.md` →
`docs/03-protocol.md` → `docs/roadmap.md` → the ADR that matches the question you have.
`docs/00-glossary.md` defines every term if something reads as jargon.
