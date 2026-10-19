"""PLAITS engines run on from note to note, as in Plaits (Task 9b): the voice wrapper (modules/machines/plaits/src/dsp/
plaits.c) initialises a voice's engine only at its first note-on and when a note-on changes the model; every other
note-on leaves the engine, the DC blocker and the interpolator running.

The oracle (tests/plaits/oracle/run_oracle.cc) is upstream's engine of each model 0..7, 12 and 21, selected as Plaits' Voice does
at an engine change (Init, LoadUserData(NULL), Reset) and then rendered continuously in sub-blocks of 16 with the
wrapper's parameter stream; Plaits' trigger as the wrapper builds it (RISING_EDGE | HIGH in a note-on's first
sub-block, HIGH after: the gate held from note-on to note-on, plaits.c) is passed through. SPEECH (7) reads the
trigger, so its note-ons start consonants and words, and RINGS (12) strikes at each; models 0..6 and 21 do not read
it.
Plaits' output stage follows the engine as in Plaits' Voice: the model's OUT and AUX gains from upstream's voice.cc,
a negative gain through upstream's stmlib::Limiter (run_oracle_post: both limiters initialised at the first note-on, the
OUT one again at each model change). The accent is the note-on's velocity / 127, latched at the note-on.
The wrapper's mix, DC blocker and gain are applied in float32 as plaits.c does. Through the wrapper the same stream
must give the same output bit for bit: note-ons at different pitches with knob changes and no model change, and
model changes at note-ons (the new engine as the oracle selects it, the DC blocker cleared)."""

from __future__ import annotations

import ctypes
from typing import Any

import numpy as np
import pytest
from support.plaits_host import FP, Port, build_port, upstream_gains
from support.plaits_oracle import Oracle, build, reference, same, up2
from support.plaits_streams import Block, as_frame, retrigger_stream, switch_stream

Arr = Any
# the run-on models: 0..7, 12 and 21 against the oracle; a note-on of the latched model is silent to the audio for
# 0..6 and 21 (SPEECH and RINGS read the trigger); those also do not read the accent
MODELS = [*range(8), 12, 21]
TRIGGERLESS = [*range(7), 21]
INV_FULL, INV_256 = np.float32(3.0757874e-05), np.float32(0.00390625)
DC_R = np.float32(0.99934550)
RISING_HIGH, HIGH = 1 | 4, 4
SUB = 16


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    p = build_port(tmp_path_factory.mktemp("port"))
    p.lib.hb_init.argtypes = [ctypes.c_void_p]
    p.lib.hb_up2.argtypes = [ctypes.c_void_p, FP, FP, ctypes.c_int]
    return p


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    return build(tmp_path_factory.mktemp("run-oracle"))


# ---- the wrapper and the reference


def wrapper(port: Port, blocks: list[Block], mode: int = 1) -> Arr:
    v = port.voice()
    return np.concatenate(
        [port.render(v, as_frame(s), s["trig"], mode) for s in blocks]
    )


def wrapper_fresh_at_each_note(port: Port, blocks: list[Block]) -> Arr:
    """The old behaviour (every note-on re-initialised the engine, the DC blocker and the interpolator): a fresh voice
    at each note-on, which is what the wrapper of Task 9 did."""
    out, v = [], port.voice()
    for s in blocks:
        if s["trig"]:
            v = port.voice()
        out.append(port.render(v, as_frame(s), s["trig"], 1))
    return np.concatenate(out)


# ---- the oracle


@pytest.mark.parametrize("model", MODELS)
def test_retriggers_equal_upstream_rendered_continuously(
    port: Port, oracle: Oracle, model: int
) -> None:
    """Requirement 2 of Task 9b: note-ons at different pitches with knob changes and no model change equal upstream's
    engine rendered continuously, bit for bit (two streams per model); the 96 kHz build is the half-band of it."""
    for seed in (1, 2):
        blocks = retrigger_stream(model, seed)
        got = wrapper(port, blocks)
        want = reference(oracle, blocks)
        assert same(got, want), (model, seed)
        assert float(np.abs(got).max()) > 0.01, model
        assert same(wrapper(port, blocks, 0), up2(port, want, blocks)), (model, seed)


