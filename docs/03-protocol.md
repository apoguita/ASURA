# Protocol

The wire facts the auth service has to reproduce. Everything here is derived from the GWPS implementation and
from captures of a working session; where a shape is only known approximately it is marked
**unverified**, and the reference code is named so the port can be checked byte for byte instead of
trusted.

Source of truth for every byte below: `PreSearing/code/OpenTyria/` (message tables in `msgdefs.c`,
handshake in `AuthSrv.c`, travel in `GameSrv.c`, captures in `PreSearing/captures/`).

## 1. Framing

After the handshake, every message in both directions is a small fixed-size structure:

```
u16 header        message id, little endian; bit 15 set means client to server
u8  body[...]     the message's fields, fixed layout per header, and nothing else
```

**There is no length field.** An earlier version of this document claimed a `u16 length` followed the
header. That was wrong, and it is the kind of wrong that costs a session: a port that writes a
length the client does not expect gets back nothing but error `007`. Corrected from the reference:

- `pack_msg` (`msgpack.c`) writes the format's fields and nothing else; `pack_helper` handles
  `TYPE_MSG_HEADER` as exactly two bytes.
- `AuthConnection_SendMessage` (`AuthSrv.c`) is called with `sizeof(struct)` as its input length and
  passes the *written* count back out of `pack_msg`; the written count comes from the format.
- The receive path (`AuthSrv_GetMessages`) reads the header, indexes the format table, and lets
  `unpack_msg` consume the fields. No length is ever read.

So a message's size is a property of its header alone. That table is
`crates/asura-protocol/src/client/tables.rs`, generated from `msgdefs.c` and `opcodes.h` by
`tools/gen-auth-tables.py`, and it carries each message's field layout beside its size so the size
is verifiable rather than trusted. Framing then has only two failure modes: a header that is not in
the table (refuse — GWPS logs the bytes and closes), and a body that has not arrived yet (wait).

The one nuance for later: some **game** messages carry a four-byte header — the u16 header plus a
u16 the captures always show as zero (`TYPE_MSG_HEADER_GAME`). It is opt-in per message in the
reference, because turning it on globally broke login. The auth family does not use it.

The whole stream is ARC4-encrypted after the key exchange.

`GWPS_TRACE=1` logs every outgoing message as header, size and the first bytes — that is the
cheapest way to compare a Rust port against the C reference.

## 2. Identifying the connection

The first message a client sends decides what the connection is. It is a bare little-endian `u32`,
**not** a framed message: it has no two-byte header, so decoding it as one would read `0x0400` as a
message id. GWPS compares it against three constants and routes the socket accordingly
(`AuthSrv.c`, `Connection_ReadVersion`):

| header | constant | message | size |
|---|---|---|---|
| `0x000C0400` | `AUTH_CMSG_VERSION_HEADER` | auth: `{header, version, h0008, h000C}` | 16 |
| `0x000C0500` | `GAME_CMSG_VERSION_HEADER` | game: the auth shape plus map/player tokens and both UUIDs | 64 |
| `0x000C0FFF` | `CTRL_CMSG_VERSION_HEADER` | control: the header alone | 4 |

The auth service must make the same decision on its listener, and must reject anything else instead
of guessing.

One fact to keep rather than to re-learn: GWPS stores `version`, `h0008` and `h000C` from the auth
version message and **never compares them to anything**. They are not a compatibility gate.
Reproducing a gate the reference does not have would be inventing protocol; keep the fields for the
packet trace.

## 3. Key exchange

Three messages and one derivation. The sizes and constants below are what
`crates/asura-protocol/src/client/handshake.rs` implements, and its arithmetic is pinned by fixtures
generated from GWPS's own C (`tools/gen-handshake-fixtures.py`).

| step | who | what |
|---|---|---|
| 1 | client → server | `MSG_CLIENT_SEED`, header `0x4200`, then **64 bytes**: the client's public value as a 512-bit integer, little endian. 66 bytes total, plaintext. |
| 2 | server | `shared = client_public ^ server_private mod prime` — a **512-bit** prime and a 511-bit exponent, both hardcoded in GWPS `main.c` and copied verbatim. |
| 3 | server | draw **20 random bytes**: the master secret. |
| 4 | server → client | `MSG_SERVER_SEED`, header `0x1601`, then **20 bytes**: `master[i] XOR shared_le[i]`. 22 bytes total, plaintext — it is how the client recovers the master secret. |
| 5 | both | `key = arc4_hash(master)` — 20 bytes, from the client's own construction, which is not a standard hash. |
| 6 | both | ARC4 keyed with those 20 bytes, one instance per direction, both starting at offset zero. Everything from here is scrambled. |

What is **not** on the wire, and matters: no generator, and no server public value. The client's patch
carries the matching parameters, so the server's only arithmetic is raising the client's number to a
fixed exponent. That is why the risk here is transcription, not key management: **the constants must be
identical** or the client fails the handshake and reports nothing but an opaque error.

One deviation, deliberate and recorded (`docs/adr/0009-crypto-primitives.md`): a client public value
congruent to zero modulo the prime yields a zero shared secret, and ASURA refuses the connection
instead of using it. GWPS would carry on with a key that had never been exchanged. The real client
cannot present such a value.

**How this is verified without a client:** the published RC4 vectors, plus fixtures produced by
compiling the reference's `arc4.c` and its `arc4_hash`, plus the exponentiation checked against
CPython's `pow()`. What they cannot prove is that the client accepts the result — that is the packet
trace diff, and it stays M3's acceptance criterion.

