#!/usr/bin/env python3
"""Generate the reference's default inventory: the seed rows, and the capture they came from.

Why: a client that receives no items at all has no inventory model, and dies when it tries to draw the
character it just created. GWPS seeds every character it loads with a fixed set -- a backpack with its
bag item, two cupcakes, six pieces of equipment -- and those items are what its `CREATE_NAMED_ITEM`,
`INVENTORY_CREATE_BAG`, `ITEM_MOVED_TO_LOCATION` and `ITEM_SET_PROFESSION` messages carry.

Until the inventory model existed this was one byte table replayed at the client. Now the messages are
*rows* (`crates/asura-db/src/inventory.rs`), so this tool produces two files from one capture:

  crates/asura-db/src/inventory/seed.rs        the seed: the same six bags and nine items, as data
  crates/asura-protocol/src/game/default_inventory.rs
                                               the captured messages themselves, verbatim, as a fixture

The second is not served any more. It is the *test*: the world server's item block, built out of rows,
has to reproduce those bytes exactly (`crates/asura-world/src/lib.rs`), which is what "the inventory is
the reference's own data" means when checked rather than asserted.

Source: a full GWPS trace (`GWPS_TRACE=1`, see tools/patch-gwps-full-trace.py), specifically the
messages the world server sends between the client's item request (`RECV 0x8091`) and its next request.

Wire layout, measured from the capture and not from `GameSrv_CreateNamedItem`'s struct (whose fields are
not the wire's widths):

  0x0161 CREATE_NAMED_ITEM   u32 item_id, u32 file_id, u8 item_type, u8 dye_tint, u16 dye_colors,
                             u16 materials, u8 unk1, u32 flags, u32 value, u32 model_id, u32 quantity,
                             u16 n_name, u16 name[n], u8 n_modifiers, u32 modifiers[m]
  0x013F INVENTORY_CREATE_BAG u16 stream_id, u8 bag_type, u8 bag_model_id, u16 bag_id, u8 slot_count,
                             u32 assoc_item_id
  0x013E ITEM_MOVED_TO_LOCATION u16 stream_id, u32 item_id, u16 bag_id, u8 slot
  0x015A ITEM_SET_PROFESSION u32 item_id, u8 profession

The modifier count is ONE byte, which is why `sizeof(struct)` is not the message (docs/03-protocol.md
section 11.4). The reference's own packer says so: `TYPE_NESTED_STRUCT` writes a `u8` count
(`msgpack.c`, `pack_helper`).

Usage:
  python tools/gen-default-inventory.py --capture C:\\path\\gwps-capture.log [--out PATH] [--seed-out PATH] [--check]

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import io
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_OUT = os.path.join(
    REPO, "crates", "asura-protocol", "src", "game", "default_inventory.rs"
)
DEFAULT_SEED_OUT = os.path.join(
    REPO, "crates", "asura-db", "src", "inventory", "seed.rs"
)

STEP_RE = re.compile(r"(SEND|RECV) 0x([0-9A-Fa-f]{4})\s+(\d+) bytes\s*:\s*([0-9A-Fa-f ]*)")

# The item family: bags, items, placements, professions. Everything else in that window is a message
# this project builds itself and is left to the world server.
ITEM_HEADERS = (0x013F, 0x0161, 0x013E, 0x015A)

# BagType values (`GmInventory.h`), for the comment beside each seeded bag.
BAG_TYPES = {
    1: "Bag",
    2: "Equipped",
    3: "NotCollected",
    4: "Storage",
    5: "MatStorage",
}
# BagModelId values, likewise.
BAG_MODELS = {
    0: "Backpack",
    1: "BeltPouch",
    2: "Bag1",
    3: "Bag2",
    4: "EquipmentPack",
    5: "MaterialStorage",
    6: "UnclaimedItems",
    7: "Storage1",
    8: "Storage2",
    21: "EquippedItems",
}


def steps(text):
    for line in text.splitlines():
        match = STEP_RE.search(line)
        if not match:
            continue
        kind = match.group(1)
        header = int(match.group(2), 16)
        size = int(match.group(3))
        data = bytes(int(part, 16) for part in match.group(4).split())
        if len(data) != size:
            raise SystemExit(
                "gen-default-inventory: a %d-byte message parsed as %d bytes" % (size, len(data))
            )
        yield kind, header, data


def item_window(text):
    """The item-family messages the world sent while answering the client's item request."""
    collecting = False
    window = []
    for kind, header, data in steps(text):
        if kind == "RECV" and header == 0x8091:
            collecting = True
            continue
        if collecting and kind == "RECV":
            break  # the next request ends the window
        if collecting and header in ITEM_HEADERS:
            window.append((header, data))
    if not window:
        raise SystemExit(
            "gen-default-inventory: no item messages found; is the trace from a real session?"
        )
    return window