def test_model_changes_equal_upstream_reselected(port: Port, oracle: Oracle) -> None:
    """A note-on that changes the model selects the new engine as the oracle does (Init, LoadUserData(NULL), Reset) and
    clears the DC blocker; a MODEL change without a note-on and a note-on of the latched model change nothing."""
    blocks = switch_stream()
    assert any(
        s["trig"] and s["model"] == p["model"]
        for p, s in zip(blocks, blocks[1:], strict=False)
    )
    got = wrapper(port, blocks)
    want = reference(oracle, blocks)
    assert same(got, want)
    assert same(wrapper(port, blocks, 0), up2(port, want, blocks))


# ---- a note-on of the latched model is a no-op for the audio


@pytest.mark.parametrize("model", TRIGGERLESS)
@pytest.mark.parametrize("mode", [1, 0])
def test_note_on_of_the_latched_model_changes_nothing(
    port: Port, model: int, mode: int
) -> None:
    """The same control stream with note-ons and with only the first: equal output, bit for bit (no restart of the
    engine, the DC blocker or the interpolator; no sweep of any smoothed parameter back to its initial value)."""
    blocks = retrigger_stream(model, 3, 120)
    once = [{**s, "trig": int(k == 0)} for k, s in enumerate(blocks)]
    assert sum(s["trig"] for s in blocks) > 10
    assert same(wrapper(port, blocks, mode), wrapper(port, once, mode)), model


# ---- clicks and sweeps (WAVETABLE, CHORD)

CLICK_CASES = {
    "wavetable": (5, {}),
    "wavetable-corner": (5, {"harmonics": 0x7F00, "timbre": 0x7F00, "morph": 0x7F00}),
    "chord": (6, {}),
    "chord-wt": (6, {"morph": 0x7F00, "timbre": 0x7F00}),
    "chord-fade": (6, {"morph": 0x4600, "timbre": 0x7F00}),
}
HOLD = 40  # blocks per note
SETTLE = 10  # blocks after a note-on before its steady part


def click_stream(model: int, knobs: dict[str, int], notes: list[int]) -> list[Block]:
    base = {"harmonics": 0x4000, "timbre": 0x4000, "morph": 0x4000, "mix": 0, **knobs}
    return [
        {"note": n, "tune": 0x4000, "model": model, "trig": int(b == 0), **base}
        for n in notes
        for b in range(HOLD)
    ]


def click_ratios(x: Arr, notes: list[int]) -> list[float]:
    """Per retrigger k = 1..: the largest sample-to-sample step within 2 blocks after it over the largest steady step
    (from block SETTLE on) of the notes either side."""
    d = np.abs(np.diff(x.astype(np.float64), prepend=x[0]))
    out = []
    for k in range(1, len(notes)):
        on = d[32 * k * HOLD : 32 * (k * HOLD + 2)].max()
        steady = max(
            d[32 * (j * HOLD + SETTLE) : 32 * (j + 1) * HOLD].max() for j in (k - 1, k)
        )
        out.append(float(on / steady))
    return out


NOTES = [24, 24, 36, 48, 48, 60, 72, 60, 0, 0]
# The steady bound is the largest step over 30 blocks (2.6 periods of the wave at note 48): a block right after a
# retrigger of the running engine may hold a step 0.02 % above it (WAVETABLE at the corner, 48 -> 48: 1.0002x).
STEADY_SLACK = 1.01


