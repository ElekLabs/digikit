#!/usr/bin/env python3
"""A module built with build options (`#ifdef NAME` in its DSP sources), as an overlay repository root.

    DIGI_PK_ROOT=$(uv run python -m tools.patchkit.pk_variant make core/dsp-voice DV_BISECT_C2 dv-c2) \
        mise run patch -- --firmware firmware/Digitakt_II_OS1.17.syx --modules ... -o out/patch/X.syx

`make MODULE NAME[,NAME] LABEL` builds the module's objects with the options (`patchkit build --define`; the committed
objects stay as they are) into out/variants/LABEL/ and prints that directory: a root whose `devices/` and `firmware/` are the
repository's and whose module roots have MODULE replaced by the variant. `patchkit`, the proof rigs
(`pk_dkrt.Tools`) and `mise run patch|preflight|pack` use it when the environment variable DIGI_PK_ROOT names it. The directory is
under out/ and never committed. The module source is one file: the option selects between alternatives in it, there is no copy.
"""

from __future__ import annotations

import argparse
import shutil
import sys
import tomllib
from pathlib import Path

from tools.patchkit import modroots, patchkit

ROOT = Path(__file__).resolve().parents[2]


def make(module: str, defines: list[str], label: str) -> Path:
    moddir = modroots.module_dir(module)
    if (
        tomllib.loads((moddir / "module.toml").read_text())["module"]["category"]
        != "core"
    ):
        raise SystemExit(
            f"{module} is a content module: its objects are cached by source hash (out/cache/dko), so a build with options cannot be "
            "an overlay; variants are for core/* modules"
        )
    base = ROOT / "out" / "variants" / label
    if base.exists():
        shutil.rmtree(base)
    (base / "modules").mkdir(parents=True)
    for top in ("devices", "firmware", "out", "ports", "sdk"):
        (base / top).symlink_to(ROOT / top)
    # module directories (same place as in the repository): every one linked, the variant's a real directory
    # (its sources linked, its objects built)
    dest_of = {}
    for d in modroots.manifests().values():
        dest = base / d.relative_to(ROOT)
        dest_of[d] = dest
        dest.parent.mkdir(parents=True, exist_ok=True)
        if d != moddir:
            dest.symlink_to(d)
            continue
        dest.mkdir()
        for child in d.iterdir():
            if child.name != "build":
                (dest / child.name).symlink_to(child)
        if (d / "build").is_dir():
            shutil.copytree(d / "build", dest / "build")
    for line in patchkit.build_module(
        moddir, False, frozenset(defines), dest_of[moddir]
    ):
        print(line, file=sys.stderr)
    return base


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("make")
    m.add_argument("module")
    m.add_argument("defines", help="comma-separated option names")
    m.add_argument("label")
    a = ap.parse_args()
    print(make(a.module, a.defines.split(","), a.label))
    return 0


if __name__ == "__main__":
    sys.exit(main())
