#!/usr/bin/env python3
"""Instruction counts of the machine adapter (core/dsp-voice) around one machine module, from the rig (tests/support/machine_rig.py).

    uv run --python pypy3.11 python -m tools.patchkit.pk_adapter_cost [MODULE] [--device dt2|dn2]

The counts are deterministic (the rig runs the concrete SHARC+ core on a friendly context): per device the block of an idle voice
(registered type, no note), the note-on block (DN2: the edge block, then the block that excites), a steady block (the median of blocks
5 to 20 of a note at the defaults) and the stock fall-through (a type the engine renders itself). docs/design/module-sdk.md 5.4 gives
the targets; docs/findings/ has the table of the migration that introduced the adapter."""

from __future__ import annotations

import argparse
import statistics
import sys

from tests.support.machine_rig import MachineRig
from tools.patchkit import pk_conform


def measure(module: str, device: str) -> dict[str, int]:
    out: dict[str, int] = {}
    rig = MachineRig(module, device)
    out["idle"] = rig.block("friendly").instructions
    rig.trigger()
    first = rig.block("friendly")
    out["note-on (first block)"] = first.instructions
    blocks = [rig.block("friendly").instructions for _ in range(24)]
    if device == "dn2":
        out["note-on (second block)"] = blocks[0]
        blocks = blocks[1:]
    out["steady"] = int(statistics.median(blocks[4:20]))
    out["steady max"] = max(blocks)
    rig = MachineRig(module, device)
    if device == "dt2":
        rig._set_type_dt2(rig.track, 3)
    else:
        rig.hw(74 + rig.voice, 3)
    out["stock fall-through"] = rig.block("friendly").instructions
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("module", nargs="?", default="test/sine")
    ap.add_argument("--device", choices=pk_conform.DEVICES, action="append")
    a = ap.parse_args()
    for dev in a.device or pk_conform.DEVICES:
        print(dev)
        for k, v in measure(a.module, dev).items():
            print(f"  {k:26s} {v:6d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
