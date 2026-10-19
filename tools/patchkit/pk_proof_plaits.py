#!/usr/bin/env python3
"""patchkit proof: the PLAITS machine (models 0..7: VA, ASYM-TRI, 2FM, FORMANT, ADDITIVE, WAVETABLE, CHORD, SPEECH; 12 RINGS, 21 TERRAIN) end to end on Digitakt II 1.17,
in the emulator.

    uv run python -m tools.patchkit.pk_proof_plaits [--image OUT.syx] [--out DIR] [--only a,b,c,d,e,g,h,i,j,k] [--jobs N]

Loads the owner image (default out/patch/plaits-10models-names-splash-48k-1.17.syx, gated; its plan next to it), drives coupled daemons
through the harness (`tools/patchkit/pk_harness.py` Session: panel inputs, health asserted after every input) and
captures the main output (`pcm.tap`):

  a  pitch     FUNC+SRC, 7 x DOWN (the 8th entry), YES commits type 11 (machines/plaits:plaits); a trig brings the
               type to the DSP (type cache DM 0x255970); note 60 held 1 s: the fundamental is within 1 cent of
               261.6256 Hz, and the track's ACTIVE byte stays 1 after the note ends (docs/findings/06).
  b  knobs     HARMONICS (C), TIMBRE (E) and MORPH (F) turned on the SRC page change their engine slot, the TX frame word
               and the spectrum of the note (harmonic magnitudes against the default note).
  c  stock     LEV (H) lowered acts through the level stage (a square) on PLAITS as on a stock sample; the filter (FLTR page, E) closed
               cuts the high band; the reverb send (FX page, F) adds the reverb's stereo return.
  d  two       PLAITS on tracks 1 and 2 (track 2 TUNE + 7): both notes sound together, at 261.6 and 392.0 Hz.
  e  persist   save the project with edited PLAITS values (MODEL 1 among them), set track 1 to ONESHOT, load: type 11
               and the values are back, MODEL included, and the track plays model 1 at note 60. The card's writes over the STOCK 1.17 card: the project loads without a fault and track 1 comes
               back as type 0 (ONESHOT): the stock loader keeps a type only when `(u8)(type + 1) < 8` (finding 02).
  h  asym-tri  MODEL (B) turned to 1 (row value 0x100): slot and TX word; note 60 of model 1 within 1 cent; its
               spectrum differs from model 0's; HARMONICS, TIMBRE and MORPH change model 1's spectrum; track 2's stock
               sample (ONESHOT) still plays beside it.
  i  models    MODEL (B) turned to 2..7 (2FM, FORMANT, ADDITIVE, WAVETABLE, CHORD, SPEECH): slot and TX word; note 60
               of each within 1 cent (2FM with HARMONICS at 0x3c00, a ratio of exactly one octave, so the carrier is the
               fundamental; CHORD with HARMONICS, TIMBRE and MORPH 0, the OCT chord in root position on the divide-down
               voices, at upstream's 0.998 detune, 261.1024 Hz; SPEECH with HARMONICS 0, the naive formant synth, a
               sustained vowel, kept for its spectrum and knob checks); each spectrum differs from model 0's;
               HARMONICS, TIMBRE and MORPH change each one's spectrum (SPEECH's HARMONICS turn reaches the word banks).
  j  persist4  as e with MODEL 4 (ADDITIVE, TIMBRE 0x800 so the fundamental leads): saved, track set to ONESHOT,
               reloaded: MODEL is back at 0x400 and the track plays model 4 at note 60 within 1 cent (no stock-card
               step: e covers it).
  k  sequence  KNOWN BROKEN, run only by --only k: pattern A02, PLAITS on track 1, trigs on the odd steps, MODEL
               p-locked to 1, 2, 3, 4 on steps 3, 5, 7, 9; PLAY: notes every 0.25 s over two passes; save, load, PLAY.
  l  control   KNOWN BROKEN, run only by --only l: k on stock 1.17 with track 1's stock machine. It fails the same way
               (one burst of notes, no second pass): the harness's PLAY does not drive the sequencer in this setup.
  g  gain      the gain calibration: track 1's stock sample (its int16 data read from DSP memory for its peak) and then
               PLAITS default VA at note 60 on the same track and LEV: the RMS of the sample normalised to full scale
               against PLAITS' (Task 6 report, "Gain calibration").

Output (WAV, card images, results.json) goes under OUT (default out/runs/plaits-proof) and is never committed. Exit 0
only when every check passes. Addresses are the DT2 1.17 ones of findings 02, 04 and 06.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from pathlib import Path
from typing import Any

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.emulator import card_overlay  # noqa: E402
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proof_machines as M  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402

PLAITS_TYPE = 11
PLAITS_DOWNS = 7  # the 8th entry of FUNC+SRC (types 7..10 are gap rows in this image)
MODS = ["machines/plaits-48k"]
TYPE_CACHE = 0x255970
RECORDS, REC = 0x2412CC, 0x1D8
PITCH_WORD = 0x80005348
FS = 48000.0
NOTE60 = 261.6256
HARM, TIMB, MORPH, LEV = (
    27,
    31,
    32,
    34,
)  # engine slots (mirror indexes) of knobs C, E, F, H
ENC = {HARM: "ENC C", TIMB: "ENC E", MORPH: "ENC F", LEV: "ENC H"}
MODEL = 26  # engine slot of knob B (MODEL: the model number << 8)
DEFAULT_IMAGE = ROOT / "out" / "patch" / "plaits-10models-names-splash-48k-1.17.syx"
NEW_MODELS = {
    2: "2FM",
    3: "FORMANT",
    4: "ADDITIVE",
    5: "WAVETABLE",
    6: "CHORD",
    7: "SPEECH",
    12: "RINGS",
    21: "TERRAIN",
}
# CHORD detunes its voices by 0.998 (upstream ChordEngine: f0 = NoteToFrequency(note) * 0.998)
CHORD_DETUNE = 0.998


def left_of(pcm: np.ndarray) -> np.ndarray:
    return pcm[0::2].astype(np.float64)


def onset(x: np.ndarray, thresh: float = 1e-4) -> int:
    idx = np.flatnonzero(np.abs(x) > thresh)
    return int(idx[0]) if len(idx) else -1


def f0_fft(x: np.ndarray, guess: float) -> float:
    """The strongest peak within +-20 % of GUESS: Hann window, 8x zero padding, parabolic interpolation in log."""
    x = x - x.mean()
    n = len(x)
    pad = 8 * n
    sp = np.abs(np.fft.rfft(x * np.hanning(n), pad))
    fr = np.fft.rfftfreq(pad, 1 / FS)
    lo, hi = np.searchsorted(fr, guess * 0.8), np.searchsorted(fr, guess * 1.2)
    i = lo + int(np.argmax(sp[lo:hi]))
    a, b, c = (math.log(sp[j] + 1e-30) for j in (i - 1, i, i + 1))
    return float((i + 0.5 * (a - c) / (a - 2 * b + c)) * FS / pad)


def harmonics_db(x: np.ndarray, f0: float, k: int = 12) -> np.ndarray:
    """Magnitudes (dB, relative to the strongest) of harmonics 1..k of f0."""
    n = len(x)
    sp = np.abs(np.fft.rfft((x - x.mean()) * np.hanning(n)))
    fr = np.fft.rfftfreq(n, 1 / FS)
    mags = []
    for h in range(1, k + 1):
        j = int(np.argmin(np.abs(fr - h * f0)))
        mags.append(float(np.max(sp[max(0, j - 2) : j + 3])))
    m = np.array(mags) + 1e-12
    return 20 * np.log10(m / m.max())


def rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(x**2))) if len(x) else 0.0


def band_db(x: np.ndarray, lo: float, hi: float) -> float:
    sp = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    fr = np.fft.rfftfreq(len(x), 1 / FS)
    return float(10 * math.log10(np.sum(sp[(fr >= lo) & (fr < hi)]) + 1e-30))


class PlaitsProof(M.Proof):
    tool = "plaits"
    image: Path = DEFAULT_IMAGE

    def plaits(self, s: H.Session) -> None:
        """A fresh start with PLAITS selected on track 1 and the SRC page open."""
        self.fresh(s)
        self.select(s, PLAITS_DOWNS)
        s.do_tap("SRC")
        s.run_ms(300)

    def play(
        self,
        s: H.Session,
        name: str,
        keys: tuple[str, ...] = ("TRIG 1",),
        hold: int = 600,
        total: int = 900,
    ) -> np.ndarray:
        """Hold KEYS together with PCM capture; the left channel."""
        assert s.emu is not None
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        for k in keys:
            s.emu.button(H.button(k), True)
        s.run_ms(hold)
        for k in keys:
            s.emu.button(H.button(k), False)
        s.run_ms(total - hold)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"play {name}")
        return left_of(hook_ab.read_wav(wav))

    def held(self, x: np.ndarray, start_ms: int = 60, ms: int = 450) -> np.ndarray:
        a = onset(x)
        if a < 0:
            return x[:0]
        a += int(start_ms * FS / 1000)
        return x[a : a + int(ms * FS / 1000)]

    def set_model(self, s: H.Session, n: int, track: int = 0) -> int:
        """Turn MODEL (knob B) one detent at a time until the slot holds n << 8; returns the slot."""
        for _ in range(120):
            v = s.mirror(MODEL, track)
            if v == n << 8:
                break
            s.turn_burst("ENC B", 1 if v < n << 8 else -1, 1, gap_ms=60)
        return s.mirror(MODEL, track)

    def pitch_check(self, x: np.ndarray, what: str, want: float = NOTE60) -> None:
        seg = self.held(x, 100, 1000)
        self.check(f"{what} sounds", rms(seg) > 1e-3, f"rms {rms(seg):.4f}")
        if rms(seg) > 1e-3:
            f = f0_fft(seg, want)
            c = 1200 * math.log2(f / want)
            self.results_extra[f"{what} Hz"] = f
            self.check(
                f"{what} within 1 cent of {want:.4f} Hz",
                abs(c) < 1.0,
                f"{f:.4f} Hz, {c:+.3f} cent",
            )

    def knobs_change(self, s: H.Session, ref: np.ndarray, tag: str) -> None:
        """HARMONICS, TIMBRE and MORPH each turned: the slot and TX word follow, the spectrum changes."""
        href = harmonics_db(ref, NOTE60)
        for idx, name, target in (
            (HARM, "HARMONICS", 0x7000),
            (TIMB, "TIMBRE", 0x7800),
            (MORPH, "MORPH", 0x0800),
        ):
            before = s.mirror(idx)
            s.turn_to(ENC[idx], idx, target)
            after = s.mirror(idx)
            x = self.held(self.play(s, f"{tag}{name.lower()}"))
            tx = s.tx_word(idx)
            h = harmonics_db(x, f0_fft(x, NOTE60) if rms(x) > 1e-3 else NOTE60)
            d = float(np.mean(np.abs(h[:10] - href[:10])))
            self.check(
                f"{tag}{name} moves its slot and the TX word",
                abs(after - target) <= 0x300 and abs(tx - after) <= 0x300,
                f"slot {before:#x} -> {after:#x}, tx {tx:#x}",
            )
            self.check(
                f"{tag}{name} changes the spectrum",
                d > 1.5,
                f"mean |d harmonic| {d:.2f} dB",
            )
            s.turn_to(ENC[idx], idx, before)

    def type_cache(self, s: H.Session) -> int:
        assert s.emu is not None
        raw = s.emu.dsp_peek(TYPE_CACHE, 4)["hex"]
        return int.from_bytes(bytes.fromhex(raw), "little")

    def session_plaits(self, tag: str = "", pool: bool = True) -> K.Lease:
        return self.session(MODS, tag, pool=pool, image=self.image)

    # -- a: select, type, pitch, ACTIVE
    def part_a(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            self.check(
                "FUNC+SRC 8th entry commits type 11",
                self.type_now(s) == PLAITS_TYPE,
                f"{self.type_now(s)}",
            )
            x = self.play(s, "note60", hold=1200, total=1500)
            tc = self.type_cache(s)
            self.check(
                "the trig brings type 11 to the DSP",
                tc & 0xFFFF == PLAITS_TYPE,
                f"{tc:#010x}",
            )
            seg = self.held(x, 100, 1000)
            self.check("note 60 sounds", rms(seg) > 1e-3, f"rms {rms(seg):.4f}")
            if rms(seg) > 1e-3:
                f = f0_fft(seg, NOTE60)
                c = 1200 * math.log2(f / NOTE60)
                self.results_extra["note60_hz"] = f
                self.check(
                    "note 60 within 1 cent of 261.6256 Hz",
                    abs(c) < 1.0,
                    f"{f:.4f} Hz, {c:+.3f} cent",
                )
            assert s.emu is not None
            act = [
                bytes.fromhex(s.emu.dsp_peek(RECORDS + v * REC + 0x1B8, 4)["hex"])[0]
                for v in (0, 1)
            ]
            self.results_extra["active_after_note"] = act
            self.check(
                "ACTIVE stays set after the note under PLAITS (never cleared)",
                act == [1, 1],
                f"{act}",
            )

    # -- b: knobs change the spectrum
    def part_b(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            ref = self.held(self.play(s, "default"))
            self.knobs_change(s, ref, "")

    # -- h: model 1 (ASYM-TRI)
    def part_h(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            ref0 = self.held(self.play(s, "model0"))
            m = self.set_model(s, 1)
            s.run_ms(100)
            tx = s.tx_word(MODEL)
            self.check(
                "MODEL knob reaches 1 (row value 0x100): slot and TX word",
                m == 0x100 and tx == 0x100,
                f"slot {m:#x}, tx {tx:#x}",
            )
            x = self.play(s, "model1-note60", hold=1200, total=1500)
            tc = self.type_cache(s)
            self.check("type 11 at the DSP", tc & 0xFFFF == PLAITS_TYPE, f"{tc:#010x}")
            self.pitch_check(x, "model 1 (ASYM-TRI) note 60")
            ref1 = self.held(self.play(s, "model1"))
            d = float(
                np.mean(
                    np.abs(
                        harmonics_db(ref1, NOTE60)[:10]
                        - harmonics_db(ref0, NOTE60)[:10]
                    )
                )
            )
            self.check(
                "model 1's spectrum differs from model 0's",
                d > 1.5,
                f"mean |d harmonic| {d:.2f} dB",
            )
            self.knobs_change(s, ref1, "model 1 ")
            # track 2: the default project's stock sample (ONESHOT), beside PLAITS model 1 on track 1
            x2 = self.held(
                self.quiet_play(s, "track2-oneshot", keys=("TRIG 2",)), 5, 200
            )
            tc = self.type_cache(s)
            self.check(
                "track 2 ONESHOT still plays beside PLAITS model 1",
                rms(x2) > 1e-3 and tc >> 16 == 0,
                f"rms {rms(x2):.4f}, type cache {tc:#010x}",
            )

    # -- i: models 2..7 (2FM, FORMANT, ADDITIVE, WAVETABLE, CHORD, SPEECH)
    def part_i(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            ref0 = self.held(self.play(s, "model0"))
            # PLAITS_PROOF_MODELS=12,21: only those models (a bounded run)
            only = {
                int(x)
                for x in os.environ.get("PLAITS_PROOF_MODELS", "").split(",")
                if x
            }
            for model, name in NEW_MODELS.items():
                if only and model not in only:
                    continue
                m = self.set_model(s, model)
                s.run_ms(100)
                tx = s.tx_word(MODEL)
                self.check(
                    f"MODEL knob reaches {model} (row value {model << 8:#x}): slot and TX word",
                    m == model << 8 and tx == model << 8,
                    f"slot {m:#x}, tx {tx:#x}",
                )
                harm, timb, morph = s.mirror(HARM), s.mirror(TIMB), s.mirror(MORPH)
                if (
                    model == 2
                ):  # a modulator ratio of exactly one octave (lut_fm_frequency_quantizer 60..62)
                    s.turn_to(ENC[HARM], HARM, 0x3C00, tol=0x80)
                if (
                    model == 6
                ):  # the OCT chord (HARMONICS 0) in root position (TIMBRE 0) on the divide-down voices (MORPH 0)
                    s.turn_to(ENC[HARM], HARM, 0)
                    s.turn_to(ENC[TIMB], TIMB, 0)
                    s.turn_to(ENC[MORPH], MORPH, 0)
                if (
                    model == 7
                ):  # the naive formant synth (HARMONICS 0): a sustained vowel at the note (the words play once per trig)
                    s.turn_to(ENC[HARM], HARM, 0)
                if (
                    model == 12
                ):  # HARMONICS 0x2600 (structure 0.3: stiffness 0, the modes are harmonics of the note)
                    s.turn_to(ENC[HARM], HARM, 0x2600, tol=0x80)
                x = self.play(s, f"model{model}-note60", hold=1200, total=1500)
                tc = self.type_cache(s)
                self.check(
                    f"model {model}: type 11 at the DSP",
                    tc & 0xFFFF == PLAITS_TYPE,
                    f"{tc:#010x}",
                )
                self.pitch_check(
                    x,
                    f"model {model} ({name}) note 60",
                    NOTE60 * CHORD_DETUNE if model == 6 else NOTE60,
                )
                if model in (2, 6, 12):
                    s.turn_to(ENC[HARM], HARM, harm)
                if model == 6:
                    s.turn_to(ENC[TIMB], TIMB, timb)
                    s.turn_to(ENC[MORPH], MORPH, morph)
                ref = self.held(self.play(s, f"model{model}"))
                d = float(
                    np.mean(
                        np.abs(
                            harmonics_db(ref, NOTE60)[:10]
                            - harmonics_db(ref0, NOTE60)[:10]
                        )
                    )
                )
                self.check(
                    f"model {model}'s spectrum differs from model 0's",
                    d > 1.5,
                    f"mean |d harmonic| {d:.2f} dB",
                )
                self.knobs_change(s, ref, f"model {model} ")
                if model == 7:
                    s.turn_to(ENC[HARM], HARM, harm)

    # -- k: the sequencer plays PLAITS steps with MODEL p-locks, before and after a save and load
    SEQ_STEPS = (1, 3, 5, 7, 9, 11, 13, 15)
    SEQ_LOCKS = {
        3: 1,
        5: 2,
        7: 3,
        9: 4,
    }  # step -> p-locked MODEL (the others play the track's MODEL 0)

    def seq_play(self, s: H.Session, name: str, ms: int = 4600) -> np.ndarray:
        assert s.emu is not None
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.do_tap("PLAY")
        s.run_ms(ms)
        s.emu.call("pcm.tap", action="capture_stop")
        s.do_tap("STOP")
        s.run_ms(500)
        s.health(f"sequence {name}")
        return left_of(hook_ab.read_wav(wav))

    def note_starts(self, x: np.ndarray) -> list[float]:
        """Seconds of each note start: the 5 ms RMS rises above half its maximum after falling below a tenth."""
        w = int(FS / 200)
        n = len(x) // w
        if not n:
            return []
        env = np.sqrt(np.mean(x[: n * w].reshape(n, w) ** 2, axis=1))
        top = float(env.max())
        out: list[float] = []
        armed = True
        for i, e in enumerate(env):
            if armed and e > 0.5 * top:
                out.append(i * w / FS)
                armed = False
            elif e < 0.1 * top:
                armed = True
        return out

    def seq_check(self, x: np.ndarray, tag: str) -> None:
        starts = self.note_starts(x)
        self.results_extra[f"{tag} note starts s"] = [round(t, 3) for t in starts]
        gaps = np.diff(starts) if len(starts) > 1 else np.array([])
        # 120 BPM: a trig every 2 steps is 0.25 s; at least two passes of the 16 steps in the window
        steady = int(np.sum(np.abs(gaps - 0.25) < 0.02))
        self.check(
            f"{tag}: the sequencer advances (notes 0.25 s apart over two passes)",
            len(starts) >= 12 and steady >= 10,
            f"{len(starts)} note starts, {steady} gaps of 0.25 s: {[round(t, 2) for t in starts[:20]]}",
        )

    def part_k(self) -> None:
        with self.session_plaits("seq", pool=False) as s:
            self.sequence_k(s, plaits=True)

    def part_l(self) -> None:
        """The control of k: the same steps on stock 1.17 with track 1's stock machine (no MODEL p-locks)."""
        with self.session([], "seq-stock", pool=False) as s:
            self.sequence_k(s, plaits=False)

    def sequence_k(self, s: H.Session, plaits: bool) -> None:
        if True:
            self.fresh(s)
            s.do_tap(["PTN", "TRIG 2"])  # pattern A02: empty in the default project
            s.run_ms(500)
            if plaits:
                self.select(s, PLAITS_DOWNS)
                s.do_tap("SRC")
                s.run_ms(300)
                m = self.set_model(s, 0)
                self.check("track MODEL 0", m == 0, f"{m:#x}")
            assert s.emu is not None
            s.do_tap("RECORD")
            s.run_ms(200)
            for k in self.SEQ_STEPS:
                s.do_tap(f"TRIG {k}")
                s.run_ms(150)
            for k, model in self.SEQ_LOCKS.items() if plaits else ():
                s.emu.button(H.button(f"TRIG {k}"), True)
                s.run_ms(150)
                for _ in range(model):
                    s.turn_burst("ENC B", 1, 1, gap_ms=80)
                s.run_ms(150)
                s.emu.button(H.button(f"TRIG {k}"), False)
                s.run_ms(200)
            s.do_tap("RECORD")
            s.run_ms(300)
            if plaits:
                self.check(
                    "the track's MODEL is still 0 after the p-locks",
                    s.mirror(MODEL) == 0,
                    f"{s.mirror(MODEL):#x}",
                )
            s.run_ms(1500)
            self.seq_check(self.seq_play(s, "seq-live"), "live pattern")
            self.save_project(s)
            if plaits:
                self.load_project(s, lambda: self.type_now(s) == PLAITS_TYPE)
            else:
                self.load_project(s)
            s.run_ms(1500)
            self.seq_check(self.seq_play(s, "seq-reloaded"), "after save and load")

    # -- c: LEV, filter, reverb send act as on a stock track
    def quiet_play(self, s: H.Session, name: str, **kw: Any) -> np.ndarray:
        """`play` after 2.5 s without a note: the default amp release of the last note has ended."""
        s.run_ms(2500)
        return self.play(s, name, **kw)

    def part_c(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            # LEV: the level stage is a square (finding 06): 20 log10 of the RMS ratio is 40 log10 of the value ratio
            full = rms(self.held(self.quiet_play(s, "lev-full")))
            lev = s.mirror(LEV)
            got = s.turn_to(ENC[LEV], LEV, lev // 2)
            half = rms(self.held(self.quiet_play(s, "lev-half")))
            d_pl = 20 * math.log10(max(half, 1e-9) / max(full, 1e-9))
            want_pl = 40 * math.log10(got / lev)
            s.turn_to(ENC[LEV], LEV, lev)
            s.do_tap(
                ["TRK", "TRIG 2"]
            )  # track 2: a stock sample (ONESHOT in the default project)
            s.run_ms(200)
            s.do_tap("SRC")
            s.run_ms(200)
            sfull = rms(
                self.held(
                    self.quiet_play(s, "stock-lev-full", keys=("TRIG 2",)), 5, 200
                )
            )
            slev = s.mirror(LEV, 1)
            sgot = s.turn_to(ENC[LEV], LEV, slev // 2, track=1)
            shalf = rms(
                self.held(
                    self.quiet_play(s, "stock-lev-half", keys=("TRIG 2",)), 5, 200
                )
            )
            d_st = 20 * math.log10(max(shalf, 1e-9) / max(sfull, 1e-9))
            want_st = 40 * math.log10(sgot / slev)
            self.check(
                "LEV lowered: PLAITS follows the level stage as a stock sample does",
                abs(d_pl - want_pl) < 0.5 and abs(d_st - want_st) < 0.5,
                f"PLAITS {d_pl:.2f} dB (LEV {lev:#x} -> {got:#x}: {want_pl:.2f}), "
                f"stock {d_st:.2f} dB ({slev:#x} -> {sgot:#x}: {want_st:.2f})",
            )
            s.do_tap(["TRK", "TRIG 1"])
            s.run_ms(200)
            # filter: FLTR page, E = FREQ (engine slot 36), from open to 0x1000
            open_ = self.held(self.quiet_play(s, "flt-open"))
            s.do_tap("FLTR")
            s.run_ms(200)
            freq = s.turn_to("ENC E", 36, 0x1000, tol=0x400)
            closed = self.held(self.quiet_play(s, "flt-closed"))
            hf = band_db(closed, 2000, 16000) - band_db(open_, 2000, 16000)
            self.check(
                "the filter closes on PLAITS (2-16 kHz)",
                hf < -12,
                f"FREQ {freq:#x}: {hf:.1f} dB",
            )
            s.turn_to("ENC E", 36, 0x7F00, tol=0x400)

            # reverb: FX page, F = REV (Reverb Send, engine slot 56). PLAITS' two voices carry the same block, so the
            # dry output has no side signal (L - R); the reverb's stereo return has.
            def side(name: str) -> float:
                wav = self.out / f"{name}.wav"
                pcm = hook_ab.read_wav(wav).astype(np.float64)
                return rms(pcm[0::2] - pcm[1::2])

            self.quiet_play(s, "rev-dry", hold=300, total=1500)
            s.do_tap("FX")
            s.run_ms(200)
            rev = s.turn_to("ENC F", 56, 0x7000, tol=0x400)
            self.quiet_play(s, "rev-send", hold=300, total=1500)
            sd, sw = side("rev-dry"), side("rev-send")
            self.check(
                "the reverb send adds the reverb's (stereo) return",
                sw > 1e-3 and sw > 30 * max(sd, 1e-7),
                f"REV {rev:#x}: side rms dry {sd:.2e}, send {sw:.2e}",
            )

    # -- d: two tracks at once
    def part_d(self) -> None:
        with self.session_plaits() as s:
            self.plaits(s)
            s.do_tap(["TRK", "TRIG 2"])
            s.run_ms(200)
            self.select(s, PLAITS_DOWNS)  # track 2 is ONESHOT too: the same 7 x DOWN
            s.do_tap("SRC")
            s.run_ms(200)
            s.do_hold(
                "TRIG 2", 40
            )  # the first trig loads the machine's defaults into the slots
            s.run_ms(300)
            s.turn_to("ENC A", 25, 0x4700, tol=0x40, track=1)  # TUNE + 7 semitones
            s.do_tap(["TRK", "TRIG 1"])
            s.run_ms(200)
            x = self.held(
                self.play(s, "two-tracks", ("TRIG 1", "TRIG 2"), hold=900, total=1100),
                100,
                700,
            )
            tc = self.type_cache(s)
            self.check(
                "both tracks are PLAITS at the DSP",
                tc == (PLAITS_TYPE | PLAITS_TYPE << 16),
                f"{tc:#010x}",
            )
            ok = rms(x) > 1e-3
            tune2 = s.mirror(25, 1)
            want2 = NOTE60 * 2 ** (
                (tune2 - 0x4000) / 256 / 12
            )  # track 2's TUNE as the knob left it
            f1 = f0_fft(x, NOTE60) if ok else 0.0
            f2 = f0_fft(x, want2) if ok else 0.0
            c1 = 1200 * math.log2(max(f1, 1) / NOTE60)
            c2 = 1200 * math.log2(max(f2, 1) / want2)
            self.check(
                "both notes sound together (track 1 note 60, track 2 note 60 + its TUNE)",
                ok and abs(c1) < 5 and abs(c2) < 5,
                f"{f1:.2f} Hz ({c1:+.2f} cent), {f2:.2f} Hz against {want2:.2f} (TUNE {tune2:#x}, {c2:+.2f} cent)",
            )

    # -- e: persistence, then the saved project on stock
    def part_e(self) -> None:
        self.persist(1)

    def part_j(self) -> None:
        self.persist(4)

    def persist(self, model: int) -> None:
        """Save with MODEL = model and edited values, ONESHOT, reload; for model 1 also the stock-card load."""
        overlay = self.out / f"card-m{model}.overlay"
        # ADDITIVE: TIMBRE low (the most prominent harmonic is the fundamental), so the pitch check has one
        want = {HARM: 0x1800, TIMB: 0x6000 if model == 1 else 0x0800, MORPH: 0x2400}
        with self.session_plaits(f"persist{model}", pool=False) as s:
            self.plaits(s)
            s.do_hold("TRIG 1", 100)
            s.run_ms(200)
            for idx, v in want.items():
                s.turn_to(ENC[idx], idx, v)
            m = self.set_model(s, model)
            self.check(
                f"MODEL set to {model} before the save", m == model << 8, f"{m:#x}"
            )
            keep = {i: s.mirror(i) for i in (25, 26, 27, 31, 32, 33, 34)}
            self.save_project(s)
            self.select(s, 0, ups=PLAITS_DOWNS + 2)  # ONESHOT
            self.check(
                "track 1 set to ONESHOT", self.type_now(s) == 0, f"{self.type_now(s)}"
            )
            self.load_project(s, lambda: self.type_now(s) == PLAITS_TYPE)
            self.check(
                "project reload brings type 11 back",
                self.type_now(s) == PLAITS_TYPE,
                f"{self.type_now(s)}",
            )
            for _ in range(8):
                s.do_hold("TRIG 1", 40)
                s.run_ms(260)
                if {i: s.mirror(i) for i in keep} == keep:
                    break
            back = {i: s.mirror(i) for i in keep}
            self.check(
                "and the PLAITS values",
                back == keep,
                f"{ {i: hex(v) for i, v in back.items()} }",
            )
            self.check(
                f"MODEL is back at {model} after the reload",
                back.get(MODEL) == model << 8,
                f"{back.get(MODEL, 0):#x}",
            )
            s.run_ms(2500)
            self.pitch_check(
                self.play(s, f"reloaded-m{model}", hold=1200, total=1500),
                f"the reloaded track (model {model}) note 60",
            )
            if model != 1:
                return
            assert s.emu is not None
            r = s.emu.call("card.overlay", path=str(overlay))
            self.check(
                "card writes exported", r["sectors"] > 0, f"{r['sectors']} sectors"
            )
            card = s.card
        types = card_overlay.sound_types(overlay.read_bytes())
        self.check(
            "the saved project stores type 11",
            types.get(PLAITS_TYPE, 0) > 0,
            f"{types}",
        )
        base = Path(card or "")
        out_img = self.out / "card-after-save.img"
        if not base.is_file():
            self.check("card image found", False, str(base))
            return
        n = card_overlay.apply(base, overlay, out_img)
        self.check("card image with the saved project written", n > 0, f"{n} sectors")
        with self.session([], "saved-card", pool=False) as s:
            assert s.emu is not None
            s.card = str(out_img)
            s.load(s.syx, settle=False)
            s.emu.drain_events()
            s.run_ms(300)
            s.emu.pcm_start()
            s.pcm_mark = 0
            s.health("stock with the saved card")
            self.load_project(s)
            t = self.type_now(s)
            self.check(
                "STOCK 1.17 loads the project without a fault; track 1 falls back to type 0 (the loader's bound)",
                t == 0,
                f"type {t}",
            )
            s.run_ms(500)
            s.do_hold("TRIG 1", 300)
            s.run_ms(300)
            s.health("stock after loading the patched project")

    # -- g: gain calibration
    def part_g(self) -> None:
        with self.session_plaits() as s:
            self.fresh(s)
            assert s.emu is not None
            # track 1's stock sample (ONESHOT in the default project): output, and its data from DSP memory
            xs = self.play(s, "stock-sample", hold=1500, total=1700)
            rec = RECORDS
            ptr = int.from_bytes(bytes.fromhex(s.emu.dsp_peek(rec, 4)["hex"]), "little")
            frames = int.from_bytes(
                bytes.fromhex(s.emu.dsp_peek(rec + 0x188, 4)["hex"]), "little"
            )
            n = min(frames, 48000)
            raw = b""
            for off in range(0, 2 * n, 1024):
                h = s.emu.dsp_peek(ptr + off, min(1024, 2 * n - off))["hex"]
                raw += bytes.fromhex(h.replace("??", "00"))
            data = np.frombuffer(raw, "<i2").astype(np.float64) / 32768.0
            a = onset(xs)
            seg = xs[a : a + len(data)] if a >= 0 else xs[:0]
            peak_out, peak_data = (
                float(np.max(np.abs(seg))) if len(seg) else 0.0,
                float(np.max(np.abs(data))),
            )
            gain_chain = peak_out / max(peak_data, 1e-9)
            target = rms(data) / max(
                peak_data, 1e-9
            )  # the sample's RMS at full scale (peak 1.0)
            # PLAITS default VA, note 60, the same track and LEV
            self.select(s, PLAITS_DOWNS)
            xp = self.held(
                self.play(s, "plaits-default", hold=1000, total=1200), 100, 800
            )
            voice_rms = rms(xp) / max(gain_chain, 1e-9)
            self.results_extra.update(
                {
                    "sample_frames": frames,
                    "sample_peak": peak_data,
                    "sample_rms_fullscale": target,
                    "chain_gain": gain_chain,
                    "plaits_out_rms": rms(xp),
                    "plaits_voice_rms": voice_rms,
                    "gain_for_match": target / max(voice_rms, 1e-9),
                }
            )
            self.check(
                "gain measured",
                peak_data > 0.01 and voice_rms > 1e-3,
                json.dumps(
                    {
                        k: round(float(v), 5)
                        for k, v in self.results_extra.items()
                        if isinstance(v, float)
                    }
                ),
            )

    results_extra: dict[str, Any] = {}

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.results_extra = {}

    def check(self, name: str, ok: bool, detail: object = "") -> bool:
        r = super().check(name, ok, detail)
        (self.out / "extra.json").write_text(
            json.dumps(self.results_extra, indent=1, default=str)
        )
        return r


P = PlaitsProof
PARTS = [
    K.Part(
        "a",
        "select, type at the DSP, note 60 within 1 cent, ACTIVE after the note",
        P.part_a,
        True,
        150,
    ),
    K.Part("b", "HARMONICS, TIMBRE, MORPH change the spectrum", P.part_b, True, 300),
    K.Part("c", "LEV, filter and reverb send as on a stock track", P.part_c, True, 400),
    K.Part("d", "PLAITS on tracks 1 and 2 at once", P.part_d, True, 200),
    K.Part(
        "e", "persistence (patched save and load, then stock load)", P.part_e, True, 250
    ),
    K.Part("g", "gain calibration against track 1's stock sample", P.part_g, True, 150),
    K.Part(
        "h",
        "model 1 (ASYM-TRI): MODEL knob, note 60 within 1 cent, knobs, track 2 ONESHOT",
        P.part_h,
        True,
        400,
    ),
    K.Part(
        "i",
        "models 2..7 (2FM, FORMANT, ADDITIVE, WAVETABLE, CHORD, SPEECH): MODEL knob, note 60 within 1 cent, knobs",
        P.part_i,
        True,
        1300,
    ),
    K.Part("j", "persistence with MODEL 4 (ADDITIVE)", P.part_j, True, 250),
    # k and l: KNOWN BROKEN in this setup, so neither tier runs them (only --only k / --only l): the harness's PLAY
    # does not advance the sequencer even on stock 1.17 (l, the control; docs/findings/07-emulator.md, "PLAY in the
    # patchkit harness"), so k is no evidence either way until the harness can drive the sequencer.
    K.Part(
        "k",
        "KNOWN BROKEN (harness): the sequencer plays PLAITS steps with MODEL p-locks, before and after save and load",
        P.part_k,
        False,
        300,
        full=False,
    ),
    K.Part(
        "l",
        "KNOWN BROKEN (harness): control of k on stock 1.17 (track 1's stock machine)",
        P.part_l,
        False,
        300,
        full=False,
    ),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--image", type=Path, default=DEFAULT_IMAGE)
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or (ROOT / "out" / "runs" / "plaits-proof")
    PlaitsProof.image = a.image.resolve()
    M.FW = "1.17"
    return K.run_tool(__file__, PlaitsProof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
