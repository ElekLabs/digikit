"""The continuous host oracle (tests/plaits/oracle/run_oracle.cc) built and driven through ctypes: upstream Plaits'
engines of the ported models (0..7, 12 RINGS, 21 TERRAIN) with a uniform select/render interface, and upstream's
output-stage limiters (run_oracle_post*). Used by tests/plaits/test_continuous_host.py (the wrapper rendered
continuously), test_modal_host.py and test_terrain_host.py (the engines on their own)."""

from __future__ import annotations

import ctypes
from pathlib import Path
from typing import Any

import numpy as np

from support.plaits_host import (
    DC_R,
    EURORACK,
    FP,
    GAIN,
    INV_FULL,
    build_oracle,
    stmlib_root,
    upstream_gains,
)

ORACLE = Path(__file__).resolve().parents[1] / "plaits" / "oracle"
Arr = Any
# a step: (note, harmonics, timbre, morph, trigger, accent) for one sub-block
Step = tuple[float, float, float, float, int, float]
# a block of the wrapper's controls (tests/support/plaits_streams.py): row values, the trigger latch, velocity
Block = dict[str, int]
INV_256 = np.float32(0.00390625)
RISING_HIGH, HIGH = 1 | 4, 4
SUB = 16


class Oracle:
    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.lib.run_oracle_select.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.lib.run_oracle_render.argtypes = [
            ctypes.c_void_p,
            *[ctypes.c_float] * 4,
            ctypes.c_int,
            ctypes.c_float,
            FP,
            FP,
            ctypes.c_int,
        ]
        self.size = self.lib.run_oracle_size()
        self.post_size = self.lib.run_oracle_post_size()
        for name in ("run_oracle_post_init", "run_oracle_post_reset"):
            getattr(self.lib, name).argtypes = [ctypes.c_void_p]
        self.lib.run_oracle_post.argtypes = [
            ctypes.c_void_p,
            ctypes.c_float,
            ctypes.c_float,
            FP,
            FP,
            ctypes.c_int,
        ]

    def engine(self, model: int, steps: list[Step], size: int = 16) -> tuple[Arr, Arr]:
        """Upstream's engine of `model` selected as Plaits' Voice does (Init, LoadUserData(NULL), Reset), then one
        render of `size` samples per step: OUT and AUX."""
        st = ctypes.create_string_buffer(self.size)
        self.lib.run_oracle_select(st, model)
        out = np.zeros(len(steps) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, (note, h, t, m, trig, accent) in enumerate(steps):
            self.lib.run_oracle_render(
                st,
                note,
                h,
                t,
                m,
                trig,
                accent,
                out[k * size :].ctypes.data_as(FP),
                aux[k * size :].ctypes.data_as(FP),
                size,
            )
        return out, aux


def build(d: Path) -> Oracle:
    plaits = EURORACK / "plaits"
    engine = plaits / "dsp" / "engine"
    # resources.cc comes before the speech sources: the linker lays out each file's constants in this order, so the
    # memory after wav_integrated_waves holds upstream's LPC word data (TERRAIN reads there: see MEMORY_MODEL)
    sources = [
        ORACLE / "run_oracle.cc",
        engine / "virtual_analog_engine.cc",
        engine / "waveshaping_engine.cc",
        engine / "fm_engine.cc",
        engine / "grain_engine.cc",
        engine / "additive_engine.cc",
        engine / "wavetable_engine.cc",
        engine / "chord_engine.cc",
        engine / "speech_engine.cc",
        engine / "modal_engine.cc",
        plaits / "dsp" / "engine2" / "wave_terrain_engine.cc",
        plaits / "dsp" / "chords" / "chord_bank.cc",
        plaits / "dsp" / "physical_modelling" / "modal_voice.cc",
        plaits / "dsp" / "physical_modelling" / "resonator.cc",
        plaits / "resources.cc",
        *(
            plaits / "dsp" / "speech" / f"{n}.cc"
            for n in (
                "lpc_speech_synth_words",
                "naive_speech_synth",
                "sam_speech_synth",
                "lpc_speech_synth",
                "lpc_speech_synth_controller",
                "lpc_speech_synth_phonemes",
            )
        ),
    ]
    stm = stmlib_root()
    if stm is not None:
        sources.append(stm / "stmlib" / "utils" / "random.cc")
    return Oracle(build_oracle(d, sources, "run_oracle"))


def port_engine(
    port: Any, name: str, steps: list[Step], size: int = 16
) -> tuple[Arr, Arr]:
    """The port's engine `name` from <name>_init, one render per step (Params with trigger and accent)."""
    return port.engine(name, [tuple(s) for s in steps], size)


def same(a: Arr, b: Arr) -> bool:
    return bool(
        np.array_equal(np.asarray(a).view(np.uint32), np.asarray(b).view(np.uint32))
    )


# ---- the wrapper rendered from upstream (tests/plaits/test_continuous_host.py, the digests)


def _unit(word: int) -> np.float32:
    return np.float32(
        min(
            max(np.float32(np.float32(word) * INV_FULL), np.float32(0.0)),
            np.float32(1.0),
        )
    )


def reference(
    oracle: Oracle,
    blocks: list[Block],
    gains: list[tuple[np.float32, np.float32]] | None = None,
) -> Arr:
    """Upstream's engine, selected at the first note-on and at each note-on that changes the model, rendered
    continuously with the accent of the last note-on's velocity; upstream's output stage (gains from voice.cc, the
    limiters); then OUT + MIX (AUX - OUT), the DC blocker (cleared at each selection) and the gain, as plaits.c."""
    st = ctypes.create_string_buffer(oracle.size)
    post = ctypes.create_string_buffer(oracle.post_size)
    gains = gains or upstream_gains()
    out16 = np.zeros(SUB, dtype=np.float32)
    aux16 = np.zeros(SUB, dtype=np.float32)
    latched = -1
    accent = np.float32(0.0)
    x1 = y1 = np.float32(0.0)
    res = []
    for s in blocks:
        if s["trig"]:
            accent = _unit(s.get("velocity", 100) << 8)
        if s["trig"] and s["model"] != latched:
            if latched < 0:
                oracle.lib.run_oracle_post_init(post)
            else:
                oracle.lib.run_oracle_post_reset(post)
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
            trig = RISING_HIGH if (s["trig"] and sub == 0) else HIGH
            oracle.lib.run_oracle_render(
                st,
                float(note),
                float(h),
                float(t),
                float(m),
                trig,
                float(accent),
                out16.ctypes.data_as(FP),
                aux16.ctypes.data_as(FP),
                SUB,
            )
            go, ga = gains[latched]
            oracle.lib.run_oracle_post(
                post,
                float(go),
                float(ga),
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


def up2(port: Any, x48: Arr, blocks: list[Block]) -> Arr:
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
