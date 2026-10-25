# Scope

Which component owns what, in an ASURA stack built on AzerothCore's structure
(`docs/adr/0004-port-azerothcore-structure.md`).

## The components

| component | responsiblity |
|---|---|
| **auth service** | answers the client's login and tells it which world to join |
| **world server** | everything a player experiences: maps, instances, placement, character state |
| **database** | the shared truth and the coordination channel between the other two |

These are the only three kinds of component that matter. The auth service is small and long-lived;
world servers are interchangeable and disposable; the database is what they both agree on. There is
no live control connection between the auth service and world servers for state or placement — the
database is the handoff.

## The auth service owns

### The login endpoint

The client's first connection. Key exchange, login against the web gate, character list, character
selection. The client uses this connection only for authentication; the gameplay connection is a
different one, to a world server.

### Sessions and login tokens

For every logged-in player: account id, session id, selected character, and the token the player's
*next* connection must present. The client drops its connection and reconnects on every travel, and
that reconnect is only accepted when it matches what was issued. Getting this wrong is a visible
client error, not a crash.

### The web gate

The portal the client talks to before it talks to us, which hands it a `user_id` and a `session_id`.
The auth service authenticates those.

### Reading the registry — not owning it

The auth service reads the `world` table to answer "where should this client connect?" It does not
decide placement, and it does not keep the directory's truth in memory: the table is the truth, and
a restart loses nothing.

## The world servers own

### Placement and capacity

A travel is decided by the world server the player is on, not by a central allocator. It picks a
destination world with the map loaded and room available, and **reserves** a slot atomically in
`world_pool` (`docs/02-architecture.md`, "Travel, end to end"). No slot means the travel is refused
while the client is still standing in its current map.

### Character state

Characters, bags, items and instance records in the `game` database — read when entering an instance,
written when leaving and on a timer, never per frame. The world server is the owner; the auth service
never writes character data.

### Instances

Outposts prebuilt per map and district; explorables prebuilt into pools per map. Created ahead of
need, cleared and recycled; never constructed because a player asked to travel.

### The gameplay connection

The client connects to the world server directly and the world server authenticates that connection
against the token the auth service issued. The auth service is not in the data path.

## The database owns

The schema, and therefore the contract between the two programs. Three logical databases: `auth`,
`game`, `content`. One owner of the schema definition (`asura-db`), migrations in version control,
and nothing in the project writes SQL outside that crate.

## Rules that must not be quietly broken

1. **Identity validation stays out of the world servers' *discretion*.** A world may verify a token
   the auth service issued; it may not admit a player who never logged in, and it may not mint
   tokens.
2. **Capacity must be able to say no.** A refusal happens before the client is told to move. Any
   design where "no room" is discovered after the client has been redirected is a re-introduction of
   the original hang.
3. **The database is the only coordination point.** No side channel that carries state between the
   auth service and a world server; the moment one exists, restarts become ambiguous.

## Phase 1 boundary: what does not exist *yet*

Simulation — agents, movement, pathfinding, combat, skills, drops — is in this project's long-term
scope, deferred until the base structure works (`docs/adr/0003-simulation-in-scope.md`). In phase 1 it
lives in the existing C world server.

- **Simulation.** A world server. Not written yet in this project.
- **Map data.** No component of ASURA stores map blobs; `Gw.dat` reading is a world-server concern.
- **Content delivery.** The client also talks to a file server for game assets: a separate service,
  not scheduled.

## Explicitly out of scope for now

- **Simulation's own design.** Deferred, not cancelled; no phase-1 decision may preclude it.
- Multiple regions or languages. One region, one language; districts are a capacity concept only.
- Cross-world chat. Chat is delivered by the world the player is on.
- Game balance and content.
