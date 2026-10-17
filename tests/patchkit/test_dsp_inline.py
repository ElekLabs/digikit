"""core/dsp-inline on the concrete SHARC+ core, and its patched 1.17 image through the encoding gate.

The hook object is assembled from the module source and linked at a test base, with the firmware addresses it reads
(voice records, type cache, frame copy, trigger latch, registry rows) stood in by L2 test pages. The three places it
continues at keep their real SW addresses (0x1c4f18, 0x1c524a, 0x1c5264) and are breakpoints. patchkit's tail is built
as patchkit builds it: the six displaced bytes, then `JUMP 0x1c4f18`. The run starts where the hook jumps from: I4 = R4
= the record, R0 = 64, I6 = the renderer's frame (DM(-4) = 32, DM(-6) = the record, DM(-7) = the output). Skips when
Selache is not built; the gate test also needs the stock image and the 1.17 sharcdb."""

import struct
from pathlib import Path

import pytest

from tools.patchkit import pk_sharc, pk_toolchains

ROOT = pk_toolchains.ROOT
SRC = ROOT / "modules" / "core" / "dsp-inline" / "src" / "dsp" / "dsp-inline.s"

pytestmark = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)

CORE_BASE, ROUTINE_BASE, TAIL_BASE = 0xB90000, 0xB92000, 0xB94000
RESUME, DECIMATE, AFTER = (
    0x1C4F18,
    0x1C524A,
    0x1C5264,
)  # real SW addresses, breakpoints here
RETURN_SW = 0xB9F100  # where the helpers return to
DISPLACED = bytes.fromhex("5499a801408c")  # SW 0x1c4f15..0x1c4f17 of the 1.17 renderer
# byte addresses of the stand-ins (L2)
TYPE_CACHE, FRAME, RECORDS, LATCH, OUT, TABLE, PROBE, STACK = (
    0x20091000,
    0x20092000,
    0x20093000,  # 32 records, 15104 bytes
    0x20097000,
    0x20098000,
    0x2009A000,
    0x2009B000,
    0x2009C000,
)
REC = 0x1D8
MODE0, MODE1 = 8, 9  # the types the two test routines are registered for

ROUTINES = """
.SECTION/PM seg_pmco;
.GLOBAL _t_mode0;
.GLOBAL _t_mode1;
.EXTERN _t_probe;
_t_mode0:
    I0 = _t_probe;
    DM(I0, M6) = R0;
    DM(I0, M6) = R1;
    DM(I0, M6) = R2;
    DM(I0, M6) = R4;
    R3 = I1;
    DM(I0, M6) = R3;
    R3 = I2;
    DM(I0, M6) = R3;
    R3 = 0x3f800000;
    R5 = 64;
_t_loop0:
    DM(I1, M6) = R3;
    R5 = R5 - 1;
    IF NE JUMP _t_loop0;
    R0 = 0;
    JUMP (M13, I13);
_t_mode1:
    I0 = _t_probe;
    DM(I0, M6) = R0;
    DM(I0, M6) = R1;
    DM(I0, M6) = R2;
    DM(I0, M6) = R4;
    R3 = I1;
    DM(I0, M6) = R3;
    R3 = I2;
    DM(I0, M6) = R3;
    R3 = 0x40000000;
    R5 = 32;
_t_loop1:
    DM(I2, M6) = R3;
    R5 = R5 - 1;
    IF NE JUMP _t_loop1;
    R0 = 1;
    JUMP (M13, I13);
"""


def nw(byte: int) -> int:
    return byte // 4


def bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def sharc_jump(target: int) -> bytes:
    """`JUMP target` as patchkit writes it (packages/patchkit/src/prim.rs sharc_jump): three parcels, little endian."""
    return struct.pack("<HHH", 0x063E, (target >> 16) & 0xFF, target & 0xFFFF)


def record(v: int) -> int:
    return RECORDS + v * REC


def output(v: int) -> int:
    return OUT + v * 0x80


