# Architecture

How the pieces fit, what crosses between them, and the rules the design is built around.

## Three roles

| role | how many | lifetime | state |
|---|---|---|---|
| **auth service** (control) | 1 (+ standby) | long | the only meaningful state: sessions, directory, allocation |
| **world server** (data) | many | disposable | map instances in memory; nothing that survives a restart |
| **content/file server** | 1+ | long | assets; outside this project |

World servers are the only place a player's map runs. The auth service never simulates anything, and it never
holds a player's gameplay connection. That is the whole point of the split: the part that is hard
to scale (instances) becomes a set of interchangeable processes, and the part that is hard to
replicate (identity and placement) stays in one small place.

## Phases

| phase | what is being built | where simulation lives |
|---|---|---|
| 1 | control plane: auth, sessions, directory, allocation, persistence, world link | the existing C world server, unchanged |
| 2 | simulation: the world server becomes part of this project | decided then - port it to Rust, or adopt the C implementation |

Phase 1 exists so that phase 2 can be judged on its own merits instead of being tangled up with
identity, allocation and the process model. Nothing in phase 1 may assume the simulation stays in C
forever: the world link is a *protocol*, not a linked library, and the interface is identical
whether the far side is Rust or C (`docs/adr/0003-simulation-in-scope.md`).

## What a world server is

A process that:

1. connects to the auth service and announces itself: an id, the address the client should be told to use,
   which maps it has loaded, and how much room it has;
2. keeps a set of ready map instances;
3. receives allocation requests and either assigns an existing ready instance or refuses;
4. accepts the player's gameplay connection itself, and verifies the token the auth service gave it;
5. reports load (instances, players, tick health) on a heartbeat;
6. exits cleanly when asked, having saved what needs saving.

Everything a world server knows can be rebuilt: the database is the truth, the auth service is the directory.
Killing a world server is an expected event, not an incident.

## The allocation contract

One connection per world server to the auth service. Every message is a small fixed-layout struct, because
both ends are implemented separately and a schema mismatch must be an immediate, loud failure
rather than a silent misparse.

| direction | message | meaning |
|---|---|---|
| world → the auth service | `WorldHello` | id, client-facing address, capability, capacity |
| world → the auth service | `MapReady` | "map X is loaded and I have room for N more instances of it" |
| world → the auth service | `Heartbeat` | instances held, players, tick latency, memory |
| The auth service → world | `AllocateInstance` | "give me an instance of district D for player P" |
| world → the auth service | `InstanceGranted` | "instance I belongs to player P, token T" |
| world → the auth service | `InstanceRefused` | "no room / no such map" — a normal answer, not an error |
| world → the auth service | `PlayerLeft` | player has left instance I (frees capacity) |
| world → the auth service | `InstanceReleased` | instance I is empty, cleared and back in the pool |
| The auth service → world | `Shutdown` | save and exit |

The auth service keeps the player-facing half of the same flow: it answers the client's travel request with
`World` + address + token (the `0x01A6` redirect, `docs/03-protocol.md`).

## The one rule: nothing is built during a travel

**A travel request is answered only with an instance that is already running, loaded and ready.**
If none exists, the request is refused *before the client is told anything*, while it is still
standing in its current map.

This is not a style preference. GWPS learned it the hard way:

- In the single-process build, a travel to a map that had no server yet constructed that server
  *inside* the hop. 7 of 53 hops (13 %) then hung on the loading screen, and no other signal
  separated the failures: not the destination, not the time between hops (failures 1735–8778 ms,
  successes 1775 ms and up), not whether the destination had been visited (3/27 vs 3/23).
- The client recorder showed what a hang *is*: on a good hop the client tears its world down
  (`map_context = 0`, `instance_type = 2`, `terrain_count = 0`) and rebuilds it 100–204 ms later.
  On the hung hop it tears down and **never** rebuilds, and the network goes silent: zero packets
  for the rest of the session.
- After every destination was created at startup instead, 12 hops in one session, zero hangs,
  including six first visits and hops at 2.8 s spacing (inside the old failure band).

So the rule is: **prebuild, pool, assign, refuse — never construct.** The extra 25 ms of building a
server is irrelevant; what matters is never sending a client to an address that is not answering.

### What this means concretely

- **Outposts and towns**: created at world-server startup, one per map per district, kept forever.
  A bounded set.
- **Explorables**: a prebuilt pool per map, depth = how many parties should be able to be inside
  that map at once. Assigned on entry, cleared and returned on exit. Never freed, never built on
  request.
- **At capacity**: `InstanceRefused` → the auth service answers the travel request with a refusal. The client
  stays where it is. No queueing, no constructing, no "wait a moment".

## Placement

The auth service chooses the world server for a request with these rules, in order:

1. **Capability first.** The world must have the destination map loaded, or be able to serve it
   from a ready pool instance. A world that would have to load the map is not a candidate.
2. **Same world preferred.** If the player's current world can serve the destination, use it. It
   avoids a cross-world hop for the client and keeps a party together.
3. **Least loaded among equals.** Fewest players per instance, then fewest instances, then lowest
   tick latency from the heartbeat.
4. **Refuse when nothing fits.** And say why, in the logs, with enough detail to know whether the
   fix is capacity or configuration.

Map affinity is a real constraint, not an optimisation: a world server can only offer what it has
loaded, and loading is not a per-request operation.

## Failure and recovery

| event | outcome |
|---|---|
| world server crashes | its players are dropped; the auth service marks them disconnected; their map state is whatever the database last saved; other worlds are unaffected; the world is restarted and re-registers |
| The auth service restarts | sessions are lost (players must log in again); the directory rebuilds from world hellos; allocation resumes |
| no room anywhere | travel is refused before the client moves |
| duplicate world id | rejected at `WorldHello`; the older connection is closed |

The asymmetry is deliberate: losing the auth service costs a login, losing a world costs a map. Neither can
corrupt the other, and neither can admit a player the other did not authenticate.

## Cost model

Measured from GWPS on the same class of hardware, per map instance:

| item | cost | after the fixes below |
|---|---|---|
| private memory | 11.9 MB | ~5 MB |
| OS thread | 1 | shared reactor, ~0 |
| SQLite connection | 1 (2 MB cache) | 1 per world, not per instance |
| `Gw.dat` index | 4.7 MB private copy | 1 per world, memory-mapped |

The two big items are duplication, not work: the same read-only index read 500 times, and a
database cache per map that all serve the same file. Sharing them per world server is the single
largest lever, and it is local to the world server — the auth service is unaffected by it.

Sizing rule of thumb: a world server with 500 instances and a shared index is roughly
500 × 5 MB ≈ 2.5 GB. Ten of them hold ~5000 instances. Growth is "add a world server", including
on another machine, because the client is told a real address, not a loopback one.

## Process and startup order

1. Database available; schema current.
2. The auth service starts, binds the auth endpoint, opens the world link listener.
3. World servers start, load their maps and pools, announce themselves, report `MapReady`.
4. The auth service accepts players; allocation answers only from worlds that have reported ready.
5. Shutdown: the auth service stops accepting, asks worlds to save and exit, then exits.

A world server that is still loading is simply not a candidate — which means a request arriving two
seconds after boot gets a correct "no room" instead of a race.

## Deliberate non-goals

- Autoscaling, orchestration, containers. Adding a world server is a config line and a process.
- Distributed consensus. There is one authority (the auth service) and everything else defers to it.
- Multi-region. One region; districts are capacity, not geography.
