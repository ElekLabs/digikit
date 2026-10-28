#!/usr/bin/env python3
"""Drive SETTINGS > DIGIKIT (core/digikit-menu on core/ui) in the emulator: screenshots of every screen, and a heap leak check.

    uv run python -m tools.patchkit.pk_dkmenu shots --device dt2|dn2 --out DIR
    uv run python -m tools.patchkit.pk_dkmenu leak  --device dt2|dn2 [--cycles 50]

One daemon on the device's `emulator` patch set and a card whose module store holds a tool, two effects (one with a send route), a machine and a
module that does not fit; the card's saved selection is the send effect. `shots` saves a PNG for the SETTINGS row, DIGIKIT, ABOUT, HEALTH, MEMORY,
MODULES (with the saved selection checked, then with one more row checked), the side panel of an effect, the ROUTE choice, INFO, the refusal of a module
that does not fit, the APPLY & RESTART? question, and MODULES after the restart with the new selection loaded, and one contact sheet per device. `leak`
opens and closes MODULES with its side panel, INFO and the APPLY question answered NO CYCLES times and compares the firmware heap (the buddy allocator
of finding 01) before and after a warm-up. Both use the harness `Session`; no golden is read or written.
"""

from __future__ import annotations

import argparse
import shutil
import sys
import time
from pathlib import Path

from tools.control.digi_client import EmuError, oled_png
from tools.patchkit import pk_devreload as DEV
from tools.patchkit import pk_harness as H
from tools.patchkit import pk_uidemo as UI

ROOT = H.ROOT
# the store: HUGE (does not fit), the tool, the two effects and the machine, in the list's order (file names, upper case before lower case)
STORE = [
    "synth:HUGE:8000000",
    "test/dyn-demo",
    "test/dyn-fx-gain-send*",
    "test/dyn-fx-gain-sendonly",
    "test/sine",
]
HUGE, TOOL, SEND_FX, SENDONLY_FX, MACHINE = range(5)


