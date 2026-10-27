#!/usr/bin/env python3
"""Diff ASURA's world-server load stream against the GWPS reference's own trace.

Why: a client that dies mid-load looks identical whether one message is wrong, of the wrong size, or
missing. Both servers can be made to print every message with its bytes -- GWPS with `GWPS_TRACE=1`
(patch in tools/patch-gwps-full-trace.py), ASURA with `cargo run -p asura-world --example dump-load` --
so the diagnosis is a diff and not an inference.

Usage:
  python tools/compare-load.py --reference tools/fixtures/gwps-capture.log --ours dump.txt

  --ours is the output of the example; `--dump` writes one by running the example itself. The
  reference trace ships as a fixture (tools/fixtures/gwps-capture.log), so this runs without the
  reference checkout.

Alignment: our messages are walked in order, each matched to the *next* reference message with the same
header. That tolerates the reference sending messages we cannot (real items, real unlocks) without
letting a message we send twice, or out of order, go unnoticed.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

LINE_RE = re.compile(r"(?:SEND|RECV)\s+0x([0-9A-Fa-f]{4})\s+(\d+)\s+bytes\s*:\s*([0-9A-Fa-f ]*)")


def parse_lines(text):
    """Every (header, size, bytes) in the order it appears."""
    messages = []
    for line in text.splitlines():
        match = LINE_RE.search(line)
        if not match:
            continue
        header = int(match.group(1), 16)
        size = int(match.group(2))
        data = bytes(int(part, 16) for part in match.group(3).split())
        messages.append((header, size, data))
    return messages


def reference_game_stream(messages):
    """The reference's server-to-client game messages, from the load head to the load finish."""
    start = None
    end = None
    for index, (header, size, _data) in enumerate(messages):
        if header == 0x017C and start is None:
            start = index
        if header == 0x018E and start is not None:
            end = index
            break
    if start is None:
        raise SystemExit("compare-load: the reference trace has no 0x017C (load head)")
    if end is None:
        raise SystemExit("compare-load: the reference trace has no 0x018E (load finish)")
    return messages[start : end + 1]


def dump_ours():
    """Run the example and return its output."""
    cargo = os.path.join(os.environ.get("USERPROFILE", ""), ".cargo", "bin", "cargo.exe")
    if not os.path.exists(cargo):
        cargo = "cargo"
    result = subprocess.run(
        [cargo, "run", "--offline", "-q", "-p", "asura-world", "--example", "dump-load"],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit("compare-load: the dump example failed:\n" + result.stderr)
    return result.stdout


def first_difference(ours, theirs):
    """The offset and the two bytes where two messages first disagree, or None."""
    for offset in range(min(len(ours), len(theirs))):
        if ours[offset] != theirs[offset]:
            return offset
    if len(ours) != len(theirs):
        return min(len(ours), len(theirs))
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", required=True, help="the GWPS trace (GWPS_TRACE=1)")
    parser.add_argument("--ours", help="the output of the dump example; omit to run it")
    parser.add_argument("--dump", action="store_true", help="only print our stream and exit")
    args = parser.parse_args()

    ours_text = open(args.ours, encoding="utf-8").read() if args.ours else dump_ours()
    if args.dump:
        sys.stdout.write(ours_text)
        return

    reference = open(args.reference, encoding="utf-8", errors="replace").read()
    reference_stream = [
        entry for entry in reference_game_stream(parse_lines(reference))
    ]
    our_stream = [entry for entry in parse_lines(ours_text)]

    cursor = 0
    problems = 0
    matched = 0
    reference_only = 0

    for header, size, data in our_stream:
        while cursor < len(reference_stream) and reference_stream[cursor][0] != header:
            reference_only += 1
            cursor += 1
        if cursor >= len(reference_stream):
            print(
                "WE SEND, THE REFERENCE DOES NOT: 0x%04X (%d bytes): %s"
                % (header, size, data.hex(" ").upper())
            )
            problems += 1
            continue
        _r_header, r_size, r_data = reference_stream[cursor]
        cursor += 1
        matched += 1
        if size != r_size:
            print(
                "SIZE  0x%04X: ours %d, reference %d" % (header, size, r_size)
            )
            print("      ours:      %s" % data.hex(" ").upper())
            print("      reference: %s" % r_data.hex(" ").upper())
            problems += 1
            continue
        offset = first_difference(data, r_data)
        if offset is not None:
            print(
                "BYTES 0x%04X at offset %d: ours %02X, reference %02X"
                % (
                    header,
                    offset,
                    data[offset] if offset < len(data) else -1,
                    r_data[offset] if offset < len(r_data) else -1,
                )
            )
            print("      ours:      %s" % data.hex(" ").upper())
            print("      reference: %s" % r_data.hex(" ").upper())
            problems += 1

    trailing = len(reference_stream) - cursor

    # Order matters as much as content, and the per-message walk above cannot see it: matching each of
    # our messages to the next reference message with the same header tolerates any reordering. A
    # message that arrives in the wrong place is a client that builds its model before it is told what
    # the model is, which is exactly the kind of bug this script exists to catch.
    our_order = [entry[0] for entry in our_stream]
    our_set = set(our_order)
    reference_order = [
        entry[0] for entry in reference_stream if entry[0] in our_set
    ]
    if our_order != reference_order:
        print("ORDER differs:")
        print("  ours:      %s" % " ".join("%04X" % header for header in our_order))
        print("  reference: %s" % " ".join("%04X" % header for header in reference_order))
        problems += 1

    print()
    print("matched %d message(s), %d problem(s)" % (matched, problems))
    print(
        "reference sent %d more message(s) in this window (items, unlocks, ticks -- things this server"
        " does not have yet)" % (reference_only + trailing)
    )
    if problems:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
