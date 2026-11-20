#!/usr/bin/env python3
"""pcdump_names: function names for a `pcdump` CSV (SHARC_PCDUMP, docs/INSTRUMENTATION.md).

    uv run python -m tools.devtools.pcdump_names out/runs/.../pcdump.csv --image dn2-1.12 [-o named.csv]

Adds a last column `func` (the function of the row's pc, from the address map of IMAGE) and writes the function after every `at 0x..` of the
`#stack-max` line. The columns of the dump stay where they are. Stock firmware names only: module DSP code has none yet.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from tools.devtools.addrnames import AddrNames, AddrNamesError

AT = re.compile(r"at (0x[0-9a-fA-F]+)")


def annotate(lines: list[str], names: AddrNames) -> list[str]:
    """The dump lines with names (the header line, data rows and `#stack-max`)."""
    pcs: list[int] = []
    for line in lines[1:]:
        if line.startswith("#"):
            pcs += [int(m, 16) for m in AT.findall(line)]
        elif line:
            pcs.append(int(line.split(",", 2)[1], 16))
    names.resolve(pcs, "sharc")  # one lookup process for the whole file

    def text(pc: int) -> str:
        return names.text(pc, "sharc")

    out = [lines[0] + ",func"] if lines else []
    for line in lines[1:]:
        if line.startswith("#"):
            out.append(
                AT.sub(lambda m: f"{m.group(0)} ({text(int(m.group(1), 16))})", line)
            )
        elif line:
            out.append(f"{line},{text(int(line.split(',', 2)[1], 16))}")
        else:
            out.append(line)
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("csv", type=Path)
    ap.add_argument("--image", required=True, help="dt2-1.17, dn2-1.12")
    ap.add_argument("-o", type=Path, help="the file (default CSV with .named.csv)")
    args = ap.parse_args(argv)
    try:
        names = AddrNames.for_stock(args.image)
    except AddrNamesError as e:
        print(f"pcdump_names: {e}", file=sys.stderr)
        return 1
    out = args.o or args.csv.with_suffix(".named.csv")
    out.write_text("\n".join(annotate(args.csv.read_text().splitlines(), names)) + "\n")
    print(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
