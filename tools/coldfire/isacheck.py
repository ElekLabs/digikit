#!/usr/bin/env python3
"""ColdFire ISA check: refuse any instruction a MCF54418 (V4e: ISA_A/B/C + EMAC, no FPU) does not have.

    uv run python -m tools.coldfire.isacheck FILE...          # .dko.json (arch cf), ELF .o, or .a archive
    uv run python -m tools.coldfire.isacheck --committed      # every committed object (under runtime/, image-modules/ and services/)
    uv run python -m tools.coldfire.isacheck --cached         # every built object in the cache out/cache/dko

This is a front end of `patchkit dko isa-check` (packages/patchkit/src/build/cf.rs `check_isa`), the one copy of the check. Every
executable section is decoded with our own decoder (the `coldfire` crate, generated from tools/coldfire/cfisa/coldfire.json). A
68020+ form (`divs.l` 64-bit, `bfextu`, `cmp2`, ...), an FPU form, a privileged form other than `cpushl` and an
illegal word are refused. A 68020-only word that the trap handler of a V4e meets is an illegal-instruction exception,
so a module with one crashes the device when the code runs, not when it loads. The same check runs inside the module
build and over the libgcc helper archive (tools/patchkit/pk_toolchains.py).
Only text sections are decoded: data in a text section (jump tables) would be decoded as code.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class IsaError(RuntimeError):
    pass


def check_file(path: Path) -> int:
    """Check one `.dko.json` (cf), ELF object or archive with `patchkit dko isa-check`; return the number of instructions (0 for an
    object that is not ColdFire). Raises IsaError listing every refused form."""
    from tools.patchkit import patchkit

    r = patchkit._build_cmd(["dko", "isa-check", str(path)])
    out = r.stdout.strip()
    if r.returncode:
        raise IsaError(
            out.split("\n", 1)[1].strip()
            if out.startswith("FAIL") and "\n" in out
            else (r.stderr or out).strip()
        )
    m = re.search(r": (\d+) instructions$", out)
    return int(m.group(1)) if m else 0


def committed_objects(root: Path = ROOT) -> list[Path]:
    """The committed objects: those of the core modules (content-module objects are build output, see `cached_objects`)."""
    from tools.patchkit.modroots import CORE_ROOTS

    return sorted(
        obj for tree in CORE_ROOTS for obj in (root / tree).glob("**/build/*.dko.json")
    )


def cached_objects(root: Path = ROOT) -> list[Path]:
    """The built objects of content modules and fixtures (`out/cache/dko`)."""
    return sorted((root / "out/cache/dko").glob("**/*.dko.json"))


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("files", nargs="*", type=Path)
    ap.add_argument("--committed", action="store_true")
    ap.add_argument("--cached", action="store_true")
    a = ap.parse_args()
    files = [
        *a.files,
        *(committed_objects() if a.committed else []),
        *(cached_objects() if a.cached else []),
    ]
    if not files:
        ap.error("no files")
    rc = 0
    for f in files:
        try:
            n = check_file(f)
        except IsaError as e:
            rc = 1
            print(f"FAIL {f}\n  {e}")
            continue
        print(f"ok   {f}: {n} instructions" if n else f"ok   {f}: no cf code")
    return rc


if __name__ == "__main__":
    sys.exit(main())
