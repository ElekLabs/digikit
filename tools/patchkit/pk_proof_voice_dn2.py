#!/usr/bin/env python3
"""patchkit core/dsp-voice on Digitone II 1.12: test/patched-pluck renders through the FM engine's later stages.

    uv run python -m tools.patchkit.pk_proof_voice_dn2 [--out DIR] [--only a,b,c] [--quick] [--jobs N]

Emulator only (finding 06, "Per-voice hook on DN2"). PLUCK is the machine type 9 that test/patched-pluck adds (selected in the
machine menu; the track's frame words are forced with DSP_FRAME_POKE where a case needs another note):

  a  stock   with core/dsp-voice and the test/patched-pluck set patched in, FM TONE selected: the PCM is bit-identical to the stock
             firmware's (the hook with no registered type changes nothing), up to the phase of the key release: a patched
             image boots to a different sub-block phase of the scripted key (which image bytes decide, not the code that
             runs), so the release can land one envelope step away. Then the audio before the release is bit-identical and
             the tail is the stock tail times one constant (finding 45, "Phase sensitivity")
  b  pitch   PLUCK: note 60 gives 261.63 Hz, note 72 (pitch word 0x4800) 523.25 Hz
  c  stages  the level knob (LEVEL encoder), the filter frequency, the overdrive and the reverb send change the pluck:
             the engine's per-track processing, FX and mix see the registered voice

Parts are independent and run in parallel processes (`--jobs`, tools/patchkit/pk_proofkit.py): a; b60 and b72 (group b); c1 level,
c2 filter, c3 overdrive, c4 reverb (group c; each renders the base note it compares with). `--quick`: a, b60, c1, c2.

Output goes under OUT (default out/runs/voice-proof-dn2), never committed. Exit 0 only when every check passes.
"""

from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.patchkit import hook_ab, pluck_ref  # noqa: E402
from tools.patchkit import pk_harness as H  # noqa: E402
from tools.patchkit import pk_proofkit as K  # noqa: E402
from tools.patchkit.pk_proofkit import aligned_equal, analyse  # noqa: E402


