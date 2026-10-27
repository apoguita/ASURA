#!/usr/bin/env python3
"""Diff the GM command surface against the reference's, and publish it as a documented measurement.

The commands are a small surface with a large blast radius: `/tp` moves a player between servers, `/maps`
is the only way to find out what a world hosts, and `/stuck` sends a different message from the cancel that
looks like it. This tool reads the reference's two dispatch points and this repo's own table and compares
them:

  * **the names**: `strcmp(command, "...")` in `GmCommands.c`'s `GameSrv_TryHandleGmCommand` and the
    `slice_u16_equals_ascii_lit` in `GmChat.c`'s `GameSrv_HandleChatCommand`, against `COMMANDS` in
    `crates/asura-world/src/lib.rs` -- which the handler itself dispatches through, so the table cannot say
    something the code does not do;
  * **the aliases**: `s_teleport_targets` in `GmCommands.c` -- twenty aliases over twelve maps -- against
    the generated `crates/asura-world/src/travel_targets.rs`, row for row.

`--write` regenerates `docs/13-command-parity.md`; `--check` fails on a command or an alias that has no
counterpart here.

The finding that made this tool worth writing: this server resolved `/tp` against each map's **name**, so
the reference's short aliases (`lakeside`, `ascalon`, `catacombs`, `northlands`, `green hills`, `barradin`,
`wizards folly`, `foibles`, `regent`, `kamadan`) and the ones whose name carries a suffix (`ascalon city`,
`barradin estate`, `regent valley`) answered "unknown map name" -- six of the reference's twenty worked.
That is fixed by generating the alias table from the reference and resolving through it; this tool is what
keeps it fixed.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import io
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCE_REPO = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_SOURCE = os.path.join(REFERENCE_REPO, "code", "OpenTyria")
DEFAULT_COMMANDS = os.path.join(REPO, "crates", "asura-world", "src", "lib.rs")
DEFAULT_ALIASES = os.path.join(REPO, "crates", "asura-world", "src", "travel_targets.rs")
DEFAULT_OUT = os.path.join(REPO, "docs", "13-command-parity.md")

# `MapType_*` to the number both sides use (`GmMaps.h`; the same mapping `tools/gen-map-config.py` applies).
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

# `strcmp(command, "tp")` in GmCommands.c, and `slice_u16_equals_ascii_lit(msg, "stuck", ...)` in GmChat.c.
STRCMP_RE = re.compile(r'strcmp\(\s*command\s*,\s*"([^"]+)"\s*\)')
ASCII_LIT_RE = re.compile(r'slice_u16_equals_ascii_lit\(\s*msg\s*,\s*"([^"]+)"')
# `TravelTarget { alias: "lakeside", ... map_id: 146, ... }` in the generated table.
TARGET_RE = re.compile(
    r'TARGET\s*\{|TravelTarget\s*\{\s*alias:\s*"([^"]+)"[^}]*?label:\s*"([^"]*)"[^}]*?'
    r'map_id:\s*(-?\d+)[^}]*?map_type:\s*(-?\d+)[^}]*?hosted:\s*(true|false)',
    re.DOTALL,
)
# `s_teleport_targets` rows: { "alias", "label", MapId_X, MapType_Y }.
C_TARGET_RE = re.compile(
    r'\{\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*(MapId_\w+)\s*,\s*(MapType_\w+)\s*\}'
)
# `COMMANDS` entries in lib.rs: names: &["tp", "teleport"], ... reference: "...", ... takes_argument: ...
COMMAND_RE = re.compile(
    r'Command\s*\{\s*names:\s*&\[([^\]]*)\][^}]*?reference:\s*"([^"]*)"[^}]*?'
    r'takes_argument:\s*(true|false)',
    re.DOTALL,
)


def read(path):
    return io.open(path, "r", encoding="utf-8", errors="replace").read()


def reference_commands(source):
    """The names the reference dispatches, in the order its two files mention them."""
    names = []
    origin = {}
    for filename, pattern in (
        ("GmCommands.c", STRCMP_RE),
        ("GmChat.c", ASCII_LIT_RE),
    ):
        text = read(os.path.join(source, filename))
        for name in pattern.findall(text):
            if name not in names:
                names.append(name)
                origin[name] = filename
    return names, origin


def reference_aliases(source):
    text = read(os.path.join(source, "GmCommands.c"))
    # `MapId_*` numbers live in GmMaps.h; resolve them so the comparison is ids and not symbols.
    numbers = dict(
        (name, int(value))
        for name, value in re.findall(r"X\(\s*(MapId_\w+)\s*,\s*(\d+)\s*\)", read(os.path.join(source, "GmMaps.h")))
    )
    aliases = []
    for alias, label, symbol, map_type in C_TARGET_RE.findall(text):
        if symbol not in numbers:
            raise SystemExit("no number for %s in GmMaps.h" % symbol)
        if map_type not in MAP_TYPES:
            raise SystemExit("unknown map type %s" % map_type)
        aliases.append((alias, label, numbers[symbol], MAP_TYPES[map_type]))
    return aliases


def our_commands(path):
    commands = []
    for names, reference, takes_argument in COMMAND_RE.findall(read(path)):
        parsed = tuple(re.findall(r'"([^"]+)"', names))
        commands.append((parsed, reference, takes_argument == "true"))
    return commands


def our_aliases(path):
    aliases = []
    for alias, label, map_id, map_type, hosted in TARGET_RE.findall(read(path)):
        aliases.append((alias, label, int(map_id), int(map_type), hosted == "true"))
    return aliases


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source", default=DEFAULT_SOURCE, help="the reference's code directory")
    parser.add_argument("--commands", default=DEFAULT_COMMANDS)
    parser.add_argument("--aliases", default=DEFAULT_ALIASES)
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    if not args.write and not args.check:
        parser.error("nothing to do: pass --check or --write")
    if not os.path.isdir(args.source):
        print("skipped: no reference at {} (pass --source)".format(args.source))
        return 0

    reference_names, origin = reference_commands(args.source)
    reference_alias_rows = reference_aliases(args.source)
    commands = our_commands(args.commands)
    aliases = our_aliases(args.aliases)

    ours = [name for names, _ref, _takes in commands for name in names]
    missing = [name for name in reference_names if name not in ours]
    extra = [name for name in ours if name not in reference_names]

    # Every reference alias has to be in the generated table with the same map, and the table must not
    # carry an alias the reference does not have.
    our_by_alias = {alias: row for alias, *row in aliases}
    alias_problems = []
    for alias, label, map_id, map_type in reference_alias_rows:
        row = our_by_alias.get(alias)
        if row is None:
            alias_problems.append((alias, "absent from the generated table"))
            continue
        _our_label, our_map, our_type, _hosted = row
        if our_map != map_id:
            alias_problems.append((alias, "map %d here, %d in the reference" % (our_map, map_id)))
        elif our_type != map_type:
            alias_problems.append((alias, "map type %d here, %d in the reference" % (our_type, map_type)))
    for alias in our_by_alias:
        if alias not in [row[0] for row in reference_alias_rows]:
            alias_problems.append((alias, "in the generated table but not in the reference"))

    lines = [
        "# 13 - The GM command surface, diffed against the reference's",
        "",
        "Generated by `tools/parity-commands.py` from `GmCommands.c`, `GmChat.c` and `GmMaps.h` against this",
        "repo's `COMMANDS` table and its generated alias table. The command surface is small and its",
        "blast radius is not: `/tp` moves a player between servers, `/maps` is the only way to find out what",
        "a world hosts, and `/stuck` sends a different message from the cancel that looks like it.",
        "",
        "```powershell",
        "python tools/parity-commands.py --check",
        "```",
        "",
        "## The commands",
        "",
        "| what a player types | where the reference handles it | takes an argument | what this server answers |",
        "|---|---|---|---|",
    ]
    for names, reference, takes_argument in commands:
        lines.append(
            "| %s | `%s` | %s | %s |"
            % (
                ", ".join("`/%s`" % name for name in names),
                reference,
                "yes" if takes_argument else "no",
                "a transfer, or the reference's refusals" if "tp" in names
                else "the alias table in the log" if "maps" in names
                else "the agent's position",
            )
        )
    lines += [
        "",
        "The reference's names, from its two dispatch points: %s."
        % ", ".join(
            "`/%s` (%s)" % (name, origin[name]) for name in reference_names
        ),
        "",
        "## The `/tp` aliases",
        "",
        "`s_teleport_targets` is twenty aliases over twelve maps: two for most, because the reference lets a",
        "tester type `lakeside` or `lakeside county`. This server resolves through the same generated table,",
        "so every one of them works -- which it did not until the table was generated: matching on each",
        "map's *name* accepted six and answered \"unknown map name\" for the rest.",
        "",
        "| alias | label | map | type | hosted here |",
        "|---|---|---|---|---|",
    ]
    for alias, label, map_id, map_type, hosted in aliases:
        lines.append(
            "| `%s` | %s | %d | %d | %s |"
            % (alias, label, map_id, map_type, "yes" if hosted else "no")
        )
    lines += [
        "",
        "`hosted` is this server's answer to \"can we actually serve that map\": the reference refuses a",
        "target with no `GmMapConfig` rather than letting its loader fall back to Kamadan, and this refuses a",
        "map the world was not started with. Both refusals are logged, and neither puts an answer on the",
        "wire -- the reference's chat gate is off in both servers, so a GM answer has always been a log",
        "line.",
        "",
    ]
    document = "\n".join(lines)

    if args.write:
        with io.open(args.out, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(document)
        print(
            "wrote %s: %d command name(s), %d alias(es)"
            % (args.out, len(reference_names), len(reference_alias_rows))
        )
        return 0

    failures = 0
    if not os.path.exists(args.out):
        print("FAIL  no document at {}".format(args.out))
        failures += 1
    elif read(args.out) != document:
        print(
            "FAIL  {} is out of date; regenerate with python tools/parity-commands.py --write".format(
                args.out
            )
        )
        failures += 1

    for name in missing:
        print("FAIL  /{}: the reference dispatches it ({}) and this repo does not".format(name, origin[name]))
        failures += 1
    for name in extra:
        print("FAIL  /{}: this repo answers it and the reference does not".format(name))
        failures += 1
    for alias, problem in alias_problems:
        print("FAIL  alias {}: {}".format(alias, problem))
        failures += 1

    if failures:
        return 1

    print(
        "the command surface matches the reference: {} name(s) over {} command(s), {} alias(es) over {} "
        "map(s)".format(
            len(reference_names),
            len(commands),
            len(reference_alias_rows),
            len({row[2] for row in reference_alias_rows}),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
