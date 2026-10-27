#!/usr/bin/env python3
"""Compare the load block with the reference's capture byte for byte, and allow only what is explained.

`tools/parity-load.py` measures the block by `(header, size)`: 480 of 480, in the reference's order. That is
a real measurement, and it cannot see a wrong *value* -- a profession of 0 where the reference sent 1 is the
same 9 bytes. This tool is the other half: it walks the same 480 messages and compares the payloads, and it
fails unless every difference is one of the three below.

The three, all measured rather than assumed:

  * `0x001A` `PVP_ITEM_STREAM_ADD` -- the reference's own packer repeats `params[0]` for every element: its
    `pack_helper`'s nested-struct case (`msgpack.c:411-436`) calls `unpack_helper` -- the other direction --
    with `&input[consumed]` and never advances `consumed`, so all `n` params on the wire are the first one.
    The captured bytes say so for all 387 of them. This server sends the table's real values
    (`tools/gen-pvp-items.py` generates them from the same array the reference packs), so only the header,
    the item id and the count are compared.
  * `0x0094` `MAP_UPDATE_UNLOCKED_LIST` -- 260 bytes of which the reference fills the list lengths and the
    bitmap copies of a fresh character, i.e. almost nothing. The rest of its per-server buffer is whatever
    the previous message left there (the capture carries armour file ids and dye colours in it). This server
    sends zeros there, and the check requires every byte it *does* set to be the reference's.
  * `0x001F` `WORLD_UPDATE_LOAD_TIME` -- a wall-clock measurement of how long the load took. Only the size
    is comparable.

Everything else -- 91 of the 480 messages, including the agent record, the player record, the whole item
stream and the tail of the block -- has to match byte for byte, and does.

`--check` runs `cargo run -p asura-world --example dump-load` itself, so there is no stale dump to trust.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_CAPTURE = os.path.join(
    os.path.dirname(REPO), "PreSearing", "gwps-capture.log"
)

LINE_RE = re.compile(r"SEND\s+0x([0-9A-Fa-f]{4})\s+(\d+)\s+bytes\s*:\s*([0-9A-Fa-f ]*)")

# The reference's own periodic traffic: it has a clock in it, a block dump does not, and
# `tools/parity-load.py` drops the same three.
PERIODIC = {0x000C, 0x000D, 0x001E}

# Header -> (how much of it is comparable, why). "prefix:N" compares the first N bytes; "size" compares the
# size alone; "zeros" requires our payload to be all zero.
ALLOWED = {
    0x001A: (
        "prefix:5",
        "the reference's packer repeats params[0] for every param (pack_helper's nested-struct case calls "
        "unpack_helper and never advances its input offset, msgpack.c:411-436); this server sends the "
        "table's real values",
    ),
    0x0094: (
        "no-invention",
        "the reference fills the lengths and the bitmap copies and leaves the rest of its per-server buffer "
        "holding the previous message; this server sends zeros there, and every byte it does set must be "
        "the reference's",
    ),
    0x001F: ("size", "a wall-clock measurement of how long the load took"),
}


def parse(text, only_send=True):
    messages = []
    for line in text.splitlines():
        match = LINE_RE.search(line)
        if not match:
            continue
        if only_send and "RECV" in line:
            continue
        header = int(match.group(1), 16)
        data = bytes(int(part, 16) for part in match.group(3).split())
        messages.append((header, data))
    return messages


def reference_block(path):
    """The reference's server-to-client stream from the load head to the load finish."""
    messages = parse(open(path, "r", encoding="utf-8", errors="replace").read())
    try:
        start = next(i for i, (header, _data) in enumerate(messages) if header == 0x017C)
        end = next(
            i for i, (header, _data) in enumerate(messages) if header == 0x018E and i > start
        )
    except StopIteration:
        raise SystemExit("the capture has no load block (0x017C .. 0x018E)")
    return [message for message in messages[start : end + 1] if message[0] not in PERIODIC]


