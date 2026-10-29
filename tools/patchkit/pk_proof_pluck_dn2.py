#!/usr/bin/env python3
"""patchkit: the PLUCK machine on Digitone II 1.12 (core/dsp-voice DN2 object + core/machines + core/params +
test/patched-pluck), in the emulator, through the panel.

    uv run python -m tools.patchkit.pk_proof_pluck_dn2 [--quick] [--only a,b,c,d,e] [--out DIR] [--jobs N]

PLUCK is a machine of type 9 (listed before MIDI) with one SYN page of its own: knobs A..D = DECAY, PICK, BRIGHT, COLOR
(sound-array slots 25..28, UI 0..127 as words 0..0x7f00; defaults 0x4000, 0x2000, 0x5000, 0x2800). The DSP routine runs
per voice slot, takes the note-on from the rising edge of frame halfword 0x11 and its words from the slot's frame block
(finding 06, "DN2 voice slots"). Independent parts, one daemon each:

  a  tuning    the fundamental of TRIG 1 (note 60) and of keyboard notes against the pitch word of the voice slot
               (tolerance 3 cents); the defaults are PLUCK's (slots 25..28 of the held voice)
  b  params    DECAY, BRIGHT, COLOR and PICK set through the encoders change the sound as documented in pluck.s: T60 (DECAY),
               HF/LF late in the note (BRIGHT), the excitation spectrum (COLOR), a notch at the 4th and 8th harmonic at
               PICK = 1/2
  c  retrigger the same key 20 times (150 ms apart, also past 16 voice slots) gives 20 plucks of the right pitch (the edge
               works for a repeated note); full mode adds 40 notes 110 ms apart (the plucks overlap: 16 slots in use)
  d  sequencer a step recorded on track 1 plays back as plucks of note 60 on both passes
  e  persist   edit DECAY and BRIGHT, save, set the track to FM TONE, load: type 9 and the values are back; stock loads
               the project without a fault

Every input asserts no fault, no DSP halt and no main-loop stall (harness health). `--quick` runs a (two notes), b (DECAY
and BRIGHT) and c (20 notes). Output (PNG, WAV) goes under OUT (default out/runs/pluck-dn2-proof), never committed.
Exit 0 only when every check passes.
"""

from __future__ import annotations

import argparse
import importlib.util
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.emulator import card_overlay  # noqa: E402
from tools.patchkit import hook_ab  # noqa: E402
from tools.patchkit import pk_dn2_common as C  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.pitch import f0_from_crossings  # noqa: E402

# The float32 model of the fixture's routine lives next to the fixture (its own copy, like the DSP source).
_spec = importlib.util.spec_from_file_location(
    "pluck_ref", ROOT / "tests/fixtures/modules/patched-pluck/tests/pluck_ref.py"
)
assert _spec and _spec.loader
pluck_ref = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pluck_ref)

MODS = ["core/machines", "core/params", "core/dsp-voice", "test/patched-pluck"]
PLUCK_DOWNS = 4  # FM TONE, FM DRUM, WAVETONE, SWARMER, PLUCK (before MIDI)
PLUCK_TYPE = 9  # core/machines types.toml, "test/patched-pluck:pluck" on dn2
DECAY, PICK, BRIGHT, COLOR = 25, 26, 27, 28
ENC = {DECAY: "ENC A", PICK: "ENC B", BRIGHT: "ENC C", COLOR: "ENC D"}
DEFAULTS = {DECAY: 0x4000, PICK: 0x2000, BRIGHT: 0x5000, COLOR: 0x2800}


