"""machines/plaits on the concrete SHARC+ core: core/dsp-inline linked at test addresses (firmware addresses stood in
by L2 test pages, as in tests/patchkit/test_inline_tone.py) and the PLAITS object of machines/plaits-48k (mode 1) or
-96k (mode 0) built as pk_build builds it, its data sections loaded at test addresses. The machine state (the C
voice array in L2) persists from block to block: each block is a fresh call of `_di_entry` on the same memory.
Used by tests/patchkit/test_plaits_module.py."""

from __future__ import annotations

import struct
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import Any

from tools.patchkit import pk_build, pk_sharc, pk_toolchains

ROOT = pk_toolchains.ROOT
MODULES = ROOT / "modules"
HOOK_SRC = MODULES / "core" / "dsp-inline" / "src" / "dsp" / "dsp-inline.s"
TYPES = MODULES / "core" / "machines" / "types.toml"
VARIANT = {0: "machines/plaits-96k", 1: "machines/plaits-48k"}

CORE_BASE, PL_BASE, TAIL_BASE = 0xB90000, 0xB92000, 0xBA8000
RESUME, DECIMATE, AFTER = 0x1C4F18, 0x1C524A, 0x1C5264
RETURN_SW = 0xBAF100
CALLER_RET = (
    0x1C6B06  # after `CJUMP 0x1c4ecf` in the voice loop of FUN_1c642a (the image rig)
)
DISPLACED = bytes.fromhex("5499a801408c")
TYPE_CACHE, FRAME, RECORDS, LATCH, OUT, TABLE, STACK = (
    0x20091000,
    0x20092000,
    0x20093000,
    0x20097000,
    0x20098000,
    0x2009A000,
    0x2009C000,
)
DATA_BASE = 0x200A0000
REC = 0x1D8
# sizeof(PlaitsVoice) (plaits.h; every member 4-byte, equal on the host: tests/support/plaits_host.py Port.voice_size)
VOICE_BYTES = 876
TUNE_REST = 0x4000


def plaits_type() -> int:
    return int(
        tomllib.loads(TYPES.read_text())["dt2"]["alloc"]["machines/plaits:plaits"]
    )


def f32(w: int) -> float:
    return float(struct.unpack("<f", struct.pack("<I", w & 0xFFFFFFFF))[0])


def sharc_jump(target: int) -> bytes:
    return struct.pack("<HHH", 0x063E, (target >> 16) & 0xFF, target & 0xFFFF)


def variant_sources(mode: int) -> list[Path]:
    moddir = MODULES / VARIANT[mode]
    manifest = tomllib.loads((moddir / "module.toml").read_text())
    (obj,) = manifest["object"]
    return [moddir / s for s in obj["sources"]]


def tune_hw(t: int) -> int:
    """Frame halfword of track t's machine slot 0 (TUNE): byte 0xda + 0x60 t."""
    return (0xDA + 0x60 * t) // 2


SLOT_HW = {"tune": 0, "model": 1, "harmonics": 2, "timbre": 6, "morph": 7, "mix": 8}


