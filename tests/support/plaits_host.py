"""The PLAITS module's C sources (modules/machines/plaits/src/dsp) built for the host and driven through ctypes:
the engines (va, ws, fm, grain, additive, wavetable, chord, speech, modal, terrain: <name>_init/<name>_render) and the voice wrapper (plaits_render); and the upstream oracle
build (tests/plaits/oracle). Used by tests/plaits/test_va_host.py and
tests/patchkit/test_plaits_module.py (the reference for the compiled code on the concrete core)."""

from __future__ import annotations

import ctypes
import os
import re
import subprocess
from pathlib import Path
from typing import Any, NoReturn

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[2]
DSP = ROOT / "modules" / "machines" / "plaits" / "src" / "dsp"
ORACLE = ROOT / "tests" / "plaits" / "oracle"
EURORACK = ROOT.parent / "eurorack"
FLAGS = ["-O2", "-ffp-contract=off", "-fno-fast-math", "-shared", "-fPIC"]
Arr = Any
FP = ctypes.POINTER(ctypes.c_float)


class Params(ctypes.Structure):
    """engines/engine.h Params: Plaits' EngineParameters (trigger and accent are read by SPEECH and RINGS only; 0
    when a setting gives four values)."""

    _fields_ = [
        *[(n, ctypes.c_float) for n in ("note", "harmonics", "timbre", "morph")],
        ("trigger", ctypes.c_int),
        ("accent", ctypes.c_float),
    ]


class Frame(ctypes.Structure):
    _fields_ = [
        *[
            (n, ctypes.c_float)
            for n in (
                "pitch",
                "tune",
                "model",
                "harmonics",
                "timbre",
                "morph",
                "mix",
                "velocity",
            )
        ],
        ("active", ctypes.c_int),
    ]


