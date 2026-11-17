#!/usr/bin/env python3
"""The DSP difference of two .syx images as a DSP patch file (packages/emulator/sharc-peer/src/dsp_patch.rs).

    uv run python -m tools.patchkit.pk_dsp_diff BASE.syx PATCHED.syx OUT.patch

Every run of bytes that the boot stream (section 7) of PATCHED loads differently from BASE becomes one `BYTES <loader address> <hex>` line (code is at its
`0x28000000 + 2 pc` alias, appended code and tables at their L2 address). `coupled-run --dsp-patch OUT.patch` applies it to the imported DSP state, so a
headless run can time a patched image without the control daemon (tools/devtools/coupled_ab.py `--patch`). Firmware-derived: write OUT outside the repository.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from tools.patchkit.dsp_patch import Stream, section_blob


def diff_runs(base: Stream, new: Stream) -> list[tuple[int, bytes]]:
    """(address, bytes) of every stretch the new stream loads that the base does not load or loads differently."""
    out: list[tuple[int, bytes]] = []
    for block in new.mem.blocks:
        if block["fill"]:
            continue
        addr, n = block["target_address"], block["byte_count"]
        data = new.read(addr, n)
        try:
            same = base.read(addr, n) == data
        except SystemExit:
            same = False
        if same:
            continue
        # byte-wise inside a changed block: the runs that differ (or are not in the base)
        run_start = None
        for i in range(n + 1):
            differs = False
            if i < n:
                try:
                    differs = base.read(addr + i, 1) != data[i : i + 1]
                except SystemExit:
                    differs = True
            if differs and run_start is None:
                run_start = i
            elif not differs and run_start is not None:
                out.append((addr + run_start, data[run_start:i]))
                run_start = None
    return out


def patch_text(base_syx: Path, new_syx: Path) -> str:
    base, new = Stream(section_blob(base_syx, 7)), Stream(section_blob(new_syx, 7))
    lines = [
        f"# {new_syx.name} minus {base_syx.name}: DSP bytes (tools/patchkit/pk_dsp_diff.py)"
    ]
    for addr, data in diff_runs(base, new):
        lines.append(f"BYTES {addr:x} {data.hex()}")
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("base", type=Path)
    ap.add_argument("patched", type=Path)
    ap.add_argument("out", type=Path)
    a = ap.parse_args(argv)
    text = patch_text(a.base, a.patched)
    a.out.write_text(text)
    print(f"{a.out}: {text.count(chr(10)) - 1} runs, {len(text)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