class PlaitsRig:
    def __init__(self, mode: int) -> None:
        from tools.sharc_dev import sharc_run as sr
        from tools.sharc_dev import sharc_trace as st
        from tools.sharc_dev.sharcldr import LoadedMemory

        self.mode = mode
        self.st = st
        self.core = pk_sharc.link(
            pk_sharc.assemble(HOOK_SRC.read_text()),
            CORE_BASE,
            {
                "_di_table": TABLE,
                "_fw_records": RECORDS,
                "_fw_type_cache": TYPE_CACHE,
                "_fw_frame": FRAME,
                "_fw_trig_latch": LATCH,
                "_fw_decimate": DECIMATE,
                "_fw_after_decimate": AFTER,
                "_fw_stock": TAIL_BASE,
            },
        )
        self.obj = pk_build.dsp_object(variant_sources(mode))
        at = DATA_BASE
        self.data_at: dict[str, int] = {}
        for sec, (blob, align, _bss) in self.obj.data.items():
            at = -(-at // max(align, 8)) * max(align, 8)
            self.data_at[sec] = at
            at += len(blob)
        externs = {
            name: self.data_at[sec] + off
            for name, (sec, off, _g) in self.obj.data_symbols.items()
        }
        for name in ("_di_frame_hw", "_di_trig", "_di_return_work", "_di_return_out"):
            externs[name] = self.core.symbols[name]
        self.symbols = dict(externs)
        self.pl = pk_sharc.link(self.obj, PL_BASE, externs)
        tail = DISPLACED + sharc_jump(RESUME)
        payload = b""
        blocks: list[dict[str, int | bool]] = []
        for base_sw, code in (
            (CORE_BASE, self.core.code_boot),
            (PL_BASE, self.pl.code_boot),
            (TAIL_BASE, tail),
        ):
            blocks.append(
                {
                    "target_address": pk_sharc.SW_ALIAS_BASE + 2 * base_sw,
                    "byte_count": len(code),
                    "fill": False,
                    "payload_offset": len(payload),
                    "payload_len": len(code),
                    "index": len(blocks),
                }
            )
            payload += code
        for sec, (blob, _align, bss) in self.obj.data.items():
            data = bytes(len(blob)) if bss else blob
            blocks.append(
                {
                    "target_address": self.data_at[sec],
                    "byte_count": len(data),
                    "fill": False,
                    "payload_offset": len(payload),
                    "payload_len": len(data),
                    "index": len(blocks),
                }
            )
            payload += data
        self.mem = LoadedMemory(payload, blocks)
        self.runner = sr.Runner(
            self.mem,
            self.core.symbols["_di_entry"],
            breakpoints=[RESUME, DECIMATE, AFTER, RETURN_SW],
            approx_recips=True,
        )
        self.type = plaits_type()
        for base, n in (
            (TYPE_CACHE, 8),
            (FRAME, 256),
            (RECORDS, 32 * REC // 4),
            (LATCH, 8),
            (OUT, 32 * 32),
            (TABLE, 34),
            (STACK - 0x200, 0x90),
        ):
            for k in range(n):
                self.poke(base + 4 * k, 0)
        self.poke(TABLE, self.type)
        self.poke(TABLE + 4, self.pl.symbols["_pl_render"])
        self.instructions: list[int] = []
        # kept by model_blocks: voice 0's latched model and the steps since its engine was last initialised
        self.latched: int | None = None
        self.since_init: list[tuple[int, int, int, dict[str, int]]] = []

    # ---- memory
    def poke(self, addr: int, word: int) -> None:
        self.st._dm_write(self.runner.state, addr, 4, self.st.Const(word & 0xFFFFFFFF))

    def peek(self, addr: int) -> int:
        x = self.st._dm_read(self.runner.state, addr, 4)
        assert x is not None, hex(addr)
        return x.value & 0xFFFFFFFF

    def words(self, addr: int, n: int) -> list[int]:
        return [self.peek(addr + 4 * k) for k in range(n)]

    def _set_half(self, addr: int, half: int, value: int) -> None:
        sh = 16 * half
        w = self.peek(addr)
        self.poke(addr, (w & ~(0xFFFF << sh)) | ((value & 0xFFFF) << sh))

    # ---- the block's inputs
    def set_type(self, track: int, t: int) -> None:
        self._set_half(TYPE_CACHE + 4 * (track // 2), track % 2, t)

    def set_hw(self, k: int, value: int) -> None:
        self._set_half(FRAME + 4 * (k // 2), k % 2, value)

    def controls(
        self, track: int, note: int, tune: int = TUNE_REST, **slots: int
    ) -> None:
        """The track's type = PLAITS, its note and TUNE, and any of model/harmonics/timbre/morph/mix (row values)."""
        self.set_type(track, self.type)
        self.set_hw(1 + track, note << 8)
        self.set_hw(tune_hw(track), tune)
        defaults = {
            "model": 0,
            "harmonics": 0x4000,
            "timbre": 0x4000,
            "morph": 0x4000,
            "mix": 0,
        }
        for name, value in {**defaults, **slots}.items():
            self.set_hw(tune_hw(track) + SLOT_HW[name], value)

    def trig(self, v: int, on: bool = True) -> None:
        a = LATCH + 4 * (v // 4)
        sh = 8 * (v % 4)
        w = self.peek(a)
        self.poke(a, (w & ~(0xFF << sh)) | ((1 if on else 0) << sh))

    def active(self, v: int, on: bool = True) -> None:
        a = RECORDS + v * REC + 0x1B8
        w = self.peek(a)
        self.poke(a, (w & ~0xFF) | (1 if on else 0))

    def clear_voice(self, track: int) -> None:
        """Zero track's voice state (plaits_voices[track], PlaitsVoice, VOICE_BYTES each), as at load: never
        triggered."""
        base = self.symbols["plaits_voices."] + track * VOICE_BYTES
        for k in range(VOICE_BYTES // 4):
            self.poke(base + 4 * k, 0)

    # ---- one block
    def block(
        self,
        v: int,
        max_steps: int = 2_000_000,
        want: int | None = None,
        on_step: Callable[[Any], None] | None = None,
    ) -> dict:
        """One render block of voice v through the hook (as the renderer calls it at SW 0x1c4f15), run to the
        continuation `want` (default: the variant's return, AFTER for mode 1, DECIMATE for mode 0; RESUME is the
        stock path of a type without a row). `on_step(state)` sees the core state before every instruction (an
        interrupt could be taken at any of those points)."""
        rec, outp = RECORDS + v * REC, OUT + v * 0x80
        for off, val in {-4: 32, -6: rec, -7: outp, -5: 0xDEAD}.items():
            self.poke(STACK + 4 * off, val)
        for k in range(32):
            self.poke(outp + 4 * k, 0x7F7F7F7F)
        regs: dict[str | int, int | str] = {
            "R0": 64,
            "R4": rec,
            "I4": rec,
            "I6": STACK,
            "I7": STACK - 104,
            # the firmware's stack is a circular buffer (B6 = B7, L6 = L7 set by its startup, SW 0x1c0f64)
            "B6": STACK - 0x400,
            "B7": STACK - 0x400,
            "L6": 0x800,
            "L7": 0x800,
            "MODE1": 0x1011800,  # the firmware's MODE1 (startup, finding 06): CBUFEN (bit 24) on
            "M5": 0,
            "M6": 1,
            "M7": -1,
            "M13": 0,
            "M14": 1,
            "M15": -1,
        }
        r = self.runner.fresh_call(
            self.core.symbols["_di_entry"], regs=regs, return_address=RETURN_SW
        )
        if on_step is not None:
            step = r.step

            def stepped() -> None:
                on_step(r.state)
                step()

            r.step = stepped  # type: ignore[method-assign]
        res = r.run(max_steps)
        self.runner = r
        if want is None:
            want = AFTER if self.mode else DECIMATE
        assert res.halt.reason == "breakpoint", res.halt.to_json()
        assert res.final_pc_sw == want, hex(res.final_pc_sw)
        self.instructions.append(res.instructions)
        uregs = r.state.uregs
        regs_out = {
            n: getattr(uregs.get(self.st.UREG_CODES[n]), "value", None)
            for n in (
                "I6",
                "I7",
                "M5",
                "M6",
                "M7",
                "M13",
                "M14",
                "M15",
                "I8",
                "I13",
                "M0",
                "M8",
                "L6",
                "L7",
                "B6",
                "B7",
            )
        }
        work = self.words(rec + 4, 64)
        out = self.words(outp, 32)
        return {
            "final": res.final_pc_sw,
            "work": work,
            "out": out,
            "samples": [f32(w) for w in (out if self.mode else work)],
            "regs": regs_out,
            "instructions": res.instructions,
        }


C_STACK = 0x200F0000  # a C frame for direct calls: [C_STACK] = 0 (caller frame), [C_STACK - 4] = return address - 1


def ccall(
    rig: PlaitsRig, name: str, args: list[int], max_steps: int = 2_000_000
) -> int:
    """Call the compiled C function `name` (its symbol, e.g. `va_render.`) with integer/pointer/raw-float `args`
    (R4, R8, R12, then the stack) on the rig's memory; return R0."""
    rig.poke(C_STACK, 0)
    rig.poke(C_STACK - 4, RETURN_SW - 1)
    regs: dict[str | int, int | str] = {
        "I6": C_STACK,
        "M5": 0,
        "M6": 1,
        "M7": -1,
        "M13": 0,
        "M14": 1,
        "M15": -1,
    }
    for reg, a in zip(("R4", "R8", "R12"), args[:3], strict=False):
        regs[reg] = a & 0xFFFFFFFF
    # stack arguments: the 4th at [I6 + 4], the 5th at [I6 + 8], ...
    for k, a in enumerate(args[3:]):
        rig.poke(C_STACK + 4 * (k + 1), a)
    regs["I7"] = C_STACK - 8
    r = rig.runner.fresh_call(rig.pl.symbols[name], regs=regs, return_address=RETURN_SW)
    res = r.run(max_steps)
    rig.runner = r
    assert res.halt.reason == "breakpoint" and res.final_pc_sw == RETURN_SW, (
        res.halt.to_json()
    )
    v = r.state.uregs.get(rig.st.UREG_CODES["R0"])
    return int(getattr(v, "value", 0) or 0)


class ImageRig:
    """A patched 1.17 image's own DSP code on the concrete core (tests/support/dsp_inline_image.py), the memory kept
    from block to block: each block calls the stock renderer (SW 0x1c4ecf) for one voice the way the voice loop of
    FUN_1c642a does and runs to the caller's return point."""

    def __init__(self, syx: Path, plan: dict) -> None:
        from support.dsp_inline_image import IMG_STACK, image_memory
        from tools.sharc_dev import sharc_run as sr
        from tools.sharc_dev import sharc_trace as st

        self.st = st
        self.stack = IMG_STACK
        self.symbols = {k.split(":", 1)[1]: v for k, v in plan["symbols"].items()}
        self.runner = sr.Runner(
            image_memory(syx), 0x1C4ECF, breakpoints=[CALLER_RET], approx_recips=True
        )
        self.runner.state.explicit_memory_model = True

    def poke(self, addr: int, word: int) -> None:
        self.st._dm_write(self.runner.state, addr, 4, self.st.Const(word & 0xFFFFFFFF))

    def poke_byte(self, addr: int, byte: int) -> None:
        self.st._dm_write(self.runner.state, addr, 1, self.st.Const(byte & 0xFF))

    def peek(self, addr: int) -> int:
        x = self.st._dm_read(self.runner.state, addr, 4)
        return 0 if x is None else x.value & 0xFFFFFFFF

    def words(self, addr: int, n: int) -> list[int]:
        return [self.peek(addr + 4 * k) for k in range(n)]

    def set_half(self, addr: int, half: int, value: int) -> None:
        sh = 16 * half
        self.poke(addr, (self.peek(addr) & ~(0xFFFF << sh)) | ((value & 0xFFFF) << sh))

    def block(self, v: int) -> dict:
        from support.dsp_inline_image import CALLEE_SAVED, IMG_OUT, IMG_RECORDS, REC

        rec, outp = IMG_RECORDS + v * REC, IMG_OUT + v * 0x80
        regs: dict[str | int, int | str] = {
            "R4": rec,
            "R8": outp,
            "R12": 32,
            "R14": 32 - v,
            "M5": 0,
            "M6": 1,
            "M7": -1,
            "M13": 0,
            "M14": 1,
            "M15": -1,
            "I6": self.stack,
            "I7": self.stack - 8,
            "B6": self.stack - 0x400,
            "B7": self.stack - 0x400,
            "L6": 0x800,
            "L7": 0x800,
            "MODE1": 0x1011800,
        }
        regs.update(CALLEE_SAVED)
        self.poke(self.stack - 4, CALLER_RET - 1)
        self.poke(self.stack, self.stack + 0x10000)
        for k in range(32):
            self.poke(outp + 4 * k, 0x7F7F7F7F)
        r = self.runner.fresh_call(0x1C4ECF, regs=regs, return_address=CALLER_RET)
        r.state.explicit_memory_model = True
        res = r.run(3_000_000)
        self.runner = r
        uregs = r.state.uregs
        return {
            "halt": res.halt.to_json(),
            "final_pc_sw": res.final_pc_sw,
            "instructions": res.instructions,
            "out": self.words(outp, 32),
            "samples": [f32(w) for w in self.words(outp, 32)],
            "work": self.words(rec + 4, 64),
            "active": self.peek(rec + 0x1B8) & 0xFF,
            "regs": {
                n: getattr(uregs.get(self.st.UREG_CODES[n]), "value", None)
                for n in (*CALLEE_SAVED, "I6", "I7")
            },
        }


def model_blocks(
    rig: PlaitsRig, steps: list[tuple[int, int]], note: int = 57, **slots: int
) -> list[Any]:
    """Voice 0 (track 0's even voice) of `rig`, one block per (model, trig) step: MODEL set to the model's row value
    (model << 8) and the trigger latch to trig; the other rows from `slots` (defaults as `controls`). Keeps
    `rig.since_init`, the steps played since the voice's engine was last initialised (its first note-on, or a note-on
    that changed the model: plaits.c note_on), which determine its state."""
    import numpy as np

    out = []
    for model, trig in steps:
        if trig and rig.latched != model:
            rig.latched = model
            rig.since_init = []
        rig.since_init.append((model, trig, note, dict(slots)))
        rig.controls(0, note, model=model << 8, **slots)
        rig.trig(0, bool(trig))
        out.append(np.array(rig.block(0)["samples"], dtype=np.float32))
    return out


def assert_model_switch(
    rig: PlaitsRig, a: int, b: int, note: int = 57, **slots: int
) -> list[tuple[int, int]]:
    """A model change on one voice of `rig` (whatever it played before, through `model_blocks`): a note-on of model a
    and 4 blocks; MODEL turned to b without a note-on (2 blocks: a runs on, bit for bit as a fresh rig's voice that
    replays the steps since the voice's last initialisation and then plays a without the change: a note-on of the
    model the voice already runs leaves its engine running); a note-on of b and 5 blocks (bit for bit a fresh rig's
    voice of b: nothing of the previous engine's state survives in the shared union). The new model must sound.
    Returns the (model, trig) steps it played, for a comparison with the host. Used by
    tests/patchkit/test_plaits_module.py for every enabled model."""
    import numpy as np

    rig.active(0)
    history = list(rig.since_init)
    steps = [(a, int(k == 0)) for k in range(4)] + [(b, 0)] * 2
    steps += [(b, int(k == 0)) for k in range(5)]
    got = model_blocks(rig, steps, note, **slots)
    ref = PlaitsRig(rig.mode)
    ref.active(0)
    for m, t, n, sl in history:
        model_blocks(ref, [(m, t)], n, **sl)
    want_a = model_blocks(ref, [(a, int(k == 0)) for k in range(6)], note, **slots)
    assert all(np.array_equal(x, y) for x, y in zip(got[:6], want_a, strict=True)), (
        f"model {a} did not run on through MODEL {b} without a note-on"
    )
    fresh = PlaitsRig(rig.mode)
    fresh.active(0)
    want_b = model_blocks(fresh, [(b, int(k == 0)) for k in range(5)], note, **slots)
    assert all(np.array_equal(x, y) for x, y in zip(got[6:], want_b, strict=True)), (
        f"model {b} after model {a} differs from a fresh voice of model {b}"
    )
    assert any(x.any() for x in want_b), f"model {b} is silent"
    return steps
