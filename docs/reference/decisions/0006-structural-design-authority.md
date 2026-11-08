# ADR 0006 - AzerothCore is the design authority

- **Status:** Accepted (owner's direction, 2026-10-25)
- **Date:** 2026-10-25
- Affects: every architectural decision in this repository.

## Context

Three bodies of prior art are available to ASURA, and they are not equal:

- **GWPS** - our own proof of concept. It works, it taught us the client protocol, and it also has the
  ceiling we measured. It is research material.
- **Guild Wars Reforged + the stealth hooks** - the actual client, instrumented in-process. That is the
  test rig: observation and validation, not design.
- **AzerothCore** - a large, mature, proven, multi-process MMO server with a community-maintained
  structure, and we have the full source. It has already answered most of the questions we are about
  to ask, at a scale we will never reach.

## Decision

**AzerothCore is the reference implementation for design decisions.** When ASURA has a structural
question - how to split processes, where state lives, how instances are managed, how a client is handed
between servers, how the database is accessed, how a world is registered - the default answer is
whatever AzerothCore does. Deliberate deviations are permitted, but each one must be written down with
its reason, in an ADR, before it is implemented.

Corollaries, which are the parts that actually bind daily work:

1. **GWPS is research, not a target.** No porting, no patching, no "we will fix this in GWPS first"
   (`docs/reference/decisions/0005-gwps-reference-boundary.md`). It is read for protocol facts and measurements.
2. **The client rig is Reforged plus the stealth hooks.** Validation means the real client, instrumented
   (in-process context readers, packet traces, the state recorder), not a guess from source.
3. **ASURA is written from scratch.** The starting skeleton is AzerothCore's shape - authservice,
   worldserver, database domains, realm/registry - with our protocol and our client's constraints on
   top, not the other way around.
4. **A deviation is a decision, not a drift.** The known one today is instance creation: AzerothCore
   creates instances on demand, and our client is suspected to hang if the destination is not already
   running. That suspicion is an experiment scheduled in ASURA (`docs/reference/decisions/0005-gwps-reference-boundary.md`),
   not a licence to keep a different architecture.

## Correspondence between the two systems

These are structural correspondences, not a feature-completion checklist. Current programs,
libraries and implementation limits are maintained in [Architecture](../../guidance/architecture.md).

| AzerothCore | ASURA | note |
|---|---|---|
| `authserver` | auth service (`asura-auth`) | login, realm list, session key |
| `authserver`'s accept loop and `AuthSession` state machine | socket layer (`asura-gateway`) | one listener takes both client connections: the login one and, after `GAME_SERVER_INFO`, the gameplay one - `docs/guidance/architecture.md`, `docs/reference/decisions/layer-decisions.md` D26 |
| `AuthSession::HandleSubsystemPacket` / `_HandleLogonChallenge` | the auth conversation, called by the socket layer | the socket layer decides *who answers*, never *what is said* |
| `SocketHandle` passed to the world over `WorldSocket` | a second connection dialled to the world, bytes copied both ways | a deliberate deviation, below |
| `worldserver` (a *realm*) | world server (`asura-world`) | hosts maps and instances |
| `realmlist` table + realm flags | `world` registry + `world_pool` | self-registration in the database |
| Login / Character / World databases | operational PostgreSQL state and file-owned content | D20/D21/D24 replace the earlier SQL-content proposal; see [storage ownership](../../guidance/architecture.md#storage-ownership). |
| `DatabaseWorkerPool` (sync + async queue) | database access through `asura-db` | Structural direction; the async write worker is not an implementation claim. See [Inside a world server](../../guidance/architecture.md#inside-a-world-server). |
| `MapInstancer` / `MapUpdater` | instance manager + update-thread pool | prebuilt pools instead of on-demand *only if the experiment says so* |
| SRP6 world auth + session key | DH + ARC4 + player token | protocol differs; the handoff model is theirs |
| `.conf` files | server settings and selected world-pack files | D20-D24 and D33-D35 govern file ownership; [World packages](../../../config/World/README.md) describes implemented readers. |

### The one deviation these rows show

AzerothCore hands a gameplay connection from `authserver` to `worldserver` over its control channel,
which passes the socket handle. ASURA copies bytes between two sockets instead, because a handle cannot
cross a process boundary cleanly here (`crates/asura-gateway/src/relay.rs`). Three things follow, and none
of them is a second architecture. The client sees the reference's sequence, because the address it was
launched with is the one that answers both its connections. The world sees a client that dialled it
directly, so it knows nothing about the socket layer. And the transfer stays claimed by the world: the
token pair and the character are checked there, so the socket layer routes and never authorises
(`docs/reference/decisions/layer-decisions.md` D26).

## Why this rule is worth adopting rather than debating

- It removes a class of argument. "Should ASURA do X?" becomes "what does AzerothCore do for X, and if
  we differ, why?" - which is answerable in minutes and produces a record.
- It gives the project a training corpus. Anyone can read their implementation; every one of our
  decisions has a citation.
- It keeps us honest about scale. Their choices are already load-tested by a large population; ours
  would be guesses.

## What the rule does not mean

- **It does not mean copying code.** Their licence and their engines are theirs; we take structure,
  patterns and rationale, not sources.
- **It does not outrank the client.** Where our client's behaviour contradicts their model - it
  reconnects per map change with an address and a token; theirs does not - the client wins, and the
  deviation is recorded.
- **It does not freeze us.** If a decision of theirs proves wrong at our scale, we deviate and say so.
