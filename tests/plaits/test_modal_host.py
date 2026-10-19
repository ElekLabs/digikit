"""PLAITS model 12, RINGS (modules/machines/plaits/src/dsp/engines/modal.c, lib/resonator.c): the C99 port of Plaits'
ModalEngine and ModalVoice against upstream (tests/plaits/oracle/run_oracle.cc, ../eurorack/plaits at 48 kHz), bit for
bit on OUT and AUX, over strike sequences: Plaits' trigger as the wrapper builds it (RISING_EDGE | HIGH in a note-on's
first sub-block, HIGH after; never UNPATCHED, so ModalVoice's sustain branch is never taken), repeats down to every
sub-block, the 1/32-note spacing, accent at 0, 0.787 (velocity 100) and 1, knob corners and sweeps, and notes over the
wrapper's whole range. Also: energy stays bounded under repeated strikes, the table indices stay in range, the
fundamental is in tune, and every note-on strikes."""

from __future__ import annotations

import ctypes
import math
from typing import Any

import numpy as np
import pytest
from support.plaits_host import GAIN, Port, build_port, frame
from support.plaits_oracle import Oracle, Step, build, port_engine, same

Arr = Any
MODEL = 12
RISING_HIGH, HIGH, LOW = 1 | 4, 4, 0
V100 = float(np.float32(np.float32(100 << 8) * np.float32(3.0757874e-05)))
ACCENTS = (0.0, V100, 1.0)


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("port"))


@pytest.fixture(scope="module")
def checked(tmp_path_factory: pytest.TempPathFactory) -> Port:
    p = build_port(tmp_path_factory.mktemp("port-idx"), ("PL_INDEX_CHECK",))
    for name in ("port_index_lo", "port_index_hi", "port_index_n"):
        getattr(p.lib, name).argtypes = [ctypes.c_int]
    return p


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    return build(tmp_path_factory.mktemp("run-oracle"))


def strikes(
    every: int, n: int, note: float, h: float, t: float, m: float, accent: float
) -> list[Step]:
    """n sub-blocks, a note-on every `every` sub-blocks (RISING_EDGE | HIGH there, HIGH between)."""
    return [
        (note, h, t, m, RISING_HIGH if k % every == 0 else HIGH, accent)
        for k in range(n)
    ]


def check(port: Port, oracle: Oracle, steps: list[Step], size: int = 16) -> Arr:
    got = port_engine(port, "modal", steps, size)
    want = oracle.engine(MODEL, steps, size)
    assert same(got[0], want[0]) and same(got[1], want[1])
    assert np.isfinite(got[0]).all() and np.isfinite(got[1]).all()
    return got[0]


CORNERS = [0.0, 0.25, 0.5, 0.75, 1.0]


@pytest.mark.parametrize("accent", ACCENTS)
@pytest.mark.parametrize("size", [16, 12])
def test_strikes_equal_upstream_at_knob_corners(
    port: Port, oracle: Oracle, accent: float, size: int
) -> None:
    """Every HARMONICS, TIMBRE, MORPH corner at 4 pitches: strikes 1 sub-block, 1 block, and 23 sub-blocks apart."""
    for h in CORNERS:
        for t in (0.0, 0.5, 1.0):
            for m in (0.0, 0.5, 1.0):
                for note in (24.0, 60.0, 96.0, 127.0):
                    steps = (
                        strikes(1, 6, note, h, t, m, accent)
                        + strikes(2, 10, note, h, t, m, accent)
                        + strikes(23, 50, note, h, t, m, accent)
                    )
                    check(port, oracle, steps, size)


def test_fast_repeats_at_32nd_spacing_equal_upstream(
    port: Port, oracle: Oracle
) -> None:
    """1/32 notes at 300, 120 and 60 BPM (25, 62.5 and 125 ms: 75, 188 and 375 sub-blocks of 16 at 48 kHz) and a
    strike in every sub-block, at the longest ring (MORPH 1) and accent 1, for 6 s each."""
    for every in (1, 75, 188, 375):
        check(port, oracle, strikes(every, 18000, 48.0, 0.3, 0.7, 1.0, 1.0))


