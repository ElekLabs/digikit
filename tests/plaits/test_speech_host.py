"""Host tests for the PLAITS SPEECH port (model 7: modules/machines/plaits/src/dsp/engines/speech.c with
lib/naive_speech_synth.c, sam_speech_synth.c, lpc_speech_synth.c, svf.c and the generated lib/speech_tables.c): the
C99 engine against upstream Plaits' SpeechEngine at 48 kHz across the HARMONICS range (naive formant synth -> SAM ->
LPC vowels -> the five word banks) and every word-bank corner, with Plaits' trigger as the wrapper builds it; the
generated tables against upstream's (the word banks decoded by upstream's own Load); the table indices; the trigger
convention; and the voice wrapper's handling of model 7.

The oracle (tests/plaits/oracle/speech_oracle.cc) links upstream's speech_engine.cc and plaits/dsp/speech/*.cc from
../eurorack/plaits with the 48 kHz dsp.h override, and stmlib's random.cc (seeded with its power-up state at each
init, as the port's per-voice state). Both sides are built with the host compiler without floating-point contraction,
so the results must be equal bit for bit. Plaits' Voice sets the engine's prosody amount and speed before each render
(plaits/dsp/voice.cc): the port fixes both at 0 (the attenuverters at noon) and the oracle is given 0 likewise;
accent is 0.8 (Plaits' value with LEVEL unpatched)."""

from __future__ import annotations

import ctypes
import math
from pathlib import Path
from typing import Any

import numpy as np
import pytest
from support.plaits_host import (
    EURORACK,
    FP,
    Params,
    Port,
    build_oracle,
    build_port,
    frame,
    stmlib_root,
    tail,
)

ORACLE = Path(__file__).resolve().parent / "oracle"
FS = 48000.0
Arr = Any
MODEL = 7
LOW, RISING, UNPATCHED, HIGH = 0, 1, 2, 4
RISING_HIGH = RISING | HIGH
ACCENT = 0.8
# a step: (note, harmonics, timbre, morph, trigger)
Step = tuple[float, float, float, float, int]


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.speech_oracle_init.argtypes = [ctypes.c_void_p]
        self.lib.speech_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            ctypes.c_int,
            *[ctypes.c_float] * 3,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.lib.speech_oracle_playback_frame.argtypes = [ctypes.c_void_p]
        self.lib.speech_oracle_sam_amplitude.restype = ctypes.c_float

    def run(
        self,
        steps: list[Step],
        size: int = 16,
        prosody: float = 0.0,
        speed: float = 0.0,
        frames: list[int] | None = None,
    ) -> tuple[Arr, Arr]:
        st = ctypes.create_string_buffer(self.lib.speech_oracle_size())
        self.lib.speech_oracle_init(st)
        out = np.zeros(len(steps) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, (note, h, t, m, trig) in enumerate(steps):
            self.lib.speech_oracle_render(
                st,
                note,
                h,
                t,
                m,
                trig,
                ACCENT,
                prosody,
                speed,
                out[k * size :].ctypes.data_as(FP),
                aux[k * size :].ctypes.data_as(FP),
                size,
            )
            if frames is not None:
                frames.append(self.lib.speech_oracle_playback_frame(st))
        return out, aux


def run_port(
    port: Port, steps: list[Step], size: int = 16, accent: float = ACCENT
) -> tuple[Arr, Arr]:
    st = ctypes.create_string_buffer(port.lib.port_speech_size())
    port.lib.speech_init(st)
    out = np.zeros(len(steps) * size, dtype=np.float32)
    aux = np.zeros_like(out)
    for k, (note, h, t, m, trig) in enumerate(steps):
        p = Params(note, h, t, m, trig, accent)
        port.lib.speech_render(
            st,
            ctypes.byref(p),
            out[k * size :].ctypes.data_as(FP),
            aux[k * size :].ctypes.data_as(FP),
            size,
        )
    return out, aux


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("port"))


