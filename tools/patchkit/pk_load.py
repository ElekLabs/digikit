"""CPU meters of a firmware in the daemon: `status.load` after a held note.

    uv run python -m tools.patchkit.pk_load dt2|dn2 [--modules a,b] [--firmware VER] [--seconds 4]

Spawns the coupled daemon like the harness (stock firmware, or the patch of the modules), holds TRIG 1 and prints the
last closed window of `status.load` (docs/CONTROL.md) as JSON and as one line. The real-chip values are estimates from
the manuals' cycle tables (devices/cycle-factors.json).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from tools.firmware import device as device_files
from tools.patchkit import pk_harness as h


def line(load: dict[str, Any]) -> str:
    cf, dsp = load["cf"], load["dsp"]
    text = f"CF {cf['real_pct']:.0f} % (high {cf['real_high_pct']:.0f} %, model {cf['model_pct']:.0f} %)"
    if dsp:
        text += (
            f" · DSP {dsp['real_pct']:.0f} % (worst {dsp['real_high_pct']:.0f} %, model {dsp['model_pct']:.0f} %,"
            f" {dsp['busy_per_block']:.0f} busy/block)"
        )
    return text


def measure(cell: h.Cell, seconds: float) -> dict[str, Any]:
    with h.Session(cell, h.Goldens(), run_name=f"load-{cell.slug}") as s:
        s.open()
        s.reset()
        s.run_ms(1500)
        assert s.emu is not None
        s.emu.status()  # opens the first window
        s.do_hold("TRIG 1", 500)
        s.run_ms(seconds * 1000)
        s.health("load")
        load = s.emu.status().get("load")
        if load is None:
            raise SystemExit("status.load is null: run longer (--seconds)")
        return load


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("device", choices=device_files.shorts())
    ap.add_argument("--modules", default="")
    ap.add_argument("--firmware", help="default: 1.17 (dt2), 1.12 (dn2)")
    ap.add_argument("--seconds", type=float, default=4.0)
    a = ap.parse_args(argv)
    fw = a.firmware or {"dt2": "1.17", "dn2": "1.12"}[a.device]
    cell = h.Cell(a.device, fw, [m for m in a.modules.split(",") if m])
    load = measure(cell, a.seconds)
    print(json.dumps(load))
    print(f"{cell.name}: {line(load)}")
    return 0


if __name__ == "__main__":
    os.environ.setdefault("DIGI_PK_PGO", "0")
    raise SystemExit(main(sys.argv[1:]))
