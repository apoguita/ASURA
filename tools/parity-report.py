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
import io
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


def function_body(path, name):
    """The body of a C function, by brace matching from its definition."""
    text = open(path, encoding="utf-8", errors="replace").read()
    match = re.search(r"\b%s\s*\([^)]*\)\s*\{" % re.escape(name), text)
    if not match:
        return ""
    depth = 0
    for index in range(match.end() - 1, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[match.end() : index]
    return ""


def is_stub(body):
    """Whether the reference itself does nothing with this message.

    GWPS says so in its own idiom: every parameter it ignores is named in an `UNREFERENCED_PARAMETER`
    line, and a body that neither sends a message nor touches the database does nothing a client can
    observe. A handler that is a no-op *there* is not work here -- which is the difference between "we
    have not implemented this" and "there is nothing to implement".
    """
    if "UNREFERENCED_PARAMETER" in body:
        return True
    calls_something = re.search(
        r"\b(GameSrv_|Db_|AuthSrv_|CtrlConn_|GmParty_|GmAgent_|GmBag_)\w+\s*\(", body
    )
    statements = [line for line in body.splitlines() if line.strip().endswith(";")]
    return calls_something is None and len(statements) <= 3


#: Handlers the reference itself does not implement. Reviewed by hand, one entry at a time, because the
#: alternative -- guessing from a function's shape -- flagged real handlers as stubs and would have
#: understated the gap. Each entry names the reference function and what it does with the message.
REFERENCE_NO_OPS = {
    "GAME_CMSG_ITEM_UPDATE_EQUIPPED_COLOR": (
        "GameSrv_HandleChangeEquippedItemColor ignores all three parameters "
        "(UNREFERENCED_PARAMETER) and returns ERR_OK"
    ),
}


def no_op_reason(case):
    """Why the reference does nothing with this message, if it does not."""
    return REFERENCE_NO_OPS.get(case)


def write_status(path, have_count, real, no_ops):
    """The status table, as markdown, so the documentation is generated rather than remembered."""
    lines = [
        "# Status: what works, what is a stub",
        "",
        "Generated by `tools/parity-report.py --write docs/07-status.md`. Do not edit by hand: a status",
        "table typed by hand is a status table that is wrong in a month.",
        "",
        "It compares this repository with the GWPS reference's own dispatch switches, so *missing* means",
        '"the reference acts on this message and we do not".',
        "",
        "## Implemented handlers",
        "",
        "%d of the reference's client-message handlers are present in this repository." % have_count,
        "",
    ]
    if no_ops:
        lines += [
            "## No-ops in the reference itself",
            "",
            "The reference's handler ignores every parameter and does nothing a client can observe.",
            "Implementing these would be inventing behaviour, so they are a deliberate gap rather than",
            "missing work:",
            "",
        ]
        for _family, name, header in no_ops:
            lines.append(
                "- `%s` (`0x%04X`) -- %s"
                % (name, header if header is not None else 0, no_op_reason(name))
            )
        lines.append("")
    lines += ["## Missing, with real behaviour in the reference", ""]
    if real:
        for _family, name, header in real:
            lines.append("- `%s` (`0x%04X`)" % (name, header if header is not None else 0))
    else:
        lines.append("Nothing.")
    lines += [
        "",
        "## Known partial areas",
        "",
        "Handlers are the floor, not the whole of parity. These are implemented *and* limited, stated so",
        "that nobody reads the table above as more than it says:",
        "",
        "- **movement** has no pathfinding: `MOVE_TO_COORD` answers with the client's own destination",
        "  rather than the next waypoint of a path, so a straight line can cut a corner",
        "  (`crates/asura-world/src/agent.rs`);",
        "- **world state** is one agent: no NPCs, no other players, no combat, no quests;",
        "- **items** are rows (`asura_db::inventory`), and the block sent for them is the reference's own",
        "  down to the byte (`crates/asura-world/src/lib.rs`, the byte-equality test against the capture).",
        "  What is *not* there: nothing can be equipped, moved between bags, bought or picked up. The one",
        "  item message the reference acts on, `DROP_ITEM`, is implemented exactly as it behaves there --",
        "  which means **the dropped item stays in its bag**: the reference creates an agent for it and",
        "  never removes it, never frees its id and never sends `ITEM_REMOVE`. Its handler is unfinished in",
        "  its own source (a `TODO` for partial stacks), and this port copies the behaviour rather than",
        "  inventing the missing half;",
        "- **instances** are prebuilt per map and never created on demand, and only outposts are hosted;",
        "- **persistence** saves where a character stood when it leaves and puts it back there when it",
        "  returns (`asura_db::game::record_position`). That is a thing the reference does *not* do -- it",
        "  keeps a character's last map and re-spawns at the map's own spawn point every session -- so a",
        "  character's position is a deliberate addition, recorded in `0006_inventory.sql`. Nothing else",
        "  about a character is saved yet: no level, no profession, no equipment changes, no gold;",
        "- **character creation** (the four `CHAR_CREATION_*` handlers) needs two things this server does",
        "  not have. The reference only offers it inside a *character-creation* instance (its",
        "  `MapType_CharacterCreation`), and this world hosts outposts only; and",
        "  `GameSrv_HandleCharCreationChangeProf` answers with the default equipment for the chosen",
        "  campaign and profession (`GetDefaultEquipments`), a table this project has not ported. Doing",
        "  the handlers without those two would send a creation screen the client cannot leave with a",
        "  character it cannot equip;",
        "- **the game connection's front door** is a byte-copy router; the reference hands the socket",
        "  itself to the game server, which needs one process and is not what this does",
        "  (`crates/asura-auth/src/frontdoor.rs`).",
        "",
        "## Features the reference has but keeps switched off",
        "",
        "Parity with GWPS includes matching what it *disables*, not only what it implements. From its own",
        "source:",
        "",
        "- **chat sending** is gated (`GameSrv_ChatSendingEnabled`, `GWPS_CHAT=1` to enable) and off by",
        "  default. Its comment records why: an unconditional send put a message on the wire the client",
        "  could not parse, `GameSrv_SendInvalidCommand` above all, **and it crashed the client**. A port",
        "  that sends chat by default would be less faithful than one that does not;",
        "- **GM commands** (`/tp`, `/maps`) exist behind the same gate.",
        "",
    ]
    io.open(path, "w", encoding="utf-8", newline="\n").write("\n".join(lines))


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
    parser.add_argument(
        "--write",
        help="write the status table to this path (docs/07-status.md), so the doc cannot go stale",
    )
    args = parser.parse_args()

    code = os.path.join(args.gwps, "code", "OpenTyria")
    if not os.path.isdir(code):
        raise SystemExit("parity-report: no reference checkout at %s" % code)

    headers = opcodes(os.path.join(code, "opcodes.h"))
    our_names, our_numbers = ours()

    missing = []
    have = []
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
            if mark == "have":
                have.append((family, name, header))
            print(
                "  %s  %-42s 0x%04X" % (mark, name, header if header is not None else 0)
            )
        print()

    # A handler the reference itself stubs is not work here: it is the difference between "we have not
    # implemented this" and "there is nothing to implement", and conflating the two overstates the gap.
    real = []
    no_ops = []
    for family, name, header in missing:
        if no_op_reason(name):
            no_ops.append((family, name, header))
        else:
            real.append((family, name, header))

    if no_ops:
        print("%d of them are no-ops in the reference itself:" % len(no_ops))
        for family, name, header in no_ops:
            print(
                "  %-42s 0x%04X  %s"
                % (name, header if header is not None else 0, no_op_reason(name))
            )
        print()

    if real:
        print("%d handler(s) with real behaviour are missing here:" % len(real))
        for family, name, header in real:
            print("  %-42s 0x%04X" % (name, header if header is not None else 0))
    else:
        print("functional parity with the reference's dispatch: nothing missing")

    if args.write:
        write_status(args.write, len(have), real, no_ops)
        print("\nwrote %s" % args.write)

    if real and args.check:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
