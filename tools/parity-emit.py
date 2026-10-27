#!/usr/bin/env python3
"""Which server-to-client messages can each server emit?

`docs/09-parity-audit.md` compares what each server does for every *client* message. It did not
compare the other direction -- everything a server can put on the wire unasked -- and whole features
hide there: a message the reference sends and this repo cannot is a feature that cannot exist,
whatever the dispatch tables say.

Both sides name messages the same way (GWPS prefixes `GAME_SMSG_`/`AUTH_SMSG_` by connection;
`asura-protocol` names its constants after the message), so the comparison is by name after stripping
the prefix. Real renames are listed in ALIASES rather than papered over.

How "can emit" is decided, so the rule is visible and arguable:

* GWPS -- a named constant appears on a source line that also builds or sends a message
  (`GameSrv_BuildMsg`, `GameSrv_Send*`, `AuthSrv_Send*`, `GameSrv_Broadcast*`). A constant that only
  sizes a table never qualifies: this measures behaviour, not the opcode list.
* ASURA -- the message has a *builder*: a function in `asura-protocol` whose body names the constant,
  or a builder that takes the header as a parameter and is called with it (`faction_max`).

Comments are stripped before scanning, because a comment saying "we could send X" is not a feature.

Read-only: the reference is opened for reading and nothing is written.
"""

import io
import os
import re
import sys

REFERENCE = r"C:\Users\Apo\Documents\GitHub\PreSearing\code\OpenTyria"
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Names that differ only in spelling. Each is a real rename, not a loosened match: the reference's
# own typo (`UDPATE`) stays on its side of the table.
ALIASES = {
    "CHARACTER_FACTION_MAX_KURZICK": "FACTION_MAX_KURZICK",
    "CHARACTER_FACTION_MAX_LUXON": "FACTION_MAX_LUXON",
    "CHARACTER_FACTION_MAX_BALTHAZAR": "FACTION_MAX_BALTHAZAR",
    "CHARACTER_FACTION_MAX_IMPERIAL": "FACTION_MAX_IMPERIAL",
    "ITEM_MOVED_TO_LOCATION": "ITEM_MOVE_TO_LOCATION",
    "CHARACTER_UDPATE_PARTY": "CHARACTER_UPDATE_PARTY",
}

NAME_RE = re.compile(r"\b(?:GAME|AUTH)_SMSG_([A-Z][A-Z0-9_]*)\b")
SENDISH = ("Build", "Send", "Broadcast")
CONST_RE = re.compile(r"pub const ([A-Z][A-Z0-9_]*):\s*u16\s*=\s*(0x[0-9A-Fa-f]{4});")
FN_RE = re.compile(r"\bpub fn ([a-z0-9_]+)\s*\(")
# World and auth code is not all `pub fn`; take any function boundary as a chunk to scan.
FN_RE2 = re.compile(r"\bfn ([a-z0-9_]+)\s*[<(]")

# Messages this repo cannot emit that are *not* gaps, each with the evidence. Kept here rather than in
# prose so the count stays honest: an explained absence is different from an unexplained one.
EXPLAINED = {
    "INSTANCE_MANIFEST_DATA": "0 in every capture -- the reference sends none when there is nothing to download",
    "INSTANCE_PLAYER_DATA_DONE": "0 in every capture -- sent only by the character-creation flow, which is queue B",
    "PARTY_HERO_ADD": "0 in every capture -- heroes only, and the reference's own scope tier puts them out",
}

PROTOCOL_FILES = (
    os.path.join("crates", "asura-protocol", "src", "game", "mod.rs"),
    os.path.join("crates", "asura-protocol", "src", "client", "messages.rs"),
)

# The generated tables are where the names live: an auth builder says `tables::AUTH_SMSG_SESSION_INFO`,
# so without these files the vocabulary would only ever contain the game-side constants and every auth
# message would look like a gap.
GENERATED_TABLES = (
    os.path.join("crates", "asura-protocol", "src", "client", "tables.rs"),
    os.path.join("crates", "asura-protocol", "src", "client", "game_tables.rs"),
)

TABLE_NAME_RE = re.compile(r'name:\s*Some\("(?:AUTH|GAME)_SMSG_([A-Z0-9_]+)"\)')
TABLE_CONST_RE = re.compile(r"pub const (?:AUTH|GAME)_SMSG_([A-Z0-9_]+):\s*u16\s*=")


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def reference_emit_set():
    """Message name -> file:line proving the reference sends it."""
    out = {}
    for name in sorted(os.listdir(REFERENCE)):
        if not name.endswith(".c"):
            continue
        with io.open(os.path.join(REFERENCE, name), "r", encoding="utf-8", errors="replace") as handle:
            for number, line in enumerate(handle, 1):
                if not any(word in line for word in SENDISH):
                    continue
                for match in NAME_RE.finditer(line):
                    message = match.group(1)
                    if re.fullmatch(r"\d{4}", message):
                        continue  # the bare opcode enumeration, not a behaviour
                    out.setdefault(ALIASES.get(message, message), "%s:%d" % (name, number))
    return out


