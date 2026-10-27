# 09 — GWPS parity audit

The question this answers: **for every behaviour GWPS has, does ASURA have it, and is it the same
behaviour?** Nothing here is a plan to rewrite ASURA like GWPS. The reference is
`C:\Users\Apo\Documents\GitHub\PreSearing` (read-only, never modified); the measure is what GWPS
*observably does*: the messages it puts on the wire, the lines it prints, and the rows it writes.

Citations are `File.c:line` inside `PreSearing\code\OpenTyria` for GWPS and `crate/src/file.rs:line`
for ASURA, so every claim below can be checked rather than believed.

## How to read the verdicts

| Verdict | Meaning |
|---|---|
| **1:1** | Same wire messages and same printed lines for the same input. |
| **1:1 (effect)** | Same observable outcome; the internals differ (e.g. ASURA answers from Postgres where GWPS keeps an in-memory index). Nothing a player or a log reader can tell apart. |
| **AHEAD** | ASURA does something GWPS does not. Always labelled, never called parity. |
| **DEVIATION** | Both answer, differently, and the difference is written down and defended. |
| **BEHIND** | GWPS does something a client can observe that ASURA does not do. |
| **STUB** | The handler exists on both sides and neither does anything a client can see. |
| **DEAD** | GWPS's own handler is unfinished (`TODO`, returns early, sends nothing) — there is nothing to match yet. |

A missing feature is only a gap if GWPS *has* it. Where GWPS is unfinished, ASURA matching GWPS would
mean copying a dead end; those rows are marked **DEAD** and are not work items.

---

## Scoreboard

| Domain | Behaviours audited | 1:1 (incl. effect) | AHEAD | DEVIATION | BEHIND | STUB/DEAD |
|---|---|---|---|---|---|---|
| Auth connection | 15 | 14 | – | – | – | 1 |
| Game connection dispatch | 18 | 12 | – | 2 | 4 | – |
| Movement & agents | 7 | 5 | – | 2 | – | – |
| Chat, commands & system messages | 8 | 7 | – | – | 1 | – |
| Items & inventory | 7 | 3 | 3 | 1 | – | – |
| Character creation | 5 | – | – | – | 5 | – |
| World hosting & map config | 6 | 5 | 1 | – | – | – |
| Persistence & schema | 6 | 3 | 3 | – | – | – |
| Tooling & observability | 6 | 3 | 3 | – | – | – |
| **Total** | **78** | **52** | **10** | **5** | **10** | **1** |

Reading it: **52 of 78 behaviours are indistinguishable from GWPS**, ten are things GWPS cannot do at
all, five differ on purpose and are argued below, ten are behind, and one is a stub on both sides.

The ten BEHIND rows are **two** work items, not ten. Nine of them are character creation: the four
`CHAR_CREATION_*` rows in §2 and the five in §6 describe the same five handlers. The tenth is `/tp`,
which needs one message pair this server cannot yet originate.

Two things this table deliberately does *not* count: GWPS's pathfinding and party modules, which are
library code with no dispatch entry point (see §11), and the five creation rows twice over.

---

## 1. Auth connection

GWPS dispatch: `AuthSrv.c:1728-1784`. ASURA dispatch: `crates/asura-auth/src/lib.rs:348-578`.

