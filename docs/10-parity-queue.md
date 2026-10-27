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
| T6 | character creation | **pass on the server side**: the four handlers, the rows, and the success message (_character_is_created_in_a_creation_instance) — a live creation screen is unverified | queue B is done on the wire |
| T7 | select → transfer → spawn in the map | **pass** for one client | — |
| T8 | no crash on the configured map | **pass** | — |
| T9 | a **second client sees the first** | **server side passes**: two agents, each introduced to the other, and each told when one leaves (`a_second_player_is_introduced_to_the_first_and_withdrawn_again`) — a live second client is unverified | queue A is done on the wire |
| T10 | movement, and invalid destinations refused | **the deviation is now asserted, not assumed**: `a_move_is_echoed_as_sent_and_a_far_report_is_clamped` pins that a walk is answered with exactly the destination sent (no path graph, so no waypoint and no refusal) and that the reference's own *position* budget is enforced on the wire (an out-of-budget report is clamped and the clamp is what the other player sees) | queue D, sized below |
| T11 | chat displays on both clients | **equal to GWPS** — both send it, neither renders it | queue G (unachievable without a layout) |
| T12 | drop: the entity appears and disappears consistently | **partial** — the stack is deducted and persisted, nothing is sent | queue E |
| T13 | 10 minutes idle, no crash | **rhythm and accounting pass at a scaled window** (`an_idle_pair_keeps_its_rhythm_and_its_count`: the 500 ms tick and 5 s ping keep arriving, an idle client is told nothing else, and the world's player list stays a pair and empties on departure); the ten-minute variant exists as an `#[ignore]`d test and the memory clause stays part of that hand run | queue H is down to the memory clause |
| T14 | disconnect removes the agent; a fresh login works | **server side passes**: the farewell pair is sent and asserted | queue A is done on the wire |
| T15 | log audit: every gap understood | **fail** — no generated audit until `tools/parity-emit.py` | queue H |

**What is left: T10 (movement is not validated), T12 (the ground item), T13 (the idle run is unrun) and the
live half of T9 and T14.** T6 and T15 have moved: T6 passes on the server side, and T15 is now two
generated audits in the battery.

## The four axes, as measured today

| axis | how it is measured | current state |
|---|---|---|
| client → server dispatch | `tools/parity-report.py` vs `GameSrv.c`/`AuthSrv.c` switches | **complete**: auth **15/15**, game **18/18**; the battery reports "nothing missing" |
| server → client emit | `tools/parity-emit.py` | **complete for every message the reference sends**: 76 of 81 reachable, **0 real gaps**, 5 explained absences |
| state kept per player | `docs/09` §9, hand-checked against `GmPlayer.h:21-33` and `DbCharacterColumnsDef` | the four traits a message reads are stored (migration 0008) and the block sends them instead of constants; the rest of the reference's ~44 character columns are queue item **B5**, listed with the reason |
| schema | migrations vs `Db.c` queries | equivalent for what is used; no `friendships` table (the stream is empty anyway) |
| the map catalogue | `tools/gen-map-config.py` against the reference's three tables | **12 maps, the reference's own list**: aliases and `MapType` from its teleport table (`GmCommands.c:32-53`), numbers from `GmMaps.h`, file ids and spawns from `GmMapsConfig.c`. The seed used to carry two hand-typed maps |
| the load block | `tools/parity-load.py` against the reference's own `GWPS_TRACE=1` log, as a generated fixture (`crates/asura-world/src/load_fixture.rs`) | **measured: 480 messages, 0 missing, 0 extra, 0 size disagreements** — it matches message for message |

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

**A1–A5 landed.** Each connection now allocates its own player and agent id instead of using the
constants, a `Peers` registry holds everyone in the map with a channel per connection, a joining player
is introduced to the others and they to it in `GameSrv_SendWorldAgents`'s order, a leaving one is
withdrawn with the farewell pair, moves and position reports are broadcast, and the test
`a_second_player_is_introduced_to_the_first_and_withdrawn_again` covers all of it. `PLAYER_ID`/`AGENT_ID`
remain only as the ids the first player gets.

The four messages it needed turned out to be wrong in the **generated table**, not just missing: the
client's own table says 5, 6, 6 and 6 bytes for them while every live capture says 12, 12, 8 and 8,
because they carry a four-byte game header (`TYPE_MSG_HEADER_GAME`, `msgdefs.h:19-23`). The generator
now applies that per message, with the captures named in `OVERRIDES`.

| # | item | state |
|---|---|---|
| A1 | an agent table per map instead of `PLAYER_ID`; each connection owns its agent | done |
| A2 | a joining player is broadcast (`PLAYER_CREATE`, `WORLD_CREATE_AGENT`) to the others | done |
| A3 | a leaving player is withdrawn (`PLAYER_DESTROY`, `WORLD_REMOVE_AGENT`) | done |
| A4 | per-player party messages (`PLAYER_UPDATE_PARTY_SIZE`, `PLAYER_UPDATE_PARTY`) | done |
| A5 | movement of one player is broadcast to the rest | done |

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

**Half of B landed, measured.** The *data* is ported and generated: `tools/gen-default-equipment.py`
reads `GmDefaultArmors.c` and writes `crates/asura-world/src/default_equipment.rs` — **32 (campaign,
profession) rows, 179 items**, the field set being the reference's own `GmItem` literal, which is also what
`CREATE_NAMED_ITEM` writes. `every_profession_starts_with_the_references_own_equipment` pins a Prophecies
Warrior's chest piece to the reference's bytes, and the battery re-generates with `--check`.

The *messages* are built too: `char_creation_start` (2 bytes), `char_creation_error` (6, header plus the
reference's `GM_ERROR_CHARACTER_NAME_ALREADY_EXIST` slot), `char_creation_success` (uuid + counted name +
the reference's own `0x2211` + counted settings) and `instance_player_data_done` (2), all sized by the
table and asserted in `the_fixed_size_messages_match_the_size_the_reference_sends`.

What remains for B is the flow itself: hosting map type 11, the four handlers, and the confirm writing the
rows — which is also what closes the last three emit gaps, because a builder with no call site is still a
gap to `tools/parity-emit.py`.

### B5. The rest of the character row (~40 columns)

The reference's `DbCharacter` (`DbSchema.h:66-110`) stores about forty-four columns per character. This
server stores the ones a message reads *today* — appearance, primary profession, unlocked professions, gold
(migration `0008_character_traits.sql`) — and nothing else, deliberately: a column nothing loads is the same
liberty in a new place.

**Most of what is left is *not* work, and that was measured rather than assumed.** Checking the columns
against what the reference actually sends split the list in two:

| column(s) | the reference's own behaviour | verdict |
|---|---|---|
| `skill1`..`skill8` | `GameSrv_SendSkillbarUpdate` sends them from the row, and `HandleCharCreationConfirm` builds that row with `= {0}` and never assigns a skill — nothing in GWPS writes one | zeros are parity |
| `unlocked_skills` | `GameSrv_SendUpdateUnlockedSkills` sends the bitmap, which is written as all-zero by the same confirm and assigned nowhere | an empty list is parity |
| skill/attribute points | `GameSrv_SendAgentAttributePoints` **hardcodes** unused 50 / used 50 | the constant is parity |
| the attribute spread | `GameSrv_SendPlayerAttributes` sends `data_len = Attribute_Count` and no values at all | the zeros are parity |
| `experience`, `skill_points`, `campaign`, `helm_status`, `is_pvp`, `secondary_profession`, the completion bitmaps | stored, never read into a message that exists | no observable difference |
| the per-slot armour `file_id_*`/`colors_*` | the reference denormalises what it already has in the equipped bag | this server derives the same answer from the items |

What *was* wrong surfaced through the same reading: the character-creation block was sending the **load**
path's health and energy (25, 400 and a regen float) where the reference's creation path sends its own three
int properties — energy 20, health 100, property 64 zero (`GameSrv_SendPlayerHealthEnergy`,
`GmPlayer.c:178-199`). Fixed, and asserted: the creation test now checks those three values and the absence of
a float property.

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

**C1–C4 landed.** `Play` carries the session and account, `/tp <alias>` (and `/teleport`) reserves the
destination's seat, mints a token, writes the transfer row and sends the pair; the connection is marked
*moved* so its teardown leaves the destination's seat alone. The test
`a_teleport_redirects_to_the_target_map_and_keeps_its_seat` drives the whole cycle: two refusals that send
nothing, the 46-byte redirect and 12-byte map update read by hand (the table's 5-byte form is the *login*
shape — the reference's "size per context" trap), the seat moving from one map to the other, a reconnect
with the handed-out token landing at the destination's own spawn point, and the old connection's exit
leaving the seat taken.

The one thing that is still this server's: it never constructs another world, so `/tp` reaches only the
maps it already hosts. The reference's auth service can place a player on any pre-spawned map server.

### D. Movement: validation, not just echo (T10)

ASURA echoes the client's destination, so the client's own prediction carries the walk and a wall is
not a wall. GWPS pathfinds (`GmPaths.c`, A* over the map's path graph from `Gw.dat`) and refuses what
it cannot reach. Evidence: `GmAgent.c:371-398`, `GmAgent.c:409-462`.

**D is sized, not started — and the sizing says it is feasible.** The reference's refusal is its pathfinder,
and the pathfinder is three units of C, all readable, plus the archive they read from:

| part | lines | what it is |
|---|---|---|
| `FaArchive.c` + `.h` | 204 + 72 | the `Gw.dat` archive: seek, read, MFT lookup by hash, and `FaDecompress` for compressed entries |
| `GmMapDataImport.c` + `.h` | 590 + 43 | the graph's chunk format: planes, trapezoids, vectors, root/X/Y/sink nodes, portal traps |
| `GmPaths.c` | 1,057 | trapezoid A*: `PathFindTrapezoid`, portal walking, waypoint reduction |

And the two things a port needs to be verified against both exist:

- **the data**: `Gw.dat` is present beside the reference at 3,655,804,520 bytes;
- **an oracle**: the reference ships built tools (`build/RelWithDebInfo/gwdat_pathing.exe`,
  `gwdat_scan.exe`) that dump the graph, so a ported reader can be compared message for message with the
  reference's own output rather than with a guess.

So `MOVE_TO_COORD` cannot validate anything *today*, because none of those three units is present — but this is
a **milestone**, not an unknown and not a missing measurement. The smallest useful first step is
`FaArchive` alone: 204 lines, no dependence on the graph format, and it can be tested against
`gwdat_pathing.exe`'s dump of one entry before any A* is written.

What *can* be matched without it is matched, and asserted: the reference's position budget
(`speed_base * elapsed + tolerance`, clamped at 2 s of elapsed time) is the same rule on the same message, and
`a_move_is_echoed_as_sent_and_a_far_report_is_clamped` says so on the wire.

**D1 landed: the archive index.** `crates/asura-world/src/gwdat.rs` is the index half of `FaArchive.c` —
header, file table, TOC hash, entry lookup, and the stored bytes of one entry. No decompressor yet, so it
answers "is this file id in the archive and where", which is what the map catalogue depends on.

`gwdat-census` (an example) prints what the real archive says about itself:

```
header:  magic 0x1A4E4133, header_size 32, block_size 512, MFT at 3631642624 (3730656 bytes)
entries: 155444
  0x0001B97D  stored 1300044  compressed 8  decompressed 2925267  Lakeside County / Ascalon City / Ashford Abbey
  0x000345CC  stored  515444  compressed 8  decompressed 1734079  Kamadan, Jewel of Istan
```

Two independent checks come out of that. **2,925,267 bytes** for `0x1B97D` is the "2.9 MB region blob" the
reference's own roadmap describes; and every blob the generated catalogue names is present, which
`the_archive_index_finds_the_blobs_the_map_catalogue_names` asserts along with a bogus id being `None`.

A detail that had to be measured rather than read: `FILE_ARCHIVE_MAGIC` is the C multi-character literal
`'\x1ANA3'`, so the four bytes `1A 4E 41 33` collapse to `0x1A4E4133`, not to their byte-reversed reading —
the first attempt compared against the reversed value and failed against the real file.

**D2 landed: the decompressor.** `crates/asura-world/src/fa_compress.rs` is `FaCompress.c`'s decoder, ported
function for function — the MSB-first bit window (`ConsumeBits`/`GetBits`), `SetupNodesandTree`, `GetNextCode`
and `FaDecompress`'s block loop with literals and back-references. Its six static tables are generated from the
reference by `tools/gen-fa-compress-tables.py` (and `--check`ed in the battery), because the first hand
transcription of `Table1` produced values that appear nowhere in the reference.

It is verified against the archive, not against my reading of the C. `GwDat::file()` decodes an entry against
the size in its last four bytes — the pair `FaGetFile` performs — and `a_map_blob_decompresses_to_the_containers_own_signature`
checks three real map blobs for the things a one-bit error would break:

- the decoded length is exactly the announced size (Kamadan: 1,734,079 bytes from 515,444 stored);
- the first four bytes are `"ffna"`, which is the reference's `RIFF_SIGNATURE` (`GmMapDataImport.c:3`);
- the fifth byte is a chunk type, which its `RiffCheckHeader` also requires.

Two details the port had to get right rather than nearly right. The C reads its input as a `u32 *`, so a tail
shorter than a word makes it read the four bytes it was told to ignore — padding reproduces that without the
out-of-bounds read. And `next_bit_encoding + 1` relies on **unsigned wraparound** (the counter runs below zero
and wraps to `u32::MAX`); C wraps silently, Rust panics, so the port wraps explicitly with the reason in a
comment.

**D3 is next**: `GmMapDataImport.c` (590 lines) over this, then `GmPaths.c`'s A*.

### E. Item lifecycle (T12)

`CREATE_UNNAMED_ITEM` cannot be emitted; the ground item cannot be sent; nothing picks anything up
(GWPS has no pick-up handler either).

**E2 landed.** `CREATE_UNNAMED_ITEM` turned out not to be a new feature but a *shape*: the reference has two item creators and uses both — `GameSrv_SendItemById` sends the named one for the inventory stream, and`GameSrv_SendItemsInBag` (`GmItem.c:111-127`) plus `GameSrv_BroadcastPlayerEquippedItems`
(`GmItem.c:129-160`) send the unnamed one for bag slots and equipped items. This repo had used the named form everywhere, so the creation screen's profession change was sending the wrong message. Fixed at the builder and at both call sites; the multi-player introduction now also broadcasts a newcomer's equipped items, visual equipment and cape in the reference's own order (`GameSrv.c:1643-1650`).

**E1 is decided, not done.** The ground item stays a log line: GWPS's own drop handler never removes the item and gives the ground agent the item's id as its model, and no capture contains a working drop. A capture is the only thing that would change this, and the reference cannot produce one — its handler is the unfinished one. Recorded in `docs/08-next-inventory-model.md`.

### F. The load block, byte-compared (T8, T15)

Only the item block is compared against a capture today. The player block is not — which is how two
missing party messages survived an audit that called the block equivalent.

| # | item | done when |
|---|---|---|
| F1 | a generated fixture from the capture (headers + sizes only, no names or UUIDs) | a test that fails when a message is dropped from the block |
| F2 | reconcile the block with the fixture | T15's audit is clean for the load |

**F1–F2 landed.** `tools/parity-load.py` reads the reference's own `GWPS_TRACE=1` log, drops the periodic
traffic (pings and simulation ticks, which the log has and a block dump does not — the reference's own "N
(non-feature)" tier), and compares the ordered `(header, size)` stream. Result: **480 messages, 0 missing,
0 extra, 0 size disagreements**. That measurement is committed as `crates/asura-world/src/load_fixture.rs`
(headers and sizes only, no payload), `the_load_stream_is_the_references_message_for_message` compares the
four blocks against it, and `tools/test-battery.ps1` step 8 re-measures with `--check` so a drift fails the
battery.

The finding that started this item is closed by it: the two party messages are in the stream, in the
reference's position, at the reference's size.

### G. Chat (T11) — equal to the reference, and that is the ceiling

Both servers send a player's line and print a generated answer; neither has ever rendered chat on
screen. `0x005D`/`0x005E`/`0x0061` appear in no capture.

| # | item | done when |
|---|---|---|
| G1 | none, unless a capture of a working chat message appears | recorded, not worked |

### H. Operations parity (T13, T15)

**H1 landed as a measurement, with one clause left to the hand run.** The cadence and the accounting are
now asserted: `an_idle_pair_keeps_its_rhythm_and_its_count` holds two connections idle for six seconds and
checks that the tick and ping keep arriving on the reference's own intervals, that an idle client is told
nothing else, and that the world's player list is a pair while they are there and empty once they leave.
`a_ten_minute_idle_pair_keeps_its_rhythm` is the same assertion at the reference's own duration, marked
`#[ignore]` because a ten-minute test is a test nobody runs; the procedure is in `docs/HANDOFF.md`.
Working-set growth is not measurable portably from inside a test, so that clause is the human's: watch
the process while the ten-minute variant runs.
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
