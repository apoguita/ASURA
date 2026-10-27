# 10 — The parity work queue

The whole surface, measured, in one place. `docs/09-parity-audit.md` answers "is behaviour X
equivalent?"; this document answers "**what still has to be built, in what order, and why**".

Two rules, both learned the hard way in this repo:

1. **A gap enters this queue only with evidence** — a capture, a dispatch table, a generated diff, or
   a line of the reference's own source. "I read it and it looked finished" is how three wrong claims
   got into `docs/09` (`/tp` "in no capture", state parity, the party messages).
2. **The reference's own tests are the acceptance criteria.** GWPS publishes T0–T15 in
   `PreSearing/docs/04-scope.md`. Where a test exists, closing a queue item means passing that test,
   not "feels done".

## The yardstick: GWPS's own test plan

| # | Test (`PreSearing/docs/04-scope.md`) | ASURA today | What it needs |
|---|---|---|---|
| T0 | build and seed | **pass** (`tools/start-server.ps1`, migrations, seed) | — |
| T1 | web gate answers without a client | **pass** (same `webgate.py`, started by the launcher) | — |
| T2 | server binds | **pass** (6112/6113, world registers) | — |
| T3 | handshake past the version check | **pass** (auth reaches the EULA/character list) | — |
| T4 | account login from the DB | **pass** | — |
| T5 | EULA + character list renders | **pass** | — |
| T6 | character creation | **fail** | queue B |
| T7 | select → transfer → spawn in the map | **pass** for one client | — |
| T8 | no crash on the configured map | **pass** | — |
| T9 | a **second client sees the first** | **fail** — one agent per map, by construction | queue A |
| T10 | movement, and invalid destinations refused | **partial** — destination echoed, never validated; no path graph | queue D |
| T11 | chat displays on both clients | **equal to GWPS** — both send it, neither renders it | queue G (unachievable without a layout) |
| T12 | drop: the entity appears and disappears consistently | **partial** — the stack is deducted and persisted, nothing is sent | queue E |
| T13 | 10 minutes idle, no crash | **not verified** | queue H |
| T14 | disconnect removes the agent; a fresh login works | **partial** — the lease is released, the agent is never removed (nobody to tell) | queue A |
| T15 | log audit: every gap understood | **fail** — no generated audit until `tools/parity-emit.py` | queue H |

**Six of the sixteen tests do not pass, and exactly one of them — T6 — is on the "obvious" list.**
The others (T9, T10, T12, T14, T15) are the ones my narrative audit treated as "known partials"
instead of work.

## The four axes, as measured today

| axis | how it is measured | current state |
|---|---|---|
| client → server dispatch | `tools/parity-report.py` vs `GameSrv.c`/`AuthSrv.c` switches | auth **15/15**, game **14/18** (the four `CHAR_CREATION_*`) |
| server → client emit | `tools/parity-emit.py` | reference **81**, ASURA **67**, **14 cannot** — **9 real** gaps and 5 explained absences |
| state kept per player | `docs/09` §9, hand-checked against `GmPlayer.h:21-33` | 12 fields vs 6; `account_id` missing, which is what `/tp` needs |
| schema | migrations vs `Db.c` queries | equivalent for what is used; no `friendships` table (the stream is empty anyway) |
| the load block | **not measured** — only the item block is byte-compared | 2 messages missing on every map load (captures: 48 and 49 occurrences) |

The emit tool counts a message as emitted only when a **send path** reaches the builder: a builder
nothing calls is a gap, and so is one only a unit test calls. That rule is why the four party/leave
builders (A4) still appear in the gap list — they exist, and nothing sends them yet.

## The queue

Ordered by dependency, not by size. Every row cites its evidence.

### A. Multi-player maps — the reason half the missing messages are missing

GWPS handles **N players per map**: it broadcasts a player's arrival to everyone, tells everyone when
a player leaves, and keeps a party per player. ASURA is single-player **by construction**: `PLAYER_ID`
is a constant, `PARTY_ID` is a constant, `Play` owns one agent, and `Agents` has one slot.

That is not a small deviation — it is the source of `PLAYER_DESTROY`, `WORLD_REMOVE_AGENT`,
`PLAYER_UPDATE_PARTY`, `PLAYER_UPDATE_PARTY_SIZE` and the `player_block`'s missing pair (T9, T12
consistency, T14). Evidence: `tools/parity-emit.py` output; `GmPlayer.c:314`, `GmAgent.c:60`,
`GmPlayer.c:322`, `GmPlayer.c:346`; capture `zone-ref-005829.jsonl` seq 254-259.

| # | item | done when |
|---|---|---|
| A1 | an agent table per map instead of `PLAYER_ID`; each connection owns its agent | two connections hold two agents in one map |
| A2 | a joining player is broadcast (`PLAYER_CREATE`, `WORLD_CREATE_AGENT`) to the others | T9: client two sees client one |
| A3 | a leaving player is withdrawn (`PLAYER_DESTROY`, `WORLD_REMOVE_AGENT`) | T14: the other client's world loses the agent |
| A4 | per-player party messages (`PLAYER_UPDATE_PARTY_SIZE`, `PLAYER_UPDATE_PARTY`) in the load block | the load block matches the capture's sequence |
| A5 | movement of one player is broadcast to the rest (`AGENT_MOVE_TO_POINT`, `AGENT_UPDATE_POSITION`) | T9: both see the other walk |

### B. Character creation (T6)