| # | Message | GWPS behaviour | ASURA state | Verdict |
|---|---|---|---|---|
| 1 | `SEND_COMPUTER_INFO` | Stores the reported user name and PC name on the connection (`AuthSrv.c:1729-1734`); sends nothing. | Ignored (`lib.rs:352`); the frame is consumed structurally before framing. Nothing observable reads the stored names in GWPS either. | 1:1 (effect) |
| 2 | `SEND_COMPUTER_HASH` | Decodes the hash, then answers `SESSION_INFO` with a salt (`AuthSrv.c:1735-1738`). | Same: random salt, `SESSION_INFO` (`lib.rs:359-373`). | 1:1 |
| 3 | `PORTAL_ACCOUNT_LOGIN` | `AuthSrv_HandlePortalAccountLogin` (`AuthSrv.c:774`): account/password against the DB, then the response the client gates on. | `answer_login` (`lib.rs:375-386`) against the same-shaped schema. | 1:1 (effect) |
| 4 | `ASK_SERVER_RESPONSE` | Answers the request with status 0 (`AuthSrv.c:837-841`), logs the header. | Answers status 0 (`lib.rs:389-403`); the header is only logged under `ASURA_TRACE`. | 1:1 (effect) |
| 5 | `SET_PLAYER_STATUS` | Validates the status and stores it on the connection (`AuthSrv.c:844-864`); nothing reads it and nothing is sent. | No-op (`lib.rs:556`). | 1:1 (effect) |
| 6 | `CHANGE_PLAY_CHARACTER` | Re-reads the character list from the DB, finds the name, remembers the index, answers 0 / network error (`AuthSrv.c:866-897`). | Reads characters from the DB, finds the name, records the selection on the session and in `sessions`, answers 0 / auth error (`lib.rs:414-468`). | 1:1 (effect) |
| 7 | `REQUEST_GAME_INSTANCE` | `AuthSrv_HandleRequestGameInstance` (`AuthSrv.c:899`): picks a live world, sends `TRANSFER_GAME_SERVER_INFO`, then success. | `travel::answer` (`crates/asura-auth/src/travel.rs:96`) probes worlds for reachability, sends the same two messages. | 1:1 |
| 8 | `HEARTBEAT` | Answers a server heartbeat (`AuthSrv.c:1263-1268`). | Answers `messages::heartbeat(16)` (`lib.rs:407-410`). | 1:1 |
| 9 | `DISCONNECT` | Logs and closes the connection (`AuthSrv.c:1758-1761`). | `Ok(false)` — the socket closes (`lib.rs:578`). | 1:1 |
| 10 | `ACCEPT_EULA` | Nothing (`AuthSrv.c:1270-1275`). | Nothing (`lib.rs:556`). | STUB |
| 11 | `ADD_ACCESS_KEY` | `AuthSrv_HandleAddAccessKey` (`AuthSrv.c:1277-1290`) logs the key text and keeps nothing. | Counts the key's units and keeps nothing (`lib.rs:485-509`); nothing is logged unless `ASURA_TRACE` is on. | 1:1 (effect) |
| 12 | `DELETE_CHARACTER` | `AuthSrv_HandleDeleteCharacter` (`AuthSrv.c:1292`): deletes when the reasons match, answers per reason. | `db_login::delete_character` with the same reason codes (`lib.rs:511-554`). | 1:1 (effect) |
| 13 | `UPDATE_CHARACTER_SETTINGS` | Answers status 0, stores nothing (`AuthSrv.c:1323-1329`). | Answers status 0 (`lib.rs:470-484`). | 1:1 |
| 14-15 | `SETTING_UPDATE_SIZE`, `SETTING_UPDATE_CONTENT` | `AuthSrv_HandleSettingUpdate` (`AuthSrv.c:656`): logs and answers status 0. | Answer status 0 (`lib.rs:470-484`). | 1:1 (effect) |

**Domain verdict: complete.** All fifteen handlers have a counterpart with the same observable result;
the two effect-level differences (stored computer name, logged access key) are not observable to a
client and are recorded here rather than "fixed" by inventing behaviour.

---

## 2. Game connection dispatch

GWPS dispatch: `GameSrv.c:2007-2074`. ASURA dispatch: `crates/asura-world/src/lib.rs:821-1195`.

