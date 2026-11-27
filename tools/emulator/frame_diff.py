#!/usr/bin/env python3
"""Which words of the ColdFire -> DSP TX frame does a scenario's parameter change?

    uv run python -m tools.emulator.frame_diff dt2 REF NAME [NAME ...] [--file tests/regression/scenarios/dt2-sendfx.ts]
    uv run python -m tools.emulator.frame_diff dn2 REF NAME ... --file tests/regression/scenarios/dn2-sendfx.ts

Runs REF and the named scenarios (coupled-run, device ready inputs, DT2 on the one-sample card) with a
guest read of the TX frame buffer at the end of each (DT2 0x80005348, 0x802 bytes; DN2 0x80005e60, 0xa80
bytes: finding 04) and prints, per scenario, the 16-bit big-endian frame words that differ from REF as
`offset: ref -> value`. Together with the Sound/Fx read-back (tests/scenarios) this gives the
parameter -> frame offset map without reading the handler.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.emulator import coupled, digi_env, fxcheck  # noqa: E402
from tools.firmware import device as device_files  # noqa: E402

TX = {"dt2": (0x80005348, 0x802), "dn2": (0x80005E60, 0xA80)}


def frames(
    device: str, scenario_file: pathlib.Path, names: list[str], out: pathlib.Path
) -> dict[str, tuple[bytes, dict[str, int]]]:
    fxcheck.configure(device)
    if device == "dt2":
        coupled.enable_dt2_card()
    addr, size = TX[device]
    peeks = (f"tx={addr:#x}:{size}", fxcheck.SND_PEEK, fxcheck.FX_PEEK)
    rep: dict[str, Any] = coupled.run_batch(
        device, scenario_file, names, out, peeks=peeks, jobs=coupled.default_jobs()
    )
    image = fxcheck.DEFAULT_IMAGE
    maps = fxcheck.name_maps(image)
    res: dict[str, tuple[bytes, dict[str, int]]] = {}
    for s in rep["scenarios"]:
        pk = s.get("peek") or {}
        vals = fxcheck.decode_arrays(pk, maps)
        res[s["name"]] = (bytes.fromhex(pk["tx"]), vals)
    return res


def diff(a: bytes, b: bytes) -> list[tuple[int, int, int]]:
    return [
        (o, int.from_bytes(a[o : o + 2], "big"), int.from_bytes(b[o : o + 2], "big"))
        for o in range(0, min(len(a), len(b)) - 1, 2)
        if a[o : o + 2] != b[o : o + 2]
    ]


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("device", choices=device_files.shorts())
    p.add_argument("ref")
    p.add_argument("names", nargs="+")
    p.add_argument("--file", type=pathlib.Path, required=True)
    p.add_argument("--out", type=pathlib.Path)
    p.add_argument("--json", type=pathlib.Path, help="write the diffs as JSON")
    a = p.parse_args(argv)
    out = a.out or digi_env.run_dir("frame-diff")
    res = frames(a.device, a.file, [a.ref, *a.names], out)
    ref_frame, ref_vals = res[a.ref]
    dump: dict[str, Any] = {}
    for n in a.names:
        fr, vals = res[n]
        d = diff(ref_frame, fr)
        ch = {
            k.split(" ")[0]: (ref_vals.get(k, 0) & 0xFFFF, v & 0xFFFF)
            for k, v in vals.items()
            if ref_vals.get(k) != v
        }
        print(f"{n}: params {ch}")
        for o, x, y in d:
            print(f"   +{o:#05x}: {x:#06x} -> {y:#06x}")
        dump[n] = {"params": ch, "frame": [[o, x, y] for o, x, y in d]}
    if a.json:
        a.json.write_text(json.dumps(dump, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
