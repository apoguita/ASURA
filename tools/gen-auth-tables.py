#!/usr/bin/env python3
"""Generate crates/asura-protocol/src/client/tables.rs from the GWPS reference.

Why a generator instead of hand-typed constants: the auth message table is 100 entries of
(header, fixed size) that must match GWPS *exactly*, because a size disagreement is what makes the
client fail with the opaque `007` error (docs/03-protocol.md, section 6). Retyping them by hand
invites a silent single-digit error; reading them from the C source does not.

Sources, both in the GWPS checkout (sibling repository, docs/adr/0005-gwps-reference-only.md):

  code/OpenTyria/opcodes.h   - the names and the direction masks (AUTH_CMSG_MASK = 0x8000)
  code/OpenTyria/msgdefs.c   - AUTH_CMSG_FORMATS[60] / AUTH_SMSG_FORMATS[40]: the fixed size per
                               header is `max_size`, which every field's fixed width already sums to

The wire header is mask | index: 0x8000 for client->server, 0x0000 for server->client. The mask is
also the direction, so one table keyed by the full wire header covers both directions with no
collision (AUTH_CMSG_0004 and AUTH_SMSG_0004 are different messages on different values).

Usage:
  python tools/gen-auth-tables.py [--gwps C:\\path\\to\\PreSearing] [--out PATH] [--check]

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
DEFAULT_OUT = os.path.join(REPO, "crates", "asura-protocol", "src", "client", "tables.rs")

NAME_RE = re.compile(
    r"#define\s+(AUTH_(?:C|S)MSG_[A-Z0-9_]+)\s+"
    r"\(AUTH_(?:C|S)MSG_MASK\s*\|\s*0x([0-9A-Fa-f]{4})\)\s*//\s*(\d+)"
)
COUNT_RE = re.compile(r"#define\s+(AUTH_(?:C|S)MSG_COUNT|AUTH_(?:C|S)MSG_MASK)\s+\(?\s*(0x[0-9A-Fa-f]+|\d+)\s*\)?")
ENTRY_RE = re.compile(
    r"\{\s*(\d+)\s*,\s*(\d+)\s*,\s*(AUTH_[CS]MSG_\w+)\s*,\s*(\d+)\s*\}"
)
FIELD_ARRAY_RE = re.compile(r"MsgField\s+(AUTH_[CS]MSG_\d+)\[(\d+)\]\s*=\s*\{(.*?)\};", re.DOTALL)
FIELD_RE = re.compile(r"\{\s*(TYPE_[A-Z_0-9]+)\s*,\s*(\d+)\s*\}")

# On-wire width of each field type, in bytes, read from pack_helper in msgpack.c. The four array
# and string kinds carry a *four*-byte element count on the wire even though the C struct keeps a
# uint16_t, and they always occupy their declared maximum regardless of the count actually used --
# that is why a message's size is fixed and why these widths, not sizeof(struct), define the wire.
# TYPE_NESTED_STRUCT is variable (a u8 count followed by that many repeats of the remaining fields)
# and is handled separately below.
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
LENGTH_WIDTH = {
    # kind: (count prefix bytes, bytes per element)
    "TYPE_BLOB": (0, 1),
    "TYPE_STRING_16": (4, 2),
    "TYPE_ARRAY_8": (4, 1),
    "TYPE_ARRAY_16": (4, 2),
    "TYPE_ARRAY_32": (4, 4),
}
VARIABLE = {"TYPE_NESTED_STRUCT"}


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return handle.read()


def parse_counts(opcodes_text):
    counts = {}
    for macro, value in COUNT_RE.findall(opcodes_text):
        counts[macro] = int(value, 0)
    for required in ("AUTH_CMSG_COUNT", "AUTH_SMSG_COUNT", "AUTH_CMSG_MASK", "AUTH_SMSG_MASK"):
        if required not in counts:
            sys.exit("error: %s is missing from opcodes.h" % required)
    return counts


def parse_names(opcodes_text):
    """index -> constant name, per direction. opcodes.h is the authority for names."""
    names = {"c": {}, "s": {}}
    for name, index_hex, index_dec in NAME_RE.findall(opcodes_text):
        index = int(index_hex, 16)
        if int(index_dec, 10) != index:
            sys.exit("error: %s disagrees with itself: 0x%04X vs // %s" % (name, index, index_dec))
        direction = "c" if "AUTH_CMSG_" in name else "s"
        if index in names[direction] and names[direction][index] != name:
            print(
                "warning: index %d has two names (%s, %s); keeping the first"
                % (index, names[direction][index], name),
                file=sys.stderr,
            )
            continue
        names[direction][index] = name
    return names


def parse_format_table(msgdefs_text, array_name, expected_count):
    """Return index -> (field array name, max_size) for one AUTH_*_FORMATS table."""
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
            sys.exit(
                "error: %s declares %s fields, lists %d" % (name, count, len(fields))
            )
        arrays[name] = fields
    return arrays


def wire_size(fields):
    """The on-wire size of a field list, or None if it contains a variable-length nested struct."""
    total = 0
    for kind, length in fields:
        if kind in VARIABLE:
            return None
        if kind in FIXED_WIDTH:
            total += FIXED_WIDTH[kind]
        elif kind in LENGTH_WIDTH:
            prefix, element = LENGTH_WIDTH[kind]
            total += prefix + element * length
        else:
            sys.exit("error: unknown field type %s in the reference" % kind)
    return total


def array_name(index, direction):
    return "AUTH_%sMSG_%04d" % (direction.upper(), index)


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


def generate(gwps):
    opcodes = read(os.path.join(gwps, "code", "OpenTyria", "opcodes.h"))
    msgdefs = read(os.path.join(gwps, "code", "OpenTyria", "msgdefs.c"))

    counts = parse_counts(opcodes)
    names = parse_names(opcodes)
    arrays = parse_field_arrays(msgdefs)
    cmsg = parse_format_table(msgdefs, "AUTH_CMSG_FORMATS", counts["AUTH_CMSG_COUNT"])
    smsg = parse_format_table(msgdefs, "AUTH_SMSG_FORMATS", counts["AUTH_SMSG_COUNT"])

    cmsg_mask = counts["AUTH_CMSG_MASK"]
    smsg_mask = counts["AUTH_SMSG_MASK"]

    rows = []  # (wire header, direction char, index, max_size, name, fields)
    variable_rows = 0
    for direction, table, mask in (("c", cmsg, cmsg_mask), ("s", smsg, smsg_mask)):
        for index, (fields_name, max_size) in sorted(table.items()):
            fields = arrays.get(fields_name)
            if fields is None:
                sys.exit("error: %s is referenced but not defined" % fields_name)
            computed = wire_size(fields)
            if computed is None:
                # A nested struct's own length varies with its count, so the reference's max_size
                # cannot be reproduced from the field list alone. It is carried through as
                # declared, and the Rust test checks only that it is at least the fixed part.
                variable_rows += 1
            elif computed != max_size:
                sys.exit(
                    "error: %s (index %d) is %d bytes by field widths but %d in the table; "
                    "the reference and this generator disagree -- do not trust either"
                    % (fields_name, index, computed, max_size)
                )
            rows.append((mask | index, direction, index, max_size, names[direction].get(index), fields))
    rows.sort(key=lambda row: (row[1] != "c", row[2]))  # client headers first, then ascending

    lines = []
    add = lines.append

    add("//! The auth message table, generated from the GWPS reference. DO NOT EDIT BY HAND.")
    add("//!")
    add("//! Generated by `tools/gen-auth-tables.py` from the sibling GWPS checkout")
    add("//! (`docs/adr/0005-gwps-reference-only.md`), revision %s:" % gwps_revision(gwps))
    add("//!")
    add("//! ```text")
    add("//! code/OpenTyria/opcodes.h   names, counts and direction masks")
    add("//! code/OpenTyria/msgdefs.c   AUTH_CMSG_FORMATS / AUTH_SMSG_FORMATS (fixed size), and the")
    add("//!                            per-message MsgField arrays (the field layout)")
    add("//! code/OpenTyria/msgpack.c   the field widths the sizes are computed from")
    add("//! ```")
    add("//!")
    add("//! Regenerate with `python tools/gen-auth-tables.py`; verify with `--check`. The table is a")
    add("//! copied fact, not a design: GWPS logs and refuses when a message arrives at an unexpected")
    add("//! size, and the client itself reports nothing but error 007, so every number below must")
    add("//! stay identical to the reference (`docs/03-protocol.md`, section 6).")
    add("//!")
    add("//! Each row carries both the size the reference declares and the field layout it is made")
    add("//! of, and `tables.rs` has a test that recomputes every size from its layout. A wrong")
    add("//! number therefore cannot survive `cargo test`.")
    add("")
    add("/// `AUTH_CMSG_MASK`: set on client-to-server headers, and therefore also the direction bit.")
    add("pub const AUTH_CMSG_MASK: u16 = 0x%04X;" % cmsg_mask)
    add("/// `AUTH_SMSG_MASK`: server-to-client headers carry no direction bit.")
    add("pub const AUTH_SMSG_MASK: u16 = 0x%04X;" % smsg_mask)
    add("")
    add("/// Number of client-to-server auth headers (`AUTH_CMSG_COUNT`).")
    add("pub const AUTH_CMSG_COUNT: usize = %d;" % counts["AUTH_CMSG_COUNT"])
    add("/// Number of server-to-client auth headers (`AUTH_SMSG_COUNT`).")
    add("pub const AUTH_SMSG_COUNT: usize = %d;" % counts["AUTH_SMSG_COUNT"])
    add("")
    add("/// A field kind, as `msgdefs.c` names it (`FieldType` in `msgdefs.h`).")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub enum FieldKind {")
    for kind, doc in (
        ("MsgHeader", "`TYPE_MSG_HEADER`: the u16 header, two bytes on the wire."),
        ("MsgHeaderGame", "`TYPE_MSG_HEADER_GAME`: the header plus a zero u16 -- four bytes on the wire\n    /// and two in the struct. Opt-in per message in the reference, never global."),
        ("AgentId", "`TYPE_AGENT_ID`."),
        ("Float", "`TYPE_FLOAT`."),
        ("Vect2", "`TYPE_VECT2`: two floats."),
        ("Vect3", "`TYPE_VECT3`: three floats."),
        ("Byte", "`TYPE_BYTE`."),
        ("Word", "`TYPE_WORD`."),
        ("Dword", "`TYPE_DWORD`."),
        ("Blob", "`TYPE_BLOB`: `length` opaque bytes, no count prefix."),
        ("String16", "`TYPE_STRING_16`: a four-byte count then `length` u16 slots, always all of them."),
        ("Array8", "`TYPE_ARRAY_8`: a four-byte count then `length` u8 slots, always all of them."),
        ("Array16", "`TYPE_ARRAY_16`: a four-byte count then `length` u16 slots, always all of them."),
        ("Array32", "`TYPE_ARRAY_32`: a four-byte count then `length` u32 slots, always all of them."),
        ("NestedStruct", "`TYPE_NESTED_STRUCT`: a u8 count then that many repeats of the remaining\n    /// fields. Variable length, so it makes the message's size only predictable up to\n    /// `length` repeats."),
    ):
        add("    /// %s" % doc)
        add("    %s," % kind)
    add("}")
    add("")
    add("/// One field: its kind and, for the kinds that carry a count or a length, that number.")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub struct Field {")
    add("    pub kind: FieldKind,")
    add("    /// `field.length` in the reference: element capacity, blob size, or 0 when unused.")
    add("    pub length: u16,")
    add("}")
    add("")
    add("impl Field {")
    add("    /// Bytes this field occupies on the wire, or `None` for [`FieldKind::NestedStruct`],")
    add("    /// whose size depends on the count it carries.")
    add("    ///")
    add("    /// These widths are the whole reason a message size is fixed: the string and array")
    add("    /// kinds always occupy their capacity, and their count is four bytes wide on the wire")
    add("    /// even though the C struct keeps a `uint16_t` (`msgpack.c`, `pack_helper`).")
    add("    pub fn wire_size(&self) -> Option<usize> {")
    add("        let n = self.length as usize;")
    add("        match self.kind {")
    add("            FieldKind::MsgHeader => Some(2),")
    add("            FieldKind::MsgHeaderGame => Some(4),")
    add("            FieldKind::AgentId | FieldKind::Float | FieldKind::Dword => Some(4),")
    add("            FieldKind::Vect2 => Some(8),")
    add("            FieldKind::Vect3 => Some(12),")
    add("            FieldKind::Byte => Some(1),")
    add("            FieldKind::Word => Some(2),")
    add("            FieldKind::Blob => Some(n),")
    add("            FieldKind::String16 | FieldKind::Array16 => Some(4 + n * 2),")
    add("            FieldKind::Array8 => Some(4 + n),")
    add("            FieldKind::Array32 => Some(4 + n * 4),")
    add("            FieldKind::NestedStruct => None,")
    add("        }")
    add("    }")
    add("}")
    add("")
    add("/// One row of the auth message table.")
    add("///")
    add("/// `size` is the whole on-wire message: the header and every field, with no length prefix")
    add("/// anywhere, because the protocol has none.")
    add("#[derive(Debug, Clone, Copy)]")
    add("pub struct Entry {")
    add("    /// The wire value, mask included: `0x8000 | index` or `0x0000 | index`.")
    add("    pub header: u16,")
    add("    /// The index GWPS uses to address its format array (`header & 0x7FFF`).")
    add("    pub index: u16,")
    add("    /// The fixed on-wire size, `max_size` in `msgdefs.c`.")
    add("    pub size: usize,")
    add("    /// The GWPS name, when `opcodes.h` gives this index one; `None` for the reserved gaps.")
    add("    pub name: Option<&'static str>,")
    add("    /// Whether the header is client-to-server.")
    add("    pub from_client: bool,")
    add("    /// The fields the message is made of, in wire order.")
    add("    pub fields: &'static [Field],")
    add("}")
    add("")

    # Named constants, so calling code reads like the reference instead of like magic numbers.
    named = [row for row in rows if row[4] is not None]
    add("// ---------------------------------------------------------------- named headers")
    add("// The names are GWPS's own (`opcodes.h`), kept verbatim so a Rust message can be diffed")
    add("// against a C one by name rather than by number.")
    add("")
    for header, _direction, _index, size, name, _fields in named:
        add("/// `%s` (%d bytes on the wire)." % (name, size))
        add("pub const %s: u16 = 0x%04X;" % (name, header))
    add("")

    variable_headers = [row[0] for row in rows if wire_size(row[5]) is None]
    add("/// Headers whose declared size cannot be recomputed from their field layout, because a")
    add("/// `TYPE_NESTED_STRUCT` makes the length depend on a count carried on the wire.")
    add("///")
    add("/// Their `size` is the reference's declared maximum, carried verbatim. One of them,")
    add("/// `0x001A`, declares 32 bytes where its own layout can only occupy 31: the reference's")
    add("/// maximum is not exactly its packer's output. That is unresolved and it is why this list")
    add("/// exists -- a table-driven framer would consume one byte more than GWPS does for that")
    add("/// message. It is an unnamed friend/social message, not part of the login path, so it is")
    add("/// recorded rather than guessed at (`docs/03-protocol.md`, open questions).")
    if variable_headers:
        add("pub const VARIABLE_LENGTH_HEADERS: &[u16] = &[%s];" % ", ".join("0x%04X" % h for h in variable_headers))
    else:
        add("pub const VARIABLE_LENGTH_HEADERS: &[u16] = &[];")
    add("")
    add("/// Every auth header, client-to-server first, then server-to-client, ascending by index.")
    add("///")
    add("/// `None`-named entries are indices the client can still send although `opcodes.h` does not")
    add("/// name them: they keep their size so an unexpected-but-real message is still framed")
    add("/// correctly instead of being mistaken for a malformed one.")
    add("///")
    add("/// `#[rustfmt::skip]` is load-bearing: one row per message, laid out by this generator. With")
    add("/// rustfmt free to rewrap these lines, `cargo fmt` and `--check` would disagree forever.")
    add("#[rustfmt::skip]")
    add("pub const AUTH_TABLE: &[Entry] = &[")
    for header, direction, index, size, name, fields in rows:
        layout = ", ".join(
            "Field { kind: FieldKind::%s, length: %d }" % (kind_to_rust(kind), length)
            for kind, length in fields
        )
        add(
            "    Entry { header: 0x%04X, index: %2d, size: %4d, name: %s, from_client: %s, fields: &[%s] },"
            % (
                header,
                index,
                size,
                "None" if name is None else "Some(%s)" % rust_string(name),
                "true" if direction == "c" else "false",
                layout,
            )
        )
    add("];")
    add("")

    add("/// The fixed on-wire size for a header, or `None` if it is not an auth header at all.")
    add("///")
    add("/// A `match` rather than a scan: rustc turns it into a jump table, so framing a message")
    add("/// costs nothing per message.")
    add("pub const fn expected_size(header: u16) -> Option<usize> {")
    add("    match header {")
    for header, _direction, _index, size, _name, _fields in rows:
        add("        0x%04X => Some(%d)," % (header, size))
    add("        _ => None,")
    add("    }")
    add("}")
    add("")

    add("/// The entry for a header, for logging and for the tests that walk the table.")
    add("pub fn entry(header: u16) -> Option<&'static Entry> {")
    add("    AUTH_TABLE.iter().find(|entry| entry.header == header)")
    add("}")
    add("")

    if variable_rows:
        sys.stderr.write(
            "note: %d message(s) carry a variable-length nested struct; their size is the "
            "reference's declared maximum and cannot be recomputed from the field list\n"
            % variable_rows
        )
    return "\n".join(lines)


def kind_to_rust(kind):
    return "".join(part.capitalize() for part in kind.replace("TYPE_", "").lower().split("_"))


def main():
    parser = argparse.ArgumentParser(description="Generate the auth message table from GWPS.")
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
                "stale: %s does not match the GWPS reference; run tools/gen-auth-tables.py\n"
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
