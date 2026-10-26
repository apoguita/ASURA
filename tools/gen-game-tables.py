#!/usr/bin/env python3
"""Generate crates/asura-protocol/src/client/game_tables.rs from the GWPS reference.

Why a generator: the game (world-server) family is 682 messages -- 194 client headers and 488
server headers -- whose header values and sizes must match GWPS *exactly*, because a wrong size in
the framing code desynchronises the stream and the client answers with nothing but the opaque `007`
error (docs/03-protocol.md, sections 1 and 6). Retyping 682 numbers by hand invites a silent
single-digit error; reading them from the C source does not.

This is the game-family twin of tools/gen-auth-tables.py: same sources, same checks, same output
shape, but a table that cannot share the auth table's lookup, because the two families give the same
header value two different meanings (`0x8004` is `AUTH_CMSG_ACCOUNT_LOGIN` on the auth connection and
a different message on the world connection). Hence GAME_TABLE with a `from_client` argument on every
lookup, and `lookup` / `expected_size` / `fields` here against `entry` / `expected_size` there.

Sources, all in the GWPS checkout (sibling repository, docs/adr/0005-gwps-reference-only.md):

  code/OpenTyria/opcodes.h   - the names, the counts, and the direction masks (GAME_CMSG_MASK 0x8000)
  code/OpenTyria/msgdefs.c   - GAME_CMSG_FORMATS[194] / GAME_SMSG_FORMATS[488]: one row per message
                               naming its field array and its `max_size`
  code/OpenTyria/msgpack.c   - the field widths, and the wire form of a counted field

The wire header is mask | index: 0x8000 for client->server, 0x0000 for server->client.

`max_size` is a maximum, not the wire size: it is the field sum with every counted field at full
capacity and a four-byte count, while the wire form of a counted field is a u16 count followed by
exactly that many units and no padding (msgpack.c `pack_helper`; measured, docs/03-protocol.md).

Usage:
  python tools/gen-game-tables.py [--gwps C:\\path\\to\\PreSearing] [--out PATH] [--check]

  --check exits non-zero if the generated file would differ from what is on disk. CI-shaped; run it
  before trusting that the table still matches the reference.

This file stays ASCII-only, like every other script in tools/: a BOM-less non-ASCII file is a
scripting trap on Windows (docs/HANDOFF.md).
"""

import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_GWPS = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_OUT = os.path.join(REPO, "crates", "asura-protocol", "src", "client", "game_tables.rs")

NAME_RE = re.compile(
    r"#define\s+GAME_([CS])MSG_([A-Z0-9_]+)\s+"
    r"\(GAME_([CS])MSG_MASK\s*\|\s*0x([0-9A-Fa-f]{4})\)\s*//\s*(\d+)"
)
COUNT_RE = re.compile(
    r"#define\s+(GAME_(?:C|S)MSG_COUNT|GAME_(?:C|S)MSG_MASK)\s+\(?\s*(0x[0-9A-Fa-f]+|\d+)\s*\)?"
)
ENTRY_RE = re.compile(r"\{\s*(\d+)\s*,\s*(\d+)\s*,\s*(GAME_[CS]MSG_\w+)\s*,\s*(\d+)\s*\}")
FIELD_ARRAY_RE = re.compile(r"MsgField\s+(GAME_[CS]MSG_\d+)\[(\d+)\]\s*=\s*\{(.*?)\};", re.DOTALL)
FIELD_RE = re.compile(r"\{\s*(TYPE_[A-Z_0-9]+)\s*,\s*(\d+)\s*\}")