Four handlers plus a map type this world refuses to host. Evidence: `GameSrv.c:1691-1976`,
`GetBagSlotForItemType` (`GameSrv.c:1741`), `GmDefaultArmors.c` (2858 lines of data),
`CHAR_CREATION_START/SUCCESS/ERROR` in the emit diff.

| # | item | done when |
|---|---|---|
| B1 | host the character-creation instance (`MapType_CharacterCreation`) | a fresh account reaches the creation screen |
| B2 | `CHAR_CREATION_REQUEST_PLAYER` + `_REQUEST_ARMORS` (the player-data and PvP blocks) | no `Unhandled GameSrv packet` in the log |
| B3 | `_CHANGE_PROF` with the default-equipment table ported from `GmDefaultArmors.c` | picking a profession swaps the equipped items |
| B4 | `_CONFIRM`: write `characters`, `bags`, `items` rows | the same three `count(*)` that T6 prints |

### C. Teleport (`/tp`, `/teleport`) — measured, not blocked

The redirect is known: `TRANSFER_GAME_SERVER_INFO` (`0x01A6`, 46 bytes as GWPS sends it) then
`MAP_UPDATE_CURRENT` (`0x0099`, 12 bytes), both in `captures/zone-ref-005829.jsonl`. What is missing is
*authority*: only the client can start a travel today. Evidence: `docs/09` §4, `GameSrv.c:1360-1435`,
`docs/08-gm-commands.md`.

| # | item | done when |
|---|---|---|
| C1 | keep `account_id` (and the session) on the connection — state parity, `docs/09` §9 | the world can write a transfer row at all |
| C2 | a world-side `reserve` + `begin_transfer` for the target map | a transfer row appears with the right token and map |
| C3 | send `0x01A6` + `0x0099`; mark the connection *moved* so its teardown does not release the new lease | `/tp lakeside → /tp ascalon` lands in Ascalon and the seat accounting is right |
| C4 | alias resolution and the reference's three refusals (unknown, no config, already here) | `/tp nonsense` prints the reference's line and sends nothing |

### D. Movement: validation, not just echo (T10)

ASURA echoes the client's destination, so the client's own prediction carries the walk and a wall is
not a wall. GWPS pathfinds (`GmPaths.c`, A* over the map's path graph from `Gw.dat`) and refuses what
it cannot reach. Evidence: `GmAgent.c:371-398`, `GmAgent.c:409-462`.

| # | item | done when |
|---|---|---|
| D1 | a position budget on `MOVE_TO_COORD`, not only on `POSITION_UPDATE` | a move far beyond walking distance is refused |
| D2 | the path graph, or a written decision not to port it | T10's third clause is either met or recorded as unported with the reason |

### E. Item lifecycle (T12)

`CREATE_UNNAMED_ITEM` cannot be emitted; the ground item cannot be sent; nothing picks anything up
(GWPS has no pick-up handler either).

| # | item | done when |
|---|---|---|
| E1 | decide the ground item on evidence: GWPS's own handler is unfinished, so the capture must come from live play or the item stays a log line (already recorded) | a written decision, with the capture if one exists |
| E2 | `CREATE_UNNAMED_ITEM` for the paths that need it (creation, drops of generated items) | the emit diff stops naming it |

### F. The load block, byte-compared (T8, T15)

Only the item block is compared against a capture today. The player block is not — which is how two
missing party messages survived an audit that called the block equivalent.

| # | item | done when |
|---|---|---|
| F1 | a generated fixture from the capture (headers + sizes only, no names or UUIDs) | a test that fails when a message is dropped from the block |
| F2 | reconcile the block with the fixture | T15's audit is clean for the load |

### G. Chat (T11) — equal to the reference, and that is the ceiling

Both servers send a player's line and print a generated answer; neither has ever rendered chat on
screen. `0x005D`/`0x005E`/`0x0061` appear in no capture.

| # | item | done when |
|---|---|---|
| G1 | none, unless a capture of a working chat message appears | recorded, not worked |

### H. Operations parity (T13, T15)

| # | item | done when |
|---|---|---|
| H1 | two clients idle for 10 minutes without a crash or a leak (T13) | the test is run and logged |
| H2 | the generated audits run in `tools/test-battery.ps1` (dispatch, emit, state) and their output is in `docs/07-status.md` | a regression in any axis fails the battery |

## What is deliberately *not* in the queue

Recorded so nobody adds it later: matching GWPS where GWPS is unfinished or does not fit this design.

- **GWPS's drop handler** — `TODO` for partial stacks, item never removed, model id set to the item id.
- **GWPS's server-side chat** — gated off because it crashed its client twice.
- **`GmParty.c` hero support** (`PARTY_HERO_ADD`) — no heroes exist here; the captures carry it 0 times.
- **Download manifest data** (`INSTANCE_MANIFEST_DATA`) — 0 times in the captures; nothing to download.
- **Campaigns, PvP, guild halls** — the reference's own `X — Out of scope` tier.

## Order of work

1. **A1 + A2 + A3 + A4 + A5** — multi-player is what most of the missing surface is *for*; it also
   makes T9, T12 and T14 measurable for the first time.
2. **C1 → C4** — `/tp` is measured end to end and unblocks the second map without a rebuild.
3. **F1 → F2** — the load block becomes provable, which is what keeps the rest honest.
4. **B1 → B4** — character creation, the largest data port.
5. **D1** → **E1/E2** → **H1/H2**.

Each step lands with its test, and the audit documents are regenerated rather than edited:
`python tools/parity-report.py --write docs/07-status.md`, `python tools/parity-emit.py`.
