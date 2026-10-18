"""Host tests for the PLAITS 2FM (model 2), FORMANT (model 3) and ADDITIVE (model 4) ports
(modules/machines/plaits/src/dsp/engines/fm.c, grain.c, additive.c, with the shared helpers and tables of src/dsp/lib):
each C99 engine against upstream Plaits' FMEngine, GrainEngine and AdditiveEngine at 48 kHz, the shared tables against
upstream's, and the voice wrapper's handling of models 2..4 (each renders its engine; a model change takes effect at
the next note-on, with no state left from the previous model).

The oracle (tests/plaits/oracle/fga_oracle.cc) links upstream's fm_engine.cc, grain_engine.cc, additive_engine.cc and
resources.cc from ../eurorack/plaits with the 48 kHz dsp.h override, as test_ws_host.py does for ASYM-TRI. Both sides
are built with the host compiler without floating-point contraction, so the results must be equal bit for bit."""

from __future__ import annotations

import ctypes
import math
from pathlib import Path
from typing import Any

import numpy as np
import pytest
from support.plaits_host import EURORACK, FP, Port, build_oracle, build_port, frame

ORACLE = Path(__file__).resolve().parent / "oracle"
FS = 48000.0
Arr = Any
ENGINES = {"fm": 0, "grain": 1, "additive": 2}
MODEL = {"fm": 2, "grain": 3, "additive": 4}


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.fga_oracle_init.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.fga_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.lib.fga_oracle_lut.restype = ctypes.c_float

    def engine(
        self, name: str, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        st = ctypes.create_string_buffer(self.lib.fga_oracle_size())
        self.lib.fga_oracle_init(st, ENGINES[name])
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            self.lib.fga_oracle_render(
                st,
                *s,
                out[k * size :].ctypes.data_as(FP),
                aux[k * size :].ctypes.data_as(FP),
                size,
            )
        return out, aux


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("port"))


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    d = tmp_path_factory.mktemp("fga-oracle")
    engine = EURORACK / "plaits" / "dsp" / "engine"
    sources = [
        ORACLE / "fga_oracle.cc",
        engine / "fm_engine.cc",
        engine / "grain_engine.cc",
        engine / "additive_engine.cc",
        EURORACK / "plaits" / "resources.cc",
    ]
    return Oracle(build_oracle(d, sources, "fga_oracle"))


SETS = [
    (48.0, 0.5, 0.5, 0.5),
    (60.0, 0.0, 0.25, 0.8),
    (36.5, 1.0, 1.0, 0.0),
    (84.0, 0.3, 0.7, 0.66),
]


def same(a: Arr, b: Arr) -> bool:
    return bool(a.view(np.uint32).tolist() == b.view(np.uint32).tolist())


def test_lib_tables_equal_upstream_bit_for_bit(port: Port, oracle: Oracle) -> None:
    for k in range(5):
        n = port.lib.port_lib_lut_size(k)
        mine = np.array(
            [port.lib.port_lib_lut(k, i) for i in range(n)], dtype=np.float32
        )
        theirs = np.array(
            [oracle.lib.fga_oracle_lut(k, i) for i in range(n)], dtype=np.float32
        )
        assert same(mine, theirs), k


def sweep(blocks: int) -> list[tuple[float, float, float, float]]:
    """Every control moving block by block (the interpolators and pitch path move)."""
    return [
        (
            84.0 - 60.0 * b / blocks,
            (0.3 + 0.9 * b / blocks) % 1.0,
            (0.7 + 1.7 * b / blocks) % 1.0,
            (0.66 + 2.3 * b / blocks) % 1.0,
        )
        for b in range(blocks)
    ]


@pytest.mark.parametrize("name", sorted(ENGINES))
@pytest.mark.parametrize("k", range(len(SETS) + 1))
def test_c99_matches_upstream_48k_bit_exact(
    port: Port, oracle: Oracle, name: str, k: int
) -> None:
    blocks = 4000
    settings = sweep(blocks) if k == len(SETS) else [SETS[k]] * blocks
    po, pa = port.engine(name, settings, 12)
    oo, oa = oracle.engine(name, settings, 12)
    assert np.isfinite(oo).all() and np.abs(oo).max() > 0.05
    assert np.abs(oa).max() > 0.05
    assert same(po, oo), name
    assert same(pa, oa), name


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_sub_block_of_16_bit_exact(port: Port, oracle: Oracle, name: str) -> None:
    """The wrapper renders sub-blocks of 16 samples (PLAITS_SUB); the sweep at that size."""
    settings = sweep(3000)
    po, pa = port.engine(name, settings, 16)
    oo, oa = oracle.engine(name, settings, 16)
    assert same(po, oo) and same(pa, oa), name