## 4. Login and session

1. The client obtains `user_id` and `session_id` (UUIDs) from the portal/web gate, not from us.
2. It sends them in the login message on the auth connection: `AUTH_CMSG_PORTAL_ACCOUNT_LOGIN`
   (`0x8038`, 150 bytes on the wire) carrying `user_id`, `session_id`, a character name and a type.
3. The auth service validates the pair, and answers with the account's character list.
4. The client selects a character; the auth service records the selection.

A login for an account that still holds a session must be resolved deterministically, not by
crashing: GWPS's rule — an incomplete transfer is cleared, a live one is refused — is the behaviour
to keep.

**The portal is not a service here.** In the rig, `user_id` and `session_id` come from a client-side
DLL (`PreSearing/portal/portal.c`) that replaces the client's portal functions and hands it a fixed
pair per test account — there is no HTTP and nothing for this project to serve. What the pair needs
is a matching row, so the database is seeded, not the wire:

| GWPS (`DbSchema.h`, `DbSessionColumnsDef`) | ASURA (`crates/asura-db/migrations/0003_portal_identity.sql`) |
|---|---|
| `session.user_id` | `session.portal_user` |
| `session.session_id` | `session.id` |
| `session.account_id` | `session.account_id` |
| a row must match **both** ids; no row is an auth error, never a crash | `login::find_session(portal_user, session)` returns `None`, and `None` is a refusal |

`tools/seed-dev.ps1` writes the three pairs the DLL knows, and re-running it reopens them; `login` is
`crates/asura-db/src/login.rs`, with the refusal rules under test in `crates/asura-db/tests/login.rs`.

**The portal is an HTTP web gate on loopback, and it *is* something this project has to serve.**
Corrected from a measurement with the real client (build 39112, 2026-10-26). The client launched with
`-portal 127.0.0.1` speaks plain HTTP to `localhost:6601` and gets its identity from four endpoints:

| # | request | reply |
|---|---|---|
| 1 | `GET /Spawned/WebGate/session/create.xml` | `<Reply><Session>UUID</Session></Reply>` — a bearer id for the next calls |
| 2 | `POST /Spawned/WebGate/users/login.xml` (`Authorization: Arena <session>`, body carrying `Provider`, `LoginName`, base64 `Password`, `GameCode`) | `<Reply><UserId>…</UserId>…</Reply>` — the `user_id` of the login message |
| 3 | `POST /Spawned/WebGate/my_account/game_accounts.xml` | the `gw1` account row |
| 4 | `POST /Spawned/WebGate/my_account/token.xml` | `<Reply><Token>…</Token></Reply>` — the `session_id` of the login message |

All four were observed answering `200` from the reference web gate (`PreSearing/tools/webgate.py`),
and the `user_id`/`session_id` it returns are the pair the login message then carries. So the auth
service's scope line — "the web gate: the portal login that hands the client its user_id/session_id"
(`docs/01-scope.md`) — is real work, not a figure of speech. Nothing else about the earlier account of
the portal was wrong (its identity pairs are fixed per test account), but "there is nothing to serve"
was wrong and is corrected here.

**This project serves it: `crates/asura-webgate`.** It is a faithful port of the reference's
`tools/webgate.py` — same four endpoints, same XML, same status codes (`400 who are you?`, `400
Unsupported provider`, `401` without a session) — with one deliberate change: the reference answers
from a hard-coded table of three emails, and this answers from the rows `tools/seed-dev.ps1` writes
through `asura_db::portal::identity_for_email`, so the identity it hands out is the very
`(portal_user, session id)` pair the auth service will resolve. No Python, no reference checkout.

**A login does not spend the session.** GWPS's login path calls `Db_GetSession` and writes nothing,
and the rig presents the same fixed pair on every login — so a service that closed the row on
disconnect or on login would refuse the client's very next attempt. "The newest wins"
(`docs/05-gw2-lessons.md`, rule 1) is a rule about live *connections* and about leases; it is not
licence to consume a row. `login::close_session` and `login::close_other_sessions` exist for the
paths that do end a session deliberately (an explicit logout, a stale sweep) and are deliberately not
called from the login path.

**The reply, in the order GWPS sends it** (`AuthSrv_HandlePortalAccountLogin`):

| # | message | contents |
|---|---|---|
| 1 | `AUTH_SMSG_CHARACTER_INFO` `0x0007`, once per character | request id, character id, name (counted UTF-16, 20 slots), a 64-byte settings block |
| 2 | `AUTH_SMSG_ACCOUNT_SETTINGS` `0x0016` | request id, `n_data`, then a **591-byte** fixed blob inside a 2048-byte field |
| 3 | `AUTH_SMSG_FRIEND_STREAM_END` `0x0014` | the request id, twice |
| 4 | `AUTH_SMSG_ACCOUNT_INFO` `0x0011` | request id, the account and selected-character ids, a 200-slot feature array of which 8 are used |
| 5 | `AUTH_SMSG_REQUEST_RESPONSE` `0x0003` | the request id and the status: `0` accepted, `11` (`GM_ERROR_AUTH_ERROR`) refused |

All five are sent, in that order, and `crates/asura-auth/tests/login.rs` asserts the sequence and the
two fixed sizes over a socket. A refusal sends only row 5.

Two details that are easy to get wrong and are now pinned by tests:

