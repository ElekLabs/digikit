#!/usr/bin/env python3
"""The `fxshift` kind of the digikit scenarios: the frequency shifter on a Digitakt II and a Digitone II (both with send cells, finding 67), page, knobs and sound (docs/findings/54-module-cookbook.md, 61-fx-pages.md, 66-freqshift-send.md).

Run by `tools/patchkit/pk_digikit_scen.py` (the scenarios of the shifter module; `[fxshift] module` names the effect). The image is the runtime with `core/dynfx` and `core/fxpages`; the sound is the run-time machine `test/sine` on track 1,
a sine of one frequency f0. The effect's manifest says `route = "send"`, MIX 127: the master proof needs the master route. The settings record on the card says `fx/route/<id of the effect>` = master
(the runtime's settings copy A, as in `fxroute`), on both devices. Boots, each in its own daemon:

  dry     the sine alone: the reference PCM and f0.
  shift   the master route: the sine and the effect (tab FSH, title Freq Shifter, the knob SHIFT -64 .. +63 bipolar, default 0, MIX default 127):
          page A  loaded with its defaults (SHIFT 0, MIX 127: fully shifted by 0 Hz) the sound is the dry sound's magnitude (an allpass pair: 0.5 % in amplitude); the page row, the names, the value indices and the defaults are the manifest's;
          B       MIX turned to 0: the PCM is the dry PCM bit for bit; MIX back to the top;
          C       SHIFT turned up: p0 is the float the knob stands for and the sine moves by exactly the shift of the routine's mapping (inc 48000 / 2^32 Hz); the image is 40 dB down;
          D       SHIFT turned down: the same downwards;
          E       the SHIFT and MIX values go into a kit (Save Kit), SHIFT is turned back up, Load Kit gives the saved value back (kit array, DSP row word, and the sound moves down again).
  send    the default route: the effect is a send effect (route word 1, the FX key's second page FSH); the amount 40 on track 1 turned on page 2 (`FSH Send=40`), SHIFT up: the FX return holds
          amount^2 x MIX x the shifted track: the PCM minus the dry PCM is the sine moved by the shift, amplitude (40/127)^2 x MIX x the dry amplitude, the image 40 dB down, and the same figures as the float32
          model `Shifter` of the module's `freqshift_ref.py` gives on the dry sound times (40/127)^2; MIX turned to half halves the added part.
  other   the amount is on track 2 only (silent) and track 1 plays: the PCM is the dry PCM bit for bit.

Screenshots (the default page, the page while SHIFT is turned each way, MIX turned, the send page) go to the scenario's work folder; `DIGIKIT_SHOTS=DIR` copies them to DIR."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from typing import Any

import numpy as np

from tools.device.dksettings import encode as encode_record
from tools.patchkit import modroots, pk_device
from tools.patchkit import pk_dkrt_machine as M
from tools.patchkit.pk_digikit_debug import uploader
from tools.patchkit.pk_digikit_fx import W_NFAULT, W_P0, chain_rows, scaled
from tools.patchkit.pk_digikit_fxpage import (
    ENC_A,
    ENC_LEVEL,
    ONE,
    SINE,
    STOCK_PAGES,
    W_MIX,
    Kit,
    Shots,
    bits,
    knob_float,
    open_send_fx,
    page_down,
    profile,
    u16,
    unbits,
)
from tools.patchkit.pk_digikit_fxpersist import KIT_AREAS, Run
from tools.patchkit.pk_digikit_fxsend import aligned

FX = ""  # the effect under test: `[fxshift] module` of the scenario
R: Any = None  # its float32 reference model (freqshift_ref.py in the tests folder of the module)
W_ROUTE, W_AMT0 = 18, 19  # row words of the send route (core/fx module.toml)
SOUND_ARRAY = 0x14
AMOUNT = 40  # the send amount the scenario turns to
SEND_RAW = 0x6800  # SHIFT of the send proof: the same as the master's step C
LO, HI = -64.0, 63.0  # the SHIFT knob
FS = 48000.0
RAW_MAX = 0x7F00


def channels(wav: Path) -> np.ndarray:
    """The left channel of a float WAV (the capture is stereo)."""
    data = wav.read_bytes()
    pos, nch = 12, 2
    while pos + 8 <= len(data):
        tag, size = (
            data[pos : pos + 4],
            int.from_bytes(data[pos + 4 : pos + 8], "little"),
        )
        if tag == b"fmt ":
            nch = int.from_bytes(data[pos + 10 : pos + 12], "little")
        elif tag == b"data":
            x = np.frombuffer(data[pos + 8 : pos + 8 + size // 4 * 4], dtype="<f4")
            return x.reshape(-1, nch)[:, 0].astype(np.float64)
        pos += 8 + size + (size & 1)
    raise ValueError(f"{wav}: no data chunk")


def amplitude(y: np.ndarray, f: float) -> float:
    """The amplitude of the component of y at exactly f Hz (a Hann-windowed single-bin projection)."""
    n = np.arange(len(y))
    w = np.hanning(len(y))
    return float(2 * abs(np.sum(w * y * np.exp(-2j * np.pi * f * n / FS))) / np.sum(w))


def find_f0(y: np.ndarray) -> float:
    """The frequency of the strongest component, refined by the projection (the window is 0.7 s: 1.4 Hz bins)."""
    spec = np.abs(np.fft.rfft(y * np.hanning(len(y)), 8 * len(y)))
    f = float(np.argmax(spec[100:])) + 100
    f0 = f * FS / (8 * len(y))
    best = f0
    for step in (0.2, 0.02, 0.002):
        grid = best + step * np.arange(-10, 11)
        best = float(grid[int(np.argmax([amplitude(y, g) for g in grid]))])
    return best


def amount_shown(rig: Any, cfg: dict[str, Any], track: int) -> int:
    """What the send page shows for TRACK (0 ..): the raw halfword of the cell's Sound index ([fx] send_indices) / 256."""
    base = rig.w(cfg["kit_ptr"]) + cfg["sound_off"] + cfg["sound_stride"] * track
    return u16(rig, base + SOUND_ARRAY + 2 * cfg["send_indices"][0]) >> 8


