"""digikit/pluck tuning, notes 24 to 100 (the test_pluck.py rig)."""

import numpy as np
import pytest
from pluck_rig import SKIP_NO_SELACHE, PluckRig, run_blocks

pytestmark = SKIP_NO_SELACHE


def f0_from_crossings(y: np.ndarray, f_guess: float, fs: float = 48000.0) -> float:
    """Frequency of the fundamental: bandpass around f_guess (FFT), then a least-squares fit of the interpolated
    upward zero crossings against their index (the slope is the period)."""
    spec = np.fft.rfft(y * np.hanning(len(y)))
    f = np.fft.rfftfreq(len(y), 1 / fs)
    spec[(f < 0.7 * f_guess) | (f > 1.3 * f_guess)] = 0
    x = np.fft.irfft(spec, len(y))
    x = x[len(x) // 10 : -len(x) // 10]
    idx = np.flatnonzero((x[:-1] < 0) & (x[1:] >= 0))
    t = idx + (-x[idx]) / (x[idx + 1] - x[idx])
    k = np.arange(len(t))
    slope = np.polyfit(k, t, 1)[0]
    return fs / slope


@pytest.mark.parametrize("note", [24, 36, 48, 60, 72, 84, 96, 100])
def test_the_string_is_in_tune(note):
    """The fundamental of the rendered string, from the phase slope of the first partial, within the model's
    analytic error (< 0.2 cents up to note 100) plus the measurement's."""
    rig = PluckRig()
    want = 440.0 * 2 ** ((note - 69) / 12)
    period = 48000.0 / want
    n_blocks = int(np.ceil(max(3200, 40 * period) / 32)) + 4
    y, _ = run_blocks(
        rig, n_blocks, note=note, decay=0x7F00, bright=0x7F00, color=0x7000, pick=0
    )
    y = y[int(4 * period) :].astype(np.float64)
    got = f0_from_crossings(y, want)
    cents = 1200 * np.log2(got / want)
    assert abs(cents) < 1.5, (note, got, want, cents)
