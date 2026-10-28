#!/usr/bin/env python3
"""Map the reference's own test plan onto the checks this repository actually has.

The reference declares sixteen test items (`T0`-`T15` in `PreSearing/docs/04-scope.md`) and each one says what
"pass" means. That is the closest thing either project has to a definition of the observable surface, and
until this tool existed the mapping from those items to this repo's checks was hand-written prose in
`docs/10-parity-queue.md`.

So here it is as data:

  * the items and their pass criteria are **parsed** from the reference, not typed;
  * the coverage of each is a declared list of check names -- test function names and tool invocations;
  * every declared check is **verified to exist** (the test function is in this repository's sources, the
    tool file is on disk), so a renamed test breaks this check instead of quietly rotting the document;
  * an item the reference adds and this table does not know about **fails the check**.

The three states, which are the whole point of writing it down:

  * `measured` -- a check in this repo covers it, and the check exists;
  * `hand` -- it needs a real client and a human watching two windows; recorded as such, never claimed;
  * `blocked` -- it needs a capture the reference cannot produce (its own handler is the unfinished one).

`--write` regenerates `docs/14-test-plan.md`. `--check` fails on a missing check, an unknown item, or a stale
document.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import io
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCE_REPO = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_PLAN = os.path.join(REFERENCE_REPO, "docs", "04-scope.md")
DEFAULT_OUT = os.path.join(REPO, "docs", "14-test-plan.md")

# `### T7 - Character select -> map transfer -> spawn` and its `**Pass:**` paragraph. The dash the reference
# uses is an em dash, so the pattern accepts either.
ITEM_RE = re.compile(r"^#{2,4}\s*(T\d+)\s*[-\u2014]+\s*(.+)$", re.MULTILINE)
PASS_RE = re.compile(r"\*\*Pass:\*\*\s*(.+?)(?:\n\n|\Z)", re.DOTALL)

# T item -> (state, checks, why). Every check is verified below: a `test:<name>` has to be a test function in
# this repository, a `tool:<path>` has to be a file, a `doc:<path>` has to be a document.
COVERAGE = {
    "T0": ("measured", ["tool:tools/test-battery.ps1", "tool:crates/asura-db/src/migrations.rs"],
           "the battery builds and migrates; the schema's own version table is asserted"),
    "T1": ("measured", ["test:the_four_endpoints_answer_what_the_reference_answers",
                        "test:the_http_framing_round_trips",
                        "test:an_unknown_email_is_400_who_are_you"],
           "the gate is this project's own `crates/asura-webgate`, a port of the reference's `webgate.py` "
           "answered from the database rows `tools/seed-dev.ps1` writes; the four calls, their exact XML and "
           "the 400/401 refusals are asserted, and the `(UserId, Token)` pair it returns is the pair the "
           "auth service resolves (the live-database half is pinned by `asura_db::portal`)"),
    "T2": ("measured", ["test:a_client_logs_in_and_receives_the_whole_login_batch", "test:a_client_logs_in_travels_and_reaches_its_map"],
           "both services bind and answer; the battery's steps 6 and 7 run them"),
    "T3": ("measured", ["test:a_client_logs_in_and_receives_the_whole_login_batch", "test:an_unknown_header_is_an_error_rather_than_a_guess"],
           "key exchange, framing and the version message are asserted at the protocol layer"),
    "T4": ("measured", ["test:a_presented_pair_resolves_to_its_account", "test:a_closed_session_cannot_log_in"],
           "the login decision, its refusals included"),
    "T5": ("measured", ["test:a_client_logs_in_and_receives_the_whole_login_batch", "test:a_character_list_is_scoped_to_its_own_account"],
           "the EULA is accepted at the protocol level and the character list is the batch's first message"),
    "T6": ("measured", ["test:a_character_is_created_in_a_creation_instance", "test:the_character_settings_come_back_from_the_row_the_creation_wrote"],
           "the creation instance, the four handlers, and the rows the confirm writes"),
    "T7": ("measured", ["test:a_client_logs_in_travels_and_reaches_its_map", "test:a_placed_client_is_loaded_into_its_map", "test:a_teleport_redirects_to_the_target_map_and_keeps_its_seat"],
           "travel, the redirect, the load block and the spawn point, end to end"),
    "T8": ("measured", ["test:the_archive_index_finds_the_blobs_the_map_catalogue_names", "test:a_map_blob_decompresses_to_the_containers_own_signature", "test:every_catalogue_blob_imports_to_the_references_own_fingerprint"],
           "the archive reader, the decompressor and the importer, against the reference's own tool"),
    "T9": ("hand", ["test:a_second_player_is_introduced_to_the_first_and_withdrawn_again"],
           "the server half is asserted (introduction, withdrawal, movement broadcasts); *seeing* the other agent needs a second real client"),
    "T10": ("measured", ["test:a_walk_is_answered_with_the_first_waypoint_of_the_servers_own_path", "test:a_walk_the_finder_refuses_is_answered_with_the_agents_position", "test:every_case_produces_the_references_own_waypoints"],
            "the walk is the reference's own path, refused the reference's way, and the finder matches it case by case; the client's *smoothness* is the hand half"),
    "T11": ("hand", ["test:the_chat_messages_a_line_produces_are_the_references_pair"],
            "a spoken line is sent as the reference sends it; whether a client *renders* it is the hand half, and neither server has ever managed it"),
    "T12": ("blocked", ["test:a_drop_deducts_the_stack_and_says_nothing", "test:a_drop_sends_nothing_because_both_candidates_crashed_a_client"],
            "the reference's own drop handler is unfinished (a TODO for stacks, no removal, the item's id as the model id) and no capture holds a working drop; this server deducts and sends nothing, which is measured, and the ground item waits on a capture that does not exist"),
    "T13": ("measured", ["test:an_idle_pair_keeps_its_rhythm_and_its_count", "test:a_ten_minute_idle_pair_keeps_its_rhythm"],
            "cadence, accounting and the working set over ten minutes, all asserted: the long variant samples "
            "the process every fifteen seconds and fails on more than 8 MiB of growth, after self-checking "
            "the sampler with a touched 16 MiB allocation"),
    "T14": ("measured", ["test:a_character_comes_back_where_it_left", "test:a_closed_session_never_resolves_again"],
            "disconnect, the agent removal, and a fresh login"),
    "T15": ("measured", ["tool:tools/parity-report.py", "tool:tools/parity-emit.py", "tool:tools/parity-summary.py"],
            "the log audit is generated rather than grepped: dispatch, emit surface and the state page"),
}


def read(path):
    return io.open(path, "r", encoding="utf-8", errors="replace").read()


def plan_items(path):
    """`### T<n> - <title>` plus its `**Pass:**` criterion, from the reference's own document."""
    text = read(path)
    items = []
    matches = list(ITEM_RE.finditer(text))
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        body = text[match.end() : end]
        criterion = PASS_RE.search(body)
        items.append(
            (
                match.group(1),
                re.sub(r"\s+", " ", match.group(2)).strip(),
                re.sub(r"\s+", " ", criterion.group(1)).strip() if criterion else "",
            )
        )
    return items


