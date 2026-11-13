#!/usr/bin/env python3
"""cfprof: the ColdFire PC profile of a running emulator, with names (docs/ADDRMAP.md).

    uv run python -m tools.devtools.cfprof --image out/patch/X.syx [--module M.dkm]... [--run-dir DIR] [--top 30]

Calls `cf.profile` of the emulator (the daemon must be built with the `cf-profile` feature: docs/INSTRUMENTATION.md, "Cargo features") and prints
the execution paths and the sampled PCs by function. The load addresses of the loaded modules are read from the runtime's module table.
`--image` is the patched .syx the emulator runs; its `.addrmap.json` and `.plan.json` (written by `patchkit patch`) lie beside it.
"""

from __future__ import annotations

import argparse
import collections
import json
import sys
from pathlib import Path
from typing import Any

from tools.control.digi_client import Emu, EmuError, discover
from tools.devtools.addrnames import (
    AddrNames,
    AddrNamesError,
    plan_symbol,
    read_load_table,
)


def by_function(samples: list[list[Any]], names: AddrNames) -> list[tuple[str, int]]:
    """The `pc_samples` rows `[hex pc, weight]` summed by function (`name`, without the offset), heaviest first."""
    pcs = [int(pc, 16) for pc, _ in samples]
    total: collections.Counter[str] = collections.Counter()
    for (_, weight), pc, hit in zip(samples, pcs, names.resolve(pcs), strict=True):
        total[hit["name"] or f"{pc:#x}"] += weight
    return total.most_common()


def format_report(report: dict[str, Any], names: AddrNames, top: int = 30) -> str:
    lines = ["paths:"]
    for p in report["paths"]:
        lines.append(
            f"  {p['path']:<10} {p['instructions']:>14} insns {p['calls']:>10} calls {p['host_ns'] / 1e6:>10.1f} ms"
        )
    for key, title in (("pc_samples", "sampled PCs"), ("idle_pc", "idle skip")):
        rows = report.get(key) or []
        total = sum(w for _, w in rows) or 1
        lines.append(
            f"{title} by function (top {top}; rows are the {len(rows)} heaviest PCs):"
        )
        for name, weight in by_function(rows, names)[:top]:
            lines.append(f"  {weight:>14} {100 * weight / total:5.1f}%  {name}")
    exits = (report.get("exit_pcs") or [])[:top]
    if exits:
        lines.append("run_fast exits by PC (observed pc range and IPL exits):")
        texts = [
            r["text"] or "" for r in names.resolve([int(pc, 16) for pc, _ in exits])
        ]
        for (pc_hex, n), text in zip(exits, texts, strict=True):
            lines.append(f"  {n:>14}  {pc_hex}  {text}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--image", type=Path, required=True)
    ap.add_argument("--module", type=Path, action="append", default=[])
    ap.add_argument(
        "--run-dir", help="the run directory of the emulator (default: discover)"
    )
    ap.add_argument("--top", type=int, default=30)
    ap.add_argument("--json", action="store_true", help="print the raw report too")
    args = ap.parse_args(argv)
    try:
        with Emu.connect(discover(args.run_dir)) as emu:
            report = emu.call("cf.profile")
            state = plan_symbol(
                json.loads(args.image.with_suffix(".plan.json").read_text()),
                "DKRT_STATE",
            )
            names = AddrNames.for_image(
                args.image, args.module, read_load_table(emu.peek32, state)
            )
            print(format_report(report, names, args.top))
    except (AddrNamesError, EmuError, OSError) as e:
        print(f"cfprof: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
