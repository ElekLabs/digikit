"""machines/plaits (models VA, ASYM-TRI, 2FM, FORMANT, ADDITIVE, WAVETABLE, CHORD) on core/dsp-inline: the compiled C on the concrete SHARC+ core and the images.

The rig (tests/support/plaits_rig.py) links core/dsp-inline at test addresses and the PLAITS object of
machines/plaits-48k (mode 1, 48 kHz into the voice output) or machines/plaits-96k (mode 0, 96 kHz into the work
buffer), built as pk_build builds it (the entry shim plus the C voice wrapper, VA engine and half-band compiled by
selcc), and runs `_di_entry` block after block on the same memory. Every model has the same tests: equivalence with
the host build (both modes), the extreme grid with an instruction bound (slow), and a model switch at a note-on to and
from VA (tests/support/plaits_rig.py `assert_model_switch`). The reference is the same C built for the host
(tests/support/plaits_host.py), driven with the PlaitsFrame the shim reads from the frame words.

The image tests patch stock 1.17 with each variant and run the four owner-image checks (container checksums, the
SHARC+ boot stream against stock, the encoding gate, sections 2, 4, 5 and 8 byte-identical to stock), then run the
image's own code from the stock renderer. Skips without Selache, the stock image, the 1.17 sharcdb or the firmware
tool."""

from __future__ import annotations

import json
import math
import subprocess
from pathlib import Path
from typing import Any

import numpy as np
import pytest
from support.plaits_host import Port, build_port, frame
from support.plaits_rig import (
    AFTER,
    DECIMATE,
    RESUME,
    PlaitsRig,
    assert_model_switch,
    plaits_type,
)

from tools.patchkit import pk_toolchains

ROOT = pk_toolchains.ROOT
pytestmark = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)
Arr = Any

# Tolerances of the brief: compiled against host, per sample.
MAX_ERR, RMS_ERR = 2e-3, 1e-4

# Four control settings (row values); each changes some controls half-way, as a p-lock would.
SETTINGS: list[dict[str, Any]] = [
    {"note": 60, "first": {}, "second": {"timbre": 0x6000}},
    {
        "note": 48,
        "tune": 0x4180,
        "first": {"harmonics": 0, "timbre": 0x7F00, "morph": 0x2000, "mix": 0x7F00},
        "second": {"harmonics": 0x1000, "morph": 0x3000},
    },
    {
        "note": 72,
        "first": {
            "harmonics": 0x7F00,
            "timbre": 0x1000,
            "morph": 0x7F00,
            "mix": 0x4000,
        },
        "second": {"timbre": 0x7F00, "mix": 0},
    },
    {
        "note": 36,
        "tune": 0x3E00,
        "first": {
            "harmonics": 0x2A00,
            "timbre": 0x6000,
            "morph": 0x5400,
            "mix": 0x2000,
        },
        "second": {"morph": 0x0800, "harmonics": 0x5000},
    },
]


@pytest.fixture(scope="module")
def port(tmp_path_factory: pytest.TempPathFactory) -> Port:
    return build_port(tmp_path_factory.mktemp("plaits-host"))


def compare(
    port: Port, mode: int, setting: dict[str, Any], blocks: int, model: int = 0
) -> tuple[float, float, float, list[int]]:
    """Track 0's even voice, triggered at block 0, against the host wrapper: (max error, RMS error, signal RMS,
    instructions per block)."""
    rig = PlaitsRig(mode)
    hv = port.voice()
    tune = setting.get("tune", 0x4000)
    core, host = [], []
    for b in range(blocks):
        slots = {
            "model": model,
            **setting["first"],
            **(setting["second"] if b >= blocks // 2 else {}),
        }
        # the frame holds MODEL's row value: the model number << 8 (frame() takes the number)
        rig.controls(0, setting["note"], tune=tune, **{**slots, "model": model << 8})
        rig.active(0)
        rig.trig(0, b == 0)
        core.append(np.array(rig.block(0)["samples"], dtype=np.float32))
        host.append(
            port.render(
                hv, frame(setting["note"], tune=tune, **slots), int(b == 0), mode
            )
        )
    c, h = np.concatenate(core), np.concatenate(host)
    err = c - h
    return (
        float(np.abs(err).max()),
        float(np.sqrt(np.mean(err.astype(np.float64) ** 2))),
        float(np.sqrt(np.mean(h.astype(np.float64) ** 2))),
        rig.instructions,
    )


@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_va_matches_host_on_concrete_core(port: Port, k: int) -> None:
    mx, rms, sig, instr = compare(port, 1, SETTINGS[k], 120)
    assert sig > 0.05, "the setting sounds"
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)
    assert max(instr) < 120_000, max(instr)


@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_va_96k_matches_host_on_concrete_core(
    port: Port, k: int, request: pytest.FixtureRequest
) -> None:
    blocks = 120 if request.config.getoption("--slow") else 24
    mx, rms, sig, instr = compare(port, 0, SETTINGS[k], blocks)
    assert sig > 0.05
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)


