# The Vanilla pack

The faithful base game: the maps, spawn points, doorways and later NPCs, quests and scripts that
reproduce the live server ASURA is measured against.

What is here today: `character_spawn.json` (who spawns where, read once by the world at start-up) and
`character_defaults.json` (what a new character gets), both read through `asura-world`; and `maps/`, one
flat folder per map id holding that map's `basic.json`, which is what the instance table is built from.
The rest of the content is still in the database and moves here in the order
`docs/38-layer-decisions.md` D22 records: write the package, switch the readers, drop the table last.
`docs/37-rework-layers.md` layer 13 tracks what has moved.

`maps/` has one level to grow (`docs/38-layer-decisions.md`, D38): `<campaign>/<area>/<subarea>/<map id>/`,
the three names read from the client's `AreaInfo` record for that map. Nothing is placed there until that
record has been read, because a folder nobody measured is a guess with a path on it.

Editing this pack is editing the base game. A custom pack is a copy of this folder under another
name in `config/World/`, never an edit here.
