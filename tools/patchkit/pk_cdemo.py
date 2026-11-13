#!/usr/bin/env python3
"""Run a C module that needs libgcc helpers through the digikit runtime in the emulator (finding 35).

    uv run python -m tools.patchkit.pk_cdemo proof [--image dt2-1.17] [--keep DIR]

Patches `core/runtime` into the stock image, packs `test/c-demo` (64-bit divide, remainder, multiply and shifts: libgcc helpers
that `patchkit build` links in from the -mcpu=54418 libgcc), puts it in a +Drive card's module store with a selection record, boots (the boot hook loads it), reads the module's result block through the address its `init` logged, and compares every word with Python's
integer arithmetic. A V4e that met a 68020-only form in a helper would take an illegal-instruction exception instead, so a
correct result is also the run-time proof that the helpers are ColdFire code. The write-watch (only the runtime and the arena
write into `cf.expansion`) and the flush check run as in tools/patchkit/pk_dkrt.py.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tempfile
from pathlib import Path

from tools.firmware import config
from tools.patchkit import pk_dkrt
from tools.patchkit.pk_dkrt import (
    ARENA_BYTES,
    DEVICE_SYX,
    OK,
    ST_LOG,
    ST_MODULES,
    ProofError,
    be32,
    sbe32,
)

M64 = (1 << 64) - 1
# c_demo_in / c_demo_n of tests/fixtures/modules/c-demo/src/c/c_demo.c
A, B = 0xFEDCBA9876543210, 0x0000000123456789
C, D = 0xFFFFFFFF89ABCDEF - (1 << 64), 12345  # as signed
N0, N1, N2 = 13, 7, 9


def trunc_div(x: int, y: int) -> int:
    """C division (toward zero) of two signed ints."""
    q = abs(x) // abs(y)
    return -q if (x < 0) != (y < 0) else q


def expected() -> list[int]:
    return [
        A // B,
        A % B,
        trunc_div(C, D) & M64,
        (C - trunc_div(C, D) * D) & M64,
        (A * B) & M64,
        (A << N0) & M64,
        A >> N1,
        (C >> N2) & M64,
        (C * D) & M64,
    ]


NAMES = ["udiv", "umod", "sdiv", "smod", "mul", "shl", "lshr", "ashr", "smul"]


def proof(image: str, keep: Path | None = None) -> dict:
    if image not in DEVICE_SYX:
        raise ProofError(f"no proof for {image}")
    device = image[:3]
    syx = config.stock(DEVICE_SYX[image])
    if not syx.exists():
        raise ProofError(f"{syx} is missing")
    tools = pk_dkrt.Tools()
    work = keep or Path(tempfile.mkdtemp(prefix="pk-cdemo-"))
    work.mkdir(parents=True, exist_ok=True)
    folder = work / "plus_drive"
    store = folder / "digikit"
    (store / "modules").mkdir(parents=True, exist_ok=True)
    patched = work / f"{image}-runtime.syx"
    plan = pk_dkrt.patch(tools, syx, patched)
    rt = pk_dkrt.runtime_ranges(plan)
    pk_dkrt.pack(tools, "test/c-demo", store / "modules" / "c-demo.dkm")
    card = work / f"{device}.img"
    pk_dkrt.build_card(tools, folder, card, syx, device)
    pk_dkrt.write_selection(
        card, ["modules/c-demo.dkm"], device
    )  # loaded by the boot hook
    st, arena = rt["state"], rt["arena"]
    region_lo = min(
        w["addr"]
        for w in plan["writes"]
        if w["kind"] == "place" and "cf.expansion" in w["what"]
    )
    watch = pk_dkrt.watch_ranges(rt, region_lo, 0x47FE0000 - 0x10000)
    allow = list(rt["code"]) + [(arena, arena + ARENA_BYTES)]
    env = {
        "DIGI_FLUSH_CHECK": "1",
        "DIGI_WRITE_WATCH": ",".join(f"{a:x}-{b:x}" for a, b in watch),
        "DIGI_WRITE_WATCH_ALLOW_PC": ",".join(f"{a:x}-{b:x}" for a, b in allow),
        "DIGI_WRITE_WATCH_AFTER": "cacr",
        "DIGI_WRITE_WATCH_OUT": str(work / "ww.jsonl"),
    }
    out_ptr = st + ST_LOG + 24  # value of log entry 0: the address of c_demo_out
    script = [
        {"op": "run", "insns": 30_000_000, "tag": "settle"},
        {
            "op": "peek",
            "addr": hex(rt["sym"]["DKRT_BOOT"] + 4 * pk_dkrt.BOOT_FIELDS),
            "len": 4,
            "tag": "load:result",
        },
        {"op": "peek", "addr": hex(st), "len": pk_dkrt.STATE_BYTES, "tag": "state"},
        {"op": "peek", "addr": hex(out_ptr), "len": 80, "tag": "out", "ptr": True},
    ]
    res = pk_dkrt.run_script(
        tools, patched, plan["base"]["syx_sha256"], card, script, env, work
    )
    by = {r["tag"]: r for r in res if r.get("tag")}
    raw = lambda tag: bytes.fromhex(by[tag]["hex"])  # noqa: E731
    checks: list[tuple[str, bool, str]] = []
    got_load = sbe32(raw("load:result"), 0)
    checks.append(("load ok", got_load == OK, f"result {got_load}"))
    state, out = raw("state"), raw("out")
    checks.append(("module record", be32(state, ST_MODULES) == 1, ""))
    checks.append(
        ("init ran (magic)", be32(out, 0) == 0x43444D4F, f"{be32(out, 0):#x}")
    )
    words = [(be32(out, 8 + 8 * i) << 32) | be32(out, 12 + 8 * i) for i in range(9)]
    for name, got, want in zip(NAMES, words, expected(), strict=True):
        checks.append((f"64-bit {name}", got == want, f"{got:#x} want {want:#x}"))
    summary = json.loads((work / "ww.summary.json").read_text())
    checks.append(
        (
            "write-watch: no write by other code",
            summary["events"] == 0,
            f"{summary['events']} events",
        )
    )
    checks.append(("flush check: 0 hits", summary["flush_hits"] == [], ""))
    return {
        "image": image,
        "work": str(work),
        "ok": all(c[1] for c in checks),
        "checks": [{"name": a, "ok": b, "detail": c} for a, b, c in checks],
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("proof")
    p.add_argument("--image", default="dt2-1.17")
    p.add_argument("--keep", type=Path)
    a = ap.parse_args(argv)
    rep = proof(a.image, a.keep)
    print(json.dumps(rep, indent=1))
    if not a.keep:  # a temp work dir: removed after a clean run, kept after a failure
        if rep["ok"]:
            shutil.rmtree(rep["work"], ignore_errors=True)
        else:
            print(f"work dir kept: {rep['work']}", file=sys.stderr)
    return 0 if rep["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
