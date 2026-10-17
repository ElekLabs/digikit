"""The PLAITS module's C sources (modules/machines/plaits/src/dsp) built for the host and driven through ctypes:
the VA engine (va_init/va_render) and the voice wrapper (plaits_render). Used by tests/plaits/test_va_host.py and
tests/patchkit/test_plaits_module.py (the reference for the compiled code on the concrete core)."""

from __future__ import annotations

import ctypes
import subprocess
from pathlib import Path
from typing import Any

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
DSP = ROOT / "modules" / "machines" / "plaits" / "src" / "dsp"
ORACLE = ROOT / "tests" / "plaits" / "oracle"
FLAGS = ["-O2", "-ffp-contract=off", "-fno-fast-math", "-shared", "-fPIC"]
Arr = Any
FP = ctypes.POINTER(ctypes.c_float)


class Params(ctypes.Structure):
    _fields_ = [(n, ctypes.c_float) for n in ("note", "harmonics", "timbre", "morph")]


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
        st = ctypes.create_string_buffer(self.va_size)
        self.lib.va_init(st)
        out = np.zeros(len(settings) * size, dtype=np.float32)
        aux = np.zeros_like(out)
        for k, s in enumerate(settings):
            p = Params(*s)
            self.lib.va_render(
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


def build_port(d: Path) -> Port:
    so = d / "libplaits_port.so"
    srcs = [
        DSP / "engines" / "va.c",
        DSP / "plaits.c",
        DSP / "halfband.c",
        ORACLE / "port_sizes.c",
    ]
    subprocess.run(
        ["cc", "-std=c99", *FLAGS, f"-I{DSP}", "-o", str(so), *map(str, srcs)],
        check=True,
    )
    return Port(so)