# On-wire width of each field type, in bytes, read from pack_helper in msgpack.c. Only the fixed
# kinds are listed: the counted kinds and the blob depend on `length`, below.
FIXED_WIDTH = {
    "TYPE_MSG_HEADER": 2,
    "TYPE_MSG_HEADER_GAME": 4,
    "TYPE_AGENT_ID": 4,
    "TYPE_FLOAT": 4,
    "TYPE_VECT2": 8,
    "TYPE_VECT3": 12,
    "TYPE_BYTE": 1,
    "TYPE_WORD": 2,
    "TYPE_DWORD": 4,
}
# kind -> bytes per unit of a counted field. The wire form is a u16 count then `count` units and
# nothing else (msgpack.c, pack_helper and unpack_helper): no padding to the declared capacity.
COUNTED_WIDTH = {
    "TYPE_STRING_16": 2,
    "TYPE_ARRAY_16": 2,
    "TYPE_ARRAY_8": 1,
    "TYPE_ARRAY_32": 4,
}
# The reference's own arithmetic for `max_size` writes the count as a u32, not a u16, and pads every
# counted field to its declarative capacity. That is what makes `max_size` a maximum rather than a
# wire size, and it is why `size` in the table below is two bytes per counted field larger than the
# biggest message the packer can produce.
MAX_COUNT_PREFIX = 4
NESTED = "TYPE_NESTED_STRUCT"


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return handle.read()


def parse_counts(opcodes_text):
    counts = {}
    for macro, value in COUNT_RE.findall(opcodes_text):
        counts[macro] = int(value, 0)
    for required in ("GAME_CMSG_COUNT", "GAME_SMSG_COUNT", "GAME_CMSG_MASK", "GAME_SMSG_MASK"):
        if required not in counts:
            sys.exit("error: %s is missing from opcodes.h" % required)
    return counts


def parse_names(opcodes_text):
    """index -> constant name, per direction, plus the indices opcodes.h gives two names.

    opcodes.h is the authority for names. A name and the mask it is built from must agree about
    the direction -- `GAME_CMSG_X (GAME_SMSG_MASK | 0x0010)` would put a client header at a server
    header value, and nothing downstream could tell. An index with two names is a collision in the
    reference (it does happen: 0x00E3 is both SKILL_CANCEL and SKILL_ACTIVATED); the first name
    wins, exactly as the auth generator does, and the pair is reported so the table can say so.
    """
    names = {"c": {}, "s": {}}
    aliases = {"c": {}, "s": {}}
    for name_char, rest, mask_char, index_hex, index_dec in NAME_RE.findall(opcodes_text):
        name = "GAME_%sMSG_%s" % (name_char, rest)
        if name_char != mask_char:
            sys.exit(
                "error: %s is built from GAME_%sMSG_MASK, so its direction disagrees with its name"
                % (name, mask_char)
            )
        index = int(index_hex, 16)
        if int(index_dec, 10) != index:
            sys.exit("error: %s disagrees with itself: 0x%04X vs // %s" % (name, index, index_dec))
        direction = "c" if name_char == "C" else "s"
        if index in names[direction] and names[direction][index] != name:
            print(
                "warning: %s index %d has two names (%s, %s); keeping the first"
                % ("client" if direction == "c" else "server",
                   index, names[direction][index], name),
                file=sys.stderr,
            )
            aliases[direction][index] = name
            continue
        names[direction][index] = name
    return names, aliases


def parse_format_table(msgdefs_text, array_name, expected_count):
    """Return index -> (field array name, max_size) for one GAME_*_FORMATS table."""
    start = msgdefs_text.find("MsgFormat %s[" % array_name)
    if start < 0:
        sys.exit("error: %s not found in msgdefs.c" % array_name)
    end = msgdefs_text.find("};", start)
    if end < 0:
        sys.exit("error: unterminated %s" % array_name)
    body = msgdefs_text[start:end]

    rows = {}
    for index, _fields, fields_name, max_size in ENTRY_RE.findall(body):
        index = int(index, 10)
        if index in rows:
            sys.exit("error: %s lists index %d twice" % (array_name, index))
        rows[index] = (fields_name, int(max_size, 10))

    if len(rows) != expected_count:
        sys.exit(
            "error: %s has %d entries, opcodes.h says %d"
            % (array_name, len(rows), expected_count)
        )
    missing = sorted(set(range(expected_count)) - set(rows))
    if missing:
        sys.exit("error: %s is missing indices %s" % (array_name, missing))
    return rows


def parse_field_arrays(msgdefs_text):
    """name -> [(TYPE_, length)] for every MsgField array."""
    arrays = {}
    for name, count, body in FIELD_ARRAY_RE.findall(msgdefs_text):
        fields = [(kind, int(length, 10)) for kind, length in FIELD_RE.findall(body)]
        if len(fields) != int(count, 10):
            sys.exit("error: %s declares %s fields, lists %d" % (name, count, len(fields)))
        arrays[name] = fields
    return arrays


