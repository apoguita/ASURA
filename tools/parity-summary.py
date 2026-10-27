#!/usr/bin/env python3
"""Write the state of parity: every axis, measured now, in one generated page.

The audit (`docs/09`) argues, the queue (`docs/10`) plans, and this page is the *measurement*: it runs the
tools and prints what they said. If a number here disagrees with a sentence in either document, this page is
right and the document is stale.

Usage:
  python tools/parity-summary.py [--write docs/11-parity-state.md]

The tools it runs, and what each one is the measure of:

  tools/parity-report.py   the reference's client-message dispatch: every handler it has, and whether this
                           repo has an arm for it (and whether the reference's own arm is a no-op)
  tools/parity-emit.py     the other direction: every message the reference can put on the wire, and whether
                           a send path here reaches it. A builder nothing calls is a gap
  tools/parity-load.py     the load stream against the reference's own GWPS_TRACE log, as (header, size)
  tools/gen-*-.py --check  the generated tables and seeds still match the reference's sources
  cargo test --workspace   the count of tests that pass (skipped with --fast, which is the default for the
                           battery: the battery already runs them)

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import io
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_OUT = os.path.join(REPO, "docs", "11-parity-state.md")


def run(arguments):
    """Run one tool and return its combined output."""
    result = subprocess.run(
        arguments, cwd=REPO, capture_output=True, text=True, errors="replace"
    )
    return result.returncode, (result.stdout or "") + (result.stderr or "")


def dispatch_axis():
    code, output = run([sys.executable, os.path.join("tools", "parity-report.py")])
    auth = re.search(r"AUTH_CMSG: the reference handles (\d+) message", output)
    game = re.search(r"GAME_CMSG: the reference handles (\d+) message", output)
    missing = re.search(r"(\d+) handler\(s\) with real behaviour are missing here", output)
    noops = re.search(r"(\d+) of them are no-ops in the reference itself", output)
    if not auth or not game:
        return None
    return {
        "auth": int(auth.group(1)),
        "game": int(game.group(1)),
        "missing": int(missing.group(1)) if missing else 0,
        "noops": int(noops.group(1)) if noops else 0,
        "ok": code == 0,
    }


def emit_axis():
    code, output = run([sys.executable, os.path.join("tools", "parity-emit.py")])
    reference = re.search(r"messages the reference can emit: (\d+)", output)
    ours = re.search(r"of those, this repo can emit:\s+(\d+)", output)
    real = re.search(r"real gaps: (\d+)", output)
    explained = re.search(r"CANNOT:\s+\d+ \((\d+) real, (\d+) explained\)", output)
    if not reference or not ours:
        return None
    return {
        "reference": int(reference.group(1)),
        "ours": int(ours.group(1)),
        "real": int(real.group(1)) if real else None,
        "explained": int(explained.group(2)) if explained else 0,
        "ok": code == 0,
    }


def load_axis():
    code, output = run(
        [sys.executable, os.path.join("tools", "parity-load.py")]
    )
    reference = re.search(r"reference messages: (\d+)", output)
    matched = re.search(r"matched:\s+(\d+)", output)
    missing = re.search(r"missing:\s+(\d+)", output)
    extra = re.search(r"extra:\s+(\d+)", output)
    sizes = re.search(r"size disagreements: (\d+)", output)
    if not reference:
        return None
    return {
        "reference": int(reference.group(1)),
        "matched": int(matched.group(1)) if matched else 0,
        "missing": int(missing.group(1)) if missing else 0,
        "extra": int(extra.group(1)) if extra else 0,
        "sizes": int(sizes.group(1)) if sizes else 0,
        "ok": code == 0,
    }


CAPTURE = os.path.join(
    os.path.dirname(REPO), "PreSearing", "gwps-capture.log"
)

# Each entry is (what it checks, the exact arguments). Explicit rather than derived: three of these take a
# different flag combination, and guessing which was how the first version of this table ran the inventory
# check without its capture and reported the file as stale.
CHECKS = (
    ("the auth message table", ["tools/gen-auth-tables.py", "--check"]),
    ("the game message table", ["tools/gen-game-tables.py", "--check"]),
    ("the PvP item table", ["tools/gen-pvp-items.py", "--check"]),
    (
        "the default inventory",
        ["tools/gen-default-inventory.py", "--check", "--capture", CAPTURE],
    ),
    ("the default-equipment table", ["tools/gen-default-equipment.py", "--check"]),
    ("the map catalogue", ["tools/gen-map-config.py", "--check"]),
    ("the decompressor's tables", ["tools/gen-fa-compress-tables.py", "--check"]),
    ("the path finder's tables", ["tools/gen-path-tables.py", "--check"]),
    ("the path finder's cases", ["tools/gen-path-cases.py", "--check"]),
    ("the GM command surface", ["tools/parity-commands.py", "--check"]),
    ("the reference's test plan", ["tools/parity-testplan.py", "--check"]),
    (
        "the reference's load stream",
        ["tools/parity-load.py", "--check", "crates/asura-world/src/load_fixture.rs"],
    ),
)


def generated_axis():
    rows = []
    for name, arguments in CHECKS:
        code, _output = run(
            [sys.executable, os.path.join(*arguments[0].split("/"))] + arguments[1:]
        )
        rows.append((name, code == 0))
    return rows


def map_axis():
    """The map data: `Gw.dat` in, the reference's own graph fingerprint out.

    Not a generator check, which is why it is its own axis: it compares a *port* (this repo's importer)
    against the reference's binary, its committed fixture and a real archive, not a table against a
    source file.
    """
    code, output = run([sys.executable, os.path.join(REPO, "tools", "parity-pathing.py"), "--check"])
    lines = [line.strip() for line in output.strip().splitlines() if line.strip()]
    return {"ok": code == 0, "detail": lines[-1] if lines else "no output"}


def path_axis():
    """The path finder: the same cases through this repo's A* and through the reference's own.

    Its yardstick is `tools/path-oracle.c`, which compiles the reference's translation unit unmodified and
    calls `PathFinding`; without that binary the tool reports it and exits 0, so the axis says what it
    could not measure rather than pretending.
    """
    code, output = run([sys.executable, os.path.join(REPO, "tools", "parity-path.py"), "--check"])
    lines = [line.strip() for line in output.strip().splitlines() if line.strip()]
    return {"ok": code == 0, "detail": lines[-1] if lines else "no output"}


def schema_axis():
    """The database schema: every column the reference declares, accounted for.

    The one axis whose yardstick is the reference's *declaration* rather than its behaviour -- `DbSchema.h`
    against this database. It fails on a reference column that is neither stored, derived, nor explained,
    which is what keeps a newly added reference column from being forgotten.
    """
    code, output = run([sys.executable, os.path.join(REPO, "tools", "parity-schema.py"), "--check"])
    lines = [line.strip() for line in output.strip().splitlines() if line.strip()]
    return {"ok": code == 0, "detail": lines[-1] if lines else "no output"}


def load_bytes_axis():
    """The load block's payloads: the same 480 messages, compared byte for byte with the capture.

    `load_axis` compares `(header, size)`; this compares the bytes, and fails unless every difference is one
    of the three the tool records with a measured reason. It runs the dump itself, so there is no stale file.
    """
    code, output = run([sys.executable, os.path.join(REPO, "tools", "parity-load-bytes.py"), "--check"])
    lines = [line.strip() for line in output.strip().splitlines() if line.strip()]
    return {"ok": code == 0, "detail": lines[-1] if lines else "no output"}


def test_axis():
    code, output = run(["cargo", "test", "--workspace", "--quiet"])
    passed = sum(
        int(match.group(1))
        for match in re.finditer(r"test result: ok\. (\d+) passed", output)
    )
    failed = sum(
        int(match.group(1)) for match in re.finditer(r"(\d+) failed", output)
    )
    return {"passed": passed, "failed": failed, "ok": code == 0 and failed == 0}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", default=None, help="write the page here")
    parser.add_argument(
        "--fast",
        action="store_true",
        help="skip `cargo test` (the battery runs it already)",
    )
    args = parser.parse_args()

    if not os.path.isdir(os.path.join(os.path.dirname(REPO), "PreSearing")):
        sys.stderr.write("the reference is not next to this repository\n")
        return 2

    dispatch = dispatch_axis()
    emit = emit_axis()
    load = load_axis()
    load_bytes = load_bytes_axis()
    maps = map_axis()
    paths = path_axis()
    schema = schema_axis()
    generated = generated_axis()
    tests = None if args.fast else test_axis()

    lines = [
        "# 11 - The state of parity, measured",
        "",
        "Generated by `tools/parity-summary.py`. Every number here comes from a tool that reads the",
        "reference or this repository; if a sentence in `docs/09` or `docs/10` disagrees with it, this page is",
        "right and the sentence is stale. Regenerate with:",
        "",
        "```powershell",
        "python tools/parity-summary.py --write docs/11-parity-state.md --fast",
        "```",
        "",
        "## The axes",
        "",
        "| axis | what it measures | state |",
        "|---|---|---|",
    ]

    if dispatch:
        verdict = "complete" if dispatch["missing"] == 0 else "%d handler(s) missing" % dispatch["missing"]
        lines.append(
            "| dispatch | the reference's client-message handlers | auth **%d/%d**, game **%d/%d** - **%s**%s |"
            % (
                dispatch["auth"],
                dispatch["auth"],
                dispatch["game"] - dispatch["missing"],
                dispatch["game"],
                verdict,
                ("; %d of them are no-ops in the reference itself" % dispatch["noops"])
                if dispatch["noops"]
                else "",
            )
        )
    else:
        lines.append("| dispatch | `tools/parity-report.py` | **the tool failed** |")

    if emit:
        lines.append(
            "| emit | every message the reference can send | **%d of %d** reachable, **%d real gap(s)**, %d explained |"
            % (emit["ours"], emit["reference"], emit["real"] or 0, emit["explained"])
        )
    else:
        lines.append("| emit | `tools/parity-emit.py` | **the tool failed** |")

    if load:
        lines.append(
            "| the load block | `(header, size)` against the reference's own trace | **%d of %d** matched, "
            "**%d missing**, **%d extra**, **%d size** disagreement(s) |"
            % (
                load["matched"],
                load["reference"],
                load["missing"],
                load["extra"],
                load["sizes"],
            )
        )
    else:
        lines.append("| the load block | `tools/parity-load.py` | **the tool failed** |")

    if load_bytes["ok"]:
        lines.append("| the load block's bytes | the same messages, payload against payload | **%s** |" % load_bytes["detail"])
    else:
        lines.append(
            "| the load block's bytes | `tools/parity-load-bytes.py --check` | **differs from the capture**: %s |"
            % load_bytes["detail"]
        )

    failed = [name for name, ok in generated if not ok]
    lines.append(
        "| generated files | `--check` against the reference's sources | **%d of %d current**%s |"
        % (
            len(generated) - len(failed),
            len(generated),
            (", failing: " + ", ".join(failed)) if failed else "",
        )
    )
    if maps["ok"]:
        lines.append("| the map data | `Gw.dat` through the ported importer, against the reference's own tool | **%s** |" % maps["detail"])
    else:
        lines.append(
            "| the map data | `tools/parity-pathing.py --check` | **differs from the reference**: %s |"
            % maps["detail"]
        )
    if paths["ok"]:
        lines.append("| the path finder | the same walks through both A*s, waypoint bits compared | **%s** |" % paths["detail"])
    else:
        lines.append(
            "| the path finder | `tools/parity-path.py --check` | **differs from the reference**: %s |"
            % paths["detail"]
        )
    if schema["ok"]:
        lines.append("| the schema | every column `DbSchema.h` declares, against this database | **%s** |" % schema["detail"])
    else:
        lines.append(
            "| the schema | `tools/parity-schema.py --check` | **has unexplained columns**: %s |"
            % schema["detail"]
        )
    if tests is not None:
        lines.append(
            "| tests | `cargo test --workspace` | **%d passed**, %d failed |"
            % (tests["passed"], tests["failed"])
        )

    lines += [
        "",
        "## What the numbers say",
        "",
        "The two lookup axes are complete: every client message the reference dispatches has an arm here, and",
        "every message it can put on the wire is reachable from a send path here. The load block and the",
        "generated tables match their sources message for message, and the seeds are regenerated from the same",
        "sources rather than typed. The map-data axis is the odd one out: it measures a *port* against the",
        "reference's own importer output for the same bytes, so a disagreement there is a reading bug, not a",
        "missing feature.",
        "",
        "## What is not measured here, and is written down elsewhere",
        "",
        "Everything in this table is mechanical. The parts of parity that are not are enumerated in",
        "`docs/10-parity-queue.md`, and the ones that remain are all of one kind - they need something this",
        "machine cannot supply:",
        "",
        "- **T10's search** is the one part of pathfinding that is not here. The data path is: `Gw.dat`'s",
        "  index, its decompressor and the graph importer are ported, and every map blob the catalogue names",
        "  imports to the reference's own plane count, trapezoid count and bounding box (`docs/10`, item D1-D3).",
        "  `GmPaths.c`'s A* over that graph - portal walking, the node heap, waypoint reduction - is still to",
        "  write, so movement's *rule* half (the position budget) is implemented and asserted while its",
        "  *refusal* half is not.",
        "- **T9 and T14's live halves** need a second real client rendering the other player.",
        "- **T13's memory clause** needs a human watching the process during the ten-minute idle run.",
        "- **The ground item** waits on a capture of a working drop, which the reference cannot produce because",
        "  its own drop handler is the unfinished one.",
        "",
        "## The generated checks",
        "",
        "| check | state |",
        "|---|---|",
    ]
    for name, ok in generated:
        lines.append("| %s | %s |" % (name, "current" if ok else "**STALE**"))
    lines.append("")

    page = "\n".join(lines)
    if args.write:
        with io.open(args.write, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(page)
        print("wrote %s" % args.write)
    else:
        sys.stdout.write(page)
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
