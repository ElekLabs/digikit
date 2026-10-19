"""Host tests for the PLAITS WAVETABLE (model 5) and CHORD (model 6) ports
(modules/machines/plaits/src/dsp/engines/wavetable.c, chord.c, with lib/wavetable_oscillator.c,
string_synth_oscillator.c, chord_bank.c and the generated lib/waves.c, lib/chord_bank_tables.h and
engines/chord_tables.h): each C99 engine against upstream Plaits' WavetableEngine and ChordEngine at 48 kHz, the
generated tables against upstream's, the table indices at every bank, row and column corner, and the voice wrapper's
handling of models 5 and 6.

The oracle (tests/plaits/oracle/wtc_oracle.cc) links upstream's wavetable_engine.cc, chord_engine.cc, chord_bank.cc
and resources.cc from ../eurorack/plaits with the 48 kHz dsp.h override, as test_fm_grain_additive_host.py does for
models 2..4. Both sides are built with the host compiler without floating-point contraction, so the results must be
equal bit for bit."""

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
    Port,
    build_oracle,
    build_port,
    frame,
    tail,
)

ORACLE = Path(__file__).resolve().parent / "oracle"
FS = 48000.0
Arr = Any
ENGINES = {"wavetable": 0, "chord": 1}
MODEL = {"wavetable": 5, "chord": 6}
IP = ctypes.POINTER(ctypes.c_int)


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.wtc_oracle_init.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.wtc_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.lib.wtc_oracle_wave.restype = ctypes.c_float
        self.lib.wtc_oracle_wave_map.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.wtc_oracle_chord_ratio.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.wtc_oracle_chord_ratio.restype = ctypes.c_float
        self.lib.wtc_oracle_chord_note_count.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.wtc_oracle_chord_inversion.argtypes = [
            ctypes.c_float,
            ctypes.c_float,
            FP,
            FP,
            IP,
        ]

    def handle(self, name: str) -> ctypes.Array[ctypes.c_char]:
        st = ctypes.create_string_buffer(self.lib.wtc_oracle_size())
        self.lib.wtc_oracle_init(st, ENGINES[name])
        return st

    def engine(
        self, name: str, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        st = self.handle(name)
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            self.lib.wtc_oracle_render(
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
def checked(tmp_path_factory: pytest.TempPathFactory) -> Port:
    """The port built with PL_INDEX_CHECK: every table read reports its index to port_sizes.c's recorder."""
    p = build_port(tmp_path_factory.mktemp("port-idx"), ("PL_INDEX_CHECK",))
    p.lib.port_chord_ratio.restype = ctypes.c_float
    return p


@pytest.fixture(scope="module")
def oracle(tmp_path_factory: pytest.TempPathFactory) -> Oracle:
    d = tmp_path_factory.mktemp("wtc-oracle")
    plaits = EURORACK / "plaits"
    sources = [
        ORACLE / "wtc_oracle.cc",
        plaits / "dsp" / "engine" / "wavetable_engine.cc",
        plaits / "dsp" / "engine" / "chord_engine.cc",
        plaits / "dsp" / "chords" / "chord_bank.cc",
        plaits / "resources.cc",
    ]
    return Oracle(build_oracle(d, sources, "wtc_oracle"))


def same(a: Arr, b: Arr) -> bool:
    return bool(a.view(np.uint32).tolist() == b.view(np.uint32).tolist())


# ---- the generated tables


def test_wave_table_equals_upstream_bit_for_bit(port: Port, oracle: Oracle) -> None:
    port.lib.port_wave.restype = ctypes.c_float
    n = oracle.lib.wtc_oracle_wave_count()
    assert n == port.lib.port_wave_count() == 192 * 132
    mine = np.array([port.lib.port_wave(i) for i in range(n)], dtype=np.float32)
    theirs = np.array(
        [oracle.lib.wtc_oracle_wave(i) for i in range(n)], dtype=np.float32
    )
    assert same(mine, theirs)
    assert float(np.abs(theirs).max()) > 10000  # int16 scale, not normalised


def test_wave_map_equals_upstream_load_user_data_null(
    port: Port, oracle: Oracle
) -> None:
    st = oracle.handle("wavetable")
    theirs = [oracle.lib.wtc_oracle_wave_map(st, i) for i in range(256)]
    mine = [port.lib.port_wave_map(i) for i in range(256)]
    assert mine == theirs
    assert all(0 <= m <= 191 * 132 and m % 132 == 0 for m in mine)


def test_chord_bank_ratios_and_counts_equal_upstream(
    checked: Port, oracle: Oracle
) -> None:
    """Each of the 11 chords, selected through set_chord as upstream does: the 4 ratios and the note count equal
    upstream's ChordBank::Reset bit for bit (the ratios are generated constants here)."""
    st = oracle.handle("chord")
    checked.lib.port_chord_num_notes.argtypes = [ctypes.c_float]
    checked.lib.port_chord_ratio.argtypes = [ctypes.c_float, ctypes.c_int]
    for i in range(11):
        # set_chord: value = parameter * 1.02 * 11 - 0.5 = i
        parameter = (i + 0.5) / 11 / 1.02
        mine = np.array(
            [checked.lib.port_chord_ratio(parameter, j) for j in range(4)], np.float32
        )
        theirs = np.array(
            [oracle.lib.wtc_oracle_chord_ratio(st, 4 * i + j) for j in range(4)],
            np.float32,
        )
        assert same(mine, theirs), i
        assert checked.lib.port_chord_num_notes(
            parameter
        ) == oracle.lib.wtc_oracle_chord_note_count(st, i), i


def test_chord_inversion_equals_upstream_over_the_whole_range(
    checked: Port, oracle: Oracle
) -> None:
    """ComputeChordInversion for every chord and inversions 0..1 (every integral 0..20, both sides of each step,
    and 1.0 itself): ratios, amplitudes, mask and chord index equal upstream; every voice index stays in 0..4."""
    checked.lib.port_chord_inversion.argtypes = [
        ctypes.c_float,
        ctypes.c_float,
        FP,
        FP,
        IP,
    ]
    eps = 1e-6
    inversions = sorted(
        {0.0, 1.0}
        | {
            min(1.0, max(0.0, k / 20 + d))
            for k in range(21)
            for d in (-eps, 0.0, eps, 0.013)
        }
    )
    params = [0.0, 1.0] + [(i + 0.5) / 11 / 1.02 for i in range(11)]
    checked.lib.port_index_reset()
    for prm in params:
        for inv in inversions:
            mr, ma = np.zeros(5, np.float32), np.zeros(5, np.float32)
            tr, ta = np.zeros(5, np.float32), np.zeros(5, np.float32)
            mi, ti = ctypes.c_int(), ctypes.c_int()
            mm = checked.lib.port_chord_inversion(
                prm, inv, mr.ctypes.data_as(FP), ma.ctypes.data_as(FP), ctypes.byref(mi)
            )
            tm = oracle.lib.wtc_oracle_chord_inversion(
                prm, inv, tr.ctypes.data_as(FP), ta.ctypes.data_as(FP), ctypes.byref(ti)
            )
            assert (mm, mi.value) == (tm, ti.value), (prm, inv)
            assert same(mr, tr) and same(ma, ta), (prm, inv)
    lo, hi = checked.lib.port_index_lo(6), checked.lib.port_index_hi(6)
    assert (lo, hi) == (0, 4)
    assert (checked.lib.port_index_lo(5), checked.lib.port_index_hi(5)) == (0, 43)


# ---- the engines against upstream

SETS = [
    (48.0, 0.5, 0.5, 0.5),
    (60.0, 0.0, 0.25, 0.8),
    (36.5, 1.0, 1.0, 0.0),
    (84.0, 0.3, 0.7, 0.66),
    # CHORD: MORPH in the crossfade between the divide-down and wavetable voices
    (57.0, 0.62, 0.1, 0.52),
]


def sweep(blocks: int) -> list[tuple[float, float, float, float]]:
    """Every control moving block by block (the interpolators, filters and pitch path move)."""
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
    assert np.isfinite(oo).all() and np.abs(oo).max() > 0.02
    assert same(po, oo), name
    assert same(pa, oa), name


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_sub_block_of_16_bit_exact(port: Port, oracle: Oracle, name: str) -> None:
    """The wrapper renders sub-blocks of 16 samples (PLAITS_SUB); the sweep at that size."""
    settings = sweep(3000)
    po, pa = port.engine(name, settings, 16)
    oo, oa = oracle.engine(name, settings, 16)
    assert same(po, oo) and same(pa, oa), name


def test_chord_morph_sweep_bit_exact(port: Port, oracle: Oracle) -> None:
    """MORPH slowly through the whole range at several chords and inversions: every voice through the crossfade of its
    fade point (0.47..0.55), the registrations and the wavetable's waveform sweep."""
    settings = []
    for h in (0.0, 0.33, 0.71, 1.0):
        for t in (0.0, 0.4, 1.0):
            settings += [(55.0, h, t, b / 400) for b in range(401)]
    po, pa = port.engine("chord", settings, 16)
    oo, oa = oracle.engine("chord", settings, 16)
    assert same(po, oo) and same(pa, oa)


# Peak bounds. Both engines start cold the way upstream does at power-up, and the wrapper restarts the engine at every
# note-on, so each note starts with upstream's cold-start transient: WAVETABLE's Differentiator starts at 0 against an
# integrated wave near -20,556, a first sample near -20 at every pitch, decaying in about 1 / (128 f) samples (at
# note -64, f is 4e-6 while the f0 interpolator starts at a0, which lifts it to about 1,800); CHORD's wavetable voices
# do the same at a high MORPH and a low note (about 11). The engines are bit exact against upstream there; the bounds
# only say that the output stays finite and within these sizes (test_note_on_transients records them).
BOUND = {"wavetable": 2000.0, "chord": 16.0}

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
        assert same(po, oo), (name, c)
        assert same(pa, oa), (name, c)
        for x in (po, pa):
            assert np.isfinite(x).all(), (name, c)
            assert float(np.abs(x).max()) < BOUND[name], (name, c)


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_corner_jumps_bit_exact_and_finite(
    port: Port, oracle: Oracle, name: str
) -> None:
    """Jumps between the corners every block (pitch by up to 254 semitones within one block, no note-on): equal to
    upstream and finite."""
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
        assert same(po, oo) and same(pa, oa), (name, stride)
        for x in (po, pa):
            assert np.isfinite(x).all(), (name, stride)
        # no bound: a jump from a high to a low pitch within one block multiplies the differentiator's state by
        # 1 / (f * 131072) at the low f (upstream's arithmetic, equal here): about 4,300 (WAVETABLE) and 5,200 (CHORD)
        print(f"{name} stride {stride}: peak {float(np.abs(po).max()):.1f}")


# ---- index ranges at every bank, row and column corner

# WAVETABLE: HARMONICS picks the bank (z = 7 HARMONICS: 0..3, then mirrored back 3..0), MORPH the row (y), TIMBRE the
# column (x). Each value k / 7 is a grid line; k / 7 +- 0.004 lies just either side of it.
GRID = sorted(
    {min(1.0, max(0.0, k / 7 + d)) for k in range(8) for d in (-0.004, 0.0, 0.004)}
)


def test_wavetable_indices_stay_inside_at_every_bank_row_column(
    checked: Port, oracle: Oracle
) -> None:
    """Every (bank, row, column) grid point and both sides of it, held until the filters settle at the note range's
    ends and middle: the wave map index stays in 0..255 and the Hermite read in 0..130 of a 132-entry wave, and the
    whole range is reached (index 0 and 255, sample 3 and 130). Each point is bit exact against upstream."""
    checked.lib.port_index_reset()
    for h in GRID:
        for m in GRID:
            for t in GRID if h in (0.0, 1.0) or m in (0.0, 1.0) else (0.0, 0.5, 1.0):
                c = [(n, h, t, m) for n in (-64.0, 60.0, 190.0) for _ in range(40)]
                po, pa = checked.engine("wavetable", c, 16)
                oo, oa = oracle.engine("wavetable", c, 16)
                assert same(po, oo) and same(pa, oa), (h, t, m)
    assert checked.lib.port_index_n(0) > 0
    assert (checked.lib.port_index_lo(0), checked.lib.port_index_hi(0)) == (0, 255)
    assert checked.lib.port_index_lo(1) >= 0 and checked.lib.port_index_hi(1) == 130
    for k in range(8):
        corner = (float((k >> 2) & 1), float((k >> 1) & 1), float(k & 1))
        checked.lib.port_index_reset()
        checked.engine("wavetable", [(60.0, corner[0], corner[1], corner[2])] * 400, 16)
        lo, hi = checked.lib.port_index_lo(0), checked.lib.port_index_hi(0)
        assert 0 <= lo <= hi <= 255, (corner, lo, hi)


def test_chord_indices_stay_inside_at_every_corner(
    checked: Port, oracle: Oracle
) -> None:
    """CHORD at every chord (HARMONICS), inversion and MORPH grid point: the registration row stays in 0..7, the
    wavetable voices' wave index in 0..14 and sample index in 0..128, the voices in 0..4; the ends are reached."""
    checked.lib.port_index_reset()
    hs = [0.0, 1.0] + [(i + 0.5) / 11 / 1.02 for i in range(11)]
    for h in hs:
        for t in (0.0, 0.26, 0.5, 0.99, 1.0):
            for m in (0.0, 0.5, 0.535, 0.56, 0.8, 1.0):
                c = [(n, h, t, m) for n in (-64.0, 48.0, 190.0) for _ in range(60)]
                po, pa = checked.engine("chord", c, 16)
                oo, oa = oracle.engine("chord", c, 16)
                assert same(po, oo) and same(pa, oa), (h, t, m)
    assert (checked.lib.port_index_lo(4), checked.lib.port_index_hi(4)) == (0, 7)
    assert (checked.lib.port_index_lo(2), checked.lib.port_index_hi(2)) == (0, 14)
    assert checked.lib.port_index_lo(3) >= 0 and checked.lib.port_index_hi(3) <= 128
    assert (checked.lib.port_index_lo(6), checked.lib.port_index_hi(6)) == (0, 4)


# ---- pitch, wrapper


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


def test_pitch_note69_at_48k(port: Port) -> None:
    """WAVETABLE at note 69 (bank 0's sines): 440 Hz on OUT within 1 cent. CHORD's root, the OCT chord at inversion 0,
    is at 440 * 0.998 Hz (upstream's detune) on AUX."""
    out, _ = port.engine("wavetable", [(69.0, 0.0, 0.0, 0.0)] * (4 * 48000 // 16), 16)
    f = f0_fft(out[48000:], 440.0)
    assert abs(1200 * math.log2(f / 440.0)) < 1.0, f
    _, aux = port.engine("chord", [(69.0, 0.0, 0.0, 0.0)] * (4 * 48000 // 16), 16)
    f = f0_fft(aux[48000:], 440.0 * 0.998)
    assert abs(1200 * math.log2(f / (440.0 * 0.998))) < 1.0, f


def _render(port: Port, v: Any, f: Any, trig: int) -> Arr:
    return port.render(v, f, trig, 1)


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_wrapper_renders_the_model(port: Port, name: str) -> None:
    """MODEL 5/6 renders wavetable/chord: OUT + MIX (AUX - OUT), then the DC blocker and the gain, bit for bit (support.plaits_host.Tail: Plaits' output stage first)."""
    inv = np.float32(3.0757874e-05)
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
        want = tail(MODEL[name], out, aux, m)
        assert same(got, want), (name, mix)
        assert float(np.abs(got).max()) > 0.02


def test_engine_union_holds_every_engine(port: Port) -> None:
    """The per-voice union is the size of its largest engine (Ruling B: each engine's state bytes)."""
    names = ("va", "ws", "fm", "grain", "additive", "wavetable", "chord", "speech")
    sizes = {n: getattr(port, f"{n}_size") for n in names}
    assert port.engine_union_size == max(sizes.values()), sizes
    print("engine state bytes:", sizes, "union", port.engine_union_size)


def test_wrapper_model_change_chain(port: Port) -> None:
    """One voice through VA -> WAVETABLE -> CHORD -> ADDITIVE -> CHORD -> WAVETABLE -> 2FM -> CHORD -> VA, each change
    made first without a note-on (the running engine goes on) and then at a note-on (the new model equals a fresh
    voice of that model bit for bit: nothing is left of the previous engine's state in the shared union)."""
    seq = [0, 5, 6, 4, 6, 5, 2, 6, 0]
    slots = {"harmonics": 0x3000, "morph": 0x4400}
    v = port.voice()
    prev = seq[0]
    ref = port.voice()
    blocks = [
        _render(port, v, frame(57, model=prev, **slots), int(b == 0)) for b in range(5)
    ]
    ref_blocks = [
        _render(port, ref, frame(57, model=prev, **slots), int(b == 0))
        for b in range(8)
    ]
    assert all(
        np.array_equal(a, b) for a, b in zip(blocks, ref_blocks[:5], strict=True)
    )
    for model in seq[1:]:
        on = [_render(port, v, frame(57, model=model, **slots), 0) for _ in range(3)]
        assert all(
            np.array_equal(a, b) for a, b in zip(on, ref_blocks[5:8], strict=True)
        ), model
        fresh = port.voice()
        want = [
            _render(port, fresh, frame(57, model=model, **slots), int(b == 0))
            for b in range(8)
        ]
        got = [
            _render(port, v, frame(57, model=model, **slots), int(b == 0))
            for b in range(8)
        ]
        assert all(np.array_equal(a, b) for a, b in zip(got, want, strict=True)), model
        assert any(x.any() for x in got), model
        ref_blocks = want[:5] + [
            _render(port, fresh, frame(57, model=model, **slots), 0) for _ in range(3)
        ]


def test_wrapper_unported_models_8_to_23_are_silent(port: Port) -> None:
    for model in (m for m in range(8, 24) if m not in (12, 21)):
        v = port.voice()
        outs = [_render(port, v, frame(60, model=model), int(b == 0)) for b in range(3)]
        assert not any(o.any() for o in outs), model


@pytest.mark.parametrize("name", sorted(ENGINES))
def test_wrapper_extremes_finite(port: Port, name: str) -> None:
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x4400, 0x7F00):
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
                                assert float(np.abs(o).max()) < BOUND[name]


def test_note_on_transients(port: Port) -> None:
    """The size of upstream's cold-start transient at a voice's first note-on (and at a note-on that changes the
    model: the wrapper initialises the engine then, and only then since Task 9b; later note-ons leave it running,
    tests/plaits/test_continuous_host.py), against the steady output, through the wrapper at default knobs (WAVETABLE)
    and MORPH 0x7f00 (CHORD's wavetable voices). Equal to upstream's cold start (the engines are bit exact from init);
    recorded for the report: WAVETABLE's first block peaks near 11.5 from note 24 up (20.9 at note 0) against a steady
    0.45, CHORD's at up to 7.5 (note 0)."""
    for name, slots in (
        ("wavetable", {}),
        ("chord", {"morph": 0x7F00, "timbre": 0x7F00}),
    ):
        for note in (0, 24, 48, 60, 72, 96):
            v = port.voice()
            x = np.concatenate(
                [
                    _render(
                        port, v, frame(note, model=MODEL[name], **slots), int(b == 0)
                    )
                    for b in range(300)
                ]
            )
            first, steady = float(np.abs(x[:1600]).max()), float(np.abs(x[4800:]).max())
            print(
                f"{name} note {note}: note-on peak {first:.3f}, steady peak {steady:.3f}"
            )
            assert steady < 1.0 and first < 25.0, (name, note, first, steady)
