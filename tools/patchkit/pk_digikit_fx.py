#!/usr/bin/env python3
"""Offline BASIC FX range, fit and peak oracles.

Device acceptance lives in tests/regression/runtime/fx-chain.test.ts.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import numpy as np

from tools.device import dkneeds


def ranges_ok(mods: list[dict[str, Any]]) -> bool:
    """The DSP ranges of the listed modules are pairwise disjoint."""
    rs = sorted(
        (m["dsp_off"], m["dsp_off"] + m["dsp_len"]) for m in mods if m["dsp_len"]
    )
    return all(rs[i][1] <= rs[i + 1][0] for i in range(len(rs) - 1))


def chain_report(snapshot: dict[str, Any], effects: list[bytes]) -> dict[str, Any]:
    """Expose the retained scenario's range and native fit oracles to host callers."""
    needs = [dkneeds.needs_of(data) for data in effects]
    return {
        "ranges_ok": ranges_ok(snapshot["modules"]),
        "live": dkneeds.fit(snapshot["capacity"], [needs[-1]]),
        "empty": dkneeds.fit(snapshot["capacity"], needs, empty=True),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Existing BASIC FX acceptance oracles")
    sub = parser.add_subparsers(dest="command", required=True)
    chain = sub.add_parser("chain-check")
    chain.add_argument("snapshot", type=Path)
    chain.add_argument("effects", nargs="+", type=Path)
    peak = sub.add_parser("pcm-peak")
    peak.add_argument("wav", type=Path)
    args = parser.parse_args(argv)
    if args.command == "chain-check":
        result = chain_report(
            json.loads(args.snapshot.read_text()),
            [path.read_bytes() for path in args.effects],
        )
    else:
        from tools.patchkit.hook_ab import read_wav

        result = {"peak": float(np.abs(read_wav(args.wav)).max())}
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