@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_asym_tri_matches_host_on_concrete_core(port: Port, k: int) -> None:
    """Model 1 (ASYM-TRI, the waveshaping engine) in mode 1: the compiled object against the host build."""
    mx, rms, sig, instr = compare(port, 1, SETTINGS[k], 120, model=1)
    assert sig > 0.05, "the setting sounds"
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)
    assert max(instr) < MAX_BLOCK_INSTRUCTIONS_WS[1], max(instr)
    print(
        f"ASYM-TRI set {k}: max {mx:.3g} rms {rms:.3g} sig {sig:.3f} instr {min(instr)}..{max(instr)}"
    )


@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_asym_tri_96k_matches_host_on_concrete_core(
    port: Port, k: int
) -> None:
    mx, rms, sig, instr = compare(port, 0, SETTINGS[k], 24, model=1)
    assert sig > 0.05
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)
    assert max(instr) < MAX_BLOCK_INSTRUCTIONS_WS[0], max(instr)


# Models 2..4 (Task 8 of the all-engines plan): 2FM, FORMANT (grain), ADDITIVE; 5, 6 (Task 9): WAVETABLE, CHORD.
NEW_MODELS = {2: "2FM", 3: "FORMANT", 4: "ADDITIVE", 5: "WAVETABLE", 6: "CHORD"}


@pytest.mark.parametrize("model", sorted(NEW_MODELS))
@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_new_models_match_host_on_concrete_core(
    port: Port, model: int, k: int
) -> None:
    """Models 2, 3 and 4 in mode 1: the compiled object against the host build."""
    mx, rms, sig, instr = compare(port, 1, SETTINGS[k], 120, model=model)
    assert sig > 0.02, "the setting sounds"
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)
    assert max(instr) < MAX_BLOCK_INSTRUCTIONS_NEW[model][1], max(instr)
    print(
        f"{NEW_MODELS[model]} set {k}: max {mx:.3g} rms {rms:.3g} sig {sig:.3f} "
        f"instr {min(instr)}..{max(instr)}"
    )


@pytest.mark.parametrize("model", sorted(NEW_MODELS))
@pytest.mark.parametrize("k", range(len(SETTINGS)))
def test_compiled_new_models_96k_match_host_on_concrete_core(
    port: Port, model: int, k: int
) -> None:
    mx, rms, sig, instr = compare(port, 0, SETTINGS[k], 24, model=model)
    assert sig > 0.02
    assert mx <= MAX_ERR and rms <= RMS_ERR, (mx, rms)
    assert max(instr) < MAX_BLOCK_INSTRUCTIONS_NEW[model][0], max(instr)


def test_compiled_lib_tables_equal_host_bits(port: Port) -> None:
    """The shared tables of models 2..4 (lib/tables.c) as selcc wrote them into the object equal the host build's
    float32 bits, which tests/plaits/test_fm_grain_additive_host.py checks against upstream (the negative values of
    the FM ratio quantizer are the selcc hazard of finding 20)."""
    obj = PlaitsRig(1).obj
    names = [
        "pl_lut_sine.",
        "pl_lut_fm_frequency_quantizer.",
        "pl_lut_4x_downsampler_fir.",
        "pl_lut_pitch_ratio_high.",
        "pl_lut_pitch_ratio_low.",
    ]
    for k, name in enumerate(names):
        sec, off, _g = obj.data_symbols[name]
        blob = obj.data[sec][0]
        n = port.lib.port_lib_lut_size(k)
        got = np.frombuffer(blob[off : off + 4 * n], dtype="<u4")
        want = np.array(
            [port.lib.port_lib_lut(k, i) for i in range(n)], dtype=np.float32
        )
        assert got.tolist() == want.view(np.uint32).tolist(), name


def test_compiled_wave_tables_equal_host_bits(port: Port) -> None:
    """The tables of models 5 and 6 as selcc wrote them into the object: the wave table (negative floats, the selcc
    hazard of finding 20) and the wave map (lib/waves.c) equal the host build's bits, which
    tests/plaits/test_wavetable_chord_host.py checks against upstream."""
    import ctypes

    obj = PlaitsRig(1).obj
    port.lib.port_wave.restype = ctypes.c_float
    n = port.lib.port_wave_count()
    sec, off, _g = obj.data_symbols["pl_wav_integrated_waves."]
    got = np.frombuffer(obj.data[sec][0][off : off + 4 * n], dtype="<u4")
    want = np.array([port.lib.port_wave(i) for i in range(n)], dtype=np.float32)
    assert got.tolist() == want.view(np.uint32).tolist()
    assert (want < 0).sum() > 1000
    sec, off, _g = obj.data_symbols["pl_wavetable_map."]
    got = np.frombuffer(obj.data[sec][0][off : off + 4 * 256], dtype="<i4")
    assert got.tolist() == [port.lib.port_wave_map(i) for i in range(256)]


