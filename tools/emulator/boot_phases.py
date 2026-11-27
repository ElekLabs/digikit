#!/usr/bin/env python3
"""Where a cold boot of the workbench's patched image first runs our runtime, and how that compares to the whole boot.

    uv run python -m tools.emulator.boot_phases --device dt2|dn2 [--cold] [--step-ms 50]

Two measurements on the image the workbench boots (the `emulator` patch set of devices/<device>.toml):

1. The coupled boot (ColdFire + DSP, as the harness and the app boot it): `firmware.load` on an empty boot cache (`--cold`) or on
   the shared one, its wall time and instruction count, then `state.save`, `state.load` and a warm `firmware.load` of the same image.
   The cold boot runs on a worker thread of the daemon and cannot be polled.
2. A stepped power-on boot of the same image (a daemon started on the patched .syx, paused, with the card of 1): `run_until` in
   STEP_MS slices of emulated time and a peek of the marker word after each, until the runtime wrote it (its first code ran), then on
   to the ready state. The marker is `DKRT_BOOT.magic`, which `dkrt_boot` (runtime/core/src/rs/src/boot.rs) sets right after
   `init_once`, before anything else it does; `dkrt_boot` is called by `dkrt_boot_hook` at the entry of the firmware's main-OS task.
   This boot has no DSP attached, so its wall time is the ColdFire share only; the instruction counts are comparable.
"""

from __future__ import annotations

import argparse
import json
import shutil
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Any

from tools.control.digi_client import Emu
from tools.patchkit import pk_harness as H

ROOT = H.ROOT
FIRMWARE = {"dt2": "1.17", "dn2": "1.12"}
DEVICE_FILE = {"dt2": "digitakt-ii.toml", "dn2": "digitone-ii.toml"}
MARKER = (
    "DKRT_BOOT"  # the runtime's boot-info static (boot.rs); `magic` is its first word
)
MARKER_VALUE = 0x444B4254  # INFO_MAGIC, "DKBT"
MS = H.MS


def emulator_set(device: str) -> list[str]:
    """The module roots of the `emulator` patch set: what the workbench patches in (runtime_patch.rs resolves the requires)."""
    data = tomllib.loads((ROOT / "devices" / DEVICE_FILE[device]).read_text())
    return list(data["patch_sets"]["emulator"])


def is_ready(cf: dict[str, Any]) -> bool:
    """The harness readiness contract (Session.reset)."""
    return (
        bool(cf["ready"])
        and cf["phase"] == "ready"
        and bool(cf["main_ui_reached"])
        and cf.get("filesystem_verified") is not False
    )


def step_until(
    emu: Emu, marker: int, step_ms: float, wall_limit: float
) -> dict[str, Any]:
    """Run from power-on in STEP_MS slices; note the first slice after which the marker changed and the one that reached ready."""
    out: dict[str, Any] = {"phases": []}
    t0 = time.monotonic()
    n = 0
    last_phase = None
    first = None
    while time.monotonic() - t0 < wall_limit:
        n += 1
        emu.run_until(icount=int(n * step_ms * MS), timeout_wall_s=wall_limit)
        cf = emu.status()["cf"]
        wall = time.monotonic() - t0
        if cf.get("error"):
            raise RuntimeError(f"guest fault: {cf['error']} pc={cf['pc']:#x}")
        if cf["phase"] != last_phase:
            last_phase = cf["phase"]
            out["phases"].append((cf["phase"], cf["icount"], wall))
        if first is None and emu.peek32(marker) == MARKER_VALUE:
            first = {"icount": cf["icount"], "wall": wall}
            out["first"] = first
        if is_ready(cf):
            out["ready"] = {"icount": cf["icount"], "wall": wall}
            return out
    raise TimeoutError(
        f"not ready after {wall_limit:.0f} s wall; phases {out['phases']}"
    )


def timed(emu: Emu, method: str, **params: Any) -> float:
    t = time.monotonic()
    emu.call(method, **params)
    return time.monotonic() - t


