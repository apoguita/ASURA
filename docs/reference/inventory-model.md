# The inventory model, and what measuring it changed

Inventory row model and dated implementation/research findings. The measurements below describe
an earlier port and its client experiments; they are not standing permission to finish or improve
a reference behavior. AGENTS.md requires faithful reproduction and owner-assigned changes.
Current feature acceptance and remaining inventory work are maintained in
[Project status](../work/project-status.md). Packet layouts belong in Client protocol.

## What exists

| piece | where |
|---|---|
| `t_bag`, `t_item`, the item-id sequence, and `t_characters.pos_plane` (renamed by migration 0026) | `crates/asura-db/migrations/0006_inventory.sql` |
| reading a character's bags and items, seeding the default set, finding the item a drop names | `crates/asura-store/src/state/inventory.rs` |
| the seed itself: six bags and nine items, kept as rows from the capture | `crates/asura-store/src/state/inventory/seed.rs` |
| `CREATE_NAMED_ITEM`, `ITEM_MOVE_TO_LOCATION`, `ITEM_SET_PROFESSION` builders (the captured bytes are kept as a recording under `recordings/`) | `crates/asura-protocol/src/game/mod.rs` |
| the item block built out of rows, the wearing ids read from the equipped bag | `crates/asura-world/src/item.rs:70` (`items_block`) and `:120` (`inventory_block`); `crates/asura-world/src/player.rs` (`player_block`) |
| `DROP_ITEM` (`0x802C`) | `crates/asura-world/src/item.rs:187` (`drop_item`), dispatched by `GAME_CMSG_DROP_ITEM` in `crates/asura-world/src/dispatch.rs` |
| the position saved on the way out and read on the way in | `asura_store::state::character::record_position` / `position`, `Standing::resume` |

The earlier document named 	he_item_block_from_rows_is_the_capture_byte_for_byte, but that
function is absent from the current tree. The current test
	he_item_block_opens_with_the_stream_and_carries_the_capture (crates/asura-world/src/lib.rs:648)
checks opening/tail headers and item/location message presence, not byte-for-byte equality of
every inventory message. The old equality claim is therefore not established by the current test.

## Earlier measurement and implementation differences

The plan said three things that the reference does not do. Each one is now behaviour, a test, and a line
in the status document, rather than a guess someone would have to rediscover.

1. **A drop does not take the item out of the bag.** `docs/reference/inventory-model.md` said "find the item, remove it from its
   slot, create the world item, broadcast". `GameSrv_HandleDropItem` finds the item, checks the count,
   creates an agent for it at the player's position plus (30, 30) and returns. It never touches the bag,
   never frees the item id, and never sends `ITEM_REMOVE` - so a dropped cupcake is a second cupcake
   standing on the ground while the first stays in the backpack. The function is plainly unfinished (its
   partial-stack branch is a `TODO` in its own comment).
   **The earlier port diverged here** - `crates/asura-store/src/state/inventory.rs`,
   `take_from_stack` - because a *measured* live session settled the argument: the client sends
   `DROP_ITEM` with `count = 1` for a stack of a hundred, so the reference's rule (`count` must equal the
   whole stack) means the reference's own default cupcake cannot be dropped **at all**, and a live client
   asked three times and got nothing. That explains the earlier implementation; a measurement alone does not authorize the divergence.
