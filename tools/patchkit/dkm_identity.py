"""Pack every run-time module on both devices, and prove that a change of the `.dkm` header table moved no code.

    uv run python -m tools.patchkit.dkm_identity pack OUT_DIR [--only ID,ID] [--jobs N]
    uv run python -m tools.patchkit.dkm_identity compare BEFORE_DIR AFTER_DIR

`pack` writes `OUT_DIR/<id with / as __>-<device>.dkm` for each module of `patchkit list` that is not a `core/*` module (those are patched
into the firmware, not packed), on `dt2` and `dn2`; a module that does not build for a device is listed as `skip` and is not an error.

`compare` reads both directories and checks, for each file in both, that the parts that run are byte-identical: the image (code and data),
the relocations, the imports, the DSP section, the bss size and the entry offsets. Only the header table, the offsets that move because
the table has another size, the size, the hash and the runtime version word may differ. A file in one directory only, or a changed part,
is a failure (exit 1). The DSP section is compared whole, so the PCM and the SHARC golden hashes cannot move.
"""

from __future__ import annotations

import argparse
import struct
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import tools

DEVICES = ("dt2", "dn2")
MAGIC = 0x444B4D46


def patchkit_bin() -> str:
    out = subprocess.run(
        [sys.executable, "-m", "tools.patchkit.patchkit", "bin"],
        check=True,
        capture_output=True,
        text=True,
        env=tools.python_env(),
    )
    return out.stdout.strip().splitlines()[-1]


def module_ids(binary: str) -> list[str]:
    out = subprocess.run([binary, "list"], check=True, capture_output=True, text=True).stdout
    ids = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0] == "module" and not parts[1].startswith("core/"):
            ids.append(parts[1].split("@")[0])
    return ids


def pack_one(binary: str, mid: str, device: str, out_dir: Path) -> tuple[str, str, bool, str]:
    path = out_dir / f"{mid.replace('/', '__')}-{device}.dkm"
    r = subprocess.run(
        [binary, "pack", mid, "--device", device, "-o", str(path)],
        capture_output=True,
        text=True,
    )
    lines = (r.stderr or r.stdout).strip().splitlines()
    return mid, device, r.returncode == 0, lines[-1] if lines else ""


def cmd_pack(args: argparse.Namespace) -> int:
    binary = patchkit_bin()
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    ids = args.only.split(",") if args.only else module_ids(binary)
    jobs = [(mid, dev) for mid in ids for dev in DEVICES]
    packed = 0
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for mid, dev, ok, note in pool.map(lambda j: pack_one(binary, j[0], j[1], out_dir), jobs):
            print(f"{'ok  ' if ok else 'skip'} {mid} {dev}" + ("" if ok else f"  ({note})"))
            packed += ok
    print(f"packed {packed} of {len(jobs)} into {out_dir}")
    return 0 if packed else 1


def u32(b: bytes, at: int) -> int:
    return struct.unpack_from(">I", b, at)[0]


def parts(f: bytes) -> dict[str, bytes | int]:
    """The parts of a `.dkm` that run, by name (docs/design/digikit-dkm.md, header layout of packages/patchkit/src/dkm.rs)."""
    if len(f) < 0xA0 or u32(f, 0) != MAGIC:
        raise ValueError("not a .dkm")
    image_off, image_size = u32(f, 0x60), u32(f, 0x64)
    reloc_off, reloc_n = u32(f, 0x70), u32(f, 0x74)
    import_off, import_n = u32(f, 0x78), u32(f, 0x7C)
    dsp_off, dsp_size = u32(f, 0x98), u32(f, 0x9C)
    return {
        "image": f[image_off : image_off + image_size],
        "relocs": f[reloc_off : reloc_off + 8 * reloc_n],
        "imports": f[import_off : import_off + 32 * import_n],
        "dsp": f[dsp_off : dsp_off + dsp_size] if dsp_size else b"",
        "bss": u32(f, 0x68),
        "align": u32(f, 0x6C),
        "init": u32(f, 0x80),
        "fini": u32(f, 0x84),
        "menu": u32(f, 0x88),
        "tick": u32(f, 0x8C),
        "name": f[0x40:0x60],
        "version": u32(f, 0x34),
    }


def cmd_compare(args: argparse.Namespace) -> int:
    before, after = Path(args.before), Path(args.after)
    names_b = {p.name for p in before.glob("*.dkm")}
    names_a = {p.name for p in after.glob("*.dkm")}
    bad = 0
    for n in sorted(names_b ^ names_a):
        print(f"FAIL {n}: only in {'before' if n in names_b else 'after'}")
        bad += 1
    same = 0
    for n in sorted(names_b & names_a):
        pb, pa = parts((before / n).read_bytes()), parts((after / n).read_bytes())
        diff = [k for k in pb if pb[k] != pa[k]]
        if diff:
            print(f"FAIL {n}: differs in {', '.join(diff)}")
            bad += 1
        else:
            same += 1
    print(f"identical code and DSP: {same} files; failures: {bad}")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("pack")
    p.add_argument("out_dir")
    p.add_argument("--only", help="comma separated module ids (default: every run-time module)")
    p.add_argument("--jobs", type=int, default=2)
    p.set_defaults(fn=cmd_pack)
    c = sub.add_parser("compare")
    c.add_argument("before")
    c.add_argument("after")
    c.set_defaults(fn=cmd_compare)
    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main())
