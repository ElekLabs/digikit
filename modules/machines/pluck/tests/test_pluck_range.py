"""digikit/pluck with every note and TUNE word: the loop period is clamped, the string never leaves the finite range.

On the Digitakt II an LFO on TUNE silenced every track: the loop period P = fs / f0 left what the delay line can serve
(about P < 2.5 or P > 4091.5), the fractional-delay fraction left [0.5, 1.5), the allpass coefficient passed the stability
limit and the string state held Inf/NaN until the next note-on (docs/findings/51-pluck-tune-guard.md). The routine now clamps x, P - tau
and a. This file checks it three ways: the coefficients of the float32 model over a dense grid of note and TUNE words
(cheap, exhaustive), 200 blocks of the model's string for every note 0 .. 127 and six TUNE words, and the routine itself
(concrete SHARC+ core) on the corners, against the model. Skips the routine part when Selache is not built."""

import numpy as np
import pluck_ref
import pytest
from pluck_rig import SKIP_NO_SELACHE, PluckRig, model_blocks, val

TUNES = (0, 0x1000, 0x4000, 0x7C00, 0x7F00, 0xFFFF)
BOUND = 2.0  # the excitation peaks near 1.5 at velocity 127, color 0x7f00; a string only loses energy


def test_the_loop_period_is_servable_for_every_note_and_tune_word():
    for bright in (0, 0x7F00):
        for note in range(128):
            for tune in (*range(0, 0x10000, 0x100), 0x7F00, 0xFFFF):
                q = pluck_ref.Params(note << 8, tune, 0x4000, 0, bright, 0, 100 << 8)
                assert 2 <= q.N <= 4090, (note, tune, q.N)
                r = float(q.P - q.tau)
                r = min(max(r, 2.501), 4091.49) - q.N
                assert 0.5 <= r < 1.5, (note, tune, r)
                assert abs(float(q.a)) <= 0.9, (note, tune, q.a)
                assert np.isfinite(q.g) and 0.0 <= float(q.g) <= 1.0, (note, tune, q.g)
                assert np.isfinite(q.inc) and np.isfinite(q.w2)


@pytest.mark.parametrize("tune", TUNES)
def test_the_string_stays_finite_and_bounded_for_200_blocks(tune):
    for note in range(128):
        q = pluck_ref.Params(note << 8, tune, 0x7F00, 0, 0x7F00, 0x7F00, 127 << 8)
        s = pluck_ref.Pluck()
        s.note_on(q)
        y = np.concatenate([s.block(q) for _ in range(200)])
        assert np.all(np.isfinite(y)), (note, tune)
        assert float(np.max(np.abs(y))) < BOUND, (note, tune)
        assert np.all(np.isfinite(s.line)) and np.isfinite(s.lp) and np.isfinite(s.ap)


@SKIP_NO_SELACHE
@pytest.mark.parametrize(
    "note,tune",
    [(0, 0), (60, 0x0400), (100, 0x7C00), (127, 0x7F00), (127, 0xFFFF)],
    ids=["note0", "low-tune", "note100-high-tune", "note127", "note127-max-word"],
)
def test_the_routine_stays_finite_and_matches_the_model_at_the_corners(note, tune):
    """The cases that made the unclamped routine hold Inf/NaN. Every output sample and the string state
    (lp', ap') is finite (the rig fills the line's unwritten part with NaN on purpose, so it is not checked), so no later block can inherit a bad state."""
    rig = PluckRig()
    kw = {"note": note, "tune": tune / 256.0}
    rig.setup(**kw)
    got = []
    for _ in range(8):
        y, out = rig.block()
        got.append(y)
        assert np.all(np.isfinite(y))
        state = [val(w) for w in out["state"][3:5]]
        assert np.all(np.isfinite(state)), state
    got = np.concatenate(got)
    want = model_blocks(8, **kw)
    assert float(np.max(np.abs(got - want))) < 2e-5 * max(
        1.0, float(np.max(np.abs(want)))
    )
