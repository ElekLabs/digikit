#!/usr/bin/env python3
"""Drive test/ui-demo (core/ui on the screen) in the emulator: screenshots and a heap leak check.

    uv run python -m tools.patchkit.pk_uidemo shots --device dt2|dn2 --out DIR
    uv run python -m tools.patchkit.pk_uidemo leak  --device dt2|dn2 [--cycles 50]

One daemon on the patched image (core/runtime, core/ui, test/ui-demo). `shots` opens SETTINGS > DIGIKIT and saves an OLED PNG of the demo
menu, the text page, the list (plain, with a check mark, with the side panel), the empty list, the confirm, the choice and the toast;
it also checks the answers the demo recorded. `leak` opens and closes the demo menu, the list with its panel and a confirm CYCLES times
each and compares the firmware heap (the buddy allocator of finding 01, walked over `peek32`, and its live-allocation count) before and
after. Both use the harness `Session`; no golden is read or written.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

from tools.control.digi_client import oled_png
from tools.firmware import device as device_files
from tools.patchkit import pk_device
from tools.patchkit import pk_harness as H

ROOT = H.ROOT
MODULES = ["core/runtime", "core/ui", "test/ui-demo"]
FIRMWARE = pk_device.acceptance_firmware()  # the harness' default firmware per device
# rows of the demo menu (tests/fixtures/modules/ui-demo/src/ui_demo.c)
TOGGLE, VALUE, TOAST, CONFIRM, CHOICE, PAGE, LIST, EMPTY, TEXT = range(9)


def heap_walk(w, desc: int) -> dict:
    """The free lists of the firmware's buddy allocator: `{init, base, bitmap, lists, max order, blocks, allocs}`; free block headers hold
    prev at +4 and next at +8 (tools/coldfire/digikit_health_proof.py has the same walk with more checks)."""
    init, base, _bitmap, lists, maxo, nblocks, allocs = (
        w(desc + 4 * k) for k in range(7)
    )
    res = {"allocs": allocs, "free": 0, "nodes": 0, "bad": []}
    if init != 1 or maxo > 31 or nblocks != 1 << maxo:
        res["bad"].append("descriptor")
        return res
    seen: set[int] = set()
    for order in range(maxo + 1):
        idx = w(lists + 4 * order)
        while idx < 0x80000000:
            if idx >= nblocks or idx in seen:
                res["bad"].append(f"order {order}: bad index {idx:#x}")
                break
            seen.add(idx)
            res["free"] += 16 << order
            res["nodes"] += 1
            idx = w(base + 16 * idx + 8)
            if res["nodes"] > 200000:
                res["bad"].append("too many nodes")
                break
    return res


class Demo:
    def __init__(self, device: str, out: Path | None):
        cell = H.Cell(device, FIRMWARE[device], list(MODULES))
        self.cell, self.out = cell, out
        self.s = H.Session(cell, H.Goldens(), cache_dir=H.SHARED_CACHE)
        self.row = 0
        self.shots: dict[str, str] = {}
        self.fail: list[str] = []

    # -- plumbing
    def sym(self, name: str) -> int:
        return self.s.symbol(name, "test/ui-demo")

    def u32(self, name: str) -> int:
        return self.s.emu.peek32(self.sym(name)) or 0

    def tap(self, key: str, wait: int = 250) -> None:
        self.s.do_tap(key)
        self.s.run_ms(wait)

    def shot(self, name: str, wait: int = 0) -> str:
        if wait:
            self.s.run_ms(wait)
        digest, raw = self.s.frame()
        self.shots[name] = digest
        if self.out:
            self.out.mkdir(parents=True, exist_ok=True)
            (self.out / f"{name}.png").write_bytes(oled_png(raw))
        print(f"shot {name} {digest[:12]}", flush=True)
        return digest

    def frame_hash(self) -> str:
        return self.s.frame()[0]

    def expect(self, name: str, ok: bool, detail: str = "") -> None:
        print(f"{'ok  ' if ok else 'FAIL'} {name} {detail}", flush=True)
        if not ok:
            self.fail.append(name)

    def goto(self, row: int) -> None:
        while self.row < row:
            self.tap("DOWN", 150)
            self.row += 1
        while self.row > row:
            self.tap("UP", 150)
            self.row -= 1

    def to_demo(self) -> None:
        """Main screen to SETTINGS > DIGIKIT (the ninth row), the demo menu open on its first row."""
        self.s.reset()
        self.tap("NO", 300)
        self.tap("SETTINGS", 400)
        for _ in range(8):
            self.tap("DOWN", 150)
        self.tap("YES", 600)
        self.row = 0

    # -- screenshots
    def shots_run(self) -> None:
        self.to_demo()
        self.shot("01_menu")
        self.tap("YES", 200)  # TOGGLE flips ON -> OFF
        self.shot("02_toggle_off")
        before = self.u32("dk_demo_value")
        self.goto(VALUE)
        self.s.turn_burst("ENC A", 1, 30)
        self.s.run_ms(200)
        after = self.u32("dk_demo_value")
        self.shot("03_value_changed")
        self.expect(
            "encoder changes the value row", after != before, f"{before} -> {after}"
        )
        self.goto(PAGE)
        self.tap("YES", 500)
        self.shot("04_text_page")
        self.tap("DOWN", 200)
        self.shot("05_text_page_down")
        self.tap("NO", 400)
        self.goto(LIST)
        self.tap("YES", 500)
        self.shot("06_list")
        self.tap("DOWN", 200)
        self.tap("YES", 300)
        self.shot("07_list_check")
        checks = [self.u32("dk_demo_checks")]
        self.expect(
            "check callback ran for row 1",
            checks[0] == 0
            and (self.s.emu.peek32(self.sym("dk_demo_checks") + 4) or 0) == 1,
        )
        self.tap("RIGHT", 400)
        self.shot("08_list_panel")
        ev0 = self.u32("dk_demo_events_n")
        self.tap("DOWN", 200)
        self.tap("YES", 150)
        self.shot("09_panel_row_toast")
        self.expect("panel row ran", self.u32("dk_demo_events_n") == ev0 + 1)
        self.s.run_ms(2500)
        self.tap("DOWN", 200)
        self.shot("10_list_after_panel")  # focus is back on the list: the cursor moves
        self.tap("YES", 300)  # check the third item: the `back` callback now refuses NO
        ev1 = self.u32("dk_demo_events_n")
        before = self.shot("10b_list_third_checked")
        self.tap("NO", 300)
        after = self.shot("10c_list_after_refused_no")
        self.expect(
            "back runs and keeps the list open",
            self.u32("dk_demo_events_n") == ev1 + 1 and before == after,
        )
        self.tap("YES", 300)  # uncheck: NO closes
        self.tap("NO", 400)
        self.goto(EMPTY)
        self.tap("YES", 500)
        self.shot("11_empty_list")
        self.tap("NO", 400)
        self.goto(CONFIRM)
        self.tap("YES", 500)
        self.shot("12_confirm")
        self.tap("NO", 400)
        self.expect("confirm NO answers 0", self.u32("dk_demo_answer_v") == 0)
        self.tap("YES", 500)
        self.tap("YES", 400)
        self.expect("confirm YES answers 1", self.u32("dk_demo_answer_v") == 1)
        self.goto(CHOICE)
        self.tap("YES", 500)
        self.shot("13_choice")
        self.tap("DOWN", 200)
        self.tap("YES", 400)
        self.expect("choice answers the index", self.u32("dk_demo_answer_v") == 1)
        self.tap("YES", 500)
        d1 = self.shot("14_choice_again")
        self.tap("NO", 600)  # cancelled: no callback, the dialog must still be gone
        self.tap("YES", 500)
        d2 = self.shot("15_choice_after_cancel")
        self.expect("a cancelled choice frees the dialog slot", d1 == d2)
        self.tap("NO", 400)
        self.goto(TOAST)
        self.tap("YES", 100)
        self.shot(
            "16_toast_behind_menu"
        )  # a toast is part of the main screen: the open menus hide it
        self.tap("NO", 150)
        self.tap("NO", 150)
        self.shot("17_toast_main_screen", 100)
        self.s.health("end")

    # -- heap
    def desc(self) -> int:
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

    def heap(self) -> dict:
        return heap_walk(lambda a: self.s.emu.peek32(a) or 0, self.desc())

    def cycle_menu(self) -> None:
        self.tap("YES", 250)
        self.tap("NO", 250)

    def cycle_list(self) -> None:
        self.goto(LIST)
        self.tap("YES", 250)
        self.tap("DOWN", 100)
        self.tap("YES", 100)  # check
        self.tap("RIGHT", 150)  # panel
        self.tap("DOWN", 100)
        self.tap("YES", 150)  # panel row: closes the panel, runs the row (a toast)
        self.tap("NO", 250)  # the list closes
        self.s.run_ms(2300)  # the toast goes away

    def cycle_list_open(self) -> None:
        self.goto(LIST)
        self.tap("YES", 250)
        self.tap("NO", 250)

    def cycle_list_check(self) -> None:
        self.goto(LIST)
        self.tap("YES", 250)
        self.tap("DOWN", 100)
        self.tap("YES", 100)
        self.tap("NO", 250)

    def cycle_list_panel(self) -> None:
        self.goto(LIST)
        self.tap("YES", 250)
        self.tap("RIGHT", 150)
        self.tap("NO", 150)  # the panel closes
        self.tap("NO", 250)  # the list closes

    def cycle_panel_row(self) -> None:
        self.goto(LIST)
        self.tap("YES", 250)
        self.tap("RIGHT", 150)
        self.tap("YES", 150)  # the row runs: a toast
        self.tap("NO", 250)
        self.s.run_ms(2300)

    def cycle_toast(self) -> None:
        self.goto(TOAST)
        self.tap("YES", 100)
        self.s.run_ms(2300)

    def cycle_choice(self) -> None:
        self.goto(CHOICE)
        self.tap("YES", 250)
        self.tap("YES", 250)

    def cycle_confirm(self) -> None:
        self.goto(CONFIRM)
        self.tap("YES", 250)
        self.tap("YES", 250)  # answer YES

    def leak_run(
        self, cycles: int, only: list[str] | None = None, blocks: int = 1
    ) -> dict:
        self.to_demo()
        report: dict = {}
        flows = [
            ("menu", self.cycle_menu),
            ("list+panel", self.cycle_list),
            ("confirm", self.cycle_confirm),
            ("list_open", self.cycle_list_open),
            ("list_check", self.cycle_list_check),
            ("list_panel", self.cycle_list_panel),
            ("panel_row", self.cycle_panel_row),
            ("toast", self.cycle_toast),
            ("choice", self.cycle_choice),
        ]
        flows = [
            f for f in flows if f[0] in (only or ["menu", "list+panel", "confirm"])
        ]
        for name, flow in flows:
            flow()  # warm up: the first use builds the menus
            self.s.run_ms(500)
            a = self.heap()
            for block in range(blocks):
                for _ in range(cycles):
                    flow()
                self.s.run_ms(500)
                b = self.heap()
                report[f"{name}#{block + 1}"] = {
                    "before": a,
                    "after": b,
                    "free_delta": b["free"] - a["free"],
                    "allocs_delta": b["allocs"] - a["allocs"],
                }
                print(
                    f"leak {name} block {block + 1} ({cycles} cycles): free {a['free']} -> {b['free']} ({b['free'] - a['free']:+d} bytes), "
                    f"live allocations {a['allocs']} -> {b['allocs']} ({b['allocs'] - a['allocs']:+d}), free-list nodes {a['nodes']} -> {b['nodes']}, "
                    f"bad {a['bad'] + b['bad']}",
                    flush=True,
                )
                a = b
        return report


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", choices=["shots", "leak", "keys"])
    ap.add_argument("--device", choices=device_files.shorts(), required=True)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--cycles", type=int, default=50)
    ap.add_argument(
        "--blocks",
        type=int,
        default=1,
        help="leak: measure this many blocks of CYCLES one after the other",
    )
    ap.add_argument(
        "--flows",
        default="",
        help="leak: flows to run (menu,list+panel,confirm,list_open,list_check,list_panel,panel_row,toast,choice)",
    )
    ap.add_argument(
        "--keys",
        default="",
        help="keys: comma list of KEY or KEY:COUNT, run after the demo menu opened (cmd keys)",
    )
    a = ap.parse_args(argv)
    d = Demo(a.device, a.out)
    with d.s:
        d.s.open()
        try:
            if a.cmd == "shots":
                d.shots_run()
            elif a.cmd == "keys":
                d.to_demo()
                for item in filter(None, a.keys.split(",")):
                    key, _, n = item.partition(":")
                    for _ in range(int(n or 1)):
                        if key.startswith("ENC"):
                            d.s.turn_burst(key, 1, 10)
                            d.s.run_ms(200)
                        else:
                            d.tap(key, 300)
                d.shot("keys")
            else:
                print(
                    json.dumps(
                        d.leak_run(
                            a.cycles, a.flows.split(",") if a.flows else None, a.blocks
                        ),
                        indent=1,
                    )
                )
        except (H.Failure, H.EmuError) as e:
            print(f"FAULT {e} {getattr(e.__cause__, 'data', None)}", flush=True)
            if d.s.emu is not None:
                print(
                    json.dumps(d.s.emu.status().get("cf"), default=str)[:1500],
                    flush=True,
                )
                try:
                    print(
                        "DIAG",
                        json.dumps(d.s.emu.call("diagnostics"), default=str)[:3000],
                        flush=True,
                    )
                except H.EmuError as e2:
                    print("DIAG none", e2, flush=True)
            raise
    for c in d.s.checks:
        if not c.ok:
            d.fail.append(f"{c.name}: {c.detail}")
    print("FAIL: " + "; ".join(d.fail) if d.fail else "PASS")
    return 1 if d.fail else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
