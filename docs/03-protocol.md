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

The client and server perform a Diffie–Hellman exchange with **fixed constants**:

- a 256-bit prime modulus, and a server private exponent — both hardcoded in GWPS `main.c`;
- the client sends its public value in the client-seed message;
- the shared secret is `client_public ^ server_private mod prime`;
- both sides derive the ARC4 key from the shared secret, and everything after that is encrypted.

The patched client (`patch-gw.py`) is what makes this work at all: it carries the matching public
parameters and disables a mutex check. **The constants must be identical** in the auth service, or the client
will fail the handshake and report an opaque error. This is the highest-risk part of the port and
the reason the handshake is scheduled last (`docs/roadmap.md`).

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

## 5. Travel: the request/redirect cycle

This is the part the whole design exists to protect.

1. **Client asks to travel.** On the game connection, the client sends the district it wants:
   `{map_id, map_type, region, language, district}` with a request id. In GWPS this arrives as
   `CTRL_CMSG`-shaped `StartPlayerTransfer` from the world, and as a direct request message on the
   auth connection at login; both are allocation requests.
2. **the auth service allocates.** It answers only from a world that has the map ready (`docs/02-architecture.md`).
3. **the auth service issues a token.** A fresh player token plus the identity the client's *next* connection
   must present.
4. **The client is redirected.** The response is `TRANSFER_GAME_SERVER_INFO` (`0x01A6`):
   `{host[24], port, map_token, region, player_token, is_explorable, map_id}` — field order as
   GWPS sends it; the captured live shape is 52 bytes where GWPS sends 46, and GWPS's 46-byte form
   is accepted by the client, so the difference is padding rather than semantics. **Do not
   reorder fields**: GWPS's order deliberately differs from the capture and works.
   - `host` is a 24-byte string. GWPS always writes `127.0.0.1`; the auth service must write a real address.
   - `is_explorable` describes the **destination** map type (1 for explorables, 0 for outposts).
     Deriving it from the source map was a bug that produced client error 005.
5. **A map update follows immediately.** `MAP_UPDATE_CURRENT` (`0x0099`) is sent right after the
   redirect — in live captures the two appear in the same millisecond. 12 bytes:
   `header, 0, map_id, 0`. See the size trap below.
6. **The client reconnects.** It drops the old connection and opens a new one to the address it was
   given, presenting the token. That reconnect is accepted only if it matches the recorded identity;
   a mismatch is client error 007.

## 6. Traps that cost real time in GWPS

| trap | what happens | rule |
|---|---|---|
| `sizeof(struct)` taken as the wire size | two extra bytes on the wire, so the client misreads every message after it | the size is the format's field sum, not the struct's size. `AuthSrv_SessionInfo` is 14 bytes in C and 10 on the wire — the format has two dwords where the struct has three |
| a length field written where the protocol has none | the client misreads the body as the next header: error `007` | there is no length field (`section 1`) |
| `0x0099` size | 12 bytes in the login batch makes the client fail with 007; the same message is 12 bytes (5 in the login batch) | size per context, not per header |
| `is_explorable` from the wrong map | error 005 after travel | always from the destination district |
| settings upload not acknowledged | hang correlated 7/7 with unacknowledged uploads | always answer `AUTH_CMSG_SETTING_UPDATE` with a request response |
| a transfer replayed from a stale queue | client lands in the previous destination | clear the queue after completing one |
| constructing a server during the hop | loading screen forever, zero packets after | the one rule, `docs/02-architecture.md` |
| chart commands sent before the client allows chat | client crash | gate server-generated chat on the client's readiness |

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

- **`AUTH_SMSG_0026` (`0x001A`) declares 32 bytes where its own layout can only occupy 31.** It is a
  `TYPE_NESTED_STRUCT` message, whose length depends on a count carried on the wire, and the
  declared maximum is one byte more than `1 + count * 2` can produce. GWPS would therefore consume
  31 bytes for it while a table-driven framer consumes 32, so a client that sends it would
  desynchronise this port and not GWPS. It is an unnamed friend/social message outside the login
  path, so it is recorded rather than guessed at; `crates/asura-protocol/src/client/tables.rs` lists
  it in `VARIABLE_LENGTH_HEADERS`. Resolve it when something needs the message.