def test_compiled_chord_tables_equal_host_bits() -> None:
    """CHORD's static tables (engines/chord_tables.h, lib/chord_bank_tables.h) as selcc wrote them equal the generated
    bit patterns."""
    import re

    obj = PlaitsRig(1).obj
    dsp = ROOT / "modules/machines/plaits/src/dsp"
    text = (dsp / "engines/chord_tables.h").read_text() + (
        dsp / "lib/chord_bank_tables.h"
    ).read_text()
    found = 0
    for name, body in re.findall(
        r"static const (?:unsigned )?int (\w+)\[\d+\] = \{(.*?)\};", text, re.S
    ):
        want = [int(t.rstrip("u"), 0) for t in re.split(r"[\s,]+", body) if t]
        syms = [
            k
            for k in obj.data_symbols
            if k.rstrip(".") == name or k.startswith(name + ".")
        ]
        assert syms, (name, sorted(obj.data_symbols)[:40])
        sec, off, _g = obj.data_symbols[syms[0]]
        got = np.frombuffer(obj.data[sec][0][off : off + 4 * len(want)], dtype="<u4")
        assert got.tolist() == want, name
        found += 1
    assert found == 5


@pytest.mark.parametrize("mode", [1, 0])
@pytest.mark.parametrize("model", sorted(NEW_MODELS))
def test_model_switch_at_note_on_to_and_from_va(
    port: Port, mode: int, model: int
) -> None:
    """VA -> model -> VA on one voice (assert_model_switch twice on the same rig): each new model equals a fresh
    voice of it after the note-on, the old one runs on through a MODEL change without one; and the whole run is
    within MAX_ERR of the host."""
    rig = PlaitsRig(mode)
    slots = {"harmonics": 0x3000, "timbre": 0x5000}
    steps = assert_model_switch(rig, 0, model, **slots)
    steps += assert_model_switch(rig, model, 0, **slots)
    hv = port.voice()
    host = [port.render(hv, frame(57, model=m, **slots), t, mode) for m, t in steps]
    rig2 = PlaitsRig(mode)
    rig2.active(0)
    from support.plaits_rig import model_blocks

    got = model_blocks(rig2, steps, **slots)
    assert float(np.abs(np.concatenate(got) - np.concatenate(host)).max()) <= MAX_ERR


def test_model_change_chain_va_fm_formant_additive_asym_tri_va() -> None:
    """Review focus 1: one voice through every enabled model in turn (VA -> 2FM -> FORMANT -> ADDITIVE -> WAVETABLE ->
    CHORD -> 2FM -> ASYM-TRI -> CHORD -> VA), each change first without a note-on, then at one."""
    rig = PlaitsRig(1)
    chain = [0, 2, 3, 4, 5, 6, 2, 1, 6, 0]
    for a, b in zip(chain, chain[1:], strict=False):
        assert_model_switch(rig, a, b, harmonics=0x5000, morph=0x2000)


@pytest.mark.parametrize("model", sorted(NEW_MODELS))
def test_new_model_c_stack_stays_inside_pl_stack(model: int) -> None:
    """The deepest C frame of models 2..6 (ADDITIVE keeps 12 interpolators on the stack, CHORD calls the voices'
    oscillators from its render) stays inside the 4096-byte _pl_stack with room to spare."""
    rig = PlaitsRig(1)
    morph = 0x4400 if model == 6 else 0  # CHORD: MORPH where both kinds of voice render
    rig.controls(0, 60, model=model << 8, harmonics=0x7F00, timbre=0x7F00, morph=morph)
    rig.active(0)
    rig.trig(0)
    lo = rig.symbols["_pl_stack"]
    code = rig.st.UREG_CODES["I7"]
    seen: list[int] = []

    def watch(state: Any) -> None:
        i7 = getattr(state.uregs.get(code), "value", None)
        if i7 is not None and lo <= i7 < lo + 4096:
            seen.append(i7)

    rig.block(0, on_step=watch)
    assert seen
    used = lo + 4096 - min(seen)
    assert used < 2048, used
    print(f"model {model}: {used} bytes of _pl_stack")


def test_compiled_ws_tables_equal_host_bits(port: Port) -> None:
    """The waveshaping tables as selcc wrote them into the object (data, which neither the encoding gate nor the
    cross-check sees; selcc mis-writes negative float initialisers, finding 20) equal the host build's float32 bits,
    which tests/plaits/test_ws_host.py checks against upstream."""
    obj = PlaitsRig(1).obj
    names = [
        "lut_sine.",
        "lut_fold.",
        "lut_fold_2.",
        "lut_ws_inverse_tan.",
        "lut_ws_inverse_sin.",
        "lut_ws_linear.",
        "lut_ws_bump.",
        "lut_ws_double_bump.",
    ]
    for k, name in enumerate(names):
        sec, off, _g = obj.data_symbols[name]
        blob = obj.data[sec][0]
        n = port.lib.port_ws_lut_size(k)
        got = np.frombuffer(blob[off : off + 4 * n], dtype="<u4")
        want = np.array(
            [port.lib.port_ws_lut(k, i) for i in range(n)], dtype=np.float32
        )
        assert got.tolist() == want.view(np.uint32).tolist(), name