class PluckProof(C.Dn2Proof):
    def pluck(self, s: H.Session) -> None:
        """A fresh start with PLUCK selected on track 1, its SYN page open."""
        self.fresh(s)
        self.select(s, PLUCK_DOWNS)
        s.do_tap("SYN")
        s.run_ms(400)

    def setp(self, s: H.Session, slot: int, value: int) -> int:
        return self.turn_held(s, ENC[slot], slot, value)

    QUICK_DECAY = 0x1800  # a ringing note costs ~35 s of wall per emulated second (interpreted DSP): short notes only

    def quick_decay(self, s: H.Session, value: int | None = None) -> None:
        """Quick tier: DECAY to the bottom with no key held (no voice rings), then up to VALUE."""
        s.turn_burst(ENC[DECAY], -4, 60)
        self.setp(s, DECAY, self.QUICK_DECAY if value is None else value)

    def cents(self, f: float, note: float) -> float:
        return 1200 * math.log2(f / C.semitone_hz(note))

    def excitation_equals_model(self, s: H.Session, label: str) -> bool:
        """Press TRIG 1 and compare the excitation the DSP wrote into the slot's delay line (the last N + 1 samples, which
        the loop does not touch for 80 ms) with the numpy model of the routine (pluck_ref.py in the tests folder of the fixture) seeded with the DSP's
        noise state before the note: the pick comb, the colour filter, the amplitude, the mean removal and N."""
        s.run_ms(500)  # the knob's words settle (a smoother follows the sound array)
        rng = int(self.dsp_words(s, self.dsp_sym(s, "_pk_rng"), 1)[0])
        buf = self.dsp_sym(
            s, "_dkstate_pluck"
        )  # the voices' state blocks of the patched-in contract machine: a row of 32 words, then the line
        v = self.hold(s, "TRIG 1")  # runs 80 ms
        line = self.dsp_words(s, buf + 0x4080 * v + 128, 4096).view("<f4")
        d = [self.word(s, v, k) for k in (DECAY, PICK, BRIGHT, COLOR)]
        vel = s._u16(C.TX + 0x34 + 2 * v)
        pitch = self.pitch_word(s, v)
        self.release(s, "TRIG 1")
        q = pluck_ref.Params(pitch, 0x4000, d[0], d[1], d[2], d[3], vel)
        pl = pluck_ref.Pluck()
        pl.rng = rng
        pl.note_on(q)
        a = line[4096 - q.N - 1 :]
        b = pl.line[4096 - q.N - 1 :]
        diff = float(np.max(np.abs(a - b)))
        return self.check(
            f"{label}: the excitation equals the model (N {q.N}, comb p {q.p})",
            diff == 0.0 and float(np.max(np.abs(b))) > 0.1,
            f"max |difference| {diff:g}, peak {float(np.max(np.abs(b))):.3f}, slot words {[hex(x) for x in d]}",
        )

    # -- a
    def part_a(self, quick: bool) -> None:
        print("a  tuning: the fundamental against the pitch word of the voice slot")
        with self.session(MODS, "a") as s:
            self.pluck(s)
            v = self.hold(s, "TRIG 1")
            got = {k: self.word(s, v, k) for k in DEFAULTS}
            self.release(s, "TRIG 1")
            self.check(
                f"the audio mirror holds type {PLUCK_TYPE} and the defaults are PLUCK's",
                self.wire_type(s) == PLUCK_TYPE and got == DEFAULTS,
                {k: hex(x) for k, x in got.items()},
            )
            if quick:
                self.quick_decay(s)
            left, pitch, _ = self.play(
                s,
                "a-trig1",
                "TRIG 1",
                hold=100 if quick else 300,
                total=330 if quick else 2200,
            )
            st = C.onset(left)
            self.check(
                "TRIG 1 plays note 60", pitch == 0x3C00, f"pitch word {pitch:#06x}"
            )
            self.check("the note sounds", st >= 0, st)
            if st >= 0:
                f = C.f0_of(
                    left, st, C.semitone_hz(pitch / 256), 4000 if quick else 30000
                )
                c = self.cents(f, pitch / 256)
                self.check(
                    f"note {pitch / 256:g}: f0 {f:.3f} Hz",
                    abs(c) < 3,
                    f"{c:+.4f} cents",
                )
            s.do_tap("KEYBOARD")
            s.run_ms(300)
            keys = (
                ("TRIG 13",)
                if quick
                else ("TRIG 2", "TRIG 3", "TRIG 5", "TRIG 13", "TRIG 16")
            )
            for key in keys:
                left, pitch, _ = self.play(
                    s,
                    f"a-{key.replace(' ', '').lower()}",
                    key,
                    hold=100 if quick else 300,
                    total=330 if quick else 2200,
                )
                st = C.onset(left)
                if st < 0:
                    self.check(
                        f"{key}: sounds", False, f"silent, pitch word {pitch:#06x}"
                    )
                    continue
                note = pitch / 256
                f = C.f0_of(left, st, C.semitone_hz(note), 4000 if quick else 30000)
                c = self.cents(f, note)
                self.check(
                    f"{key}: note {note:g}, f0 {f:.3f} Hz",
                    abs(c) < 3,
                    f"{c:+.4f} cents",
                )
            s.health("end of a")

    def part_b_quick(self, s: H.Session, note) -> None:  # noqa: ANN001
        """DECAY (slope of the first 0.15 s at two settings), BRIGHT (HF/LF 0.125 s in, dark against bright) and the
        PICK excitation against the model (bit for bit), on short notes."""
        self.quick_decay(s, 0x1800)
        reached = self.setp(s, BRIGHT, 0x7F00)
        self.check("BRIGHT set to its top", reached >= 0x7C00, f"{reached:#x}")
        slope = {}
        for label, value in (("short", 0x1800), ("mid", 0x2800)):
            self.setp(s, DECAY, value)
            left, st = note(f"b-decay-{label}", 330)
            slope[label] = C.db_slope(left, st)
            print(f"     DECAY {value:#x}: {slope[label]:.1f} dB/s")
        self.check(
            "DECAY lengthens the note (slope at 0x2800 < 0.75 x slope at 0x1800)",
            slope["mid"] < 0.75 * slope["short"],
            f"{slope['short']:.1f} dB/s -> {slope['mid']:.1f} dB/s, ratio {slope['mid'] / slope['short']:.2f}",
        )
        hf = {}  # DECAY stays at 0x2800: the damping filter acts once per pass, the effect shows late in the note
        for label, value in (("dark", 0x0800), ("bright", 0x7F00)):
            self.setp(s, BRIGHT, value)
            left, st = note(f"b-bright-{label}", 330)
            hf[label] = C.band_db(left, st + 6000, 2048, 3000, 12000) - C.band_db(
                left, st + 6000, 2048, 100, 1000
            )
            print(f"     BRIGHT {value:#x}: HF/LF {hf[label]:.1f} dB")
        self.check(
            "BRIGHT raises the high frequencies",
            hf["bright"] - hf["dark"] > 6,
            f"{hf['bright'] - hf['dark']:.1f} dB",
        )
        self.setp(s, PICK, 0x3F80)
        self.excitation_equals_model(s, "PICK 1/2")
        s.health("end of b (quick)")

    # -- b
    def part_b(self, quick: bool) -> None:
        print("b  parameters set through the encoders change the sound")
        with self.session(MODS, "b") as s:
            self.pluck(s)

            def note(name: str, total: int = 2200) -> tuple[np.ndarray, int]:
                # the key stays down: the engine's gate must not cut the string
                left, _, _ = self.play(
                    s,
                    name,
                    "TRIG 1",
                    hold=total - 200 if not quick else 200,
                    total=total,
                )
                return left, C.onset(left)

            if quick:
                self.part_b_quick(s, note)
                return
            reached = self.setp(s, BRIGHT, 0x7F00)
            self.check("BRIGHT set to its top", reached >= 0x7C00, f"{reached:#x}")
            res = {}
            for label, value, total in (
                ("short", 0x2000, 2200),
                ("mid", 0x4000, 3200),
                ("long", 0x5000, 6000),
            ):
                self.setp(s, DECAY, value)
                left, st = note(f"b-decay-{label}", total)
                res[label] = C.t60_of(left, st)
                print(f"     DECAY {value:#x}: T60 {res[label]}")
            self.check(
                "DECAY lengthens the note (T60 short < mid < long)",
                all(res.values()) and res["short"] < res["mid"] < res["long"],
                {k: round(x, 3) if x else x for k, x in res.items()},
            )
            self.check(
                "DECAY 0x4000 with BRIGHT at the top gives T60 about 1.02 s",
                res["mid"] is not None and abs(res["mid"] / 1.02 - 1) < 0.25,
                res["mid"],
            )
            self.setp(s, DECAY, 0x4000)
            hf = {}
            for label, value in (("dark", 0x0800), ("bright", 0x7F00)):
                self.setp(s, BRIGHT, value)
                left, st = note(f"b-bright-{label}")
                hf[label] = C.band_db(left, st + 9600, 4096, 3000, 12000) - C.band_db(
                    left, st + 9600, 4096, 100, 1000
                )
                print(f"     BRIGHT {value:#x}: HF/LF {hf[label]:.1f} dB")
            self.check(
                "BRIGHT raises the high frequencies late in the note",
                hf["bright"] - hf["dark"] > 6,
                f"{hf['bright'] - hf['dark']:.1f} dB",
            )
            self.setp(s, PICK, 0x3F80)
            self.excitation_equals_model(s, "PICK 1/2")
            if quick:
                s.health("end of b (quick)")
                return
            self.setp(s, BRIGHT, 0x7F00)
            col = {}
            for label, value in (("white", 0x0000), ("soft", 0x7F00)):
                self.setp(s, COLOR, value)
                left, st = note(f"b-color-{label}")
                col[label] = C.band_db(left, st + 200, 4096, 4000, 20000) - C.band_db(
                    left, st + 200, 4096, 100, 1000
                )
                print(f"     COLOR {value:#x}: HF/LF {col[label]:.1f} dB")
            self.check(
                "COLOR (excitation lowpass) lowers the high frequencies",
                col["white"] - col["soft"] > 6,
                f"{col['white'] - col['soft']:.1f} dB",
            )
            self.setp(s, COLOR, 0x2800)
            # PICK: the excitation, bit for bit, at three settings; the notch is then the model's. The audio statistic is
            # printed for information (the noise excitation scatters a pluck's harmonics by several dB, and the engine's
            # filter tilts the others-against-notch baseline): six plucks per setting.
            for value in (0x0000, 0x2000, 0x7F00):
                self.setp(s, PICK, value)
                self.excitation_equals_model(s, f"PICK {value:#x}")
            for value in (0x0000, 0x7F00):
                self.setp(s, COLOR, value)
                self.excitation_equals_model(s, f"COLOR {value:#x}")
            self.setp(s, COLOR, 0x2800)
            comb = {}
            for label, value in (("none", 0x0000), ("half", 0x3F80)):
                self.setp(s, PICK, value)
                notch = other = 0.0
                for k in range(6):
                    left, st = note(f"b-pick-{label}-{k}", 1000)
                    f0 = C.f0_of(left, st, C.semitone_hz(60))
                    lev = {
                        m: 10 ** (C.harmonic_db(left, st + 400, 8192, f0, m) / 10)
                        for m in range(2, 10)
                    }
                    notch += lev[4] + lev[8]
                    other += (lev[2] + lev[3] + lev[5] + lev[6] + lev[7] + lev[9]) / 3
                comb[label] = 10 * math.log10(other / notch)
            self.check(
                "PICK = 1/2 puts the 4th and 8th harmonics down against the others (6 plucks each)",
                comb["half"] - comb["none"] > 2.5,
                f"others / (H4+H8): {comb['half']:.1f} dB against {comb['none']:.1f} dB without (the model: 8.5 and -0.7)",
            )
            s.health("end of b")

    # -- c
    def burst(
        self, s: H.Session, name: str, n: int, gap: int, hold: int, tail: int = 1500
    ) -> np.ndarray:
        """N presses of TRIG 1, GAP ms apart (HOLD ms down), PCM captured; the left channel."""
        assert s.emu is not None
        wav = self.out / f"{name}.wav"
        s.emu.call("pcm.tap", action="capture", path=str(wav))
        s.run_ms(100)  # the onset detector skips the first 10 ms
        for _ in range(n):
            s.do_hold("TRIG 1", hold)
            s.run_ms(gap - hold)
        s.run_ms(tail)
        s.emu.call("pcm.tap", action="capture_stop")
        s.health(f"burst {name}")
        self.slot = (self.slot + n) % C.SLOTS
        return C.left_of(hook_ab.read_wav(wav))

    def part_c(self, quick: bool) -> None:
        print("c  repeated notes of one key (the edge) and more than 16 in a row")
        cases = [("20 notes, 150 ms", 20, 150, 60)]
        if quick:
            cases = [("18 notes, 150 ms", 18, 150, 60)]  # more than the 16 voice slots
        if not quick:
            cases.append(("40 notes, 110 ms", 40, 110, 40))
        with self.session(MODS, "c") as s:
            self.pluck(s)
            if quick:
                self.quick_decay(s)
            for label, n, gap, hold in cases:
                left = self.burst(s, f"c-{n}", n, gap, hold)
                on = C.onsets(left)
                # a note is heard when its window holds a pluck of the right pitch
                self.check(
                    f"{label}: {n} plucks",
                    len(on) == n,
                    f"{len(on)} onsets, spacing {np.diff(on).min() / 48 if len(on) > 1 else 0:.0f}..{np.diff(on).max() / 48 if len(on) > 1 else 0:.0f} ms",
                )
                if len(on) == n:
                    bad = 0
                    for st in on[:: max(1, n // 10)]:
                        seg = left[st + 300 : st + 300 + 6000]
                        if len(seg) < 3000:
                            continue
                        f = f0_from_crossings(seg, C.semitone_hz(60))
                        if abs(self.cents(f, 60)) > 10:
                            bad += 1
                    self.check(
                        f"{label}: sampled notes are in tune (note 60)",
                        bad == 0,
                        f"{bad} off",
                    )
                # let every string ring out, then a note must still work (slots idle, state kept)
                s.run_ms(500)
            left, _, _ = self.play(s, "c-after", "TRIG 1", total=330 if quick else 2200)
            self.check("a note after the burst sounds", C.onset(left) >= 0)
            s.health("end of c")

    # -- d
    def part_d(self, quick: bool) -> None:
        print("d  sequencer: a recorded step plays back")
        with self.session(MODS, "d") as s:
            self.pluck(s)
            s.do_tap("RECORD")
            s.run_ms(200)
            s.do_tap("PLAY")
            s.run_ms(100)
            s.do_hold("TRIG 1", 200)
            s.run_ms(2500)
            s.do_tap("STOP")
            s.run_ms(500)
            s.do_tap("RECORD")
            s.run_ms(200)
            assert s.emu is not None
            wav = self.out / "d-seq.wav"
            s.emu.call("pcm.tap", action="capture", path=str(wav))
            s.do_tap("PLAY")
            s.run_ms(5200)
            s.emu.call("pcm.tap", action="capture_stop")
            s.do_tap("STOP")
            s.run_ms(300)
            s.health("sequence")
            left = C.left_of(hook_ab.read_wav(wav))
            on = C.onsets(left)
            self.check(
                "the sequencer plucks the string on both passes of the pattern",
                len(on) >= 2 and 1.5 < (on[1] - on[0]) / 48000 < 3.0,
                f"onsets at {[round(o / 48000, 3) for o in on]} s",
            )
            if on:
                f = C.f0_of(left, on[0], C.semitone_hz(60))
                self.check(
                    "the first sounds at note 60",
                    abs(self.cents(f, 60)) < 3,
                    f"{f:.3f} Hz",
                )

    # -- e
    def part_e(self, quick: bool) -> None:
        print("e  persistence (save and load, then stock load)")
        overlay = self.out / "card.overlay"
        want = {DECAY: 0x2000, BRIGHT: 0x6000}
        with self.session(MODS, "e") as s:
            self.pluck(s)
            for slot, val in want.items():
                self.setp(s, slot, val)
            v = self.hold(s, "TRIG 1")
            keep = {k: self.word(s, v, k) for k in DEFAULTS}
            self.release(s, "TRIG 1")
            self.check(
                "the edited values are in the voice words",
                abs(keep[DECAY] - 0x2000) <= 0x300
                and abs(keep[BRIGHT] - 0x6000) <= 0x300,
                {k: hex(x) for k, x in keep.items()},
            )
            self.save_project(s)
            r = s.emu.call("card.overlay", path=str(overlay)) if s.emu else {}
            self.check(
                "card writes exported", r.get("sectors", 0) > 0, r.get("sectors")
            )
            types = card_overlay.sound_types(overlay.read_bytes(), type_at=0xF4)
            self.check(
                f"the saved project stores type {PLUCK_TYPE} in sound records",
                types.get(PLUCK_TYPE, 0) > 0,
                types,
            )
            s.do_tap(["FUNC", "SRC"])
            s.run_ms(300)
            for _ in range(6):
                s.do_tap("UP")
                s.run_ms(80)
            s.do_tap("YES")
            s.run_ms(300)
            s.do_tap("YES")
            s.run_ms(300)
            self.check("track set to FM TONE", self.menu_row(s, "e-fmtone") == 0)
            self.load_project(s)
            self.check(
                "project reload brings PLUCK back (row 4)",
                self.menu_row(s, "e-loaded") == 4,
            )
            v = self.hold(s, "TRIG 1")
            self.release(s, "TRIG 1")
            back = {k: self.word(s, v, k) for k in DEFAULTS}
            self.check(
                "and the four knob values",
                back == keep,
                f"{ {k: hex(x) for k, x in back.items()} } vs { {k: hex(x) for k, x in keep.items()} }",
            )
            left, _, _ = self.play(s, "e-loaded", "TRIG 1")
            self.check("the loaded PLUCK sounds", C.onset(left) >= 0)
            s.load(s.syx, settle=False)
            s.emu.drain_events()
            s.run_ms(2000)
            s.emu.pcm_start()
            s.pcm_mark = 0
            s.health("stock firmware")
            self.load_project(s)
            self.check(
                "STOCK loads the project: track 1 is FM TONE",
                self.menu_row(s, "e-stock") == 0,
            )
            s.do_hold("TRIG 1", 300)
            s.run_ms(300)
            s.health("stock after loading the patched project")


P = PluckProof
PARTS = [
    K.Part("a", "tuning", lambda p: p.part_a(p.quick), True, 80),
    K.Part(
        "b", "parameters through the encoders", lambda p: p.part_b(p.quick), True, 100
    ),
    K.Part("c", "repeated notes", lambda p: p.part_c(p.quick), True, 80),
    K.Part("d", "sequencer", lambda p: p.part_d(p.quick), False, 80),
    K.Part("e", "persistence", lambda p: p.part_e(p.quick), False, 120),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or ROOT / "out" / "runs" / "pluck-dn2-proof"
    return K.run_tool(__file__, PluckProof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