class Reader:
    """A little-endian reader over a message body, which refuses to run past the end."""

    def __init__(self, data, what):
        self.data = data
        self.at = 0
        self.what = what

    def take(self, count):
        end = self.at + count
        if end > len(self.data):
            raise SystemExit(
                "gen-default-inventory: %s is %d bytes, wanted %d"
                % (self.what, len(self.data), end)
            )
        chunk = self.data[self.at : end]
        self.at = end
        return chunk

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return int.from_bytes(self.take(2), "little")

    def u32(self):
        return int.from_bytes(self.take(4), "little")

    def done(self):
        return self.at == len(self.data)


def parse_item(data):
    """One `CREATE_NAMED_ITEM` body, field for field."""
    reader = Reader(data, "0x0161")
    item = {
        "id": reader.u32(),
        "file_id": reader.u32(),
        "item_type": reader.u8(),
        "dye_tint": reader.u8(),
        "dye_colors": reader.u16(),
        "materials": reader.u16(),
        "unk1": reader.u8(),
        "flags": reader.u32(),
        "value": reader.u32(),
        "model_id": reader.u32(),
        "quantity": reader.u32(),
    }
    item["name"] = [reader.u16() for _ in range(reader.u16())]
    # One byte, not two: `TYPE_NESTED_STRUCT` on the wire.
    item["modifiers"] = [reader.u32() for _ in range(reader.u8())]
    if not reader.done():
        raise SystemExit(
            "gen-default-inventory: 0x0161 parsed %d of %d bytes; the layout is wrong"
            % (reader.at, len(data))
        )
    return item


def parse_bag(data):
    reader = Reader(data, "0x013F")
    stream_id = reader.u16()
    bag = {
        "stream_id": stream_id,
        "bag_type": reader.u8(),
        "bag_model_id": reader.u8(),
        "bag_id": reader.u16(),
        "slot_count": reader.u8(),
        "assoc_item_id": reader.u32(),
    }
    if not reader.done():
        raise SystemExit(
            "gen-default-inventory: 0x013F parsed %d of %d bytes" % (reader.at, len(data))
        )
    return bag


def parse_move(data):
    reader = Reader(data, "0x013E")
    move = {
        "stream_id": reader.u16(),
        "item_id": reader.u32(),
        "bag_id": reader.u16(),
        "slot": reader.u8(),
    }
    if not reader.done():
        raise SystemExit(
            "gen-default-inventory: 0x013E parsed %d of %d bytes" % (reader.at, len(data))
        )
    return move


def parse_profession(data):
    reader = Reader(data, "0x015A")
    profession = {"item_id": reader.u32(), "profession": reader.u8()}
    if not reader.done():
        raise SystemExit(
            "gen-default-inventory: 0x015A parsed %d of %d bytes" % (reader.at, len(data))
        )
    return profession


def decode(window):
    """The window as bags and placed items, in the order the reference created them."""
    bags = []
    items = []
    by_id = {}
    for header, data in window:
        # The two header bytes are not the payload: every parser below reads a body.
        body = data[2:]
        if header == 0x013F:
            bags.append(parse_bag(body))
        elif header == 0x0161:
            item = parse_item(body)
            if item["id"] in by_id:
                raise SystemExit(
                    "gen-default-inventory: item %d created twice" % item["id"]
                )
            item["bag_id"] = None
            item["slot"] = None
            item["profession"] = 0
            by_id[item["id"]] = item
            items.append(item)
        elif header == 0x013E:
            move = parse_move(body)
            item = by_id.get(move["item_id"])
            if item is None:
                raise SystemExit(
                    "gen-default-inventory: a move names item %d, which was never created"
                    % move["item_id"]
                )
            item["bag_id"] = move["bag_id"]
            item["slot"] = move["slot"]
        elif header == 0x015A:
            profession = parse_profession(body)
            item = by_id.get(profession["item_id"])
            if item is None:
                raise SystemExit(
                    "gen-default-inventory: a profession names item %d, which was never created"
                    % profession["item_id"]
                )
            item["profession"] = profession["profession"]
    if not bags or not items:
        raise SystemExit("gen-default-inventory: the window has no bags or no items")
    return bags, items


