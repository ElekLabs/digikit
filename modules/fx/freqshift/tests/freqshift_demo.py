"""A demo render of the frequency shifter for listening: the real DSP routine behind core/fx on the concrete SHARC+ core (plain Python, so that it also runs under PyPy).

    PYTHONPATH=<repo>:<repo>/tests uv run --no-project --python pypy3.11 python <this file> --out FILE.wav [--seconds-per-part 2]

The input is a loop made here: a plucked-string arpeggio (Karplus-Strong), a kick and a hat on every beat, 100 BPM, stereo. The effect runs through its parts, each
`--seconds-per-part` long (the shift is set the way the knob does it: the SHIFT word of the row, a float from -64 to +63; MIX is the row's mix word):

  1  MIX 127  SHIFT 0          (the allpass pair alone: the input as it is)
  2  MIX 127  +3 Hz            (a small shift: a slight detune, the chain's shimmer)
  3  MIX 127  +100 Hz
  4  MIX 127  -300 Hz
  5  MIX 127  +1 kHz
  6  MIX 127  a sweep of the knob from -48 to +48 (zero in the middle of the part)
  7  MIX 64   +1 Hz            (the dry sound and a slow shift: a phaser-like beating)
  8  MIX 64   +100 Hz

Writes a 16-bit stereo WAV at 48 kHz and prints the part table. Not committed (audio).

`--send` renders the send route instead (docs/findings/66-freqshift-send.md): three tracks, a kick and hat track and a plucked bass track that stay dry, and a plucked lead line (track 3) that
sends to the shifter. The send hook of core/fx runs on the concrete core (the track bus times the amount squared, the effect's wet times MIX added to the FX return); the master mixes the return at unity.
Parts (each `--seconds-per-part`): amount 0 (the lead is dry), then amount 127 with +3 Hz, +100 Hz, -300 Hz, +1 kHz, a sweep of the knob from -48 to +48, and amount 64 with +100 Hz; MIX 127 throughout."""

from __future__ import annotations

import argparse
import math
import random
import struct
import wave
from pathlib import Path

from support.fx_rig import MIX, P0, ROUTE_SEND, FxRig, Row, f2w, w2f

SRC = Path(__file__).resolve().parents[1] / "src/dsp/freqshift.s"
FS = 48000
SPAN, KHZ = (
    5.5,
    0.63,
)  # the mapping of freqshift_ref.py (a number copied here: this file is plain Python)
MIX64 = 0x4000 / 0x7F00  # MIX 64 on the knob


def v_for_hz(f: float) -> float:
    return math.copysign(SPAN * math.log2(abs(f) / KHZ + 1.0), f)


def f32(x: float) -> float:
    return w2f(f2w(x))


def pluck(freq: float, seconds: float, rng: random.Random) -> list[float]:
    """Karplus-Strong: a noise burst in a delay line of one period, averaged with its neighbour."""
    n = max(2, round(FS / freq))
    line = [rng.uniform(-1, 1) for _ in range(n)]
    out = []
    for i in range(int(seconds * FS)):
        j = i % n
        y = line[j]
        line[j] = 0.4985 * (y + line[(j + 1) % n])
        out.append(y)
    return out


