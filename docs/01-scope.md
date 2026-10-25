# Scope

What ASURA is responsible for, and what it deliberately hands to someone else.

## The split

Two kinds of program make up a running server:

- **ASURA** — one instance, or a primary and a standby. It answers the client, owns identity and
  the directory, and allocates.
- **World servers** — many. Each hosts some map instances and simulates them. They are disposable:
  a world server holds no information that cannot be recreated from the database and the
  directory.

The boundary is a small one: a handful of fixed-layout messages over one TCP connection per world
(`docs/03-protocol.md`, "world link"). Nothing else crosses.

## ASURA owns

### The auth endpoint

The client's first connection. ASURA terminates it: key exchange, login against the web gate,
character list, character selection. This is the connection the client only uses for
authentication and re-negotiation; the gameplay connection is a different one.

### Identity and sessions

For every logged-in player ASURA knows: account id, session id, selected character, and the token
that the player's *next* connection must present. The last one matters: the game client drops its
connection and reconnects on every travel, and that reconnect is accepted only if it matches what
ASURA recorded. Getting this wrong is visible as a client error, not a crash.

### The directory

Which world servers exist, where they are reachable, which maps each one has loaded and how much
room each one has. The directory is the only place that knows the whole picture, and it is small
enough to keep in memory and rebuild from world heartbeats after a restart.

### Allocation

Every travel request is an allocation question: *this player, this destination district — where do
they go?* ASURA answers with a world, an address and a token, or refuses. The rules are in
`docs/02-architecture.md`.

### Persistence

The accounts, sessions, characters, bags and items. ASURA owns the schema and the writes; worlds
read what they need for the maps they run. Player state is written on the way out of an instance,
not held as a world's private truth.

### The web gate

The portal the client talks to before it talks to us, which hands it a `user_id` and `session_id`.
ASURA authenticates those.

## Phase 1 boundary: what ASURA does not do *yet*

Simulation - agents, movement, pathfinding, combat, skills, drops - is part of this project's
long-term scope, deferred until the control plane is correct
(`docs/adr/0003-simulation-in-scope.md`). In phase 1 it lives where it already does, in a world
server:

- **Simulation.** A world server. Not ASURA, not yet.
- **The gameplay connection**, once accepted. If the world owns its own listener (the target
  design), the client's gameplay socket is between the client and that world; ASURA is not in the
  data path.
- **Map data.** ASURA stores no map blobs and reads no `Gw.dat`. Loading and caching map data is a
  world concern.
- **Content delivery.** The client also talks to a file server for game assets; that is a separate
  service, not part of this project.

## Two things that must not be quietly moved

1. **Identity validation must stay in ASURA.** A world server may be trusted to *present* an
   identity, but not to decide that an unauthenticated connection is allowed in. A compromised or
   buggy world must not be able to admit a player who never logged in.
2. **Capacity must be able to say no.** If there is no room for a travel request, ASURA refuses it
   while the client is still standing in its current map. Any design where "no room" is discovered
   after the client has been told to move is a re-introduction of the original hang.

## Explicitly out of scope for now

- **Simulation.** In scope for the project, out of scope until the control plane works. "Deferred"
  is doing real work in that sentence: it is not cancelled, and no phase-1 decision may preclude it.
- Multiple regions or languages. One region, one language, and districts only as a capacity
  concept.
- Server-to-server chat between worlds: chat is delivered through the world server the player is
  on, and cross-world chat is a later problem.
- Anything about game balance or content.
