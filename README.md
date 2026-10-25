# A.S.U.R.A. — Automated Server Utility for Remote Authentication

The project, not a program: everything that makes a private Guild Wars server run.

ASURA is made of a small number of services, each with one job. Naming them explicitly is the point
of this section, because "ASURA does X" is ambiguous — the project does many things, and no single
process does them all.

| component | what it is | what it does |
|---|---|---|
| **auth service** (`asura-auth`) | one program | answers the client's login: key exchange, session, character list, character select. Owns identity. |
| **world server** (`asura-world`) | many programs | runs maps for players: simulation, instances, gameplay connections. Disposable. |
| **database** (`asura-db`) | PostgreSQL | the shared truth: accounts, sessions, characters, the world registry, pool capacity |
| **protocols** (`asura-protocol`) | a library | the client-facing message definitions and the server-to-server control messages |

There is no single program called "ASURA". When a document means the login side it says **auth
service**; when it means a map-running process it says **world server**.

- **Status:** design phase. No code yet; this repository currently holds the specification.
- **Sibling project:** [`PreSearing`/GWPS](https://github.com/apoguita/GWPS) is the existing
  single-process implementation, and remains the *reference* for every protocol fact in this repo.
  Nothing here is proven until it behaves against a real client the way GWPS does.

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

The auth service takes the control half out and does it properly. The worlds stay in C and keep simulating.

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

| document | what it settles |
|---|---|
| [`docs/01-scope.md`](docs/01-scope.md) | the boundary: what is the auth service's job and what is explicitly not |
| [`docs/02-architecture.md`](docs/02-architecture.md) | the process model, the allocation contract, and the one rule that must not be broken |
| [`docs/03-protocol.md`](docs/03-protocol.md) | the wire facts: framing, key exchange, login, travel, and the world-link protocol |
| [`docs/04-from-gwps.md`](docs/04-from-gwps.md) | what is inherited from GWPS, what must be re-proven, and where the reference code is |
| [`docs/roadmap.md`](docs/roadmap.md) | milestones, in the order they can be tested |
| [`docs/adr/`](docs/adr/) | why Rust, why a separate project, and later decisions |

## The one rule

**No server is constructed while a client is travelling.** A travel request is either answered
with something that is already running and ready, or refused — before the client is told to go
anywhere. Every design decision in this repository is downstream of that, and
`docs/02-architecture.md` explains what it cost to learn.

## Language

Rust for the control plane (`docs/adr/0001-rust.md`). The world servers remain the existing C
implementation for now; whether the simulation is eventually ported to Rust or adopted as a C
component of this project is a decision deliberately left to the simulation phase
(`docs/adr/0003-simulation-in-scope.md`).

## Working with the sibling repository

GWPS is not deprecated by this project — it is the oracle. When the auth service disagrees with the client,
the question "what does GWPS send here, byte for byte?" is answered in
`PreSearing/code/OpenTyria/`, and the captures and diffing tools live in
`PreSearing/tools/` and `PreSearing/captures/`.
