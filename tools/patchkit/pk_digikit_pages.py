#!/usr/bin/env python3
"""Parameter pages of a machine chosen on track 1: the OLED frames of the patched-in machine against the run-time one (finding 50).

Run by `tools/patchkit/pk_digikit_scen.py` as the `pages` kind. Both boots choose the machine in the SRC list, then press SRC again
(the page key) and turn encoders; the frames go to out/runs/lanes/srcpage-pages/ (never committed; DIGIKIT_PAGES_OUT). Kept checks:
the run-time machine's frames equal the patched-in twin's (the same machine, the same data; only the type number differs and it is
not drawn); an added machine has ONE SRC page: no page dots, the page key leaves the frame unchanged, the encoders still work; the
stock sample machines keep two pages (dots, sample view on one of them), also after an added machine was on the track.
"""

from __future__ import annotations

import io
import os
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image

from tools.patchkit import pk_dkrt_machine as M

ROOT = Path(__file__).resolve().parents[2]


def out_dir() -> Path:
    d = Path(
        os.environ.get(
            "DIGIKIT_PAGES_OUT", ROOT / "out" / "runs" / "lanes" / "srcpage-pages"
        )
    )
    d.mkdir(parents=True, exist_ok=True)
    return d


WAVE_ROW = 157  # the waveform's baseline in the sample view (the knob page has no long line there)


def sample_view(png: bytes) -> bool:
    """True when the frame is the sample view (SAMPLE, WERP, STRETCH, REPITCH: a line across the waveform area)."""
    im = np.array(Image.open(io.BytesIO(png)).convert("L"))
    return int((im[WAVE_ROW] > 128).sum()) > 300


DOTS = (
    slice(120, 172),
    slice(496, 512),
)  # the page dots at the right edge of the SRC page


def page_dots(png: bytes) -> bool:
    """True when the frame shows the page dots (the view has more than one page)."""
    im = np.array(Image.open(io.BytesIO(png)).convert("L"))
    return bool((im[DOTS] > 128).any())


def body(png: bytes) -> bytes:
    """The frame below the header line (the header shows the project name, a popup or the machine name by timing)."""
    return np.array(Image.open(io.BytesIO(png)).convert("L"))[44:].tobytes()


def pick(rig: Any, downs: int) -> None:
    """Choose the machine `downs` below the top of the SRC list (the list opens on the current machine: go to the top first)."""
    s = rig.s
    s.do_tap(["FUNC", "SRC"])
    s.run_ms(200)
    for _ in range(14):
        s.do_tap("UP")
        s.run_ms(80)
    for _ in range(downs):
        s.do_tap("DOWN")
        s.run_ms(80)
    s.do_tap("YES")
    s.run_ms(300)
    s.do_tap("YES")
    s.run_ms(300)


