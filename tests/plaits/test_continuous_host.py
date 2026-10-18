"""PLAITS engines run on from note to note, as in Plaits (Task 9b): the voice wrapper (modules/machines/plaits/src/dsp/
plaits.c) initialises a voice's engine only at its first note-on and when a note-on changes the model; every other
note-on leaves the engine, the DC blocker and the interpolator running.

The oracle (tests/plaits/oracle/run_oracle.cc) is upstream's engine of each model 0..6, selected as Plaits' Voice does
at an engine change (Init, LoadUserData(NULL), Reset) and then rendered continuously in sub-blocks of 16 with the
wrapper's parameter stream; Plaits' trigger (RISING_EDGE | HIGH in a note-on's first sub-block) is passed through.
The wrapper's mix, DC blocker and gain are applied in float32 as plaits.c does. Through the wrapper the same stream
must give the same output bit for bit: note-ons at different pitches with knob changes and no model change, and
model changes at note-ons (the new engine as the oracle selects it, the DC blocker cleared)."""

from __future__ import annotations

import ctypes
from pathlib import Path
from typing import Any

import numpy as np
import pytest
from support.plaits_host import EURORACK, FP, Port, build_oracle, build_port
from support.plaits_streams import Block, as_frame, retrigger_stream, switch_stream

ORACLE = Path(__file__).resolve().parent / "oracle"
Arr = Any
MODELS = range(7)
INV_FULL, INV_256 = np.float32(3.0757874e-05), np.float32(0.00390625)
DC_R, GAIN = np.float32(0.99934550), np.float32(0.601)
RISING_HIGH, LOW = 1 | 4, 0
SUB = 16


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.run_oracle_select.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.run_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            ctypes.c_int,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.size = self.lib.run_oracle_size()


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    p = build_port(tmp_path_factory.mktemp("port"))
    p.lib.hb_init.argtypes = [ctypes.c_void_p]
    p.lib.hb_up2.argtypes = [ctypes.c_void_p, FP, FP, ctypes.c_int]
    return p


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    d = tmp_path_factory.mktemp("run-oracle")
    plaits = EURORACK / "plaits"
    engine = plaits / "dsp" / "engine"
    sources = [
        ORACLE / "run_oracle.cc",
        engine / "virtual_analog_engine.cc",
        engine / "waveshaping_engine.cc",
        engine / "fm_engine.cc",
        engine / "grain_engine.cc",
        engine / "additive_engine.cc",
        engine / "wavetable_engine.cc",
        engine / "chord_engine.cc",
        plaits / "dsp" / "chords" / "chord_bank.cc",
        plaits / "resources.cc",
    ]
    return Oracle(build_oracle(d, sources, "run_oracle"))


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


def _unit(word: int) -> np.float32:
    return np.float32(
        min(
            max(np.float32(np.float32(word) * INV_FULL), np.float32(0.0)),
            np.float32(1.0),
        )
    )


def reference(oracle: Oracle, blocks: list[Block]) -> Arr:
    """Upstream's engine, selected at the first note-on and at each note-on that changes the model, rendered
    continuously; then OUT + MIX (AUX - OUT), the DC blocker (cleared at each selection) and the gain, as plaits.c."""
    st = ctypes.create_string_buffer(oracle.size)
    out16 = np.zeros(SUB, dtype=np.float32)
    aux16 = np.zeros(SUB, dtype=np.float32)
    latched = -1
    x1 = y1 = np.float32(0.0)
    res = []
    for s in blocks:
        if s["trig"] and s["model"] != latched:
            latched = s["model"]
            oracle.lib.run_oracle_select(st, latched)
            x1 = y1 = np.float32(0.0)
        assert latched >= 0
        note = np.float32(np.float32(s["note"] << 8) * INV_256) + np.float32(
            np.float32(np.float32(s["tune"]) * INV_256) - np.float32(64.0)
        )
        h, t, m, mix = (_unit(s[k]) for k in ("harmonics", "timbre", "morph", "mix"))
        blk = np.zeros(2 * SUB, dtype=np.float32)
        for sub in range(2):
            trig = RISING_HIGH if (s["trig"] and sub == 0) else LOW
            oracle.lib.run_oracle_render(
                st,
                float(note),
                float(h),
                float(t),
                float(m),
                trig,
                out16.ctypes.data_as(FP),
                aux16.ctypes.data_as(FP),
                SUB,
            )
            blk[sub * SUB : (sub + 1) * SUB] = out16 + mix * (aux16 - out16)
        for i in range(2 * SUB):
            x = blk[i]
            y1 = np.float32(np.float32(x - x1) + np.float32(DC_R * y1))
            x1 = x
            blk[i] = np.float32(y1 * GAIN)
        res.append(blk)
    return np.concatenate(res)


def up2(port: Port, x48: Arr, blocks: list[Block]) -> Arr:
    """The 96 kHz output from the 48 kHz one: hb_up2 per block, the interpolator cleared at each engine selection."""
    hb = ctypes.create_string_buffer(4096)
    out = []
    latched = -1
    for k, s in enumerate(blocks):
        if s["trig"] and s["model"] != latched:
            latched = s["model"]
            port.lib.hb_init(hb)
        src = np.ascontiguousarray(x48[32 * k : 32 * (k + 1)], dtype=np.float32)
        dst = np.zeros(64, dtype=np.float32)
        port.lib.hb_up2(hb, src.ctypes.data_as(FP), dst.ctypes.data_as(FP), 32)
        out.append(dst)
    return np.concatenate(out)


def same(a: Arr, b: Arr) -> bool:
    return bool(
        np.array_equal(np.asarray(a).view(np.uint32), np.asarray(b).view(np.uint32))
    )


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


@pytest.mark.parametrize("model", MODELS)
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
