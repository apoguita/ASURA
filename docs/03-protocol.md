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
u16 header        message id, little endian
u16 length        total size including these four bytes
u8  body[...]     fixed layout per header
```

and the whole stream is ARC4-encrypted after the key exchange. Message sizes are fixed per header
(a table lookup, not a length prefix to be trusted); both sides size-check before parsing. GWPS
refuses and logs when a message arrives with an unexpected size.

`GWPS_TRACE=1` logs every outgoing message as header, size and the first bytes — that is the
cheapest way to compare a Rust port against the C reference.

## 2. Identifying the connection

The first message a client sends decides what the connection is. GWPS compares the header against
the auth, game and ctrl "version" headers (`AuthSrv.c`, connection classification) and routes the
socket accordingly. The auth service needs the same discrimination on its listener, and must reject anything
it does not recognise instead of guessing.

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
2. It sends them in the login message on the auth connection.
3. The auth service validates the pair, and answers with the account's character list.
4. The client selects a character; the auth service records the selection.

A login for an account that still holds a session must be resolved deterministically, not by
crashing: GWPS's rule — an incomplete transfer is cleared, a live one is refused — is the behaviour
to keep.

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

## 8. World link (the auth service ↔ world servers)

The auth service's own protocol, entirely ours, so it can be clean:

```
u32 magic          "ASUR"
u16 version        protocol version; mismatched worlds are rejected loudly
u16 kind           message kind
u32 length         body length
u8  body[length]   fixed layout per kind
```

Message kinds are listed in `docs/02-architecture.md` ("The allocation contract"). Rules:

- one connection per world, opened by the world;
- the first message must be `WorldHello`, with a protocol version the auth service accepts;
- every message is version-checked; unknown kinds are a fatal protocol error, not a warning;
- a heartbeat that stops has the same consequences as the connection dropping.

## 9. What must be re-proven in Rust

Not "ported and hoped". Each item gets a capture-based test:

1. handshake completes and the client reaches the character list;
2. login works for a fresh account, and a second login for a signed-in account behaves the same way
   GWPS does;
3. a travel request produces a redirect the client accepts, and the client reconnects;
4. the reconnect is validated, including the rejection path (wrong token → error, not silence);
5. the map update after the redirect leaves the client in the right map.
