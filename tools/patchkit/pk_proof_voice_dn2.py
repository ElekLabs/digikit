#!/usr/bin/env python3
"""patchkit core/dsp-voice on Digitone II 1.12: with no machine loaded the per-voice hook changes nothing.

    uv run python -m tools.patchkit.pk_proof_voice_dn2 [--out DIR] [--quick] [--jobs N]

Emulator only (finding 06, "Per-voice hook on DN2"). With core/dsp-voice alone patched in and FM TONE selected, the PCM is
bit-identical to the stock firmware's (the hook with no registered type changes nothing), up to the phase of the key release: a
patched image boots to a different sub-block phase of the scripted key (which image bytes decide, not the code that runs), so
the release can land one envelope step away. Then the audio before the release is bit-identical and the tail is the stock tail
times one constant (finding 45, "Phase sensitivity").

Output goes under OUT (default out/runs/voice-proof-dn2), never committed. Exit 0 only when every check passes.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools.patchkit import hook_ab  # noqa: E402
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


MODS = ["core/dsp-voice"]
READY = ROOT / "out" / "ready-fastweb-dn2"


class Proof(K.Proof):
    tool = "voice-dn2"
    device = "dn2"
    fw = "1.12"
    ready_env = {"DIGI_PK_READY_DN2_112": str(READY)}

    def render(self, name: str, mods: list[str]) -> np.ndarray:
        """One fresh daemon: hold TRIG 1, capture the left channel."""
        with self.session(mods, "p" if mods else "s") as s:
            s.reset()
            s.run_ms(
                800 if self.quick else 1500
            )  # quick: the machine menu needs no sample to load
            wav = self.out / f"{name}.wav"
            s.emu.call("pcm.tap", action="capture", path=str(wav))
            s.emu.tap(25, hold=int(1000 * H.MS))
            s.run_ms(1300)
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
            "core/dsp-voice with no machine: FM TONE equals stock",
            ok and not analyse(inter).get("silent", True),
            f"{analyse(inter).get('active_samples')} active samples, {detail}",
        )


P = Proof
PARTS = [
    K.Part(
        "a",
        "stock (hook patched in, FM TONE selected)",
        P.part_a,
        True,
        80,
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
