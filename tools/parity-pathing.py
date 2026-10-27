#!/usr/bin/env python3
"""Fingerprint the map importer against the reference's own tool.

`GmMapDataImport.c` turns a map blob into a path graph, and the only way to know a port of it produced
the *reference's* graph rather than bytes that merely parsed is to ask the reference. Its own
`tools/gwdat-pathing.c` (built as `gwdat_pathing.exe`) imports a blob with `GmImportMap` and prints

    file_id,size,planes,trapezoids,min_x,min_y,max_x,max_y

-- plane count, trapezoid count, and the box every trapezoid corner falls in. Three numbers that agree
exactly only when both sides read the same file the same way.

This tool holds the three sides together:

  * the **reference** binary, run now, for the blobs `tools/map-config.sql` names;
  * the **fixture** committed beside the tests (`--check` fails when the reference has moved on);
  * **our** importer, via `cargo run -p asura-world --example map-census -- --fingerprint`.

`--write` regenerates the fixture from the reference. `--check` compares all three and fails on any
difference, printing the column that differs.

Missing inputs are not failures: without `Gw.dat`, the reference checkout's built tool, or cargo, each
step says so and the tool exits 0, the same way the archive-backed tests skip.

This file stays ASCII-only (docs/HANDOFF.md).
"""

import argparse
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCE_REPO = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_GWDAT = os.path.join(REFERENCE_REPO, "Gw.dat")
DEFAULT_EXE = os.path.join(REFERENCE_REPO, "build", "RelWithDebInfo", "gwdat_pathing.exe")
DEFAULT_FIXTURE = os.path.join(
    REPO, "crates", "asura-world", "tests", "fixtures", "gwdat-pathing.csv"
)
DEFAULT_CATALOGUE = os.path.join(REPO, "tools", "map-config.sql")

HEADER = "file_id,size,planes,trapezoids,min_x,min_y,max_x,max_y"

# A generated catalogue row: (map, type, blob, spawn count, x, y, plane, 'name'), followed by a comma.
# The name may hold commas and doubled apostrophes, so it is matched to the closing quote instead of by
# splitting on separators.
ROW_RE = re.compile(
    r"^\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,"
    r"\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(\d+)\s*,\s*'(.*)'\s*\)"
)


def catalogue_blobs(path):
    """The distinct map blobs the catalogue names, in id order."""
    if not os.path.exists(path):
        return []
    blobs = set()
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            match = ROW_RE.match(line)
            if match:
                blobs.add(int(match.group(3)))
    return sorted(blobs)


def parse_rows(text):
    """(file_id, values) pairs from fingerprint CSV text, header skipped."""
    rows = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("file_id"):
            continue
        fields = [field.strip() for field in line.split(",")]
        if len(fields) != 8:
            continue
        rows[int(fields[0], 16)] = fields
    return rows


def run_reference(exe, gwdat, blob):
    """The reference's fingerprint of one blob, or None with the reason."""
    result = subprocess.run(
        [exe, gwdat, "--dump", hex(blob)],
        capture_output=True,
        text=True,
        cwd=os.path.dirname(exe) or None,
    )
    if result.returncode != 0:
        return None, (result.stderr or result.stdout).strip()
    for line in result.stdout.splitlines():
        if line.startswith("0x"):
            return parse_rows(line).get(blob), None
    return None, "the reference printed no row"


def run_ours(gwdat, catalogue):
    """Our importer's fingerprints, by running the example the tests share."""
    result = subprocess.run(
        [
            "cargo",
            "run",
            "--offline",
            "-q",
            "-p",
            "asura-world",
            "--example",
            "map-census",
            "--",
            gwdat,
            "--fingerprint",
            catalogue,
        ],
        capture_output=True,
        text=True,
        cwd=REPO,
    )
    if result.returncode != 0:
        return None, (result.stderr or result.stdout).strip()
    return parse_rows(result.stdout), None


