"""Host tests for the PLAITS 48 -> 96 kHz half-band interpolator."""

import ctypes
import subprocess
from pathlib import Path
from typing import Any

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[2]
DSP = ROOT / "modules" / "machines" / "plaits" / "src" / "dsp"
FS48 = 48000.0
BLOCK = 32
Arr = Any


class Lib:
    def __init__(self, path: Path) -> None:
        self.lib = ctypes.CDLL(str(path))
        self.size = ctypes.c_int.in_dll(self.lib, "hb_state_size").value
        fp = ctypes.POINTER(ctypes.c_float)
        self.lib.hb_init.argtypes = [ctypes.c_void_p]
        self.lib.hb_up2.argtypes = [ctypes.c_void_p, fp, fp, ctypes.c_int]
        self.delay = ctypes.c_int.in_dll(self.lib, "hb_group_delay").value

    def run(self, x: Arr, block: int) -> Arr:
        st = ctypes.create_string_buffer(self.size)
        self.lib.hb_init(st)
        x = np.ascontiguousarray(x, dtype=np.float32)
        out = np.zeros(2 * len(x), dtype=np.float32)
        fp = ctypes.POINTER(ctypes.c_float)
        for i in range(0, len(x), block):
            n = min(block, len(x) - i)
            self.lib.hb_up2(
                st,
                x[i:].ctypes.data_as(fp),
                out[2 * i :].ctypes.data_as(fp),
                n,
            )
        return out


@pytest.fixture(scope="module")
def hb(tmp_path_factory: pytest.TempPathFactory) -> Lib:
    d = tmp_path_factory.mktemp("hb")
    so = d / "libhb.so"
    subprocess.run(
        [
            "cc",
            "-O2",
            "-std=c99",
            "-shared",
            "-fPIC",
            "-o",
            str(so),
            str(DSP / "halfband.c"),
        ],
        check=True,
    )
    return Lib(so)


def sine(f: float, n: int) -> Arr:
    t = np.arange(n)
    return (0.5 * np.sin(2 * np.pi * f / FS48 * t)).astype(np.float32)


def tone_amp(y: Arr, f: float, skip: int) -> float:
    """Amplitude of frequency f (Hz) in the 96 kHz signal y, via correlation."""
    y = y[skip:].astype(np.float64)
    t = np.arange(len(y))
    w = np.hanning(len(y))
    c = np.sum(y * w * np.exp(-2j * np.pi * f / (2 * FS48) * t))
    return float(2 * abs(c) / np.sum(w))


def test_passband_flat(hb: Lib) -> None:
    worst = 0.0
    for f in np.linspace(100, 20000, 60):
        y = hb.run(sine(float(f), 8192), BLOCK)
        g = 20 * np.log10(tone_amp(y, float(f), 1000) / 0.5)
        worst = max(worst, abs(g))
    assert worst <= 0.1, worst


def test_image_rejection(hb: Lib) -> None:
    worst = -999.0
    for f in np.linspace(100, 20000, 60):
        y = hb.run(sine(float(f), 8192), BLOCK)
        g = 20 * np.log10(tone_amp(y, FS48 - float(f), 1000) / 0.5 + 1e-30)
        worst = max(worst, g)
    assert worst <= -90.0, worst


def test_linear_phase(hb: Lib) -> None:
    """Constant group delay: measured phase delay equals hb_group_delay."""
    for f in (500.0, 3000.0, 8000.0, 15000.0, 20000.0):
        n = 8192
        y = hb.run(sine(f, n), BLOCK)
        skip = 1000
        yy = y[skip:].astype(np.float64)
        t = np.arange(len(yy)) + skip
        c = np.sum(yy * np.exp(-2j * np.pi * f / (2 * FS48) * t))
        # y = sin(w (t - D)) => c ~ e^{-jwD} / (2j)
        phase = float(np.angle(c * 2j))
        w = 2 * np.pi * f / (2 * FS48)
        period = 2 * FS48 / f
        d = (-phase / w) % period
        want = hb.delay % (2 * FS48 / f)
        diff = abs(d - want)
        diff = min(diff, 2 * FS48 / f - diff)
        assert diff < 0.05, (f, d, want)


def test_no_sample_duplication(hb: Lib) -> None:
    y = hb.run(sine(5000.0, 4096), BLOCK)[2000:4000]
    assert np.all(np.abs(y[1::2] - y[0::2]) > 1e-4)
    # and the interpolated output is smooth: no 2-sample plateaus
    assert np.max(np.abs(np.diff(y))) < 0.5 * 2 * np.pi * 5000 / 96000 * 1.2


def test_blockwise_equals_single_call(hb: Lib) -> None:
    rng = np.random.default_rng(1)
    x = (rng.standard_normal(2048) * 0.3).astype(np.float32)
    a = hb.run(x, BLOCK)
    b = hb.run(x, len(x))
    c = hb.run(x, 7)
    assert np.array_equal(a, b)
    assert np.array_equal(a, c)


def test_impulse_response_symmetric(hb: Lib) -> None:
    x = np.zeros(256, dtype=np.float32)
    x[0] = 1.0
    h = hb.run(x, BLOCK)
    nz = np.nonzero(np.abs(h) > 0)[0]
    last = int(nz[-1])
    assert last == 2 * hb.delay
    seg = h[: last + 1]
    assert np.allclose(seg, seg[::-1], atol=1e-7)
    assert abs(h[hb.delay] - 1.0) < 1e-6