- **The settings blob is a copied fact.** 591 bytes the client is handed on every login, and nothing in
  this project knows what most of it means. It is parsed out of the reference's own literal by
  `tools/gen-handshake-fixtures.py` rather than typed — see `client/login_fixtures.rs`.
- **A counted field still occupies its full width.** The feature list in row 4 is an `ARRAY_8` of 200
  slots with a count of 8: the four-byte count, then all 200 bytes. Sizing a message from the count
  instead of the capacity is how a body ends up 192 bytes short.

**UUIDs on the wire are the RFC 4122 little-endian variant** — the first three fields little endian,
the clock sequence and node not (`uuid_enc_le`, GWPS `GmUuid.h`). `uuid::Uuid::to_bytes_le` and
`from_bytes_le` are the conversions; reading them in canonical order swaps the halves of every id, and
the only symptom would be a login that never resolves.

## 5. Travel: the request/redirect cycle

This is the part the whole design exists to protect.

1. **Client asks to travel.** On the game connection, the client sends the district it wants:
   `{map_id, map_type, region, language, district}` with a request id. In GWPS this arrives as
   `CTRL_CMSG`-shaped `StartPlayerTransfer` from the world, and as a direct request message on the
   auth connection at login; both are allocation requests.
2. **the auth service allocates.** It answers only from a world that has the map ready (`docs/02-architecture.md`).
3. **the auth service issues a token.** A fresh player token plus the identity the client's *next* connection
   must present.