@pytest.mark.parametrize("mode", [1, 0])
def test_model_switch_at_note_on_va_asym_tri_va(port: Port, mode: int) -> None:
    """One voice: VA, then MODEL 1 without a note-on (VA runs on), a note-on (ASYM-TRI), MODEL 0 and a note-on (VA).
    After each note-on the compiled output equals a fresh rig's voice of the new model bit for bit, and the host."""

    def run(rig: PlaitsRig, steps: list[tuple[int, int]]) -> list[Arr]:
        out = []
        for model, trig in steps:
            rig.controls(0, 57, model=model << 8, harmonics=0x3000, timbre=0x5000)
            rig.trig(0, bool(trig))
            out.append(np.array(rig.block(0)["samples"], dtype=np.float32))
        return out

    seq = [(0, int(b == 0)) for b in range(4)] + [(1, 0)] * 2
    seq += [(1, int(b == 0)) for b in range(5)] + [(0, int(b == 0)) for b in range(5)]
    rig = PlaitsRig(mode)
    rig.active(0)
    got = run(rig, seq)
    # VA runs on through the MODEL change (blocks 4, 5) as if it had not happened
    ref = PlaitsRig(mode)
    ref.active(0)
    va_only = run(ref, [(0, int(b == 0)) for b in range(6)])
    assert all(np.array_equal(a, b) for a, b in zip(got[:6], va_only, strict=True))
    for start, model in ((6, 1), (11, 0)):
        fresh = PlaitsRig(mode)
        fresh.active(0)
        want = run(fresh, [(model, int(b == 0)) for b in range(5)])
        assert all(
            np.array_equal(a, b)
            for a, b in zip(got[start : start + 5], want, strict=True)
        ), model
        assert any(want[1])
    hv = port.voice()
    host = [
        port.render(hv, frame(57, model=m, harmonics=0x3000, timbre=0x5000), t, mode)
        for m, t in seq
    ]
    assert float(np.abs(np.concatenate(got) - np.concatenate(host)).max()) <= MAX_ERR


def test_model1_reaches_asym_tri_from_the_model_row() -> None:
    """The MODEL row's range is 0 .. 0x1700 (UI 0 .. 23): row value N << 8 (UI N, the knob or a p-lock) is model N;
    models 0..6 sound, each differently, and 7 (not ported yet) is silent."""
    import tomllib

    manifest = tomllib.loads((ROOT / "modules/machines/plaits/module.toml").read_text())
    (row,) = [e for e in manifest["entry"] if e.get("id") == "plaits_model"]
    assert (row["min"], row["max"], row["default"]) == (0, 0x1700, 0)
    rig = PlaitsRig(1)
    rig.active(0)
    outs = {}
    for model in range(8):
        rig.controls(0, 60, model=model << 8)  # the row value: UI 1 is 0x100
        rig.trig(0, True)
        outs[model] = np.array(rig.block(0)["samples"], dtype=np.float32)
        rig.trig(0, False)
    assert all(outs[m].any() for m in range(7)) and not outs[7].any()
    for a in range(7):
        for b in range(a + 1, 7):
            assert not np.array_equal(outs[a], outs[b]), (a, b)


def f0_autocorr(x: Arr, lo: float, hi: float, fs: float = 48000.0) -> float:
    """The fundamental (Hz) of x between lo and hi by the autocorrelation peak, parabolic interpolation."""
    x = np.asarray(x, dtype=np.float64)
    x = x - x.mean()
    r = np.correlate(x, x, "full")[len(x) - 1 :]
    a, b = int(fs / hi), int(fs / lo) + 1
    i = a + int(np.argmax(r[a:b]))
    y0, y1, y2 = r[i - 1], r[i], r[i + 1]
    return float(fs / (i + 0.5 * (y0 - y2) / (y0 - 2 * y1 + y2)))


@pytest.mark.parametrize("mode", [1, 0])
def test_two_voices_independent_state(port: Port, mode: int) -> None:
    rig = PlaitsRig(mode)
    rig.controls(0, 60)
    rig.controls(1, 67)
    for v in range(4):
        rig.active(v)
    host = {0: port.voice(), 2: port.voice()}
    notes = {0: 60, 2: 67}
    got: dict[int, list[Arr]] = {0: [], 1: [], 2: [], 3: []}
    want: dict[int, list[Arr]] = {0: [], 2: []}
    blocks = 40 if mode else 20
    for b in range(blocks):
        for v in (0, 2):
            rig.trig(v, b == 0)
        for v in range(
            4
        ):  # the voice loop's order: each track's even voice, then its odd voice
            got[v].append(np.array(rig.block(v)["samples"], dtype=np.float32))
        for v in (0, 2):
            want[v].append(port.render(host[v], frame(notes[v]), int(b == 0), mode))
    for v in (0, 2):
        g, w = np.concatenate(got[v]), np.concatenate(want[v])
        # each voice equals the host voice rendered on its own: no state shared with the other track
        assert float(np.abs(g - w).max()) <= MAX_ERR
        # the odd voice of the track (its right bus) carries the same block
        assert np.array_equal(np.concatenate(got[v + 1]), g)
    if mode:
        f60 = f0_autocorr(np.concatenate(got[0])[256:], 150, 600)
        f67 = f0_autocorr(np.concatenate(got[2])[256:], 150, 600)
        assert abs(1200 * math.log2(f60 / 261.6256)) < 10, f60
        assert abs(1200 * math.log2(f67 / 391.9954)) < 10, f67


