#!/usr/bin/env python3
"""Build and pack every run-time module for every digikit device into one folder per device: `out/modules/<device>/<id>.dkm`.

    uv run python -m tools.device.dkpackall            # mise run module:pack-all (`mise run workbench` runs it first)
    uv run python -m tools.device.dkpackall --force    # rebuild the objects whose source hash is unchanged

The desktop workbench's Transfer page does not build: it reads this folder (apps/desktop/src-tauri/src/library.rs). Only what changed
costs time: `patchkit build` skips an object whose stored `source_sha256` equals the hash of its sources (`patchkit.module_status`; the objects are
cached in out/cache/dko), and
`dktransfer.pack_for_transfer` reuses a packed file newer than everything a pack reads. A file is rewritten only when its bytes change, and
files of modules that no longer pack for a device are removed. A module that does not pack for a device (no DSP object for it, no
register records for it) is left out of that device's folder, and so is a module whose `ports` name no firmware of the device (the packer
accepts it, but the folder is what the workbench ships).
"""

from __future__ import annotations

import argparse
import shutil
import sys
import time
import tomllib
from pathlib import Path

from tools.device import dkrun as R
from tools.device import dktransfer as T
from tools.patchkit import modroots, patchkit, pk_device

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "out" / "modules"


def supports(mid: str, dev: str) -> bool:
    """Does the module's `ports` (when it has any) name a firmware of the device?"""
    ports = tomllib.loads((modroots.module_dir(mid) / "module.toml").read_text())[
        "module"
    ].get("ports")
    return ports is None or bool(set(ports) & set(pk_device.port_images(dev)))


def pack_all(
    force: bool = False, devices: list[str] | None = None
) -> dict[str, dict[str, str]]:
    """Returns, per device, the module ids written and whether each is "new", "changed" or "same"."""
    ids = T.candidates()
    for mid in ids:
        if force or patchkit.module_status(modroots.module_dir(mid)) != "unchanged":
            R.build_objects(mid)
    result: dict[str, dict[str, str]] = {}
    for dev in devices or pk_device.store_devices():
        folder = OUT / dev
        written: dict[str, str] = {}
        for mid in ids:
            if not supports(mid, dev):
                continue
            try:
                packed = T.pack_for_transfer(mid, dev)
            except R.RunError:
                continue  # does not pack for this device
            dest = folder / f"{mid}.dkm"
            data = packed.read_bytes()
            if dest.is_file() and dest.read_bytes() == data:
                written[mid] = "same"
                continue
            written[mid] = "changed" if dest.is_file() else "new"
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        keep = {folder / f"{mid}.dkm" for mid in written}
        for f in sorted(folder.rglob("*.dkm")) if folder.is_dir() else []:
            if f not in keep:
                f.unlink()
        for d in sorted((p for p in folder.rglob("*") if p.is_dir()), reverse=True):
            if not any(d.iterdir()):
                shutil.rmtree(d)
        result[dev] = written
    return result


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--force",
        action="store_true",
        help="rebuild objects whose source hash is unchanged",
    )
    args = ap.parse_args(argv)
    t0 = time.monotonic()
    try:
        res = pack_all(args.force)
    except R.RunError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    for dev, mods in res.items():
        changed = [m for m, s in mods.items() if s != "same"]
        print(
            f"module:pack-all {dev}: {len(mods)} modules in out/modules/{dev}"
            + (f", written: {', '.join(changed)}" if changed else ", unchanged")
        )
    print(f"module:pack-all: {time.monotonic() - t0:.1f} s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
