"""Host tests for the PLAITS VA port (modules/machines/plaits/src/dsp): the C99 engine against upstream Plaits.

The oracle (tests/plaits/oracle/va_oracle.cc) links upstream's VirtualAnalogEngine from ../eurorack/plaits, with
tests/plaits/oracle/include earlier on the include path: its plaits/dsp/dsp.h is upstream's with kCorrectedSampleRate
= 48000 (the Digitakt runs at exactly 48 kHz). Upstream's stmlib is a git submodule of ../eurorack (commit e3bd7c9);
when that checkout is empty, set PLAITS_STMLIB to a directory holding a `stmlib/` checkout of the same commit.
Without either the oracle tests skip; PLAITS_ORACLE_REQUIRED=1 makes that a failure.
Both sides are built with the host compiler without floating-point contraction, so the results must be equal bit
for bit."""

from __future__ import annotations

import ctypes
import math
import os
import subprocess
from pathlib import Path
from typing import Any, NoReturn

import numpy as np
import pytest
from support.plaits_host import FLAGS, FP, Frame, Port, build_port

ROOT = Path(__file__).resolve().parents[2]
ORACLE = Path(__file__).resolve().parent / "oracle"
EURORACK = ROOT.parent / "eurorack"
FS = 48000.0
Arr = Any


def stmlib_root() -> Path | None:
    for cand in (os.environ.get("PLAITS_STMLIB"), str(EURORACK)):
        if cand and (Path(cand) / "stmlib" / "stmlib.h").exists():
            return Path(cand)
    return None


