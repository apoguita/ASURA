# The inventory model, and what measuring it changed

`docs/08` used to be a plan: three tables, an item block built from rows, `DROP_ITEM`, and a position
that survives a logout. This is what of it is now true, and — more usefully — the four places where the
reference disagreed with the plan. Two of them are in `GmInventory.c` and were settled by reading it; one
is in `msgpack.c` and was settled by counting the bytes in the capture; and one is only visible in the
capture's ordering.

## What exists

| piece | where |
|---|---|
| `bag` and `item`, the item-id sequence, and `characters.pos_plane` | `crates/asura-db/migrations/0006_inventory.sql` |
| reading a character's bags and items, seeding the default set, finding the item a drop names | `crates/asura-db/src/inventory.rs` |
| the seed itself: six bags and nine items, generated from the capture | `crates/asura-db/src/inventory/seed.rs` (+ `tools/gen-default-inventory.py`) |
| the captured messages, kept as the fixture the rows have to reproduce | `crates/asura-protocol/src/game/default_inventory.rs` |
| `CREATE_NAMED_ITEM`, `ITEM_MOVE_TO_LOCATION`, `ITEM_SET_PROFESSION` builders | `crates/asura-protocol/src/game/mod.rs` |
| the item block built out of rows, the wearing ids read from the equipped bag | `crates/asura-world/src/lib.rs` (`inventory_block`, `items_block`, `player_block`) |
| `DROP_ITEM` (`0x802C`) | same file, the `0x802C` arm and `drop_item` |
| the position saved on the way out and read on the way in | `asura_db::game::record_position` / `position`, `Standing::resume` |

**The proof that this is the reference's inventory and not an approximation of it**: the block built from
six bag rows and nine item rows is compared to the captured messages — every message, in order, byte for
byte (`the_item_block_from_rows_is_the_capture_byte_for_byte`). The inventory did not go from "replayed" to
"rebuilt and close enough"; it went from replayed to rebuilt and *identical*, and the test says so on every
run.

## What the measurement changed

The plan said three things that the reference does not do. Each one is now behaviour, a test, and a line
in the status document, rather than a guess someone would have to rediscover.

1. **A drop does not take the item out of the bag.** `docs/08` said "find the item, remove it from its
   slot, create the world item, broadcast". `GameSrv_HandleDropItem` finds the item, checks the count,
   creates an agent for it at the player's position plus (30, 30) and returns. It never touches the bag,
   never frees the item id, and never sends `ITEM_REMOVE` — so a dropped cupcake is a second cupcake
   standing on the ground while the first stays in the backpack. The function is plainly unfinished (its
   partial-stack branch is a `TODO` in its own comment), and this port copies it. Removing the item would
   be inventing the half the reference never wrote, and the honest place for that is an item-removal
   message that does not exist yet.
2. **An item agent's model id is the item's id**, not the item's model id (`item_agent->model_id =
   item->item_id`). Copied as well, with the note in the handler, because a client given a different
   model id draws a different thing and "we fixed their bug" is a parity claim nobody can check.
3. **`CREATE_NAMED_ITEM`'s modifier count is one byte, not two.** Every other counted field in both
   message families is a `u16`; `TYPE_NESTED_STRUCT` in the reference's own packer writes a `u8`
   (`msgpack.c`). That is why the cupcake's message is 36 bytes and `sizeof(GameSrv_CreateNamedItem)` is
   425 — the same trap `docs/03-protocol.md` records for the auth messages, one field further in.
4. **The bags go out in model order, not id order.** The capture's bags are ids 1, 2, 6, 3, 4, 5, because
   the reference walks its `GmBagArray` (backpack, material storage, unclaimed, storage, equipped) and the
   ids were handed out in a different order. Sorting the rows by `bag_id` produces a stream that differs
   from the reference's in message order alone — which `tools/compare-load.py` would have reported as an
   ordering fault in a block whose bytes were all correct.

**And one thing that is deliberately not the reference's.** The reference keeps a character's last *map*
(`characters.last_outpost`) and re-spawns it at that map's spawn point on every session; it never stores a
coordinate. `0006_inventory.sql` stores one, the world writes it when the player leaves and reads it when
the player returns, and `Standing::resume` uses it only in the map it was taken in. This is a recorded
deviation, not drift: `docs/07-status.md` says so, and the column's comment says why (`pos_plane IS NULL`
is what "never saved" means, because (0, 0) is a legal coordinate).

## The traps this left behind

- **An item id is a runtime allocation, not a row number.** The reference hands them out with
  `GameSrv_AllocateItem` and a client only needs them to be consistent within a session. ASURA writes them
  from `item_id_seq`, so the seed's capture ids (1..9) are renumbered as they are written and two
  characters never share one. A fresh database seeds the first character with 1..9 and therefore
  reproduces the capture exactly — but no test depends on that, because a test that depends on a sequence
  is a test that fails on the second character.
- **A bag's own item is a row with no slot.** The reference's `items` table is keyed by
  `(account_id, char_id, bag_model_id, slot)`, so it cannot store one at all; it rebuilds the backpack
  from a struct compiled into the server (`g_Backpack`). Here it is an ordinary item row with `bag_id` and
  `slot` NULL, and `bag.assoc_item_id` names it — which is why the schema has a CHECK that both are NULL
  or neither is, and a *partial* unique index on the slot.
- **`GAME_CMSG_POSITION_UPDATE` is 26 bytes, not 12.** `{position, plane, unk0, unk1}`; the world reads
  the first three fields and the framer still wants all 26. A test that writes 12 bytes leaves the
  connection waiting for the rest of a message it never finishes, and looks exactly like a hang.
- **The spawn answer is where the agent is, not where the map says to arrive.** `GameSrv_HandleInstance
  LoadRequestSpawn` sends `agent->position`, so a client that asks after it has walked gets the walked
  position back — and it is the same number `WORLD_CREATE_AGENT` carries. That was invisible while the
  server kept no position at all; it is the first thing that makes a stored position observable.

## What is next

The four `CHAR_CREATION_*` handlers are what is left of the reference's dispatch (`docs/07-status.md`), and
they need the two things `docs/08` named: a character-creation instance, and the default-equipment table
that `GetDefaultEquipments` answers with. The equipment half is now cheap — an item row is a row, and the
table that `GmDefaultArmors.c` carries is 2820 lines of it — so the instance type is what is actually
missing.
