#!/usr/bin/env python3
"""Check tools/firmware/aplibpack.py against the stock streams and the firmware's depacker.

    uv run python -m tools.firmware.aplib_check [IMAGE ...] [--no-native] [--sections 2,3,7,8]

For each acceptance image and each packed
section, from out/sections/IMAGE/: pack it, read it back with tools/firmware/elz.py, and
run the same bytes through the bootstrap's own depacker (section 2, found by
its prologue) on the native ColdFire core (packages/emulator/boot example depack_check,
built on first use). The bootstrap stages a packed section at 0x40400000 and
unpacks to 0x40000400, which the example reproduces. Prints stored sizes,
the stock size from the .syx, and seconds.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]

from tools import python_env  # noqa: E402
from tools.firmware import aplibpack, config, elz  # noqa: E402
from tools.firmware.container import sections  # noqa: E402

IMAGES = config.DEVICE_SYX
NAMES = {2: "DSP", 3: "MAIN_OS", 7: "BLOB", 8: "SECTION"}
# The depacker's prologue: lea -20(a7),a7; clr.l d1; movem.l d2-d5/a2,(a7);
# move.l 28(a7),d4; moveq #1,d3; movea.l 24(a7),a0; movea.l d4,a1; addq.l #8,a0.
DEPACK_PROLOGUE = bytes.fromhex("4fefffec428148d7043c282f001c7601206f001822445088")


def depacker_entry(bootstrap: bytes) -> int:
    at = bootstrap.find(DEPACK_PROLOGUE)
    if at < 0 or bootstrap.find(DEPACK_PROLOGUE, at + 1) >= 0:
        raise ValueError("bootstrap depacker prologue not unique")
    return 0x80000400 + at


def native_binary() -> pathlib.Path:
    from tools.emulator import native_target

    target = pathlib.Path(native_target.target_dir(None))
    subprocess.run(
        [
            sys.executable,
            "-m",
            "tools.devtools.benchlock",
            "run",
            "--shared",
            "--",
            "cargo",
            "build",
            "--release",
            "--locked",
            "--target-dir",
            str(target),
            "-p",
            "elektron-native-boot",
            "--example",
            "depack_check",
        ],
        cwd=ROOT,
        env=python_env(),
        check=True,
        capture_output=True,
    )
    return target / "release" / "examples" / "depack_check"


def firmware_depack(binary, bootstrap_path, stream: bytes) -> dict:
    boot = bootstrap_path.read_bytes()
    entry = depacker_entry(boot)
    with tempfile.TemporaryDirectory() as tmp:
        f = pathlib.Path(tmp) / "stream.bin"
        f.write_bytes(stream)
        out = subprocess.run(
            [str(binary), str(bootstrap_path), str(f), hex(entry)],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    return json.loads(out)


def check_image(image: str, ids: list[int], binary) -> int:
    sdir = ROOT / "out" / "sections" / image
    _, table = sections(str(config.stock(IMAGES[image])))
    stock = {sid: clen for sid, _, clen, _ in table}
    bootstrap = sdir / "section_2_DSP.bin"
    failures = 0
    for sid in ids:
        path = sdir / f"section_{sid}_{NAMES[sid]}.bin"
        if not path.exists():
            continue
        data = path.read_bytes()
        t0 = time.time()
        stream = aplibpack.pack_section(data)
        dt = time.time() - t0
        ok_py = elz.depack_section(stream) == data
        ok_fw = None
        if binary is not None:
            fw = firmware_depack(binary, bootstrap, stream)
            ok_fw = (
                fw["len"] == len(data)
                and fw["sha256"] == hashlib.sha256(data).hexdigest()
            )
        declared = struct.unpack_from(">I", stream, 0)[0]
        failures += not ok_py or ok_fw is False
        print(
            "%-9s section %d %-8s %9d B -> %9d B (%.3f)  stock %9d B  store-only %9d B"
            "  %5.1fs  elz %s  firmware %s"
            % (
                image,
                sid,
                NAMES[sid],
                len(data),
                len(stream),
                len(stream) / len(data),
                stock.get(sid, 0),
                len(data) + len(data) // 8 + 16 + 8,
                dt,
                "ok" if ok_py else "FAIL",
                {None: "-", True: "ok", False: "FAIL"}[ok_fw],
            )
        )
        assert declared == len(stream) - 8
    return failures


def main(argv=None) -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("images", nargs="*", default=list(IMAGES))
    p.add_argument("--no-native", action="store_true")
    p.add_argument("--sections", default="2,3,7,8")
    args = p.parse_args(argv)
    binary = None if args.no_native else native_binary()
    ids = [int(x) for x in args.sections.split(",")]
    failures = sum(check_image(i, ids, binary) for i in args.images)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