@pytest.fixture(scope="module")
def checked(tmp_path_factory: pytest.TempPathFactory) -> Port:
    """The port built with PL_INDEX_CHECK: every table read reports its index to port_sizes.c's recorder."""
    return build_port(tmp_path_factory.mktemp("port-idx"), ("PL_INDEX_CHECK",))


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    d = tmp_path_factory.mktemp("speech-oracle")
    plaits = EURORACK / "plaits"
    speech = plaits / "dsp" / "speech"
    stm = stmlib_root()
    sources = [
        ORACLE / "speech_oracle.cc",
        plaits / "dsp" / "engine" / "speech_engine.cc",
        speech / "naive_speech_synth.cc",
        speech / "sam_speech_synth.cc",
        speech / "lpc_speech_synth.cc",
        speech / "lpc_speech_synth_controller.cc",
        speech / "lpc_speech_synth_phonemes.cc",
        speech / "lpc_speech_synth_words.cc",
        plaits / "resources.cc",
    ]
    if stm is not None:
        sources.append(stm / "stmlib" / "utils" / "random.cc")
    return Oracle(build_oracle(d, sources, "speech_oracle"))


def same(a: Arr, b: Arr) -> bool:
    return bool(
        np.array_equal(np.asarray(a).view(np.uint32), np.asarray(b).view(np.uint32))
    )


def f32(x: float) -> np.float32:
    return np.float32(x)


# ---- HARMONICS and the word banks


# The word bank quantizer (HysteresisQuantizer2, 6 steps, not symmetric): bank b is step b + 1, centred on
# (group - 2) * 0.275 * 6 - 0.5 = b + 1, group = HARMONICS * 6.
def bank_harmonics(b: int) -> float:
    return (2.0 + (b + 1.5) / 1.65) / 6.0


NUM_FRAMES = [229, 229, 524, 604, 926]
NUM_WORDS = [7, 11, 26, 26, 22]
# HARMONICS at the regions' centres and edges: naive (group 0..1), naive -> SAM, SAM, LPC vowels -> SAM, the edges of
# the word-bank range, and each bank
REGIONS = (
    [0.0, 0.5 / 6, 1.0 / 6, 1.3 / 6, 2.0 / 6, 1.7 / 6, 2.01 / 6]
    + [bank_harmonics(b) for b in range(5)]
    + [1.0]
)


def stream(
    blocks: int,
    harmonics: Any,
    timbre: Any,
    morph: Any,
    note: Any = 60.0,
    every: int = 37,
    after: int = HIGH,
) -> list[Step]:
    """`blocks` sub-blocks; each control a constant or a function of the block; a trigger (Plaits' RISING_EDGE |
    HIGH, as the wrapper's note-on) every `every` blocks from block 0, `after` in the others."""

    def val(x: Any, b: int) -> float:
        return float(x(b)) if callable(x) else float(x)

    return [
        (
            val(note, b),
            val(harmonics, b),
            val(timbre, b),
            val(morph, b),
            RISING_HIGH if b % every == 0 else after,
        )
        for b in range(blocks)
    ]


# ---- the generated tables


def test_naive_and_sam_tables_equal_upstream(port: Port, oracle: Oracle) -> None:
    lib, o = port.lib, oracle.lib
    lib.port_naive_phoneme.restype = ctypes.c_float
    lib.port_sam_phoneme.restype = ctypes.c_float
    for p in range(5):
        for r in range(5):
            for i in range(5):
                for k in range(2):
                    assert lib.port_naive_phoneme(p, r, i, k) == float(
                        o.speech_oracle_naive_phoneme(p, r, i, k)
                    ), (p, r, i, k)
    for p in range(18):
        for i in range(3):
            assert lib.port_sam_phoneme(p, i, 0) == float(
                o.speech_oracle_sam_phoneme(p, i, 0)
            )
            amp = o.speech_oracle_sam_amplitude(o.speech_oracle_sam_phoneme(p, i, 1))
            assert f32(lib.port_sam_phoneme(p, i, 1)) == f32(amp), (p, i)