def check_names_against_formats(names, tables, counts):
    """Fail loudly if opcodes.h and msgdefs.c disagree about which messages exist.

    Both directions of the comparison, because either one alone hides a dropped message: an opcode
    with no format entry is a header the packet could carry but nothing can size, and the auth
    generator's per-index `None` name is only acceptable in the reverse case -- an index the client
    can send that opcodes.h does not name.
    """
    unnamed = {"c": 0, "s": 0}
    for direction, table, array_name in (
        ("c", tables["c"], "GAME_CMSG_FORMATS"),
        ("s", tables["s"], "GAME_SMSG_FORMATS"),
    ):
        label = "client" if direction == "c" else "server"
        for index in sorted(names[direction]):
            if index not in table:
                sys.exit(
                    "error: %s (index %d, header 0x%04X) has an opcode in opcodes.h but no entry "
                    "in %s" % (names[direction][index], index, counts["GAME_%sMSG_MASK" % direction.upper()] | index, array_name)
                )
        for index in sorted(table):
            if index not in names[direction]:
                unnamed[direction] += 1
    return unnamed


def check_field_arrays_are_used(arrays, tables):
    """Fail loudly on a field array nothing references, or a reference with no array.

    A silent skip is the failure mode this guards: 682 rows and 682 arrays is a bijection, so a
    count that does not line up means the reference grew a message and this generator did not grow
    with it.
    """
    referenced = set()
    for direction in ("c", "s"):
        for index, (fields_name, _max_size) in tables[direction].items():
            if fields_name not in arrays:
                sys.exit("error: %s is referenced by index %d but not defined" % (fields_name, index))
            if fields_name in referenced:
                sys.exit("error: %s is referenced by more than one format entry" % fields_name)
            referenced.add(fields_name)
    orphaned = sorted(set(arrays) - referenced)
    if orphaned:
        sys.exit(
            "error: %d MsgField array(s) are defined but no format entry uses them (e.g. %s); "
            "the reference and this generator disagree about which messages exist"
            % (len(orphaned), ", ".join(orphaned[:5]))
        )


def declared_max_size(fields):
    """The reference's own arithmetic for a row's `max_size`, from its field list.

    Fixed kinds add their width, a blob adds its length, a counted field adds a four-byte count
    plus `length` units at full capacity (MAX_COUNT_PREFIX, above), and a nested struct adds a
    four-byte count plus `length` repeats of everything after it, evaluated by the same rule.

    This is NOT the wire size. It is what `msgdefs.c` declares, and recomputing it here is the only
    way to notice that the reference's numbers are self-consistent -- 680 of the 682 rows are, and
    the two that are not are reported rather than smoothed over.
    """
    total = 0
    for position, (kind, length) in enumerate(fields):
        if kind in FIXED_WIDTH:
            total += FIXED_WIDTH[kind]
        elif kind == "TYPE_BLOB":
            total += length
        elif kind in COUNTED_WIDTH:
            total += MAX_COUNT_PREFIX + length * COUNTED_WIDTH[kind]
        elif kind == NESTED:
            total += MAX_COUNT_PREFIX + length * declared_max_size(fields[position + 1:])
            break
        else:
            sys.exit("error: unknown field type %s in the reference" % kind)
    return total