class Menu:
    def __init__(self, device: str, out: Path | None):
        self.device, self.out = device, out
        modules = DEV.resolve_modules("emulator", device)
        self.cell = H.Cell(device, DEV.FIRMWARE[device], modules)
        self.work = ROOT / "out" / "runs" / "dkmenu" / device
        shutil.rmtree(self.work, ignore_errors=True)
        self.work.mkdir(parents=True)
        self.s = H.Session(
            self.cell, H.Goldens(), cache_dir=H.SHARED_CACHE, run_dir=self.work / "run"
        )
        self.s.card_override = str(DEV.store_card(device, STORE, self.work))
        self.row = 0
        self.frames: list[tuple[str, bytes]] = []
        self.fail: list[str] = []

    # -- plumbing
    def tap(self, key: str, wait: int = 250) -> None:
        self.s.do_tap(key)
        self.s.run_ms(wait)

    def shot(self, name: str, wait: int = 0) -> None:
        if wait:
            self.s.run_ms(wait)
        digest, raw = self.s.frame()
        self.frames.append((name, raw))
        if self.out:
            self.out.mkdir(parents=True, exist_ok=True)
            (self.out / f"{name}.png").write_bytes(oled_png(raw))
        print(f"shot {name} {digest[:12]}", flush=True)

    def expect(self, name: str, ok: bool, detail: str = "") -> None:
        print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}", flush=True)
        if not ok:
            self.fail.append(name)

    def goto(self, row: int) -> None:
        """The cursor on MODULES row ROW (five rows; UP stops at the top)."""
        for _ in range(5):
            self.tap("UP", 120)
        for _ in range(row):
            self.tap("DOWN", 120)

    def start(self) -> None:
        self.s.open()
        self.s.reset()

    def to_digikit(self) -> None:
        self.tap("NO", 300)
        self.tap("SETTINGS", 500)
        for _ in range(8):
            self.tap("DOWN", 150)

    def open_digikit(self) -> None:
        self.tap("YES", 600)

    def to_modules(self) -> None:
        """From the SETTINGS list on DIGIKIT: the menu, the cursor on MODULES (the fourth row), the list."""
        self.tap("YES", 600)
        for _ in range(4):
            self.tap("UP", 120)
        for _ in range(3):
            self.tap("DOWN", 120)
        self.tap("YES", 1200)

    def wait_restart(self, timeout_s: float = 300.0) -> None:
        """After the menu's `dkrt_restart`: step until the emulator's soft reset has cold-booted the firmware again."""
        emu = self.s.emu
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout_s:
            try:
                emu.step(400_000)
            except (
                EmuError
            ):  # a cold boot runs on its worker: steps are refused until it is done
                time.sleep(0.5)
            for ev in emu.drain_events():
                if ev.get("kind") == "firmware.loaded":
                    if not ev.get("ok"):
                        raise RuntimeError(f"restart: the cold boot failed {ev}")
                    emu.watchdog(self.cell_stall())
                    emu.pcm_start()
                    self.s.run_ms(4000)
                    return
        raise RuntimeError("restart: no new boot")

    def cell_stall(self) -> int:
        return self.s.cell.stall_ms

    # -- screenshots
    def shots_run(self) -> None:
        self.start()
        self.to_digikit()
        self.shot("01_settings")
        self.open_digikit()
        self.shot("02_digikit")
        self.tap("YES", 700)
        self.shot("03_about")
        self.tap("NO", 400)
        self.tap("DOWN", 150)
        self.tap("YES", 1200)
        self.shot("04_health")
        self.tap("NO", 400)
        self.tap("DOWN", 150)
        self.tap("YES", 700)
        self.shot("05_memory")
        self.tap("NO", 400)
        self.tap("DOWN", 150)
        self.tap("YES", 1500)
        self.shot("06_modules")
        self.goto(HUGE)
        self.tap("YES", 600)
        self.shot("07_refused_no_fit")
        self.tap("YES", 500)  # the notice
        self.goto(TOOL)
        self.tap("YES", 300)
        self.shot("08_modules_checked")
        self.goto(SEND_FX)
        self.tap("RIGHT", 500)
        self.shot("09_panel_effect")
        self.tap("DOWN", 200)
        self.tap("YES", 600)
        self.shot("10_route_choice")
        self.tap("DOWN", 200)
        self.tap("YES", 400)  # MASTER
        self.goto(TOOL)
        self.tap("RIGHT", 400)
        self.tap("DOWN", 200)
        self.tap("YES", 900)
        self.shot("11_info")
        self.tap("NO", 400)
        self.tap("NO", 800)
        self.shot("12_apply_question")
        self.tap("YES", 300)
        self.wait_restart()
        self.to_digikit()
        self.to_modules()
        self.shot("13_modules_after_restart")
        self.s.health("end")

    # -- heap
    def heap(self) -> dict:
        return UI.heap_walk(lambda a: self.s.emu.peek32(a) or 0, self.desc())

    def desc(self) -> int:
        import tomllib

        port = tomllib.loads(
            (
                ROOT
                / "devices"
                / "ports"
                / f"{self.cell.device}-{self.cell.firmware}.toml"
            ).read_text()
        )
        for a in port["anchor"]:
            if a["name"] == "cf.mem.site.heap.desc_lit":
                return self.s.emu.peek32(a["addr"]) or 0
        raise KeyError("cf.mem.site.heap.desc_lit")

    def cycle(self) -> None:
        """MODULES opens at the DIGIKIT menu's MODULES row: the side panel, INFO, one row checked, the APPLY question answered NO."""
        self.tap("YES", 900)  # the list
        self.tap("DOWN", 100)
        self.tap("RIGHT", 150)  # the panel
        self.tap("DOWN", 100)
        self.tap("YES", 500)  # INFO
        self.tap("NO", 200)  # back to the list
        self.tap("YES", 150)  # check the row
        self.tap("NO", 400)  # changes pending: the question
        self.tap("NO", 400)  # NO: the changes are dropped and the list closes

    def leak_run(self, cycles: int) -> dict:
        self.start()
        self.to_digikit()
        self.tap("YES", 600)
        for _ in range(4):
            self.tap("UP", 120)
        for _ in range(3):
            self.tap("DOWN", 120)
        for _ in range(
            5
        ):  # warm-up: the first use builds the menus, the lists grow their row arrays
            self.cycle()
        self.s.run_ms(500)
        a = self.heap()
        for _ in range(cycles):
            self.cycle()
        self.s.run_ms(500)
        b = self.heap()
        print(
            f"leak MODULES+panel+INFO+APPLY question ({cycles} cycles): free {a['free']} -> {b['free']} ({b['free'] - a['free']:+d} bytes), "
            f"live allocations {a['allocs']} -> {b['allocs']} ({b['allocs'] - a['allocs']:+d}), free-list nodes {a['nodes']} -> {b['nodes']}, "
            f"bad {a['bad'] + b['bad']}",
            flush=True,
        )
        return {"before": a, "after": b}


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", choices=["shots", "leak"])
    ap.add_argument("--device", choices=["dt2", "dn2"], required=True)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--cycles", type=int, default=50)
    a = ap.parse_args(argv)
    m = Menu(a.device, a.out)
    try:
        if a.cmd == "shots":
            m.shots_run()
            if a.out and m.frames:
                DEV.contact_sheet(m.frames, a.out / "sheet.png")
        else:
            rep = m.leak_run(a.cycles)
            b, e = rep["before"], rep["after"]
            m.expect(
                "the heap is flat after the warm-up: no free byte lost, no live allocation gained, the free lists are sound",
                e["free"] >= b["free"] and e["allocs"] <= b["allocs"] and not e["bad"],
                f"{e['free'] - b['free']:+d} bytes, {e['allocs'] - b['allocs']:+d} allocations",
            )
    finally:
        m.s.close()
    return 1 if m.fail else 0


if __name__ == "__main__":
    import os

    os.chdir(ROOT)
    sys.exit(main(sys.argv[1:]))