4. **The client is redirected.** The response is `TRANSFER_GAME_SERVER_INFO` (`0x01A6`): **GWPS's
   exact struct** (`GameMsg.h:659`, sent as `sizeof(*msg2)`) — **46 bytes, a two-byte header,
   `host[24]`, then five dwords in GWPS's order: `map_token, region, player_token, is_explorable,
   map_id`.** The field order is GWPS's, not the live capture's: `player_token` comes before
   `is_explorable`/`map_id`, and `map_id` is last. Measured against a real client: the captured
   order in a 46-byte frame makes the client read the map id out of the token slot (`007`), and a
   52-byte frame with a four-byte header makes it read `00 00` as the address family (`005`).
   GWPS's bytes are the port target because they are what a working server sends.
   - `host` is 24 bytes: `AF_INET` little endian, port **big** endian, four address bytes, then zero.
   - `is_explorable` describes the **destination** map type — `MapType_MainExplorable |
     MissionExplorable | ArenaExplorable` (1/4/6, `GmMap.h:22-27`) are explorables.
     Deriving it from the source map was a bug that produced client error 005.
5. **A map update follows immediately.** `MAP_UPDATE_CURRENT` (`0x0099`) is sent right after the
   redirect — in live captures the two appear in the same millisecond. 12 bytes:
   `header, map_id, 0`. See the size trap below.
6. **The client reconnects.** It drops the old connection and opens a new one to the address it was
   given, presenting the token. That reconnect is accepted only if it matches the recorded identity;
   a mismatch is client error 007.

### 5c. Is precaching the destination a requirement? (the `--on-demand` experiment)

The reference pre-spawns **one game server per configured map** at startup
(`AuthSrv_PreSpawnMapServers`, `AuthSrv.c:410-449`) so that a travel never constructs anything; its
own comments record why: a map server built *inside* a teleport made the client wait on the loading
screen for the whole construction. This server preloads every hosted map for the same reason — the
`map_config` row, the seat pool row and the path graph are all in place before the first client
connects.

Whether a client actually **requires** the destination to exist before the hop is untested here. That
is what `--on-demand` (or `ASURA_WORLD_ON_DEMAND=1`) measures. With no `--maps` it means **nothing is
preloaded**: the world starts empty and every map is loaded the first time a client names it — at
login (the client's first connection names the character's map) or at `/tp`. No map id is needed on
the command line; that is the whole point: start the server, log in, play, travel.

When a map is named, the demand-load walks the three steps a preload would have done at startup — the
row from `map_config`, the seat pool row in `world_pool`, the path graph out of `Gw.dat` — and logs
each step's cost:

```
world: map 148 'Ascalon City (pre-Searing)' loaded on demand in N ms (row a ms, pool b ms, path graph c ms)
world: "…" teleporting: map 146 -> map 148 …
```

The one thing an on-demand world publishes up front is a **seat pool row per catalogue map** — a
line in the database saying *"this world can host map X"*, which is what placement reads to decide
where a login may go. It is not an instance and loads nothing: the map's data still arrives when a
client first names it.

The runbook is the experiment itself:

```powershell
.\tools\start-server.ps1 -OnDemand
# client: F:\GW\GW1\Gw.custom.exe -authsrv 127.0.0.1 -portal 127.0.0.1
# just log in, then /tp ascalon
```

The verdict is the client's loading screen plus the console lines above. **Measured 2026-10-27 on
the real client**: `start-server.ps1 -OnDemand` with nothing preloaded, then a loop of teleports
148 -> 163 -> 164 -> 165 -> 166 -> 148 ... — every destination was loaded on demand (`0-2 ms` per
map, with the path-graph step failing fast because no `Gw.dat` was local) and the client took each
hop without an issue. **Precaching is therefore not a requirement on ASURA**; the loading-screen
hang that forced the reference to pre-spawn one server per map (`AuthSrv_PreSpawnServers`) was an
OpenTyria property, not a property of the client or of this world. Give the world a local `Gw.dat`
(`ASURA_GW_DAT` or `data\Gw.dat`) to get the real path-graph cost in that number.

The server half of the experiment is pinned by
`a_connection_to_an_unhosted_map_is_loaded_on_demand` and
`a_travel_to_an_unhosted_map_is_loaded_on_demand` (`crates/asura-world/tests/load.rs`): the
connection or transfer names the unhosted map, its entry and pool row appear when it is demanded,
and the client lands in the destination's own configured spawn point.

### 5d. Path graphs without `Gw.dat` (the dump)

The server paths agents from its own copy of each map's walkable geometry: planes, trapezoids,
portals and the split tree the reference's A★ (`GmPaths.c`) walks. That geometry lives in the
client's asset archive, `Gw.dat`, as one blob per map — there is no separate map file. Reading the
archive at runtime works but needs 3.6 GB on every server machine, so the graph can be **dumped
once** and loaded instead:

```powershell
# once, with a client archive (ASURA_GW_DAT or --gwdat):
asura-world --dump-path-graphs data\path-graphs.bin
# every start after that, no archive needed:
asura-world            # picks data\path-graphs.bin up automatically
```

- `--dump-path-graphs FILE` writes the maps in `map_config` (the twelve of the reference);
  `--dump-all` writes every pathing map in the archive instead. `--path-graphs` /
  `ASURA_PATH_GRAPHS` name a dump explicitly; `--gwdat` stays the fallback when no dump exists.
- **Measured 2026-10-27**: 12 of 12 catalogue maps, 486 planes, 62,326 trapezoids, **11.1 MB** —
  the world then starts with `path graphs from the dump data\path-graphs.bin: 12 of 12 map(s)
  bound` and never opens the archive.
- The format is ours (`crates/asura-world/src/graph_dump.rs`), and it carries exactly what the
  finder consumes. The community `.pmap` files (ACB's Pathingmap Builder, e.g. the archives the
  GW-Pathing-Map visualization projects ship) are *visualization* data: trapezoids and adjacency
  only, no portals or node tree, so they cannot feed the reference's A★. Our dump is produced from
  a client archive once and then travels with the server.
- **The bit-31 trap**: a catalogue blob id may carry the archive's "replacement pending" bit
  (`0x8005EA96` for The Northlands) while an archive binds the plain id (`0x5EA96`). Both are tried
  (`find_path_blob`, `docs/03-roadmap.md`), and a substitution is logged.
- Parity is pinned by `crates/asura-world/tests/path_dump.rs`: the walks from a dumped graph are
  compared, waypoint for waypoint, with the walks from the archive import (maps 146 and 148, which
  share one blob), the dump command's own file is loaded back and bound to the catalogue, and the
  **bundled** `data/path-graphs.bin` is walked directly with no archive present.

The dump is derived game data and it **is bundled with this repository**: `data/path-graphs.bin` is
committed (11.1 MB), so a fresh clone answers pathfinding without finding a client archive — the
whole server, pathing included, is one clone. The raw `Gw.dat` itself stays local (3.6 GB, the
game's file); `/Gw.dat` and `/data/Gw.dat` are the only ignored archive paths, and the dump is
regenerated with `--dump-path-graphs` when the catalogue or the client changes.

### 5a. The auth-side pair (measured from the reference)

Before any of that, on the **auth** connection, the client asks for a map and the auth service says
where to go. Two fixed-size messages, and one row for each in the generated table:

| message | header | wire size | fields after the header |
|---|---|---|---|
| `AUTH_CMSG_REQUEST_GAME_INSTANCE` | `0x8029` | 26 | `req_id, map_type, map_id, region, district, language` — six dwords |
| `AUTH_SMSG_GAME_SERVER_INFO` | `0x0009` | 42 | `req_id, map_token, map_id, host[24], player_token` |

* `map_token` is the **world's id** in GWPS (`metadata->server_id`), and the game server is what
  decides whether it means anything.
* `player_token` is random per travel (`random_uint32`). It is the same number the auth service hands
  to the world server, so it is the only thing that ties the client's next connection to this one.
* `host[24]` is not a string: 2 bytes of address family (`AF_INET` little-endian, `AF_INET6`
  little-endian — Winsock's 2 and 23, not Linux's 10), then the port **big-endian**, then the address
  bytes, then zero padding. The little/big asymmetry is the reference's own.
* The info message is followed by an ordinary `AUTH_SMSG_REQUEST_RESPONSE` with status 0. A refusal
  is the response with `GM_ERROR_NETWORK_ERROR` (2) **and no info message** — the client then shows an
  error it can describe, where silence is a loading screen that never ends. Sending the info message
  for a port nothing is listening on is worse than either: the client leaves character select and dies
  on a connect timeout.

### 5e. Which wire the client on this server speaks (measured 2026-10-28)

There are two wire shapes for the same message set, and **this client parses only one of them**. Measured,
not inferred — three runs of the real client against this server:

| what was sent | what the client did |
|---|---|
| `0x001E` **8 bytes** (`1e 00 00 00` + elapsed) as in the retail capture | connection accepted, load block played, then **error 007** about 300 ms after the spawn. Server log: `joined map 148` → `move[spawn]` → `socket: early eof`, 3 ticks sent |
| `0x001E` **6 bytes** (`1e 00` + elapsed) | loads and plays |
| click answered with `0x002B` (16 B) + `0x0029` **24 B** (trap and plane as dwords), retail layout | **the client crashed** on the first click |

So the retail capture (`recordings/client/feature/movement/movement-sniff.log`, a `Gw.exe` on live servers) describes the **four-byte
header** generation of the protocol: `1e 00 00 00` ticks, `29 00 00 00 | agent | x | y | trap | plane`,
`25 00 00 00` directions, `2b 00 00 00` speed. The client used against this server — the build GWPS is
patched onto and the one that has always worked here — parses **two-byte headers** with the reference's own
sizes: a 6-byte tick, an 18-byte `0x0029` (`agent, x, y, plane u16, current_plane u16`), `0x0028` for a
stop, `0x002C` for a position, and it has no `0x0025` or `0x002B` at all.

The consequence is the rule this project follows for the wire: **the shapes are the reference's, because
that is what this client parses.** The retail capture stays the authority on *behaviour* (what a click, a
manual walk and a cancel mean, and what the server does with the agent's copy), and it is kept in the
protocol crate as builders and tests (`agent_move_to_point_live` and friends) so the difference is recorded
rather than remembered. Sending the retail shapes to this client is not a parity improvement; it is a
disconnect (`007`) or a crash.

**One path, two planes: the client asserts.** A walk whose first waypoint is on a different plane from the
agent crashed the client with `Assertion: pos.y >= worldDims.y0`, `P:\Code\Engine\Agent\agint.h(929)` — the
agent's position is validated against the box of the plane it is on. Measured case (map 148, Lakeside blob
`0x1B97D`), click from `(9461.8, 7481.2)` plane 0 to `(8076.0, 6151.5)`:

| waypoint | reference's own path (`tools/path-oracle.exe`, GWPS's `PathFinding`) | retail server |
|---|---|---|
| 0 | `(9251.4, 7279.3)` **plane 19**, trap 5506 | never leads with a plane change |
| 1 | `(8426.5, 6487.8)` plane 0, trap 1915 | |
| 2 | `(8076.0, 6151.5)` plane 0, trap 1913 | |

Both this port and the reference produce waypoint 0 **stamped with the plane it enters, in the middle of
that plane and 290 units from the agent**. The retail servers' cross-plane destinations are always portal
corners reached after a same-plane waypoint (`(10860.0, 4532.0)`, which is exactly plane 18's own corner).
So the world hands the client the reference's path minus any **leading** waypoint on another plane
(`client_walk` in `asura-world`), and the client walks the straight segment across the slope itself. The
reference's own list is unchanged and still compared to the oracle; only what reaches the wire is filtered.

## 6. Traps that cost real time in GWPS

| trap | what happens | rule |
|---|---|---|
| `AUTH_CMSG_SEND_COMPUTER_INFO` sized from the table | the client sends it as **42 bytes** — `u16` counts, variable-length strings, no padding — while the table says 138; framing by the table swallows the message behind it and desynchronises the stream | consume this one structurally (`client::messages::decode_computer_info`), not by table size. Measured against a real client, build 39112: it arrived as 42 bytes of computer info plus a 22-byte `SEND_COMPUTER_HASH` |
| `sizeof(struct)` taken as the wire size | two extra bytes on the wire, so the client misreads every message after it | the size is the format's field sum, not the struct's size. `AuthSrv_SessionInfo` is 14 bytes in C and 10 on the wire — the format has two dwords where the struct has three |
| a length field written where the protocol has none | the client misreads the body as the next header: error `007` | there is no length field (`section 1`) |
| `0x0099` size | 12 bytes in the login batch makes the client fail with 007; the same message is 12 bytes (5 in the login batch) | size per context, not per header |
| `is_explorable` from the wrong map | error 005 after travel | always from the destination district |
| settings upload not acknowledged | hang correlated 7/7 with unacknowledged uploads | always answer `AUTH_CMSG_SETTING_UPDATE` with a request response |
| a transfer replayed from a stale queue | client lands in the previous destination | clear the queue after completing one |
| constructing a server during the hop | loading screen forever, zero packets after | the one rule, `docs/02-architecture.md` |
| "chart commands" — chat the **server** generates — sent instead of printed | client crash | print it: the reference returns before the wire for every server-generated answer (`GmChat.c:150-153`, `GmChat.c:209-212`), so printing is the behaviour to match, not a workaround |

Each of these was diagnosed by logging every message at each hop, not by reading code. Keep that
capability: a per-message trace with direction, header, size and first bytes, toggled by an
environment variable.

## 7. File check

The client also asks a file service whether a file is current: a 21-byte request answered with 32
bytes. GWPS answers it correctly when it receives it (`AuthSrv_TryHandleFileCheck`). Whether the auth service
needs to answer it depends on whether the client is pointed at us for content; today it is not.

## 8. Coordination between programs: the database, not a protocol

There is no control protocol between the auth service and the world servers for state or placement
(`docs/adr/0004-port-azerothcore-structure.md`). They coordinate through tables:

| table | written by | read by | carries |
|---|---|---|---|
| `world` | each world server | auth service | id, address, port, flags, population, heartbeat |
| `world_pool` | every travel (a reservation), the reaper | travel decisions | per world, per map: capacity and leases |
| `lease` | every travel (a reservation), the reaper | the reaper, the reconciler | one row per reserved slot: world, map, session |
| `session` | auth service | world servers | account, character, issued token |
| `game` schema | world servers | world servers, auth service | characters, items, instance records |

Rules that make this safe rather than hopeful:

- **A reservation is one atomic statement.** `UPDATE world_pool SET leased = leased + 1 WHERE
  world_id = $1 AND map_id = $2 AND leased < capacity RETURNING world_id` either wins a slot or
  matches no row. Two processes cannot both take the last one.
- **A lease is time-bounded.** The reaper expires leases whose owner stopped heartbeating, so a crash
  costs capacity temporarily, never permanently.
- **Every program may fail and re-register.** No state is negotiated live, so there is nothing to
  resynchronise after a restart.
- **The protocol version lives in the schema.** A world server checks the schema version on startup
  and refuses to run against a mismatched database, loudly.

What remains a protocol is the client-facing half: framing, handshake, login and the redirect, all in
the sections above.

## 9. What must be re-proven in Rust

Not "ported and hoped". Each item gets a capture-based test:

0. the message table still matches the reference: `python tools/gen-auth-tables.py --check` (no
   client needed, so this runs first and stays cheap);
1. handshake completes and the client reaches the character list;
2. login works for a fresh account, and a second login for a signed-in account behaves the same way
   GWPS does;
3. a travel request produces a redirect the client accepts, and the client reconnects;
4. the reconnect is validated, including the rejection path (wrong token → error, not silence);
5. the map update after the redirect leaves the client in the right map.

## 10. Open questions

- **Why does the client drop the auth connection after its preamble, against this service but not
  against GWPS?** Measured 2026-10-26. Both servers see the same sequence — version, client seed,
  server seed, then the 64-byte preamble — and the client's traffic decrypts correctly in both, so the
  key exchange is right. Against GWPS the portal login follows about nine seconds later (when the
  credentials are submitted) and the session works; against this service the client opened a *second*
  auth connection, sent the same preamble, and then reset it without sending the login. GWPS's own log
  for the working run shows it mis-framed that same preamble (`Unhandled AuthSrv packet with header 35`),
  so the difference is not "GWPS handles it better" — something else about the exchange differs.
  Next evidence to gather: the exact error text the client displays, and a run of this service with
  timestamps and connection ids on every trace line. Both are cheap; neither has been done yet.
- **`AUTH_SMSG_0026` (`0x001A`) declares 32 bytes where its own layout can only occupy 31.** It is a
  `TYPE_NESTED_STRUCT` message, whose length depends on a count carried on the wire, and the
  declared maximum is one byte more than `1 + count * 2` can produce. GWPS would therefore consume
  31 bytes for it while a table-driven framer consumes 32, so a client that sends it would
  desynchronise this port and not GWPS. It is an unnamed friend/social message outside the login
  path, so it is recorded rather than guessed at; `crates/asura-protocol/src/client/tables.rs` lists
  it in `VARIABLE_LENGTH_HEADERS`. Resolve it when something needs the message.

## 11. The game connection: from placement to a character in a map

After `AUTH_SMSG_GAME_SERVER_INFO` the client opens a **second** connection, to the address the game
was launched with, and presents the tokens it was issued. The reference names its own address in that
message, which is why it never noticed; a client handed a *different* address still dials the launch
address (measured, build 39112), so ASURA's front door relays that connection to the world
(`crates/asura-auth/src/frontdoor.rs`). Everything below is measured from the reference: the
message layouts from its packed structs (`GameMsg.h`), the sizes from its own send log, and the order
from the functions named beside each step.

### 11.1 How the connection begins

1. **Classify.** The first four bytes are a `u32` and they name the connection kind, exactly as on the
   auth port: `0x000C_0500` is a game connection (`GAME_CMSG_VERSION_HEADER`).
2. **`GAME_CMSG_VERSION`, 64 bytes, fixed.** The client identifies itself and its travel:

   | off | size | field |
   |---|---|---|
   | 0 | 4 | `header` = `0x000C_0500` |
   | 4 | 4 | `version` (the client's build; **never validated**, by us or by the reference) |
   | 8 | 4 | `h0008` (observed 1, never compared) |
   | 12 | 4 | `map_token` — the world id from `GAME_SERVER_INFO` |
   | 16 | 4 | `map_id` |
   | 20 | 4 | `player_token` — the random token issued for this travel |
   | 24 | 16 | `account_uuid` (RFC 4122 little-endian) |
   | 40 | 16 | `character_uuid` (same variant) |
   | 56 | 8 | `h0038`, `h003C` (both 0) |

   The server **sends nothing in reply** to this message. There is no acknowledgement and no error
   message: a token pair the server does not believe in is answered by closing the socket, and the
   client reports that itself as error 007.
3. **Key exchange**, the same as on the auth port: `MSG_CLIENT_SEED` (`0x4200`, 66 bytes) →
   `MSG_SERVER_SEED` (`0x1601`, 22 bytes), then every byte in both directions is ARC4 with one shared
   keystream.

### 11.2 The token pair is the credential

The reference validates `(map_token, player_token)` plus the character id against what the auth
service recorded when it answered the travel, and closes the connection on any mismatch
(`AuthSrv_DoPostHandshake`). It keeps two pairs per account — *current* and *pending* — so a client
that stayed where it was and a client that took a transfer are both recognised, whichever order the
messages arrive in.

ASURA records that intent in the `transfer` table instead (migration 0004), and `claim_transfer` is a
single `UPDATE ... WHERE claimed_at IS NULL RETURNING`, so two connections presenting the same token
cannot both be believed. A world server checks the character id too: a live token with someone else's
character is refused.

### 11.3 The load block (server → client, sent unasked)

| # | message | header | size |
|---|---|---|---|
| 1 | `INSTANCE_LOAD_HEAD` | `0x017C` | 6 |
| 2 | `INSTANCE_PLAYER_DATA_START` | `0x0186` | 2 |
| 3 | `INSTANCE_LOAD_PLAYER_NAME` | `0x017D` | `2 + 2 + 2n` |
| 4 | `INSTANCE_LOAD_INFO` | `0x019A` | 16 |

`LOAD_INFO` is `{agent, map_id(u16), is_explorable(u8), district(u32), language(u8), is_observer(u8),
unk(u8)}`. The reference hardcodes `is_explorable = 1` with the comment `// what to put here?`.

