# Glossary

Plain-language definitions for every word this project leans on. If a document uses a term without
explaining it, it is defined here.

### Programs and state

Component responsibilities are maintained in [Architecture](../guidance/architecture.md#components).
These are short lookup terms, not a second inventory.

- **ASURA**: the whole server project, not one process.
- **Socket layer / gateway**: the client-facing listener and gameplay relay.
- **Auth**: the login conversation library invoked by the gateway.
- **Rules**: pure decisions over caller-provided values; the Rust/Lua scope remains unresolved in the scripting guide.
- **World server**: the process hosting map instances and connected players.
- **Simulator**: the world-state advancement and actor-operation machinery; full NPC simulation is planned.
- **Control plane**: identity, placement and service/registry coordination.
- **Registry**: database records describing services/worlds; `t_service` and `t_world` have different owners.
- **Pool capacity**: advertised and leased seats in `t_world_pool`; this is not a pool of fully simulated parties.
- **Reservation / lease**: a session's claimed seat; cleanup limits are in Architecture's lease ledger.
- **Connection pool**: reusable database connections.
- **Map updater**: the task advancing an instance; current implementation and proposed scheduling are distinct.
- **GWPS**: the reference server project; it is not a runtime dependency.

### Maps and players

Definitions and dated assertions for **account**, **character**, **agent**, **map**, **instance**,
**outpost**, **explorable**, **district**, **party** and **unlock** are maintained in the
[World model](game-world-model.md#definitions-and-code-names). A map is static; an instance is a
running copy. Earlier glossary wording that treated them as synonyms is superseded by R1.
The model records the distinction between observed client vocabulary and unimplemented rules.

**Prop**: scenery from map data, distinct from a running agent; an archive prop index is not
established as the identifier in an agent-touch travel message.

**Map instance count**: the number of running copies of a map, not the catalogue's map count.

### Movement

**Travel / hop** - moving a character from one instance to another, as the game does when you walk through
a portal. The client disconnects, is told where to go, and reconnects.

**Portal**: an entry/exit between map instances. Archive rendering, geometric doorway detection
and agent-touch messages are distinct subjects; see the World model and Client protocol.

**Unlock**: reached-map state; its storage and unresolved gating policy are in the World model.

**Redirect** - the message that tells the client where to go: an address, a port, a map, and a key.
Until the client receives one, it does not move.

**Token** - a value the auth service hands out for one travel, so that when the client reconnects, the new
connection can be recognised as the one that was expected. A wrong token is refused.

**Allocation** - deciding which world server a travel request goes to, and saying no if none fits.

**Refusal** - the honest answer when there is no room: the client is told nothing about a
destination it cannot go to. It never enters a loading screen.

### Prebuilding

**Prebuild / pre-cache** - build map servers *before* anybody needs them, at startup.

**Pool** - a set of prebuilt instances waiting to be handed out. A party takes one, and returns it
cleared when it leaves.

**Destination readiness**: placement must offer a reachable destination. Current preload and
optional on-demand behavior are described in Architecture and the protocol's measured experiment.

### The network

**Handshake** - the opening exchange where the client and server agree a shared secret so the rest
of the conversation is encrypted.

**DH (Diffie-Hellman)** - the specific handshake scheme the client uses, with fixed constants baked
into both the server and the patched client.

**ARC4** - the stream cipher the client uses on everything after the handshake.

**Framing** - how messages are delimited on the wire: a header-selected field layout, including any counts; there is no outer length field. See [Client protocol](client-protocol.md#1-framing).

**No world link** - there is deliberately no control protocol between the auth service and the world
servers. They coordinate through the registry and pool tables. The only protocols left are the
client-facing ones.

**Heartbeat** - a periodic "still here, this full, this busy" message from a world server, so the auth service
can tell a live world from a dead one.

**Directory** - a lookup of world locations and capacity. Current placement reads the database registry, not a separately established in-memory directory; see Architecture.

### Machinery

**Thread** - one line of work inside a program. The CPU can only do one thing per core at a time;
threads are the things it switches between.

**Reactor** - one thread that watches many connections at once and handles whichever is ready. Lets
many instances share a few threads instead of one thread each.

**mmap** - reading a file's index straight from disk without loading a private copy into memory,
so many copies of the same data can share it.

**`Gw.dat`** - the game's asset archive, several gigabytes. Our servers read an index of it; the
client streams the rest from ArenaNet's file servers.

**Manifest** - the list of files the client is told to fetch. Serving it ourselves is what would
make the server fully independent of ArenaNet; it is a separate service, not part of the auth service.