@pytest.mark.parametrize("mode", [1, 0])
def test_retrigger_restarts_engine(mode: int) -> None:
    rig = PlaitsRig(mode)
    rig.controls(3, 55, harmonics=0x6000, timbre=0x3000)
    rig.active(6)
    first, again = [], []
    for b in range(6):
        rig.trig(6, b == 0)
        first.append(rig.block(6)["samples"])
    for _ in range(5):  # the engine runs on
        rig.trig(6, False)
        rig.block(6)
    for b in range(
        4
    ):  # a new note-on: the engine (and DC blocker, interpolator) start over
        rig.trig(6, b == 0)
        again.append(rig.block(6)["samples"])
    assert again == first[:4]
    assert any(x != 0.0 for x in again[0])


@pytest.mark.parametrize("mode", [1, 0])
def test_switch_type_away_and_back_is_silent_until_trig(mode: int) -> None:
    rig = PlaitsRig(mode)
    rig.controls(0, 60)
    rig.active(0)
    rig.active(1)
    rig.trig(0)
    played = rig.block(0)["samples"]
    assert any(played)
    rig.trig(0, False)
    rig.block(0)
    # away: the track's type changes (SAMPLE); its voices take the stock path, and the stock renderer clears ACTIVE
    # when the sample it plays ends (docs/findings/06, "Flags and seed")
    rig.set_type(0, 0)
    assert rig.block(0, want=RESUME)["final"] == RESUME
    rig.active(0, False)
    rig.active(1, False)
    # back to PLAITS without a note-on (as after a kit load): silent, both voices
    rig.set_type(0, plaits_type())
    for _ in range(3):
        for v in (0, 1):
            out = rig.block(v)
            assert out["samples"] == [0.0] * (32 if mode else 64), v
    # the next note-on (the trigger path sets ACTIVE again) plays from the start
    rig.active(0)
    rig.active(1)
    rig.trig(0)
    assert rig.block(0)["samples"] == played


def test_routine_keeps_the_hook_abi_registers() -> None:
    for mode in (1, 0):
        rig = PlaitsRig(mode)
        rig.controls(5, 64)
        rig.active(10)
        rig.trig(10)
        r = rig.block(10)["regs"]
        want_i7 = 0x2009C000 - 104 - (8 if mode else 0)
        assert (r["I6"], r["I7"]) == (0x2009C000, want_i7)
        assert (r["M5"], r["M6"], r["M7"]) == (0, 1, 0xFFFFFFFF)
        assert (r["M13"], r["M14"], r["M15"]) == (0, 1, 0xFFFFFFFF)
        assert (r["I8"], r["I13"], r["M0"], r["M8"]) == (0, 0, 0, 0)
        # the firmware's circular stack registers are back (L6/L7 are 0 only during the C call)
        assert (r["L6"], r["L7"], r["B6"], r["B7"]) == (
            0x800,
            0x800,
            0x2009C000 - 0x400,
            0x2009C000 - 0x400,
        )
        assert rig.symbols["_pl_frame"] and (AFTER if mode else DECIMATE)


ISR_WORDS = 64  # words an interrupt handler is assumed to push through I7 (DM(I7, M7) = ..., post-modify)
CBUFEN = 1 << 24


def _isr_writes(i7: int, b7: int, l7: int, mode1: int) -> list[int]:
    """Byte addresses an interrupt taken now would write with ISR_WORDS pushes through I7: post-modify by one word,
    wrapping around B7/L7 (L scaled to bytes) when MODE1.CBUFEN is set and L7 != 0 (the manual's rule, as the core
    applies it: tools/sharc_dev/sharc_core/memory.py `_circular_step`)."""
    from tools.sharc_dev.sharc_core.memory import _circular_step

    out = []
    for _ in range(ISR_WORDS):
        out.append(i7)
        if mode1 & CBUFEN and l7:
            i7 = _circular_step(i7, b7, 4 * l7, -4)
        else:
            i7 -= 4
    return out


def _interrupt_stack_check(
    mode: int,
) -> tuple[list[str], dict[str, int], tuple[int, int]]:
    """Before every instruction of one block of voice 0, where an interrupt's pushes would land: (states whose pushes
    leave both stacks, counts, the firmware buffer as the hook is entered)."""
    rig = PlaitsRig(mode)
    rig.controls(0, 60)
    rig.active(0)
    rig.trig(0)
    codes = [rig.st.UREG_CODES[n] for n in ("I7", "B7", "L7", "MODE1")]
    pl_lo = rig.symbols["_pl_stack"]
    seen = {"steps": 0, "own": 0}
    fw: list[tuple[int, int]] = []
    bad: list[str] = []

    def check(state: Any) -> None:
        i7, b7, l7, mode1 = (getattr(state.uregs.get(c), "value", None) for c in codes)
        assert (
            i7 is not None and b7 is not None and l7 is not None and mode1 is not None
        )
        if not fw:
            fw.append((b7, b7 + 4 * l7))
        seen["steps"] += 1
        writes = _isr_writes(i7, b7, l7, mode1)
        inside_fw = all(fw[0][0] <= a < fw[0][1] for a in writes)
        inside_pl = all(pl_lo <= a < pl_lo + 4096 for a in writes)
        seen["own"] += inside_pl
        if not (inside_fw or inside_pl) and len(bad) < 5:
            bad.append(
                f"pc {state.pc_sw:#x}: I7 {i7:#x} B7 {b7:#x} L7 {l7:#x} -> writes "
                f"{min(writes):#x}..{max(writes):#x}"
            )

    rig.block(0, on_step=check)
    return bad, seen, fw[0]


