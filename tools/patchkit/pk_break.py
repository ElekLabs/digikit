#!/usr/bin/env python3
"""Deliberately broken patched images, for the teeth test of the proofs' quick tiers (never committed: out/broken/).

    uv run python -m tools.patchkit.pk_break persist|rows --modules a,b [--device dt2] [--fw 1.17] [-o OUT.syx]
    PK_PROOF_IMAGE=OUT.syx uv run python -m tools.patchkit.pk_proof_pluck --quick

The break is made in a copy of the module sources (out/broken/KIND/modules; the repository is untouched), built with
`patchkit --root`, so the image is what a module author's mistake would produce:

  persist  core/machines leaves the persistence immediate at the stock value: a saved project does not keep the
           machine type (the type falls back to SAMPLE or FM TONE when the project is loaded).
  rows     test/patched-pluck exchanges the words of DECAY and PICK in its DK_PARAM declarations (a wrong parameter
           mapping; the module's objects are rebuilt in the copy): the knobs, the names and the sound of those two
           parameters are exchanged.

`PK_PROOF_IMAGE` makes every proof session that has modules load that image instead of the patch of its modules
(tools/patchkit/pk_proofkit.py); the plan (symbols) is read from OUT.plan.json next to it.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.firmware import config  # noqa: E402
from tools.patchkit import modroots, patchkit, pk_device  # noqa: E402

# kind -> (module manifest relative to the copied root, old text, new text); the old text must occur exactly once
BREAKS = {
    "persist": (
        "modules/core/adapters/machines/module.toml",
        'value = "image.u8(cf.machine.site.sound_load_type_bound + 1) + machines.count"',
        'value = "image.u8(cf.machine.site.sound_load_type_bound + 1)"',
    ),
    "rows": (
        "tests/fixtures/modules/patched-pluck/src/module.c",
        'DK_PARAM(pluck, 0, pluck_decay, "Decay", "DECAY", 0, 127, 64);\n'
        'DK_PARAM(pluck, 1, pluck_pick, "Pick position", "PICK", 0, 127, 32);',
        'DK_PARAM(pluck, 1, pluck_decay, "Decay", "DECAY", 0, 127, 64);\n'
        'DK_PARAM(pluck, 0, pluck_pick, "Pick position", "PICK", 0, 127, 32);',
    ),
}
# kinds whose edit is in a C source: the module's objects are rebuilt for the copied root (the cache is keyed by the source hash)
REBUILD = {"rows": "tests/fixtures/modules/patched-pluck"}
FIRMWARE = pk_device.port_firmware()


def build(kind: str, mods: str, device: str, fw: str, out: Path) -> Path:
    manifest, old, new = BREAKS[kind]
    root = ROOT / "out" / "broken" / kind
    if root.exists():
        shutil.rmtree(root)
    for r in modroots.REPO_ROOTS:
        shutil.copytree(ROOT / r, root / r)
    # the toolchain, the object cache and the SDK headers are the repository's: patchkit builds the stale objects of the copied modules
    for top in ("devices", "out", "sdk"):
        (root / top).symlink_to(ROOT / top)
    path = root / manifest
    text = path.read_text()
    if text.count(old) != 1:
        raise SystemExit(
            f"{manifest}: the text to break occurs {text.count(old)} times"
        )
    path.write_text(text.replace(old, new))
    if kind in REBUILD:
        try:
            patchkit.build_module(root / REBUILD[kind])
        except patchkit.BuildError as e:
            raise SystemExit(f"patchkit build failed:\n{str(e)[-1500:]}") from e
    out.parent.mkdir(parents=True, exist_ok=True)
    plan = out.with_suffix(".plan.json")
    r = subprocess.run(
        [str(patchkit.binary()), "--root", str(root), "apply", "--firmware", str(config.stock(FIRMWARE[(device, fw)])),
         "--modules", mods, "-o", str(out), "--plan", str(plan)],
        cwd=ROOT, capture_output=True, text=True,
    )  # fmt: skip
    if r.returncode:
        raise SystemExit(f"patchkit apply failed:\n{(r.stderr or r.stdout)[-1500:]}")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("kind", choices=sorted(BREAKS))
    ap.add_argument("--modules", required=True)
    ap.add_argument("--device", default="dt2")
    ap.add_argument("--fw", default="1.17")
    ap.add_argument("-o", type=Path)
    a = ap.parse_args()
    out = a.o or ROOT / "out" / "broken" / f"{a.kind}-{a.device}-{a.fw}.syx"
    print(build(a.kind, a.modules, a.device, a.fw, out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