| # | Message | GWPS behaviour | ASURA state | Verdict |
|---|---|---|---|---|
| 1 | `DISCONNECT` | Closes the connection (`GameSrv.c:1999`). | `Ok(false)` (`lib.rs:1105`). | 1:1 |
| 2 | `HEARTBEAT` | Ignored (`GameSrv.c:2014-2015`). | Ignored (`lib.rs:1194`). | 1:1 |
| 3 | `PING_REPLY` | Ignored (`GameSrv.c:2016-2017`). | Ignored (`lib.rs:1191`). | 1:1 |
| 4 | `PING_REQUEST` | Answers `PING_REPLY` (`GameSrv.c:1978`). | Answers `ping_reply` (`lib.rs:863-866`). | 1:1 |
| 5 | `INSTANCE_LOAD_REQUEST_SPAWN` | The agent's position and the map file id (`GameSrv.c:1566`). | `spawn_block` (`lib.rs:838-847`, block built at `lib.rs:767`). | 1:1 |
| 6 | `INSTANCE_LOAD_REQUEST_PLAYERS` | The player block ending on `INSTANCE_LOAD_FINISH` (`GameSrv.c:1600`). | `player_block` (`lib.rs:851-860`, built at `lib.rs:1602`). | 1:1 |
| 7 | `INSTANCE_LOAD_REQUEST_ITEMS` | Item stream + inventory + weapon slots + gold + faction caps + character info (`GameSrv.c:1660-1690`). | `items_block` (`lib.rs:825-834`, built at `lib.rs:641`). | 1:1 |
| 8 | `CHAR_CREATION_REQUEST_PLAYER` | Warrior default, then a full player-data start/stop block (`GameSrv.c:1691-1719`). | No arm — the creation instance is not modelled. | BEHIND |
| 9 | `CHAR_CREATION_REQUEST_ARMORS` | PvP skills, PvP heroes, PvP items, account features (`GameSrv.c:1721-1739`). | No arm. | BEHIND |
| 10 | `CHAR_CREATION_CHANGE_PROF` | Re-equips the profession's defaults from `GetDefaultEquipments` (`GameSrv.c:1786-1845`, table in `GmDefaultArmors.c`). | No arm; the table is 2858 lines of generated data that ASURA does not carry. | BEHIND |
| 11 | `CHAR_CREATION_CONFIRM` | Creates the character's bags **and writes the item rows once**, then enters the map (`GameSrv.c:1855-1976`; `GameSrv_CreatePlayerBags` at `GameSrv.c:1230`). | No arm; ASURA seeds rows at login instead (`crates/asura-db/src/inventory.rs:358`). | BEHIND (mechanism) / 1:1 (rows) |
| 12 | `ITEM_UPDATE_EQUIPPED_COLOR` | Declared as a handler but a no-op: it ignores all three parameters (`GameSrv.c:1847-1853`) | No arm: the fallthrough frames it and ignores it, naming it under trace (`lib.rs:1200-1208`). `docs/07-status.md` lists it as a missing arm because it counts dispatch entries, not behaviour | 1:1 (effect) |
| 13 | `SEND_CHAT_MESSAGE` | `GameSrv_HandleChatMessage` (`GmChat.c:279-306`). | `0x8064` (`lib.rs:982-1102`). | 1:1 (effect) |
| 14 | `MOVE_TO_COORD` | Pathfinds and broadcasts the first waypoint (`GmAgent.c:371-398`). | Echoes the client's own destination as `AGENT_MOVE_TO_POINT` (`lib.rs:873-903`). | DEVIATION (no navmesh — see §3) |
| 15 | `POSITION_UPDATE` | Adopts / clamps / refuses by a speed budget (`GmAgent.c:409-462`). | `adopt_report` with the same budget (`lib.rs:909-932`). | 1:1 (effect) |
| 16 | `CANCEL_MOVEMENT` | Tick, cancel, broadcast `AGENT_STOP_MOVING` (`GmAgent.c:533-545`). | `agent_stop_moving` (`lib.rs:936-945`). | 1:1 |
| 17 | `LAST_POS_BEFORE_MOVE_CANCELED` | Accepts a short same-plane correction, refuses a long one, sends nothing (`GmAgent.c:547-563`). | `adopt_correction` (`lib.rs:950-972`). | 1:1 (effect) |
| 18 | `DROP_ITEM` | Finds the item, refuses count < quantity with a `TODO`, else creates an item agent and returns (`GmInventory.c:236-300`). | Deducts for real, persists, prints, sends nothing (`lib.rs:1120-1129`, `drop_item` at `lib.rs:1218`). | DEVIATION (deliberate — see §5) |