def test_sweeps_with_strikes_equal_upstream(port: Port, oracle: Oracle) -> None:
    """All four parameters moving (HARMONICS through the stiffness table's negative, flat and positive parts),
    strikes at irregular spacing with changing accent, notes over the wrapper's range -64..190."""
    rng = np.random.default_rng(12)
    steps: list[Step] = []
    nxt, accent = 0, 0.8
    n = 9000
    for k in range(n):
        x = k / n
        trig = RISING_HIGH if k == nxt else HIGH
        if k == nxt:
            nxt = k + int(rng.choice([1, 2, 3, 7, 30, 120, 400]))
            accent = float(rng.choice([0.0, V100, 1.0, float(rng.random())]))
        steps.append(
            (
                -64.0 + 254.0 * (0.5 - 0.5 * math.cos(6 * math.pi * x)),
                x,
                0.5 + 0.5 * math.sin(5 * x),
                1.0 - x,
                trig,
                accent,
            )
        )
    check(port, oracle, steps)


def test_extreme_notes_are_finite_and_equal_upstream(
    port: Port, oracle: Oracle
) -> None:
    for note in (-64.0, -20.0, 0.0, 120.0, 150.0, 190.0):
        for h, t, m in ((0, 0, 0), (1, 1, 1), (1, 0, 1), (0, 1, 0)):
            check(port, oracle, strikes(5, 200, note, h, t, m, 1.0))


def test_trigger_reads_only_the_rising_edge(port: Port) -> None:
    """ModalEngine reads `trigger & UNPATCHED` and `trigger & RISING_EDGE` only: HIGH or LOW after the edge is the same
    (so the gate length cannot matter), and RISING_EDGE without HIGH is the same as with it; a sub-block without the
    edge does not strike."""
    base = strikes(40, 400, 60.0, 0.3, 0.6, 0.6, V100)
    a = port_engine(port, "modal", base)[0]
    low = [(*s[:4], LOW if s[4] == HIGH else s[4], s[5]) for s in base]
    bare = [(*s[:4], 1 if s[4] == RISING_HIGH else s[4], s[5]) for s in base]
    assert same(port_engine(port, "modal", low)[0], a)
    assert same(port_engine(port, "modal", bare)[0], a)
    none = [(*s[:4], HIGH, s[5]) for s in base]
    assert not port_engine(port, "modal", none)[0].any()


def test_every_note_on_strikes(port: Port) -> None:
    """Through the wrapper: each note-on puts the click on AUX at the note-on's first sample (MIX at AUX) and adds a
    strike to OUT; the same stream without the second note-on differs from it only from that note-on on."""
    v = port.voice()
    trigs = [int(b in (0, 20)) for b in range(40)]
    x = np.concatenate(
        [port.render(v, frame(60, model=MODEL, morph=0x6000), t, 1) for t in trigs]
    )
    v = port.voice()
    y = np.concatenate(
        [
            port.render(v, frame(60, model=MODEL, morph=0x6000), int(b == 0), 1)
            for b in range(40)
        ]
    )
    assert same(x[: 32 * 20], y[: 32 * 20])
    assert not same(x[32 * 20 : 32 * 21], y[32 * 20 : 32 * 21])
    v = port.voice()
    a = np.concatenate(
        [port.render(v, frame(60, model=MODEL, mix=0x7F00), t, 1) for t in trigs]
    )
    for on in (0, 20):
        blk = np.abs(a[32 * on : 32 * on + 32])
        assert (
            blk.max() > 10 * np.abs(a[32 * on - 64 : 32 * on]).max()
            if on
            else blk.max() > 0.01
        )


def test_accent_changes_the_strike(port: Port, oracle: Oracle) -> None:
    """Accent raises the click and brightens and lengthens the ring (ModalVoice: brightness and damping += 0.25 accent
    (1 - x), click (0.12 + 0.08 accent)): louder at 1 than at 0, bit exact at both."""
    lo = check(port, oracle, strikes(400, 400, 60.0, 0.3, 0.5, 0.5, 0.0))
    hi = check(port, oracle, strikes(400, 400, 60.0, 0.3, 0.5, 0.5, 1.0))
    r = float(
        np.sqrt(np.mean(hi.astype(np.float64) ** 2))
        / np.sqrt(np.mean(lo.astype(np.float64) ** 2))
    )
    print(f"RINGS RMS accent 1 / accent 0: {r:.2f}")
    assert r > 1.3