class Port:
    """The module's C sources built for the host."""

    # set from the library in __init__ (port_<name>_size) for the engines without a named method
    modal_size: int
    terrain_size: int

    def __init__(self, so: Path) -> None:
        self.lib = ctypes.CDLL(str(so))
        self.va_size = self.lib.port_va_size()
        self.ws_size = self.lib.port_ws_size()
        self.lib.port_ws_lut.restype = ctypes.c_float
        self.lib.ws_init.argtypes = [ctypes.c_void_p]
        self.lib.ws_render.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(Params),
            FP,
            FP,
            ctypes.c_int,
        ]
        for name in (
            "fm",
            "grain",
            "additive",
            "wavetable",
            "chord",
            "speech",
            "modal",
            "terrain",
        ):
            setattr(self, f"{name}_size", getattr(self.lib, f"port_{name}_size")())
            getattr(self.lib, f"{name}_init").argtypes = [ctypes.c_void_p]
            getattr(self.lib, f"{name}_render").argtypes = [
                ctypes.c_void_p,
                ctypes.POINTER(Params),
                FP,
                FP,
                ctypes.c_int,
            ]
        self.engine_union_size = self.lib.port_engine_union_size()
        self.lib.port_lib_lut.restype = ctypes.c_float
        self.voice_size = self.lib.port_voice_size()
        assert self.lib.port_frame_size() == ctypes.sizeof(Frame)
        self.lib.va_init.argtypes = [ctypes.c_void_p]
        self.lib.va_render.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(Params),
            FP,
            FP,
            ctypes.c_int,
        ]
        self.lib.plaits_render.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(Frame),
            ctypes.c_int,
            FP,
            ctypes.c_int,
            ctypes.c_int,
        ]
        self.lib.plaits_render.restype = ctypes.c_int
        self.lib.plaits_voice_clear.argtypes = [ctypes.c_void_p]
        self.lib.plaits_dc_block.argtypes = [ctypes.c_void_p, FP, ctypes.c_int]
        self.lib.port_out_gain.restype = ctypes.c_float
        self.lib.port_aux_gain.restype = ctypes.c_float

    def va(
        self, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        """OUT and AUX for one block of `size` per entry of `settings` (note, harmonics, timbre, morph)."""
        return self.engine("va", settings, size)

    def ws(
        self, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        """The waveshaping engine (ASYM-TRI), as `va`."""
        return self.engine("ws", settings, size)

    def engine(
        self, name: str, settings: list[tuple[float, float, float, float]], size: int
    ) -> tuple[Arr, Arr]:
        st = ctypes.create_string_buffer(getattr(self, f"{name}_size"))
        getattr(self.lib, f"{name}_init")(st)
        render = getattr(self.lib, f"{name}_render")
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            p = Params(*s)
            render(
                st,
                ctypes.byref(p),
                out[k * size :].ctypes.data_as(FP),
                aux[k * size :].ctypes.data_as(FP),
                size,
            )
        return out, aux

    def voice(self) -> ctypes.Array[ctypes.c_char]:
        v = ctypes.create_string_buffer(self.voice_size)
        self.lib.plaits_voice_clear(v)
        return v

    def render(
        self, v: ctypes.Array[ctypes.c_char], f: Frame, trig: int, mode: int
    ) -> Arr:
        """One Digitakt block of the wrapper: 32 samples at 48 kHz (mode 1) or 64 at 96 kHz (mode 0)."""
        n = 32 if mode else 64
        out = np.zeros(n, dtype=np.float32)
        self.lib.plaits_render(
            v,
            ctypes.byref(f),
            trig,
            out.ctypes.data_as(FP),
            n,
            48000 if mode else 96000,
        )
        return out


def frame(
    note: int,
    tune: int = 0x4000,
    model: int = 0,
    harmonics: int = 0x4000,
    timbre: int = 0x4000,
    morph: int = 0x4000,
    mix: int = 0,
    active: int = 1,
    velocity: int = 100,
) -> Frame:
    """A PlaitsFrame from the 16-bit row values, as the entry shim converts them (velocity: 0..127, the frame word is
    velocity << 8; 100 is the Digitakt's default)."""
    return Frame(
        float(note << 8),
        float(tune),
        float(model << 8),
        float(harmonics),
        float(timbre),
        float(morph),
        float(mix),
        float(velocity << 8),
        active,
    )


def build_port(d: Path, defines: tuple[str, ...] = ()) -> Port:
    """The module's C sources and tests/plaits/oracle/port_sizes.c as d/libplaits_port.so; `defines` are extra -D
    macros (PL_INDEX_CHECK: the engines report their table indices to port_sizes.c's recorder)."""
    so = d / "libplaits_port.so"
    srcs = [
        DSP / "engines" / "va.c",
        DSP / "engines" / "waveshaping.c",
        DSP / "engines" / "fm.c",
        DSP / "engines" / "grain.c",
        DSP / "engines" / "additive.c",
        DSP / "engines" / "wavetable.c",
        DSP / "engines" / "chord.c",
        DSP / "engines" / "speech.c",
        DSP / "engines" / "modal.c",
        DSP / "engines" / "terrain.c",
        DSP / "lib" / "tables.c",
        DSP / "lib" / "waves.c",
        DSP / "lib" / "wavetable_oscillator.c",
        DSP / "lib" / "string_synth_oscillator.c",
        DSP / "lib" / "chord_bank.c",
        DSP / "lib" / "svf.c",
        DSP / "lib" / "naive_speech_synth.c",
        DSP / "lib" / "sam_speech_synth.c",
        DSP / "lib" / "lpc_speech_synth.c",
        DSP / "lib" / "speech_tables.c",
        DSP / "lib" / "resonator.c",
        DSP / "lib" / "fast_sine_oscillator.c",
        DSP / "lib" / "terrain_memory.c",
        DSP / "plaits.c",
        DSP / "halfband.c",
        ORACLE / "port_sizes.c",
    ]
    subprocess.run(
        # -fno-strict-aliasing: the engines read their bit-pattern tables through a float pointer (ws_tables.h,
        # lib/tables.h), and itof_nonneg its union through one
        [
            "cc",
            "-std=c99",
            *FLAGS,
            "-fno-strict-aliasing",
            *(f"-D{x}" for x in defines),
            f"-I{DSP}",
            "-o",
            str(so),
            *map(str, srcs),
        ],
        check=True,
    )
    return Port(so)


def stmlib_root() -> Path | None:
    """A directory holding upstream's stmlib/ (PLAITS_STMLIB, else ../eurorack when its submodule is populated)."""
    for cand in (os.environ.get("PLAITS_STMLIB"), str(EURORACK)):
        if cand and (Path(cand) / "stmlib" / "stmlib.h").exists():
            return Path(cand)
    return None


def oracle_missing(why: str) -> NoReturn:
    """Skip the oracle tests, or fail them under PLAITS_ORACLE_REQUIRED=1 (a gate that must run the oracle)."""
    if os.environ.get("PLAITS_ORACLE_REQUIRED") == "1":
        pytest.fail(f"PLAITS_ORACLE_REQUIRED=1: {why}")
    pytest.skip(why)


ORACLE_MISSING = (
    "BIT-EXACT ORACLE DISABLED: upstream stmlib not found (../eurorack/stmlib is empty). Enable it with "
    "`git -C ../eurorack submodule update --init stmlib` (commit e3bd7c9) or "
    "PLAITS_STMLIB=<directory holding a stmlib/ checkout at e3bd7c9, the commit ../eurorack pins>, "
    "e.g. PLAITS_STMLIB=../mutable_instruments_eurorack; ../eurorack/plaits is needed too"
)


def build_oracle(d: Path, sources: list[Path], name: str) -> Path:
    """Build upstream sources (paths relative to ../eurorack/plaits or absolute) with the oracle's dsp.h override
    into d/lib<name>.so; skips (or fails, PLAITS_ORACLE_REQUIRED=1) without upstream."""
    stm = stmlib_root()
    if stm is None or not (EURORACK / "plaits" / "dsp" / "engine").exists():
        oracle_missing(ORACLE_MISSING)
    inc = d / "stm"
    inc.mkdir(exist_ok=True)
    if not (inc / "stmlib").exists():
        (inc / "stmlib").symlink_to(stm / "stmlib")
    so = d / f"lib{name}.so"
    srcs = [*sources, stm / "stmlib" / "dsp" / "units.cc"]
    subprocess.run(
        [
            "c++",
            "-std=c++14",
            *FLAGS,
            f"-I{ORACLE / 'include'}",
            f"-I{EURORACK}",
            f"-I{inc}",
            "-o",
            str(so),
            *map(str, srcs),
        ],
        check=True,
    )
    return so


# ---- the wrapper's output stage in float32 (plaits.c post, the mix, the DC blocker and the gain)

INV_FULL = np.float32(3.0757874e-05)
DC_R = np.float32(0.99934550)
# plaits.c plaits_gain: 0.601 (Task 6's calibration of default VA) / 0.8 (VA's OUT gain in Plaits)
GAIN = np.float32(0.75125)
# plaits.c post: Plaits' Clip16 of its DAC step, in full-scale units (Clip16(1 + (int)(x * gain * -32767)))
CLIP_LO, CLIP_HI = np.float32(-1.0), np.float32(1.0)


def upstream_gains() -> list[tuple[np.float32, np.float32]]:
    """Plaits' OUT and AUX gains of each PLAITS model n, read from upstream's plaits/dsp/voice.cc (RegisterInstance in
    registration order; model n is upstream's engine (n + 8) mod 24)."""
    voice = EURORACK / "plaits" / "dsp" / "voice.cc"
    if not voice.exists():
        oracle_missing(ORACLE_MISSING)
    regs = re.findall(
        r"RegisterInstance\(&\w+?_, (?:true|false), (-?[\d.]+)f, (-?[\d.]+)f\)",
        voice.read_text(),
    )
    assert len(regs) == 24, len(regs)
    return [
        (np.float32(regs[(n + 8) % 24][0]), np.float32(regs[(n + 8) % 24][1]))
        for n in range(24)
    ]


def limiter(peak: np.float32, pre: np.float32, x: Arr) -> tuple[np.float32, Arr]:
    """stmlib's Limiter::Process(pre, x) in float32: (peak_ after, output)."""
    out = np.zeros(len(x), dtype=np.float32)
    c_up, c_down, one, k = (
        np.float32(0.05),
        np.float32(0.00002),
        np.float32(1.0),
        np.float32(0.8),
    )
    for i, v in enumerate(np.asarray(x, dtype=np.float32)):
        s = np.float32(v * pre)
        error = np.float32(np.float32(abs(s)) - peak)
        peak = np.float32(peak + np.float32((c_up if error > 0 else c_down) * error))
        g = one if peak <= one else np.float32(one / peak)
        out[i] = np.float32(np.float32(s * g) * k)
    return peak, out


class Tail:
    """The wrapper after the engine for one voice: Plaits' output stage per model (both limiters initialised at the
    first note-on, the OUT one again at each model change), OUT + MIX (AUX - OUT), the DC blocker and the gain."""

    def __init__(self) -> None:
        self.gains = upstream_gains()
        self.out_peak = self.aux_peak = np.float32(0.5)
        self.x1 = self.y1 = np.float32(0.0)

    def select(self) -> None:
        """A note-on that initialises the engine (the voice's first or a model change)."""
        self.out_peak = np.float32(0.5)
        self.x1 = self.y1 = np.float32(0.0)

    def post(self, model: int, out: Arr, aux: Arr) -> tuple[Arr, Arr]:
        go, ga = self.gains[model]
        res = []
        for which, g, x in (("out", go, out), ("aux", ga, aux)):
            x = np.asarray(x, dtype=np.float32)
            if g < 0:
                pk, y = limiter(getattr(self, f"{which}_peak"), np.float32(-g), x)
                setattr(self, f"{which}_peak", pk)
            else:
                y = (x * g).astype(np.float32)
            res.append(np.clip(y, CLIP_LO, CLIP_HI).astype(np.float32))
        return res[0], res[1]

    def block(self, model: int, out: Arr, aux: Arr, mix: np.float32) -> Arr:
        o, a = self.post(model, out, aux)
        eng = (o + mix * (a - o)).astype(np.float32)
        res = np.zeros_like(eng)
        x1, y1 = self.x1, self.y1
        for i, x in enumerate(eng):
            y1 = np.float32(np.float32(x - x1) + np.float32(DC_R * y1))
            x1 = x
            res[i] = np.float32(y1 * GAIN)
        self.x1, self.y1 = x1, y1
        return res


def tail(model: int, out: Arr, aux: Arr, mix: np.float32) -> Arr:
    """A fresh voice's wrapper output for the engine output (out, aux) of `model` at a constant MIX `mix`."""
    return Tail().block(model, out, aux, mix)