2. **An item agent's model id is the item's id**, not the item's model id (`item_agent->model_id =
   item->item_id`), which is why the live session saw nothing on the ground once a drop did get through.
   This port sends the item's `model_id`; the reference's line is kept in the handler's comment so the
   difference is not a mystery.
3. **`CREATE_NAMED_ITEM`'s modifier count is one byte, not two.** Every other counted field in both
   message families is a `u16`; `TYPE_NESTED_STRUCT` in the reference's own packer writes a `u8`
   (`msgpack.c`). That is why the cupcake's message is 36 bytes and `sizeof(GameSrv_CreateNamedItem)` is
   425 - the same trap `docs/reference/client-protocol.md` records for the auth messages, one field further in.
4. **The bags go out in model order, not id order.** The capture's bags are ids 1, 2, 6, 3, 4, 5, because
   the reference walks its `GmBagArray` (backpack, material storage, unclaimed, storage, equipped) and the
   ids were handed out in a different order. Sorting the rows by `bag_id` produces a stream that differs
   from the reference's in message order alone - which `tools/compare-load.py` would have reported as an
   ordering fault in a block whose bytes were all correct.

**And two things that are deliberately not the reference's**, both now implemented and both recorded in
`docs/history/gwps-parity/handler-status.md`:

- **moving an item and wearing one.** `GAME_CMSG_ITEM_MOVE` (`0x8072`, measured: `{item_id u32, bag_id
  u16, slot u8}`, the inverse of the `ITEM_MOVE_TO_LOCATION` we send) and `GAME_CMSG_EQUIP_ITEM`
  (`0x8030`) are messages the reference does not dispatch at all - its whole client-message list is 18
  cases and none of them touch an item. So this behaviour is this project's: a move is checked against
  the bag, the slot and its occupant, and a refused move is answered by telling the client where the item
  really is, because the client moves it in its own model before it asks. Wearing something moves it into
  the equipped-items bag slot its `ItemType` belongs to (`game::equipped_slot`) and re-sends
  `UPDATE_AGENT_VISUAL_EQUIPMENT`.
- **Character position** is stored independently of inventory. Current save/entry handling is maintained
  in [Architecture](../guidance/architecture.md#inside-a-world-server), with acceptance in Project status.

## The traps this left behind

- **A message nobody has measured will crash a client, and three did - one per guess.** `ITEM_REMOVE`
  (`0x014D`) and the item re-creation crashed it; the ground agent (`0x0020` with `AgentType_Item`)
  crashed it; `!hello` crashed it. None of `0x014D`, the item-agent form of `0x0020`, `0x005D`, `0x005E`
  or `0x0061` appears in the capture, and the reference's own comments predicted the chat one exactly
  (*"sending chat crashed the client twice ... every message we send is a guess"*). What survived is what a
  capture or a client measured: a drop that sends **nothing** (the client does its own subtraction, and
  the stack went down in a live session with no message at all), and chat that sends nothing unless
  `ASURA_CHAT=1`. This is the dated reason those guessed sends were withdrawn; AGENTS.md governs new behavior changes.
- **The move's confirmation is still open, and both answers were reported.** With the echo (`0x013E`,
  which *is* in the capture - eight times, in the inventory block) a live client moved the item; with
  silence, the item sprang back and the move did nothing. So the echo is the default. The same session
  also reported a clone, but that build was re-creating items on a drop, so the clone's cause is not
  settled: `ASURA_ITEM_MOVE=silent` and `ASURA_DROP=none` separate them in two runs.
- **A connection that panics must say so.** The accept loop reaps each connection and prints the panic's
  own message, because a task that dies of a panic leaves the process alive, so from outside "the server
  crashed" and "one connection panicked" look identical.
- **An item id is a runtime allocation, not a row number.** The reference hands them out with
  `GameSrv_AllocateItem` and a client only needs them to be consistent within a session. ASURA writes them
  from `item_id_seq`, so the seed's capture ids (1..9) are renumbered as they are written and two
  characters never share one. A fresh database seeds the first character with 1..9 and therefore
  reproduces the capture exactly - but no test depends on that, because a test that depends on a sequence
  is a test that fails on the second character.
- **A bag's own item is a row with no slot.** The reference's `items` table is keyed by
  `(account_id, char_id, bag_model_id, slot)`, so it cannot store one at all; it rebuilds the backpack
  from a struct compiled into the server (`g_Backpack`). Here it is an ordinary item row with `bag_id` and
  `slot` NULL, and `bag.assoc_item_id` names it - which is why the schema has a CHECK that both are NULL
  or neither is, and a *partial* unique index on the slot.
- **`GAME_CMSG_POSITION_UPDATE` is 26 bytes, not 12.** `{position, plane, unk0, unk1}`; the world reads
  the first three fields and the framer still wants all 26. A test that writes 12 bytes leaves the
  connection waiting for the rest of a message it never finishes, and looks exactly like a hang.
- **The spawn answer is where the agent is, not where the map says to arrive.** `GameSrv_HandleInstance
  LoadRequestSpawn` sends `agent->position`, so a client that asks after it has walked gets the walked
  position back - and it is the same number `WORLD_CREATE_AGENT` carries. That was invisible while the
  server kept no position at all; it is the first thing that makes a stored position observable.
- **Reference stack-rebuild pattern, not an accepted ASURA send path.** Neither family has one, so a stack that loses something
  is removed from the client's model (`ITEM_REMOVE`, `0x014D`) and re-created at what is left, in the slot
  it was in. `GameSrv_SendBagItems` is the same rebuild - create, then move - which is where the pattern
  comes from.
- **The client moves an item in its own model before it asks.** A refused `ITEM_MOVE` that answers with
  silence leaves the two sides disagreeing until the next inventory reload, so a refusal sends
  `ITEM_MOVE_TO_LOCATION` for where the item actually is.

## Remaining work

[Project status](../work/project-status.md#remaining-feature-and-evidence-gaps) owns the inventory
backlog and its evidence. This reference does not maintain another creation/pickup/trade task list.