def frame_floats(f: list[int]) -> Arr:
    """What upstream's PlayFrame and BlendCoefficient compute from a Frame {energy, period, k0..k9}."""
    energy, period, *k = f
    inv = f32(0.0) if period == 0 else f32(1.0) / f32(period)
    vals = [
        inv,
        f32(energy) / f32(256.0),
        f32(k[0]) / f32(32768.0),
        f32(k[1]) / f32(32768.0),
    ]
    vals += [f32(x) / f32(128.0) for x in k[2:]]
    return np.array(vals, dtype=np.float32)


def test_lpc_phonemes_and_excitation_pulse_equal_upstream(
    port: Port, oracle: Oracle
) -> None:
    lib, o = port.lib, oracle.lib
    lib.port_lpc_phoneme.restype = ctypes.c_float
    lib.port_lpc_excitation_pulse.restype = ctypes.c_float
    for p in range(15):
        want = frame_floats([o.speech_oracle_lpc_phoneme(p, k) for k in range(12)])
        got = np.array(
            [lib.port_lpc_phoneme(p, k) for k in range(12)], dtype=np.float32
        )
        assert same(got, want), p
    for i in range(640):
        assert f32(lib.port_lpc_excitation_pulse(i)) == f32(
            f32(o.speech_oracle_excitation_pulse(i)) / f32(128.0)
        ), i


def test_word_banks_equal_upstream_load(port: Port, oracle: Oracle) -> None:
    """Each of the five banks decoded by upstream's LPCSpeechSynthWordBank::Load: the frame and word counts, every word
    boundary and every frame (as PlayFrame's floats) equal the generated tables."""
    lib, o = port.lib, oracle.lib
    lib.port_lpc_word_frame.argtypes = [ctypes.c_int, ctypes.c_int, FP]
    for b in range(5):
        o.speech_oracle_load_bank(b)
        n, w = o.speech_oracle_bank_num_frames(), o.speech_oracle_bank_num_words()
        assert (n, w) == (
            lib.port_lpc_bank_num_frames(b),
            lib.port_lpc_bank_num_words(b),
        )
        assert (n, w) == (NUM_FRAMES[b], NUM_WORDS[b])
        for i in range(w + 1):
            assert lib.port_lpc_bank_boundary(b, i) == o.speech_oracle_bank_boundary(
                i
            ), (b, i)
        got = np.zeros(12, dtype=np.float32)
        for i in range(n):
            want = frame_floats([o.speech_oracle_bank_frame(i, k) for k in range(12)])
            lib.port_lpc_word_frame(b, i, got.ctypes.data_as(FP))
            assert same(got, want), (b, i)


# ---- the engine against upstream


def test_harmonics_sweep_bit_exact(port: Port, oracle: Oracle) -> None:
    """HARMONICS up and down through the whole range (naive -> SAM -> LPC vowels -> banks 0..4 and back), TIMBRE and
    MORPH moving, the note moving, a trigger every 37 sub-blocks: OUT and AUX equal upstream's, bit for bit."""
    blocks = 9000
    steps = stream(
        blocks,
        lambda b: 1.0 - abs(1.0 - 2.0 * b / blocks),
        lambda b: (0.2 + 1.3 * b / blocks) % 1.0,
        lambda b: (0.66 + 3.1 * b / blocks) % 1.0,
        lambda b: 84.0 - 50.0 * b / blocks,
    )
    po, pa = run_port(port, steps)
    oo, oa = oracle.run(steps)
    assert np.isfinite(oo).all() and float(np.abs(oo).max()) > 0.05
    assert same(po, oo) and same(pa, oa)


@pytest.mark.parametrize("h", REGIONS)
@pytest.mark.parametrize("size", [16, 12])
def test_each_region_bit_exact(port: Port, oracle: Oracle, h: float, size: int) -> None:
    """Each region and edge of HARMONICS held, at several TIMBRE (the formant shift and LPC rate; the time stretch
    below 0.4 and above 0.6) and MORPH values and notes, triggered every 23 sub-blocks; in sub-blocks of 16 (the
    wrapper's) and 12 (Plaits')."""
    for t, m, note in (
        (0.5, 0.5, 60.0),
        (0.0, 0.1, 36.0),
        (1.0, 0.9, 96.0),
        (0.37, 0.63, 48.5),
    ):
        steps = stream(1500, h, t, m, note, every=23)
        po, pa = run_port(port, steps, size)
        oo, oa = oracle.run(steps, size)
        assert np.isfinite(oo).all()
        assert same(po, oo) and same(pa, oa), (h, t, m, note)


