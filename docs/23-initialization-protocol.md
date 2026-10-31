# 23 - The initialization protocol: everything a client is fed when it enters an instance

**Status: the protocol, written down; the gap table says what is missing.** The owner's direction,
2026-10-31: "this must mean we are still ignoring data on initialization, we will need parity, we are
ignoring too many messages and our functionality is being ignored there ... lets keep a full
initialization protocol so we know all our structures are fed appropriately."

So this document is the standing specification for one thing: **the set and order of messages an instance
sends**. A feature that works against a live server but not here is a difference in this table until the
table says otherwise.

## The rule

An instance's initialization is not a bag of messages a client tolerates; it is a sequence that fills a
fixed set of client structures. A world that sends a subset leaves structures empty, and the client then
does nothing where it would otherwise do something - the exploration grid was the first instance of this
(`docs/03-protocol.md` 11.4), the empty unlocked list was the second, and the portal is the third
candidate.

The source for the sequence is a live load, not the reference's C: `GAME_SMSG_*` order and content are
read from `recordings/client/feature/maps/pre-searing-load-3.jsonl` (instance 148, a real client against a
live server) and cross-checked against today's own run in `logs/world-8001.log`.

## The live sequence, in order

Non-tick messages of that load, grouped in the order they arrived; the counts are that one capture's.

| # | messages | count | what structure it fills |
|---|---|---|---|
| 1 | `0x017C` `INSTANCE_LOAD_HEAD` | 1 | the load begins |
| 2 | `0x0186` `INSTANCE_PLAYER_DATA_START` | 1 | |
| 3 | `0x017D` `INSTANCE_LOAD_PLAYER_NAME` | 1 | |
| 4 | `0x019A` `INSTANCE_LOAD_INFO` | 1 | which instance this is |
| 5 | `0x0144` `ITEM_STREAM_CREATE`, `0x0148` `ITEM_SET_ACTIVE_WEAPON_SET` | 1 + 1 | the item stream opens |
| 6 | `0x0161` `CREATE_NAMED_ITEM`, `0x013E` `ITEM_MOVED_TO_LOCATION`, `0x013F` `INVENTORY_CREATE_BAG`, `0x013A` `ITEM_UPDATE_CUSTOMIZED_NAME`, `0x015A` `ITEM_SET_PROFESSION` | 76 + 57 + 6 + 2 + 2 | the inventory |
| 7 | `0x0196` `INSTANCE_LOAD_SPAWN_POINT` | 1 | where the character stands |
| 8 | `0x001D` `PVP_UPDATE_UNLOCKED_SKILLS`, `0x0018` `PVP_UPDATE_UNLOCKED_HEROES`, `0x001A` `PVP_ITEM_STREAM_ADD` | 1 + 1 + 62 | the PvP unlock tables |
| 9 | `0x0059` `PLAYER_CREATE`, `0x0020` `WORLD_CREATE_AGENT`, `0x0056` `NPC_UPDATE_PROPERTIES`, `0x0057` `NPC_UPDATE_MODEL`, `0x006D` `NPC_UPDATE_WEAPONS`, `0x00A6` `AGENT_UPDATE_PROFESSION`, `0x009A` `AGENT_UPDATE_SCALE`, `0x009B` `AGENT_UPDATE_NPC_NAME`, `0x00F0` `AGENT_INITIAL_EFFECTS`, `0x003C` (unnamed), `0x009F` `AGENT_PROPERTY_UPDATE_INT`, `0x00A2` `AGENT_PROPERTY_UPDATE_FLOAT`, `0x0048` `AGENT_DISPLAY_CAPE`, `0x006E` `UPDATE_AGENT_VISUAL_EQUIPMENT`, `0x00B0` `PLAYER_UPDATE_PARTY_SIZE`, `0x00B1` `PLAYER_UPDATE_PARTY` | 25 + 42 + 14 + 13 + 19 + 31 + 10 + 22 + 42 + 29 + 82 + 4 + 23 + 23 + 30 + 29 | **the agent array** - every body in the instance |
| 10 | `0x004C` `QUEST_DESCRIPTION`, `0x0053` `QUEST_ADD_MARKER` | 1 + 1 | the quest log |
| 11 | `0x0118` `GUILD_PLAYER_ROLE`, `0x0121` `GUILD_GENERAL_INFO` | 1 + 14 | the guild window |
| 12 | `0x00F3` `TITLE_RANK_DATA`, `0x00F4` `TITLE_RANK_DISPLAY` | 3 + 8 | the title track |
| 13 | `0x01A0` (unnamed) | 10 | the instance table (`docs/21-surrounding-packets.md`) |
| 14 | `0x006B` `POST_PROCESS`, `0x003E` `AGENT_ALLY_DESTROY`, `0x0021` `WORLD_REMOVE_AGENT`, `0x005A` `PLAYER_DESTROY` | 1 each | the tail |

## What this world sends today, in order

`load_block` then `items_block` then `player_block`, from `crates/asura-world/src/{map,item,player}.rs`:

| # | messages | state |
|---|---|---|
| 1-4 | `0x017C`, `0x0186`, `0x017D`, `0x019A` | sent, same order |
| 5 | `0x008B` `MAP_INIT_START` + `0x008A` `CARTOGRAPHY_DATA` | sent (this client needs it; the capture predates it) |
| 6 | `0x01A0` instance table, ten batches | **sent (2026-10-31)** |
| 7 | `0x0144`, `0x0148`, items | sent on the client's `0x8091` |
| 8 | `0x0196` | sent on the client's `0x8088` |
| 9 | PvP streams | sent |
| 10 | `0x0094` with the reached instances in the fifth list | **sent (2026-10-31)** |
| 11 | `0x0099`, `0x01AC`, manifest `0x0199`/`0x0198`, `0x00F2`, `0x018E` | sent |
| 12 | **the agent array: 21 or 42 bodies with their per-agent streams** | **not sent - one agent, the player** |
| 13 | quests, guild, titles | **not sent** |

## The gap, and the order it is paid

1. **The agent array (live row 9).** The largest gap and the one that changes what the client builds for an
   instance: every body with its name, model, properties, weapons, profession, scale and effects. Source:
   `pre-searing-load-3.jsonl`, message by message, 42 creatures in that outpost.
2. **Quests, guild and titles (rows 10 to 12).** Each is its own structure; a live instance sends them
   even for a character with none (empty lists and zero rows), because an unsent structure is not an empty
   one.
3. **The `0x0196` tail.** The reference zeroes the last eight bytes (`GameMsg.h:346`); a live server fills
   them with two dwords that change per visit. Unmeasured meaning, recorded here as a gap rather than
   guessed at.
4. **The client's own initialization messages we do not answer:** `0x8092` (120 bytes) and `0x8093`
   (4 bytes, `{u16 instance}`), sent 25 times each in one session while browsing the world map. Unnamed in
   the table and unhandled. A live capture of the *retail* build does not contain them, so what they are
   owed has to be measured against this client, not copied.

## How this document is used

- A new feature starts by finding its row here and asking what fills it.
- A message added to an instance sends the whole set its row names, not the one field the feature wanted.
- The row that changes says so in `docs/03-protocol.md` with the measurement behind it.
- When the whole table matches, the initialization is in parity, and that is the state the portal question
  is asked from.