---

## 3. Movement & agents

GWPS: `GmAgent.c` (564 lines) plus `GmPaths.c` (A*, 1057 lines) and `GmPathHeap.c`.
ASURA: `crates/asura-world/src/lib.rs` arms + `Player::adopt_report`/`adopt_correction`.

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Walk to a coordinate | A* over the map's path graph; the agent walks waypoint by waypoint (`GmAgent.c:385-395`) | One straight leg to the client's destination (`lib.rs:890-901`) | DEVIATION — no navmesh in ASURA; the client's own prediction carries the walk, and the server never claims a path it did not compute |
| World tick | Every `GAME_SRV_MAX_TIME_BETWEEN_WORLD_TICK_MS`, broadcast + advance movement + regen health/energy (`GmAgent.c:502-531`) | `WORLD_SIMULATION_TICK` on the reference's interval; position comes from the client's reports instead of server-side integration | 1:1 (effect) — the tick is the same message on the same clock |
| Position reports | Adopted within a speed budget, clamped beyond it, refused across a long plane change (`GmAgent.c:409-462`) | Same three outcomes (`lib.rs:920-931`) | 1:1 (effect) |
| Cancel a walk | `AGENT_STOP_MOVING` after a tick (`GmAgent.c:533-545`) | `AGENT_STOP_MOVING` (`lib.rs:936-945`) | 1:1 |
| Correct after a cancel | Short correction on the same plane taken, long refused, nothing sent (`GmAgent.c:547-563`) | Same (`lib.rs:950-972`) | 1:1 (effect) |
| `/stuck` | `WorldTick` + cancel + broadcast position (`GmChat.c:243-251`) | `AGENT_UPDATE_POSITION` (`lib.rs:1038-1062`); the 500 ms ticker covers the tick | 1:1 |
| Agent health/energy regen | Server-side integration during the tick (`GmAgent.c:524-529`) | Fixed values sent once in `player_block`; no regen integration | DEVIATION — invisible with one player and no combat; recorded, not fixed |

---

## 4. Chat, commands & system messages

GWPS: `GmChat.c` (306 lines), `GmCommands.c` (259 lines), `GmText.c`.
ASURA: `lib.rs:974-1102`, `maps_answer` at `lib.rs:1497`.

The single most important finding of this audit: **GWPS puts no chat text on the wire for anything the
*server* generates.** Every server-side answer funnels through `GameSrv_SendSystemChatMessage`
(`GmChat.c:195-233`) or `GameSrv_SendInvalidCommand` (`GmChat.c:145-180`), and both return early at
`GmChat.c:209-212` / `GmChat.c:150-153` unless `GWPS_CHAT=1`. The reference's own comment
(`GmChat.c:129-138`) says why: sending chat crashed its client twice, and its layouts are still
unsettled. A player's `!`/`#`/`$` line *is* sent (`GmChat.c:259-277`, not gated), and the user has
never seen one appear — in either server.

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Player speaks on `!` | Ungated broadcast of the text + `CHAT_MESSAGE_LOCAL` on `Channel_All` (`GmChat.c:259-277`, `290`) | `chat_message_core` + `chat_message_local(PLAYER_ID, CHANNEL_ALL)` (`lib.rs:1010-1030`) | 1:1 (effect) — bytes on the wire in both, visible in neither |
| Player speaks on `#` / `$` | Same path with `Channel_Group` / `Channel_Trade` (`GmChat.c:293-294`) | Same (`lib.rs:1011-1015`) | 1:1 (effect) |
| Player speaks on `@`, `%`, `"` | Silently ignored (`GmChat.c:291-298`) | Ignored (`lib.rs:1086-1095`) | 1:1 |
| Player speaks on any other channel | Error log, `ERR_BAD_USER_DATA` (`GmChat.c:300-302`) | Prints a refusal line (`lib.rs:1096-1099`) | 1:1 (effect) |
| `/maps` | Chat aliases via the gated sender **plus** an unconditional `log_info` table: `Teleport targets (N aliases, every one with a map config):` then `  /tp %-15s -> map %3d  %s` (`GmCommands.c:113-152`) | The same table, printed, character for character (`maps_answer`, `lib.rs:1497-1529`); the chat half is printed only under trace and never sent (`lib.rs:1063-1078`) | 1:1 |
| Unknown `/command` | "Unknown command" through the gated sender ⇒ log line only (`GmChat.c:145-180`) | `world: Unknown command` (`lib.rs:1082`) | 1:1 |
| `/stuck` | Cancel + position broadcast (`GmChat.c:243-251`) | `AGENT_UPDATE_POSITION` (`lib.rs:1038-1062`) | 1:1 |
| `/tp`, `/teleport` | `GameSrv_HandleTeleportCommand` (`GmCommands.c:155-216`): refuses a missing or unknown alias, refuses a map with no config (it would silently fall back to Kamadan, `GmCommands.c:180-190`), refuses "already here", then asks the control connection to transfer the player (`GameSrvCtrl_StartPlayerTransferTo`, `GmCommands.c:214`). Both refusals go through the gated chat sender, so with the gate off they are log lines. | Not implemented — ASURA's world cannot originate a travel (`lib.rs:1034-1036`). | BEHIND |

