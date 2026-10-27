#!/usr/bin/env python3
"""Generate the `map_config` seed from the reference's own tables.

Three C sources, each authoritative for one column:

  code/OpenTyria/GmCommands.c     `s_teleport_targets`: every map the server has a config for, with its
                                  aliases and its `MapType`. The reference's own comment calls this table
                                  "every map the server has a config for" and says the map type travels with
                                  it, which is why the type is read from here and not guessed.
  code/OpenTyria/GmMaps.h         `MapId_<Name>` -> its number
  code/OpenTyria/GmMapsConfig.c   the `GmMapConfig` per map: `map_file_id` and the spawn points

The output is `tools/map-config.sql`: an idempotent upsert that `tools/seed-map.ps1` applies. Two maps here
were until now the only ones a client could reach — Lakeside County and Ascalon City — because the seed was
typed by hand from a measurement rather than generated from the catalogue.

Usage:
  python tools/gen-map-config.py [--gwps PATH] [--out PATH] [--check]

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_GWPS = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_OUT = os.path.join(REPO, "tools", "map-config.sql")

TARGET_RE = re.compile(
    r'\{\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*(MapId_\w+)\s*,\s*(MapType_\w+)\s*\}'
)
MAPID_RE = re.compile(r"X\(\s*(MapId_\w+)\s*,\s*(\d+)\s*\)")
CONFIG_RE = re.compile(r"GmMapConfig\s+(s_\w+)\s*=\s*\{(.*?)\};", re.DOTALL)
CONFIG_ID_RE = re.compile(r"\.map_id\s*=\s*(MapId_\w+)")
CONFIG_FILE_RE = re.compile(r"\.map_file_id\s*=\s*(0x[0-9A-Fa-f]+|\d+)")
SPAWN_RE = re.compile(r"\{\s*(-?\d+(?:\.\d*)?)f?\s*,\s*(-?\d+(?:\.\d*)?)f?\s*,\s*(\d+)\s*\}")

# `MapType_*` to the number this repo's `map_config.map_type` stores. The comment on each is the reference's
# own enum member, so the two vocabularies cannot drift silently.
MAP_TYPES = {
    "MapType_MainTown": 0,
    "MapType_MainExplorable": 1,
    "MapType_GuildHall": 2,
    "MapType_MissionOutpost": 3,
    "MapType_MissionExplorable": 4,
    "MapType_ArenaOutpost": 5,
    "MapType_ArenaExplorable": 6,
    "MapType_HeroesAscentOutpost": 7,
    "MapType_CharacterCreation": 11,
}


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return handle.read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--gwps", default=DEFAULT_GWPS)
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    source = os.path.join(args.gwps, "code", "OpenTyria")
    if not os.path.isdir(source):
        sys.stderr.write("no reference at %s\n" % source)
        return 2

    commands = read(os.path.join(source, "GmCommands.c"))
    maps = read(os.path.join(source, "GmMaps.h"))
    configs = read(os.path.join(source, "GmMapsConfig.c"))

    map_ids = {name: int(value) for name, value in MAPID_RE.findall(maps)}

    # One row per map id: the first alias is the short one the reference lists, and the label is what
    # `/maps` prints. A map with two aliases appears twice in the reference's table and once here.
    targets = {}
    for alias, label, symbol, map_type in TARGET_RE.findall(commands):
        if symbol not in map_ids:
            raise SystemExit("no number for %s in GmMaps.h" % symbol)
        if map_type not in MAP_TYPES:
            raise SystemExit("unknown map type %s" % map_type)
        entry = targets.setdefault(map_ids[symbol], {"label": label, "type": MAP_TYPES[map_type], "aliases": []})
        entry["aliases"].append(alias)

    configured = {}
    for _variable, body in CONFIG_RE.findall(configs):
        symbol = CONFIG_ID_RE.search(body).group(1) if CONFIG_ID_RE.search(body) else None
        if symbol is None or symbol not in map_ids:
            continue
        file_id = CONFIG_FILE_RE.search(body)
        spawn = SPAWN_RE.search(body)
        configured[map_ids[symbol]] = {
            "file_id": int(file_id.group(1), 0) if file_id else 0,
            "spawn": (spawn.group(1), spawn.group(2), spawn.group(3)) if spawn else None,
        }

    rows = []
    skipped = []
    for map_id in sorted(targets):
        entry = targets[map_id]
        config = configured.get(map_id)
        if config is None:
            skipped.append((map_id, entry["label"], "no GmMapConfig"))
            continue
        if config["file_id"] == 0:
            skipped.append((map_id, entry["label"], "no map_file_id"))
            continue
        if config["spawn"] is None:
            skipped.append((map_id, entry["label"], "no spawn point"))
            continue
        x, y, plane = config["spawn"]
        rows.append(
            (
                map_id,
                entry["type"],
                config["file_id"],
                x,
                y,
                plane,
                entry["label"],
                entry["aliases"],
            )
        )

    lines = [
        "-- Generated by tools/gen-map-config.py. Do not edit by hand.",
        "--",
        "-- Every map the reference has a config for, from its own tables: the alias table in GmCommands.c",
        "-- (which also carries the MapType), the numbers in GmMaps.h, and the file id and spawn point in",
        "-- GmMapsConfig.c. Applied by tools/seed-map.ps1.",
        "--",
        "-- Maps the reference configures but cannot spawn into are not here: the five late-campaign maps",
        "-- have a config with no spawn point, so a client sent to one has nowhere to stand. They are listed",
        "-- at the bottom of this file as a comment rather than as rows nothing can use.",
        "",
        "INSERT INTO map_config (map_id, map_type, map_file_id, spawn_count, spawn_x, spawn_y, spawn_plane, name)",
        "VALUES",
    ]
    value_lines = []
    for map_id, map_type, file_id, x, y, plane, label, _aliases in rows:
        value_lines.append(
            "    (%d, %d, %d, 1, %s, %s, %s, '%s')" % (map_id, map_type, file_id, x, y, plane, label.replace("'", "''"))
        )
    lines.append(",\n".join(value_lines))
    lines.append("ON CONFLICT (map_id) DO UPDATE SET")
    lines.append("    map_type    = EXCLUDED.map_type,")
    lines.append("    map_file_id = EXCLUDED.map_file_id,")
    lines.append("    spawn_count = EXCLUDED.spawn_count,")
    lines.append("    spawn_x     = EXCLUDED.spawn_x,")
    lines.append("    spawn_y     = EXCLUDED.spawn_y,")
    lines.append("    name        = EXCLUDED.name,")
    lines.append("    spawn_plane = EXCLUDED.spawn_plane;")
    lines.append("")
    if skipped:
        lines.append("-- Configured in the reference, not seedable here:")
        for map_id, label, why in skipped:
            lines.append("--   %-32s (%d): %s" % (label, map_id, why))
        lines.append("")
    lines.append("-- Aliases, for /maps and /tp (the reference's own table in GmCommands.c):")
    for map_id, _map_type, _file_id, _x, _y, _plane, label, aliases in rows:
        lines.append("--   %-32s %s" % (label, ", ".join(aliases)))
    lines.append("")
    generated = "\n".join(lines)

    if args.check:
        if not os.path.exists(args.out):
            sys.stderr.write("no generated file at %s\n" % args.out)
            return 1
        if read(args.out) != generated:
            sys.stderr.write(
                "the map seed is out of date with the reference; regenerate with "
                "python tools/gen-map-config.py\n"
            )
            return 1
        print("the map seed is current: %s" % args.out)
        return 0

    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(generated)
    print("wrote %s: %d map(s), %d skipped" % (args.out, len(rows), len(skipped)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