def word_corners(b: int) -> list[float]:
    """MORPH at each word's centre and either side of each word boundary, and at 0 and 1."""
    n = NUM_WORDS[b]
    eps = 1e-4
    vals = {0.0, 1.0}
    for k in range(n):
        vals.add((k + 0.5) / n)
        vals.add(max(0.0, k / n - eps))
        vals.add(min(1.0, k / n + eps))
    return sorted(vals)


@pytest.mark.parametrize("b", range(5))
def test_every_word_bank_corner_bit_exact_and_in_range(
    checked: Port, oracle: Oracle, b: int
) -> None:
    """Bank b: every word (MORPH at its centre and either side of each boundary, 0 and 1) triggered and played to its
    end and 30 frames past it (the controller holds the last frame), at TIMBRE 0.5 (no time stretch) and at the
    corners 0 and 1 (stretched and rate extremes) for 300 sub-blocks, notes 36/60/96: bit for bit with upstream; and
    every table index inside its table: word frames in 0..num_frames - 1, words in 0..num_words - 1, the end boundary
    at most num_words, the excitation pulse in 0..639."""
    lib = checked.lib
    h = bank_harmonics(b)
    bounds = [lib.port_lpc_bank_boundary(b, i) for i in range(NUM_WORDS[b] + 1)]
    lib.port_index_reset()
    for m in word_corners(b):
        w = min(int(np.float32(m) * np.float32(NUM_WORDS[b])), NUM_WORDS[b] - 1)
        frames = bounds[w + 1] - bounds[w]
        full = (frames + 30) * 1200 // 16 + 2
        for t, note, blocks in ((0.5, 60.0, full), (0.0, 36.0, 300), (1.0, 96.0, 300)):
            steps = stream(blocks, h, t, m, note, every=blocks + 1)
            po, pa = run_port(checked, steps)
            oo, oa = oracle.run(steps)
            assert np.isfinite(oo).all()
            assert same(po, oo) and same(pa, oa), (b, m, t, note)
    assert (lib.port_index_lo(16), lib.port_index_hi(16)) == (b, b)
    assert lib.port_index_lo(13) >= 0 and lib.port_index_lo(14) >= 0
    assert (
        lib.port_index_hi(13) == NUM_FRAMES[b] - 1
    )  # the last word plays to the bank's last frame
    assert (lib.port_index_lo(15), lib.port_index_hi(15)) == (0, NUM_WORDS[b] - 1)
    assert lib.port_index_lo(17) == 0  # the last word's end boundary is num_words
    assert lib.port_index_lo(11) >= 0 and lib.port_index_hi(11) <= 639
    print(
        f"bank {b}: frames {lib.port_index_lo(13)}..{lib.port_index_hi(13)} of {NUM_FRAMES[b]}, "
        f"words {lib.port_index_lo(15)}..{lib.port_index_hi(15)} of {NUM_WORDS[b]}, "
        f"pulse {lib.port_index_lo(11)}..{lib.port_index_hi(11)}"
    )


