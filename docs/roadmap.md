# Roadmap

Milestones in the order they can be tested. Each one ends with something a real client, a test
harness, or a measurement can prove — not with "the code compiles".

## M0 — Specification

This repository's documents, and the decisions they depend on.

**Done when:** `docs/01-scope.md`, `docs/02-architecture.md`, `docs/03-protocol.md`,
`docs/04-from-gwps.md`, the ADRs, and this roadmap are complete enough to answer two questions
without asking the author: *what does the auth service do?* and *what does a world server decide?*

**Test:** a reader who has never seen GWPS can describe the allocation flow and the one rule.

## M1 — Cargo workspace, no networking

The skeleton follows AzerothCore's shape (auth service, world server, database domains, registry),
not GWPS's single-process layout.

Three crates, no sockets yet:

- `asura` — the binary (config loading, logging, startup order, shutdown).
- `asura-db` — the schema, migrations, and the queries both programs need.
- `asura-db` — schema ownership and the queries the auth service needs.

**Done when:** every world-link message round-trips byte-for-byte in a unit test, the protocol
version mismatch path is tested, and `asura --version` starts and exits cleanly.

**Test:** `cargo test`. Nothing depends on a client.

## M2 — Schema, registry, and reservations against a fake world

Schema first, then coordination through it: world servers register in `world`, publish pool capacity
in `world_pool`, and travel decisions reserve slots atomically.

**Done when:** a test harness (a "fake world" binary in the workspace) can register, heartbeat, be
marked stale when it goes silent, and have its leases reaped. Reservation is proven atomic under
concurrency: with a pool of capacity N-1, N parallel reservations succeed exactly N-1 times.

**Test:** harness-only, fully automated, no game client involved. The concurrency case runs the
reservation from many tasks at once.

**Delivered.** `asura-fakeworld` is the harness; `crates/asura-db/src/placement.rs` holds the queries;
`crates/asura-db/tests/placement.rs` proves the race, the refusal, the reaper and the reconciler
against the live database; `tools/m2-verify.ps1` runs both in one command. A `lease` ledger sits
beside `world_pool` so a lease can be identified, reaped and reconciled (`docs/02-architecture.md`).
The suite runs repeatedly without flakiness — it is the first thing to re-run after any change to
placement.

## M3 — The auth endpoint (the hard one)

Port the handshake and login: framing, ARC4, DH with the fixed constants, login against the web
gate, character list, character select.

**Done when:** the patched `Gw.custom.exe` reaches the character list against the auth service, and the
handshake produces traffic that diffs clean against the GWPS reference for the same actions.

**Test:** real client, login only — no worlds needed yet. Diff with
`PreSearing/tools/diff-transitions.py` and the `GWPS_TRACE` logs as the reference side.

**Risk:** the highest in the project. Failure is an opaque client error (`007`), so keep the packet
trace and compare every message. This is why the reference implementation stays runnable.

**Delivered 2026-10-26.** The patched client (build 39112) logged in against this service, took the
EULA and reached character select. `docs/06-reference-session.md` records the whole session from both
servers — every direction, header and size — and `asura-auth` reproduced it line for line up to the
client's travel request, which it refuses because no world server exists: `REQUEST_GAME_INSTANCE`
(`0x8029`) needs `GAME_SERVER_INFO` (`0x0009`, 42 bytes: address, port, tokens) and then a second
connection speaking the game protocol, which is M4.

The pieces behind that: connection classification from the first four bytes; framing by walking each
message's field layout (u16 counts, no padding — the wire is variable-length in **both** directions,
which the table's sizes do not describe; see `docs/03-protocol.md` §1); the full auth message table
generated out of the GWPS reference; the key exchange (ARC4, the fixed-constant Diffie–Hellman, the
protocol's own key derivation, pinned by fixtures **compiled from the reference's own C**,
`docs/adr/0009-crypto-primitives.md`); the file check; typed layouts for every message on the login
path, including the 591-byte settings blob and the 62-byte character settings block copied from a live
reply; the login decision against the database; and the listener that puts them together with a
per-message trace behind `ASURA_TRACE=1`. `crates/asura-auth/tests/login.rs` proves it over a socket
without a game, `tools/m3-verify.ps1` runs the whole path in one command, and the comparison tools
(`probe`, `tools/compare-replies.py`, `tools/inspect-reply.py`) are what settled the shapes when
reading the reference was not enough.

## M4 — End-to-end travel

The auth service allocates, a world grants an instance, the client is redirected to that world's own address
and reconnects with the token.

**Depends on:** GWPS step 1 — a world server that accepts and authenticates its own gameplay
connections (`docs/04-from-gwps.md`, step 1).

**Done when:** one C world server, one the auth service, one client: log in → enter a map → travel to another
map, and the client's world rebuilds (the in-client recorder should show a tear-down followed by a
rebuild, as it does on GWPS: 100–204 ms).

**Test:** real client, plus the client-state recorder to prove the client actually settled.

## M5 — Capacity, refusal, and persistence

The auth service sizes the world, refuses when full, and owns player state.

**Done when:** with one explorable slot configured, a second character travelling there is refused
*while still standing in the outpost* — the client never enters a loading screen. Player state is
written on leaving an instance and read back on entering.

**Test:** two clients, or one client and a harness that holds the slot.

## M6 — More than one machine

A world on a second host, reachable because step 1 made the client-facing address real.

**Done when:** a hop takes a character from a world on host A to a world on host B, and back, with
nothing but a longer loading screen to show for it.

**Test:** real client, two hosts.

## M6.5 — The instance-creation experiment

The one question we cannot answer from GWPS without more work there, so it is answered here
(`docs/adr/0005-gwps-reference-only.md`):

- Default: a travel is answered only with an already-running instance (prebuilt pool).
- Test: create the instance during the travel, redirect only after the world reports the instance
  ready, and watch the real client with the recorder. Rebuilt world = on-demand creation is
  acceptable; torn down and never rebuilt = the pool requirement is real, and we have a reproduction
  in our own code instead of an inference from a confounded one.

**Done when:** the answer is recorded with the client state that proves it, either way.

## Phase 2 - simulation (deferred, not cancelled)

These begin only when phase 1 works end to end. They are listed here so that phase-1 decisions are
made knowing what is coming.

| milestone | outcome |
|---|---|
| S1 | decide the simulation's language and structure: port the C world server to Rust, or adopt it as a C component owned by this project. This is a decision, not an accident |
| S2 | the world server builds and runs from this repository, with its own maps and player load/save |
| S3 | the simulation reads what it needs from the database through `asura-db`, with no shortcuts and no private SQL |
| S4 | instance teardown: an empty explorable is cleared and returned to its pool, and memory does not grow across a session |

## After M6

- Districts as capacity (a town that is full gains a second instance) — same machinery as
  explorable pools, different policy.
- Chat across worlds, if it is ever wanted.
- Observability worth keeping: per-world instance/player/RSS/tick metrics, exported and logged.

## The tests that must exist before each milestone is called done

| milestone | automated | real client | measurement |
|---|---|---|---|
| M1 | unit round-trips, version mismatch | — | — |
| M2 | harness register/heartbeat/stale/refuse | — | — |
| M3 | message table matches the reference (`gen-auth-tables.py --check`), key exchange matches fixtures compiled from the reference's C, framing round-trips, identity-pair refusals, and a client logging in over a socket (`asura-auth/tests/login.rs`) | login to character list | packet diff vs GWPS |
| M4 | — | hop, and client state settles | tear-down/rebuilt, memory |
| M5 | — | refusal while in outpost | saved state round-trip |
| M6 | — | cross-host hop | latency |
