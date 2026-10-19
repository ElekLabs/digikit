"""PLAITS model 21, TERRAIN (modules/machines/plaits/src/dsp/engines/terrain.c, lib/fast_sine_oscillator.c, lib/sine.h,
lib/waves.c): the C99 port of Plaits' WaveTerrainEngine against upstream (tests/plaits/oracle/run_oracle.cc,
../eurorack/plaits at 48 kHz), bit for bit on OUT and AUX: every terrain and both sides of each crossfade, TIMBRE and
MORPH corners, notes over the wrapper's whole range, sweeps of all four parameters, sub-blocks of 16 and 12. Also: the
indices upstream leaves unchecked stay in range (Sine's phase positive, the wavetable terrains inside their bank), the
path is in tune, the output is finite at the extremes.

Upstream reads outside two of its tables when the path leaves the unit square after parameter jumps (undefined
behaviour: before and past wav_integrated_waves, below lut_sine). The port guards exactly those inputs (engines/
terrain.c: the Sine phase wrapped into [0, 1), the wave coordinates kept inside the table); the oracle applies the same
guard to upstream's source only there (support.plaits_oracle.TERRAIN_GUARD), so the fuzzed jump sequences compare bit
for bit and every read stays inside the tables."""

from __future__ import annotations

import ctypes
import math
from typing import Any

import numpy as np
import pytest
from support.plaits_host import Port, build_port, frame
from support.plaits_oracle import Oracle, Step, build, port_engine, same

Arr = Any
MODEL = 21
HIGH = 4
# HARMONICS at each terrain's centre and around each crossfade edge: z = min(1.05 h, 1) * 6.9999
TERRAIN_H = sorted(
    {0.0, 1.0}
    | {
        min(1.0, (k + d) / (6.9999 * 1.05))
        for k in range(8)
        for d in (0.0, 0.5, -1e-4, 1e-4)
    }
    - {-0.0}
)
TERRAIN_H = [h for h in TERRAIN_H if 0.0 <= h <= 1.0]


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


def held(n: int, note: float, h: float, t: float, m: float) -> list[Step]:
    return [(note, h, t, m, HIGH, 0.8)] * n


def check(port: Port, oracle: Oracle, steps: list[Step], size: int = 16) -> Arr:
    got = port_engine(port, "terrain", steps, size)
    want = oracle.engine(MODEL, steps, size)
    assert same(got[0], want[0]) and same(got[1], want[1])
    assert np.isfinite(got[0]).all() and np.isfinite(got[1]).all()
    return got


@pytest.mark.parametrize("size", [16, 12])
def test_every_terrain_equals_upstream(port: Port, oracle: Oracle, size: int) -> None:
    """Each terrain and crossfade (HARMONICS), TIMBRE and MORPH corners, 4 pitches; 40 sub-blocks each, from a freshly
    selected engine (the interpolators start from 0) and continued."""
    for h in TERRAIN_H:
        for t in (0.0, 0.5, 1.0):
            for m in (0.0, 0.5, 1.0):
                steps: list[Step] = []
                for note in (24.0, 60.0, 96.0, 127.0):
                    steps += held(10, note, h, t, m)
                check(port, oracle, steps, size)


def test_sweeps_equal_upstream(port: Port, oracle: Oracle) -> None:
    """All four parameters moving (HARMONICS through all eight terrains, notes -64..190) for 6 s."""
    steps: list[Step] = []
    n = 18000
    for k in range(n):
        x = k / n
        steps.append(
            (
                -64.0 + 254.0 * (0.5 - 0.5 * math.cos(5 * math.pi * x)),
                0.5 - 0.5 * math.cos(7 * math.pi * x),
                0.5 + 0.5 * math.sin(11 * x),
                0.5 + 0.5 * math.sin(3 + 17 * x),
                HIGH,
                0.8,
            )
        )
    check(port, oracle, steps)


def test_extremes_are_finite_and_equal_upstream(port: Port, oracle: Oracle) -> None:
    for note in (-64.0, 0.0, 120.0, 150.0, 190.0):
        for h in (0.0, 0.62, 0.8, 1.0):
            for t, m in ((0, 0), (1, 1), (1, 0), (0, 1)):
                check(port, oracle, held(60, note, h, t, m))


def test_jumps_equal_upstream(port: Port, oracle: Oracle) -> None:
    """Steps between extreme settings every few sub-blocks (the interpolators and the path's renormalisation)."""
    rng = np.random.default_rng(21)
    steps: list[Step] = []
    for _ in range(300):
        s = (
            float(rng.choice([0.0, 30.0, 60.0, 100.0, 127.0, 160.0])),
            *(float(rng.choice([0.0, 1.0, rng.random()])) for _ in range(3)),
        )
        steps += held(int(rng.integers(1, 12)), *s)
    check(port, oracle, steps)


