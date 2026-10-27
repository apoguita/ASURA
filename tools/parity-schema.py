#!/usr/bin/env python3
"""Diff the reference's database schema against this one, column by column, and fail on anything unexplained.

The goal of this project is parity across the *observable* surface, and the database is the least observable
part of it -- which is exactly why it needs a measurement rather than an opinion. The reference declares its
tables as `X(Type, name)` lists in `DbSchema.h`; this repository's schema is what its migrations produce. This
tool reads both and says, for every column the reference stores:

  * **stored**    -- this schema has it, under the same name or a recorded one;
  * **derived**   -- this schema answers it from another table (the armour pieces come from the equipped bag);
  * **explained** -- neither, and a written reason says why that is not a difference a client can see.

A reference column that is none of those three **fails the check**. That is the point: a new column in
`DbSchema.h` cannot be silently ignored, which is how the character-settings block (`docs/10`, item B5) was
missed for as long as it was.

Usage:
  python tools/parity-schema.py --check
  python tools/parity-schema.py --write docs/12-schema-parity.md

Without a database URL the tool still checks that every reference column is mapped or explained; the
per-column "this schema has it" half needs the database and says so when it is missing.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import io
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCE_REPO = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_SCHEMA = os.path.join(REFERENCE_REPO, "code", "OpenTyria", "DbSchema.h")
DEFAULT_OUT = os.path.join(REPO, "docs", "12-schema-parity.md")

# The reference's struct names to its table names, and those to this repository's.
REFERENCE_TABLES = ("Session", "Account", "Character", "Bag", "Item")
OUR_TABLE = {
    "session": "session",
    "account": "account",
    "character": "characters",
    "bag": "bag",
    "item": "item",
}

# Reference column -> this schema's column. A mapping is a claim that the two hold the same value; it is
# checkable against the database, and the check fails if the column named here does not exist.
MAPPED = {
    ("session", "session_id"): "id",
    ("session", "user_id"): "portal_user",
    ("session", "account_id"): "account_id",
    ("session", "created_at"): "started_at",
    ("account", "account_id"): "id",
    ("account", "created_at"): "created_at",
    ("character", "char_id"): "id",
    ("character", "charname"): "name",
    ("character", "last_outpost"): "map_id",
    ("character", "appearance"): None,  # the packed word below
    ("bag", "char_id"): "character_id",
    ("item", "char_id"): "character_id",
}
# The reference unpacks `Appearance` into seven columns; this schema stores the packed word.
PACKED_INTO = {
    "character": (
        ("sex", "appearance"),
        ("height", "appearance"),
        ("skin_color", "appearance"),
        ("hair_color", "appearance"),
        ("face_style", "appearance"),
        ("hair_style", "appearance"),
        ("race", "appearance"),
    )
}

# Everything that is deliberately not a column here, with the reason. This is the table the check enforces:
# a reference column that is neither stored, nor derived, nor in here fails.
EXPLAINED = {
    ("session", "updated_at"): "no analogue: this schema closes a session with `ended_at` and never updates a row in place",
    ("account", "updated_at"): "no analogue: account rows are written once",
    ("account", "eula_accepted"): "unobservable: `AuthSrv.c:684` sends a hardcoded 24 for it, so the value on the wire is a constant in the reference too",
    ("account", "current_char_id"): "kept on `session.char_id` here: the reference puts the last selected character on the account, this scopes it to the login that selected it",
    ("account", "current_territory"): "unobservable: read into the account row and never sent; a travel here is a `transfer` row",
    ("account", "storage_gold"): "not stored: `GameSrv.c:825` sends it in the storage-gold message, which this server sends from the character's own gold -- the reference never fills the storage bags either",
    ("character", "created_at"): "no analogue: rows are created once and `updated_at` carries the only timestamp this schema reads",
    ("character", "updated_at"): "stored as `characters.updated_at`",
    ("character", "last_guild_hall_id"): "zero: nothing here creates a guild, and the reference's own value for a lone character is the null uuid the settings blob carries",
    ("character", "gold"): "stored as `characters.gold`",
    ("character", "active_weapon_set"): "unobservable: `GameSrv_SendWeaponSlots` sends four zero slots and never reads the column",
    ("character", "skill_points"): "zeros are parity: read by `GameSrv_SendPlayerFactions`, written by nothing -- no handler awards experience or points",
    ("character", "skill_points_total"): "zeros are parity, as `skill_points`",
    ("character", "experience"): "zeros are parity: read by `GameSrv_SendPlayerFactions`, assigned by no handler in the reference",
    ("character", "unlocked_skills"): "zeros are parity: `GameSrv_SendUpdateUnlockedSkills` sends the bitmap, and the reference's own confirm writes it all-zero",
    ("character", "unlocked_maps"): "zeros are parity: stored and never read into a message",
    ("character", "completed_missions_nm"): "zeros are parity: stored and never read into a message",
    ("character", "completed_bonuses_nm"): "zeros are parity: stored and never read into a message",
    ("character", "completed_missions_hm"): "zeros are parity: stored and never read into a message",
    ("character", "completed_bonuses_hm"): "zeros are parity: stored and never read into a message",
    ("bag", "account_id"): "derived: every bag query here is scoped by character, which implies the account",
    ("bag", "created_at"): "no analogue: rows are written once, at creation",
    ("bag", "updated_at"): "no analogue: bags are not updated",
    ("item", "account_id"): "derived: every item query here is scoped by character, which implies the account",
    ("item", "bag_model_id"): "stored as `item.bag_id`, a reference to the bag row rather than a copy of its model id",
    ("item", "created_at"): "no analogue: rows are written once, at creation",
    ("item", "updated_at"): "no analogue: an item is rewritten, never dated",
}
for skill in range(1, 9):
    EXPLAINED[("character", "skill%d" % skill)] = (
        "zeros are parity: `GameSrv_SendSkillbarUpdate` sends them, and every writer in the reference "
        "leaves them zero"
    )
for piece in ("body", "legs", "head", "boots", "gloves"):
    EXPLAINED[("character", "file_id_%s" % piece)] = (
        "derived: the settings blob reads the piece's file id from the item in that equipped slot"
    )
    EXPLAINED[("character", "colors_%s" % piece)] = (
        "derived: the settings blob reads the dye colours from the item in that equipped slot"
    )

for faction in ("balthazar", "kurzick", "luxon", "imperial"):
    EXPLAINED[("account", "%s_points_max" % faction)] = (
        "fresh-account parity: this server sends the cap the reference sent for an untouched account "
        "(10000) in the four `FACTION_MAX_*` messages; the reference reads the column and nothing in it "
        "ever writes one"
    )
    EXPLAINED[("account", "%s_points_amount" % faction)] = (
        "zeros are parity: `GameSrv_SendPlayerFactions` reads it, and no handler in the reference assigns one"
    )
    EXPLAINED[("account", "%s_points_total" % faction)] = (
        "zeros are parity, as `%s_points_amount`" % faction
    )

# Columns this schema has that the reference does not, with why they exist. Anything not in here and not
# matched by a mapping above is reported as an addition without a reason -- not a failure, but a question.
ADDED = {
    ("session", "portal_user"): "the portal identity the rig presents (`migration 0003`); the reference's `user_id`",
    ("session", "char_id"): "the character this login selected; the reference keeps it on the account",
    ("session", "world_id"): "the world a travel placed this session on",
    ("session", "map_id"): "the map that travel asked for",
    ("session", "ended_at"): "closing a session is what makes a second login win",
    ("account", "username"): "the account name the rig logs in with",
    ("account", "token"): "the login token the world validates a reconnect against",
    ("account", "token_at"): "when that token was minted",
    ("account", "last_ip"): "the address the last login came from",
    ("characters", "pos_x"): "where the character stood, saved on leaving",
    ("characters", "pos_y"): "as `pos_x`",
    ("characters", "pos_plane"): "as `pos_x`",
    ("bag", "bag_id"): "the number the client is told; per character, as the wire wants it",
    ("bag", "assoc_item_id"): "the item that *is* the bag (`bag_item_id` in the reference's in-memory model)",
    ("item", "materials"): "`CREATE_NAMED_ITEM`'s materials field",
    ("item", "unk1"): "`CREATE_NAMED_ITEM`'s unexplained field, kept so the message can be rebuilt",
    ("item", "value"): "`CREATE_NAMED_ITEM`'s value",
    ("item", "name"): "the item's name, as the UTF-16 units the wire carries",
    ("item", "modifiers"): "the item's modifier list, as the wire carries it",
}


def parse_schema(path):
    """`DbXColumnsDef` blocks -> {table: [(ctype, column)]}, one `X(Type, name)` per line.

    Parsed line by line on purpose: a type can hold a comma of its own (`static_array(uint16_t, 20)`), so
    splitting on the first comma reads the type as the column name -- which is exactly what the first
    version of this tool did, and it "found" ninety-five columns called `uint32_t`.
    """
    text = io.open(path, "r", encoding="utf-8").read()
    tables = {}
    for name in REFERENCE_TABLES:
        start = text.index("#define Db%sColumnsDef" % name)
        end = text.index("typedef struct Db%s" % name, start)
        columns = []
        for line in text[start:end].splitlines():
            line = line.strip().rstrip("\\").strip()
            if not line.startswith("X("):
                continue
            body = line[2:].strip()
            if body.endswith(")"):
                body = body[:-1]
            ctype, _, column = body.rpartition(",")
            columns.append((ctype.strip(), column.strip()))
        tables[name.lower()] = columns
    return tables


def database_url():
    if os.environ.get("ASURA_DATABASE_URL"):
        return os.environ["ASURA_DATABASE_URL"].strip()
    env_file = os.path.join(REPO, ".env")
    if os.path.exists(env_file):
        for line in io.open(env_file, "r", encoding="utf-8"):
            line = line.strip()
            if line.startswith("ASURA_DATABASE_URL="):
                return line.split("=", 1)[1].strip().strip('"').strip("'")
    return None


def find_psql():
    for candidate in ("psql",):
        for directory in os.environ.get("PATH", "").split(os.pathsep):
            path = os.path.join(directory, candidate + ".exe")
            if os.path.exists(path):
                return path
    root = r"C:\Program Files\PostgreSQL"
    if os.path.isdir(root):
        versions = sorted(os.listdir(root), reverse=True)
        for version in versions:
            path = os.path.join(root, version, "bin", "psql.exe")
            if os.path.exists(path):
                return path
    return None


def read_our_schema(url):
    """{table: [column]} from the live database, or None when it cannot be asked."""
    psql = find_psql()
    if psql is None:
        return None
    query = (
        "SELECT table_name || '.' || column_name FROM information_schema.columns "
        "WHERE table_schema = 'public' ORDER BY table_name, ordinal_position"
    )
    result = subprocess.run(
        [psql, "-X", "-q", "-A", "-t", "-c", query, url], capture_output=True, text=True
    )
    if result.returncode != 0:
        return None
    tables = {}
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line or "." not in line:
            continue
        table, column = line.split(".", 1)
        tables.setdefault(table, []).append(column)
    return tables


def verdicts(reference, ours):
    """One row per reference column: (table, column, state, detail)."""
    rows = []
    for table, columns in reference.items():
        our_table = OUR_TABLE[table]
        present = set(ours.get(our_table, [])) if ours is not None else None
        packed = dict(PACKED_INTO.get(table, ()))
        for _ctype, column in columns:
            key = (table, column)
            if column in packed:
                rows.append(
                    (
                        table,
                        column,
                        "packed",
                        "the packed `Appearance` word, `characters.%s`" % packed[column],
                    )
                )
                continue
            mapped = MAPPED.get(key)
            if mapped:
                state = "mapped"
                detail = "`%s.%s`" % (our_table, mapped)
                if present is not None and mapped not in present:
                    state = "MISSING"
                    detail = "`%s.%s` is not in the database" % (our_table, mapped)
                rows.append((table, column, state, detail))
                continue
            if ours is not None and column in present:
                rows.append((table, column, "stored", "`%s.%s`" % (our_table, column)))
                continue
            if key in EXPLAINED:
                rows.append((table, column, "explained", EXPLAINED[key]))
                continue
            if ours is None:
                rows.append((table, column, "unchecked", "no database to ask"))
                continue
            rows.append((table, column, "UNEXPLAINED", "no mapping and no recorded reason"))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--schema", default=DEFAULT_SCHEMA)
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--write", action="store_true", help="write the document")
    parser.add_argument("--check", action="store_true", help="fail on an unexplained column")
    args = parser.parse_args()

    if not args.write and not args.check:
        parser.error("nothing to do: pass --check or --write")
    if not os.path.exists(args.schema):
        print("skipped: no reference schema at {} (pass --schema)".format(args.schema))
        return 0

    reference = parse_schema(args.schema)
    url = database_url()
    ours = read_our_schema(url) if url else None
    rows = verdicts(reference, ours)

    unexplained = [row for row in rows if row[2] in ("UNEXPLAINED", "MISSING")]
    unchecked = [row for row in rows if row[2] == "unchecked"]
    merged = [row for row in rows if row[2] == "packed"]

    lines = [
        "# 12 - The database schema, diffed against the reference's",
        "",
        "Generated by `tools/parity-schema.py` from `PreSearing/code/OpenTyria/DbSchema.h` and this",
        "database. Every column the reference declares is either stored here, derived from another table,",
        "or explained; a column that is none of those fails the check, which is how the character settings",
        "block was missed for as long as it was.",
        "",
        "```powershell",
        "python tools/parity-schema.py --write docs/12-schema-parity.md",
        "```",
        "",
        "## What the reference declares, and where it lives here",
        "",
    ]
    for table in ("session", "account", "character", "bag", "item"):
        our_table = OUR_TABLE[table]
        table_rows = [row for row in rows if row[0] == table]
        stored = sum(1 for row in table_rows if row[2] in ("stored", "mapped", "packed"))
        lines.append(
            "### `%s` -> `%s`: %d of %d column(s) stored"
            % (table, our_table, stored, len(table_rows))
        )
        lines.append("")
        lines.append("| reference column | state | here |")
        lines.append("|---|---|---|")
        for _table, column, state, detail in table_rows:
            lines.append("| `%s` | %s | %s |" % (column, state, detail))
        added = [
            (name, reason)
            for (our, name), reason in ADDED.items()
            if our == our_table and (ours is None or name in ours.get(our_table, []))
        ]
        if added:
            lines.append("")
            lines.append("Columns this schema has that the reference does not:")
            lines.append("")
            for name, reason in added:
                lines.append("- `%s` -- %s" % (name, reason))
        lines.append("")

    lines += [
        "## What this does not claim",
        "",
        "The database is the least observable surface this project has, and a column being stored here is not",
        "the same as a feature working. What the check does claim is narrower and worth more: no column of the",
        "reference's schema has been forgotten, and every one that is absent is absent for a recorded reason.",
        "",
        "The two mappings a reader should look at first are `session.user_id` -> `session.portal_user` (the",
        "portal identity the client presents, which the reference calls `user_id`) and `character.charname` ->",
        "`characters.name` (UTF-16 units on the wire, text here).",
        "",
    ]
    document = "\n".join(lines)

    if args.write:
        with io.open(args.out, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(document)
        print("wrote %s: %d reference column(s)" % (args.out, len(rows)))
        # Writing is not checking: report what a check would say, but do not fail a deliberate rewrite.
        if unexplained:
            print(
                "%d column(s) have no explanation yet -- run --check to see them"
                % len(unexplained)
            )
        return 0

    if not os.path.exists(args.out):
        print("FAIL  no document at {}".format(args.out))
        return 1

    existing = io.open(args.out, "r", encoding="utf-8").read()
    if existing != document:
        print(
            "FAIL  {} is out of date with the reference and the database; regenerate with "
            "python tools/parity-schema.py --write docs/12-schema-parity.md".format(args.out)
        )
        return 1

    if unchecked:
        print(
            "skipped the stored/derived half: no database to ask ({} column(s) unverified)".format(
                len(unchecked)
            )
        )
    if unexplained:
        for table, column, state, detail in unexplained:
            print("FAIL  {}.{}: {}".format(table, column, detail))
        return 1

    covered = {}
    for table, _column, state, _detail in rows:
        covered[state] = covered.get(state, 0) + 1
    print(
        "the schema accounts for all {} reference column(s): {}".format(
            len(rows),
            ", ".join("%d %s" % (count, state) for state, count in sorted(covered.items())),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
