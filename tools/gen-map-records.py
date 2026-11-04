"""Write one folder per map under config/World/Vanilla/maps/<id>/ from the instance table's own data.

The table's 137 pairs are `(map id, instance value)`; 112 of the values are distinct, so the value is the
map's own and belongs in the map's file. The folders are deliberately flat for now: the campaign/region
level comes when the wiki's mapping is in the tree (docs/38 D36), and the reader walks recursively, so
moving a folder later changes no code.

    python tools/gen-map-records.py
"""

import json
import pathlib
import re

PACK = pathlib.Path("config/World/Vanilla")
TABLE = pathlib.Path("crates/asura-world/src/instance_table.rs")


def pairs() -> list[tuple[int, int]]:
    text = TABLE.read_text(encoding="utf-8")
    return [
        (int(map_id, 16), int(value, 16))
        for map_id, value in re.findall(r"\(0x([0-9A-Fa-f]{4}), 0x([0-9A-Fa-f]{8})\)", text)
    ]


def main() -> None:
    written = 0
    for map_id, value in pairs():
        folder = PACK / "maps" / f"{map_id:03d}"
        folder.mkdir(parents=True, exist_ok=True)
        record = {
            "map_id": map_id,
            "instance_value": value,
            "doc": "The instance value a live load sent with this map in GAME_SMSG_INSTANCE_TABLE "
            "(0x01A0): the client resolves a destination by this pair, so a map without one cannot be "
            "named. Copied from the recording of the load of instance 148 "
            "(recordings/client/feature/maps/pre-searing-load-3.jsonl). Maps that share a value belong "
            "to one base map or region, which is why the number is not always unique.",
        }
        (folder / "basic.json").write_text(
            json.dumps(record, indent=2) + "\n", encoding="utf-8", newline=""
        )
        written += 1
    print(f"wrote {written} map folders under {PACK / 'maps'}")


if __name__ == "__main__":
    main()