def loop(total: int, rng: random.Random) -> list[float]:
    """The demo input, mono, TOTAL samples."""
    beat = 60.0 / 100.0
    out = [0.0] * (total + 3 * FS)
    chords = [(57, 60, 64), (53, 57, 60), (48, 52, 55), (55, 59, 62)]  # Am F C G
    n = 0
    t = 0.0
    while t * FS < total:
        chord = chords[(int(t / beat) // 8) % 4]
        note = chord[n % 3] + (12 if n % 8 == 5 else 0)
        s = pluck(440 * 2 ** ((note - 69) / 12), 1.4, rng)
        at = int(t * FS)
        for i, y in enumerate(s):
            out[at + i] += 0.25 * y
        if n % 2 == 0:  # a kick on every beat
            for i in range(int(0.18 * FS)):
                ph = (
                    2
                    * math.pi
                    * (45 * i / FS + 75 * 0.03 * (1 - math.exp(-i / (0.03 * FS))))
                )
                out[at + i] += 0.5 * math.sin(ph) * math.exp(-i / (0.06 * FS))
        else:  # a hat
            for i in range(int(0.06 * FS)):
                out[at + i] += 0.25 * rng.uniform(-1, 1) * math.exp(-i / (0.012 * FS))
        n += 1
        t += beat / 2
    return out[:total]


PARTS = [
    ("MIX 127, SHIFT 0", 127, lambda k, n: 0.0),
    ("MIX 127, +3 Hz", 127, lambda k, n: v_for_hz(3)),
    ("MIX 127, +100 Hz", 127, lambda k, n: v_for_hz(100)),
    ("MIX 127, -300 Hz", 127, lambda k, n: v_for_hz(-300)),
    ("MIX 127, +1 kHz", 127, lambda k, n: v_for_hz(1000)),
    ("MIX 127, knob sweep -48 to +48", 127, lambda k, n: -48 + 96 * k / n),
    ("MIX 64, +1 Hz (phaser-like)", 64, lambda k, n: v_for_hz(1)),
    ("MIX 64, +100 Hz", 64, lambda k, n: v_for_hz(100)),
]


def render(seconds_per_part: float, out: Path) -> None:
    rng = random.Random(5)
    blocks = round(seconds_per_part * FS / 32)
    total = blocks * 32 * len(PARTS)
    mono = loop(total, rng)
    rig = FxRig([Row("_fs_process", "", desc="_fs_desc", params=(0,))], sources=(SRC,))
    left: list[float] = []
    right: list[float] = []
    for p, (name, mix, shift) in enumerate(PARTS):
        rig.set_row_word(0, MIX, f2w(1.0 if mix == 127 else MIX64))
        for k in range(blocks):
            rig.set_row_word(0, P0, f2w(shift(k, blocks)))
            at = (p * blocks + k) * 32
            x = [f32(s) for s in mono[at : at + 32]]
            b = rig.block(x + x, ctx="friendly")
            assert b.returned and not b.ctx_bad and not b.regs_bad
            left.extend(b.master[:32])
            right.extend(b.master[32:])
        print(f"{p + 1}  {p * seconds_per_part:6.1f} s  {name}")
    write_wav(left, right, out)


SEND_TRACK = 2  # the lead line


def send_tracks(total: int, rng: random.Random) -> list[list[float]]:
    """Three mono tracks of TOTAL samples: 0 kick and hat, 1 plucked bass, 2 the plucked lead (the sent track)."""
    beat = 60.0 / 100.0
    drums = [0.0] * (total + 2 * FS)
    bass = [0.0] * (total + 2 * FS)
    lead = [0.0] * (total + 2 * FS)
    chords = [(57, 60, 64), (53, 57, 60), (48, 52, 55), (55, 59, 62)]  # Am F C G
    n = 0
    t = 0.0
    while t * FS < total:
        step = int(t / (beat / 2))
        chord = chords[(step // 8) % 4]
        at = int(t * FS)
        if n % 2 == 0:  # a kick on every beat
            for i in range(int(0.18 * FS)):
                ph = (
                    2
                    * math.pi
                    * (45 * i / FS + 75 * 0.03 * (1 - math.exp(-i / (0.03 * FS))))
                )
                drums[at + i] += 0.5 * math.sin(ph) * math.exp(-i / (0.06 * FS))
        else:  # a hat
            for i in range(int(0.06 * FS)):
                drums[at + i] += 0.25 * rng.uniform(-1, 1) * math.exp(-i / (0.012 * FS))
        if (
            n % 4 == 0
        ):  # the bass: the root of the chord, an octave down, on every second beat
            s = pluck(440 * 2 ** ((chord[0] - 12 - 69) / 12), 0.9, rng)
            for i, y in enumerate(s):
                bass[at + i] += 0.3 * y
        note = chord[(n * 5 // 2) % 3] + (
            12 if n % 3 == 1 else 24
        )  # the lead: a rising and falling arpeggio, two octaves up
        s = pluck(440 * 2 ** ((note - 69) / 12), 0.7, rng)
        for i, y in enumerate(s):
            lead[at + i] += 0.3 * y
        n += 1
        t += beat / 2
    return [drums[:total], bass[:total], lead[:total]]


def render_send(seconds_per_part: float, out: Path) -> None:
    rng = random.Random(5)
    parts = [
        ("amount 0 (the lead is dry)", 0, lambda k, n: 0.0),
        ("amount 127, +3 Hz", 127, lambda k, n: v_for_hz(3)),
        ("amount 127, +100 Hz", 127, lambda k, n: v_for_hz(100)),
        ("amount 127, -300 Hz", 127, lambda k, n: v_for_hz(-300)),
        ("amount 127, +1 kHz", 127, lambda k, n: v_for_hz(1000)),
        ("amount 127, knob sweep -48 to +48", 127, lambda k, n: -48 + 96 * k / n),
        ("amount 64, +100 Hz", 64, lambda k, n: v_for_hz(100)),
    ]
    blocks = round(seconds_per_part * FS / 32)
    total = blocks * 32 * len(parts)
    tracks = send_tracks(total, rng)
    rig = FxRig(
        [Row("_fs_process", "", desc="_fs_desc", params=(0,), route=ROUTE_SEND)],
        sources=(SRC,),
    )
    left: list[float] = []
    right: list[float] = []
    zero = [0.0] * 64
    for p, (name, amount, shift) in enumerate(parts):
        rig.set_amounts(0, [0] * SEND_TRACK + [amount])
        for k in range(blocks):
            rig.set_row_word(0, P0, f2w(shift(k, blocks)))
            at = (p * blocks + k) * 32
            seg = [[f32(x) for x in tr[at : at + 32]] for tr in tracks]
            rig.set_track(SEND_TRACK, seg[SEND_TRACK] + seg[SEND_TRACK])
            dry = [f32(sum(sg[i] for sg in seg)) for i in range(32)]
            rig.set_return(zero)
            b = rig.send_block(dry + dry, ctx="friendly")
            assert b.returned and not b.ctx_bad and not b.regs_bad
            # the master mixes the FX return at unity
            left.extend(m + r for m, r in zip(b.master[:32], b.ret[:32], strict=True))
            right.extend(m + r for m, r in zip(b.master[32:], b.ret[32:], strict=True))
        print(f"{p + 1}  {p * seconds_per_part:6.1f} s  {name}")
    write_wav(left, right, out)


def write_wav(left: list[float], right: list[float], out: Path) -> None:
    peak = max(max(abs(s) for s in left), max(abs(s) for s in right))
    gain = min(1.0, 0.9 / peak)
    with wave.open(str(out), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(FS)
        w.writeframes(
            b"".join(
                struct.pack("<hh", round(32767 * gain * a), round(32767 * gain * b))
                for a, b in zip(left, right, strict=True)
            )
        )
    print(f"peak {peak:.3f}, scaled by {gain:.3f}; wrote {out}")


def main() -> None:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--seconds-per-part", type=float, default=2.0)
    ap.add_argument(
        "--send",
        action="store_true",
        help="the send route: one track of three goes to the shifter",
    )
    a = ap.parse_args()
    (render_send if a.send else render)(a.seconds_per_part, a.out)


if __name__ == "__main__":
    main()
