# A.S.U.R.A. — Automated Server Utility for Remote Authentication

A private Guild Wars 1 server: not one program, but a small set of services that each do one job, keep
their shared state in one place, and can grow across machines without being rebuilt.

**Language: Rust.** **Database: PostgreSQL.** **Design authority: AzerothCore.** Each of those is a
recorded decision, not a default: `docs/adr/0007-rust-translation.md`,
`docs/adr/0008-database-vendor.md`, `docs/adr/0006-azerothcore-is-the-reference.md`.

## The shape of the server

There is no single program called "ASURA". The client talks to one front door (the **auth service**),
gets its portal identity from the **web gate**, is placed into one of many **world servers** that run
the maps, and everything durable — accounts, sessions, characters, items, the world registry — lives in
**PostgreSQL**. Each machine runs a **supervisor** that starts and watches its local services.

```
client ──portal login──▶ web gate  :6601  (who am I?)
client ──login────────▶ auth       :6112  (key exchange, session, character)
client ──gameplay────▶ auth       :6112 ──relays──▶ world :6113+  (one of many)
                                                       │
           registry · sessions · characters · items ───┴──▶ PostgreSQL (the shared truth)
```

The client dials the address it was launched with for **both** of its connections, so the auth service
is the only program at that address and it relays the gameplay connection to the world it chose for the
player. That is a measured client fact, not a preference (`docs/02-architecture.md`).

## The components

| component | what it is | what it does |
|---|---|---|
| **auth service** (`asura-auth`) | one program | the client's front door: key exchange, login, session, character list, character select; looks a world up for a travel or refuses it cleanly; relays the game connection to the world it picked |
| **world server** (`asura-world`) | many programs | runs maps for players: instances, agents, movement, items, chat. Creates its state at startup and is disposable — any world can be replaced by a newer build without touching the players inside it |
| **web gate** (`asura-webgate`) | one program | the portal login the client asks before it logs in: four `/Spawned/WebGate` endpoints, answered from the database |
| **supervisor** (`asura-supervisor`) | one program per machine | starts and watches this machine's services from `config/services.toml`: start, stop, status, restart, rolling deploy. A process manager, not an orchestrator |
| **database** (`asura-db`) | PostgreSQL | the shared truth: accounts, sessions, characters, bags and items, the `world`/`world_pool` placement tables, the `service` registry |
| **libraries** (`asura-protocol`, `asura-db`) | code, not processes | the client-facing message definitions, and the database access layer every service uses |

The services do not talk to each other over a wire protocol: they coordinate through the database
(`docs/03-protocol.md` §8). That is the design that makes them independently restartable.

## A login, end to end

1. The client asks the web gate who it is and takes its `user_id`/`session_id` from there.
2. It dials the auth service: key exchange, login, character list, character select.
3. A travel request reaches the auth service; it reads the registry, picks a world that is already
   running and answering, and hands the client its address and a one-use transfer token — or refuses,
   before the client moves.
4. The client reconnects to the same address; the auth service validates the token, dials the chosen
   world at its **registered** address, and relays the whole session.
5. The world runs the map. Everything durable is written to the database as it happens.

## Why it scales (and where it does not)

- **Worlds are disposable and many.** A world keeps no persistent state of its own — characters,
  their positions and items are in the database, and the map geometry is bundled derived data that
  every build reads — so replacing one is starting another process. It can be a second map on the same
  machine, a second machine, or a newer build during a deploy. Nothing has to remember which world
  serves what: the registry row does (`world`, `world_pool`).
- **Placement is a database decision, not a bookkeeping conversation.** A world that stops heartbeating
  stops being offered, and its abandoned leases are freed by the reaper. A travel is answered with
  something that is answering right now, or refused — **the one rule**.
- **Machines are added, not rebuilt around.** `config/services.toml` is what *this* machine runs; a
  second machine runs its own supervisor with its own list, and its worlds appear in the same registry.
- **Upgrades do not kick anyone.** A rolling deploy starts the new build beside the old one; new
  arrivals and reconnects land on the newest build, and the old world leaves when its last player has
  reconnected (`docs/02-architecture.md`, Deployment and draining).
- **The limits are stated, not hidden.** The auth service is the single front door and it relays
  gameplay, so its restart drops the sessions it is relaying; the database is a single shared
  dependency; and cross-host behaviour has been proven on one machine (`docs/18-multi-machine.md`), not
  yet across a real network's latency and firewalls.

## Why not one program (history)

The earlier implementation, GWPS, is one process that runs the front desk and every map inside it, one
thread and one private copy of the game-file index per map: **~12 MB per map**, and a crash takes
everything down. That shape cannot cross machines. ASURA keeps the same client-facing protocol and
turns the map side into disposable processes, with the database as the only shared truth. GWPS stays on
disk as history — it is not a build or run dependency (`docs/04-from-gwps.md`).

## The rules this project works under

- **A port is a port.** When a feature is ported from the reference, its code, message bytes, field
  order and sizes are copied verbatim: the user's command is the specification, not a draft to be
  re-decided (`docs/HANDOFF.md` § *Working directive*).
- **Nothing is invented on top of an instruction.** Six times a working feature has been broken by an
  unrequested addition; the ledger and the pre-flight rules are the first section of
  `docs/HANDOFF.md`.
- **The real client is the test and the suite is not a gate.** A scoped change is verified by its own
  targeted check and by a real client doing the thing (`docs/HANDOFF.md` § *Testing policy*).
- **The design authority is AzerothCore** (`docs/adr/0006-azerothcore-is-the-reference.md`), and
  where a deviation is needed it is written down (`docs/adr/`).

## Status

The whole path works against a real client: the patched client (build 39112) logs in through the web
gate and `asura-auth`, enters a world on `asura-world`, walks, travels between maps, and uses its
inventory — items, drops, moves, equipment — against the live server. The supervisor starts and stops
the stack, the registry tracks every service, a rolling deploy was proven on one machine, and the front
door was proven to dial a world registered at a second address rather than its bind address. The one
open verification is the same two things done with the client in hand: a player standing in a world
during a deploy, and a client travel to the second address
(`docs/16-implementation-plan.md`, `docs/18-multi-machine.md`).

## Read in this order

| document | what it settles |
|---|---|
| [`docs/HANDOFF.md`](docs/HANDOFF.md) | **start here** — the rules, environment status, what is decided, the next tasks |
| [`docs/19-running.md`](docs/19-running.md) | **how to run it** — what you need once, the one command, what success looks like, troubleshooting |
| [`docs/02-architecture.md`](docs/02-architecture.md) | the components in detail, the registry and pool tables, travel end to end, and the one rule |
| [`docs/03-protocol.md`](docs/03-protocol.md) | the wire facts: framing, key exchange, login, travel, the game connection |
| [`docs/01-scope.md`](docs/01-scope.md) | the boundary: what each service owns and what it explicitly does not |
| [`docs/roadmap.md`](docs/roadmap.md) | milestones, in the order they can be tested |
| [`docs/16-implementation-plan.md`](docs/16-implementation-plan.md) | the delivered plan: supervisor, registry, rolling deploy, multi-machine proof, run document, CI |
| [`docs/adr/`](docs/adr/) | every decision that is not obvious from the code |