def equipped_ids(bags, items):
    """The item ids in the equipped-items bag, by slot, as `UPDATE_AGENT_VISUAL_EQUIPMENT` needs them."""
    equipped = [bag for bag in bags if bag["bag_model_id"] == BAG_MODELS_INVERSE["EquippedItems"]]
    if not equipped:
        raise SystemExit(
            "gen-default-inventory: no equipped-items bag in this trace, so the capture says nothing "
            "about what the character wears"
        )
    bag_id = equipped[0]["bag_id"]
    ids = [0] * 9
    for item in items:
        if item["bag_id"] != bag_id or item["slot"] is None:
            continue
        if item["slot"] >= len(ids):
            raise SystemExit(
                "gen-default-inventory: item %d is at slot %d of the equipped-items bag, which holds %d"
                % (item["id"], item["slot"], len(ids))
            )
        ids[item["slot"]] = item["id"]
    if not any(ids):
        raise SystemExit(
            "gen-default-inventory: the equipped-items bag is empty in this trace"
        )
    return ids


BAG_MODELS_INVERSE = {name: value for value, name in BAG_MODELS.items()}


def render_capture(window, bags, items):
    """The captured messages themselves, as the fixture the rows have to reproduce."""
    lines = []
    lines.append("//! The reference's item messages, captured from a GWPS trace. DO NOT EDIT BY HAND.")
    lines.append("//!")
    lines.append("//! Generated by `tools/gen-default-inventory.py` from a full-byte trace of the")
    lines.append("//! reference (`GWPS_TRACE=1`) working: the `CREATE_NAMED_ITEM`, `INVENTORY_CREATE_BAG`,")
    lines.append("//! `ITEM_MOVED_TO_LOCATION` and `ITEM_SET_PROFESSION` messages it sends for the character")
    lines.append("//! it loads. %d messages." % len(window))
    lines.append("//!")
    lines.append("//! **This is a fixture, not what the server sends.** The same window is generated as rows")
    lines.append("//! (`crates/asura-db/src/inventory/seed.rs`), and the world server builds the item block")
    lines.append("//! from the rows. These bytes are the test: the block built from rows has to reproduce")
    lines.append("//! them, message for message and byte for byte (`crates/asura-world/src/lib.rs`,")
    lines.append("//! `the_item_block_from_rows_is_the_capture`). If this file and the seed ever disagree")
    lines.append("//! about what the reference sends, that test is what says so.")
    lines.append("//!")
    lines.append("//! Regenerate with `python tools/gen-default-inventory.py --capture <trace>`; verify with")
    lines.append("//! `--check`.")
    lines.append("")
    lines.append("/// The reference's item messages for its default character, in its order.")
    lines.append("///")
    lines.append("/// Each entry is a whole message: the header, then the body.")
    # rustfmt would reflow these one-line entries and `cargo fmt` runs in the battery, so the generated
    # file must be skipped by it -- otherwise `--check` reports staleness that is only formatting, which
    # is a false alarm that trains a reader to ignore the check.
    lines.append("#[rustfmt::skip]")
    lines.append("pub const REFERENCE_ITEM_MESSAGES: &[(u16, &[u8])] = &[")
    for header, data in window:
        body = ", ".join("0x%02X" % byte for byte in data[2:])
        lines.append("    (0x%04X, &[%s])," % (header, body))
    lines.append("];")
    lines.append("")
    lines.append("/// The item ids in the equipped-items bag, by slot: what `UPDATE_AGENT_VISUAL_EQUIPMENT`")
    lines.append("/// carries, and what makes a character visible with its armour on. Read out of the")
    lines.append("/// messages above, so the server and this file are checked against the same capture.")
    ids = equipped_ids(bags, items)
    lines.append("pub const REFERENCE_EQUIPPED_ITEM_IDS: [u32; 9] = [%s];" % ", ".join(str(i) for i in ids))
    lines.append("")
    return "\n".join(lines)


