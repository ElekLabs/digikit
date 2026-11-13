#!/usr/bin/env python3
"""addrmap_export: the dev-only address map (layer fw) of one firmware image, from the reverse-engineering databases.

    uv run python -m tools.coldfire.addrmap_export dt2-1.17            # writes out/addrmap/dt2-1.17.dev.json
    uv run python -m tools.coldfire.addrmap_export dn2-1.12 --out FILE

The ColdFire functions come from out/cfdb (the function table, with the image's own name list where it has one: `fw.CF.names`), the SHARC
functions from out/sharcdb. The file has the format of docs/ADDRMAP.md (kind `dev`) and is keyed by the SHA-256 of the stock image, taken from
the port file `devices/ports/IMAGE.toml`. It is derived from firmware: it stays under out/ and is never committed or shipped.
`patchkit addr` reads it together with the ship map `patchkit apply --addrmap` writes.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

from tools.coldfire import fw

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "out" / "addrmap"
FORMAT = "dk-addrmap"
VERSION = 1


def port_of(image: str) -> dict:
    path = ROOT / "devices" / "ports" / f"{image}.toml"
    if not path.is_file():
        raise fw.FwError(
            f"no port file {path.relative_to(ROOT)}: only ported images have an address map"
        )
    return tomllib.loads(path.read_text())["port"]


def entries(image: str) -> list[dict]:
    out: list[dict] = []
    cf = fw.CF(image)
    names = cf.names()
    for entry, end, name in cf.db.execute(
        "SELECT entry, end, name FROM functions WHERE image=? AND end>entry",
        (cf.db_name,),
    ):
        out.append(
            {
                "cpu": "cf",
                "addr": entry,
                "size": end - entry,
                "name": names.get(entry) or name,
                "layer": "fw",
            }
        )
    try:
        sharc = fw.Sharc(image)
    except fw.FwError:
        return out  # no SHARC database for this image: ColdFire names only
    for entry, end, name, label in sharc.db.execute(
        "SELECT entry_sw, end_sw, name, label FROM functions WHERE image=? AND end_sw>entry_sw",
        (sharc.image,),
    ):
        out.append(
            {
                "cpu": "sharc",
                "addr": entry,
                "size": end - entry,
                "name": name or label or f"FUN_{entry:06x}",
                "layer": "fw",
            }
        )
    return out


def export(image: str) -> dict:
    port = port_of(image)
    return {
        "format": FORMAT,
        "version": VERSION,
        "kind": "dev",
        "key": {
            "device": port["device"],
            "firmware": port["version"],
            "image_sha256": port["syx_sha256"],
        },
        "entries": sorted(
            entries(image), key=lambda e: (e["cpu"], e["addr"], e["name"])
        ),
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("image", help="dt2-1.17, dn2-1.12, ...")
    ap.add_argument(
        "--out", type=Path, help="the file (default out/addrmap/IMAGE.dev.json)"
    )
    args = ap.parse_args(argv)
    try:
        doc = export(args.image)
    except fw.FwError as e:
        print(f"addrmap_export: {e}", file=sys.stderr)
        return 1
    out = args.out or OUT / f"{args.image}.dev.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
    n = len(doc["entries"])
    print(f"{out}: {n} entries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