def test_an_interrupt_at_any_instruction_pushes_inside_one_stack() -> None:
    """M2: the shim moves I7 between the firmware's circular stack and its own linear one and changes L6/L7. An
    interrupt between two of those instructions must still push inside one stack. The safe states are: firmware I7
    with either L (inside the firmware buffer a linear push writes the same addresses as a circular one, as long as
    it does not reach B7, which the stock firmware itself never does), and our I7 with L = 0. Our I7 with the
    firmware's L is not: I7 lies above B7 + L7, so every push wraps by -L into foreign memory. Hence L is zeroed
    before I7 moves to _pl_stack and restored after I7 is back. Checked before every instruction of a block."""
    for mode in (1, 0):
        bad, seen, fw = _interrupt_stack_check(mode)
        assert not bad, (mode, bad)
        assert seen["own"] > 1000, seen  # the C call ran on _pl_stack
        assert fw == (0x2009C000 - 0x400, 0x2009C000 - 0x400 + 4 * 0x800)


# ---- the H2 images: stock 1.17 + machines/plaits-48k or -96k, and the four owner-image checks

STOCK = ROOT / "firmware" / "Digitakt_II_OS1.17.syx"
STOCK_DB = ROOT / "out" / "sharcdb" / "dt2-1.17.sqlite"
FWTOOL = ROOT.parent / "elektron-firmware-tool" / "elektron-firmware-tool"
UNCHANGED = (2, 4, 5, 8)
VARIANT = {1: "machines/plaits-48k", 0: "machines/plaits-96k"}
H2 = {1: "plaits-va-48k", 0: "plaits-va-96k"}

needs_image = pytest.mark.skipif(
    not STOCK.exists() or not STOCK_DB.exists(),
    reason="needs firmware/Digitakt_II_OS1.17.syx and out/sharcdb/dt2-1.17.sqlite",
)


def patchkit_binary() -> Path:
    from tools.patchkit import patchkit

    try:
        return patchkit.binary()
    except SystemExit as e:
        exe = patchkit.binary(build=False)
        if exe.exists():
            return exe
        pytest.skip(f"patchkit does not build: {str(e)[-300:]}")


def sections(fwtool: Path, syx: Path, d: Path) -> dict[int, bytes]:
    r = subprocess.run(
        [str(fwtool), "-i", str(syx), "-o", str(d)], capture_output=True, text=True
    )
    assert r.returncode == 0, r.stderr[-2000:]
    return {int(p.name.split("_")[1]): p.read_bytes() for p in d.glob("section_*")}