def test_corner_indices_naive_sam_lpc_vowels(checked: Port, oracle: Oracle) -> None:
    """The {0, 1} corners of TIMBRE and MORPH (and 0.5) in each region below the word banks, with triggers, notes
    -64..190 (the wrapper's range): bit for bit and finite, and the naive rows in 0..4, the SAM phoneme in 0..17 (17 is
    upstream's guard entry, read only as the second of an interpolated pair), its sine index in 0..511, the LPC vowel
    and consonant frames in 0..14."""
    lib = checked.lib
    lib.port_index_reset()
    for h in (0.0, 0.5 / 6, 1.0 / 6, 1.5 / 6, 2.0 / 6):
        for t in (0.0, 0.5, 1.0):
            for m in (0.0, 0.5, 1.0):
                for note in (-64.0, 0.0, 60.0, 127.0, 190.0):
                    steps = stream(120, h, t, m, note, every=17)
                    po, pa = run_port(checked, steps)
                    oo, oa = oracle.run(steps)
                    assert np.isfinite(po).all() and np.isfinite(pa).all()
                    assert same(po, oo) and same(pa, oa), (h, t, m, note)
    assert (lib.port_index_lo(7), lib.port_index_hi(7)) == (0, 4)
    assert (lib.port_index_lo(8), lib.port_index_hi(8)) == (0, 4)
    assert lib.port_index_lo(9) == 0 and lib.port_index_hi(9) <= 17
    assert lib.port_index_lo(10) >= 0 and lib.port_index_hi(10) <= 511
    # (the consonant a trigger starts lasts 75 sub-blocks, longer than the 17 between triggers: frames 5..14 here)
    assert lib.port_index_lo(12) >= 0 and lib.port_index_hi(12) <= 14
    assert lib.port_index_n(13) == 0  # no word bank frame below the word-bank range


def test_free_running_words_bit_exact(port: Port, oracle: Oracle) -> None:
    """With TRIGGER_UNPATCHED (not what the wrapper passes; Plaits without a TRIG cable) the controller scans the
    word bank with MORPH: still bit for bit with upstream."""
    for b in range(5):
        steps = [
            (60.0, bank_harmonics(b), 0.5, (k / 2000) % 1.0, UNPATCHED)
            for k in range(2000)
        ]
        po, pa = run_port(port, steps)
        oo, oa = oracle.run(steps)
        assert same(po, oo) and same(pa, oa), b


# ---- the trigger convention


def test_trigger_high_or_low_after_the_edge_is_the_same_to_speech(
    port: Port, oracle: Oracle
) -> None:
    """Plaits' Voice keeps TRIGGER_HIGH while the gate is high (voice.cc). SPEECH reads the trigger only as
    `== RISING_EDGE`, `& RISING_EDGE` and `& UNPATCHED`, so after the rising edge HIGH and LOW give the same output, in
    every region: holding the gate high from note-on to note-on (what the wrapper does) behaves as Plaits on a held
    gate and on a short one."""
    blocks = 6000
    for after in (HIGH, LOW):
        steps = stream(
            blocks,
            lambda b: 1.0 - abs(1.0 - 2.0 * b / blocks),
            lambda b: (0.3 + 1.1 * b / blocks) % 1.0,
            lambda b: (0.2 + 2.3 * b / blocks) % 1.0,
            every=41,
            after=after,
        )
        if after == HIGH:
            ref = oracle.run(steps)
        else:
            low = oracle.run(steps)
            assert same(ref[0], low[0]) and same(ref[1], low[1])
            po, pa = run_port(port, steps)
            assert same(po, low[0]) and same(pa, low[1])


def test_rising_edge_alone_would_differ_from_plaits(oracle: Oracle) -> None:
    """A trigger of RISING_EDGE without HIGH (which Plaits' patched trigger never is) would start the naive synth's
    click and SAM's consonant, which never sound in Plaits: in the naive and SAM regions the output differs from
    RISING_EDGE | HIGH; in the word banks (which read `& RISING_EDGE`) it does not. So the wrapper passes HIGH with the
    edge."""
    for h, differs in (
        (0.0, True),
        (0.5 / 6, True),
        (1.5 / 6, True),
        (bank_harmonics(2), False),
    ):
        a = stream(400, h, 0.5, 0.4, every=50)
        b = [(n, hh, t, m, RISING if g == RISING_HIGH else g) for n, hh, t, m, g in a]
        oa, ob = oracle.run(a)[0], oracle.run(b)[0]
        assert (not same(oa, ob)) == differs, h


def test_unpatched_would_free_run(oracle: Oracle) -> None:
    """TRIGGER_UNPATCHED (Plaits without a TRIG cable) never starts a word: the controller scans the bank with MORPH.
    The wrapper never passes it."""
    a = stream(600, bank_harmonics(1), 0.5, 0.3, every=200)
    b = [(n, h, t, m, UNPATCHED) for n, h, t, m, _ in a]
    assert not same(oracle.run(a)[0], oracle.run(b)[0])