---

## 5. Items & inventory

GWPS: `GmInventory.c` (drop handler), `GameSrv_CreatePlayerBags` (`GameSrv.c:1230`), `GmItem`.
ASURA: `crates/asura-db/src/inventory.rs` (rows + moves), `crates/asura-world/src/lib.rs` (blocks),
`crates/asura-protocol/src/game/mod.rs` (builders).

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Where items live | In-memory `GmBag`/`GmItem` arrays on the player, written to the DB once at character creation (`GameSrv.c:1230`, called only from `HandleCharCreationConfirm`) | Rows in `bag`/`item` (`crates/asura-db/migrations/0006_inventory.sql`), read at login (`inventory.rs:314`) | AHEAD — ASURA's items survive a restart; GWPS's do not |
| The item block on entering a map | Item stream, one `CREATE_NAMED_ITEM` per item with a **`u8`** modifier count, bags in the reference's own fixed order | Byte-identical block, proven against the capture fixture (`lib.rs:1895` `the_item_block_from_rows_is_the_capture_byte_for_byte`) | 1:1 |
| Move an item to a slot | **No handler.** `GAME_CMSG_ITEM_MOVE` is not in GWPS's dispatch | `0x8072` → `move_item` → echo `ITEM_MOVE_TO_LOCATION`, persisted (`lib.rs:1137-1160`, `inventory.rs:585`) | AHEAD |
| Equip an item | **No handler.** No unequip, trade, split, salvage, identify or use handler either | `0x8030` → equipped bag + `UPDATE_AGENT_VISUAL_EQUIPMENT` (`lib.rs:1167-1176`, `equipped_slot` at `mod.rs:200`) | AHEAD |
| Drop an item | Refuses `count < quantity` (`TODO`), never removes the row, creates an agent whose model id is the *item* id (`GmInventory.c:236-300`) | Deducts, persists, prints; sends nothing because both candidate messages crashed a live client and neither is in a capture (`lib.rs:1107-1129`, `drop_item` `lib.rs:1218`, `drop_messages` `lib.rs:1282`) | DEVIATION — deliberately not the reference's dead end |
| Unequip | **No handler.** | `0x804F` logged as unimplemented — its layout carries no item id (`lib.rs:1180-1186`) | 1:1 (both do nothing, ASURA says so) |
| Item colour update | Declared, ignores all three parameters (`GameSrv.c:1847-1853`) | No arm; the fallthrough frames it and ignores it (`lib.rs:1200-1208`) | 1:1 (effect) — neither does anything |

**The honest summary of this domain:** GWPS's item support is a load request and a broken drop. Three of
ASURA's item behaviours have no GWPS counterpart at all, so "parity" here means ASURA is ahead and the
gap list has one real entry: the **ground-item message** ASURA cannot send yet because no capture
contains it.

---

## 6. Character creation