def release_phase_equal(
    ref: np.ndarray, test: np.ndarray, max_shift: int = 256
) -> tuple[bool, str]:
    """TEST equals REF before the key release and REF's tail times one constant after it.

    The release lands one envelope step (a fraction of a DSP block) earlier or later when the patched image boots to another
    phase of the scripted key; a decaying tail then differs by a constant factor exp(-step / tau) and nothing else. Mono
    signals; the capture phase (whole frames) is searched like aligned_equal."""
    for k in sorted(range(-max_shift, max_shift + 1), key=abs):
        x, y = (ref[k:], test) if k >= 0 else (ref, test[-k:])
        n = min(len(x), len(y))
        x, y = x[:n], y[:n]
        act = np.flatnonzero(np.abs(x) > 1e-6)
        if len(act) <= 1000:
            continue
        a, b = act[0], act[-1]
        bad = np.flatnonzero(x[a:b] != y[a:b])
        if len(bad) == 0:
            return True, f"identical, shift {k}"
        first = a + int(bad[0])
        if first < a + (b - a) // 2:
            continue  # differs in the first half: not a release-phase effect
        seg = np.flatnonzero(np.abs(x[first:b]) > 1e-3 * np.abs(x).max())
        if len(seg) < 1000:
            continue
        r = y[first:b][seg] / x[first:b][seg]
        late = r[len(r) // 2 :]  # past the release ramp itself
        med = float(np.median(late))
        if 0.9 < med < 1.1 and float(np.max(np.abs(late - med))) < 1e-4:
            return (
                True,
                f"identical up to sample {first - a} of {b - a}, then x{med:.5f}, shift {k}",
            )
    return False, "no alignment"


MODS = ["core/machines", "core/params", "core/dsp-voice", "test/patched-pluck"]
PLUCK_DOWNS = 4  # FM TONE, FM DRUM, WAVETONE, SWARMER, PLUCK (before MIDI)
READY = ROOT / "out" / "ready-fastweb-dn2"


def semitone_hz(note: float) -> float:
    return 440.0 * 2 ** ((note - 69) / 12)


class Proof(K.Proof):
    tool = "voice-dn2"
    device = "dn2"
    fw = "1.12"
    ready_env = {"DIGI_PK_READY_DN2_112": str(READY)}

    def render(
        self,
        name: str,
        mods: list[str],
        *,
        downs: int = 0,
        poke: str = "",
        page: str = "",
        turns: tuple[tuple[str, int], ...] = (),
        hold_ms: int = 1000,
        run_ms: int = 1300,
    ) -> np.ndarray:
        """One fresh daemon: select the machine, set the knobs, hold TRIG 1, capture the left channel."""
        env = {"DSP_FRAME_POKE": poke} if poke else None
        with self.session(
            mods, f"{'p' if mods else 's'}-{poke or 'none'}", env=env
        ) as s:
            s.reset()
            s.run_ms(
                800 if self.quick else 1500
            )  # quick: the machine menu needs no sample to load
            if downs:
                s.do_tap(["FUNC", "SRC"])
                s.run_ms(300)
                for _ in range(downs):
                    s.do_tap("DOWN")
                    s.run_ms(80)
                s.do_tap("YES")
                s.run_ms(300)
                s.do_tap("YES")
                s.run_ms(300)
            if page:
                s.do_tap(page)
                s.run_ms(300)
            for enc, by in turns:
                for _ in range(abs(by) // 10 + 1):
                    s.do_turn(enc, int(math.copysign(min(abs(by), 10), by)))
                    s.run_ms(60)
                    by = int(math.copysign(max(abs(by) - 10, 0), by))
            wav = self.out / f"{name}.wav"
            s.emu.call("pcm.tap", action="capture", path=str(wav))
            s.emu.tap(25, hold=int(hold_ms * H.MS))
            s.run_ms(run_ms)
            s.emu.call("pcm.tap", action="capture_stop")
            s.health(f"render {name}")
        return hook_ab.read_wav(wav)[0::2].astype(np.float64)

    def part_a(self) -> None:
        ref = self.render("stock", [])
        got = self.render("patched-fmtone", MODS)
        full = np.stack([ref, ref])  # aligned_equal takes interleaved stereo
        inter = np.empty(2 * len(ref))
        inter[0::2] = ref
        inter[1::2] = ref
        other = np.empty(2 * len(got))
        other[0::2] = got
        other[1::2] = got
        del full
        ok, shift = aligned_equal(inter, other)
        detail = f"shift {shift}"
        if not ok:
            ok, detail = release_phase_equal(ref, got)
        self.check(
            "FM TONE with the hook patched in equals stock",
            ok and not analyse(inter).get("silent", True),
            f"{analyse(inter).get('active_samples')} active samples, {detail}",
        )

    def part_b(self, notes: tuple[int, ...] = (60, 72)) -> None:
        for note, poke in ((60, ""), (72, "0x02=0x4800")):
            if note not in notes:
                continue
            x = self.render(f"pluck{note}", MODS, downs=PLUCK_DOWNS, poke=poke)
            act = np.flatnonzero(np.abs(x) > 1e-5)
            seg = x[act[0] + 2000 : act[0] + 26000] if len(act) > 30000 else x[:0]
            want = semitone_hz(note)
            f = pluck_ref.f0_from_crossings(seg, want) if len(seg) else 0.0
            cents = 1200 * math.log2(f / want) if f else 99.0
            self.check(
                f"PLUCK note {note}: {want:.2f} Hz",
                abs(cents) < 2.0,
                f"{f:.3f} Hz, {cents:+.2f} cents",
            )

    def part_c(
        self, sections: tuple[str, ...] = ("level", "filter", "overdrive", "reverb")
    ) -> None:
        # quick tier: the level part renders 1.5 s (the release check needs the tail), the filter part 0.7 s and
        # compares 8000..30000 samples (the note is steady from 8000 on)
        run = (1500 if "level" in sections else 700) if self.quick else 2600
        hold = 500 if self.quick and "level" not in sections else 1000
        end = 30000 if self.quick and "level" not in sections else 50000
        base = (
            self.render("c-base", MODS, downs=PLUCK_DOWNS, run_ms=run)
            if {"level", "overdrive", "reverb"} & set(sections)
            else None
        )
        rms = lambda x, a=8000, b=end: float(np.sqrt(np.mean(x[a:b] ** 2)))  # noqa: E731
        if "level" in sections:
            lev = self.render(
                "c-level", MODS, downs=PLUCK_DOWNS, turns=(("LEVEL", -300),), run_ms=run
            )
            db = 20 * math.log10(max(rms(lev), 1e-9) / rms(base))
            self.check("the LEVEL knob lowers the pluck", db < -10, f"{db:+.1f} dB")
            env = [rms(base, k, k + 4800) for k in range(0, 40000, 4800)]
            tail = rms(base, 60000, 66000) / max(rms(base, 8000, 14000), 1e-9)
            self.check(
                "the amplitude envelope releases the voice after the note (the engine's gate)",
                tail < 0.01,
                f"tail / body {tail:.4f}, rms per 100 ms {[round(e, 4) for e in env[:8]]}",
            )
        if "filter" in sections:
            hi = "0x02=0x6000"  # note 96, 2093 Hz
            open_ = self.render(
                "c-flt-open",
                MODS,
                downs=PLUCK_DOWNS,
                poke=hi,
                hold_ms=hold,
                run_ms=700 if self.quick else 1300,
            )
            closed = self.render(
                "c-flt-closed",
                MODS,
                downs=PLUCK_DOWNS,
                poke=hi,
                page="FLTR",
                turns=(("ENC E", -300),),
                hold_ms=hold,
                run_ms=700 if self.quick else 1300,
            )
            db = 20 * math.log10(max(rms(closed), 1e-9) / rms(open_))
            self.check(
                "the filter frequency closes on the pluck", db < -6, f"{db:+.1f} dB"
            )
        if "overdrive" in sections:
            od = self.render(
                "c-overdrive",
                MODS,
                downs=PLUCK_DOWNS,
                page="FX",
                turns=(("ENC B", 300),),
            )
            crest = lambda x: float(np.abs(x).max() / np.sqrt(np.mean(x**2)))  # noqa: E731
            c0, c1 = crest(base[8000:40000]), crest(od[8000:40000])
            self.check(
                "the overdrive clips the pluck (the crest factor falls by 10 percent or more)",
                c1 < 0.9 * c0,
                f"crest factor {c0:.2f} -> {c1:.2f}",
            )
        if "reverb" in sections:
            rev = self.render(
                "c-reverb",
                MODS,
                downs=PLUCK_DOWNS,
                page="FX",
                turns=(("ENC F", 300),),
                run_ms=2600,
            )
            a = rms(base, 70000, 100000)
            b = rms(rev, 70000, 100000)
            self.check(
                "the reverb send adds a tail after the note",
                b > 5 * max(a, 1e-9),
                f"tail rms {a:.5f} -> {b:.5f}",
            )


P = Proof
PARTS = [
    K.Part("a", "stock (hook patched in, FM TONE selected)", P.part_a, True, 80),
    K.Part(
        "b60", "pitch: PLUCK at note 60", lambda p: p.part_b((60,)), True, 50, group="b"
    ),
    K.Part(
        "b72",
        "pitch: PLUCK at note 72",
        lambda p: p.part_b((72,)),
        False,
        50,
        group="b",
    ),
    K.Part(
        "c1",
        "the LEVEL knob and the envelope see the voice",
        lambda p: p.part_c(("level",)),
        True,
        60,
        group="c",
    ),
    K.Part(
        "c2",
        "the filter sees the voice",
        lambda p: p.part_c(("filter",)),
        True,
        60,
        group="c",
    ),
    K.Part(
        "c3",
        "the overdrive sees the voice",
        lambda p: p.part_c(("overdrive",)),
        False,
        60,
        group="c",
    ),
    K.Part(
        "c4",
        "the reverb send sees the voice",
        lambda p: p.part_c(("reverb",)),
        False,
        80,
        group="c",
    ),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    K.add_args(ap, PARTS)
    a = ap.parse_args()
    a.out = a.out or ROOT / "out" / "runs" / "voice-proof-dn2"
    return K.run_tool(__file__, Proof, PARTS, a)


if __name__ == "__main__":
    sys.exit(main())
