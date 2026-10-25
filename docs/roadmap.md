# Roadmap

Milestones in the order they can be tested. Each one ends with something a real client, a test
harness, or a measurement can prove — not with "the code compiles".

## M0 — Specification

This repository's documents, and the decisions they depend on.

**Done when:** `docs/01-scope.md`, `docs/02-architecture.md`, `docs/03-protocol.md`,
`docs/04-from-gwps.md`, the ADRs, and this roadmap are complete enough to answer two questions
without asking the author: *what does ASURA do?* and *what crosses the world link?*

**Test:** a reader who has never seen GWPS can describe the allocation flow and the one rule.

## M1 — Cargo workspace, no networking

Three crates, no sockets yet:

- `asura` — the binary (config loading, logging, startup order, shutdown).
- `asura-protocol` — message layouts for the world link, with encode/decode and a protocol version.
- `asura-db` — schema ownership and the queries ASURA needs.

**Done when:** every world-link message round-trips byte-for-byte in a unit test, the protocol
version mismatch path is tested, and `asura --version` starts and exits cleanly.

**Test:** `cargo test`. Nothing depends on a client.

## M2 — World link against a fake world

ASURA listens for world servers, maintains the directory, and tracks liveness.

**Done when:** a test harness (a "fake world" binary in the workspace) can register, report
`MapReady`, heartbeat, and be marked stale when it goes silent. An allocation request against an
empty directory returns a refusal, not a panic.

**Test:** harness-only, fully automated. No game client involved.

## M3 — The auth endpoint (the hard one)

Port the handshake and login: framing, ARC4, DH with the fixed constants, login against the web
gate, character list, character select.

**Done when:** the patched `Gw.custom.exe` reaches the character list against ASURA, and the
handshake produces traffic that diffs clean against the GWPS reference for the same actions.

**Test:** real client, login only — no worlds needed yet. Diff with
`PreSearing/tools/diff-transitions.py` and the `GWPS_TRACE` logs as the reference side.

**Risk:** the highest in the project. Failure is an opaque client error (`007`), so keep the packet
trace and compare every message. This is why the reference implementation stays runnable.

## M4 — End-to-end travel

ASURA allocates, a world grants an instance, the client is redirected to that world's own address
and reconnects with the token.

**Depends on:** GWPS step 1 — a world server that accepts and authenticates its own gameplay
connections (`docs/04-from-gwps.md`, step 1).

**Done when:** one C world server, one ASURA, one client: log in → enter a map → travel to another
map, and the client's world rebuilds (the in-client recorder should show a tear-down followed by a
rebuild, as it does on GWPS: 100–204 ms).

**Test:** real client, plus the client-state recorder to prove the client actually settled.

## M5 — Capacity, refusal, and persistence

ASURA sizes the world, refuses when full, and owns player state.

**Done when:** with one explorable slot configured, a second character travelling there is refused
*while still standing in the outpost* — the client never enters a loading screen. Player state is
written on leaving an instance and read back on entering.

**Test:** two clients, or one client and a harness that holds the slot.

## M6 — More than one machine

A world on a second host, reachable because step 1 made the client-facing address real.

**Done when:** a hop takes a character from a world on host A to a world on host B, and back, with
nothing but a longer loading screen to show for it.

**Test:** real client, two hosts.

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
| M3 | — | login to character list | packet diff vs GWPS |
| M4 | — | hop, and client state settles | tear-down/rebuilt, memory |
| M5 | — | refusal while in outpost | saved state round-trip |
| M6 | — | cross-host hop | latency |
