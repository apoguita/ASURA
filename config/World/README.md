# World packages

One folder per package, and one package loaded per server start
(`docs/38-layer-decisions.md`, D23, D24).

```
config/World/Vanilla     the faithful base game: the pack ASURA reproduces
config/World/<name>      any other pack, copied from Vanilla and edited
```

A pack is self-contained: it does not inherit from `Vanilla`, and loading it does not reach into
another folder for missing content. A pack that has three quests has three quests.

Nothing reads this folder yet. Content still lives in the database, and it leaves in the order
D22 describes: write the package, switch its readers, drop the table last. `Vanilla` is where the
first content table (map settings) lands when that move is scheduled.

The packaging format inside a pack is not chosen. It is not one loose file per map: D23 says a
pack is the unit an author copies and edits.
