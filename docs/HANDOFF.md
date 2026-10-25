# Start here (session handoff)

State of the project as of the last commit, so a new session can begin without re-reading everything.

## Where this stands

- **No implementation yet.** The repository is a specification plus a scaffold that compiles. That is
  deliberate: the architecture was argued and recorded before any code existed.
- **Ten documents and seven ADRs** describe what is built, what is refused, and why.
- **GWPS is research material** (`docs/adr/0005-gwps-reference-only.md`). ASURA is written from
  scratch; nothing is ported or patched there.
- **AzerothCore is the design authority** (`docs/adr/0006-azerothcore-is-the-reference.md`). When a
  structural question comes up, the default answer is what their tree does, and a deviation has to be
  written down before it is implemented.

## Decisions already made (do not re-litigate without reading these)

| decision | where |
|---|---|
| Rust for the whole stack, translated from AzerothCore's structure | `docs/adr/0001-rust.md`, `docs/adr/0007-rust-translation.md` |
| Separate project from GWPS | `docs/adr/0002-separate-project.md` |
| Simulation is in scope, deferred behind the base structure | `docs/adr/0003-simulation-in-scope.md` |
| AzerothCore's server structure, ported as a whole | `docs/adr/0004-port-azerothcore-structure.md` |
| GWPS is reference only; instance creation is settled by experiment | `docs/adr/0005-gwps-reference-only.md` |
| AzerothCore is the design authority | `docs/adr/0006-azerothcore-is-the-reference.md` |
| The one rule: never send a client to something that is not already answering | `docs/02-architecture.md` |

## Prerequisites not yet present on this machine

| what | why | how |
|---|---|---|
| **Rust (rustup)** | nothing builds without it | `winget install Rustlang.Rustup`, or rustup-init from rustup.rs |
| **MySQL 8.0.16+** | the database is the coordination channel; same vendor as AzerothCore | `winget install Oracle.MySQL` |
| The Reforged client + stealth hooks | validation means the real client, instrumented | already present from the PreSearing work |

## First session tasks, in order

1. `cargo test` — the scaffold has never been compiled on this machine. Fix whatever it says.
   Expected: the workspace builds with no dependencies, and tests pass in `asura-protocol`,
   `asura-db`, `asura-auth`, `asura-world`.
2. **M2**: schema applied to a real MySQL, `world` registration, `world_pool` capacity, and the
   reservation statement under concurrency — N parallel reservations against capacity N-1 must
   succeed exactly N-1 times. This is the milestone that makes placement safe, and it needs no client.
3. **M3**: the auth endpoint. Handshake, ARC4, DH with the fixed constants, login, character list.
   This is the hard one; GWPS is the oracle for every byte (`docs/03-protocol.md`).

## What is deliberately not here yet

- No networking, no `tokio`, no `sqlx`. Dependencies get added when the code that needs them exists.
- No CI. Tests must run locally first.
- No simulation. It arrives in phase 2 (`docs/roadmap.md`).

## Reading order for a fresh session

`docs/00-glossary.md` → `docs/01-scope.md` → `docs/02-architecture.md` → `docs/03-protocol.md` →
`docs/roadmap.md` → the ADR that matches the question you have.