def asura_emit_set():
    """Message name -> the builder(s) that produce it."""
    produced = {}

    builders = []  # (function, body, takes_header)
    vocabulary = set()
    for relative in GENERATED_TABLES:
        path = os.path.join(REPO, relative)
        if not os.path.exists(path):
            continue
        text = io.open(path, "r", encoding="utf-8").read()
        vocabulary.update(match.group(1) for match in TABLE_NAME_RE.finditer(text))
        vocabulary.update(match.group(1) for match in TABLE_CONST_RE.finditer(text))
    for relative in PROTOCOL_FILES:
        path = os.path.join(REPO, relative)
        if not os.path.exists(path):
            continue
        text = strip_comments(io.open(path, "r", encoding="utf-8").read())
        vocabulary.update(match.group(1) for match in CONST_RE.finditer(text))
        marks = [(match.start(), match.group(1)) for match in FN_RE.finditer(text)]
        for index, (start, function) in enumerate(marks):
            end = marks[index + 1][0] if index + 1 < len(marks) else len(text)
            body = text[start:end]
            head = body[: body.find(")") + 1]
            builders.append((function, body, "header" in head))

    # Names that builders take as a parameter, and the constants passed to them. Scanned per function,
    # because a header can be chosen in a loop (`for faction in [game::A, game::B] { faction_max(faction) }`)
    # and in that shape the constant is an argument one line up, not next to the call.
    call_sites = {}
    for folder in (("crates", "asura-world", "src"), ("crates", "asura-auth", "src")):
        base = os.path.join(REPO, *folder)
        for file_name in os.listdir(base):
            if not file_name.endswith(".rs"):
                continue
            text = strip_comments(
                io.open(os.path.join(base, file_name), "r", encoding="utf-8", errors="replace").read()
            )
            marks = [(match.start(), match.group(1)) for match in FN_RE2.finditer(text)]
            for index, (start, _function) in enumerate(marks):
                end = marks[index + 1][0] if index + 1 < len(marks) else len(text)
                body = text[start:end]
                for function, _b, takes_header in builders:
                    if not takes_header:
                        continue
                    if not re.search(r"\b(?:game|messages)::%s\(" % re.escape(function), body):
                        continue
                    for match in re.finditer(r"\b(?:game|messages)::([A-Z][A-Z0-9_]+)\b", body):
                        call_sites.setdefault(match.group(1), set()).add(function)

    for function, body, _takes_header in builders:
        for name in vocabulary:
            # Matches both a bare constant and the generated `tables::AUTH_SMSG_X` spelling.
            pattern = r"\b(?:AUTH_SMSG_|GAME_SMSG_)?%s\b" % re.escape(name)
            if re.search(pattern, body):
                produced.setdefault(name, set()).add(function)
    for name, functions in call_sites.items():
        produced.setdefault(name, set()).update(functions)

    return produced


def main():
    if not os.path.isdir(REFERENCE):
        sys.stderr.write("the reference is not at %s\n" % REFERENCE)
        return 2

    theirs = reference_emit_set()
    ours = asura_emit_set()
    missing = sorted(name for name in theirs if name not in ours)
    shared = sorted(name for name in theirs if name in ours)
    extra = sorted(name for name in ours if name not in theirs)
    explained = [name for name in missing if name in EXPLAINED]
    real = [name for name in missing if name not in EXPLAINED]

    write = sys.stdout.write
    write("messages the reference can emit: %d\n" % len(theirs))
    write("of those, this repo can emit:    %d\n" % len(shared))
    write("of those, this repo CANNOT:      %d (%d real, %d explained)\n"
          % (len(missing), len(real), len(explained)))
    write("real gaps: %d\n" % len(real))
    write("real gap names: %s\n\n" % ", ".join(real))

    if real:
        write("CANNOT EMIT -- feature surface this repo does not have\n")
        for name in real:
            write("  %-40s %s\n" % (name, theirs[name]))
        write("\n")
    if explained:
        write("CANNOT EMIT, explained (not gaps)\n")
        for name in explained:
            write("  %-40s %s\n" % (name, EXPLAINED[name]))
        write("\n")
    if extra:
        write("this repo can emit, the reference cannot\n")
        for name in extra:
            write("  %-40s %s\n" % (name, ", ".join(sorted(ours[name])[:3])))
        write("\n")
    write("shared: %s\n" % ", ".join(shared))
    return 0


if __name__ == "__main__":
    sys.exit(main())
