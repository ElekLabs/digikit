"""Pitch measurement shared by the patchkit proof tools."""

from __future__ import annotations

import numpy as np

FS = 48000.0


def f0_from_crossings(y: np.ndarray, f_guess: float, fs: float = FS) -> float:
    """Frequency of the fundamental: bandpass around f_guess (FFT), then a least-squares fit of the interpolated
    upward zero crossings against their index (the slope is the period)."""
    spec = np.fft.rfft(y * np.hanning(len(y)))
    f = np.fft.rfftfreq(len(y), 1 / fs)
    spec[(f < 0.7 * f_guess) | (f > 1.3 * f_guess)] = 0
    x = np.fft.irfft(spec, len(y))
    x = x[len(x) // 10 : -len(x) // 10]
    idx = np.flatnonzero((x[:-1] < 0) & (x[1:] >= 0))
    t = idx + (-x[idx]) / (x[idx + 1] - x[idx])
    slope = np.polyfit(np.arange(len(t)), t, 1)[0]
    return fs / slope
