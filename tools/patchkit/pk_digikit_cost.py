#!/usr/bin/env python3
"""What the debug log costs when nobody polls it (docs/findings/46-digikit-debug.md): one paired measurement in the emulator.

    uv run python -m tools.patchkit.pk_digikit_cost run [--base GIT_REF] [--image dt2-1.17] [--seconds 30] [--keep DIR]

A is the runtime of GIT_REF (default: the commit before the log was added, `COST_BASE`), B is the runtime in the working tree; both with
`core/health`, the same card (a selection that loads a machine) and the same script on the coupled daemon: boot, play one note on the
run-time machine and capture its PCM, then SECONDS of emulated idle time. The emulator is deterministic, so the figures are counts, not timings:

  - the PCM of the note: equal or not (`numpy.array_equal`);
  - the ColdFire instructions the interpreter executed per emulated second of idle UI loop (`interpreted_instructions`; the idle loop is
    fast-forwarded and is not in it), and the difference B - A as a share of A;
  - the HEALTH figures `dkrt_health` reports (CPU load in tenths of a percent, the idle-loop iterations of the last window) over the same time.

A's runtime is built from `git archive GIT_REF modules/core/runtime` laid over a copy of the module roots (patchkit's `--root`), so nothing in the
working tree changes.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]

import numpy as np  # noqa: E402

from tools.patchkit import modroots  # noqa: E402
from tools.patchkit import pk_dkrt_boot as B  # noqa: E402
from tools.patchkit import pk_dkrt_machine as M  # noqa: E402

# the last commit before the debug log (Phase 9 "Card safety" and "Debug")
COST_BASE = "3a0e769d"
MODS = "core/health," + B.RT
HEALTH_WORDS = {
    "flags": 2,
    "blocks": 3,
    "windows": 4,
    "cpu_x10": 5,
    "idle_win": 6,
    "idle_ref": 7,
    "dsp_avg_x10": 11,
    "dsp_max_x10": 12,
}


def old_root(ref: str, work: Path) -> Path:
    """A patchkit root whose `core/runtime` is the one of GIT_REF: a copy of the module roots, the old runtime laid over it, `devices/` linked."""
    root = work / f"root-{ref}"
    if root.exists():
        shutil.rmtree(root)
    for r in modroots.REPO_ROOTS:
        shutil.copytree(ROOT / r, root / r)
    shutil.rmtree(root / "modules/core/runtime")
    tar = subprocess.run(
        ["git", "archive", ref, "modules/core/runtime"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    ).stdout
    subprocess.run(["tar", "-x", "-C", str(root)], input=tar, check=True)
    (root / "devices").symlink_to(ROOT / "devices")
    return root


def build(env: B.Env, name: str, root: Path | None) -> B.Img:
    """The patched image with MODS; ROOT None is the working tree."""
    if root is None:
        return env.build_image(MODS, name)
    syx = env.work / f"{env.image}-{name}.syx"
    plan = syx.with_suffix(".plan.json")
    subprocess.run(
        [
            str(env.tools.patchkit),
            "--root",
            str(root),
            "apply",
            "--firmware",
            str(env.stock),
            "--modules",
            MODS,
            "-o",
            str(syx),
            "--plan",
            str(plan),
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return B.Img(syx, json.loads(plan.read_text()), MODS)


def health(rig: B.BootRig) -> dict[str, int]:
    rig.post_raw(6, b"")  # CMD_HEALTH: dkrt_health into DKRT_HEALTH
    base = M.sym(rig.plan, "core/runtime", "DKRT_HEALTH")
    return {k: rig.w(base + 4 * i) for k, i in HEALTH_WORDS.items()}


def measure(
    env: B.Env, img: B.Img, card: Path, tag: str, seconds: int
) -> dict[str, Any]:
    trig = M.TRIG_FRAME
    with env.boot(img, card, tag, watch=False) as rig:
        b = rig.boot_info()
        rig.select_type(rig.dyn_type(M.MODULE, M.MACHINE), 7)
        pcm = rig.note(env.work / f"{tag}.wav", trig)
        rig.s.run_ms(2000)  # the health windows are one second each: settle
        st0 = rig.emu.status()
        h0 = health(rig)
        rig.s.run_ms(seconds * 1000)
        st1 = rig.emu.status()
        h1 = health(rig)
        cf0, cf1 = st0["cf"], st1["cf"]
        emu_s = (
            (st1["elapsed"] - st0["elapsed"]) / 1e9
            if "elapsed" in st1
            else float(seconds)
        )
        return {
            "loaded": b["loaded"],
            "pcm": pcm,
            "interpreted_per_s": (
                cf1["interpreted_instructions"] - cf0["interpreted_instructions"]
            )
            / emu_s,
            "icount_per_s": (cf1["icount"] - cf0["icount"]) / emu_s,
            "emulated_s": emu_s,
            "health_before": h0,
            "health_after": h1,
        }


def run(base: str, image: str, seconds: int, keep: Path | None) -> int:
    work = keep or Path(tempfile.mkdtemp(prefix="digikit-cost-"))
    work.mkdir(parents=True, exist_ok=True)
    env = B.Env(image, work)
    env.pack(M.MODULE, "cost.dkm")
    card = env.card(["modules/cost.dkm"], tag="cost")
    a_img = build(env, "A", old_root(base, work))
    b_img = build(env, "B", None)
    a = measure(env, a_img, card, "costA", seconds)
    b = measure(env, b_img, card, "costB", seconds)
    equal = bool(np.array_equal(a["pcm"], b["pcm"])) and len(a["pcm"]) > 0
    peak = float(np.abs(a["pcm"]).max())
    d_int = (
        (b["interpreted_per_s"] - a["interpreted_per_s"]) / a["interpreted_per_s"] * 100
    )
    print(
        f"image {image}, A = runtime of {base}, B = working tree, {a['emulated_s']:.1f} emulated seconds of idle UI loop after one note"
    )
    print(f"  both loaded the module: {a['loaded']} and {b['loaded']}")
    print(
        f"  PCM of the note ({len(a['pcm'])} samples, peak {peak:.3f}): {'EQUAL' if equal else 'DIFFERENT'}"
    )
    print(
        f"  interpreted ColdFire instructions per emulated second: A {a['interpreted_per_s']:,.0f}  B {b['interpreted_per_s']:,.0f}  ({d_int:+.3f} %)"
    )
    for k in ("cpu_x10", "idle_win", "idle_ref", "dsp_avg_x10", "dsp_max_x10"):
        print(
            f"  HEALTH {k:12s} A {a['health_after'][k]:>10,}  B {b['health_after'][k]:>10,}"
        )
    out = {
        "base": base,
        "image": image,
        "pcm_equal": equal,
        "interpreted_delta_pct": d_int,
        "A": {k: v for k, v in a.items() if k != "pcm"},
        "B": {k: v for k, v in b.items() if k != "pcm"},
    }
    (work / "cost.json").write_text(json.dumps(out, indent=1))
    if keep or not equal:  # a temp work dir is removed after a clean run
        print(f"  report: {work / 'cost.json'}")
    else:
        shutil.rmtree(work, ignore_errors=True)
    return 0 if equal else 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("run")
    p.add_argument("--base", default=COST_BASE)
    p.add_argument("--image", default="dt2-1.17")
    p.add_argument("--seconds", type=int, default=30)
    p.add_argument("--keep", type=Path)
    a = ap.parse_args(argv)
    return run(a.base, a.image, a.seconds, a.keep)


if __name__ == "__main__":
    sys.exit(main())
