"""The PLAITS module's C sources (modules/machines/plaits/src/dsp) built for the host and driven through ctypes:
the engines (va, ws, fm, grain, additive, wavetable, chord, speech: <name>_init/<name>_render) and the voice wrapper (plaits_render); and the upstream oracle
build (tests/plaits/oracle). Used by tests/plaits/test_va_host.py and
tests/patchkit/test_plaits_module.py (the reference for the compiled code on the concrete core)."""

from __future__ import annotations

import ctypes
import os
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
    """engines/engine.h Params: Plaits' EngineParameters (trigger and accent are read by SPEECH only; 0 when a
    setting gives four values)."""

    _fields_ = [
        *[(n, ctypes.c_float) for n in ("note", "harmonics", "timbre", "morph")],
        ("trigger", ctypes.c_int),
        ("accent", ctypes.c_float),
    ]


class Frame(ctypes.Structure):
    _fields_ = [
        *[
            (n, ctypes.c_float)
            for n in ("pitch", "tune", "model", "harmonics", "timbre", "morph", "mix")
        ],
        ("active", ctypes.c_int),
    ]


class Port:
    """The module's C sources built for the host."""

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
        for name in ("fm", "grain", "additive", "wavetable", "chord", "speech"):
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
) -> Frame:
    """A PlaitsFrame from the 16-bit row values, as the entry shim converts them."""
    return Frame(
        float(note << 8),
        float(tune),
        float(model << 8),
        float(harmonics),
        float(timbre),
        float(morph),
        float(mix),
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