@pytest.fixture(scope="module")
def h2_images(tmp_path_factory: pytest.TempPathFactory) -> dict[int, tuple[Path, Path]]:
    """Stock 1.17 patched with each routine module (its requires pull in the machine, core/params and
    core/dsp-inline)."""
    d = tmp_path_factory.mktemp("h2")
    images = {}
    for mode, name in H2.items():
        out, plan = d / f"{name}.syx", d / f"{name}.plan.json"
        r = subprocess.run(
            [
                str(patchkit_binary()),
                "--root",
                str(ROOT),
                "apply",
                "--firmware",
                str(STOCK),
                "--modules",
                VARIANT[mode],
                "-o",
                str(out),
                "--plan",
                str(plan),
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        assert r.returncode == 0, r.stderr[-2000:]
        images[mode] = (out, plan)
    return images


@needs_image
@pytest.mark.skipif(not FWTOOL.exists(), reason="needs ../elektron-firmware-tool")
def test_plaits_image_passes_all_owner_image_checks(
    h2_images: dict[int, tuple[Path, Path]], tmp_path: Path
) -> None:
    from tools.patchkit import pk_encoding_gate as gate
    from tools.sharc_dev import sharc_bootcheck as boot

    stock_sections = sections(FWTOOL, STOCK, tmp_path / "stock")
    for mode, (out, plan) in h2_images.items():
        name = H2[mode]
        doc = json.loads(plan.read_text())
        mods = {str(m).split("@")[0] for m in doc["modules"]}
        assert {
            VARIANT[mode],
            "machines/plaits",
            "core/dsp-inline",
            "core/params",
        } <= mods, mods
        # 1. container checksums
        r = subprocess.run(
            [str(FWTOOL), "-i", str(out)], capture_output=True, text=True
        )
        assert r.returncode == 0 and "checksums : ok" in r.stdout, r.stdout[-2000:]
        # 2. the SHARC+ boot stream: the stock stream plus inserted blocks, no error
        blob = boot.section7(out)
        found = list(boot.check(blob).findings) + boot.check_against_stock(
            boot.section7(STOCK), blob
        )
        assert [f for f in found if f.level == "error"] == [], name
        # 3. the encoding gate, over code that is really there (the compiled C included)
        ranges = gate.code_ranges(doc)
        assert sum(hi - lo for lo, hi in ranges) > 6000, ranges
        assert gate.check_image(out, plan) == [], name
        # 4. bootloader, updater, metadata and section 8 byte-identical to stock
        got = sections(FWTOOL, out, tmp_path / name)
        for sid in UNCHANGED:
            assert got[sid] == stock_sections[sid], f"{name}: section {sid} changed"
        assert got[7] != stock_sections[7], f"{name}: the DSP blob is unchanged"


IMG_FRAME, IMG_LATCH, IMG_TYPES = 0x2558DC, 0x24F0CC, 0x255970


def _image_controls(img: Any, t: int, note: int) -> None:
    """Track t: type PLAITS in the DSP's type cache, its note and default rows in the frame copy."""
    img.set_half(IMG_TYPES + 4 * (t >> 1), t & 1, plaits_type())
    hw = {1 + t: note << 8}
    base = (0xDA + 0x60 * t) // 2
    hw.update(
        {
            base: 0x4000,
            base + 1: 0,
            base + 2: 0x4000,
            base + 6: 0x4000,
            base + 7: 0x4000,
            base + 8: 0,
        }
    )
    for k, value in hw.items():
        img.set_half(IMG_FRAME + 4 * (k // 2), k % 2, value)


def _image_trig(img: Any, v: int, on: bool) -> None:
    img.poke_byte(IMG_LATCH + v, 1 if on else 0)


@needs_image
@pytest.mark.parametrize("mode", [1, 0])
def test_plaits_image_renders_va_through_the_stock_renderer(
    h2_images: dict[int, tuple[Path, Path]], port: Port, mode: int
) -> None:
    from support.dsp_inline_image import CALLEE_SAVED, IMG_RECORDS, IMG_STACK, REC
    from support.plaits_rig import CALLER_RET, ImageRig

    out, plan = h2_images[mode]
    img = ImageRig(out, json.loads(plan.read_text()))
    v, t = 20, 10
    _image_controls(img, t, 62)
    for w in (v, v + 1):
        img.poke_byte(
            IMG_RECORDS + w * REC + 0x1B8, 1
        )  # ACTIVE, as the trigger path leaves it
    hv = port.voice()
    for b in range(4):
        _image_trig(img, v, b == 0)
        _image_trig(img, v + 1, b == 0)
        even = img.block(v)
        odd = img.block(v + 1)
        assert even["final_pc_sw"] == CALLER_RET and odd["final_pc_sw"] == CALLER_RET, (
            even["halt"]
        )
        want = port.render(hv, frame(62), int(b == 0), mode)
        if mode:
            got = np.array(even["samples"], dtype=np.float32)
            assert odd["out"] == even["out"], (
                "the odd voice copies the even voice's block"
            )
        else:
            got = np.array(
                [
                    float(np.frombuffer(np.uint32(w).tobytes(), np.float32)[0])
                    for w in even["work"]
                ]
            )
            assert odd["work"] == even["work"]
            assert all(w != 0x7F7F7F7F for w in even["out"]), (
                "the decimator wrote the output"
            )
        assert float(np.abs(got - want).max()) <= MAX_ERR, b
        assert any(x != 0 for x in got)
        # the renderer's epilogue restored the caller's registers and frame
        assert {n: even["regs"][n] for n in CALLEE_SAVED} == {
            n: x & 0xFFFFFFFF for n, x in CALLEE_SAVED.items()
        }
        assert (even["regs"]["I6"], even["regs"]["I7"]) == (
            IMG_STACK + 0x10000,
            IMG_STACK,
        )


@needs_image
def test_image_away_on_a_stock_type_clears_active_and_plaits_stays_silent_until_trig(
    h2_images: dict[int, tuple[Path, Path]],
) -> None:
    """In the image: PLAITS plays; the type goes to SAMPLE and the stock renderer plays a sample to its end, which
    clears ACTIVE; back on PLAITS without a note-on the voice is silent; a note-on plays again."""
    from support.dsp_inline_image import IMG_RECORDS, REC
    from support.plaits_rig import ImageRig

    out, plan = h2_images[1]
    img = ImageRig(out, json.loads(plan.read_text()))
    v, t = 4, 2
    rec = IMG_RECORDS + v * REC
    _image_controls(img, t, 60)
    img.poke_byte(rec + 0x1B8, 1)
    _image_trig(img, v, True)
    assert any(img.block(v)["samples"])
    _image_trig(img, v, False)
    # away: SAMPLE (type 0) with a playing sample 20 frames before its end (docs/findings/06, the voice record)
    img.set_half(IMG_TYPES + 4 * (t >> 1), t & 1, 0)
    sample, length = 0x310000, 4000
    step = round(0.7 * 2**31)
    pokes = {
        rec: sample,
        rec + 0x17C: 0,
        rec + 0x180: 0,
        rec + 0x188: length,
        rec + 0x18C: 0,
        rec + 0x190: 0,
        rec + 0x194: 0,
        rec + 0x198: 0,
        rec + 0x19C: 0,
        rec + 0x1A0: (length << 31) & 0xFFFFFFFF,
        rec + 0x1A4: length >> 1,
        rec + 0x1A8: step & 0xFFFFFFFF,
        rec + 0x1AC: step >> 32,
        rec + 0x1B0: ((length - 20) << 31) & 0xFFFFFFFF,
        rec + 0x1B4: (length - 20) >> 1,
        rec + 0x1BC: 0,
    }
    for a, w in pokes.items():
        img.poke(a, w)
    for k in range(length // 2):
        img.poke(sample + 4 * k, (1000 + k) & 0x7FFF | (((1000 + k) & 0x7FFF) << 16))
    for row in range(256):
        for tap in range(6):
            img.poke(
                0x25D940 + 24 * row + 4 * tap,
                (0x08000000 * (tap + 1) + row * 0x1000) & 0x7FFFFFFF,
            )
    img.poke_byte(rec + 0x1B8, 1)
    away = img.block(v)
    assert away["active"] == 0, "the stock renderer ended the sample and cleared ACTIVE"
    # back to PLAITS without a note-on: silent
    img.set_half(IMG_TYPES + 4 * (t >> 1), t & 1, plaits_type())
    for _ in range(2):
        assert img.block(v)["samples"] == [0.0] * 32
    # a note-on (the trigger path arms ACTIVE): PLAITS plays
    img.poke_byte(rec + 0x1B8, 1)
    _image_trig(img, v, True)
    assert any(img.block(v)["samples"])


# Worst-case instruction bound per even-voice block. The review measured 76,236 instructions (mode 1) at extreme
# parameters and about 25,000 more in mode 0 (the half-band); the bounds leave about 15 % margin.
MAX_BLOCK_INSTRUCTIONS = {1: 88_000, 0: 118_000}
# The same for model 1 (ASYM-TRI). Measured (Task 7) on the extreme grid: 42,307 instructions (mode 1) and 65,842
# (mode 0) at worst; the bounds leave about 15 % margin. (Model 0's worst case with the model dispatch: 76,249 and
# 99,784, 13 instructions above H2's.)
MAX_BLOCK_INSTRUCTIONS_WS = {1: 49_000, 0: 76_000}
# Models 2..4 (Task 8): the worst blocks measured on the extreme grid (test_extreme_parameters_compiled_core_new_models)
# are 2FM 57,442 (mode 1) / 80,979 (mode 0), FORMANT 80,917 / 104,454, ADDITIVE 88,288 / 111,825 instructions, all at
# note 127 + TUNE 0x7f00; the bounds leave about 15 % margin. {model: {mode: bound}}.
MAX_BLOCK_INSTRUCTIONS_NEW = {
    2: {1: 66_000, 0: 93_000},
    3: {1: 93_000, 0: 120_000},
    4: {1: 101_500, 0: 128_500},
    # Task 9: WAVETABLE's extreme grid peaks at 41,168 (mode 1) / 64,705 (mode 0) instructions; CHORD's at 46,254 /
    # 69,791, but MORPH inside the crossfade (0x3c00..0x4600, outside the grid) costs more: 53,180 in set 0 of the
    # equivalence test, about 51,800 at steady settings (out/runs/task9). Bounds: about 15 % over each.
    5: {1: 47_500, 0: 74_500},
    6: {1: 62_000, 0: 86_000},
}


@pytest.mark.slow
@pytest.mark.parametrize("mode", [1, 0])
def test_extreme_parameters_compiled_core(port: Port, mode: int) -> None:
    """All {0, 0x7f00}^3 of HARMONICS/TIMBRE/MORPH at notes 0 and 127 and TUNE 0, 0x4000, 0x7f00: finite output,
    within MAX_ERR of the host build, every block within the instruction bound."""
    extremes(port, mode, 0, MAX_BLOCK_INSTRUCTIONS[mode])


@pytest.mark.slow
@pytest.mark.parametrize("mode", [1, 0])
def test_extreme_parameters_compiled_core_asym_tri(port: Port, mode: int) -> None:
    """The same grid for model 1 (ASYM-TRI)."""
    extremes(port, mode, 1, MAX_BLOCK_INSTRUCTIONS_WS[mode])


@pytest.mark.slow
@pytest.mark.parametrize("mode", [1, 0])
@pytest.mark.parametrize("model", sorted(NEW_MODELS))
def test_extreme_parameters_compiled_core_new_models(
    port: Port, model: int, mode: int
) -> None:
    """The same grid for models 2, 3 and 4."""
    extremes(port, mode, model, MAX_BLOCK_INSTRUCTIONS_NEW[model][mode])


def extremes(port: Port, mode: int, model: int, bound: int) -> None:
    rig = PlaitsRig(mode)
    rig.active(0)
    worst, worst_at = 0, None
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x7F00):
                        slots = {
                            "model": model,
                            "harmonics": h,
                            "timbre": t,
                            "morph": m,
                        }
                        rig.controls(
                            0, note, tune=tune, **{**slots, "model": model << 8}
                        )
                        hv = port.voice()
                        for b in range(3):
                            rig.trig(0, b == 0)
                            got = np.array(rig.block(0)["samples"], dtype=np.float32)
                            want = port.render(
                                hv, frame(note, tune=tune, **slots), int(b == 0), mode
                            )
                            where = (note, hex(tune), hex(h), hex(t), hex(m), b)
                            assert np.isfinite(got).all(), where
                            assert float(np.abs(got - want).max()) <= MAX_ERR, where
                            if rig.instructions[-1] > worst:
                                worst, worst_at = rig.instructions[-1], where
                            assert rig.instructions[-1] <= bound, (
                                where,
                                rig.instructions[-1],
                            )
    print(f"mode {mode} model {model}: worst block {worst} instructions at {worst_at}")