def render_seed(bags, items):
    """The same window as rows: what the server seeds a character with, and then reads back."""
    lines = []
    lines.append("//! The reference's default inventory, as seed rows. DO NOT EDIT BY HAND.")
    lines.append("//!")
    lines.append("//! Generated by `tools/gen-default-inventory.py` from the same capture as")
    lines.append("//! `asura_protocol::game::default_inventory`: the six bags and %d items GWPS creates for the" % len(items))
    lines.append("//! character it loads, with its own ids, file ids, models, flags and placements.")
    lines.append("//!")
    lines.append("//! Why the reference's data rather than invented items: a client given no items has no")
    lines.append("//! inventory model and dies when it draws the character it has just created, and every")
    lines.append("//! field here (a file id into `Gw.dat`, a model id, a flag word) is the reference's data and")
    lines.append("//! not something this project can derive.")
    lines.append("//!")
    lines.append("//! The ids are the capture's. `asura_db::inventory::seed_default` renumbers them from the")
    lines.append("//! item sequence when it writes them, because an item id is a runtime allocation in the")
    lines.append("//! reference too (`GameSrv_AllocateItem`) and two characters cannot both own item 1. They are")
    lines.append("//! kept here so the item that *is* the backpack can be named, and so a fresh database --")
    lines.append("//! whose sequence starts at one -- reproduces the capture exactly.")
    lines.append("//!")
    lines.append("//! Regenerate with `python tools/gen-default-inventory.py --capture <trace>`; verify with")
    lines.append("//! `--check`.")
    lines.append("")
    lines.append("use crate::inventory::{SeedBag, SeedItem};")
    lines.append("")
    lines.append("/// The bags, in the order the reference sends them: its bag array's order, which is by bag")
    lines.append("/// model id (backpack, material storage, unclaimed, storage, equipped items).")
    lines.append("#[rustfmt::skip]")
    lines.append("pub const BAGS: &[SeedBag] = &[")
    for bag in bags:
        lines.append(
            "    // %s: BagType_%s, %d slots"
            % (
                BAG_MODELS.get(bag["bag_model_id"], "model %d" % bag["bag_model_id"]),
                BAG_TYPES.get(bag["bag_type"], "type %d" % bag["bag_type"]),
                bag["slot_count"],
            )
        )
        lines.append(
            "    SeedBag { bag_id: %d, bag_model_id: %d, bag_type: %d, slot_count: %d, assoc_item: %d },"
            % (
                bag["bag_id"],
                bag["bag_model_id"],
                bag["bag_type"],
                bag["slot_count"],
                bag["assoc_item_id"],
            )
        )
    lines.append("];")
    lines.append("")
    lines.append("/// The items, in the order the reference created them -- which is the order the item")
    lines.append("/// sequence hands out ids, so a fresh database keeps the capture's numbering.")
    lines.append("#[rustfmt::skip]")
    lines.append("pub const ITEMS: &[SeedItem] = &[")
    for item in items:
        where = (
            "no slot (the bag's own item)"
            if item["bag_id"] is None
            else "bag %d slot %d" % (item["bag_id"], item["slot"])
        )
        lines.append(
            "    // item %d: type %d, model %d, %s"
            % (item["id"], item["item_type"], item["model_id"], where)
        )
        lines.append(
            "    SeedItem {"
        )
        lines.append(
            "        id: %d, bag_id: %s, slot: %s,"
            % (
                item["id"],
                "None" if item["bag_id"] is None else "Some(%d)" % item["bag_id"],
                "None" if item["slot"] is None else "Some(%d)" % item["slot"],
            )
        )
        lines.append(
            "        file_id: 0x%X, item_type: %d, model_id: %d,"
            % (item["file_id"], item["item_type"], item["model_id"])
        )
        lines.append(
            "        dye_tint: %d, dye_colors: %d, materials: %d, unk1: %d,"
            % (
                item["dye_tint"],
                item["dye_colors"],
                item["materials"],
                item["unk1"],
            )
        )
        lines.append(
            "        flags: 0x%X, value: %d, quantity: %d, profession: %d,"
            % (
                item["flags"],
                item["value"],
                item["quantity"],
                item["profession"],
            )
        )
        name = ", ".join("0x%04X" % unit for unit in item["name"])
        modifiers = ", ".join("0x%08X" % word for word in item["modifiers"])
        lines.append("        name: &[%s]," % name)
        lines.append("        modifiers: &[%s]," % modifiers)
        lines.append("    },")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def write(path, text, check):
    if check:
        if not os.path.exists(path):
            raise SystemExit("gen-default-inventory: %s is missing" % path)
        if io.open(path, encoding="utf-8").read() != text:
            raise SystemExit("gen-default-inventory: %s is stale" % path)
        return False
    directory = os.path.dirname(path)
    if directory and not os.path.isdir(directory):
        os.makedirs(directory)
    io.open(path, "w", encoding="utf-8", newline="\n").write(text)
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--capture", required=True, help="a full GWPS trace")
    parser.add_argument("--out", default=DEFAULT_OUT, help="the captured messages (a fixture)")
    parser.add_argument("--seed-out", default=DEFAULT_SEED_OUT, help="the seed rows")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    text = io.open(args.capture, encoding="utf-8", errors="replace").read()
    window = item_window(text)
    bags, items = decode(window)

    capture = render_capture(window, bags, items)
    seed = render_seed(bags, items)

    if args.check:
        write(args.out, capture, check=True)
        write(args.seed_out, seed, check=True)
        print(
            "up to date: %s (%d messages), %s (%d bags, %d items)"
            % (args.out, len(window), args.seed_out, len(bags), len(items))
        )
        return

    write(args.out, capture, check=False)
    write(args.seed_out, seed, check=False)
    print(
        "wrote %s (%d messages) and %s (%d bags, %d items)"
        % (args.out, len(window), args.seed_out, len(bags), len(items))
    )


if __name__ == "__main__":
    main()

