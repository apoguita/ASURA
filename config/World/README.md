# World pack files

This page owns the current pack selection, file layout and loading description.
[Game packs](../../docs/guidance/game-packs.md) owns the intended profile model and pending design decisions.

## Selection and loading

Auth and world select the pack through `ASURA_WORLD_PACK`; the default is
`config/World/Vanilla`. The shared map catalogue is cached for the process lifetime
(`crates/asura-config/src/world.rs`, `pack`, `load`, `catalogue`).

Map descriptions are read from the selected pack, not from SQL. The world reads spawn lists
and portals from that pack too (`crates/asura-world/src/map_spawns.rs` and `portals.rs`).
Legacy SQL tables and database helpers still exist; their presence does not make them the
serving source. Startup seeding is described in the [runbook](../../docs/operations/running-the-server.md).

These are implemented readers, not proof that every map value is measured or that the planned
profile isolation and Lua behavior are implemented.

## Layout

The recorded layout decision is [D38/D39](../../docs/reference/decisions/layer-decisions.md).

```text
config/World/<pack>/maps/<campaign>/<area>/<subarea>/<map id>/
  basic.json
  spawns.json
  portals.json
```

- `<campaign>` and `<area>` use the client's campaign and region names. The map kind is
  `Arenas`, `Explorables`, `Missions` or `Outposts`; dungeon levels use `Explorables`.
- Maps without established folder placement live under `maps/unplaced/<map id>/`.
- `basic.json` holds the map description and instance-table value. `spawns.json` holds its
  spawn list. `portals.json` holds doorway geometry and destination-owned arrivals.
- Catalogue readers walk recursively. The numeric folder and `map_id` must agree;
  malformed catalogue records and duplicate hosted map ids are errors, with no SQL fallback.
- A record with no known `map_type` is not a hosted-map description. It can still supply an
  instance-table value. A null `instance_value` is omitted from that table.
- At pack root, `character_spawn.json` supplies initial-map values and
  `character_defaults.json` supplies initial character data.

The catalogue's validation does not establish that every other pack reader has identical
validation. A map record alone does not establish that the client can load it.