@pytest.mark.parametrize("case", sorted(CLICK_CASES))
def test_retrigger_has_no_click(port: Port, case: str) -> None:
    """A retrigger at the same pitch, no model change: the largest sample-to-sample step in the next two blocks stays
    within the steady-state bound measured on the held notes (STEADY_SLACK), where the per-note re-initialisation of Task 9
    exceeded it (WAVETABLE by 70..640x, CHORD by up to 74x at note 0). A retrigger at another pitch: the two blocks
    equal the same pitch change without a note-on, bit for bit (the step there is upstream's response to the pitch
    change: 7.3x for WAVETABLE at a jump from note 60 to 0, measured, with or without a note-on)."""
    model, knobs = CLICK_CASES[case]
    blocks = click_stream(model, knobs, NOTES)
    x = wrapper(port, blocks)
    held = wrapper(port, [{**s, "trig": int(k == 0)} for k, s in enumerate(blocks)])
    new = click_ratios(x, NOTES)
    old = click_ratios(wrapper_fresh_at_each_note(port, blocks), NOTES)
    pairs = [f"{a}->{b}" for a, b in zip(NOTES, NOTES[1:], strict=False)]
    print(f"{case} (retrigger step / steady step):")
    print(
        "  "
        + "  ".join(
            f"{p} {o:.2f}->{n:.2f}" for p, o, n in zip(pairs, old, new, strict=True)
        )
    )
    same_pitch = [k for k in range(1, len(NOTES)) if NOTES[k] == NOTES[k - 1]]
    assert max(new[k - 1] for k in same_pitch) <= STEADY_SLACK, (case, new)
    assert max(old[k - 1] for k in same_pitch) > 1.0, (case, old)
    for k in range(1, len(NOTES)):
        win = slice(32 * k * HOLD, 32 * (k * HOLD + 2))
        assert same(x[win], held[win]), (case, k)


@pytest.mark.parametrize("model", [5, 6])
def test_retrigger_does_not_sweep_from_the_defaults(port: Port, model: int) -> None:
    """WAVETABLE's x/y/z and pitch smoothing and CHORD's TIMBRE/MORPH smoothing are not reset at a note-on: after a
    retrigger at the same pitch and knobs, the next 20 ms equal the output of a voice that was not retriggered, while
    the old per-note restart differed from it (the sweep from wave (0,0,0), CHORD's rotation through inversions)."""
    knobs = {"harmonics": 0x6000, "timbre": 0x7F00, "morph": 0x7F00}
    blocks = click_stream(model, knobs, [48, 48])
    held = [{**s, "trig": int(k == 0)} for k, s in enumerate(blocks)]
    a, b = wrapper(port, blocks), wrapper(port, held)
    win = slice(32 * HOLD, 32 * (HOLD + 30))
    assert same(a[win], b[win])
    old = wrapper_fresh_at_each_note(port, blocks)
    assert float(np.abs(old[win] - b[win]).max()) > 0.05


# ---- Plaits' output stage and the accent (Task 10b)


def test_output_gains_equal_upstream_voice_cc(port: Port) -> None:
    """The wrapper's OUT and AUX gain of every model 0..23 is the float upstream's voice.cc registers for its engine
    (model n = upstream engine (n + 8) mod 24); negative ones (SPEECH, SWARM, NOISE, DUST, STRING, RINGS) go through the
    limiter."""
    for n, (go, ga) in enumerate(upstream_gains()):
        assert np.float32(port.lib.port_out_gain(n)) == go, n
        assert np.float32(port.lib.port_aux_gain(n)) == ga, n


def loud_speech_stream() -> list[Block]:
    """SPEECH where its OUT is loudest (the naive formants at HARMONICS 0, TIMBRE and MORPH 0, high notes), with
    retriggers and a MODEL change away and back: the OUT limiter engages, releases and is reset."""
    out: list[Block] = []
    for k, (model, note) in enumerate([(7, 84), (7, 96), (0, 60), (7, 96), (7, 72)]):
        for b in range(60):
            out.append(
                {
                    "note": note,
                    "tune": 0x4000,
                    "model": model,
                    "trig": int(b in (0, 30)),
                    "harmonics": 0 if b < 45 else 0x2A00,
                    "timbre": 0,
                    "morph": 0,
                    "mix": 0x7F00 if (k == 4 and b >= 50) else 0,
                    "velocity": 127,
                }
            )
    return out