### 11.4 The three things the client asks for

The client asks in the order **items (`0x8091`), spawn (`0x8088`), players (`0x8090`)**; the reference
answers whatever arrives, in whatever order.

* **items (`0x8091`, 4 bytes)** → the item stream opens, the inventory itself, four weapon slots, gold
  storage, the four faction caps, hard mode, character info, `MAP_UPDATE_CURRENT` (5 bytes, the *login*
  shape), `READY_FOR_MAP_SPAWN` (6), and the six-message download manifest.

  The inventory is `GameSrv_SendInventory`: for every bag **in bag-model order** (backpack `0`, material
  storage `5`, unclaimed `6`, storage `7`+, equipped `21`) — and *not* in bag-id order, which is what the
  capture's ids (1, 2, 6, 3, 4, 5) would suggest — first the item that *is* the bag if it has one, then
  `INVENTORY_CREATE_BAG` (`0x013F`, 13 bytes), then each item in it by slot:

  | message | header | wire layout |
  |---|---|---|
  | `CREATE_NAMED_ITEM` | `0x0161` | `u32 item_id, u32 file_id, u8 item_type, u8 dye_tint, u16 dye_colors, u16 materials, u8 unk1, u32 flags, u32 value, u32 model_id, u32 quantity, u16 n_name, u16 name[n], u8 n_modifiers, u32 modifiers[m]` |
  | `ITEM_MOVE_TO_LOCATION` | `0x013E` | `u16 stream, u32 item_id, u16 bag_id, u8 slot` (11 bytes) |
  | `ITEM_SET_PROFESSION` | `0x015A` | `u32 item_id, u8 profession` (7 bytes), sent only when the profession is not 0 |

  `CREATE_NAMED_ITEM` has **no fixed size**, and its two counted fields count differently: the name is a
  `u16` count of UTF-16 units and the modifier list is a **one-byte** count of dwords. That is the
  reference's own packer (`TYPE_NESTED_STRUCT`, `msgpack.c`), and it is why a cupcake's message is 36
  bytes while `sizeof(GameSrv_CreateNamedItem)` is 425. An item name is not a string: the backpack's name
  is a `Gw.dat` template id packed into units (`0x21A8 0xD157 0xB58F 0x166F`), one of which is a lone
  surrogate.