class Rig:
    def __init__(self) -> None:
        externs = {
            "_di_table": TABLE,
            "_fw_records": RECORDS,
            "_fw_type_cache": TYPE_CACHE,
            "_fw_frame": FRAME,
            "_fw_trig_latch": LATCH,
            "_fw_decimate": DECIMATE,
            "_fw_after_decimate": AFTER,
            "_fw_stock": TAIL_BASE,
        }
        self.core = pk_sharc.link(
            pk_sharc.assemble(SRC.read_text()), CORE_BASE, externs
        )
        self.routines = pk_sharc.link(
            pk_sharc.assemble(ROUTINES), ROUTINE_BASE, {"_t_probe": PROBE}
        )
        tail = DISPLACED + sharc_jump(RESUME)
        self.tail = pk_sharc.Linked(TAIL_BASE, tail, tail, {})
        self.dm: dict[int, int] = {}
        for k in range(8):
            self.dm[nw(TYPE_CACHE) + k] = 0
        for k in range(32 * REC // 4):
            self.dm[nw(RECORDS) + k] = 0
        for k in range(32 * 32):
            self.dm[nw(OUT) + k] = 0
        for k in range(8):
            self.dm[nw(LATCH) + k] = 0
        for k in range(256):
            self.dm[nw(FRAME) + k] = 0
        for k in range(16):
            self.dm[nw(PROBE) + k] = 0
        self.rows: list[tuple[int, int]] = []
        self.write_rows()

    def write_rows(self) -> None:
        for k in range(34):
            self.dm[nw(TABLE) + k] = 0
        for i, (t, pc) in enumerate(self.rows):
            self.dm[nw(TABLE) + 2 * i] = t
            self.dm[nw(TABLE) + 2 * i + 1] = pc

    def register(self, t: int, mode: int) -> None:
        self.rows.append((t, self.routines.symbols["_t_mode1" if mode else "_t_mode0"]))
        self.write_rows()

    def set_type(self, track: int, t: int) -> None:
        word = nw(TYPE_CACHE) + track // 2
        sh = 16 * (track % 2)
        self.dm[word] = (self.dm[word] & ~(0xFFFF << sh) & 0xFFFFFFFF) | (t << sh)

    def hook(self, v: int, max_steps: int = 5000) -> dict:
        rec = record(v)
        frame = {-4: 32, -6: rec, -7: output(v), -5: 0xDEAD}
        for off, val in frame.items():
            self.dm[nw(STACK) + off] = val
        regs = {
            "R0": 64,
            "R4": rec,
            "I4": rec,
            "R5": 0x7005,
            "I6": STACK,
            "I7": STACK - 104,  # after the renderer's MODIFY(I7, -26) (NW)
            "M5": 0,
            "M6": 1,
            "M7": -1,
            "M13": 0,
            "M14": 1,
            "M15": -1,
            "I8": 0x20000080,
            "M8": 5,
        }
        out = pk_sharc.run_call(
            self.core,
            "_di_entry",
            regs=regs,
            dm_words=dict(self.dm),
            reads={
                "work": (nw(rec + 4), 64),
                "out": (nw(output(v)), 32),
                "probe": (nw(PROBE), 6),
                "frame": (nw(STACK) - 8, 8),  # DM(-8..-1, I6)
            },
            return_sw=RETURN_SW,
            others=(self.routines, self.tail),
            read_regs=("I4", "I6", "I7", "I8", "M5", "M6", "M7", "M8", "M13"),
            max_steps=max_steps,
            extra_breaks=(RESUME, DECIMATE, AFTER),
        )
        out["in"] = regs
        return out

    def helper(self, name: str, r0: int) -> int:
        out = pk_sharc.run_call(
            self.core,
            name,
            regs={"R0": r0, "M5": 0, "M6": 1, "M7": -1, "M13": 0, "M14": 1, "M15": -1},
            dm_words=dict(self.dm),
            return_sw=RETURN_SW,
            max_steps=200,
        )
        assert stops_at(out, RETURN_SW), out["halt"]
        regs = out["regs"]
        assert isinstance(regs, dict)
        return int(regs["R0"])


def stops_at(out: dict, sw: int) -> bool:
    return out["halt"]["reason"] == "breakpoint" and out["final_pc_sw"] == sw


def frame_slot(out: dict, n: int) -> int:
    """DM(-n, I6) as the run left it."""
    return int(out["frame"][8 - n])


def test_every_word_redecodes():
    rig = Rig()
    assert rig.core.checked > 70


def test_unregistered_type_runs_displaced_and_returns_to_0x1c4f18():
    rig = Rig()
    rig.register(MODE0, 0)
    rig.set_type(3, MODE0 + 1)  # an unregistered type on the voice's track
    rig.set_type(
        2, MODE0
    )  # the registered type on the neighbour track in the same cache word
    v = 7
    rig.dm[nw(record(v))] = 0x00345678  # record word +0, the sample pointer
    out = rig.hook(v)
    assert stops_at(out, RESUME), out["halt"]
    regs = out["regs"]
    # the displaced pair ran: R4 = DM(I4, M5), R12 = LEFTZ R4 (10 leading zeros)
    assert regs["R4"] == 0x00345678
    assert regs["R12"] == 10
    # R0 and I4 as the stock code expects them at 0x1c4f18; the frame, I6, I7 and the constants untouched
    assert regs["R0"] == 64 and regs["I4"] == record(v)
    assert frame_slot(out, 5) == 64, "the delay-slot store of 0x1c4f1b, done early"
    assert frame_slot(out, 4) == 32 and frame_slot(out, 6) == record(v)
    assert frame_slot(out, 7) == output(v)
    assert (regs["I6"], regs["I7"]) == (STACK, STACK - 104)
    assert (regs["M5"], regs["M6"], regs["M7"], regs["M13"]) == (0, 1, 0xFFFFFFFF, 0)
    assert (regs["I8"], regs["M8"], regs["R5"]) == (0x20000080, 5, 0x7005)
    assert out["probe"] == [0] * 6, "no routine ran"


def test_an_empty_registry_and_a_type_zero_track_fall_through():
    rig = Rig()
    out = rig.hook(0)
    assert stops_at(out, RESUME), out["halt"]
    rig.register(MODE0, 0)
    out = rig.hook(31)
    assert stops_at(out, RESUME), out["halt"]


def test_registered_type_mode0_continues_at_decimate_with_work_buffer_written():
    rig = Rig()
    rig.register(MODE1 + 5, 1)  # another type first: the scan has to step over it
    rig.register(MODE0, 0)
    rig.set_type(4, MODE0)
    v = 9
    out = rig.hook(v)
    assert stops_at(out, DECIMATE), out["halt"]
    assert out["work"] == [bits(1.0)] * 64
    assert out["out"] == [0] * 32, "the decimator writes the output, not the routine"
    regs = out["regs"]
    # what 0x1c524a reads: the frame and I6, I7 as the stock render path leaves them
    assert (regs["I6"], regs["I7"]) == (STACK, STACK - 104)
    assert frame_slot(out, 4) == 32 and frame_slot(out, 6) == record(v)
    assert frame_slot(out, 7) == output(v) and frame_slot(out, 5) == 64
    assert (regs["M5"], regs["M6"], regs["M7"], regs["M13"]) == (0, 1, 0xFFFFFFFF, 0)


def test_registered_type_mode1_continues_after_decimate_with_output_written():
    rig = Rig()
    rig.register(MODE1, 1)
    rig.set_type(15, MODE1)
    v = 30
    out = rig.hook(v)
    assert stops_at(out, AFTER), out["halt"]
    assert out["out"] == [bits(2.0)] * 32
    assert out["work"] == [0] * 64
    regs = out["regs"]
    # at 0x1c5264 the decimator's CJUMP leaves I7 two words below the renderer's; its delay slot adds them back
    assert regs["I6"] == STACK
    assert regs["I7"] == STACK - 104 - 8
    assert frame_slot(out, 4) == 32 and frame_slot(out, 6) == record(v)
    assert frame_slot(out, 7) == output(v)
    assert (regs["M5"], regs["M6"], regs["M7"], regs["M13"]) == (0, 1, 0xFFFFFFFF, 0)


@pytest.mark.parametrize("v", [0, 1, 30, 31])
def test_voice_and_track_derived_from_record_for_v0_v1_v30_v31(v):
    rig = Rig()
    rig.register(MODE0, 0)
    rig.set_type(v >> 1, MODE0)
    rig.set_type((v >> 1) ^ 1, MODE0 + 3)
    out = rig.hook(v)
    assert stops_at(out, DECIMATE), out["halt"]
    # the routine's inputs: R0 = v, R1 = type, R2 = track, R4 = record, I1 = record + 4, I2 = the output
    assert out["probe"] == [v, MODE0, v >> 1, record(v), record(v) + 4, output(v)]
    # the neighbour track's voice has another type and falls through
    other = 2 * ((v >> 1) ^ 1) + (v & 1)
    assert stops_at(rig.hook(other), RESUME)


def test_trig_latch_byte_per_voice():
    rig = Rig()
    # Step 1 pinned the latch as 32 bytes, latch + v for voice v (docs/findings/06): only voice 5's byte set
    rig.dm[nw(LATCH) + 1] = 0x01 << 8
    got = [rig.helper("_di_trig", v) for v in range(32)]
    assert got[5] != 0
    assert [g for v, g in enumerate(got) if v != 5] == [0] * 31
    # every voice its own byte
    for w in range(8):
        rig.dm[nw(LATCH) + w] = sum((4 * w + k + 1) << (8 * k) for k in range(4))
    assert [rig.helper("_di_trig", v) for v in range(32)] == list(range(1, 33))


def test_frame_hw_reads_both_halves_of_a_word():
    rig = Rig()
    rig.dm[nw(FRAME) + 0x34 // 2] = 0xBEEF1234  # halfwords 0x34 (low) and 0x35 (high)
    assert rig.helper("_di_frame_hw", 0x34) == 0x1234
    assert rig.helper("_di_frame_hw", 0x35) == 0xBEEF
    assert rig.helper("_di_frame_hw", 0x36) == 0


STOCK = ROOT / "firmware" / "Digitakt_II_OS1.17.syx"
STOCK_DB = ROOT / "out" / "sharcdb" / "dt2-1.17.sqlite"
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


@pytest.fixture(scope="module")
def patched(tmp_path_factory) -> tuple[Path, Path]:
    """core/dsp-inline (with core/machines, which it requires) on stock 1.17, no machine module: no registry row."""
    import subprocess

    d = tmp_path_factory.mktemp("inline")
    out, plan = d / "inline.syx", d / "inline.plan.json"
    r = subprocess.run(
        [
            str(patchkit_binary()),
            "--root",
            str(ROOT),
            "apply",
            "--firmware",
            str(STOCK),
            "--modules",
            "core/dsp-inline,core/machines",
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
    return out, plan


@needs_image
def test_image_passes_encoding_gate(patched):
    import json

    from tools.patchkit import pk_encoding_gate as gate

    out, plan = patched
    assert gate.check_image(out, plan) == []
    hooks = [h for h in json.loads(plan.read_text())["hooks"] if h["cpu"] == "sharc"]
    assert len(hooks) == 1
    h = hooks[0]
    assert h["displaced"] == "run" and h["ret"] == RESUME and h["tail"]


# ---- the patched image's own DSP code, run from the renderer's entry 0x1c4ecf to the caller's return point
# (tests/support/dsp_inline_image.py)

from support.dsp_inline_image import (  # noqa: E402
    CALLEE_SAVED,
    CALLER_RET,
    IMG_OUT,
    IMG_PROBE,
    IMG_RECORDS,
    IMG_ROUTINES,
    IMG_STACK,
    IMG_TYPES,
    image_memory,
    render_voice,
)


@needs_image
def test_patched_image_without_rows_renders_an_idle_voice_as_stock(patched):
    stock, mem = image_memory(STOCK), image_memory(patched[0])
    for v, active in ((0, 0), (17, 0)):
        a = render_voice(stock, v, {}, active)
        b = render_voice(mem, v, {}, active)
        assert stops_at(b, CALLER_RET), b["halt"]
        assert (a["out"], a["regs"]) == (b["out"], b["regs"])
        assert b["out"] == [0] * 32


@needs_image
@pytest.mark.parametrize("v,mode", [(6, 0), (6, 1), (31, 1)])
def test_patched_image_runs_a_registered_routine_and_returns_to_the_caller(
    patched, v, mode
):
    import json

    routines = pk_sharc.link(
        pk_sharc.assemble(ROUTINES), IMG_ROUTINES, {"_t_probe": IMG_PROBE}
    )
    mem = image_memory(patched[0], routines)
    doc = json.loads(patched[1].read_text())
    rows = next(
        a for k, a in doc["symbols"].items() if k.endswith(":inline_voices.rows")
    )
    t, typ = v >> 1, 0x21
    pokes = {
        IMG_TYPES + 4 * (t >> 1): typ << (16 * (t & 1)),
        rows: typ,
        rows + 4: routines.symbols["_t_mode1" if mode else "_t_mode0"],
        rows + 8: 0,
    }
    got = render_voice(mem, v, pokes)
    assert stops_at(got, CALLER_RET), got["halt"]
    rec = IMG_RECORDS + v * REC
    assert got["probe"] == [v, typ, t, rec, rec + 4, IMG_OUT + v * 0x80]
    if mode:
        assert got["out"] == [bits(2.0)] * 32
    else:
        assert all(w != 0x7F7F7F7F for w in got["out"]), (
            "the stock decimator wrote the output"
        )
    # the renderer's epilogue restored the caller's registers and frame, as after a stock render
    assert {n: got["regs"][n] for n in CALLEE_SAVED} == {
        n: x & 0xFFFFFFFF for n, x in CALLEE_SAVED.items()
    }
    assert (got["regs"]["I6"], got["regs"]["I7"]) == (IMG_STACK + 0x10000, IMG_STACK)


# An active stock voice: a sample playing forward from position 10 at step 0.7 (record fields per docs/findings/06,
# "The voice record contract"), int16 samples at SAMPLE_BASE and a non-zero interpolation table (init fills
# 0x25d940 at run time; the static image holds zeros there), so the renderer's main path from 0x1c4f81 runs and
# produces audio. Stock and patched images get the same inputs.
SAMPLE_BASE, SAMPLE_LEN, COEFS = 0x310000, 4000, 0x25D940


def active_voice_pokes(v: int) -> dict[int, int]:
    import math

    rec = IMG_RECORDS + v * REC
    step = round(0.7 * 2**31)
    pokes = {
        rec: SAMPLE_BASE,
        rec + 0x17C: 0,  # fade-in, zero-cross mute, reseed: off
        rec + 0x180: 0,  # previous sample
        rec + 0x188: SAMPLE_LEN,
        rec + 0x18C: 0,
        rec + 0x190: 0,  # loop start (Q31 int64, low word first)
        rec + 0x194: 0,
        rec + 0x198: 0,  # start
        rec + 0x19C: 0,
        rec + 0x1A0: (SAMPLE_LEN << 31) & 0xFFFFFFFF,  # end
        rec + 0x1A4: SAMPLE_LEN >> 1,
        rec + 0x1A8: step & 0xFFFFFFFF,  # step
        rec + 0x1AC: step >> 32,
        rec + 0x1B0: (10 << 31) & 0xFFFFFFFF,  # phase
        rec + 0x1B4: 10 >> 1,
        rec + 0x1B8: 0,  # ACTIVE (set to 1 by render_voice), +0x1b9..+0x1bb, reverse 0
        rec + 0x1BC: 0,  # loop 0
    }
    for k in range(SAMPLE_LEN // 2):
        lo, hi = (
            round(12000 * math.sin(2 * math.pi * (2 * k + j) / 37.0)) & 0xFFFF
            for j in (0, 1)
        )
        pokes[SAMPLE_BASE + 4 * k] = lo | (hi << 16)
    for row in range(256):
        for tap in range(6):
            pokes[COEFS + 24 * row + 4 * tap] = (
                0x08000000 * (tap + 1) + row * 0x1000
            ) & 0x7FFFFFFF
    return pokes


@needs_image
@pytest.mark.parametrize("v", [4, 29])
def test_patched_image_renders_an_active_voice_as_stock_with_a_non_matching_row(
    patched, v
):
    import json

    routines = pk_sharc.link(
        pk_sharc.assemble(ROUTINES), IMG_ROUTINES, {"_t_probe": IMG_PROBE}
    )
    doc = json.loads(patched[1].read_text())
    rows = next(
        a for k, a in doc["symbols"].items() if k.endswith(":inline_voices.rows")
    )
    t = v >> 1
    pokes = active_voice_pokes(v)
    stock = render_voice(image_memory(STOCK), v, pokes)
    assert stops_at(stock, CALLER_RET), stock["halt"]
    # the main sample path ran: audio in the output, the playback phase moved on, the voice still ACTIVE
    assert any(w not in (0, 0x7F7F7F7F) for w in stock["out"])
    lo, hi = stock["record"][0x1B0 // 4], stock["record"][0x1B0 // 4 + 1]
    assert ((hi << 32) | lo) == 10 * 2**31 + 64 * round(0.7 * 2**31), "64 steps of 0.7"
    assert stock["record"][0x1B8 // 4] & 0xFF == 1
    # the patched image: a registry row for another type (0x21; the track's type stays 0, SAMPLE)
    pokes.update(
        {
            IMG_TYPES + 4 * (t >> 1): 0,
            rows: 0x21,
            rows + 4: routines.symbols["_t_mode0"],
            rows + 8: 0,
        }
    )
    got = render_voice(image_memory(patched[0], routines), v, pokes)
    assert stops_at(got, CALLER_RET), got["halt"]
    assert got["probe"] == [0] * 6, "the registered routine did not run"
    assert got["out"] == stock["out"]
    assert got["record"] == stock["record"], [
        hex(4 * k)
        for k, (a, b) in enumerate(zip(stock["record"], got["record"], strict=True))
        if a != b
    ]
    assert got["regs"] == stock["regs"]