def column_differences(reference, candidate):
    """Which columns of two fingerprint rows differ, by name."""
    names = HEADER.split(",")
    return [
        name
        for name, left, right in zip(names, reference, candidate)
        if left != right
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--gwdat", default=os.environ.get("ASURA_GW_DAT", DEFAULT_GWDAT))
    parser.add_argument("--reference", default=DEFAULT_EXE, help="the reference's gwdat_pathing binary")
    parser.add_argument("--fixture", default=DEFAULT_FIXTURE)
    parser.add_argument("--catalogue", default=DEFAULT_CATALOGUE)
    parser.add_argument("--write", action="store_true", help="rewrite the fixture from the reference")
    parser.add_argument("--check", action="store_true", help="compare reference, fixture and this repo")
    args = parser.parse_args()

    if not args.write and not args.check:
        parser.error("nothing to do: pass --check or --write")

    blobs = catalogue_blobs(args.catalogue)
    if not blobs:
        print("skipped: {} names no map blobs".format(args.catalogue))
        return 0
    print("catalogue: {} map blob(s) in {}".format(len(blobs), args.catalogue))

    have_gwdat = os.path.exists(args.gwdat)
    have_exe = os.path.exists(args.reference)
    if not have_gwdat:
        print("skipped: no Gw.dat at {} (set ASURA_GW_DAT or pass --gwdat)".format(args.gwdat))
        return 0
    if not have_exe:
        print("skipped: no reference tool at {} (pass --reference)".format(args.reference))
        return 0

    reference = {}
    for blob in blobs:
        row, reason = run_reference(args.reference, args.gwdat, blob)
        if row is None:
            print("FAIL  {:#010X}: the reference could not read it: {}".format(blob, reason))
            return 1
        reference[blob] = row
    print("reference: {} row(s) from {}".format(len(reference), args.reference))

    if args.write:
        lines = [HEADER]
        for blob in sorted(reference):
            lines.append(",".join(reference[blob]))
        with open(args.fixture, "w", encoding="utf-8", newline="\n") as handle:
            handle.write("\n".join(lines) + "\n")
        print("wrote {} row(s) to {}".format(len(reference), args.fixture))
        return 0

    failures = 0

    fixture = {}
    if os.path.exists(args.fixture):
        with open(args.fixture, "r", encoding="utf-8") as handle:
            fixture = parse_rows(handle.read())
    else:
        print("FAIL  no fixture at {}".format(args.fixture))
        failures += 1

    for blob in sorted(reference):
        if blob not in fixture:
            print("FAIL  {:#010X}: the fixture does not carry it".format(blob))
            failures += 1
            continue
        differing = column_differences(reference[blob], fixture[blob])
        if differing:
            print(
                "FAIL  {:#010X}: the fixture has moved on in {}".format(
                    blob, ", ".join(differing)
                )
            )
            print("        reference: {}".format(",".join(reference[blob])))
            print("        fixture:   {}".format(",".join(fixture[blob])))
            failures += 1

    ours, reason = run_ours(args.gwdat, args.catalogue)
    if ours is None:
        print("FAIL  the importer did not run: {}".format(reason))
        failures += 1
    else:
        for blob in sorted(reference):
            if blob not in ours:
                print("FAIL  {:#010X}: this repo's importer produced no row".format(blob))
                failures += 1
                continue
            differing = column_differences(reference[blob], ours[blob])
            if differing:
                print(
                    "FAIL  {:#010X}: this repo differs from the reference in {}".format(
                        blob, ", ".join(differing)
                    )
                )
                print("        reference: {}".format(",".join(reference[blob])))
                print("        this repo: {}".format(",".join(ours[blob])))
                failures += 1

    if failures:
        print("{} difference(s) between the reference, the fixture and this repo".format(failures))
        return 1

    trapezoids = sum(int(row[3]) for row in reference.values())
    planes = sum(int(row[2]) for row in reference.values())
    print(
        "the importer matches the reference on {} blob(s): {} plane(s), {} trapezoid(s), "
        "same bounding boxes".format(len(reference), planes, trapezoids)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
