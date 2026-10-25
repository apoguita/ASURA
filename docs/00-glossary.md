# Glossary

Plain-language definitions for every word this project leans on. If a document uses a term without
explaining it, it is defined here.

### The programs

**ASURA** — *Automated Server Utility for Remote Authentication.* The program this repository is
about. It answers the game client, knows who is logged in, and decides where a player's map runs.
It never runs a map itself.

**World server** — a program that runs maps for players. In phase 1 these are the existing C game
servers; in phase 2 the simulation becomes part of this project. Either way they are disposable:
restarting one loses a session, not the database.

**GWPS** — the existing single-process project (`PreSearing`). It does the job of ASURA *and* the
world servers in one program. It stays as the working reference.

### Maps and players

**Map** — a place in the game: Ascalon City, Lakeside County.

**Outpost / town** — a map everyone shares. All players in Ascalon City see each other.

**Explorable** — a map a single party shares privately. Two parties in Lakeside County are in two
separate copies and do not meet.

**Instance** — one running copy of a map. Three solo players in Lakeside = three instances. A
shared outpost with fifty players = one instance.

**District** — a numbered copy of the *same* map, used when a shared map is full. Ascalon City 1, 2,
3… are separate instances of the same outpost.

**Party** — the group of players that shares an explorable. One player counts as a party of one.

**Map instance count** — how many copies of a map exist at once. This is what costs memory.

### Movement

**Travel / hop** — moving a player from one map to another, as the game does when you walk through
a portal. The client disconnects, is told where to go, and reconnects.

**Redirect** — the message that tells the client where to go: an address, a port, a map, and a key.
Until the client receives one, it does not move.

**Token** — a value ASURA hands out for one travel, so that when the client reconnects, the new
connection can be recognised as the one that was expected. A wrong token is refused.

**Allocation** — deciding which world server a travel request goes to, and saying no if none fits.

**Refusal** — the honest answer when there is no room: the client is told nothing about a
destination it cannot go to. It never enters a loading screen.

### Prebuilding

**Prebuild / pre-cache** — build map servers *before* anybody needs them, at startup.

**Pool** — a set of prebuilt instances waiting to be handed out. A party takes one, and returns it
cleared when it leaves.

**The one rule** — a travel request is only ever answered with something already running. Nothing
is constructed while a client is loading. Violating this is what made hops hang in GWPS.

### The network

**Handshake** — the opening exchange where the client and server agree a shared secret so the rest
of the conversation is encrypted.

**DH (Diffie–Hellman)** — the specific handshake scheme the client uses, with fixed constants baked
into both the server and the patched client.

**ARC4** — the stream cipher the client uses on everything after the handshake.

**Framing** — how messages are delimited on the wire: a header, a size, then a body.

**World link** — our own protocol between ASURA and a world server. It is entirely ours, so it is
versioned and explicit.

**Heartbeat** — a periodic "still here, this full, this busy" message from a world server, so ASURA
can tell a live world from a dead one.

**Directory** — ASURA's in-memory list of which world servers exist, what they host, and how full
they are.

### Machinery

**Thread** — one line of work inside a program. The CPU can only do one thing per core at a time;
threads are the things it switches between.

**Reactor** — one thread that watches many connections at once and handles whichever is ready. Lets
many instances share a few threads instead of one thread each.

**mmap** — reading a file's index straight from disk without loading a private copy into memory,
so many copies of the same data can share it.

**`Gw.dat`** — the game's asset archive, several gigabytes. Our servers read an index of it; the
client streams the rest from ArenaNet's file servers.

**Manifest** — the list of files the client is told to fetch. Serving it ourselves is what would
make the server fully independent of ArenaNet; it is a separate service, not part of ASURA.