* **spawn (`0x8088`, 2 bytes)** → `INSTANCE_LOAD_SPAWN_POINT` (`0x0196`), 26 bytes:
  `{map_file_id(u32), position(2×f32), plane(u16), unk0, is_cinematic, unk1[8]}`. These two numbers —
  the Gw.dat blob and the coordinate inside it — are what a client needs to load a map at all.
* **players (`0x8090`, 19 bytes)** → the long block: PvP unlocks, the account features, the unlocked
  map list, `INSTANCE_LOADED` (the player's team token), skills and attributes, factions, the load
  time, `PLAYER_CREATE` (`0x0059`, `25 + 2n`), `WORLD_CREATE_AGENT` (`0x0020`, **99 bytes, fixed**),
  the agent's health/energy/level/effects/equipment/cape, `WORLD_UPDATE_CONTROLLED_AGENT`, the party
  (create/add/end + pointer), and finally **`INSTANCE_LOAD_FINISH` (`0x018E`, 2 bytes)** — the message
  that takes the client off the loading screen.

### 11.5 The client's own messages, and the one that moves an item

Six of the reference's dispatch cases are answered by a world that has nothing to simulate: the three load
requests above, the ping, the heartbeat, and a disconnect. The two that carry state are movement
(`0x803E`, `0x803D`, `0x8028`, `0x8047`) and the drop:

| message | header | size | layout |
|---|---|---|---|
| `DROP_ITEM` | `0x802C` | 7 | `u32 item_id, u8 count` |
| `ITEM_MOVE` | `0x8072` | 9 | `u32 item_id, u16 bag_id, u8 slot` |
| `EQUIP_ITEM` | `0x8030` | 6 | `u32 item_id` |
| `UNEQUIP_ITEM` | `0x804F` | 6 | `u8, u16, u8` — no item id, and never seen from a live client |
| `POSITION_UPDATE` | `0x803D` | 26 | `Vec2f position, u32 plane, Vec2f unk0, u32 unk1` |

`POSITION_UPDATE` is 26 bytes even though the position is in its first twelve: the framer wants the whole
message, and a port that reads twelve and stops leaves the connection waiting for a message it has already
been given.

`ITEM_MOVE` is the exact inverse of `ITEM_MOVE_TO_LOCATION` (`0x013E`) and is what a live client sent, twice,
when an item was dragged between slots. A move the server accepts is answered with `ITEM_MOVE_TO_LOCATION`
for where the item ended up — with no answer at all the item sprang back and the move did nothing — and a
refused move is answered with the same message naming the slot the item is really in, because the client has
already moved it in its own model.

`EQUIP_ITEM` carries only an item id: which slot it goes in follows from the item's type (`EquippedItemSlot`,
`GmInventory.h`). Wearing something moves the row, takes whatever was worn there off into the first free
carrying slot (and *that* move is echoed, because the client did not make it), and re-sends
`UPDATE_AGENT_VISUAL_EQUIPMENT` — the one thing the client cannot work out for itself.
`UNEQUIP_ITEM`'s three fields hold no item id, so what it means is not known — unequipping by dragging arrives
as `ITEM_MOVE`.