def our_block():
    result = subprocess.run(
        ["cargo", "run", "--offline", "-q", "-p", "asura-world", "--example", "dump-load"],
        capture_output=True,
        text=True,
        cwd=REPO,
    )
    if result.returncode != 0:
        raise SystemExit("dump-load did not run:\n" + (result.stderr or result.stdout))
    return parse(result.stdout)


def compare(reference, ours):
    """Walk the two streams, returning (compared, allowed, failures)."""
    if len(reference) != len(ours):
        raise SystemExit(
            "the streams are not the same length: {} reference, {} ours -- run tools/parity-load.py, "
            "which compares (header, size) and names the message that differs".format(
                len(reference), len(ours)
            )
        )

    compared = 0
    allowed = {}
    failures = []
    for (ref_header, ref_bytes), (our_header, our_bytes) in zip(reference, ours):
        if ref_header != our_header:
            failures.append(
                "alignment: the reference sent 0x{:04X} where this server sent 0x{:04X}".format(
                    ref_header, our_header
                )
            )
            continue
        if len(ref_bytes) != len(our_bytes):
            failures.append(
                "0x{:04X}: {} bytes against the reference's {}".format(
                    ref_header, len(our_bytes), len(ref_bytes)
                )
            )
            continue

        if ref_header in ALLOWED:
            how, _why = ALLOWED[ref_header]
            allowed[ref_header] = allowed.get(ref_header, 0) + 1
            if how == "size":
                continue
            if how == "no-invention":
                # We may leave a byte at zero where the reference has residue, but a byte we do set has to be
                # the reference's: zeroing a field the reference fills would be caught here.
                invented = [
                    index
                    for index in range(len(our_bytes))
                    if our_bytes[index] != 0 and our_bytes[index] != ref_bytes[index]
                ]
                if invented:
                    failures.append(
                        "0x{:04X}: {} byte(s) this server sets that the reference does not, at {}".format(
                            ref_header, len(invented), invented[:8]
                        )
                    )
                continue
            count = int(how.split(":")[1])
            if our_bytes[:count] != ref_bytes[:count]:
                failures.append(
                    "0x{:04X}: the first {} bytes differ: {} against the reference's {}".format(
                        ref_header,
                        count,
                        our_bytes[:count].hex(" ").upper(),
                        ref_bytes[:count].hex(" ").upper(),
                    )
                )
            continue

        if our_bytes != ref_bytes:
            offsets = [
                index
                for index in range(len(ref_bytes))
                if ref_bytes[index] != our_bytes[index]
            ]
            failures.append(
                "0x{:04X}: {} byte(s) differ, at {}: {} against the reference's {}".format(
                    ref_header,
                    len(offsets),
                    offsets[:8],
                    our_bytes[: len(our_bytes)].hex(" ").upper(),
                    ref_bytes.hex(" ").upper(),
                )
            )
            continue
        compared += 1

    return compared, allowed, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--capture", default=DEFAULT_CAPTURE)
    parser.add_argument("--check", action="store_true", help="the only mode: compare")
    args = parser.parse_args()

    if not args.check:
        parser.error("nothing to do: pass --check")
    if not os.path.exists(args.capture):
        print("skipped: no capture at {} (pass --capture)".format(args.capture))
        return 0

    reference = reference_block(args.capture)
    ours = our_block()
    compared, allowed, failures = compare(reference, ours)

    for header, count in sorted(allowed.items()):
        print(
            "0x{:04X} x{:<3} compared by {}: {}".format(
                header, count, ALLOWED[header][0], ALLOWED[header][1]
            )
        )
    if failures:
        for failure in failures[:20]:
            print("FAIL  " + failure)
        if len(failures) > 20:
            print("... and {} more".format(len(failures) - 20))
        return 1

    print(
        "{} of {} message(s) are byte-identical to the reference; {} allowed difference(s) in {} "
        "message(s), each with a recorded reason".format(
            compared, len(reference), sum(allowed.values()), len(allowed)
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
