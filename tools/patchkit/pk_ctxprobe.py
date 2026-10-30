#!/usr/bin/env python3
"""Read the SHARC context registers at the voice hook in the coupled emulator (docs/findings/45-dsp-voice-context.md).

    uv run python -m tools.patchkit.pk_ctxprobe [--device dt2|dn2] [--fw 1.17] [--blocks 600] [--note]
    uv run python -m tools.patchkit.pk_ctxprobe --cost --modules core/dsp-voice,core/dsp-link --blocks 200   # DSP cycles per block

core/dsp-voice stores, once per block at the first voice's entry, a heartbeat and M5, M6, M7, M13, M14, M15, I6, I7, B7, L7 and
MODE1 into `_dv_diag` (the words dl_diag reports to the ColdFire). This tool patches the image, boots it coupled, runs BLOCKS DSP
blocks (with TRIG 1 held when --note is given) and prints those words as the emulator's DSP holds them, with the
numbers the old rig assumed beside them. With core/dsp-link in --modules it also reads the words from the ColdFire's copy of the reply record (what dl_diag returns).
Prints one JSON object; exit 0 when the words are plausible (the heartbeat moved, the record carries the block).
"""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]

from tools import python_env  # noqa: E402
from tools.patchkit import pk_dsplink as D  # noqa: E402

NAMES = (
    "beat",
    "M5",
    "M6",
    "M7",
    "M13",
    "M14",
    "M15",
    "I6",
    "I7",
    "B7",
    "L7",
    "MODE1",
    "crumb",
    "ndisp",
    "nret",
)
REGS = NAMES[1:12]  # the context registers (record words 3 to 13)
FRIENDLY = {"M5": 0, "M6": 1, "M7": -1, "M13": 0, "M14": 1, "M15": -1}


def patch(device: str, fw: str, modules: list[str]) -> tuple[Path, dict[str, Any]]:
    syx = {"dt2": "Digitakt_II_OS", "dn2": "Digitone_II_OS"}[device] + fw + ".syx"
    out = ROOT / "out" / "patch" / f"ctxprobe-{device}-{fw}.syx"
    r = subprocess.run(
        [sys.executable, "-m", "tools.patchkit.patchkit", "patch", "--firmware", str(ROOT / "firmware" / syx),
         "--modules", ",".join(modules), "-o", str(out)],
        cwd=ROOT, env=python_env(), capture_output=True, text=True,
    )  # fmt: skip
    if r.returncode:
        raise D.ProofError(f"patch failed:\n{r.stdout}{r.stderr}")
    return out, json.loads(out.with_suffix(".plan.json").read_text())


def read_words(emu: Any, addr: int, n: int) -> list[int]:
    raw = bytes.fromhex(emu.dsp_peek(addr, 4 * n)["hex"].replace("??", "00"))
    return list(struct.unpack(f"<{n}I", raw))


