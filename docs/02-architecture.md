# Architecture

The structure is AzerothCore's, ported as a whole (`docs/adr/0004-port-azerothcore-structure.md`),
with one deviation forced by our client. This document describes what that means concretely.

## Components

| component | how many | what it does | state |
|---|---|---|---|
| **auth service** (`asura-auth`) | 1 (+ standby) | login, key exchange, sessions, character list/select; tells the client which world to join | sessions in memory, accounts in `auth` |
| **world server** (`asura-world`) | many | maps, players, instances, placement, character persistence | instances in memory; nothing that survives a restart |
| **database** (`asura-db`) | 1 (+ standby) | the shared truth and the coordination channel | everything |
| **content service** | 1+ | game assets (`Gw.dat` streaming) | outside ASURA |

The auth service never runs a map and is not in the gameplay data path. World servers are
interchangeable and disposable. The database is the only thing both kinds of program agree on.

## The five structural elements

Ported from AzerothCore, deliberately not cherry-picked:

1. **Two program types**, with no live state channel between them for placement or character state.
2. **Three databases**, each with its own connection pool:
   - `auth` — accounts, sessions, login tokens
   - `game` — characters, items, bags, instance records
   - `content` — map configuration and spawns, effectively read-only
3. **A registry table** that world servers write themselves into, exactly as `realmlist` works.
4. **Character state owned and written by world servers**, through a pool with an asynchronous
   write queue.
5. **Instance management and pooled map-update threads inside the world server.**

## The registry

```sql
CREATE TABLE world (
    id            int          PRIMARY KEY,     -- world server id, configured per process
    name          text         NOT NULL,
    address       text         NOT NULL,        -- what the client is told to connect to
    port          int          NOT NULL,
    flags         int          NOT NULL,        -- online / draining / maintenance
    population    int          NOT NULL,        -- players currently connected
    heartbeat_at  timestamptz  NOT NULL         -- last time this world said it was alive
);

CREATE TABLE world_pool (                       -- capacity, per world, per map
    world_id   int  REFERENCES world (id),
    map_id     int  NOT NULL,
    capacity   int  NOT NULL,                   -- instances prebuilt and ready
    leased     int  NOT NULL DEFAULT 0,         -- currently assigned to a party
    PRIMARY KEY (world_id, map_id)
);
```

A world server inserts or updates its `world` row at startup (flags = online), keeps `population` and
`heartbeat_at` current, and marks itself draining or offline on exit. The auth service reads `world`
to answer "where is this player's map?" — the registry is data, so it survives restarts, and no
handshake is needed to keep it honest.

`world_pool` is what makes allocation safe across processes: it is the prebuilt capacity, and only
one process can win a given row.

## Travel, end to end

1. The player asks to travel, either from inside a map (on their current world server) or at login
   (on the auth connection).
2. The deciding process looks for a destination world that has the map and a free slot, then
   **reserves** it atomically:

   ```sql
   UPDATE world_pool SET leased = leased + 1
    WHERE world_id = $1 AND map_id = $2 AND leased < capacity
    RETURNING world_id;
   ```

   Choosing the world is a `SELECT` (same world preferred, then least loaded); the `UPDATE` is what
   makes the choice binding. Two simultaneous hops cannot both take the last slot.
3. **No row returned means no room** — the travel is refused *now*, while the client is still in its
   current map. Nothing is constructed, and the client is never told to move.
4. On success the client is redirected: the destination world's `address` and `port` from the
   registry, a map token, and a player token the destination will validate
   (`docs/03-protocol.md`, "the redirect").
5. The player leaves the old world and connects to the new one. The old world records the departure;
   the new world clears the instance it assigned and marks it leased to that session.
6. **Lease hygiene.** A lease belongs to a player session. When the player disconnects, leaves the
   instance, or the owning world stops heartbeating, a reaper returns the slot (`leased - 1`). A
   stuck lease costs capacity, never correctness, and the reaper bounds it.

## Instances

| map type | lifecycle | count |
|---|---|---|
| outpost / town | prebuilt at world startup, one per map per district, never destroyed | bounded by content |
| explorable | prebuilt **pool** per map; capacity is how many parties may be inside at once | configured |

- Assignment takes an instance from the pool, clears it (agents, pending messages, party and item
  state) and hands it to the party. Release clears it and returns it.
- The world server creates instances **ahead of need**, into its pool, and destroys them only when
  the pool is deliberately shrunk. This is the deviation from AzerothCore, which creates on demand:
  our client hangs when the destination is not ready, so on-demand creation is not available to us.
- Instance records persist in `game`, so a restart rebuilds the same pools and "who is where"
  survives an auth-service restart.

## Inside a world server

- **One IO reactor** accepts and serves gameplay connections. The client connects here directly; the
  auth service is not in the data path.
- **A bounded pool of update threads with map affinity.** A hot map is always updated by the same
  thread, so simulation state needs no locking, and the pool size is configuration rather than a
  function of map count. Idle maps cost nothing.
- **Database access through a pool per database**, with writes enqueued to an asynchronous worker so
  the simulation loop never waits on the disk. Multi-statement changes are transactions. Queue depth
  is a metric, because a growing queue is the first sign of trouble.
- **Character state** is loaded on entering an instance, saved on leaving it and on a timer, never
  per frame. Position and inventory need to survive a crash within a minute, not every tick.

## Failure behaviour

| event | consequence |
|---|---|
| world server crashes | heartbeat stops; the auth service stops sending players to it; leases held by its players expire and are reaped; players log in again; the database is not corrupted, because saves are transactional |
| world server restarts | re-registers, rebuilds its pools from the database, becomes a candidate again once ready |
| auth service restarts | players keep playing (it is not in the data path); new logins wait; in-memory sessions are lost, so a travelling player must re-authenticate |
| database unavailable | fail closed: no logins, no travels. Players already in a map stay there, and the world server keeps what it can in memory until it must stop |
| stale registry row | the reaper marks the world offline and frees its leases; the auth service skips it |

The asymmetry is deliberate: losing the auth service interrupts *entry*, losing a world interrupts
*one map*, losing the database interrupts *everything* — which is why the database gets the
operational attention (backups, a restore test, later a standby).

## Startup order

1. Database reachable, schema current.
2. World servers start: connect, prebuild outposts and pools, register in `world`.
3. Auth service starts: binds the login endpoint, reads the registry.
4. Players log in; the auth service answers with a world that is registered, online and has capacity.
5. Shutdown: worlds flush character state, mark themselves draining, exit; the auth service stops
   accepting, then exits.

A world that is still prebuilding is not advertised, so a request arriving during startup gets a
correct "no room" instead of a race.

## Cost model

Measured from GWPS, per map instance, against what this structure changes:

| item | GWPS today | ASURA target |
|---|---|---|
| private memory per instance | 11.9 MB | ~5 MB (shared `Gw.dat` index, database pool per world not per instance) |
| OS threads | 1 per instance | bounded pool, constant with instance count |
| database connections | 1 per instance | a small pool per world server |
| `Gw.dat` index | private copy per instance (4.7 MB) | one memory-mapped copy per world server |
| SQLite page cache | 2 MB per instance | gone — pooled PostgreSQL connections |

Sizing: a world server holding 500 instances is roughly 2.5 GB. More worlds, more machines, same
code — the client is told a real address, so nothing in the protocol changes when a world moves.

## Deliberate non-goals

- Orchestration and autoscaling. Adding a world is a config entry and a process.
- Distributed consensus. The database is the arbiter; there is no second source of truth.
- Multi-region. One region; districts are capacity, not geography.
