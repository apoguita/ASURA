#!/usr/bin/env python3
"""Generate the default-equipment table from the reference: what a new character wears.

`GameSrv_HandleCharCreationChangeProf` answers a profession change with
`GetDefaultEquipments(campaign, profession)` (`GmDefaultArmors.c`), a static table of `GmItem` literals:
armour by piece, a weapon, and for some professions a focus. Retyping 2,858 lines of that is exactly the
kind of transcription the other generators exist to avoid, so it is read out of the reference instead.

The output is `crates/asura-world/src/default_equipment.rs`: one `ItemSpec` per item, grouped by
`(campaign, profession)`. An `ItemSpec` is the same field set the reference's `GmItem` literal has, which is
also what `GAME_SMSG_CREATE_NAMED_ITEM` writes on the wire, so nothing is reinterpreted on the way in.

Sources, all in the GWPS checkout:
  code/OpenTyria/GmDefaultArmors.c   the tables, and the campaign -> table mapping
  code/OpenTyria/GmItem.h            ItemType
  code/OpenTyria/GmColors.h          DyeColor
  code/OpenTyria/GmChar.h            Profession, Campaign

Usage:
  python tools/gen-default-equipment.py [--gwps PATH] [--out PATH] [--check]

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_GWPS = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_OUT = os.path.join(REPO, "crates", "asura-world", "src", "default_equipment.rs")

ITEM_RE = re.compile(r"GmItem\s+(\w+)\[\]\s*=\s*\{", re.MULTILINE)
SLICE_RE = re.compile(r"GmItemSlice\s+(\w+)\[\]\s*=\s*\{(.*?)\};", re.DOTALL)
SLICE_ENTRY_RE = re.compile(r"\[Profession_(\w+)\]\s*=\s*\{\s*(\w+)\s*,")
# The campaign each slice belongs to, read from `GetDefaultEquipments`'s own switch rather than guessed
# from the variable's name.
SWITCH_RE = re.compile(r"case\s+Campaign_(\w+)\s*:\s*\n\s*return\s+(\w+)\[prof\]")
NUM = r"(?:0x[0-9A-Fa-f]+|\d+)"


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return handle.read()


def enum_values(header_text, prefix):
    """`Prefix_Name = 12,` -> {'Name': 12}."""
    out = {}
    for match in re.finditer(r"\b%s_(\w+)\s*=\s*(\d+)\s*," % prefix, header_text):
        out[match.group(1)] = int(match.group(2))
    return out


def braced_block(text, start):
    """The text between the brace at `start` and its partner."""
    depth = 0
    for index in range(start, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[start + 1 : index]
    raise SystemExit("unbalanced braces at %d" % start)


def depth_one_blocks(body):
    """The contents of each depth-one `{ ... }` in `body`.

    A regex cannot do this: an item's `.name = {4, {0x.., 0x..}}` nests two levels, and every
    `\\{[^{}]*\\}`-style pattern matches the inner pair as though it were an item.
    """
    blocks = []
    depth = 0
    start = None
    for index, character in enumerate(body):
        if character == "{":
            if depth == 0:
                start = index
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0 and start is not None:
                blocks.append(body[start + 1 : index])
                start = None
    return blocks


def parse_items(text):
    """array name -> [item dict], from `GmItem <name>[] = { { ... }, ... };`."""
    arrays = {}
    for match in ITEM_RE.finditer(text):
        name = match.group(1)
        body = braced_block(text, text.index("{", match.start()))
        items = []
        for fields in depth_one_blocks(body):
            item = {}
            for field, value in re.findall(
                r"\.(\w+)\s*=\s*(%s|DyeColor_\w+|ItemType_\w+|Profession_\w+)" % NUM, fields
            ):
                item[field] = value
            for field in ("name", "modifiers"):
                counted = re.search(
                    r"\.%s\s*=\s*\{\s*(\d+)\s*,\s*\{([^}]*)\}\s*,?\s*\}" % field, fields
                )
                if counted:
                    units = [part.strip() for part in counted.group(2).split(",") if part.strip()]
                    if len(units) != int(counted.group(1)):
                        raise SystemExit(
                            "%s count %s does not match %d units"
                            % (field, counted.group(1), len(units))
                        )
                    item[field] = units
            if "file_id" not in item:
                continue
            items.append(item)
        arrays[name] = items
    return arrays


def number(value):
    return int(value, 0)


def rustfmt(path):
    """Format the file the way `cargo fmt` would, so `--check` compares like with like."""
    try:
        subprocess.run(["rustfmt", "--edition", "2021", path], check=True, capture_output=True)
    except FileNotFoundError:
        sys.stderr.write("warning: rustfmt not found; the output may not be formatted\n")
    except subprocess.CalledProcessError as error:
        sys.exit("rustfmt failed on %s:\n%s" % (path, error.stderr.decode(errors="replace")))


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

    armors = read(os.path.join(source, "GmDefaultArmors.c"))
    item_types = enum_values(read(os.path.join(source, "GmItem.h")), "ItemType")
    campaigns = enum_values(read(os.path.join(source, "GmChar.h")), "Campaign")
    professions = enum_values(read(os.path.join(source, "GmChar.h")), "Profession")
    dye_colors = enum_values(read(os.path.join(source, "GmColors.h")), "DyeColor")

    items = parse_items(armors)

    # campaign value -> array name -> profession value -> item list
    table = {}
    for campaign_name, slice_name in SWITCH_RE.findall(armors):
        if campaign_name not in campaigns:
            raise SystemExit("unknown campaign %s" % campaign_name)
        slices = {name: body for name, body in SLICE_RE.findall(armors)}
        if slice_name not in slices:
            raise SystemExit("no slice array %s" % slice_name)
        rows = {}
        for profession_name, array_name in SLICE_ENTRY_RE.findall(slices[slice_name]):
            if profession_name not in professions:
                raise SystemExit("unknown profession %s" % profession_name)
            rows[professions[profession_name]] = items[array_name]
        table[campaigns[campaign_name]] = rows

    def item_rust(item):
        kind = item["item_type"].split("_", 1)[1]
        if kind not in item_types:
            raise SystemExit("unknown item type %s" % kind)
        dye = item.get("dye_colors", "DyeColor_None")
        if dye.startswith("DyeColor_"):
            dye_value = dye_colors[dye[len("DyeColor_"):]]
        else:
            dye_value = number(dye)
        profession = item.get("profession", "Profession_None").split("_", 1)[1]
        if profession not in professions:
            raise SystemExit("unknown profession %s" % profession)
        name = ", ".join(item.get("name", []))
        modifiers = ", ".join(item.get("modifiers", []))
        return (
            "ItemSpec {\n"
            "            file_id: %s,\n"
            "            item_type: %d,\n"
            "            dye_tint: %s,\n"
            "            dye_colors: %d,\n"
            "            materials: %s,\n"
            "            unk1: %s,\n"
            "            flags: %s,\n"
            "            value: %s,\n"
            "            model_id: %s,\n"
            "            quantity: %s,\n"
            "            profession: %d,\n"
            "            name: &[%s],\n"
            "            modifiers: &[%s],\n"
            "        }"
            % (
                item["file_id"],
                item_types[kind],
                item.get("dye_tint", "0"),
                dye_value,
                item.get("materials", "0"),
                item.get("unk1", "0"),
                item.get("flags", "0"),
                item.get("value", "0"),
                item.get("model_id", "0"),
                item.get("quantity", "1"),
                professions[profession],
                name,
                modifiers,
            )
        )

    lines = [
        "//! Generated by `tools/gen-default-equipment.py`. Do not edit by hand.",
        "//!",
        "//! What a new character wears, per campaign and profession, read out of the reference's",
        "//! `GetDefaultEquipments` (`GmDefaultArmors.c`). The field set is that table's `GmItem` literal, which",
        "//! is also what `GAME_SMSG_CREATE_NAMED_ITEM` writes, so nothing is reinterpreted on the way in.",
        "",
        "/// One item of a profession's starting set, in the reference's own field order.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct ItemSpec {",
        "    pub file_id: u32,",
        "    pub item_type: u8,",
        "    pub dye_tint: u8,",
        "    pub dye_colors: u16,",
        "    pub materials: u16,",
        "    pub unk1: u8,",
        "    pub flags: u32,",
        "    pub value: u32,",
        "    pub model_id: u32,",
        "    pub quantity: u32,",
        "    pub profession: u8,",
        "    pub name: &'static [u16],",
        "    pub modifiers: &'static [u32],",
        "}",
        "",
        "/// `(campaign, profession, items)` -- the reference's four campaigns, eleven professions.",
        "pub const EQUIPMENT: &[(u8, u8, &[ItemSpec])] = &[",
    ]
    total = 0
    for campaign in sorted(table):
        for profession in sorted(table[campaign]):
            entries = table[campaign][profession]
            total += len(entries)
            lines.append("    // campaign %d, profession %d" % (campaign, profession))
            lines.append("    (")
            lines.append("        %d," % campaign)
            lines.append("        %d," % profession)
            lines.append("        &[")
            for item in entries:
                lines.append("        " + item_rust(item) + ",")
            lines.append("        ],")
            lines.append("    ),")
    lines.append("];")
    lines.append("")
    lines.append("/// The starting set for one campaign and profession, or an empty slice if the reference has")
    lines.append("/// none for that pair.")
    lines.append("pub fn default_equipment(campaign: u8, profession: u8) -> &'static [ItemSpec] {")
    lines.append("    let mut index = 0;")
    lines.append("    while index < EQUIPMENT.len() {")
    lines.append("        let (entry_campaign, entry_profession, items) = &EQUIPMENT[index];")
    lines.append("        if *entry_campaign == campaign && *entry_profession == profession {")
    lines.append("            return items;")
    lines.append("        }")
    lines.append("        index += 1;")
    lines.append("    }")
    lines.append("    &[]")
    lines.append("}")
    lines.append("")
    generated = "\n".join(lines)

    if args.check:
        if not os.path.exists(args.out):
            sys.stderr.write("no generated file at %s\n" % args.out)
            return 1
        import tempfile

        with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False, encoding="utf-8", newline="\n") as handle:
            handle.write(generated)
            temporary = handle.name
        try:
            rustfmt(temporary)
            same = read(temporary) == read(args.out)
        finally:
            os.unlink(temporary)
        if not same:
            sys.stderr.write(
                "the equipment table is out of date with the reference; regenerate with "
                "python tools/gen-default-equipment.py\n"
            )
            return 1
        print("the equipment table is current: %s" % args.out)
        return 0

    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(generated)
    rustfmt(args.out)
    print(
        "wrote %s: %d (campaign, profession) rows, %d items"
        % (args.out, sum(len(table[campaign]) for campaign in table), total)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