CORNERS = [
    (n, h, t, m)
    for n in (-64.0, 0.0, 127.0, 190.0)
    for h in (0.0, 1.0)
    for t in (0.0, 1.0)
    for m in (0.0, 1.0)
]


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_extreme_params_bit_exact_and_finite(
    port: Port, oracle: Oracle, name: str
) -> None:
    """Every {0, 1}^3 corner of HARMONICS/TIMBRE/MORPH at notes -64 and 190 (the wrapper's range), 0, 127: equal to
    upstream, finite, bounded."""
    for c in CORNERS:
        po, pa = port.engine(name, [c] * 300, 16)
        oo, oa = oracle.engine(name, [c] * 300, 16)
        for x in (po, pa):
            assert np.isfinite(x).all(), (name, c)
            assert float(np.abs(x).max()) < 4.0, (name, c)
        assert same(po, oo), (name, c)
        assert same(pa, oa), (name, c)


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_corner_jumps_bit_exact_and_finite(
    port: Port, oracle: Oracle, name: str
) -> None:
    """Jumps between the corners every block (pitch by up to 254 semitones within one block, no note-on): equal to
    upstream (none of these engines reads outside a table there), finite and bounded."""
    corners = [
        (n, h, t, m)
        for n in (-64.0, -20.0, 0.0, 127.0, 160.0, 190.0)
        for h in (0.0, 1.0)
        for t in (0.0, 1.0)
        for m in (0.0, 1.0)
    ]
    for stride in (7, 11, 13):
        jumps = [corners[(stride * b) % len(corners)] for b in range(3000)]
        po, pa = port.engine(name, jumps, 16)
        oo, oa = oracle.engine(name, jumps, 16)
        for x in (po, pa):
            assert np.isfinite(x).all(), (name, stride)
            assert float(np.abs(x).max()) < 4.0, (name, stride)
        assert same(po, oo) and same(pa, oa), (name, stride)


def f0_fft(x: Arr, guess: float) -> float:
    x = np.asarray(x, dtype=np.float64)
    x = x - x.mean()
    n = len(x)
    pad = 4 * n
    sp = np.abs(np.fft.rfft(x * np.hanning(n), pad))
    fr = np.fft.rfftfreq(pad, 1 / FS)
    lo, hi = np.searchsorted(fr, guess * 0.8), np.searchsorted(fr, guess * 1.25)
    i = lo + int(np.argmax(sp[lo:hi]))
    a, b, c = (math.log(sp[j] + 1e-30) for j in (i - 1, i, i + 1))
    off = 0.5 * (a - c) / (a - 2 * b + c)
    return float((i + off) * FS / pad)