def oracle_missing(why: str) -> NoReturn:
    """Skip the oracle tests, or fail them under PLAITS_ORACLE_REQUIRED=1 (a gate that must run the oracle)."""
    if os.environ.get("PLAITS_ORACLE_REQUIRED") == "1":
        pytest.fail(f"PLAITS_ORACLE_REQUIRED=1: {why}")
    pytest.skip(why)


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.oracle_a0.restype = ctypes.c_float
        self.lib.oracle_init.argtypes = [ctypes.c_void_p]
        self.lib.oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            FP,
            FP,
            ctypes.c_int,
        ]

    def va(
        self, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        st = ctypes.create_string_buffer(self.lib.oracle_size())
        self.lib.oracle_init(st)
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            self.lib.oracle_render(
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
    stm = stmlib_root()
    if stm is None or not (EURORACK / "plaits" / "dsp" / "engine").exists():
        why = (
            "BIT-EXACT ORACLE DISABLED: upstream stmlib not found (../eurorack/stmlib is empty). Enable it with "
            "`git -C ../eurorack submodule update --init stmlib` (commit e3bd7c9) or "
            "PLAITS_STMLIB=<directory holding a stmlib/ checkout at e3bd7c9, the commit ../eurorack pins>, "
            "e.g. PLAITS_STMLIB=../mutable_instruments_eurorack; ../eurorack/plaits is needed too"
        )
        oracle_missing(why)
    d = tmp_path_factory.mktemp("oracle")
    inc = d / "stm"
    inc.mkdir()
    (inc / "stmlib").symlink_to(stm / "stmlib")
    so = d / "libva_oracle.so"
    srcs = [
        ORACLE / "va_oracle.cc",
        EURORACK / "plaits" / "dsp" / "engine" / "virtual_analog_engine.cc",
        stm / "stmlib" / "dsp" / "units.cc",
    ]
    subprocess.run(
        [
            "c++",
            "-std=c++14",
            *FLAGS,
            f"-I{ORACLE / 'include'}",
            f"-I{EURORACK}",
            f"-I{inc}",
            "-o",
            str(so),
            *map(str, srcs),
        ],
        check=True,
    )
    return Oracle(so)


SETS = [
    (48.0, 0.5, 0.5, 0.5),
    (60.0, 0.0, 0.25, 0.8),
    (36.5, 1.0, 1.0, 0.0),
    (84.0, 0.3, 0.7, 0.66),
]


def test_oracle_uses_the_48k_reference(oracle: Oracle) -> None:
    assert oracle.lib.oracle_a0() == np.float32(55.0) / np.float32(48000.0)


@pytest.mark.parametrize("k", range(len(SETS)))
def test_va_c99_matches_upstream_48k_bit_exact(
    port: Port, oracle: Oracle, k: int
) -> None:
    blocks = 4000
    if (
        k == 3
    ):  # the last set sweeps every control block by block (the interpolators and pitch path move)
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
    po, pa = port.va(settings, 12)
    oo, oa = oracle.va(settings, 12)
    assert np.isfinite(oo).all() and np.abs(oo).max() > 0.05
    assert float(np.max(np.abs(po - oo))) == 0.0
    assert float(np.max(np.abs(pa - oa))) == 0.0


def f0_fft(x: Arr, guess: float) -> float:
    """The frequency of the strongest peak near `guess` (Hz), Hann window, parabolic interpolation in dB."""
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


def test_va_pitch_note69_is_440_hz_at_48k(port: Port) -> None:
    # HARMONICS 0.5: no detune between the two oscillators; TIMBRE 0: no sync; 4 s of audio
    out, _ = port.va([(69.0, 0.5, 0.0, 0.5)] * (4 * 48000 // 16), 16)
    f = f0_fft(out[4800:], 440.0)
    assert abs(1200 * math.log2(f / 440.0)) < 1.0, f


def test_va_extreme_params_finite(port: Port) -> None:
    for note in (0.0, 127.0):
        for h in (0.0, 1.0):
            for t in (0.0, 1.0):
                for m in (0.0, 1.0):
                    out, aux = port.va([(note, h, t, m)] * 300, 16)
                    for x in (out, aux):
                        assert np.isfinite(x).all(), (note, h, t, m)
                        assert float(np.abs(x).max()) < 4.0, (note, h, t, m)


def test_wrapper_dc_blocker_removes_offset(port: Port) -> None:
    st = (ctypes.c_float * 2)(0.0, 0.0)
    x = np.ones(int(0.5 * FS), dtype=np.float32)
    for i in range(0, len(x), 32):
        port.lib.plaits_dc_block(st, x[i:].ctypes.data_as(FP), min(32, len(x) - i))
    assert x[0] == 1.0
    assert abs(float(x[-1])) < 1e-4
    # a 5 Hz corner: a 1 kHz tone passes unchanged within 0.01 dB
    t = np.arange(int(FS))
    y = np.sin(2 * np.pi * 1000 * t / FS).astype(np.float32)
    st2 = (ctypes.c_float * 2)(0.0, 0.0)
    port.lib.plaits_dc_block(st2, y.ctypes.data_as(FP), len(y))
    rms_in = math.sqrt(0.5)
    rms = float(np.sqrt(np.mean(y[24000:].astype(np.float64) ** 2)))
    assert abs(20 * math.log10(rms / rms_in)) < 0.01


def _frame(note: int, tune: int = 0x4000, model: int = 0, active: int = 1) -> Frame:
    return Frame(
        float(note << 8),
        float(tune),
        float(model << 8),
        16384.0,
        16384.0,
        16384.0,
        0.0,
        active,
    )


def test_wrapper_renders_va_after_a_trigger_only(port: Port) -> None:
    v = ctypes.create_string_buffer(port.voice_size)
    port.lib.plaits_voice_clear(v)
    out = np.zeros(32, dtype=np.float32)
    f = _frame(60)
    assert (
        port.lib.plaits_render(v, ctypes.byref(f), 0, out.ctypes.data_as(FP), 32, 48000)
        == 0
    )
    assert not out.any()
    assert (
        port.lib.plaits_render(v, ctypes.byref(f), 1, out.ctypes.data_as(FP), 32, 48000)
        == 1
    )
    assert out.any()
    # ACTIVE cleared (the stock renderer ended the voice while the type was away): silent until the next trigger
    f0 = _frame(60, active=0)
    assert (
        port.lib.plaits_render(
            v, ctypes.byref(f0), 0, out.ctypes.data_as(FP), 32, 48000
        )
        == 0
    )
    assert not out.any()
    # a model without an engine is silent; the model is latched at the trigger
    f7 = _frame(60, model=7)
    assert (
        port.lib.plaits_render(
            v, ctypes.byref(f7), 1, out.ctypes.data_as(FP), 32, 48000
        )
        == 0
    )
    assert (
        port.lib.plaits_render(v, ctypes.byref(f), 0, out.ctypes.data_as(FP), 32, 48000)
        == 0
    )
    assert (
        port.lib.plaits_render(v, ctypes.byref(f), 1, out.ctypes.data_as(FP), 32, 48000)
        == 1
    )


def test_wrapper_96k_is_the_48k_block_interpolated(port: Port) -> None:
    a = ctypes.create_string_buffer(port.voice_size)
    b = ctypes.create_string_buffer(port.voice_size)
    for v in (a, b):
        port.lib.plaits_voice_clear(v)
    f = _frame(57)
    o48 = np.zeros(32 * 40, dtype=np.float32)
    o96 = np.zeros(64 * 40, dtype=np.float32)
    for k in range(40):
        trig = 1 if k == 0 else 0
        port.lib.plaits_render(
            a, ctypes.byref(f), trig, o48[32 * k :].ctypes.data_as(FP), 32, 48000
        )
        port.lib.plaits_render(
            b, ctypes.byref(f), trig, o96[64 * k :].ctypes.data_as(FP), 64, 96000
        )
    # the odd 96 kHz samples are the 48 kHz samples delayed by the half-band's 21 input samples
    assert np.array_equal(o96[1::2][21:], o48[: len(o48) - 21])


def test_missing_oracle_fails_when_required(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PLAITS_ORACLE_REQUIRED", "1")
    with pytest.raises(pytest.fail.Exception, match="PLAITS_ORACLE_REQUIRED=1"):
        oracle_missing("no stmlib")
    monkeypatch.delenv("PLAITS_ORACLE_REQUIRED")
    with pytest.raises(pytest.skip.Exception):
        oracle_missing("no stmlib")
