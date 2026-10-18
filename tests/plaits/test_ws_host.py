"""Host tests for the PLAITS ASYM-TRI port (modules/machines/plaits/src/dsp/engines/waveshaping.c): the C99 engine
against upstream Plaits' WaveshapingEngine at 48 kHz, its tables against upstream's, and the voice wrapper's model
handling (model 1 renders ASYM-TRI; a model change takes effect at the next note-on).

The oracle (tests/plaits/oracle/ws_oracle.cc) links upstream's waveshaping_engine.cc and resources.cc from
../eurorack/plaits with the 48 kHz dsp.h override, as test_va_host.py does for VA. Both sides are built with the host
compiler without floating-point contraction, so the results must be equal bit for bit."""

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
ASYM_TRI = 1


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.ws_oracle_init.argtypes = [ctypes.c_void_p]
        self.lib.ws_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.lib.ws_oracle_lut.restype = ctypes.c_float

    def ws(
        self, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        st = ctypes.create_string_buffer(self.lib.ws_oracle_size())
        self.lib.ws_oracle_init(st)
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            self.lib.ws_oracle_render(
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
    d = tmp_path_factory.mktemp("ws-oracle")
    plaits = EURORACK / "plaits"
    sources = [
        ORACLE / "ws_oracle.cc",
        plaits / "dsp" / "engine" / "waveshaping_engine.cc",
        plaits / "resources.cc",
    ]
    return Oracle(build_oracle(d, sources, "ws_oracle"))


SETS = [
    (48.0, 0.5, 0.5, 0.5),
    (60.0, 0.0, 0.25, 0.8),
    (36.5, 1.0, 1.0, 0.0),
    (84.0, 0.3, 0.7, 0.66),
]


def test_ws_tables_equal_upstream_bit_for_bit(port: Port, oracle: Oracle) -> None:
    for k in range(8):
        n = port.lib.port_ws_lut_size(k)
        mine = np.array(
            [port.lib.port_ws_lut(k, i) for i in range(n)], dtype=np.float32
        )
        theirs = np.array(
            [oracle.lib.ws_oracle_lut(k, i) for i in range(n)], dtype=np.float32
        )
        assert mine.view(np.uint32).tolist() == theirs.view(np.uint32).tolist(), k


@pytest.mark.parametrize("k", range(len(SETS) + 1))
def test_ws_c99_matches_upstream_48k_bit_exact(
    port: Port, oracle: Oracle, k: int
) -> None:
    blocks = 4000
    if k == len(
        SETS
    ):  # every control swept block by block (the interpolators and pitch path move)
        settings = [
            (
                84.0 - 60.0 * b / blocks,
                (0.3 + 0.9 * b / blocks) % 1.0,
                (0.7 + 1.7 * b / blocks) % 1.0,
                (0.66 + 2.3 * b / blocks) % 1.0,
            )
            for b in range(blocks)
        ]
    else:
        settings = [SETS[k]] * blocks
    po, pa = port.ws(settings, 12)
    oo, oa = oracle.ws(settings, 12)
    assert np.isfinite(oo).all() and np.abs(oo).max() > 0.05
    assert np.abs(oa).max() > 0.05
    assert po.view(np.uint32).tolist() == oo.view(np.uint32).tolist()
    assert pa.view(np.uint32).tolist() == oa.view(np.uint32).tolist()


def test_ws_extreme_params_bit_exact_and_finite(port: Port, oracle: Oracle) -> None:
    """Every {0, 1}^3 corner of HARMONICS/TIMBRE/MORPH at notes -64 and 190 (the wrapper's range), 0, 127: equal to
    upstream, finite, bounded."""
    corners = [
        (n, h, t, m)
        for n in (-64.0, 0.0, 127.0, 190.0)
        for h in (0.0, 1.0)
        for t in (0.0, 1.0)
        for m in (0.0, 1.0)
    ]
    for c in corners:
        po, pa = port.ws([c] * 300, 16)
        oo, oa = oracle.ws([c] * 300, 16)
        for x in (po, pa):
            assert np.isfinite(x).all(), c
            assert float(np.abs(x).max()) < 4.0, c
        assert po.view(np.uint32).tolist() == oo.view(np.uint32).tolist(), c
        assert pa.view(np.uint32).tolist() == oa.view(np.uint32).tolist(), c


def test_ws_pitch_jumps_without_note_on_stay_finite(port: Port) -> None:
    """Jumps between the corners every block (pitch by up to 254 semitones within one block, no note-on): the slope
    oscillator's polyblep then makes huge values, where upstream reads outside its tables (undefined; it crashed the
    host build before the port kept the indices in range). The port stays finite and bounded and does not hang."""
    corners = [
        (n, h, t, m)
        for n in (-64.0, -20.0, 0.0, 127.0, 160.0, 190.0)
        for h in (0.0, 1.0)
        for t in (0.0, 1.0)
        for m in (0.0, 1.0)
    ]
    for stride in (7, 11, 13):
        jumps = [corners[(stride * b) % len(corners)] for b in range(3000)]
        po, pa = port.ws(jumps, 16)
        for x in (po, pa):
            assert np.isfinite(x).all(), stride
            assert float(np.abs(x).max()) < 4.0, stride


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


def test_ws_pitch_note69_is_440_hz_at_48k(port: Port) -> None:
    out, aux = port.ws([(69.0, 0.5, 0.3, 0.5)] * (4 * 48000 // 16), 16)
    for x in (out, aux):
        f = f0_fft(x[4800:], 440.0)
        assert abs(1200 * math.log2(f / 440.0)) < 1.0, f


def _render(port: Port, v: Any, f: Any, trig: int) -> Arr:
    return port.render(v, f, trig, 1)


def test_wrapper_model1_renders_asym_tri(port: Port) -> None:
    """MODEL 1 renders ws_render: OUT + MIX (AUX - OUT), then the DC blocker and the gain, bit for bit (the wrapper's
    arithmetic in float32: unit() is word * (1 / 0x7f00) clamped to [0, 1])."""
    inv = np.float32(3.0757874e-05)
    gain, r = np.float32(0.601), np.float32(0.99934550)
    p = float(np.float32(16384.0) * inv)
    out, aux = port.ws([(60.0, p, p, p)] * 40, 16)
    for mix in (0, 0x2000, 0x7F00):
        m = min(np.float32(np.float32(mix) * inv), np.float32(1.0))
        v = port.voice()
        got = np.concatenate(
            [
                _render(port, v, frame(60, model=ASYM_TRI, mix=mix), int(b == 0))
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
        assert got.view(np.uint32).tolist() == want.view(np.uint32).tolist(), mix
        assert float(np.abs(got).max()) > 0.05


def test_wrapper_model_change_takes_effect_at_the_next_note_on(port: Port) -> None:
    """VA -> ASYM-TRI -> VA on one voice: MODEL moves at once, the engine follows only at the next note-on, and then
    equals a fresh voice of the new model bit for bit."""
    v = port.voice()
    va = [_render(port, v, frame(57, model=0), int(b == 0)) for b in range(6)]
    # MODEL turned to 1 without a note-on: VA runs on (equal to a VA voice that never saw the change)
    ref = port.voice()
    ref_blocks = [
        _render(port, ref, frame(57, model=0), int(b == 0)) for b in range(10)
    ]
    on = [_render(port, v, frame(57, model=ASYM_TRI), 0) for _ in range(4)]
    assert all(np.array_equal(a, b) for a, b in zip(va, ref_blocks[:6], strict=True))
    assert all(np.array_equal(a, b) for a, b in zip(on, ref_blocks[6:], strict=True))
    # the note-on: ASYM-TRI from a fresh engine
    fresh = port.voice()
    want = [
        _render(port, fresh, frame(57, model=ASYM_TRI), int(b == 0)) for b in range(8)
    ]
    got = [_render(port, v, frame(57, model=ASYM_TRI), int(b == 0)) for b in range(8)]
    assert all(np.array_equal(a, b) for a, b in zip(got, want, strict=True))
    assert not np.array_equal(got[2], ref_blocks[2])
    # back to VA at a note-on: equal to a fresh VA voice
    fresh = port.voice()
    want = [_render(port, fresh, frame(57, model=0), int(b == 0)) for b in range(8)]
    got = [_render(port, v, frame(57, model=0), int(b == 0)) for b in range(8)]
    assert all(np.array_equal(a, b) for a, b in zip(got, want, strict=True))


def test_wrapper_unported_models_are_silent(port: Port) -> None:
    for model in range(5, 24):  # 2..4 are ported since Task 8 (test_fm_grain_additive_host.py)
        v = port.voice()
        outs = [_render(port, v, frame(60, model=model), int(b == 0)) for b in range(3)]
        assert not any(o.any() for o in outs), model


def test_wrapper_model1_extremes_finite(port: Port) -> None:
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
                                model=ASYM_TRI,
                                harmonics=h,
                                timbre=t,
                                morph=m,
                                mix=mix,
                            )
                            for b in range(20):
                                o = _render(port, v, f, int(b == 0))
                                assert np.isfinite(o).all()
                                assert float(np.abs(o).max()) < 4.0