# A setting whose fundamental is clear, and where each output's fundamental lies: FM's AUX is the sub-oscillator,
# an octave down; ADDITIVE's centroid at the lowest harmonics.
PITCHED = {
    "fm": ((69.0, 0.5, 0.0, 0.5), {"out": 440.0, "aux": 220.0}),
    "grain": ((69.0, 0.5, 0.3, 0.6), {"out": 440.0, "aux": 440.0}),
    "additive": ((69.0, 0.0, 0.0, 0.3), {"out": 440.0, "aux": 440.0}),
}


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_pitch_note69_at_48k(port: Port, name: str) -> None:
    setting, want = PITCHED[name]
    out, aux = port.engine(name, [setting] * (4 * 48000 // 16), 16)
    for label, x in (("out", out), ("aux", aux)):
        f = f0_fft(x[48000:], want[label])
        assert abs(1200 * math.log2(f / want[label])) < 1.0, (name, label, f)


def _render(port: Port, v: Any, f: Any, trig: int) -> Arr:
    return port.render(v, f, trig, 1)


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_wrapper_renders_the_model(port: Port, name: str) -> None:
    """MODEL 2/3/4 renders fm/grain/additive: OUT + MIX (AUX - OUT), then the DC blocker and the gain, bit for bit
    (the wrapper's arithmetic in float32: unit() is word * (1 / 0x7f00) clamped to [0, 1])."""
    inv = np.float32(3.0757874e-05)
    gain, r = np.float32(0.601), np.float32(0.99934550)
    p = float(np.float32(16384.0) * inv)
    out, aux = port.engine(name, [(60.0, p, p, p)] * 40, 16)
    for mix in (0, 0x2000, 0x7F00):
        m = min(np.float32(np.float32(mix) * inv), np.float32(1.0))
        v = port.voice()
        got = np.concatenate(
            [
                _render(port, v, frame(60, model=MODEL[name], mix=mix), int(b == 0))
                for b in range(20)
            ]
        )
        eng = (out + m * (aux - out)).astype(np.float32)
        x1 = y1 = np.float32(0.0)
        want = np.zeros_like(eng)
        for i, x in enumerate(eng):
            y1 = np.float32(np.float32(x - x1) + np.float32(r * y1))
            x1 = x
            want[i] = np.float32(y1 * gain)
        assert same(got, want), (name, mix)
        assert float(np.abs(got).max()) > 0.02


def test_engine_union_holds_every_engine(port: Port) -> None:
    """The per-voice union holds each of these engines (Ruling B: each engine's state bytes; the union's size against
    every engine is in test_wavetable_chord_host.py)."""
    sizes = {
        n: getattr(port, f"{n}_size") for n in ("va", "ws", "fm", "grain", "additive")
    }
    assert port.engine_union_size >= max(sizes.values()), sizes
    print("engine state bytes:", sizes, "union", port.engine_union_size)


def test_wrapper_model_change_chain(port: Port) -> None:
    """One voice through VA -> 2FM -> FORMANT -> ADDITIVE -> 2FM -> ADDITIVE -> FORMANT -> 2FM -> ASYM-TRI -> VA (every
    engine after each other engine's state where the order allows), each change made first without a note-on (the
    running engine goes on as if nothing happened) and then at a note-on (the new model equals a fresh voice of that
    model bit for bit: nothing is left of the previous engine's state in the shared union)."""
    seq = [0, 2, 3, 4, 2, 4, 3, 2, 1, 0]
    v = port.voice()
    prev = seq[0]
    ref = port.voice()
    blocks = [
        _render(port, v, frame(57, model=prev, harmonics=0x3000), int(b == 0))
        for b in range(5)
    ]
    ref_blocks = [
        _render(port, ref, frame(57, model=prev, harmonics=0x3000), int(b == 0))
        for b in range(8)
    ]
    assert all(
        np.array_equal(a, b) for a, b in zip(blocks, ref_blocks[:5], strict=True)
    )
    for model in seq[1:]:
        # MODEL moves without a note-on: the latched engine runs on (equal to the reference that never saw it)
        on = [
            _render(port, v, frame(57, model=model, harmonics=0x3000), 0)
            for _ in range(3)
        ]
        assert all(
            np.array_equal(a, b) for a, b in zip(on, ref_blocks[5:8], strict=True)
        ), model
        fresh = port.voice()
        want = [
            _render(port, fresh, frame(57, model=model, harmonics=0x3000), int(b == 0))
            for b in range(8)
        ]
        got = [
            _render(port, v, frame(57, model=model, harmonics=0x3000), int(b == 0))
            for b in range(8)
        ]
        assert all(np.array_equal(a, b) for a, b in zip(got, want, strict=True)), model
        assert any(x.any() for x in got), model
        ref_blocks = want[:5] + [
            _render(port, fresh, frame(57, model=model, harmonics=0x3000), 0)
            for _ in range(3)
        ]


def test_wrapper_models_7_to_23_are_silent(port: Port) -> None:
    """Models 5 and 6 sound since Task 9 (tests/plaits/test_wavetable_chord_host.py)."""
    for model in range(7, 24):
        v = port.voice()
        outs = [_render(port, v, frame(60, model=model), int(b == 0)) for b in range(3)]
        assert not any(o.any() for o in outs), model


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_wrapper_extremes_finite(port: Port, name: str) -> None:
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x7F00):
                        for mix in (0, 0x7F00):
                            v = port.voice()
                            f = frame(
                                note,
                                tune=tune,
                                model=MODEL[name],
                                harmonics=h,
                                timbre=t,
                                morph=m,
                                mix=mix,
                            )
                            for b in range(20):
                                o = _render(port, v, f, int(b == 0))
                                assert np.isfinite(o).all()
                                assert float(np.abs(o).max()) < 4.0