def run_pages(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT

    t = c.table()
    shots = out_dir()
    mods = [c.pack(m, f"m{i}.dkm") for i, m in enumerate(t["modules"])]
    run_img = c.image_for(RT, "run")
    ref_img = c.image_for(RT + "," + t["twin"], "ref")
    empty = c.card(None)
    card = c.card(mods)
    frames: dict[str, dict[str, bytes]] = {"ref": {}, "run": {}}

    def sequence(rig: Any, who: str, want: int) -> None:
        def snap(name: str) -> None:
            png = rig.emu.frame_png()
            frames[who][name] = png
            (shots / f"{who}-{name}.png").write_bytes(png)

        rig.select_type(want, t["hint"])
        rig.s.run_ms(500)
        snap("p0-after-select")
        rig.s.run_ms(2500)  # the header text and any popup settle
        snap("p0-idle")
        for key in ("DOWN", "UP"):  # the keys that flip the pages of a parameter view
            rig.s.do_tap(key)
            rig.s.run_ms(500)
            snap(f"p0-{key.lower()}")
        for page in (1, 2):
            rig.s.do_tap("SRC")  # the page key
            rig.s.run_ms(500)
            snap(f"p{page}-src")
            for enc in (1, 2, 5):  # a touched knob shows its parameter's name and value
                rig.s.turn_burst(enc, 4, 12)
                rig.s.run_ms(100)
                snap(f"p{page}-turn{enc}")
            rig.s.run_ms(2500)
            snap(f"p{page}-idle")
        rig.s.run_ms(300)

    def ref_boot() -> None:
        with c.rig(ref_img, empty, "ref") as rig:
            sequence(rig, "ref", t["static_type"])
            c.done(rig)

    def run_boot() -> None:
        with c.rig(run_img, card, "run") as rig:
            sequence(
                rig,
                "run",
                rig.dyn_type(
                    t.get("play_module", M.MODULE), t.get("play_machine", M.MACHINE)
                ),
            )
            # the same track, now a stock machine: its two pages come back (the hook cut the page vector for the added type)
            for downs in t.get("stock_downs", []):
                pick(rig, downs)
                typ = rig.type_now()
                if typ >= 7 or typ in after:
                    continue
                rig.s.run_ms(500)
                pair = []
                for page in (1, 2):
                    rig.s.do_tap("SRC")
                    rig.s.run_ms(500)
                    png = rig.emu.frame_png()
                    (shots / f"after{typ}-p{page}.png").write_bytes(png)
                    pair.append(png)
                after[typ] = pair
            c.done(rig)

    stock: dict[int, bytes] = {}
    after: dict[int, list[bytes]] = {}

    def stock_boot() -> None:
        with c.rig(run_img, empty, "stock") as rig:
            for downs in t.get("stock_downs", []):
                rig.select(downs)
                typ = rig.type_now()
                rig.s.run_ms(500)
                for page in (1, 2):
                    rig.s.do_tap("SRC")
                    rig.s.run_ms(500)
                    png = rig.emu.frame_png()
                    (shots / f"stock{typ}-p{page}.png").write_bytes(png)
                    if page == 1:
                        stock[typ] = png
            c.done(rig)

    c.parallel(ref_boot, run_boot, stock_boot)
    for name, png in frames["run"].items():
        c.check(
            f"frame {name}: the run-time machine draws what the patched-in twin draws",
            png == frames["ref"].get(name),
            f"{len(png)} / {len(frames['ref'].get(name, b''))} bytes",
        )
    shown = {n: sample_view(png) for n, png in frames["run"].items()}
    c.check(
        "an added machine never draws the sample view (finding 50)",
        not any(shown.values()),
        str([n for n, v in shown.items() if v]),
    )
    # one SRC page for an added machine: no dots on any frame, the page key changes nothing, the encoders work
    for who in ("ref", "run"):
        f = frames[who]
        dotted = [n for n, png in f.items() if page_dots(png)]
        c.check(
            f"{who}: no page dots on any SRC frame (one page)", not dotted, str(dotted)
        )
        c.check(
            f"{who}: the page keys (SRC again, DOWN, UP) leave the frame body unchanged",
            body(f["p1-src"]) == body(f["p0-idle"])
            and body(f["p2-src"]) == body(f["p1-idle"])
            and body(f["p0-down"]) == body(f["p0-idle"])
            and body(f["p0-up"]) == body(f["p0-idle"]),
            str(
                [
                    n
                    for n, ref in (
                        ("p1-src", "p0-idle"),
                        ("p2-src", "p1-idle"),
                        ("p0-down", "p0-idle"),
                        ("p0-up", "p0-idle"),
                    )
                    if body(f[n]) != body(f[ref])
                ]
            ),
        )
        moved = [e for e in ("turn1", "turn2", "turn5") if f[f"p1-{e}"] != f["p0-idle"]]
        c.check(
            f"{who}: the encoders still work (each turn changes the frame)",
            len(moved) == 3,
            str(moved),
        )
    sv = {typ: sample_view(png) for typ, png in stock.items()}
    c.check(
        "the stock sample machines still draw the sample view on their second page (the hook leaves types below machines.first alone)",
        bool(stock)
        and all(v for typ, v in sv.items() if typ in (0, 1, 3))
        and {0, 1, 3} <= set(sv),
        str(sv),
    )
    c.check(
        "stock SAMPLE, WERP, REPITCH keep two pages: dots on the knob page, the sample view on the other",
        bool(stock)
        and all(page_dots(png) for typ, png in stock.items() if typ in (0, 1, 3))
        and all(sv.get(typ) for typ in (0, 1, 3) if typ in sv),
        str({typ: page_dots(png) for typ, png in stock.items() if typ in (0, 1, 3)}),
    )
    after_ok = {
        typ: any(sample_view(p) for p in pair) and all(page_dots(p) for p in pair)
        for typ, pair in after.items()
        if typ in (0, 1, 3)
    }
    c.check(
        "after an added machine was on the track, a stock sample machine has its two pages again",
        bool(after_ok) and all(after_ok.values()),
        str(after_ok),
    )
    c.common_checks()
