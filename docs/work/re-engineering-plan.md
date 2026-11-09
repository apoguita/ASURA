# Re-engineering plan: AzerothCore's skeleton, this client's front door

Status: proposed, 2026-11-08. Owner's direction: additive, retro-compatible, step by step, and every
step works with the whole project before the next one starts. This document is the plan and the
statement of the target. It is the single place that describes what the server is meant to become;
each other document is updated only when a phase makes its own sentence true.

## Why this plan exists

The tree works, but it fuses the two things AzerothCore keeps apart. One process answers the login
conversation and relays gameplay; one database holds identity, character state and content; one
configuration directory and one log serve everything. The structural elements of the reference are
recorded in `docs/guidance/architecture.md:59-74`, and the two places where they were explicitly not
built are the store split and the bounded parallel map update (`architecture.md:260-264`). The
documentation described that fused code, so each document was accurate about a wrong skeleton, which
is how the documents came to contradict each other.

## The target composition

Two doors, one per conversation, each a program with its own listener, configuration, store access
and log. The client walks in through the login door, is told the world's own address, and walks in
through the game door.

| part | what it is | owns |
|---|---|---|
| login door | `asura-gateway`'s auth half plus `asura-auth` | file check, version, key exchange, account, character list, select, redirect |
| world door | `asura-world`'s listener and everything behind it | the gameplay connection, maps, instances, players |
| registry | the database row per world | which world serves a map, its capacity, its freshness |
| identity store | accounts, credentials, sessions | who this is |
| state store | characters, items, leases, transfers | what happened to whom |
| content | `config/World/<pack>/` files | what the world is made of |

Deliberate deviations from AzerothCore, each already decided: the client asks a portal web service who
it is before login; it begins with a file check and a version exchange; content is JSON files rather
than SQL (D20, D21, D24); placement carries a player token. Unresolved and not decided here: whether
the token survives, whether the login door keeps a relay switch, and how a script dispatcher reaches
mechanics.

## The rules for every phase

1. **Additive.** The new structure is built beside the old one and callers move onto it; the old path
   is deleted only when nothing uses it.
2. **The packet stream is frozen.** No phase changes a byte a client receives. Migrations may only
   add. Configuration files may only gain keys.
3. **One phase, one commit, green.** Build succeeds, unit tests pass, and the real client logs in, is
   redirected, and enters a map with byte-identical frames. No phase carries a feature.
4. **A phase that cannot be built beside what runs is split again** rather than forced.

## The phases

| phase | what lands | what proves it | documents updated with it |
|---|---|---|---|
| 0 | this plan | you read it | this file; one pointer line in `docs/README.md` and the handoff |
| 1 | two doors: the login door becomes its own program with its own config file and log; the world door accepts the client directly; the redirect carries the world's address; the relay stays behind a switch for one phase | real client: login, redirect into the world's own address, in-world; frames identical; the switch flipped both ways | `operations/running-the-server.md`, `config/Server/README.md`, `guidance/architecture.md` (today), `work/project-status.md` |
| 2 | three stores by owner: identity, state, content stop sharing one access path; one interface per store | login, travel, item persistence, a full world refused cleanly; migrations unchanged | `config/Server/README.md`, the storage section of `guidance/architecture.md` |
| 3 | placement as one responsibility: four operations - find worlds, reserve, release, record a transfer - and both doors call them | travel in both directions; a full world refused without disturbing the holder | `guidance/architecture.md`, the entry and travel rows of `reference/game-world-model.md` |
| 4 | the world's inside split: listener, sessions, simulation, mechanics as separate files with private data; the reference's bounded parallel map update replaces one task per map | real client in-world; ping and tick cadence measured; two clients see each other | `work/layer-rework-checklist.md`, `work/world-simulation-plan.md` |
| 5 | hosting: per-daemon configuration and logs; the supervisor owns start, stop, deploy, health and nothing about players | supervisor start path; a draining deploy with a player present | `operations/running-the-server.md`, `operations/running-on-linux.md`, `config/*/README.md` |
| 6 | game sealed on content: mechanics read values from files only; behaviour reaches them through a dispatcher in AzerothCore's script-manager position | a JSON edit changes behaviour after restart, with no rebuild | `guidance/scripting-and-behavior.md`, `work/world-simulation-plan.md`, `guidance/game-packs.md` |

## How the documents change

Every document is one of three kinds, and its first lines say which:

- **A dated record** - status, handoff, research, history, recordings. Frozen with its date; a wrong
  sentence is replaced, never appended beside.
- **A statement of today** - the runbook, the storage section, the component inventory. Updated in the
  same commit as the phase that changes it, so it is never half-migrated.
- **A statement of a target** - this plan. It describes what is being built, not what runs.

A document that describes today is not edited to describe intent; that is the mistake that produced
the contradictions this plan replaces.

## Open questions, reported and not decided here

- Does the player token survive, or does the world validate a session the way AzerothCore does?
- Does the login door keep a relay path for the client's second connection, and for how long?
- Which script dispatcher reaches mechanics, and when it is introduced.
- The bounded update pool's size and its map affinity.
