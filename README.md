# A.S.U.R.A. — Automated Server Utility for Remote Authentication

A private Guild Wars 1 server. **Language: Rust. Database: PostgreSQL.**

## The shape of the server

The client talks to the **auth service** (login, session, character list, and the front door of the
game connection), gets its portal identity from the **web gate**, and is placed into one of many
**instance servers** that host the maps. Everything durable — accounts, sessions, characters, items,
the instance registry — lives in **PostgreSQL**. Each machine runs a **supervisor** that starts and
watches its local services.

```
client ──portal login──▶ web gate  :6601  (who am I?)
client ──login────────▶ auth       :6112  (key exchange, session, character)
client ──gameplay────▶ auth       :6112 ──relays──▶ instance server :6113+  (one of many)
                                                          │
           registry · sessions · characters · items ───────┴──▶ PostgreSQL (the shared truth)
```

Every player is always in an instance. An **outpost** is a shared hub instance; an **explorable** is a
per-party instance; travel is an instance change, which the client performs by reconnecting. The client
dials the address it was launched with for both of its connections, so the auth service is the only
program at that address and it relays the gameplay connection to the instance server it picked.

## The components

| component | what it is | what it does |
|---|---|---|
| **auth service** (`asura-auth`) | one program | the client's front door: key exchange, login, session, character list, character select; looks up an instance server for a travel or refuses it cleanly; relays the game connection to the server it picked |
| **instance servers** (`asura-world`) | many programs | host maps for players: outposts, explorables, agents, movement, items, chat. Each creates its state at startup and is disposable — a server can be replaced by a newer build without touching the players inside it |
| **web gate** (`asura-webgate`) | one program | the portal login the client asks before it logs in: four `/Spawned/WebGate` endpoints, answered from the database |
| **supervisor** (`asura-supervisor`) | one program per machine | starts and watches this machine's services from `config/services.toml`: start, stop, status, restart, rolling deploy |
| **database** (`asura-db`) | PostgreSQL | the shared truth: accounts, sessions, characters, bags and items, the server and pool placement tables, the `service` registry |
| **libraries** (`asura-protocol`, `asura-db`) | Rust code the services share | the client-facing message definitions, and the database access layer every service uses |

Every service reads and writes the database; that is how the services coordinate.

## How it scales

- **Instance servers are disposable and many.** A server keeps no persistent state of its own —
  characters, their positions and items are in the database, and the map geometry is bundled derived
  data that every build reads — so replacing one is starting another process. It can be another
  outpost or explorable on the same machine, a second machine, or a newer build during a deploy.
  Nothing has to remember which server hosts what: the registry rows do.
- **Placement is read from the database.** A server that stops heartbeating stops being offered, and
  its abandoned leases are freed by the reaper. A travel is answered with something that is answering,
  or refused.
- **Adding a machine** means starting a supervisor on it with its own list in `config/services.toml`;
  its servers appear in the same registry as every other machine's.
- **Upgrades do not kick anyone.** A rolling deploy starts the new build beside the old one; new
  arrivals and reconnects land on the newest build, and the old server leaves when its last player has
  reconnected.
- **The limits:** the auth service is the single front door and it relays gameplay, so its restart
  drops the sessions it is relaying; the database is a single shared dependency; and cross-host
  behaviour has been proven on one machine, not yet across a real network's latency and firewalls.

## Status

The whole path works against a real client: the patched client (build 39112) logs in through the web
gate and `asura-auth`, enters an outpost on an instance server, walks, travels between maps, and uses
its inventory — items, drops, moves, equipment — against the live server. The supervisor starts and
stops the stack, the registry tracks every service, a rolling deploy was proven on one machine, and the
front door was proven to dial a server registered at a second address rather than its bind address. The
open verification is the same two things done with the client in hand: a player standing in an instance
during a deploy, and a client travel to the second address.

## Read in this order

| document | what it settles |
|---|---|
| [`docs/HANDOFF.md`](docs/HANDOFF.md) | environment status and the next tasks |
| [`docs/19-running.md`](docs/19-running.md) | **how to run it** — what you need once, the one command, what success looks like, troubleshooting |
| [`docs/02-architecture.md`](docs/02-architecture.md) | the components in detail, the registry and pool tables, travel end to end |
| [`docs/03-protocol.md`](docs/03-protocol.md) | the wire facts: framing, key exchange, login, travel, the game connection |
| [`docs/01-scope.md`](docs/01-scope.md) | the boundary: what each service owns and what it explicitly does not |
| [`docs/roadmap.md`](docs/roadmap.md) | milestones, in the order they can be tested |
