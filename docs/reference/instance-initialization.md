# The initialization protocol: everything a client is fed when it enters an instance

**Status: the protocol, written down; the gap table says what is missing.** The owner's direction,
2026-10-31: "this must mean we are still ignoring data on initialization, we will need parity, we are
ignoring too many messages and our functionality is being ignored there ... lets keep a full
initialization protocol so we know all our structures are fed appropriately."

This reference owns initialization sequence explanations. The captured sequence and current code path
are separate records; neither makes an unmeasured message, count or inferred purpose a settled rule.

Apply the [recorded-behavior reproduction rule](client-protocol.md#recorded-behavior-reproduction)
to load work; missing or unidentified messages remain gaps in the sequence.

## The rule

An instance's initialization is not a bag of messages a client tolerates; it is a sequence that fills a
fixed set of client structures. A world that sends a subset leaves structures empty, and the client then
does nothing where it would otherwise do something - the exploration grid was the first instance of this
(`docs/reference/client-protocol.md` 11.4), the empty unlocked list was the second, and the portal is the third
candidate.

The source for the sequence is a live load, not the reference's C: `GAME_SMSG_*` order and content are
read from `recordings/client/feature/maps/pre-searing-load-3.jsonl` (instance 148, a real client against a
live server) and cross-checked against today's own run in `logs/world-8001.log`. **Order, count and trigger
are what that file is read for**; a size in it is the client's message struct and never a wire size, so no
size and no field layout comes from it (`docs/reference/client-protocol.md` 5e) - those come from a wire capture, and a
message uses the catalogue field description and counted framing, checked against wire evidence.

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
| 9 | `0x0059` `PLAYER_CREATE`, `0x0020` `WORLD_CREATE_AGENT`, `0x0056` `NPC_UPDATE_PROPERTIES`, `0x0057` `NPC_UPDATE_MODEL`, `0x006D` `NPC_UPDATE_WEAPONS`, `0x00A6` `AGENT_UPDATE_PROFESSION`, `0x009A` `AGENT_UPDATE_SCALE`, `0x009B` `AGENT_UPDATE_NPC_NAME`, `0x00F0` `AGENT_INITIAL_EFFECTS`, `0x003C` `PLAYER_UPDATE_FLAGS`, `0x009F` `AGENT_PROPERTY_UPDATE_INT`, `0x00A2` `AGENT_PROPERTY_UPDATE_FLOAT`, `0x0048` `AGENT_DISPLAY_CAPE`, `0x006E` `UPDATE_AGENT_VISUAL_EQUIPMENT`, `0x00B0` `PLAYER_UPDATE_PARTY_SIZE`, `0x00B1` `PLAYER_UPDATE_PARTY` | 25 + 42 + 14 + 13 + 19 + 31 + 10 + 22 + 42 + 29 + 82 + 4 + 23 + 23 + 30 + 29 | **the agent array** - every body in the instance |
| 10 | `0x004C` `QUEST_DESCRIPTION`, `0x0053` `QUEST_ADD_MARKER` | 1 + 1 | the quest log |
| 11 | `0x0118` `GUILD_PLAYER_ROLE`, `0x0121` `GUILD_GENERAL_INFO` | 1 + 14 | the guild window |
| 12 | `0x00F3` `TITLE_RANK_DATA`, `0x00F4` `TITLE_RANK_DISPLAY` | 3 + 8 | the title track |
| 13 | `0x01A0` `INSTANCE_TABLE` | 10 | the instance table (`docs/research/client-session-packet-analysis.md`) |
| 14 | `0x006B` `POST_PROCESS`, `0x003E` `AGENT_ALLY_DESTROY`, `0x0021` `WORLD_REMOVE_AGENT`, `0x005A` `PLAYER_DESTROY` | 1 each | the tail |

## The two layers, measured (2026-10-31)

One run entered the same outpost twice, with a different-district load of that outpost between them
(`recordings/asura/world/load-layers-20261031.jsonl`, five loads with their per-header counts). The
district load carried **0 agent creates** and 260 messages, against 8 creates and 539 messages for the
full load of the same map. That splits the initialization into two layers by measurement rather than
by guess:

| layer | what it is | sent on | headers |
|---|---|---|---|
| per character | items, titles, party, player flags, guild, the instance table | every load, a district change included | `0x0161`, `0x015E`?, `0x013E`, `0x013F`, `0x00F3`, `0x00F4`, `0x00B0`, `0x00B1`, `0x003C`, `0x0118`, `0x0121`, `0x01A0` |
| per map population | every body in the instance and its streams | only when the world sends a population | `0x0020`, `0x0021`, `0x0025`, `0x0029`, `0x002B`, `0x0028`, `0x0048`, `0x0056`, `0x0057`, `0x0059`, `0x005A`, `0x006D`, `0x006E`, `0x008C`, `0x009B`, `0x00A6`, `0x00AA`, `0x00F0`, `0x00F1`, `0x0115`, `0x0107` |

The same run measured the per-map shape of a load: 166 loaded with 8 creates and 538 messages, 162
with 4 and 3 creates and 342 and 238 messages, and `0x008A` carried 132 bytes on every load but the
session's first. `0x0196` carried a different position per map. So the population block is
`population = creates` and it is per instance, never a constant to copy from one capture.

What the client asks while this happens, in the same run: `0x8093` eight times, one per instance
(`166, 145, 146, 148, 160, 161, 162, 164`) when the world map opened; `0x00B1` once, naming 166 as the
chosen instance; `0x00C1` twice at a portal, `{1, 0}` then `{0, 0}`. Of the three, the world-map request
is answered - `GAME_CMSG_PARTY_TRAVEL` ends in the same transfer cycle (`crates/asura-world/src/
dispatch.rs:58`, `crates/asura-world/src/travel.rs:298`). `0x8093` is named
(`GAME_CMSG_INSTANCE_QUERY`) and read, and `0x80C1` is read: neither starts anything
(`crates/asura-world/src/travel.rs:154-166`).

## What this world sends today, in order

Checked against `crates/asura-world/src/map.rs`, `connection.rs`, `item.rs`, `agent.rs` and
`player.rs` during the documentation pass, 2026-11-08. This is a code-path description, not a
new client test. Packet layouts are maintained in [Client protocol](client-protocol.md).

| Stage | Current source and behavior | Limits |
|---|---|---|
| Unasked head | `map::load_block`: load head, player-data start, name, load info, then optional grid and instance table | `ASURA_LOAD_EXTRA=off` suppresses the extra grid/table block |
| Instance table | `instance_table::table_for` reads pack records; `frames` batches the resulting pairs | Batch count depends on installed values, not a fixed ten messages |
| Item request (`0x8091`) | `item::handle_request_items` supplies inventory and download manifest | Inventory content depends on the character |
| Spawn request (`0x8088`) | `agent::handle_request_spawn` sends current entry position | Placement repairs still await client verification in Project status |
| Players request (`0x8090`) | `player::handle_request_players`: progress, player/agent and party messages, peer introductions, then load finish | Supports connected players; does not supply the captured NPC/gadget population, quest/guild/title completeness |
| Simulation activation | The player slot is marked loaded after its players block is sent (`player.instance.loaded`) | The instance tick skips unloaded slots; it does not intentionally tick that player throughout the load |

The capture's 42 agent creates and its quest/guild/title streams are evidence for that particular
live load, not constants to replay for every instance. Feature acceptance is maintained in
[Project status](../work/project-status.md); header coverage in [Packet coverage](../work/packet-coverage.md).

## The explorable: one field decides which load the client runs (measured 2026-10-31)

`is_explorable` in the redirect is not a label a client stores. It selects **which load the client runs**, and
an explorable's load is a different procedure from an outpost's. Measured on the patched client against this
world, with one field moved and nothing else (`ASURA_TRAVEL_EXPLORABLE`,
`crates/asura-world/src/travel.rs`), over the same walk through the same portal:

| what the redirect carried | destination | redirect to the players block |
|---|---|---|
| `is_explorable 1` - the destination's own kind | 146 Lakeside County, explorable | **5138 ms** |
| `is_explorable 0` - forced | 146, the same walk | **849 ms** |
| `is_explorable 0` - forced | 148 Ascalon City, outpost | 849 ms |

The client's own clock agrees: in `recordings/client/feature/travel/explorable-flag-on-20261031-223636.jsonl`
the load with the flag at 1 spends **4984 ticks (7.0 s)** between its spawn request and its players request,
and the crossing back into the outpost in the same file spends 640 ticks (0.90 s). The live figure for that
step, over 36 loads in five captures, is 0.507-1.015 s, median 0.763 s.

**Observed delay; cause unresolved.** The spawn-to-players-request gap occurs before the request
that asks for the population block. Missing agents are a completeness gap, but this timing does
not establish them as the cause of the delay. Quiet network time is consistent with work inside
the client; the exact structure, timeout or missing input has not been identified.

The earlier one-variable experiments, recorded 2026-10-31, reported the same 5.1 s after changing
the travel-withdrawer burst, tick gating (49 early ticks versus none), and the optional grid/table
block. A 400-iteration database probe reported a slowest round trip of 156 ms. Those reports limit
particular explanations; they do not prove that every database or network cause is excluded.
No additional durable recording is named here for that probe.

`tools/context-dump.py` was proposed for before/during/after observations in both flag modes.
Follow [capture prerequisites](../operations/capturing-instance-initialization.md). Context changes
and timestamps would be evidence to investigate, not an automatic causal diagnosis. This proposal
does not create a prerequisite for independently assigned population work.

**Current implementation.** `travel::explorable_flag_for` defaults to zero; environment value `1`
forces one, while `auto` maps selected destination kinds to one. This was checked in
`crates/asura-world/src/travel.rs:30-68`. The code's `auto` rule is not a verified universal live
rule: `recordings/client/feature/character_creation/maps-tour3.jsonl` contains both zero and one
for destination 29. A default change needs its own assigned behavior change and evidence.
Reported UI/party effects of announcing an explorable as an outpost remain investigation claims;
this documentation pass did not run a client test.

## The gap, and the order it is paid

The order below is the order of *completeness*. The explorable's 4.3 s wait is not proven to sit in any of
these rows - the section above says why, and the context dump is a proposed next observation - so the
the list does not choose implementation order or require that proposed dump before other assigned work.

1. **The agent array (live row 9).** The largest gap and the one that changes what the client builds for an
   instance: every body with its name, model, properties, weapons, profession, scale and effects. Source:
   `pre-searing-load-3.jsonl`, message by message, 42 agent creates in that captured outpost. The cause of the recorded load delay is unresolved; this population gap does not prove that cause.
2. **Quests, guild and titles (rows 10 to 12).** Each is its own structure; a live instance sends them
   even for a character with none (empty lists and zero rows), because an unsent structure is not an empty
   one.
3. **The `0x0196` tail.** The reference zeroes the last eight bytes (`GameMsg.h:346`); a live server fills
   them with two dwords that change per visit. Unmeasured meaning, recorded here as a gap rather than
   guessed at.
4. **The client's own initialization messages we do not answer:** `0x8092` (120 bytes, still `Unnamed`)
   and `0x8093` (4 bytes, `{u16 instance}`, now named `GAME_CMSG_INSTANCE_QUERY` and `Observed`,
   `crates/asura-protocol/src/packets.rs:480`), sent 25 times each in one session while browsing the world
   map. Both are unhandled. A live capture of the *retail* build does not contain them, so what they are
   owed has to be measured against this client, not copied.

## How this document is used

Use the named live capture for sequence/count evidence, Client protocol for field layouts, and
Packet coverage for header status. A matched table alone is not complete stream parity; the real
client and the reference stream remain the judge under AGENTS.md. This reference adds no behavior-change policy.
