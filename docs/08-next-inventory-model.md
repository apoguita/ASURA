# Next: the inventory model (unblocks DROP_ITEM and persistence)

The last five handlers are blocked on this, so it is written down before it is started.

## What exists

`game::default_inventory` is the reference's own item messages, replayed as bytes. The client is happy;
the server knows nothing. Nothing can be dropped, equipped, moved or saved because there is no row, bag
or slot to name.

## What replaces it

Three tables, in a migration, modelled on what the reference keeps in memory:

- `item` -- id, character, file id, model id, item type, flags, quantity, position (bag + slot), the
  modifier list and the name, because those are the fields `CREATE_NAMED_ITEM` carries;
- `bag` -- character, bag id, model id, type, slot count, associated item id (the reference's
  `GmBag`/`GmBagArray`);
- `character` gains the three things persistence needs that it lacks: map id (has), position and plane.

The default inventory then becomes *seed data* rather than replayed bytes: one migration inserts the
same nine items the capture shows, with the same ids. `tools/gen-default-inventory.py` becomes the
generator for that seed instead of for a byte table.

## Order

1. migration + `asura_db::inventory` (rows, read a character's bags and items);
2. the item block built from rows instead of bytes -- the load stream must stay byte-identical to the
   capture, which `tools/compare-load.py` checks;
3. `DROP_ITEM` (`0x802C`): find the item, remove it from its slot, create the world item, broadcast;
4. position written when the player leaves, read when it enters (M5's persistence).

## Why not before

Character creation also depends on equipment (GWPS answers `GET_DEFAULT_EQUIPMENTS`), so an inventory
model is a prerequisite for both of the remaining scope items, and for keeping a character across
sessions. It is the single highest-value next step.