def test_limiter_engages_and_equals_upstream(port: Port, oracle: Oracle) -> None:
    """SPEECH's OUT limiter (gain -0.7) on its loudest settings: the wrapper equals upstream's engine through
    upstream's stmlib::Limiter bit for bit, and the limiter engages there: the engine's OUT times 0.7 exceeds 1, and
    the voice's RMS is under 0.8 x that of the same OUT through a plain gain of 0.7 (and the clip). The bit-exact
    comparison is the proof; the RMS check only shows that the limiter engages."""
    blocks = loud_speech_stream()
    got = wrapper(port, blocks)
    assert same(got, reference(oracle, blocks))
    assert same(wrapper(port, blocks, 0), up2(port, reference(oracle, blocks), blocks))
    # the engine's own OUT on the first note (upstream, no output stage)
    st = ctypes.create_string_buffer(oracle.size)
    oracle.lib.run_oracle_select(st, 7)
    o, a = np.zeros(SUB, dtype=np.float32), np.zeros(SUB, dtype=np.float32)
    peak = 0.0
    for b in range(80):
        oracle.lib.run_oracle_render(
            st,
            84.0,
            0.0,
            0.0,
            0.0,
            RISING_HIGH if b == 0 else HIGH,
            1.0,
            o.ctypes.data_as(FP),
            a.ctypes.data_as(FP),
            SUB,
        )
        peak = max(peak, float(np.abs(o).max()))
    plain = upstream_gains()
    plain[7] = (np.float32(0.7), plain[7][1])
    unlimited = reference(oracle, blocks, plain)

    def rms(x: Arr) -> float:
        return float(np.sqrt(np.mean(x.astype(np.float64) ** 2)))

    lim, unl = rms(got), rms(unlimited)
    print(
        f"SPEECH raw OUT peak {peak:.3f}; voice RMS limited {lim:.4f}, plain gain 0.7 (clipped) {unl:.4f}"
    )
    assert peak * 0.7 > 1.0
    assert lim < 0.8 * unl


@pytest.mark.parametrize("model", TRIGGERLESS)
def test_velocity_changes_nothing_for_models_without_accent(
    port: Port, model: int
) -> None:
    """Models 0..6 and 21 do not read Plaits' accent: the retrigger stream at velocity 0, 127 and its own mix of velocities
    gives the same output bit for bit."""
    blocks = retrigger_stream(model, 4, 120)
    base = wrapper(port, blocks)
    for vel in (0, 127):
        assert same(wrapper(port, [{**s, "velocity": vel} for s in blocks]), base), (
            model,
            vel,
        )


def test_velocity_reaches_speech(port: Port, oracle: Oracle) -> None:
    """SPEECH reads the accent (the LPC word gain): velocity changes its output, and every velocity equals upstream
    with accent velocity / 127."""
    blocks = [
        {**s, "harmonics": 0x5800, "velocity": v}
        for v in (127, 30)
        for s in retrigger_stream(7, 5, 60)
    ]
    a = wrapper(port, blocks)
    assert same(a, reference(oracle, blocks))
    b = wrapper(port, [{**s, "velocity": 100} for s in blocks])
    assert not same(a, b)


@pytest.mark.parametrize("gain", [-0.7, 0.8])
def test_output_stage_takes_nan_and_inf_as_zero(port: Port, gain: float) -> None:
    """plaits_post: a NaN or infinite sample leaves the stage as 0 (as a 0 input would), the limiter's peak stays
    finite, and the samples after it come out exactly as without the bad sample's state (the stage recovers on the
    next sample)."""
    port.lib.plaits_post.argtypes = [ctypes.c_void_p, ctypes.c_float, FP, ctypes.c_int]
    good = np.array([0.5, 1.2, 0.0, -2.0, 0.3, 0.9], dtype=np.float32)
    for bad in (np.nan, np.inf, -np.inf):
        x = good.copy()
        x[2] = bad
        pk_a, pk_b = ctypes.c_float(0.5), ctypes.c_float(0.5)
        y = good.copy()
        port.lib.plaits_post(ctypes.byref(pk_a), gain, x.ctypes.data_as(FP), len(x))
        port.lib.plaits_post(ctypes.byref(pk_b), gain, y.ctypes.data_as(FP), len(y))
        assert np.isfinite(x).all() and np.isfinite(pk_a.value), bad
        assert same(x, y) and pk_a.value == pk_b.value, bad
