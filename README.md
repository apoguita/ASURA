# A.S.U.R.A. — Automated Server Utility for Remote Authentication

The project, not a program: everything that makes a private Guild Wars server run.

**Language: Rust.** **Database: PostgreSQL.** **Design authority: AzerothCore.** Each of those is a
recorded decision, not a default: `docs/adr/0007-rust-translation.md`,
`docs/adr/0008-database-vendor.md`, `docs/adr/0006-azerothcore-is-the-reference.md`.

ASURA is made of a small number of services, each with one job. Naming them explicitly is the point
of this section, because "ASURA does X" is ambiguous — the project does many things, and no single
process does them all.

| component | what it is | what it does |
|---|---|---|
| **auth service** (`asura-auth`) | one program | answers the client's login: key exchange, session, character list, character select. Owns identity. |
| **world server** (`asura-world`) | many programs | runs maps for players: simulation, instances, gameplay connections. Disposable. |
| **web gate** (`asura-webgate`) | one program | the portal login the client asks who it is before it logs in: four `/Spawned/WebGate` endpoints (ported from the reference's `tools/webgate.py`), answered from the database |
| **database** (`asura-db`) | PostgreSQL | the shared truth: accounts, sessions, characters, the world registry, pool capacity |
| **protocols** (`asura-protocol`) | a library | the client-facing message definitions and the server-to-server control messages |

There is no single program called "ASURA". When a document means the login side it says **auth
service**; when it means a map-running process it says **world server**.

- **Status:** the whole path works against a real client: the patched client (build 39112) logs in
  through the web gate and `asura-auth`, enters a world on `asura-world`, and `/tp` moves it between
  maps — a district-style transfer (the reference's own `0x01A6` struct), then a reconnect into the
  destination spawn. Everything a run needs is in this repository and its scripts: PostgreSQL, the
  web gate, the services, and the seeded identities. `docs/HANDOFF.md` states the current state and
  the parity queue.
- **Design authority:** where a structural question arises, the default answer is *what AzerothCore
  does* (`docs/adr/0006-azerothcore-is-the-reference.md`). GWPS is research material, not a target; the
  client rig is Reforged plus the stealth hooks.
- **Sibling project:** [`PreSearing`/GWPS](https://github.com/apoguita/GWPS) is the existing
  single-process implementation and the historical research material behind this one. Parity was closed
  (docs/16) and the parity tooling was retired; nothing here is proven until a real client does the thing.
- **A port is a port:** when the user asks for a feature to be ported from the reference, the
  reference is copied verbatim — its code, its message bytes, its field order, its sizes. The
  user's command is the specification; it is not re-decided here. This is binding:
  `docs/HANDOFF.md` § *Working directive*, and it exists because the teleport port was done wrong
  first (`e0f671c`).
- **Nothing is invented on top of an instruction.** Six times a working feature has been broken by an
  addition nobody asked for (the teleport struct, the retail movement frames, an exact-adoption clamp
  replacement, a waypoint filter, plane flattening, and a "rule" derived from an explanation of what a
  plane is). Each feature has cost the owner three to four times the work in test sessions. The ledger,
  the named failure mode and the pre-flight rules that follow are the first section of
  `docs/HANDOFF.md` — read them before editing.
- **The real client is the test, and the suite is not a gate:** a scoped change is verified by its
  own targeted check and by a real client doing the thing — never by running every suite, and never
  by growing new scaffolding around a small change. `docs/HANDOFF.md` § *Testing policy*. This is an
  owner's standing instruction (2026-10-28), not a preference.

## Why this exists

GWPS works: the client is patched, logs in, connects, and travels between maps. It was built as
one program that runs the front desk and every map in the same process, one thread and one private
copy of the game-file index per map. Measured cost: **~12 MB per map instance**, one thread, one
SQLite connection. That is fine for a proof of concept and fine for dozens of maps. It is not a
shape that grows to hundreds of instances or multiple machines, because:

- every instance repeats a read-only 4.7 MB index and a 2 MB database cache;
- every instance owns an operating-system thread;
- a map server is constructed *inside* a teleport, which is the defect behind the loading-screen
  hang (see `docs/02-architecture.md`);
- the process that answers the client is also the process that runs the worlds, so a crash takes
  everything with it and the whole deployment lives on one machine.

The auth service takes the control half out and does it properly. The world server is this repository's own Rust server, `crates/asura-world` — a real client has logged in, travelled, and stood in a map against it since M4 (`docs/HANDOFF.md`).

## What the auth service owns

| responsibility | detail |
|---|---|
| Auth endpoint | the client's login connection: key exchange, login, character list, character select |
| Session/identity | who is logged in, which character, what key a reconnect must present |
| Directory | which world server is where, what it hosts, how full it is |
| Allocation | pick a world for a travel request, or refuse it cleanly before the client moves |
| Persistence | accounts, sessions, characters, bags/items — the databases the worlds read |
| Web gate | the portal login that hands the client its `user_id`/`session_id` |

## What the auth service does not own *yet*

Map simulation, agents, pathfinding, combat and chat delivery are in this project's long-term scope
- this repository is meant to hold the whole server stack, not only its control half - but
simulation is explicitly deferred until the control plane works. See
`docs/adr/0003-simulation-in-scope.md`.

## Read in this order

Starting a session? Read [`docs/HANDOFF.md`](docs/HANDOFF.md) first: it states what is decided,
what is missing from this machine, and what the next three tasks are.

| document | what it settles |
|---|---|
| [`docs/HANDOFF.md`](docs/HANDOFF.md) | **start here** — environment status, what is decided, the next three tasks |
| [`docs/19-running.md`](docs/19-running.md) | **how to run it** — one command to start the stack, what success looks like, the everyday commands, troubleshooting |
| [`docs/01-scope.md`](docs/01-scope.md) | the boundary: what is the auth service's job and what is explicitly not |
| [`docs/02-architecture.md`](docs/02-architecture.md) | the components, the registry and pool tables, travel end to end, and the one rule that must not be broken |
| [`docs/03-protocol.md`](docs/03-protocol.md) | the wire facts: framing, key exchange, login, travel, and the world-link protocol |
| [`docs/adr/0008-database-vendor.md`](docs/adr/0008-database-vendor.md) | **database vendor** — PostgreSQL, a justified deviation from AzerothCore's MySQL; structure unchanged |
| [`docs/adr/0006-azerothcore-is-the-reference.md`](docs/adr/0006-azerothcore-is-the-reference.md) | **the design authority** — follow AzerothCore by default, deviate only in writing |
| [`docs/05-gw2-lessons.md`](docs/05-gw2-lessons.md) | what the GW2 microservices talk changes here — the restartability rules, and what we deliberately do not copy |
| [`docs/04-from-gwps.md`](docs/04-from-gwps.md) | what is inherited from GWPS, what must be re-proven, and where the reference code is |
| [`docs/roadmap.md`](docs/roadmap.md) | milestones, in the order they can be tested |
| [`docs/adr/`](docs/adr/) | why Rust, why a separate project, and later decisions |

## The one rule

**No server is constructed while a client is travelling.** A travel request is either answered
with something that is already running and ready, or refused — before the client is told to go
anywhere. Every design decision in this repository is downstream of that, and
`docs/02-architecture.md` explains what it cost to learn.

## Language

Rust for the whole stack in this repository (`docs/adr/0001-rust.md`): the control plane and the
world server. The world server is `crates/asura-world`, delivered with the real client in M4
(`docs/HANDOFF.md`); the earlier plan for a C world server (`docs/adr/0003-simulation-in-scope.md`)
did not hold — the Rust world was delivered first. GWPS is the historical source of every protocol and
simulation fact this port reproduces.

## The sibling repository

GWPS was the research material and the runnable oracle, and it stays on disk as history: it is not a
build or run dependency of anything here. Parity was closed (docs/16), the parity tooling — the oracle,
the `parity-*` and `gen-*` scripts, the reference fixtures — was retired, and the faithful recordings
that remain are the public archive under `recordings/`.