`DROP_ITEM` is answered by **the stack shrinking and nothing else**: the client sends `count = 1` for a stack
of a hundred, so the server takes that many off the row, writes it down, prints it, and sends no message at
all. Three versions have met a live client:

| what the server sent | what the client did |
|---|---|
| `ITEM_REMOVE`, `CREATE_NAMED_ITEM` at what was left, `ITEM_MOVE_TO_LOCATION` | crashed; in a later build the same code cloned the item |
| `WORLD_CREATE_AGENT` for an item agent only | **crashed** — the log's last line is `world -> 0x0020 (99 bytes)` |
| nothing at all | survived, and the stack went down on its own: the client does its own subtraction |

So a drop sends nothing, and there is no switch to turn the other two back on: both candidates are in no
capture, and an *item* agent never can be — the single `0x0020` a capture carries is a **player's** (type 1),
and the one code path that would produce an item agent is the reference's own unfinished drop handler. The
message a working drop needs is a measurement this project does not have yet.

### Chat: what is sent, and what is only printed

`GAME_CMSG_SEND_CHAT_MESSAGE` (`0x8064`) is `{agent_id u32, text String16}`; the first unit of the text is
the channel: `!` all, `#` group, `$` trade, `/` a command, and `@`/`%`/`"` are dropped, as the reference
drops them. A player's line is answered with two messages — `CHAT_MESSAGE_CORE` (`0x005D`) carrying the text
and `CHAT_MESSAGE_LOCAL` (`0x0061`, speaker and channel) — and that pair is **sent**, exactly as the
reference sends it: `GmChat.c:259-277` is not behind the reference's chat switch.