GWPS: `GameSrv.c:1691-1976`, `GetBagSlotForItemType` (`GameSrv.c:1741-1784`), `GmDefaultArmors.c`.

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Creation instance | `MapType_CharacterCreation` (11), its own map type in `MapType` | Not modelled: the world hosts type 0 only and skips anything else by name (`main.rs:165-175`) | BEHIND |
| `CHAR_CREATION_REQUEST_PLAYER` | Warrior by default, then a full player-data block | No handler | BEHIND |
| `CHAR_CREATION_REQUEST_ARMORS` | PvP skills/heroes/items, account features | No handler | BEHIND |
| `CHAR_CREATION_CHANGE_PROF` | Re-equips from a 2858-line generated table | No handler, no table | BEHIND |
| `CHAR_CREATION_CONFIRM` | Writes the item rows once, then enters the map | Rows are seeded at login (`inventory.rs:358`) | BEHIND (mechanism) |

This is the largest genuine gap, and it is one *feature* — "new characters can be created" — wearing
five dispatch names. It needs a creation instance plus the armour table, and the table is data, not
logic. It is also the only place where ASURA's "rows at login" shortcut is visible to a player: a
character created through GWPS has its items written by the confirm handler.

---

## 7. World hosting & map config

GWPS: `GmMapsConfig.c` (252 lines) + pre-spawned map servers + `CtrlConn` (`GameSrv.c:48-75`,
`1327-1389`). ASURA: `crates/asura-world/src/main.rs`, `crates/asura-db/src/game.rs`.

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Where a map's facts come from | `GmMapsConfig.c`: file id, type, spawn point, spawn plane, capacity, aliases | `map_config` rows + migration 0007's `name` column | 1:1 (effect) |
| Aliases for `/tp` and `/maps` | Config string, spaces collapsed | Derived from the name (`MapEntry::alias`, `lib.rs:129`) | 1:1 (effect) |
| Refusing a map a client cannot load | `map_file_id == 0` refused | Refused with a reason (`main.rs:161-176`) | 1:1 |
| Refusing explorable areas as hosts | Type check | Same check (`main.rs:170`) | 1:1 |
| Advertising a world | `CtrlConn` message to the auth service; heartbeat keeps it alive | `register_world`, heartbeat task (`main.rs:206-240`) | 1:1 (effect) |
| Starting the map server for a travel | Auth service spawns the map server on demand | Auth service registers/chooses a world; `start-server.ps1` starts the process | AHEAD (process control by script vs by the auth service) |

---

## 8. Persistence & schema

GWPS schema (read at `Db.c:269-360`): sessions, accounts, characters, bags, items, friendships; the
binaries check a schema version. ASURA: migrations 0001-0007 — `schema_version`, `account`, `session`,
`world`, `world_pool`, `characters`, `map_config`, `lease`, `transfer`, `bag`, `item`.

| Table / behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| accounts / sessions | Present, same shape (`Db.c:278-287`) | `account`, `session` (`0001`) | 1:1 |
| characters | Present (`Db.c:296`) | Present + `pos_plane` for saved positions (`0006`) | AHEAD (positions persist) |
| bags / items | Present (`Db.c:325`, `Db.c:355`); rows written once at character creation | `bag`, `item`; read per map entry, moved and persisted per action | AHEAD |
| friendships | Table read, but the login response only sends the stream **terminator** (`AuthSrv.c:831`); no entry is ever sent | No table; `friend_stream_end` is sent at the same point in the login reply (`crates/asura-auth/src/lib.rs:698`) | 1:1 (effect) |
| Migrations | A schema version the binaries check | `schema_version` row; the world refuses to start against a stale schema (`main.rs:142-147`) | 1:1 (effect) |
| SQL ownership | Queries scattered through the C code | Only `asura-db` writes SQL (`docs/08`) | AHEAD |

---

## 9. Tooling & observability