def run(device: str, cold: bool, step_ms: float, wall_limit: float) -> dict[str, Any]:
    cell = H.Cell(device, FIRMWARE[device], emulator_set(device))
    work = Path(tempfile.mkdtemp(prefix=f"bootphases-{device}-"))
    result: dict[str, Any] = {"device": device, "cold": cold}
    cache = work / "cache" if cold else H.SHARED_CACHE
    try:
        # 1. the coupled boot
        with H.Session(cell, H.Goldens(), cache_dir=cache, run_dir=work / "run-a") as s:
            s.open()
            emu = s.emu
            assert emu is not None and s.image is not None
            report = s.load_report
            st = emu.status()
            cf = st["cf"]
            result["coupled"] = {
                "start_used": report.get("start_used"),
                "load_s": s.timings["load_s"],
                "icount": cf["icount"],
            }
            snap = work / "ready.state"
            result["state_save_s"] = timed(emu, "state.save", path=str(snap))
            result["state_save_mb"] = snap.stat().st_size / 1e6
            t = time.monotonic()
            r2 = s.load(s.image)
            result["warm_load_s"] = time.monotonic() - t
            result["warm_start_used"] = r2.get("start_used")
            plan = json.loads(s.image.with_suffix(".plan.json").read_text())
            marker = H.plan_symbol(plan, MARKER)
            result["marker"] = marker
            image, card = s.image, s.card
            # 2. the stepped power-on boot of the same image
            spawn_args = ["--paused"]
            if card:
                spawn_args += ["--card-image", str(Path(card).resolve())]
            exe = H.daemon_binary(device, s.syx)
            with Emu.spawn(
                str(image), *spawn_args, binary=exe, run_dir=work / "run-b", timeout=120
            ) as e2:
                e2.call("run.mode", mode="paused")
                if e2.status()["cf"]["icount"] != 0:
                    raise RuntimeError("the stepped daemon did not start at power-on")
                result["stepped"] = step_until(e2, marker, step_ms, wall_limit)
                snap2 = work / "stepped-ready.state"
                result["state_save2_s"] = timed(e2, "state.save", path=str(snap2))
            # state.load restores into a runtime that has not run (power-on): a fresh daemon, as after a firmware.load of a new image
            with Emu.spawn(
                str(image), *spawn_args, binary=exe, run_dir=work / "run-c", timeout=120
            ) as e3:
                result["state_load_s"] = timed(e3, "state.load", path=str(snap2))
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return result


def table(r: dict[str, Any]) -> str:
    c, s = r["coupled"], r["stepped"]
    first, ready = s["first"], s["ready"]
    rows = [
        f"{'device':6} {'mark':14} {'emu s':>8} {'wall s':>8} {'icount':>14}",
        f"{r['device']:6} {'first runtime':14} {first['icount'] / MS / 1000:8.2f} {first['wall']:8.2f} {first['icount']:14,}",
        f"{r['device']:6} {'ready':14} {ready['icount'] / MS / 1000:8.2f} {ready['wall']:8.2f} {ready['icount']:14,}",
        f"before runtime: {100 * first['icount'] / ready['icount']:.1f}% of emulated boot, "
        f"{100 * first['wall'] / ready['wall']:.1f}% of stepped wall (no DSP in this boot)",
        f"stepped boot phases (phase, icount, wall s): {[(p, i, round(w, 2)) for p, i, w in s['phases']]}",
        f"coupled boot ({c['start_used']}): load {c['load_s']:.2f} s wall, {c['icount']:,} instr at ready "
        f"({c['icount'] / MS / 1000:.2f} emu s); first runtime at {100 * first['icount'] / c['icount']:.1f}% of its instructions, "
        f"about {c['load_s'] * first['icount'] / c['icount']:.1f} s wall if the wall time is linear in instructions",
        f"marker {MARKER} at {r['marker']:#x}; step {r.get('step_ms')} ms",
        f"coupled ready: state.save {r['state_save_s']:.3f} s ({r['state_save_mb']:.1f} MB), "
        f"warm firmware.load {r['warm_load_s']:.3f} s ({r['warm_start_used']})",
        f"ColdFire-only ready: state.save {r['state_save2_s']:.3f} s, state.load into a power-on daemon {r['state_load_s']:.3f} s",
    ]
    return "\n".join(rows)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--device", choices=sorted(FIRMWARE), required=True)
    ap.add_argument(
        "--cold", action="store_true", help="empty boot cache: a real cold boot"
    )
    ap.add_argument("--step-ms", type=float, default=50.0)
    ap.add_argument("--wall-limit", type=float, default=300.0)
    a = ap.parse_args(argv)
    r = run(a.device, a.cold, a.step_ms, a.wall_limit)
    r["step_ms"] = a.step_ms
    print(table(r))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