# ---- prosody and speed


def test_prosody_and_speed_are_plaits_defaults(port: Port, oracle: Oracle) -> None:
    """The port's prosody amount and speed are 0 (Plaits' Voice with its FM and MORPH attenuverters at noon): equal to
    the oracle given 0 every block, and different from prosody 1 (the words' own intonation) and speed 0.5 (faster
    words)."""
    steps = stream(3000, bank_harmonics(3), 0.5, 0.45, every=1000)
    po, _ = run_port(port, steps)
    assert same(po, oracle.run(steps, prosody=0.0, speed=0.0)[0])
    assert not same(po, oracle.run(steps, prosody=1.0)[0])
    assert not same(po, oracle.run(steps, speed=0.5)[0])


# ---- the one place the port differs: a word's frame index in the vowel table


def test_word_then_lpc_vowels_without_trigger_is_defined(
    checked: Port, oracle: Oracle
) -> None:
    """A word triggered at bank 3, then HARMONICS moved into the LPC vowel range with no new trigger. Upstream reads
    LPCSpeechSynthController::phonemes_[playback_frame_] past its 15 entries (playback_frame_ is the word's frame
    index); the port ends the playback instead (back to scan mode, lib/lpc_speech_synth.c). Equal to upstream up to
    the move; after it finite, and every vowel or consonant frame read inside the table."""
    lib = checked.lib
    move = 200
    steps: list[Step] = [
        (
            60.0,
            bank_harmonics(3) if k < move else 1.5 / 6,
            0.5,
            0.55,
            RISING_HIGH if k == 0 else HIGH,
        )
        for k in range(800)
    ]
    frames: list[int] = []
    oo, _ = oracle.run(steps, frames=frames)
    assert (
        frames[move - 1] >= 15
    )  # upstream's index at the move is a word frame, outside phonemes_
    lib.port_index_reset()
    po, pa = run_port(checked, steps)
    assert same(po[: 16 * move], oo[: 16 * move])
    assert np.isfinite(po).all() and np.isfinite(pa).all()
    assert float(np.abs(po).max()) < 4.0
    assert lib.port_index_hi(12) <= 14


# ---- pitch


def f0_fft(x: Arr, guess: float) -> float:
    pad = 1 << int(math.ceil(math.log2(len(x) * 4)))
    win = np.hanning(len(x))
    sp = np.abs(np.fft.rfft((x - x.mean()) * win, pad))
    fr = np.fft.rfftfreq(pad, 1 / FS)
    lo, hi = np.searchsorted(fr, guess * 0.8), np.searchsorted(fr, guess * 1.25)
    i = lo + int(np.argmax(sp[lo:hi]))
    a, b, c = (math.log(sp[j] + 1e-30) for j in (i - 1, i, i + 1))
    off = 0.5 * (a - c) / (a - 2 * b + c)
    return float((i + off) * FS / pad)


@pytest.mark.parametrize("h", [0.0, 1.5 / 6])
def test_pitch_note69_at_48k(port: Port, h: float) -> None:
    """Note 69 is 440 Hz within 1 cent: the naive synth's glottal pulse (AUX) and the LPC vowels (prosody 0: the
    pulse follows the note)."""
    steps = [
        (69.0, h, 0.5, 0.2, RISING_HIGH if k == 0 else HIGH) for k in range(4 * 3000)
    ]
    _, aux = run_port(port, steps)
    f = f0_fft(aux[48000:], 440.0)
    assert abs(1200 * math.log2(f / 440.0)) < 1.0, f


# ---- the voice wrapper


def wrapper_trigger(trig: int, sub: int) -> int:
    return RISING_HIGH if (trig and sub == 0) else HIGH