The answers the *server* generates are the other half, and they are printed, not sent. Every GM answer and
every "unknown command" in the reference funnels through `GameSrv_SendSystemChatMessage` or
`GameSrv_SendInvalidCommand`, and both return before touching the wire unless `GWPS_CHAT=1`
(`GmChat.c:150-153`, `GmChat.c:209-212`). So the reference's `/maps` answer is a log line; this server's
`/maps` answer is a console line; neither puts a byte on the wire. That is parity, not a switch. No chat text
has ever reached a screen in either server, and sending it here crashed a live client — `0x005D`, `0x005E`
and `0x0061` appear in no capture, so the layouts are guesses.

Nothing in this build is behind a switch (§11). The split above is what the reference does, not what this
server enables: a spoken line goes out; a generated answer is printed.

The commands, as the reference's `GmChat.c`/`GmCommands.c` define them: `/stuck` cancels the movement and
broadcasts the agent's position (`0x002C`, **not** `AGENT_STOP_MOVING`, which is what `0x8028` answers);
`/maps` prints the reference's table verbatim — `Teleport targets (N aliases, every one with a map
config):`, then one `  /tp <alias> -> map <id>  <name>` row per map — and sends nothing; `/tp <map>` and
`/teleport <map>` are **not implemented**, because they need a *world-initiated* travel (reserve a slot,
write a transfer row, send `TRANSFER_GAME_SERVER_INFO`, then `MAP_UPDATE_CURRENT`). The redirect is not
the unknown part — §5 above has its bytes, GWPS sends it on every `/tp`, and `mod.rs:59` already carries
the header and length; what this server lacks is a way for a *world* to start the cycle that §5 describes
from the client's side. An unknown command prints `Unknown command`, which is the line the reference logs.

Three answers change nothing and send nothing: item id `0`, an item that is not in the backpack/belt
pouch/bag 1/bag 2 (`GmBag_FindItem` looks no further, so storage is not droppable), and a count larger than
the stack. A move is refused when the bag is not the character's, the slot is outside it, or the slot is
occupied — and a refused move is answered with `ITEM_MOVE_TO_LOCATION` for where the item really is, because
the client has already moved it in its own model.

### 11.6 Keeping the session alive

The server sends, without being asked:

* `WORLD_SIMULATION_TICK` (`0x001E`, 6 bytes) at least every 500 ms — a client that stops receiving it
  stops moving;
* `PING_REQUEST` (`0x000C`, 2 bytes) every 5 s, and answers the client's own `PING_REQUEST` (`0x800D`)
  with `PING_REPLY` (`0x000D`, 6 bytes, `{u32 ping}`).

There is **no idle timeout** in the reference and it never disconnects over a missing heartbeat. The
only reliable disconnects are a client `DISCONNECT` (`0x8008`) and a socket error.

### 11.7 Framing on this connection

Same rule as the auth family, different table: a `u16` header (client→server headers carry `0x8000`,
server→client ones do not), and a body whose length is fixed per header except for counted fields,
which carry a `u16` count and then exactly that many units. The two families **collide on header
values** — `0x8000` is `AUTH_CMSG_HEARTBEAT` on the auth connection and `GAME_CMSG_TRADE_ACKNOWLEDGE`
on this one — which is why there are two tables and why the game lookup takes a direction.

The whole game family is generated from the reference: 194 client-to-server headers and 488
server-to-client ones, with each message's field layout
(`crates/asura-protocol/src/client/game_tables.rs`, `tools/gen-game-tables.py`).