def diag_off(j: int) -> int:
    return 0x1C + 0x54 * (1 + j // 2) + (0x48 if j & 1 else 0x44)


def record_words(emu: Any, rec: int) -> list[int]:
    """The 32 diagnostic words as dl_diag reads them from the ColdFire's copy of the DSP's reply record."""
    out = []
    for j in range(32):
        v = int(
            emu.peek32(rec + diag_off(j)) or 0
        )  # big-endian: the low half of the DSP word comes first
        out.append(((v >> 16) & 0xFFFF) | ((v & 0xFFFF) << 16))
    return out


def block_cycles(emu: Any, rec: int) -> int:
    """The DSP's own cycle count of the last block: word 0 of the reply record, rotated by 16 bits (finding 41)."""
    v = int(emu.peek32(rec) or 0)  # big-endian halves, the DSP word's low half first
    w = ((v >> 16) & 0xFFFF) | ((v & 0xFFFF) << 16)
    return ((w >> 16) | (w << 16)) & 0xFFFFFFFF


def cost(a: argparse.Namespace) -> dict[str, Any]:
    """DSP cycles per block over a window of blocks (the emulator counts one per instruction, so the figure is exact and a
    paired comparison of two module versions needs no repeats): the mean and the extremes."""
    fw = a.fw or {"dt2": "1.17", "dn2": "1.12"}[a.device]
    modules = a.modules.split(",")
    image, plan = patch(a.device, fw, modules)
    lit = int(
        next(
            x["addr"]
            for x in D.plan_anchors(a.device, fw)
            if x["name"] == "cf.link.site.reply_copy_lit"
        )
    )
    xs = []
    with D.Rig(a.device, fw, modules, f"cost-{a.device}-{fw}") as rig:
        emu = rig.emu
        emu.restart(auto_no=True)
        emu.drain_events()
        base = rig.frames()
        emu.call("run_until", frames=base + a.warmup)
        rec = int(emu.peek32(lit) or 0)
        n0 = int(emu.call("audio.report")["audio"]["dsp_instructions_since_restore"])
        for k in range(a.blocks):
            emu.call("run_until", frames=base + a.warmup + 1 + k)
            xs.append(block_cycles(emu, rec))
        n1 = int(emu.call("audio.report")["audio"]["dsp_instructions_since_restore"])
    return {
        "per_block_total": (n1 - n0) / a.blocks,
        "device": a.device,
        "fw": fw,
        "modules": modules,
        "blocks": len(xs),
        "mean": sum(xs) / len(xs),
        "min": min(xs),
        "max": max(xs),
        "first": xs[:8],
    }


def probe(a: argparse.Namespace) -> dict[str, Any]:
    fw = a.fw or {"dt2": "1.17", "dn2": "1.12"}[a.device]
    modules = a.modules.split(",")
    image, plan = patch(a.device, fw, modules)
    diag = D.sym(plan, "_dv_diag")
    rep: dict[str, Any] = {
        "device": a.device,
        "fw": fw,
        "modules": modules,
        "diag": hex(diag),
        "samples": [],
    }
    lit = (
        int(
            next(
                x["addr"]
                for x in D.plan_anchors(a.device, fw)
                if x["name"] == "cf.link.site.reply_copy_lit"
            )
        )
        if "core/dsp-link" in modules
        else 0
    )
    with D.Rig(a.device, fw, modules, f"ctx-{a.device}-{fw}") as rig:
        emu = rig.emu
        emu.restart(auto_no=True)
        emu.drain_events()
        base = rig.frames()
        if a.note:
            if a.device == "dn2":
                emu.schedule(D.DN2_NOTE)
            else:
                emu.call("input.schedule", events=[
                    {"at_instr": 20_000_000, "button": D.TRIG1, "down": True},
                    {"at_instr": 600_000_000, "button": D.TRIG1, "down": False},
                ])  # fmt: skip
        for k in range(a.samples):
            emu.call("run_until", frames=base + (k + 1) * a.blocks // a.samples)
            w = read_words(emu, diag, len(NAMES))
            rep["samples"].append(dict(zip(NAMES, w, strict=True)))
            if "core/dsp-link" in modules:
                rec = int(
                    emu.peek32(lit) or 0
                )  # the handler's literal: the record copy dl_diag reads
                rep.setdefault("record", []).append(
                    {
                        "rec": hex(rec),
                        "words": [hex(x) for x in record_words(emu, rec)]
                        if rec
                        else [],
                    }
                )
    s = rep["samples"]
    rep["heartbeat_moved"] = len(s) > 1 and s[-1]["beat"] > s[0]["beat"] > 0
    rep["distinct"] = {n: sorted({x[n] for x in s}) for n in REGS}
    if "record" in rep:
        last = rep["record"][-1]["words"]
        w = [int(x, 16) for x in last]
        rep["record_ok"] = (
            bool(w)
            and (w[0] & 0xFFFF) == 0xD1A6
            and (w[0] >> 16) == 32
            and w[1] > 0
            and w[2] > 0
        )
        # words 3 .. 13 are M5 .. MODE1 of the same block (or the one before): equal to the DSP's own copy
        rep["record_regs_equal_dsp"] = w[3:14] == [s[-1][n] for n in REGS]
    rep["friendly"] = {k: v & 0xFFFFFFFF for k, v in FRIENDLY.items()}
    rep["differs_from_friendly"] = {
        k: [hex(v) for v in rep["distinct"][k]]
        for k in FRIENDLY
        if rep["distinct"][k] != [FRIENDLY[k] & 0xFFFFFFFF]
    }
    return rep


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--device", default="dt2", choices=["dt2", "dn2"])
    ap.add_argument("--fw")
    ap.add_argument("--modules", default="core/dsp-voice")
    ap.add_argument("--blocks", type=int, default=600)
    ap.add_argument("--samples", type=int, default=6)
    ap.add_argument("--note", action="store_true")
    ap.add_argument(
        "--cost",
        action="store_true",
        help="report the DSP cycles per block instead (A/B of two module versions)",
    )
    ap.add_argument("--warmup", type=int, default=300)
    a = ap.parse_args(argv)
    if a.cost:
        print(json.dumps(cost(a)))
        return 0
    rep = probe(a)
    print(json.dumps(rep, indent=1))
    return 0 if rep["heartbeat_moved"] and rep.get("record_ok", True) else 1


if __name__ == "__main__":
    sys.exit(main())