def set_amount(rig: Any, cfg: dict[str, Any], enc: str, track: int, want: int) -> None:
    """Turn ENC until the send knob of TRACK shows WANT: bursts of fast events for the distance, single detents at the end."""
    for _ in range(200):
        got = amount_shown(rig, cfg, track)
        if got == want:
            rig.s.run_ms(40)
            return
        err = want - got
        if abs(err) > 12:
            rig.s.turn_burst(enc, 8 if err > 0 else -8, 2)
        else:
            rig.s.turn_burst(enc, 1 if err > 0 else -1, 1, gap_ms=40)
        rig.s.run_ms(20)
    raise RuntimeError(
        f"turning {enc}: shows {amount_shown(rig, cfg, track)}, wanted {want}"
    )


def model_out(x: np.ndarray, v: float) -> np.ndarray:
    """The float32 model of the routine (`Shifter` of the module's model) on the mono signal X (both channels the same), SHIFT word V, from silent state; the left channel."""
    sh = R.Shifter()
    out = []
    for b in range(0, len(x) // 32 * 32, 32):
        blk = np.concatenate([x[b : b + 32], x[b : b + 32]]).astype(np.float32)
        out.append(sh.block(blk, v)[:32])
    return np.concatenate(out).astype(np.float64)


def dsp_amount(rig: Any, slots: int, row: int) -> list[int]:
    """The 16 amount bytes of the DSP row ROW (words 19 to 22, little endian)."""
    w = chain_rows(rig, slots)[row][W_AMT0 : W_AMT0 + 4]
    return [(w[k // 4] >> (8 * (k % 4))) & 0xFF for k in range(16)]


def load_model(module: str) -> Any:
    """The reference model `freqshift_ref.py` of the module folder, loaded by path."""
    path = modroots.module_dir(module) / "tests" / "freqshift_ref.py"
    spec = importlib.util.spec_from_file_location("freqshift_ref", path)
    assert spec and spec.loader, path
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run_fxshift(c: Any) -> None:
    global FX, R
    from tools.patchkit.pk_digikit_scen import RT, cstr

    t = c.table()
    FX = t["module"]
    R = load_model(FX)
    hint = t.get("hint", 7)
    trig = t.get("trig_frame", M.TRIG_FRAME)
    cfg = profile(c.device)
    img = c.image_for(RT + ",core/dynfx,core/fxpages", "fxshift")
    sine = c.pack(SINE, "m0.dkm")
    fxf = c.pack(FX, "m1.dkm")
    send_slots = pk_device.limits(c.device).send_slots
    image_send = send_slots  # both devices have send cells (finding 64, finding 67)
    slots = pk_device.limits(c.device).fx_slots
    st = c.store
    route_key = b"fx/route/" + FX.encode()

    def seeded(route: bytes) -> Path:
        """A card with the sine and the effect and the settings record (copy A, seq 1) that stores the route of the effect."""
        rec = encode_record(1, [(route_key, route)])
        pad = rec.ljust(((len(rec) + 511) // 512) * 512, b"\0")
        return c.card([sine, fxf], foreign={st.set_a: pad})

    card_sine = c.card([sine])
    # the master proof: the settings store `master` (the manifest says send)
    card_fx = seeded(b"\x00")
    card_send = c.card([sine, fxf])  # the manifest's default route
    pcm: dict[str, np.ndarray] = {}
    raw_pcm: dict[str, np.ndarray] = {}
    exp: dict[str, float] = {}  # the p0 words of the runs (floats)

    def to_main(rig: Any) -> None:
        rig.s.do_tap("NO", 20)
        rig.s.run_ms(300)

    def sound(rig: Any, name: str, again: bool = True) -> None:
        wav = c.work / f"{name}.wav"
        raw_pcm[name] = rig.note(wav, rig.frames() + 300 if again else trig)
        pcm[name] = channels(wav)

    def dry() -> None:
        with c.rig(img, card_sine, "dry") as rig:
            info = rig.boot_info()
            c.check(
                "dry: the sine loaded", info["results"] == [0], f"{info['results']}"
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            to_main(rig)
            sound(rig, "dry", again=False)
            c.done(rig)

    def shift() -> None:
        with c.rig(img, card_fx, "shift") as rig:
            info = rig.boot_info()
            c.check(
                "shift: the sine and the effect loaded",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            shots = Shots(c.work, "fsh")
            plan = rig.plan
            open_send_fx(rig)
            for _ in range(3):
                page_down(rig)
            rig.s.run_ms(600)
            shots.take(rig, "page-default")
            # the page the runtime wrote
            count = rig.w(M.sym(plan, "core/fxpages", "fxp_count"))
            lst = M.sym(plan, "core/fxpages", "fxp_list_buf")
            listed = [rig.w(lst + 4 * i) for i in range(count)]
            row0 = M.sym(plan, "core/fxpages", "fxp_rows")
            first = cfg["page_first"]
            c.check(
                "shift: the FX view has four pages: the three stock pages and the effect's",
                count == 4 and listed == [*STOCK_PAGES[c.device], first],
                f"count {count} {listed}",
            )
            p_first = M.sym(plan, "core/fxpages", "params.fp0.index")
            codes = [rig.w(row0 + 8 + 4 * i) for i in range(9)]
            c.check(
                "shift: the page row has SHIFT at position 0, nothing at 1 to 7 and MIX on the level slot",
                codes == [p_first, 0, 0, 0, 0, 0, 0, 0, p_first + 7],
                f"{codes}",
            )
            c.check(
                "shift: the page's tab is FSH and its title Freq Shifter",
                cstr(rig, rig.w(row0)) == "FSH"
                and cstr(rig, rig.w(row0 + 4)) == "Freq Shifter",
                f"{cstr(rig, rig.w(row0))!r} {cstr(rig, rig.w(row0 + 4))!r}",
            )
            pf = M.sym(plan, "core/params", "pf_table")

            def prow(code: int) -> list[int]:
                return [rig.w(pf + 60 * code + 4 * i) for i in range(15)]

            g, m = prow(p_first), prow(p_first + 7)
            idx = cfg["value_indices"]
            c.check(
                "shift: the SHIFT row is the effect row's value index 0, range 0 .. 0x7f00, default 0x4000 (SHIFT 0, the middle of the bipolar knob); MIX is index 7, default 0x7f00 (127)",
                (g[1], g[2], g[3], g[4]) == (idx[0], 0, 0x7F00, 0x4000)
                and (m[1], m[2], m[3], m[4]) == (idx[7], 0, 0x7F00, 0x7F00),
                f"{g[1:5]} {m[1:5]}",
            )
            c.check(
                "shift: the names are Freq Shift / SHIFT and Mix / MIX",
                (cstr(rig, g[10]), cstr(rig, g[12]), cstr(rig, m[10]), cstr(rig, m[12]))
                == ("Freq Shift", "SHIFT", "Mix", "MIX"),
                f"{cstr(rig, g[10])} {cstr(rig, g[12])} {cstr(rig, m[10])} {cstr(rig, m[12])}",
            )
            kit = Kit(rig, cfg)
            c.check(
                "shift: the kit's value array holds the defaults: SHIFT raw 0x4000, MIX raw 0x7f00 (127)",
                (kit.raw(0, 0), kit.raw(0, 7)) == (0x4000, RAW_MAX),
                f"{kit.raw(0, 0):#x} {kit.raw(0, 7):#x}",
            )
            row = chain_rows(rig, slots)[0]
            c.check(
                "shift: the chain's row has p0 = 0.0 and the mix word 1.0 (MIX 127)",
                row[W_P0] == 0 and row[W_MIX] == ONE,
                f"{row[W_P0]:#x} {row[W_MIX]:#x}",
            )
            c.check(
                "shift: the master route holds: route word 0 and no amounts (stored `master`; the manifest's default is send)",
                row[W_ROUTE : W_AMT0 + 4] == (0,) * 5,
                f"{[hex(x) for x in row[W_ROUTE : W_AMT0 + 4]]}",
            )
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            c.check(
                f"shift: CAPACITY SEND_SLOTS has the image's limit ({image_send}) and none used",
                cap == {"limit": image_send, "used": 0, "largest": 0},
                f"{cap}",
            )

            def words() -> tuple[int, int]:
                r = chain_rows(rig, slots)[0]
                return r[W_P0], r[W_MIX]

            r = Run(c, rig, cfg)

            def to_page() -> None:
                r.fx_page(
                    3
                )  # the FX view keeps its last page: top first, then DEL, REV, CHO, the effect's

            # A: the defaults (SHIFT 0, MIX 127)
            to_main(rig)
            sound(rig, "a")
            # B: MIX to the bottom: the dry sound, bit for bit
            to_page()
            kit.set_raw(ENC_LEVEL, 0, 7, 0x1000)
            rig.s.turn_burst(ENC_LEVEL, -8, 60)  # on to the bottom stop
            rig.s.run_ms(60)
            c.check(
                "shift: MIX turned to the bottom gives raw 0 and a mix word of exactly 0.0",
                kit.raw(0, 7) == 0 and words()[1] == 0,
                f"{kit.raw(0, 7):#x} {words()[1]:#x}",
            )
            shots.take(rig, "page-mix-zero")
            to_main(rig)
            sound(rig, "b")
            # and back to the top
            to_page()
            kit.set_raw(ENC_LEVEL, 0, 7, 0x7000)
            rig.s.turn_burst(ENC_LEVEL, 8, 60)  # on to the top stop
            rig.s.run_ms(60)
            c.check(
                "shift: MIX turned to the top gives raw 0x7f00 and a mix word of exactly 1.0",
                kit.raw(0, 7) == RAW_MAX and words()[1] == ONE,
                f"{kit.raw(0, 7):#x} {words()[1]:#x}",
            )
            shots.take(rig, "page-mix-top")

            def turn_to(raw: int, name: str) -> float:
                to_page()
                got = kit.set_raw(ENC_A, 0, 0, raw)
                rig.s.run_ms(100)
                shots.take(rig, f"page-shift-{name}")
                want = knob_float(LO, HI, got)
                p0, mix = words()
                c.check(
                    f"shift: turning SHIFT to raw {got:#x} puts p0 = {want:.5f} (min + span * raw / 0x7f00 in float32) in the chain's row, the mix stays at 1.0",
                    p0 == bits(want) and mix == ONE,
                    f"p0 {unbits(p0):.6f} want {want:.6f}, mix {mix:#x}",
                )
                exp[name] = want
                return want

            # C: up, D: down
            turn_to(0x6800, "up")
            to_main(rig)
            sound(rig, "c")
            turn_to(0x1800, "down")
            to_main(rig)
            sound(rig, "d")
            # E: save with SHIFT down (A:002), turn it up, load: the saved value is back and the sound moves down again
            c.check(
                "shift: no fault of the effect so far (the row's fault count is 0)",
                chain_rows(rig, slots)[0][W_NFAULT] == 0,
                f"{chain_rows(rig, slots)[0][W_NFAULT]}",
            )
            saved = (kit.raw(0, 0), kit.raw(0, 7))
            to_main(rig)
            r.save_kit(2, "fsh")
            to_page()
            kit.set_raw(ENC_A, 0, 0, 0x6800)
            to_main(rig)
            up_raw = kit.raw(0, 0)
            out = r.load_kit(2, "fsh")
            c.check(
                "shift: Load Kit gives SHIFT and MIX back (the kit array, the DSP row's p0 and mix word, the row's words after the load)",
                (kit.raw(0, 0), kit.raw(0, 7)) == saved
                and up_raw != saved[0]
                and words() == (bits(exp["down"]), ONE),
                f"saved {saved} before {up_raw:#x} after {kit.raw(0, 0):#x} {kit.raw(0, 7):#x} row {words()} want {bits(exp['down']):#x} seen {out['seen'][:4]}",
            )
            exp["e"] = exp["down"]
            to_page()
            rig.s.run_ms(300)
            shots.take(rig, "page-loaded")
            to_main(rig)
            sound(rig, "e")
            c.check(
                "shift: no fault of the effect at the end (the row's fault count is 0)",
                chain_rows(rig, slots)[0][W_NFAULT] == 0,
                f"{chain_rows(rig, slots)[0][W_NFAULT]}",
            )
            c.done(rig)

    mixes: dict[str, float] = {}  # the mix words (floats) of the send runs

    def send() -> None:
        with c.rig(img, card_send, "send") as rig:
            info = rig.boot_info()
            c.check(
                "send: the sine and the effect loaded",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            row = chain_rows(rig, slots)[0]
            c.check(
                "send: the manifest's route holds (no setting): route word 1, the amount words 0, p0 0.0, the mix word 1.0 (MIX 127)",
                row[W_ROUTE] == 1
                and row[W_AMT0 : W_AMT0 + 4] == (0,) * 4
                and row[W_P0] == 0
                and row[W_MIX] == ONE,
                f"{[hex(x) for x in row[W_P0 : W_P0 + 1]]} mix {row[W_MIX]:#x} {[hex(x) for x in row[W_ROUTE : W_AMT0 + 4]]}",
            )
            cap = uploader(rig).capacity()["SEND_SLOTS"]
            cnt = rig.w(M.sym(rig.plan, "core/fxpages", "fxp_send_count"))
            c.check(
                f"send: CAPACITY SEND_SLOTS is {send_slots} with one used; the FX key's count cell is 2 (the send page follows the stock page)",
                cap == {"limit": send_slots, "used": 1, "largest": 0} and cnt == 2,
                f"{cap} count {cnt}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            shots = Shots(c.work, "send")
            r = Run(c, rig, cfg)
            r.tap("NO", 300)
            r.tap("FX", 400)
            r.tap("FX", 400)
            shots.take(rig, "page2")
            set_amount(rig, cfg, ENC_A, 0, AMOUNT)
            shots.take(rig, "turning")
            rig.s.run_ms(300)
            c.check(
                f"send: turning the send knob of track 1 to {AMOUNT} puts the amount {AMOUNT} in the DSP row (the other 15 tracks 0)",
                dsp_amount(rig, slots, 0) == [AMOUNT] + [0] * 15,
                f"{[hex(x) for x in chain_rows(rig, slots)[0][W_AMT0 : W_AMT0 + 4]]}",
            )
            # SHIFT up, on the effect's own page (FUNC + FX)
            r.fx_page(3)
            rig.s.run_ms(300)
            shots.take(rig, "fsh-page")
            got = r.kit.set_raw(ENC_A, 0, 0, SEND_RAW)
            want = knob_float(LO, HI, got)
            exp["send"] = want
            rig.s.run_ms(100)
            row = chain_rows(rig, slots)[0]
            c.check(
                f"send: SHIFT turned to raw {got:#x} puts p0 = {want:.5f} in the row, MIX stays 127 (1.0); route and amount are as before",
                row[W_P0] == bits(want)
                and row[W_MIX] == ONE
                and row[W_ROUTE] == 1
                and dsp_amount(rig, slots, 0)[0] == AMOUNT,
                f"p0 {unbits(row[W_P0]):.6f} want {want:.6f} mix {row[W_MIX]:#x}",
            )
            mixes["send"] = unbits(row[W_MIX])
            r.to_main()
            sound(rig, "send")
            # MIX to about half: the return level
            r.fx_page(3)
            m = r.kit.set_raw(ENC_LEVEL, 0, 7, 0x4000)
            rig.s.run_ms(100)
            mix = unbits(chain_rows(rig, slots)[0][W_MIX])
            mixes["half"] = mix
            c.check(
                f"send: MIX turned to raw {m:#x} puts the mix word {mix:.5f} in the row",
                abs(mix - m / RAW_MAX) < 1e-4,
                f"{mix} {m / RAW_MAX}",
            )
            r.to_main()
            sound(rig, "half")
            c.check(
                "send: no fault of the effect (the row's fault count is 0)",
                chain_rows(rig, slots)[0][W_NFAULT] == 0,
                f"{chain_rows(rig, slots)[0][W_NFAULT]}",
            )
            c.done(rig)

    def other() -> None:
        with c.rig(img, card_send, "other") as rig:
            info = rig.boot_info()
            c.check(
                "other: the sine and the effect loaded",
                info["results"] == [0, 0] and info["safe"] == 0,
                f"{info['results']}",
            )
            rig.pick(rig.dyn_type(SINE, "sine"), hint)
            r = Run(c, rig, cfg)
            r.tap("NO", 300)
            r.tap(["TRK", "TRIG 2"], 400)
            r.tap("FX", 400)
            r.tap("FX", 400)
            set_amount(rig, cfg, ENC_A, 1, 100)
            rig.s.run_ms(300)
            amts = dsp_amount(rig, slots, 0)
            c.check(
                "other: track 2 has the amount 100, track 1 has none",
                amts[1] == 100 and amts[0] == 0 and sum(amts) == 100,
                f"{amts}",
            )
            r.tap(["TRK", "TRIG 1"], 400)
            r.fx_page(3)
            got = r.kit.set_raw(ENC_A, 0, 0, SEND_RAW)
            rig.s.run_ms(100)
            c.check(
                "other: SHIFT is up as in the send run",
                chain_rows(rig, slots)[0][W_P0] == bits(knob_float(LO, HI, got)),
            )
            r.to_main()
            sound(rig, "other", again=True)
            c.done(rig)

    c.parallel(dry, shift, send, other)
    # the sound: the sine of the dry run, then the effect's runs
    win = slice(int(0.3 * FS), int(1.0 * FS))
    y0 = pcm["dry"][win]
    f0 = find_f0(y0)
    a0 = amplitude(y0, f0)
    c.check(
        "dry: the sine is a clean tone (f0 found, amplitude above 0.01)",
        a0 > 0.01 and 20 < f0 < 8000,
        f"f0 {f0:.3f} Hz, amplitude {a0:.4f}",
    )
    rd = scaled(raw_pcm["dry"], raw_pcm["b"], 1.0)
    c.check(
        "shift B: with MIX 0 the PCM is the dry PCM bit for bit from the onset on",
        bool(rd["identical"]) and abs(rd["shift"]) <= 128,
        f"onset shift {rd['shift']}, max err {rd.get('max_abs_err')}",
    )
    ya = pcm["a"][win]
    aa = amplitude(ya, f0)
    rms = float(np.sqrt(np.mean(ya**2)) / np.sqrt(np.mean(y0**2)))
    c.check(
        "shift A: the defaults (MIX 127, SHIFT 0) keep the sine's frequency and level (an allpass pair: amplitude within 0.5 %, RMS within 0.5 %)",
        abs(aa / a0 - 1) < 0.005 and abs(rms - 1) < 0.005,
        f"f0 {f0:.3f}: amplitude ratio {aa / a0:.5f}, rms ratio {rms:.5f}",
    )
    for name, key in (("c", "up"), ("d", "down"), ("e", "e")):
        v = exp[key]
        inc = R.target_inc(v)
        sh = inc * FS / 2**32
        y = pcm[name][win]
        wanted, image, still = (
            amplitude(y, f0 + sh),
            amplitude(y, abs(f0 - sh)),
            amplitude(y, f0),
        )
        db = 20 * np.log10(wanted / max(image, 1e-12))
        c.check(
            f"shift {name.upper()}: SHIFT {v:+.3f} moves the sine from {f0:.2f} Hz by {sh:+.3f} Hz (the routine's mapping: inc {inc}); level within 0.5 %, the image at {abs(f0 - sh):.2f} Hz is {db:.1f} dB down (at least 40), nothing is left at f0",
            abs(wanted / a0 - 1) < 0.005 and db > 40.0 and still / wanted < 0.01,
            f"wanted {wanted:.4f} ({wanted / a0:.4f} of dry), image {image:.5f}, at f0 {still:.5f}, rejection {db:.1f} dB",
        )
    g = (AMOUNT / 127) ** 2
    v = exp["send"]
    inc = R.target_inc(v)
    sh = inc * FS / 2**32
    d_al, _ = aligned(pcm["dry"], pcm["send"])
    # the model: the shifter on the track's bus (the dry sound times the send gain), from silent state at 0.2 s (the filters settle in far less)
    t0, t1, t2 = int(0.2 * FS), int(0.3 * FS), int(1.0 * FS)
    mo = model_out(g * d_al[t0:t2], v)[t1 - t0 :]
    a_mod, i_mod = amplitude(mo, f0 + sh), amplitude(mo, abs(f0 - sh))
    figures = {}
    for tag in ("send", "half"):
        d, o = aligned(pcm["dry"], pcm[tag])
        added = (o - d)[win]
        mix = mixes[tag]
        wanted, image, still = (
            amplitude(added, f0 + sh),
            amplitude(added, abs(f0 - sh)),
            amplitude(added, f0),
        )
        db = 20 * np.log10(wanted / max(image, 1e-12))
        figures[tag] = wanted
        c.check(
            f"send {tag}: the FX return is the shifted track: amount {AMOUNT} (gain {g:.5f}) x MIX {mix:.4f} x the dry amplitude {a0:.4f}, at {f0 + sh:.2f} Hz (f0 {f0:.2f} + {sh:+.3f}); level within 0.5 %, the image at {abs(f0 - sh):.2f} Hz at least 40 dB down, nothing left at f0",
            abs(wanted / (g * mix * a0) - 1) < 0.005
            and db > 40.0
            and still / wanted < 0.01,
            f"wanted {wanted:.6f} ({wanted / (g * mix * a0):.5f} of g x mix x dry), image {image:.7f}, at f0 {still:.7f}, rejection {db:.1f} dB",
        )
        c.check(
            f"send {tag}: the float32 model (`Shifter` of the module's model on the dry sound times {g:.5f}) agrees: level of the shifted tone within 0.5 % of the model's x MIX, image level within 25 % of the model's",
            abs(wanted / (a_mod * mix) - 1) < 0.005
            and abs(image / (i_mod * mix) - 1) < 0.25,
            f"model {a_mod:.6f} (measured / model x mix {wanted / (a_mod * mix):.5f}), image {i_mod:.7f} (measured / model x mix {image / (i_mod * mix):.3f})",
        )
    r = figures["half"] / figures["send"]
    want = mixes["half"] / mixes["send"]
    c.check(
        f"send: MIX at {mixes['half']:.4f} halves the return: the added level is {want:.5f} of MIX 127's",
        abs(r / want - 1) < 0.005,
        f"{r:.5f} against {want:.5f}",
    )
    c.check(
        "send: the PCM differs from the dry PCM (the return is audible)",
        not bool(scaled(raw_pcm["dry"], raw_pcm["send"], 1.0)["identical"]),
        "",
    )
    ro = scaled(raw_pcm["dry"], raw_pcm["other"], 1.0)
    c.check(
        "other: the amount on track 2 only (silent) adds nothing to track 1: the PCM is the dry PCM bit for bit from the onset on, the shifter running on an empty bus",
        bool(ro["identical"]) and abs(ro["shift"]) <= 128,
        f"onset shift {ro['shift']}, max err {ro.get('max_abs_err')}",
    )
    # the send amounts reach the settings record (the two copies; the send boots turn knobs)
    c.common_checks(KIT_AREAS[c.device], store_extra=frozenset({st.set_a, st.set_b}))
