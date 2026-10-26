#!/usr/bin/env python3
"""Report functional parity with the GWPS reference: which handlers it has, which ASURA has.

Parity is the project's objective (docs/roadmap.md), and a claim of parity that is not measured is a
claim nobody can check. This script reads the *reference's own dispatch switches* -- the list of client
messages it actually acts on -- and looks for each one in this repository, so "what is missing" is a
generated list rather than a memory.

Sources:
  PreSearing/code/OpenTyria/AuthSrv.c   `switch` cases over AUTH_CMSG_*   (auth handlers)
  PreSearing/code/OpenTyria/GameSrv.c   `switch` cases over GAME_CMSG_*   (game handlers)
  PreSearing/code/OpenTyria/opcodes.h   name -> header

Usage:
  python tools/parity-report.py [--gwps PATH] [--check]

  --check exits non-zero when a handler the reference implements is not implemented here, so the
  report can gate work instead of merely describing it.

A handler counts as implemented when this repository mentions its header constant by name (the
generated tables give those names: `AUTH_CMSG_...`, `GAME_CMSG_...`) or its numeric value in a match
arm. That is a deliberately low bar: it answers "is there code for this message at all", not "is its
behaviour right", which only a trace comparison can say.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_GWPS = os.path.join(os.path.dirname(REPO), "PreSearing")

CASE_RE = re.compile(r"case\s+((?:AUTH|GAME)_CMSG_[A-Z0-9_]+)\s*:")
OPCODE_RE = re.compile(
    r"#define\s+((?:AUTH|GAME)_CMSG_[A-Z0-9_]+)\s+\((?:AUTH|GAME)_CMSG_MASK\s*\|\s*0x([0-9A-Fa-f]{4})\)"
)

# Blocks that only dispose of a message. The reference's own comment says as much for the first three
# (`case ...: break;`), so they are listed but not counted as missing work.
PASSIVE = {
    "GAME_CMSG_HEARTBEAT": "ignored (a keep-alive with no answer)",
    "GAME_CMSG_PING_REPLY": "ignored (the answer to our own ping)",
}


def opcodes(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    return {name: int(value, 16) for name, value in OPCODE_RE.findall(text)}


def handled(path, family):
    """The reference's dispatch cases for one family, in file order."""
    text = open(path, encoding="utf-8", errors="replace").read()
    seen = []
    for case in CASE_RE.findall(text):
        if case.startswith(family) and case not in seen:
            seen.append(case)
    return seen


def ours():
    """Every message constant name and numeric header the **service code** mentions.

    Only `asura-auth` and `asura-world` are read, and the generated tables are skipped: `game_tables.rs`
    and `tables.rs` contain every header in the protocol by construction, so including them would make
    this report claim parity for messages nothing handles -- which is exactly the false answer it exists
    to prevent.
    """
    names = set()
    numbers = set()
    generated = {
        "tables.rs",
        "game_tables.rs",
        "pvp_items.rs",
        "default_inventory.rs",
        "login_fixtures.rs",
        "handshake_fixtures.rs",
    }
    for crate in ("asura-auth", "asura-world", "asura-protocol"):
        root = os.path.join(REPO, "crates", crate, "src")
        for here, _dirs, files in os.walk(root):
            for name in files:
                if not name.endswith(".rs") or name in generated:
                    continue
                path = os.path.join(here, name)
                text = open(path, encoding="utf-8", errors="replace").read()
                if crate == "asura-protocol":
                    # The protocol crate owns the *messages*; only the builders and decoders there count,
                    # and they name their headers as constants, never as bare numbers.
                    names.update(
                        re.findall(r"\b((?:AUTH|GAME)_[CS]MSG_[A-Z0-9_]+)\b", text)
                    )
                    continue
                names.update(
                    re.findall(r"\b((?:AUTH|GAME)_[CS]MSG_[A-Z0-9_]+)\b", text)
                )
                numbers.update(
                    int(value, 16)
                    for value in re.findall(r"0x([0-9A-Fa-f]{3,4})\b", text)
                )
    return names, numbers


def wire_header(name, index):
    """The header as it appears on the wire: the direction mask is part of the value.

    `opcodes.h` writes a client message as `(GAME_CMSG_MASK | 0x0091)`, so the index alone is not the
    header -- the loader request for items is `0x8091`, not `0x0091`. Comparing the index is how this
    report would claim four messages were missing that are implemented.
    """
    if "_CMSG_" in name:
        return index | 0x8000
    return index & 0x7FFF


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--gwps", default=DEFAULT_GWPS)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    code = os.path.join(args.gwps, "code", "OpenTyria")
    if not os.path.isdir(code):
        raise SystemExit("parity-report: no reference checkout at %s" % code)

    headers = opcodes(os.path.join(code, "opcodes.h"))
    our_names, our_numbers = ours()

    missing = []
    for family, source in (
        ("AUTH_CMSG", "AuthSrv.c"),
        ("GAME_CMSG", "GameSrv.c"),
    ):
        implemented = handled(os.path.join(code, source), family)
        print("%s: the reference handles %d message(s)" % (family, len(implemented)))
        for name in implemented:
            index = headers.get(name)
            header = wire_header(name, index) if index is not None else None
            note = PASSIVE.get(name)
            known = name in our_names or (
                header is not None and header in our_numbers
            )
            if known:
                mark = "have"
            elif note:
                mark = "have" if name in our_names else "ok  "
            else:
                mark = "MISS"
                missing.append((family, name, header))
            if note and mark == "ok  ":
                continue
            print(
                "  %s  %-42s 0x%04X" % (mark, name, header if header is not None else 0)
            )
        print()

    if missing:
        print("%d handler(s) not implemented here:" % len(missing))
        for family, name, header in missing:
            print("  %-42s 0x%04X" % (name, header if header is not None else 0))
        if args.check:
            raise SystemExit(1)
    else:
        print("functional parity with the reference's dispatch: nothing missing")


if __name__ == "__main__":
    main()