def test_repeated_strikes_stay_bounded(port: Port) -> None:
    """Energy stays bounded under repeated strikes: at the longest ring (MORPH 1, accent 1, the resonator's highest Q)
    a strike every block for 20 s, and every 1/32 at 300 BPM: the engine stays finite and its peak grows no further
    in the last 5 s than to 1.1x the peak of the 5 s before (the resonator is linear and stable, so periodic strikes
    reach a steady level); through the wrapper the OUT limiter (gain -1) keeps the voice under 1.25 x 0.8 x the
    output gain once it has caught up (after 0.5 s)."""
    for every, note in ((2, 48.0), (75, 48.0), (2, 96.0)):
        out = port_engine(
            port, "modal", strikes(every, 60000, note, 0.3, 1.0, 1.0, 1.0)
        )[0]
        assert np.isfinite(out).all()
        p3 = float(np.abs(out[16 * 30000 : 16 * 45000]).max())
        p4 = float(np.abs(out[16 * 45000 :]).max())
        print(
            f"RINGS every {every} sub-blocks, note {note}: raw peak 10-15 s {p3:.2f}, 15-20 s {p4:.2f}"
        )
        assert p4 <= 1.1 * p3
    v = port.voice()
    trigs = [int(b % 37 == 0) for b in range(1500 * 10)]
    x = np.concatenate(
        [
            port.render(
                v,
                frame(
                    48,
                    model=MODEL,
                    harmonics=0x2600,
                    timbre=0x7F00,
                    morph=0x7F00,
                    velocity=127,
                ),
                t,
                1,
            )
            for t in trigs
        ]
    )
    assert np.isfinite(x).all()
    late = float(np.abs(x[24000:]).max())
    print(
        f"RINGS through the wrapper, strikes every 37 blocks for 10 s: peak after 0.5 s {late:.3f}"
    )
    assert late < 1.25 * 0.8 * float(GAIN)


def test_stiffness_index_in_range(checked: Port) -> None:
    """lut_stiffness (65 entries) is read at entries 0..64 (Interpolate reads index_integral + 1): HARMONICS 0..1,
    held at 1 for 20 s (the one-pole stays below 1.0)."""
    checked.lib.port_index_reset()
    steps = [(60.0, k / 3000, 0.5, 0.5, HIGH, 0.8) for k in range(3001)]
    steps += [(60.0, 1.0, 0.5, 0.5, HIGH, 0.8)] * 60000
    port_engine(checked, "modal", steps)
    lo, hi = checked.lib.port_index_lo(18), checked.lib.port_index_hi(18)
    print(f"stiffness index {lo}..{hi}")
    assert checked.lib.port_index_n(18) > 0
    assert lo == 0 and hi == 63


def test_fundamental_in_tune(port: Port) -> None:
    """At HARMONICS 0.3 (stiffness 0: the modes are harmonics of f0) the lowest mode of a ringing strike at note 69
    (the spectrum's peak between 400 and 480 Hz) is 440 Hz within 2 cents (a0 from exactly 48 kHz)."""
    out = port_engine(port, "modal", strikes(100000, 6000, 69.0, 0.3, 0.2, 0.9, V100))[
        0
    ]
    x = out[4800:].astype(np.float64) * np.hanning(len(out) - 4800)
    spec = np.abs(np.fft.rfft(x, 1 << 21))
    lo, hi = (int(f * (1 << 21) / 48000) for f in (400, 480))
    k = lo + int(np.argmax(spec[lo:hi]))
    a, b, c = spec[k - 1], spec[k], spec[k + 1]
    f = (k + 0.5 * (a - c) / (a - 2 * b + c)) * 48000 / (1 << 21)
    print(f"RINGS note 69: {f:.3f} Hz")
    assert abs(1200 * math.log2(f / 440.0)) < 2.0


def test_state_bytes(port: Port) -> None:
    """Ruling B: RINGS' per-voice state (24 mode amplitudes, 6 batches of 4 two-pole states, the exciter's filter, the
    HARMONICS one-pole) fits in the union."""
    print("modal state bytes", port.modal_size, "union", port.engine_union_size)
    assert port.modal_size <= port.engine_union_size