def gwps_revision(gwps):
    try:
        out = subprocess.run(
            ["git", "-C", gwps, "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            check=False,
        )
        return out.stdout.strip() or "unknown"
    except OSError:
        return "unknown"


def rust_string(value):
    return '"%s"' % value.replace("\\", "\\\\").replace('"', '\\"')


def kind_to_rust(kind):
    return "".join(part.capitalize() for part in kind.replace("TYPE_", "").lower().split("_"))


def add_slice_const(add, declaration, elements):
    """Emit a `&[...]` constant, one element per line when the single line would be too wide.

    rustfmt is held to 100 columns, and this file is checked against it: a generated line that
    rustfmt wants to rewrap would make `cargo fmt` and `--check` disagree forever, the same trap
    the big table avoids with `#[rustfmt::skip]`.
    """
    one_line = "%s = &[%s];" % (declaration, ", ".join(elements))
    if len(one_line) <= 100 or not elements:
        add(one_line)
        return
    add("%s = &[" % declaration)
    for element in elements:
        add("    %s," % element)
    add("];")


def generate(gwps):
    opcodes = read(os.path.join(gwps, "code", "OpenTyria", "opcodes.h"))
    msgdefs = read(os.path.join(gwps, "code", "OpenTyria", "msgdefs.c"))

    counts = parse_counts(opcodes)
    names, aliases = parse_names(opcodes)
    arrays = parse_field_arrays(msgdefs)
    tables = {
        "c": parse_format_table(msgdefs, "GAME_CMSG_FORMATS", counts["GAME_CMSG_COUNT"]),
        "s": parse_format_table(msgdefs, "GAME_SMSG_FORMATS", counts["GAME_SMSG_COUNT"]),
    }
    unnamed = check_names_against_formats(names, tables, counts)
    check_field_arrays_are_used(arrays, tables)

    cmsg_mask = counts["GAME_CMSG_MASK"]
    smsg_mask = counts["GAME_SMSG_MASK"]

    rows = []  # (wire header, direction char, index, max_size, name, fields)
    size_mismatches = []  # (header, direction char, index, declared, recomputed)
    for direction, table, mask in (("c", tables["c"], cmsg_mask), ("s", tables["s"], smsg_mask)):
        label = "client" if direction == "c" else "server"
        for index in sorted(table):
            fields_name, max_size = table[index]
            fields = arrays[fields_name]
            recomputed = declared_max_size(fields)
            if recomputed != max_size:
                size_mismatches.append((mask | index, direction, index, max_size, recomputed))
                print(
                    "warning: %s (index %d, %s) declares %d bytes but its own field layout adds up "
                    "to %d; carrying %d through as declared" % (fields_name, index, label, max_size, recomputed, max_size),
                    file=sys.stderr,
                )
            rows.append((mask | index, direction, index, max_size, names[direction].get(index), fields))
    rows.sort(key=lambda row: (row[1] != "c", row[2]))  # client headers first, then ascending

    named_count = sum(1 for row in rows if row[4] is not None)
    index_width = max(len(str(row[2])) for row in rows)
    size_width = max(4, max(len(str(row[3])) for row in rows))
    ambiguous = sorted(
        (mask | index, direction, names[direction][index], alias)
        for direction, mask in (("c", cmsg_mask), ("s", smsg_mask))
        for index, alias in aliases[direction].items()
    )

    lines = []
    add = lines.append

    add("//! The game (world) message table, generated from the GWPS reference. DO NOT EDIT BY HAND.")
    add("//!")
    add("//! Generated by `tools/gen-game-tables.py` from the sibling GWPS checkout")
    add("//! (`docs/adr/0005-gwps-reference-only.md`), revision %s:" % gwps_revision(gwps))
    add("//!")
    add("//! ```text")
    add("//! code/OpenTyria/opcodes.h   names, counts and direction masks")
    add("//! code/OpenTyria/msgdefs.c   GAME_CMSG_FORMATS / GAME_SMSG_FORMATS (max_size), and the")
    add("//!                            per-message MsgField arrays (the field layout)")
    add("//! code/OpenTyria/msgpack.c   field widths, and the wire form of a counted field")
    add("//! ```")
    add("//!")
    add("//! Regenerate with `python tools/gen-game-tables.py`; verify with `--check`. The table is a")
    add("//! copied fact, not a design: a size disagreement with GWPS is what desynchronises the")
    add("//! stream, and the client itself reports nothing but error 007 (`docs/03-protocol.md`,")
    add("//! section 1).")
    add("//!")
    add("//! ## What `size` is")
    add("//!")
    add("//! [`Entry::size`] is the reference's `max_size`: a **maximum**, not the size the message")
    add("//! occupies on the wire. The reference computes it as the field sum with every counted")
    add("//! field at full capacity behind a four-byte count -- that is, the size of the struct it")
    add("//! unpacks into, not of the packet.")
    add("//!")
    add("//! A counted field on the wire -- `TYPE_STRING_16`, `TYPE_ARRAY_8`, `TYPE_ARRAY_16`,")
    add("//! `TYPE_ARRAY_32` -- is a **u16 count followed by exactly that many units and nothing")
    add("//! else**: no padding out to the capacity the field layout declares, and no four-byte")
    add("//! count (`msgpack.c`, `pack_helper` and `unpack_helper`). This is a measured fact in this")
    add("//! project, recorded in `docs/03-protocol.md` section 1 and section 6: a real client sends")
    add("//! `AUTH_CMSG_SEND_COMPUTER_INFO` as 42 bytes where the auth table declares 138.")
    add("//!")
    add("//! The consequences for framing code, which are the reason this paragraph is here:")
    add("//!")
    add("//! * `size` is an upper bound, two bytes per counted field looser than the largest message")
    add("//!   the packer can produce. It is never \"read exactly this many bytes\".")
    add("//! * A body can only be consumed structurally, field by field, following the counted")
    add("//!   lengths as they are read. No header fixes the length of a message with a counted")
    add("//!   field, and nothing on the wire delimits the body.")
    add("//! * [`declared_max_size`] recomputes `size` from a row's own field layout, so a wrong")
    add("//!   number cannot survive a test. Every row reproduces exactly except the two listed in")
    add("//!   [`SIZE_NOT_RECOMPUTABLE_FROM_LAYOUT`].")
    add("//!")
    add("//! ## Two things the reference does not answer")
    add("//!")
    add("//! %d of the %d indices below have no name in `opcodes.h`; they carry the reference's" % (unnamed["c"] + unnamed["s"], len(rows)))
    add("//! size and stay `None`-named, so an unexpected-but-real header is still recognised instead")
    add("//! of being mistaken for a malformed message. And %d header%s two names in `opcodes.h`;" % (len(ambiguous), " carries" if len(ambiguous) == 1 else "s carry"))
    add("//! see [`AMBIGUOUS_NAME_HEADERS`] and the alias constant beside the name it collides with.")
    add("")
    add("/// `GAME_CMSG_MASK`: set on client-to-server headers, and therefore also the direction bit.")
    add("pub const GAME_CMSG_MASK: u16 = 0x%04X;" % cmsg_mask)
    add("/// `GAME_SMSG_MASK`: server-to-client headers carry no direction bit.")
    add("pub const GAME_SMSG_MASK: u16 = 0x%04X;" % smsg_mask)
    add("")
    add("/// Number of client-to-server game headers (`GAME_CMSG_COUNT`).")
    add("pub const GAME_CMSG_COUNT: usize = %d;" % counts["GAME_CMSG_COUNT"])
    add("/// Number of server-to-client game headers (`GAME_SMSG_COUNT`).")
    add("pub const GAME_SMSG_COUNT: usize = %d;" % counts["GAME_SMSG_COUNT"])
    add("")
    add("/// A field kind, as `msgdefs.c` names it (`FieldType` in `msgdefs.h`).")
    add("///")
    add("/// The same kinds the auth table uses, and the same field data on the wire, but the counted")
    add("/// kinds are documented for what they are: a u16 count then that many units, no padding.")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub enum FieldKind {")
    for kind, doc in (
        ("MsgHeader", "`TYPE_MSG_HEADER`: the u16 header, two bytes on the wire."),
        ("MsgHeaderGame", "`TYPE_MSG_HEADER_GAME`: the header plus a zero u16, four bytes on the wire.\n    /// Defined, packed and unpacked in the reference, but **no** row of either game format\n    /// table uses it -- opt-in per message, and nothing is opted in. Kept here so a capture\n    /// that shows one can be named."),
        ("AgentId", "`TYPE_AGENT_ID`: a four-byte agent number."),
        ("Float", "`TYPE_FLOAT`."),
        ("Vect2", "`TYPE_VECT2`: two floats."),
        ("Vect3", "`TYPE_VECT3`: three floats."),
        ("Byte", "`TYPE_BYTE`."),
        ("Word", "`TYPE_WORD`."),
        ("Dword", "`TYPE_DWORD`."),
        ("Blob", "`TYPE_BLOB`: `length` opaque bytes, no count prefix."),
        ("String16", "`TYPE_STRING_16`: a u16 count then that many u16 units. `length` is the capacity,\n    /// not the length on the wire."),
        ("Array8", "`TYPE_ARRAY_8`: a u16 count then that many u8 units."),
        ("Array16", "`TYPE_ARRAY_16`: a u16 count then that many u16 units."),
        ("Array32", "`TYPE_ARRAY_32`: a u16 count then that many u32 units."),
        ("NestedStruct", "`TYPE_NESTED_STRUCT`: a u8 count then that many repeats of the remaining fields.\n    /// `length` is the maximum count."),
    ):
        add("    /// %s" % doc)
        add("    %s," % kind)
    add("}")
    add("")
    add("/// One field: its kind and, for the kinds that carry a count or a length, that number.")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub struct Field {")
    add("    pub kind: FieldKind,")
    add("    /// `field.length` in the reference: the unit capacity, the blob size, or 0 when unused.")
    add("    pub length: u16,")
    add("}")
    add("")
    add("impl Field {")
    add("    /// Bytes this field occupies on the wire when a counted field carries `count` units, or")
    add("    /// `None` for [`FieldKind::NestedStruct`], whose size depends on the repeats it carries.")
    add("    ///")
    add("    /// `count` is ignored by every kind that has no count of its own. A counted field is a")
    add("    /// two-byte count then `count` units and nothing else, so even at capacity it is two")
    add("    /// bytes smaller than the field's share of [`declared_max_size`]: the declared maximum")
    add("    /// bounds the wire, it is not the wire.")
    add("    pub fn wire_size(&self, count: u16) -> Option<usize> {")
    add("        let n = self.length as usize;")
    add("        let count = count as usize;")
    add("        match self.kind {")
    add("            FieldKind::MsgHeader => Some(2),")
    add("            FieldKind::MsgHeaderGame => Some(4),")
    add("            FieldKind::AgentId | FieldKind::Float | FieldKind::Dword => Some(4),")
    add("            FieldKind::Vect2 => Some(8),")
    add("            FieldKind::Vect3 => Some(12),")
    add("            FieldKind::Byte => Some(1),")
    add("            FieldKind::Word => Some(2),")
    add("            FieldKind::Blob => Some(n),")
    add("            // The capacity is only a maximum; the count the body carries decides the wire,")
    add("            // and the reference rejects a count above the capacity (`pack_helper`).")
    add("            FieldKind::String16 | FieldKind::Array16 => Some(2 + count * 2),")
    add("            FieldKind::Array8 => Some(2 + count),")
    add("            FieldKind::Array32 => Some(2 + count * 4),")
    add("            FieldKind::NestedStruct => None,")
    add("        }")
    add("    }")
    add("}")
    add("")
    add("/// The value `msgdefs.c` derives a row's `max_size` from: the reference's own arithmetic,")
    add("/// with every counted field at full capacity behind a four-byte count.")
    add("///")
    add("/// This is *not* a wire size -- see the module documentation. It exists so the table can")
    add("/// check itself: for every row, `declared_max_size(entry.fields) == entry.size`, with the")
    add("/// two exceptions in [`SIZE_NOT_RECOMPUTABLE_FROM_LAYOUT`].")
    add("pub fn declared_max_size(fields: &[Field]) -> usize {")
    add("    let mut total = 0;")
    add("    for (position, field) in fields.iter().enumerate() {")
    add("        let n = field.length as usize;")
    add("        total += match field.kind {")
    add("            FieldKind::MsgHeader => 2,")
    add("            FieldKind::MsgHeaderGame => 4,")
    add("            FieldKind::AgentId | FieldKind::Float | FieldKind::Dword => 4,")
    add("            FieldKind::Vect2 => 8,")
    add("            FieldKind::Vect3 => 12,")
    add("            FieldKind::Byte => 1,")
    add("            FieldKind::Word => 2,")
    add("            FieldKind::Blob => n,")
    add("            FieldKind::String16 | FieldKind::Array16 => 4 + n * 2,")
    add("            FieldKind::Array8 => 4 + n,")
    add("            FieldKind::Array32 => 4 + n * 4,")
    add("            FieldKind::NestedStruct => 4 + n * declared_max_size(&fields[position + 1..]),")
    add("        };")
    add("        if field.kind == FieldKind::NestedStruct {")
    add("            break;")
    add("        }")
    add("    }")
    add("    total")
    add("}")
    add("")
    add("/// One row of the game message table.")
    add("#[derive(Debug, Clone, Copy)]")
    add("pub struct Entry {")
    add("    /// The wire value, mask included: `0x8000 | index` or `0x0000 | index`.")
    add("    ///")
    add("    /// The same value means a different message in the auth family, which is why every")
    add("    /// lookup below also takes the direction.")
    add("    pub header: u16,")
    add("    /// The index GWPS uses to address its format array (`header & 0x7FFF`).")
    add("    pub index: u16,")
    add("    /// The declared maximum, `max_size` in `msgdefs.c` -- **not** the size on the wire.")
    add("    ///")
    add("    /// Two bytes per counted field more than the packer can ever produce. See the module")
    add("    /// documentation; a framer must not read it as a body length.")
    add("    pub size: usize,")
    add("    /// The GWPS name, when `opcodes.h` gives this index one; `None` for the %d indices it" % (unnamed["c"] + unnamed["s"]))
    add("    /// does not name.")
    add("    pub name: Option<&'static str>,")
    add("    /// Whether the header is client-to-server (`0x8000` set).")
    add("    pub from_client: bool,")
    add("    /// The fields the message is made of, in wire order.")
    add("    pub fields: &'static [Field],")
    add("}")
    add("")

    # Named constants, so calling code reads like the reference instead of like magic numbers.
    add("// ---------------------------------------------------------------- named headers")
    add("// The names are GWPS's own (`opcodes.h`), kept verbatim so a Rust message can be diffed")
    add("// against a C one by name rather than by number. %d of the %d headers have one." % (named_count, len(rows)))
    add("")
    for header, direction, index, size, name, _fields in rows:
        if name is None:
            continue
        add("/// `%s` (declared maximum %d bytes; header `0x%04X`)." % (name, size, header))
        alias = aliases[direction].get(index)
        if alias is not None:
            add("///")
            add("/// `opcodes.h` also gives this header the name `%s`." % alias)
        add("pub const %s: u16 = 0x%04X;" % (name, header))
        if alias is not None:
            add("/// `%s`: a second name for `%s`, the same header `0x%04X`." % (alias, name, header))
            add("pub const %s: u16 = 0x%04X;" % (alias, header))
    add("")

    add("/// Headers `opcodes.h` gives more than one name: `(header, from_client, the name this table")
    add("/// uses, the other name)`.")
    add("///")
    add("/// A collision in the reference, not in this table: one header value, two message names.")
    add("/// Nothing distinguishes them on the wire, so the row keeps the first name, the alias above")
    add("/// keeps the second, and a decoder has to decide from the body which message it has.")
    add("///")
    add("/// `#[rustfmt::skip]` for the same reason the table has it: one row per entry, laid out by")
    add("/// this generator, so `cargo fmt` cannot rewrap it into disagreement with `--check`.")
    add("#[rustfmt::skip]")
    add(
        "pub const AMBIGUOUS_NAME_HEADERS: &[(u16, bool, &str, &str)] = &[%s];"
        % ", ".join(
            "(0x%04X, %s, %s, %s)"
            % (
                header,
                "true" if direction == "c" else "false",
                rust_string(name),
                rust_string(alias),
            )
            for header, direction, name, alias in ambiguous
        )
    )
    add("")

    add("/// Rows whose `size` does not follow from their own field layout: %d of %d." % (len(size_mismatches), len(rows)))
    add("///")
    add("/// The reference wrote these two rows with a two-byte array count where all 680 other rows")
    add("/// use a four-byte one, so each declares two bytes less per counted field than")
    add("/// [`declared_max_size`] recomputes. Both numbers are carried through as declared -- the")
    add("/// reference is the authority for a message's declared maximum, and `max_size` is never the")
    add("/// wire size in any case, so the difference is recorded rather than guessed at.")
    add_slice_const(
        add,
        "pub const SIZE_NOT_RECOMPUTABLE_FROM_LAYOUT: &[(u16, bool)]",
        [
            "(0x%04X, %s)" % (header, "true" if direction == "c" else "false")
            for header, direction, _index, _declared, _recomputed in size_mismatches
        ],
    )
    add("")

    add("/// Every game header, client-to-server first, then server-to-client, ascending by index.")
    add("///")
    add("/// `size` is the declared maximum, not the wire size (module documentation). A body with")
    add("/// counted fields is shorter and only the counts inside it say by how much.")
    add("///")
    add("/// `None`-named entries are indices the client can still send although `opcodes.h` does not")
    add("/// name them: they keep their size so an unexpected-but-real message is still recognised")
    add("/// instead of being mistaken for a malformed one.")
    add("///")
    add("/// `#[rustfmt::skip]` is load-bearing: one row per message, laid out by this generator. With")
    add("/// rustfmt free to rewrap these lines, `cargo fmt` and `--check` would disagree forever.")
    add("#[rustfmt::skip]")
    add("pub const GAME_TABLE: &[Entry] = &[")
    for header, direction, index, size, name, fields in rows:
        layout = ", ".join(
            "Field { kind: FieldKind::%s, length: %d }" % (kind_to_rust(kind), length)
            for kind, length in fields
        )
        add(
            "    Entry { header: 0x%04X, index: %*d, size: %*d, name: %s, from_client: %s, fields: &[%s] },"
            % (
                header,
                index_width,
                index,
                size_width,
                size,
                "None" if name is None else "Some(%s)" % rust_string(name),
                "true" if direction == "c" else "false",
                layout,
            )
        )
    add("];")
    add("")

    add("/// The row for a header in one direction, or `None` if the game family has no such message.")
    add("///")
    add("/// The direction is part of the key because the auth family has headers with the same value:")
    add("/// `0x8004` is `AUTH_CMSG_ACCOUNT_LOGIN` on the auth connection and a different message on")
    add("/// the world connection, with a different size.")
    add("pub fn lookup(header: u16, from_client: bool) -> Option<&'static Entry> {")
    add("    GAME_TABLE")
    add("        .iter()")
    add("        .find(|entry| entry.header == header && entry.from_client == from_client)")
    add("}")
    add("")

    add("/// The declared maximum size for a header in one direction, or `None` if it is not a game")
    add("/// header at all.")
    add("///")
    add("/// A maximum, not the wire size: never read this many bytes to consume a message whose")
    add("/// layout has a counted field (module documentation). A `match` rather than a scan, so")
    add("/// framing costs nothing per message.")
    add("pub const fn expected_size(header: u16, from_client: bool) -> Option<usize> {")
    add("    match (header, from_client) {")
    for header, direction, _index, size, _name, _fields in rows:
        add("        (0x%04X, %s) => Some(%d)," % (header, "true" if direction == "c" else "false", size))
    add("        _ => None,")
    add("    }")
    add("}")
    add("")

    add("/// The field layout for a header in one direction, or `None` if it is not a game header.")
    add("///")
    add("/// The layout is what a decoder walks: the counted lengths inside the body decide where the")
    add("/// next field starts, and the header alone does not.")
    add("pub fn fields(header: u16, from_client: bool) -> Option<&'static [Field]> {")
    add("    match lookup(header, from_client) {")
    add("        Some(entry) => Some(entry.fields),")
    add("        None => None,")
    add("    }")
    add("}")
    add("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Generate the game message table from GWPS.")
    parser.add_argument("--gwps", default=DEFAULT_GWPS, help="path to the PreSearing checkout")
    parser.add_argument("--out", default=DEFAULT_OUT, help="the Rust file to write")
    parser.add_argument(
        "--check",
        action="store_true",
        help="do not write; exit 1 if the file on disk is stale",
    )
    args = parser.parse_args()

    if not os.path.isdir(args.gwps):
        sys.exit("error: GWPS checkout not found at %s (pass --gwps)" % args.gwps)

    generated = generate(args.gwps)

    if args.check:
        current = read(args.out) if os.path.exists(args.out) else ""
        if current.replace("\r\n", "\n") != generated.replace("\r\n", "\n"):
            sys.stderr.write(
                "stale: %s does not match the GWPS reference; run tools/gen-game-tables.py\n"
                % os.path.relpath(args.out, REPO)
            )
            return 1
        print("up to date: %s" % os.path.relpath(args.out, REPO))
        return 0

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(generated)
    print("wrote %s" % os.path.relpath(args.out, REPO))
    return 0


if __name__ == "__main__":
    sys.exit(main())