def jump_steps(seed: int, n: int = 300) -> list[Step]:
    """Jumps between a high, bright path (notes 100..135, TIMBRE 1: the magic circle's ellipse at its widest) and
    other settings every 1..5 sub-blocks: the path runs outside the unit square."""
    rng = np.random.default_rng(seed)
    steps: list[Step] = []
    for _ in range(n):
        if rng.random() < 0.5:
            note, t = float(rng.uniform(100, 135)), 1.0
        else:
            note = float(rng.choice([-64.0, 0.0, 30.0, 60.0, rng.uniform(-64, 100)]))
            t = float(rng.choice([1.0, rng.random()]))
        h = float(rng.random())
        m = float(rng.choice([0.0, 0.5, 1.0, rng.random()]))
        steps += held(int(rng.integers(1, 6)), note, h, t, m)
    return steps


def ranges(p: Port, tables: tuple[int, ...]) -> dict[int, tuple[int, int]]:
    return {t: (p.lib.port_index_lo(t), p.lib.port_index_hi(t)) for t in tables}


def test_indices_in_range(checked: Port) -> None:
    """On the held settings (every terrain, TIMBRE and MORPH corner, notes -64..190, and a sweep) the path stays
    inside the unit square and every read inside upstream's tables: Sine's phase (table 19) is never negative, the wave
    coordinate (20) is 0..62 and the sample coordinate (21) 0..127, the last entry read (22) is inside the 25,344
    floats; the terrain index (23) is 0..7."""
    checked.lib.port_index_reset()
    for h in TERRAIN_H:
        for t in (0.0, 1.0):
            for m in (0.0, 0.5, 1.0):
                for note in (-64.0, 0.0, 24.0, 60.0, 96.0, 127.0, 190.0):
                    port_engine(checked, "terrain", held(30, note, h, t, m))
    steps = [
        (
            60.0 + 60 * math.sin(k / 500),
            0.5 - 0.5 * math.cos(k / 300),
            1.0,
            0.5 + 0.5 * math.sin(k / 77),
            HIGH,
            0.8,
        )
        for k in range(20000)
    ]
    port_engine(checked, "terrain", steps)
    rng = ranges(checked, (19, 20, 21, 22, 23))
    print("TERRAIN index ranges:", rng)
    assert rng[19][0] >= 0
    assert rng[20][0] >= 0 and rng[20][1] <= 62
    assert rng[21][0] >= 0 and rng[21][1] <= 127
    assert rng[22][1] < 25344
    assert rng[23] == (0, 7)


def test_jumps_leave_the_tables_and_equal_upstream(
    port: Port, checked: Port, oracle: Oracle
) -> None:
    """After jumps the path leaves the unit square (|x|, |y| up to about 1.5) where upstream would read outside its
    tables: the guards engage (a negative Sine phase is seen) and the port equals the guarded oracle bit for bit; every
    wave read stays inside the 25,344 entries."""
    checked.lib.port_index_reset()
    for seed in range(40):
        steps = jump_steps(seed)
        check(port, oracle, steps)
        port_engine(checked, "terrain", steps)
    rng = ranges(checked, (19, 24, 25, 26, 27))
    print(
        "TERRAIN jumps: path x, y (x 1e4)",
        rng[24],
        rng[25],
        "wave reads",
        rng[26],
    )
    assert rng[19][0] == -1  # a negative Sine phase was seen
    assert min(rng[24][0], rng[25][0]) < -10000 or max(rng[24][1], rng[25][1]) > 10000
    assert rng[26][0] >= 0 and rng[26][1] + 2 < 25344


def test_path_in_tune(port: Port) -> None:
    """The path circles at f0: at note 69 OUT's fundamental is 440 Hz within 1 cent (terrain 0, MORPH 0.5)."""
    out = port_engine(port, "terrain", held(6000, 69.0, 0.0, 0.5, 0.5))[0]
    x = out[4800:].astype(np.float64) * np.hanning(len(out) - 4800)
    spec = np.abs(np.fft.rfft(x, 1 << 21))
    lo, hi = (int(f * (1 << 21) / 48000) for f in (400, 480))
    k = lo + int(np.argmax(spec[lo:hi]))
    a, b, c = spec[k - 1], spec[k], spec[k + 1]
    f = (k + 0.5 * (a - c) / (a - 2 * b + c)) * 48000 / (1 << 21)
    print(f"TERRAIN note 69: {f:.3f} Hz")
    assert abs(1200 * math.log2(f / 440.0)) < 1.0


def test_wrapper_renders_terrain(port: Port) -> None:
    """MODEL 21 sounds through the wrapper, with each knob changing it."""

    def run(**k: int) -> Arr:
        v = port.voice()
        return np.concatenate(
            [
                port.render(v, frame(60, model=MODEL, **k), int(b == 0), 1)
                for b in range(40)
            ]
        )

    base = run()
    assert float(np.abs(base).max()) > 0.05
    for knob in ("harmonics", "timbre", "morph"):
        assert not same(run(**{knob: 0x7000}), base), knob


def test_state_bytes(port: Port) -> None:
    print("terrain state bytes", port.terrain_size, "union", port.engine_union_size)
    assert port.terrain_size <= port.engine_union_size