| Behaviour | GWPS | ASURA | Verdict |
|---|---|---|---|
| Packet trace | `GWPS_TRACE=1` | `ASURA_TRACE=1` (`tools/play.ps1 -Trace`) | 1:1 |
| Event log that is not a packet log | `log_info` lines for dispatch and GM answers | Unconditional `eprintln!` event lines (`docs/03` §11) | 1:1 |
| Captures in the repository | `GWPS_TRACE=1` output kept as text | `crates/asura-protocol/src/game/default_inventory.rs` is a capture turned into a fixture | AHEAD (a capture that fails the build when it drifts) |
| Web login gate | `tools/webgate.py` on 6601 | The same script, started by `tools/start-server.ps1` | 1:1 |
| One-command server | Several consoles | `tools/start-server.ps1` / `stop-server.ps1` / `play.ps1` | AHEAD |
| Test battery | None in the reference | `tools/test-battery.ps1` + the workspace suite (`cargo test`) | AHEAD |

---

## 10. ASURA behaviour GWPS does not have (explicitly AHEAD)

Recorded here so nobody later "restores parity" by deleting working features:

1. Items survive a restart, and a drop deducted from a stack is gone after a reconnect.
2. Item moves and equips are handled, persisted and echoed.
3. Character positions persist across sessions (`pos_plane IS NULL` means never saved).
4. `map_config.name`, and a `/maps` table that names the map as well as aliasing it.
5. Integration tests that assert the wire (`the_item_block_from_rows_is_the_capture_byte_for_byte`,
   `maps_prints_its_table_and_puts_nothing_on_the_wire`).
6. The world refuses to start against a stale schema instead of failing later.

---

## 11. Where GWPS is itself unfinished (the "missing" that is not a gap)

These are GWPS rows that look like features and are not. Matching them would be copying a dead end:

1. **Drop with `count < quantity`** — `log_warn("TODO: we must still implement this")`
   (`GmInventory.c:283-288`). The client always sends `count = 1` for a stack, so dropping a stack does
   nothing at all in GWPS.
2. **The item agent** — `item_agent->model_id = item->item_id` (`GmInventory.c:300`), so the thing on
   the ground is drawn as nothing; and the item is never removed from the bag.
3. **All server chat** — gated off by default because it crashed the client twice (`GmChat.c:129-143`).
4. **`GmPaths.c` / `GmPathHeap.c`** — A* with no dispatch entry point; `GameSrv_HandleMoveToCoord` is its
   only caller.
5. **`GmParty.c`** (120 lines) — `GameSrv_`-prefixed writers with no `GAME_CMSG_PARTY_*` handler.
6. **`GmDefaultArmors.c`** — a table with one caller, `CharCreationChangeProf`.

---

## 12. The real gap list

Ordered by what a player or tester would notice first. Each entry says what it needs, and nothing else
is added to it.

| # | Gap | What it needs | Blocked on |
|---|---|---|---|
| 1 | `/tp` and `/teleport` | A world-initiated travel: `GAME_SMSG_TRANSFER_GAME_SERVER_INFO` from the world, then the client reconnects | A capture of a working teleport; the message is in no capture |
| 2 | The ground item for a drop | The message that spawns a dropped item on the ground | A GWPS capture of dropping a quantity-1 item |
| 3 | Character creation (5 handlers) | Creation instance (map type 11) + the default-armour table | The table, and a capture of the creation flow |
| 4 | Unequip (`0x804F`) | The item id the client means | A capture; the current layout carries no id |
| 5 | Walking with server-side pathing | A navmesh or GWPS's path graph files | Not a parity defect: ASURA's straight-line echo is already what the client sees |

---

## 13. How to check this audit

```powershell
# GWPS dispatch, the measure for §2
Select-String -Path C:\Users\Apo\Documents\GitHub\PreSearing\code\OpenTyria\GameSrv.c -Pattern 'case GAME_CMSG_'
# the chat gate every server-side answer passes through
Select-String -Path C:\Users\Apo\Documents\GitHub\PreSearing\code\OpenTyria\GmChat.c -Pattern 'ChatSendingEnabled'
# ASURA's arms
Select-String -Path crates\asura-world\src\lib.rs -Pattern '^\s+0x8[0-9A-F]{3} =>'
cargo test --workspace
.\tools\test-battery.ps1
```
