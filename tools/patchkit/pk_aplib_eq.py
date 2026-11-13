#!/usr/bin/env python3
"""patchkit's aPLib packer must emit the same bytes as tools/firmware/aplibpack.py on every packed section.

    uv run python -m tools.patchkit.pk_aplib_eq [FIRMWARE.syx ...] [--only 2,3,7,8] [--rust BIN]

For each firmware, depacks sections 2, 3, 7 and 8, packs them with the Python packer (slow: about a minute
for MAIN OS; results are cached by input sha256 in out/cache/pk-aplib/) and with `patchkit pack-section`,
and compares the stored bytes. Exit status 1 on any difference.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

from tools.firmware import aplibpack, config  # noqa: E402
from tools.firmware.container import sections  # noqa: E402
from tools.firmware.elz import depack_section  # noqa: E402
from tools.firmware.extract import classify  # noqa: E402

FIRMWARE = list(config.DEVICE_SYX.values())
CACHE = ROOT / "out" / "cache" / "pk-aplib"


def rust_bin() -> Path:
    import os

    t = os.environ.get("PATCHKIT_BIN")
    if t:
        return Path(t)
    from tools.patchkit import patchkit

    return patchkit.binary(build=False)


def python_pack(raw: bytes) -> bytes:
    CACHE.mkdir(parents=True, exist_ok=True)
    key = CACHE / (hashlib.sha256(raw).hexdigest() + ".bin")
    if key.exists():
        return key.read_bytes()
    out = aplibpack.pack_section(raw)
    key.write_bytes(out)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("syx", nargs="*")
    ap.add_argument("--only", default="2,3,7,8")
    ap.add_argument("--rust", type=Path)
    a = ap.parse_args()
    want = {int(x) for x in a.only.split(",")}
    rust = a.rust or rust_bin()
    bad = 0
    total = 0
    for name in a.syx or FIRMWARE:
        path = Path(name) if Path(name).exists() else config.stock(name)
        if not path.exists():
            print(f"skip {name} (not present)")
            continue
        c, table = sections(str(path))
        for sid, off, clen, _dest in table:
            if sid not in want:
                continue
            kind, payload = classify(bytes(c[off : off + clen]))
            if kind != "packed":
                continue
            raw = depack_section(payload)
            t0 = time.time()
            py = python_pack(raw)
            t1 = time.time()
            with tempfile.TemporaryDirectory() as d:
                i, o = Path(d) / "in", Path(d) / "out"
                i.write_bytes(raw)
                subprocess.run([str(rust), "pack-section", str(i), str(o)], check=True)
                rs = o.read_bytes()
            t2 = time.time()
            total += 1
            same = rs == py
            bad += not same
            print(
                f"{path.name} section {sid}: {len(raw)} -> {len(py)} bytes; python {t1 - t0:.1f}s, rust {t2 - t1:.2f}s: "
                + ("EQUAL" if same else "DIFFERENT"),
                flush=True,
            )
    print(f"{total - bad}/{total} equal")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
