#!/usr/bin/env python3
"""The `fxpage` kind of the digikit scenarios: the FX view pages and knobs of run-time effects (docs/design/digikit-registry.md "Effect pages", finding 61).

Run by `tools/patchkit/pk_digikit_scen.py`. The image is the runtime with `core/dynfx` and `core/fxpages` (the Digitakt II); the sound is the run-time machine
`test/sine` on track 1, the effect `test/dyn-fx-gain` (tab GAN, title Test Gain, one knob GAIN 0 .. 1.27 default 0.5, MIX default 127). Four boots, each in its
own daemon:

  step1   an image without `core/fxpages` (what step 1 of the effects feature built), the sine alone: the screenshots of the three stock pages and the PCM that
          `core/fxpages` must not change.
  stock   the image with the pages and no effect loaded: the same three screenshots, pixel for pixel, and the same PCM bit for bit.
  effect  the sine and the effect: the FX view has four pages (the fourth shows tab, title, GAIN and MIX), the rows and the list say so; turning GAIN changes the
          PCM as declared, MIX 0 is the dry PCM bit for bit, MIX 127 the wet one, an intermediate MIX the linear blend.
  gone    the effect taken out of the selection and a restart: three pages again (the stock screenshots) and the dry PCM.

Screenshots go to the scenario's work folder; `DIGIKIT_SHOTS=DIR` copies them to DIR.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import struct
import tomllib
from pathlib import Path
from typing import Any

import numpy as np

from tools.control.digi_client import oled_png
from tools.patchkit import pk_device
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_fx import W_P0, chain_rows, scaled

ROOT = Path(__file__).resolve().parents[2]

SINE = "test/sine"
FX = "test/dyn-fx-gain"
# DKRT_FXP (modules/core/runtime/src/rs/src/fxpage.rs `FxpState`, repr(C)): byte offsets of its fields
FXP_KIT, FXP_SENT, FXP_ERRORS, FXP_SEEDS = 4, 16, 20, 24
FXP_OWNER, FXP_NKNOB, FXP_CUR, FXP_DSP = 28, 32, 44, 172
W_MIX = 13
ONE = 0x3F800000
# the firmware's FX view list (DEL, REV, CHO): rodata at `cf.page.data.fx_view_list`, the page numbers differ by device
STOCK_PAGES = {"dt2": [13, 14, 15], "dn2": [17, 18, 19]}
# the encoders: knob A and the level knob
ENC_A, ENC_LEVEL = "ENC A", "LEVEL"


def f32(x: float) -> np.float32:
    return np.float32(x)


def bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def unbits(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFFFFFF))[0]


class Shots:
    """Screenshots of the OLED: the PNG in the work folder (and in DIGIKIT_SHOTS), and the hash of the raw frame."""

    def __init__(self, work: Path, prefix: str):
        self.dir = work / "shots"
        self.dir.mkdir(parents=True, exist_ok=True)
        self.prefix = prefix
        self.hashes: dict[str, str] = {}

    def take(self, rig: Any, name: str) -> str:
        _, raw = rig.emu.frame()
        png = oled_png(raw, 3)
        fn = f"{self.prefix}-{name}.png"
        (self.dir / fn).write_bytes(png)
        if out := os.environ.get("DIGIKIT_SHOTS"):
            Path(out).mkdir(parents=True, exist_ok=True)
            shutil.copy(self.dir / fn, Path(out) / fn)
        h = hashlib.sha256(raw).hexdigest()
        self.hashes[name] = h
        return h


def open_send_fx(rig: Any) -> None:
    """FUNC + FX: the SEND FX pages, the first (Delay)."""
    rig.s.do_tap(["FUNC", "FX"], 20)
    rig.s.run_ms(400)


def page_down(rig: Any) -> None:
    rig.s.do_tap("DOWN", 20)
    rig.s.run_ms(300)


def stock_pages(rig: Any, shots: Shots) -> list[str]:
    """The three stock pages as frames: DEL, REV, CHO."""
    open_send_fx(rig)
    out = []
    for i, n in enumerate(("del", "rev", "cho")):
        if i:
            page_down(rig)
        out.append(shots.take(rig, n))
    return out


def fxp(rig: Any) -> int:
    return M.sym(rig.plan, "core/runtime", "DKRT_FXP")


def fxp_words(rig: Any, off: int, n: int) -> list[int]:
    base = fxp(rig) + off
    return [
        struct.unpack(">i", struct.pack(">I", rig.w(base + 4 * i)))[0] for i in range(n)
    ]


def fxp_state(rig: Any) -> dict[str, Any]:
    owner = struct.pack(">I", rig.w(fxp(rig) + FXP_OWNER))
    nknob = struct.pack(">I", rig.w(fxp(rig) + FXP_NKNOB))
    return {
        "kit": rig.w(fxp(rig) + FXP_KIT),
        "sent": rig.w(fxp(rig) + FXP_SENT),
        "errors": rig.w(fxp(rig) + FXP_ERRORS),
        "seeds": rig.w(fxp(rig) + FXP_SEEDS),
        "owner": list(owner),
        "nknob": list(nknob),
        "cur0": fxp_words(rig, FXP_CUR, 8),
        "dsp0": fxp_words(rig, FXP_DSP, 8),
    }


def profile(dev: str) -> dict[str, Any]:
    """`[fx]` of the device file: the facts the runtime and the patch take from it."""
    f = {"dt2": "digitakt-ii", "dn2": "digitone-ii"}[dev]
    return tomllib.loads((ROOT / "devices" / f"{f}.toml").read_text())["fx"]


def u16(rig: Any, addr: int) -> int:
    """The big-endian halfword at ADDR, at any alignment (the Digitone II's kit objects start at odd addresses)."""
    return (
        (rig.w(addr & ~3) >> (8 * (2 - addr % 4)) & 0xFFFF)
        if addr % 4 <= 2
        else ((rig.w(addr & ~3) & 0xFF) << 8 | rig.w((addr & ~3) + 4) >> 24)
    )


def put16(rig: Any, addr: int, raw: int) -> None:
    """Write the big-endian halfword at ADDR, at any alignment (aligned words are read and written whole)."""
    for i, byte in enumerate((raw >> 8 & 0xFF, raw & 0xFF)):
        a = addr + i
        shift = 8 * (3 - a % 4)
        rig.emu.poke32(a & ~3, (rig.w(a & ~3) & ~(0xFF << shift)) | byte << shift)


class Kit:
    """The live kit's array of effect values, as the firmware's pages edit it."""

    def __init__(self, rig: Any, fx: dict[str, Any]):
        self.rig, self.fx = rig, fx

    def raw(self, k: int, j: int) -> int:
        kit = self.rig.w(self.fx["kit_ptr"])
        return u16(
            self.rig,
            kit + self.fx["array_off"] + 2 * self.fx["value_indices"][8 * k + j],
        )

    def set_raw(self, enc: str, k: int, j: int, target: int, tol: int = 0x200) -> int:
        """Turn ENC (bursts of fast events, the sign by the error) until the value reads within TOL of TARGET; returns it. Settles the knob's writes after."""
        for _ in range(120):
            err = target - self.raw(k, j)
            if abs(err) <= tol:
                self.rig.s.run_ms(60)
                return self.raw(k, j)
            self.rig.s.turn_burst(
                enc, 8 if err > 0 else -8, 3 if abs(err) < 0x1000 else 20
            )
        raise RuntimeError(
            f"turning {enc}: value {self.raw(k, j):#x}, wanted {target:#x}"
        )

    def poke(self, index: int, raw: int) -> None:
        """Write the halfword INDEX of the live kit's array (what a page's knob does, without the page)."""
        kit = self.rig.w(self.fx["kit_ptr"])
        put16(self.rig, kit + self.fx["array_off"] + 2 * index, raw)

    def snapshot(self) -> list[int]:
        """All values of the live kit's array (`[fx] tail_off` bytes: 71 on the Digitakt II, 101 on the Digitone II)."""
        kit = self.rig.w(self.fx["kit_ptr"])
        return [
            u16(self.rig, kit + self.fx["array_off"] + 2 * i)
            for i in range(self.fx["tail_off"] // 2)
        ]

    def to_min(self, enc: str) -> None:
        self.rig.s.turn_burst(enc, -8, 80)
        self.rig.s.run_ms(60)

    def to_max(self, enc: str, k: int, j: int) -> None:
        """Turn ENC up in bursts until the value sits at the top (0x7f00). How many events of a burst the firmware takes depends on the
        timing of its UI passes (the encoder accelerates by the time between events), so one burst may stop short."""
        for _ in range(6):
            self.rig.s.turn_burst(enc, 8, 80)
            self.rig.s.run_ms(60)
            if self.raw(k, j) == 0x7F00:
                return


def knob_float(lo: float, hi: float, raw: int) -> float:
    """The runtime's conversion in float32: min + span * (raw / 0x7f00)."""
    return float(f32(lo) + f32(f32(hi) - f32(lo)) * (f32(raw) / f32(32512)))


def run_fxpage(c: Any) -> None:
    from tools.patchkit.pk_digikit_scen import RT, cstr

    t = c.table()
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    slots = pk_device.limits(c.device).fx_slots
    fxp_cfg = profile(c.device)
    img = c.image_for(RT + ",core/dynfx,core/fxpages", "fxpage")
    img1 = c.image_for(RT + ",core/dynfx", "fxpage-step1")
    sine = c.pack(SINE, "m0.dkm")
    fxf = c.pack(FX, "m1.dkm")
    card_sine, card_fx = (
        c.card([sine]),
        c.card([sine, fxf]),
    )  # built before the parts run side by side (one file for one content)
    pcm: dict[str, np.ndarray] = {}
    frames: dict[str, list[str]] = {}
    exp: dict[str, float] = {}
    LO, HI = 0.0, 1.27  # the GAIN knob of test/dyn-fx-gain

    def sound(rig: Any, wav: Path, restarted: bool = False) -> np.ndarray:
        return rig.note(wav, rig.frames() + 300 if restarted else trig)

    def to_main(rig: Any) -> None:
        rig.s.do_tap("NO", 20)
        rig.s.run_ms(300)

    def check_pcm(
        tag: str,
        ref: np.ndarray,
        got: np.ndarray,
        gain: float,
        what: str,
        ulps: float = 2.0,
    ) -> None:
        r = scaled(ref, got, gain)
        db = 20 * np.log10(gain) if gain > 0 else float("-inf")
        c.check(
            f"{tag}: the PCM is the dry PCM times {gain:.6f} ({what}), within {ulps} ulps from the onset on",
            r["rms_db"] is not None
            and r["max_err_ulps"] <= ulps
            and abs(r["rms_db"] - db) < 0.001
            and abs(r["shift"]) <= 128,
            f"rms {r['rms_db']} want {db:.4f}, max err {r['max_err_ulps']:.2f} ulps, onset shift {r['shift']}",
        )

    def step1() -> None:
        with c.rig(img1, card_sine, "step1") as rig:
            info = rig.boot_info()
            c.check(
                "step1: the sine loaded", info["results"] == [0], f"{info['results']}"
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            frames["step1"] = stock_pages(rig, Shots(c.work, "step1"))
            to_main(rig)
            pcm["step1"] = sound(rig, c.work / "step1.wav")
            c.done(rig)

    def stock() -> None:
        with c.rig(img, card_sine, "stock") as rig:
            info = rig.boot_info()
            c.check(
                "stock: the sine loaded", info["results"] == [0], f"{info['results']}"
            )
            c.check(
                "stock: no effect, so the FX view's page count cell is 0 (the stock list) and no page is owned",
                rig.w(M.sym(rig.plan, "core/fxpages", "fxp_count")) == 0
                and fxp_state(rig)["owner"] == [0, 0, 0, 0],
                f"{fxp_state(rig)['owner']}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            frames["stock"] = stock_pages(rig, Shots(c.work, "stock"))
            to_main(rig)
            pcm["stock"] = sound(rig, c.work / "stock.wav")
            # a second note on the same boot, as the effect part takes its notes: the control for the note with the free indices poked
            pcm["ctl"] = rig.note(c.work / "stock-ctl.wav", rig.frames() + 300)
            # the free value indices of [fx]: poked with distinct values in a kit with no effect, nothing else moves and the sound is the same
            kit = Kit(rig, fxp_cfg)
            idxs = [*fxp_cfg["value_indices"], fxp_cfg["mark_index"]]
            quiet0 = kit.snapshot()
            rig.s.run_ms(300)
            drift = {
                i
                for i, (x, y) in enumerate(zip(quiet0, kit.snapshot(), strict=True))
                if x != y
            }
            before = kit.snapshot()
            for n, i in enumerate(idxs):
                kit.poke(i, 0x1000 + 0x111 * n)
            rig.s.run_ms(500)
            after = kit.snapshot()
            moved = {
                i for i, (x, y) in enumerate(zip(before, after, strict=True)) if x != y
            }
            c.check(
                f"stock: the {len(idxs)} free value indices ([fx] value_indices and mark_index) take distinct values and keep them; no other index moved",
                all(after[i] == 0x1000 + 0x111 * n for n, i in enumerate(idxs))
                and not (moved - set(idxs) - drift),
                f"moved {sorted(moved - set(idxs))} drift {sorted(drift)}",
            )
            pcm["poked"] = rig.note(c.work / "stock-poked.wav", rig.frames() + 300)
            c.done(rig)

    def effect() -> None:
        with c.rig(img, card_fx, "effect") as rig:
            info = rig.boot_info()
            c.check(
                "effect: the sine and the effect loaded",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            shots = Shots(c.work, "effect")
            open_send_fx(rig)
            pages = []
            for i in range(4):
                if i:
                    page_down(rig)
                pages.append(shots.take(rig, f"page{i + 1}"))
            c.check(
                "effect: the FX view has four pages; the first three are the stock pages with the title count (n/4), the fourth is new",
                len(set(pages)) == 4,
                f"{[p[:8] for p in pages]}",
            )
            # the rows the runtime wrote
            st = fxp_state(rig)
            plan = rig.plan
            count = rig.w(M.sym(plan, "core/fxpages", "fxp_count"))
            lst = M.sym(plan, "core/fxpages", "fxp_list_buf")
            listed = [rig.w(lst + 4 * i) for i in range(count)]
            row0 = M.sym(plan, "core/fxpages", "fxp_rows")
            first = fxp_cfg["page_first"]
            c.check(
                "effect: the page list holds the three stock pages and the effect's page",
                count == 4 and listed == [*STOCK_PAGES[c.device], first],
                f"count {count} {listed}",
            )
            p_first = M.sym(plan, "core/fxpages", "params.fp0.index")
            codes = [rig.w(row0 + 8 + 4 * i) for i in range(9)]
            c.check(
                "effect: the page row has the knob at position 0, empty positions 1 to 7 and MIX on the level slot",
                codes == [p_first, 0, 0, 0, 0, 0, 0, 0, p_first + 7],
                f"{codes}",
            )
            tab_at, title_at = rig.w(row0), rig.w(row0 + 4)
            c.check(
                "effect: the page row's tab and title are the manifest's",
                cstr(rig, tab_at) == "GAN" and cstr(rig, title_at) == "Test Gain",
                f"{cstr(rig, tab_at)!r} {cstr(rig, title_at)!r}",
            )
            pf = M.sym(plan, "core/params", "pf_table")

            def prow(code: int) -> list[int]:
                return [rig.w(pf + 60 * code + 4 * i) for i in range(15)]

            g, m = prow(p_first), prow(p_first + 7)
            idx = fxp_cfg["value_indices"]
            c.check(
                "effect: the GAIN row's mirror index is the effect row's value index 0, range 0 .. 0x7f00, default 0x3200 (0.5); the MIX row is index 7, default 0x7f00",
                (g[1], g[2], g[3], g[4]) == (idx[0], 0, 0x7F00, 0x3200)
                and (m[1], m[2], m[3], m[4]) == (idx[7], 0, 0x7F00, 0x7F00),
                f"{g[1:5]} {m[1:5]}",
            )
            c.check(
                "effect: the rows' names are the manifest's (Gain / GAIN) and Mix / MIX",
                (cstr(rig, g[10]), cstr(rig, g[12]), cstr(rig, m[10]), cstr(rig, m[12]))
                == ("Gain", "GAIN", "Mix", "MIX"),
                f"{cstr(rig, g[10])} {cstr(rig, g[12])} {cstr(rig, m[10])} {cstr(rig, m[12])}",
            )
            kit = Kit(rig, fxp_cfg)
            c.check(
                "effect: the kit's value array holds the defaults (the init hook of core/fxpages or the runtime put them in the live kit)",
                (kit.raw(0, 0), kit.raw(0, 7)) == (0x3200, 0x7F00),
                f"{kit.raw(0, 0):#x} {kit.raw(0, 7):#x} seeds {st['seeds']}",
            )
            row = chain_rows(rig, slots)[0]
            c.check(
                "effect: the chain's row has p0 = 0.5 and the mix word 1.0",
                row[W_P0] == bits(0.5) and row[W_MIX] == ONE,
                f"{row[W_P0]:#x} {row[W_MIX]:#x}",
            )

            def back_to_page4() -> None:
                open_send_fx(rig)
                for _ in range(3):
                    page_down(rig)

            def row_words() -> tuple[int, int]:
                r = chain_rows(rig, slots)[0]
                return r[W_P0], r[W_MIX]

            # A: the defaults (gain 0.5, mix 1.0): the effect's old behaviour
            to_main(rig)
            pcm["a"] = sound(rig, c.work / "effect-a.wav")
            # B: turn GAIN; the rest of the array may only move as it moves without a turn (LFOs and smoothing run on their own)
            back_to_page4()
            quiet0 = kit.snapshot()
            rig.s.run_ms(300)
            drift = {
                i
                for i, (x, y) in enumerate(zip(quiet0, kit.snapshot(), strict=True))
                if x != y
            }
            before = kit.snapshot()
            raw_g = kit.set_raw(ENC_A, 0, 0, 0x6400)
            moved = {
                i
                for i, (x, y) in enumerate(zip(before, kit.snapshot(), strict=True))
                if x != y
            }
            own = {fxp_cfg["value_indices"][0]}
            c.check(
                "effect: turning GAIN changed the kit's value array at its own index only (no stock effect parameter moved)",
                own <= moved and not (moved - own - drift),
                f"moved {sorted(moved)} own {sorted(own)} drift {sorted(drift)}",
            )
            shots.take(rig, "page4-gain-turned")
            want_g = knob_float(LO, HI, raw_g)
            exp["g"] = want_g
            p0, mix = row_words()
            c.check(
                f"effect: turning GAIN to raw {raw_g:#x} puts p0 = {want_g:.7f} (min + span * raw / 0x7f00 in float32) in the chain's row and leaves the mix at 1.0",
                p0 == bits(want_g) and mix == ONE,
                f"p0 {unbits(p0):.7f} want {want_g:.7f}, mix {mix:#x}",
            )
            to_main(rig)
            pcm["b"] = note_again(rig, "effect-b")
            # C: MIX in the middle
            back_to_page4()
            raw_m = kit.set_raw(ENC_LEVEL, 0, 7, 0x4000)
            shots.take(rig, "page4-mix-turned")
            want_m = float(f32(raw_m) / f32(32512))
            exp["m"] = want_m
            p0, mix = row_words()
            c.check(
                f"effect: turning MIX to raw {raw_m:#x} puts {want_m:.7f} (raw / 0x7f00) in the row's mix word",
                mix == bits(want_m) and p0 == bits(want_g),
                f"mix {unbits(mix):.7f} want {want_m:.7f}",
            )
            to_main(rig)
            pcm["c"] = note_again(rig, "effect-c")
            # D: MIX 0
            back_to_page4()
            kit.to_min(ENC_LEVEL)
            c.check(
                "effect: turning MIX to the bottom gives raw 0 and a mix word of 0",
                kit.raw(0, 7) == 0 and row_words()[1] == 0,
                f"{kit.raw(0, 7)} {row_words()[1]:#x}",
            )
            shots.take(rig, "page4-mix-zero")
            to_main(rig)
            pcm["d"] = note_again(rig, "effect-d")
            # E: MIX back to the top
            back_to_page4()
            kit.to_max(ENC_LEVEL, 0, 7)
            c.check(
                "effect: turning MIX to the top gives raw 0x7f00 and a mix word of exactly 1.0",
                kit.raw(0, 7) == 0x7F00 and row_words()[1] == ONE,
                f"{kit.raw(0, 7):#x} {row_words()[1]:#x}",
            )
            to_main(rig)
            pcm["e"] = note_again(rig, "effect-e")
            st2 = fxp_state(rig)
            c.check(
                "effect: every word that changed was sent (no transfer failed) and the DSP has what the kit has",
                st2["errors"] == 0
                and st2["sent"] >= 4
                and st2["cur0"][0] == st2["dsp0"][0]
                and st2["cur0"][7] == st2["dsp0"][7],
                f"{st2}",
            )
            # the effect taken out: three pages and the dry sound again
            r = rig.set_selection([sine])
            c.check("gone: SELECT of the sine alone is accepted", r == 0, f"{r}")
            rig.restart()
            rig.emu.pcm_start()
            rig.s.pcm_mark = 0
            info = rig.boot_info()
            c.check(
                "gone: after the restart only the sine is loaded",
                info["results"] == [0] and info["safe"] == 0,
                f"{info['results']}",
            )
            c.check(
                "gone: the page list cell is 0 again and no page is owned",
                rig.w(M.sym(plan, "core/fxpages", "fxp_count")) == 0
                and fxp_state(rig)["owner"] == [0, 0, 0, 0],
                f"{fxp_state(rig)['owner']}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            frames["gone"] = stock_pages(rig, Shots(c.work, "gone"))
            to_main(rig)
            pcm["gone"] = sound(rig, c.work / "gone.wav", restarted=True)
            c.done(rig)

    def note_again(rig: Any, name: str) -> np.ndarray:
        return rig.note(c.work / f"{name}.wav", rig.frames() + 300)

    c.parallel(step1, stock, effect)
    # the stock view and the PCM, with and without the pages
    c.check(
        "stock: the three stock pages (DEL, REV, CHO) are pixel for pixel the pages of the image without core/fxpages",
        frames["stock"] == frames["step1"],
        f"{[f[:8] for f in frames['stock']]} {[f[:8] for f in frames['step1']]}",
    )
    r0 = scaled(pcm["step1"], pcm["stock"], 1.0)
    c.check(
        "stock: with the pages and no effect the PCM is the PCM of the image without them, bit for bit from the onset on",
        bool(r0["identical"]) and abs(r0["shift"]) <= 128,
        f"onset shift {r0['shift']}, max err {r0.get('max_abs_err')}",
    )
    # the second and third note of one boot retrigger a sine that still sounds (it lasts 2 s, a capture is 1 s), so the first non-zero sample is not the note's start and the
    # two captures are not bit for bit equal: the sound is compared by its level over the common length (the sine has the same level whatever its phase)
    r1 = scaled(pcm["ctl"], pcm["poked"], 1.0)
    c.check(
        "stock: with the free value indices poked the sound is the control note's: the same level within 0.2 dB (nothing reads the indices)",
        r1["rms_db"] is not None
        and abs(r1["rms_db"]) < 0.2
        and abs(r1["shift"]) <= 512,
        f"rms {r1['rms_db']} dB, onset shift {r1['shift']}",
    )
    dry = pcm["stock"]
    g, m = exp["g"], exp["m"]
    check_pcm(
        "effect A",
        dry,
        pcm["a"],
        0.5,
        "defaults: GAIN 0.5, MIX 127: the effect's old behaviour",
    )
    check_pcm(
        "effect B",
        dry,
        pcm["b"],
        g,
        f"GAIN turned to {g:.6f}, MIX 127: the wet signal",
        ulps=3.0,
    )
    blend_gain = 1.0 + m * (g - 1.0)
    check_pcm(
        "effect C",
        dry,
        pcm["c"],
        blend_gain,
        f"MIX {m:.6f}: dry + mix * (wet - dry) = dry * (1 + mix * (gain - 1))",
        ulps=4.0,
    )
    rd = scaled(dry, pcm["d"], 1.0)
    c.check(
        "effect D: MIX 0 gives the dry PCM bit for bit from the onset on",
        bool(rd["identical"]) and abs(rd["shift"]) <= 128,
        f"onset shift {rd['shift']}, max err {rd.get('max_abs_err')}",
    )
    check_pcm(
        "effect E", dry, pcm["e"], g, "MIX back at 127: the wet signal again", ulps=3.0
    )
    rg = scaled(pcm["step1"], pcm["gone"], 1.0)
    c.check(
        "gone: after deselecting the effect and restarting the PCM is the dry PCM, bit for bit from the onset on",
        bool(rg["identical"]) and abs(rg["shift"]) <= 128,
        f"onset shift {rg['shift']}, max err {rg.get('max_abs_err')}",
    )
    c.check(
        "gone: the three stock pages are pixel for pixel the stock pages again (title count (n/3))",
        frames["gone"] == frames["step1"],
        f"{[f[:8] for f in frames['gone']]} {[f[:8] for f in frames['step1']]}",
    )
    c.common_checks()
