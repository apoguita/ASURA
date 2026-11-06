# The Vanilla pack

The faithful base game: the maps, spawn points, doorways and later NPCs, quests and scripts that
reproduce the live server ASURA is measured against.

Map descriptions, spawn lists, doorways and arrivals are read directly from this pack by auth and
world. `basic.json` supplies the map id, name, kind and archive file id; `spawns.json` supplies spawn
points; `portals.json` supplies doorway segments or boxes and destination-owned arrivals. The SQL
content tables and compiled doorway tables are no longer runtime sources. The supervisor seeds only
dev identities; it does not pipe map content into SQL.

`character_spawn.json` chooses a new character's initial map, and `character_defaults.json` supplies
its initial character data. Map records with an unknown file id cannot load a client. A null
`instance_value` is omitted from the instance table instead of inventing a value.

`maps/` is laid out as `<campaign>/<area>/<subarea>/<map id>/` (`docs/38-layer-decisions.md`, D38), and
every map id holds its own `basic.json`:

The previously configured database catalogue was migrated into map files without overwriting the
owner's existing map edits or restoring deliberately removed spawn files. Legacy maps with no
established folder placement are under `maps/unplaced/`; their campaign and region are not guessed.
Both the catalogue and instance-table readers walk recursively, so moving a folder does not change
its map id. Only records with a known map kind describe a hosted map; instance-table-only records
remain usable for announcing measured instance values.

Editing this pack is editing the base game. A custom pack is a copy of this folder under another
name in `config/World/`, never an edit here.
