#!/usr/bin/env python3
"""Compare this repo's path finder against the reference's own, case by case and bit for bit.

The reference refuses walks it cannot make (`GmPaths.c`'s A*), and "refuses the same walks, along the same
waypoints" is the parity claim. Nothing in the reference exposes that on the wire without a game client, so
`tools/path-oracle.c` compiles the reference's own translation unit unmodified and calls `PathFinding`
directly; this tool runs it and `cargo run --example path-census` over the same cases file and compares:

  * **result** -- whether a walk exists at all;
  * **count** -- how many waypoints it comes back with;
  * **waypoints** -- trap id and the raw bytes of each coordinate, so a one-ulp difference is visible.

Both sides print floats as IEEE-754 bit patterns in hex for exactly that reason.

The cases (`crates/asura-world/tests/fixtures/path-cases.txt`) are generated from the reference's own map
catalogue by `tools/gen-path-cases.py`: every map's spawn point, to itself, to four near offsets, to two
diagonal ones, and to a point outside the map -- so the run covers "yes", "no", and portal crossings between
planes.

`--write` records the reference's answers as the fixture the tests use. `--check` re-runs everything and
fails on any difference. Missing inputs (no `Gw.dat`, no oracle binary, no cargo) are not failures: each
step says what is missing and the tool exits 0, like the archive-backed tests skipping.

The oracle is run more than once per check, because the reference is *not* deterministic in one corner:
`PathBuildAddWaypointAndReduce` reads `t` and `s` uninitialised when a step is parallel to the line from
the current start point to the new point, and exactly one of the 96 cases hits that. Cases where the
reference disagrees with itself are reported as such, compared on `result` only, and listed in the fixture's
header -- a fixture cannot pin an answer the reference does not have.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCE_REPO = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_GWDAT = os.path.join(REFERENCE_REPO, "Gw.dat")
DEFAULT_ORACLE = os.path.join(REPO, "tools", "path-oracle.exe")
DEFAULT_CASES = os.path.join(
    REPO, "crates", "asura-world", "tests", "fixtures", "path-cases.txt"
)
DEFAULT_FIXTURE = os.path.join(
    REPO, "crates", "asura-world", "tests", "fixtures", "path-results.csv"
)

HEADER = "blob,src_x,src_y,src_plane,dst_x,dst_y,dst_plane,result,count,waypoints"
# The columns that identify a case; the rest is the answer.
CASE_FIELDS = 7


def run_oracle(oracle, gwdat, cases):
    result = subprocess.run(
        [oracle, gwdat, cases], capture_output=True, text=True
    )
    if result.returncode != 0:
        return None, (result.stderr or result.stdout).strip()
    rows = {}
    order = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line.startswith("0x"):
            continue
        fields = line.split(",")
        key = ",".join(fields[:CASE_FIELDS])
        rows[key] = line
        order.append(key)
    return (rows, order), None


def run_ours(gwdat, cases):
    result = subprocess.run(
        [
            "cargo",
            "run",
            "--offline",
            "-q",
            "-p",
            "asura-world",
            "--example",
            "path-census",
            "--",
            gwdat,
            cases,
        ],
        capture_output=True,
        text=True,
        cwd=REPO,
    )
    if result.returncode != 0:
        return None, (result.stderr or result.stdout).strip()
    rows = {}
    order = []
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line.startswith("0x"):
            continue
        fields = line.split(",")
        key = ",".join(fields[:CASE_FIELDS])
        rows[key] = line
        order.append(key)
    return (rows, order), None


def classify(runs):
    """Per case: every distinct answer the reference gave, in first-seen order."""
    answers = {}
    order = []
    for rows, run_order in runs:
        for key in run_order:
            if key not in answers:
                answers[key] = []
                order.append(key)
            line = rows[key]
            if line not in answers[key]:
                answers[key].append(line)
    return answers, order


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gwdat", default=os.environ.get("ASURA_GW_DAT", DEFAULT_GWDAT))
    parser.add_argument("--oracle", default=DEFAULT_ORACLE)
    parser.add_argument("--cases", default=DEFAULT_CASES)
    parser.add_argument("--fixture", default=DEFAULT_FIXTURE)
    parser.add_argument("--runs", type=int, default=3, help="how often to run the reference")
    parser.add_argument("--write", action="store_true", help="record the reference's answers")
    parser.add_argument("--check", action="store_true", help="compare this repo against them")
    args = parser.parse_args()

    if not args.write and not args.check:
        parser.error("nothing to do: pass --check or --write")

    for needed, what in ((args.gwdat, "Gw.dat"), (args.cases, "the cases file")):
        if not os.path.exists(needed):
            print("skipped: no {} at {} (pass --gwdat / --cases)".format(what, needed))
            return 0
    if not os.path.exists(args.oracle):
        print(
            "skipped: no oracle at {} -- build it with tools/build-path-oracle.ps1".format(
                args.oracle
            )
        )
        return 0

    runs = []
    for index in range(max(2, args.runs)):
        run, reason = run_oracle(args.oracle, args.gwdat, args.cases)
        if run is None:
            print("FAIL  the oracle did not run: {}".format(reason))
            return 1
        runs.append(run)
    answers, order = classify(runs)
    unstable = [key for key in order if len(answers[key]) > 1]
    stable = [key for key in order if len(answers[key]) == 1]
    print(
        "reference: {} case(s), {} deterministic, {} not".format(
            len(order), len(stable), len(unstable)
        )
    )

    if args.write:
        lines = [
            "# Generated by tools/parity-path.py --write from the reference's own path finder",
            "# (tools/path-oracle.c, compiled against {}'s sources). Do not edit.".format(
                os.path.basename(REFERENCE_REPO)
            ),
            "#",
            "# One row per case: the answer the reference gives. `result` is whether a walk exists,",
            "# `count` how many waypoints it comes back with, and each waypoint is",
            "# trap_id,x,y,plane with coordinates as IEEE-754 bit patterns.",
            "#",
        ]
        if unstable:
            lines += [
                "# The reference answers these cases with more than one result across {} runs: it reads".format(
                    max(2, args.runs)
                ),
                "# `t` and `s` uninitialised when a step is parallel to the line it is being cut against",
                "# (PathBuildAddWaypointAndReduce), so there is no answer to pin. They are listed with the",
                "# `result` all runs agree on, and the tests compare only that.",
            ]
            for key in unstable:
                result_field = answers[key][0].split(",")[7]
                lines.append("# unstable,{},{}".format(key, result_field))
            lines.append("#")
        lines.append(HEADER)
        for key in order:
            if key in unstable:
                continue
            lines.append(answers[key][0])
        with open(args.fixture, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("\n".join(lines) + "\n")
        print("wrote {} row(s) to {}".format(len(stable), args.fixture))
        return 0

    failures = 0

    fixture_rows = {}
    fixture_unstable = {}
    if not os.path.exists(args.fixture):
        print("FAIL  no fixture at {}".format(args.fixture))
        failures += 1
    else:
        with open(args.fixture, "r", encoding="utf-8") as handle:
            for line in handle:
                line = line.strip()
                if line.startswith("# unstable,"):
                    fields = line[len("# unstable,") :].split(",")
                    fixture_unstable[",".join(fields[:CASE_FIELDS])] = fields[CASE_FIELDS]
                    continue
                if not line or line.startswith("#") or line.startswith("blob,"):
                    continue
                fields = line.split(",")
                fixture_rows[",".join(fields[:CASE_FIELDS])] = line

    for key in order:
        if key in unstable:
            continue
        if key not in fixture_rows:
            print("FAIL  {}: the fixture does not carry this case".format(key))
            failures += 1
            continue
        if fixture_rows[key] != answers[key][0]:
            first = fixture_rows[key].split(",")
            second = answers[key][0].split(",")
            differing = [
                name
                for name, left, right in zip(
                    HEADER.split(","), first, second
                )
                if left != right
            ]
            print(
                "FAIL  {}: the reference now differs from the fixture in {}".format(
                    key, ", ".join(differing)
                )
            )
            failures += 1

    ours, reason = run_ours(args.gwdat, args.cases)
    if ours is None:
        print("FAIL  this repo's finder did not run: {}".format(reason))
        return 1
    rows, our_order = ours

    for key in order:
        if key not in rows:
            print("FAIL  {}: this repo produced no answer".format(key))
            failures += 1
            continue
        if key in unstable:
            # Only the `result` field can be compared: the reference has no single answer here.
            expected = {line.split(",")[7] for line in answers[key]}
            found = rows[key].split(",")[7]
            if found not in expected:
                print(
                    "FAIL  {}: this repo says result={}, the reference says {}".format(
                        key, found, "/".join(sorted(expected))
                    )
                )
                failures += 1
            continue
        if rows[key] != answers[key][0]:
            left = rows[key].split(",")
            right = answers[key][0].split(",")
            differing = [
                name
                for name, ours_field, theirs in zip(HEADER.split(","), left, right)
                if ours_field != theirs
            ]
            print(
                "FAIL  {}: differs from the reference in {}".format(
                    key, ", ".join(differing)
                )
            )
            print("        reference: {}".format(answers[key][0]))
            print("        this repo: {}".format(rows[key]))
            failures += 1

    if failures:
        print("{} difference(s) between the reference and this repo".format(failures))
        return 1

    solved = sum(1 for key in order if answers[key][0].split(",")[7] == "1")
    waypoints = sum(int(answers[key][0].split(",")[8]) for key in stable)
    print(
        "the path finder matches the reference on {} case(s): {} walk(s) found, {} waypoint(s) "
        "identical, {} case(s) compared on the answer alone".format(
            len(order), solved, waypoints, len(unstable)
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