def test_names():
    """Every `fn <name>()` in this repository's sources, which is what a `test:` claim points at."""
    names = set()
    pattern = re.compile(r"\bfn\s+([a-z0-9_]+)\s*\(")
    for root, _dirs, files in os.walk(os.path.join(REPO, "crates")):
        if "target" in root.split(os.sep):
            continue
        for name in files:
            if not name.endswith(".rs"):
                continue
            for found in pattern.findall(read(os.path.join(root, name))):
                names.add(found)
    return names


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plan", default=DEFAULT_PLAN)
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    if not args.write and not args.check:
        parser.error("nothing to do: pass --check or --write")
    if not os.path.exists(args.plan):
        print("skipped: no reference plan at {} (pass --plan)".format(args.plan))
        return 0

    items = plan_items(args.plan)
    if not items:
        print("FAIL  no T items in {}".format(args.plan))
        return 1

    names = test_names()
    problems = []
    unknown = [item for item in items if item[0] not in COVERAGE]
    for item_id, title, _criterion in unknown:
        problems.append("{} ({}): the reference has it and this table has no coverage claim".format(item_id, title))

    missing = []
    for item_id, _title, _criterion in items:
        state, checks, _why = COVERAGE.get(item_id, ("measured", [], ""))
        for check in checks:
            kind, _, where = check.partition(":")
            if kind == "test" and where not in names:
                missing.append("{}: the test `{}` is not in this repository".format(item_id, where))
            elif kind == "tool" and not os.path.exists(os.path.join(REPO, where)):
                missing.append("{}: the tool `{}` is not in this repository".format(item_id, where))
            elif kind not in ("test", "tool"):
                missing.append("{}: `{}` is not a `test:` or `tool:` claim".format(item_id, check))
    problems.extend(missing)

    counts = {}
    for item_id, _title, _criterion in items:
        state = COVERAGE.get(item_id, ("measured", [], ""))[0]
        counts[state] = counts.get(state, 0) + 1

    lines = [
        "# 14 - The reference's test plan, mapped onto the checks in this repository",
        "",
        "Generated by `tools/parity-testplan.py` from `PreSearing/docs/04-scope.md` and this repository's own",
        "test functions and tools. The reference's plan is the closest thing either project has to a",
        "definition of the observable surface: sixteen items, each with a stated pass criterion. This page",
        "says, for each one, which check here covers it -- and every named check is verified to exist, so a",
        "renamed test breaks the check rather than quietly rotting the page.",
        "",
        "```powershell",
        "python tools/parity-testplan.py --check",
        "```",
        "",
        "| state | items |",
        "|---|---|",
    ]
    for state in ("measured", "hand", "blocked", "absent"):
        if state in counts:
            lines.append("| %s | %d |" % (state, counts[state]))
    lines += [
        "",
        "`measured` is a check in this repository. `hand` needs a rig run or a human watching two windows, and is",
        "recorded with whatever automated half exists. `blocked` needs a capture the reference cannot produce,",
        "because its own handler is the unfinished one. `absent` is a component the reference has and this",
        "stack does not -- named, explained, and not pretended away.",
        "",
    ]
    for item_id, title, criterion in items:
        state, checks, why = COVERAGE.get(item_id, ("measured", [], ""))
        lines.append("## {}: {} ({})".format(item_id, title, state))
        lines.append("")
        if criterion:
            lines.append("**The reference's pass criterion:** {}".format(criterion))
            lines.append("")
        lines.append("**Here:** {}".format(why))
        lines.append("")
        for check in checks:
            kind, _, where = check.partition(":")
            lines.append("- `{}` -- {}".format(where, "a test" if kind == "test" else "a tool"))
        lines.append("")
    document = "\n".join(lines)

    if args.write:
        with io.open(args.out, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(document)
        print(
            "wrote {}: {} item(s) -- {}".format(
                args.out,
                len(items),
                ", ".join("%d %s" % (count, state) for state, count in sorted(counts.items())),
            )
        )
        return 0

    if not os.path.exists(args.out):
        problems.append("no document at {}".format(args.out))
    elif read(args.out) != document:
        problems.append("{} is out of date; regenerate with python tools/parity-testplan.py --write".format(args.out))

    if problems:
        for problem in problems:
            print("FAIL  " + problem)
        return 1

    print(
        "the test plan is accounted for: {} item(s) -- {}".format(
            len(items),
            ", ".join("%d %s" % (count, state) for state, count in sorted(counts.items())),
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