def test_wrapper_renders_the_model(port: Port) -> None:
    """MODEL 7 renders speech_render with Plaits' trigger as the wrapper builds it (RISING_EDGE | HIGH in a note-on's
    first sub-block, HIGH after) and the accent of the note-on's velocity (frame() default 100: 0x6400 / 0x7f00), then
    Plaits' output stage (OUT through the limiter at 0.7), OUT + MIX (AUX - OUT), the DC blocker and the gain, bit for
    bit; a retrigger reaches the engine without re-initialising it."""
    inv = np.float32(3.0757874e-05)
    accent = float(np.float32(np.float32(100 << 8) * inv))
    for hw in (0x0800, 0x2800, 0x4000, 0x7000):
        hv = float(np.float32(np.float32(hw) * inv))
        tv = float(np.float32(np.float32(0x3000) * inv))
        mv = float(np.float32(np.float32(0x5000) * inv))
        trigs = [int(b in (0, 37, 80)) for b in range(120)]
        steps = [
            (60.0, hv, tv, mv, wrapper_trigger(t, sub))
            for t in trigs
            for sub in range(2)
        ]
        out, aux = run_port(port, steps, accent=accent)
        for mix in (0, 0x2000, 0x7F00):
            m = min(np.float32(np.float32(mix) * inv), np.float32(1.0))
            v = port.voice()
            got = np.concatenate(
                [
                    port.render(
                        v,
                        frame(
                            60,
                            model=MODEL,
                            harmonics=hw,
                            timbre=0x3000,
                            morph=0x5000,
                            mix=mix,
                        ),
                        t,
                        1,
                    )
                    for t in trigs
                ]
            )
            want = tail(MODEL, out, aux, m)
            assert same(got, want), (hw, mix)
            assert float(np.abs(got).max()) > 0.01


def test_engine_union_holds_speech(port: Port) -> None:
    """The per-voice union is the size of its largest engine (Ruling B: each engine's state bytes)."""
    names = ("va", "ws", "fm", "grain", "additive", "wavetable", "chord", "speech")
    sizes = {n: getattr(port, f"{n}_size") for n in names}
    assert port.engine_union_size == max(sizes.values()), sizes
    print(
        "engine state bytes:",
        sizes,
        "union",
        port.engine_union_size,
        "voice",
        port.voice_size,
    )


def test_wrapper_model_change_chain_through_speech(port: Port) -> None:
    """One voice VA -> SPEECH -> CHORD -> SPEECH -> WAVETABLE -> SPEECH -> VA, each change at a note-on: from the note-on
    on, equal to a fresh voice of the new model bit for bit (nothing of the previous engine left in the union)."""
    seq = [0, 7, 6, 7, 5, 7, 0]
    slots = {"harmonics": 0x5800, "morph": 0x4400, "timbre": 0x3000}
    v = port.voice()
    for model in seq:
        fresh = port.voice()
        want = [
            port.render(fresh, frame(57, model=model, **slots), int(b == 0), 1)
            for b in range(60)
        ]
        got = [
            port.render(v, frame(57, model=model, **slots), int(b == 0), 1)
            for b in range(60)
        ]
        assert all(np.array_equal(a, b) for a, b in zip(got, want, strict=True)), model
        assert any(x.any() for x in got), model


def test_wrapper_unported_models_8_to_23_are_silent(port: Port) -> None:
    for model in (m for m in range(8, 24) if m not in (12, 21)):
        v = port.voice()
        outs = [
            port.render(v, frame(60, model=model), int(b == 0), 1) for b in range(3)
        ]
        assert not any(o.any() for o in outs), model


BOUND = 8.0


def test_wrapper_extremes_finite(port: Port) -> None:
    """Every corner of the wrapper's controls (notes 0 and 127, TUNE 0 / 0x4000 / 0x7f00, HARMONICS across the
    regions, TIMBRE and MORPH 0 / 1, MIX 0 / 1), retriggered: finite and bounded."""
    peak = 0.0
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x1500, 0x2A00, 0x3800, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x7F00):
                        for mix in (0, 0x7F00):
                            v = port.voice()
                            f = frame(
                                note,
                                tune=tune,
                                model=MODEL,
                                harmonics=h,
                                timbre=t,
                                morph=m,
                                mix=mix,
                            )
                            for b in range(40):
                                o = port.render(v, f, int(b % 13 == 0), 1)
                                assert np.isfinite(o).all()
                                peak = max(peak, float(np.abs(o).max()))
    print(f"SPEECH extremes peak {peak:.3f}")
    assert peak < BOUND
