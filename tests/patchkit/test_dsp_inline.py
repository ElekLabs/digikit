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

CALLER_RET = 0x1C6B06  # after `CJUMP 0x1c4ecf` in the voice loop of FUN_1c642a
IMG_RECORDS, IMG_OUT, IMG_TYPES = 0x2412CC, 0x252DF8, 0x255970
IMG_STACK = 0x2A0000
IMG_ROUTINES, IMG_PROBE = (
    0xBF0000,
    0x200E8000,
)  # free L2, above everything the image loads
CALLEE_SAVED: dict[str, int] = {
    **{f"R{n}": 0x7000 + n for n in (3, 5, 6, 7, 9, 10, 11, 13, 15)},
    **{f"I{n}": 0x20000010 * (n + 1) for n in (0, 1, 2, 3, 5)},
    **{"M1": 11, "M2": 12, "M3": 13, "B3": 0x200, "I8": 0x20000080, "M8": 8},
}


def image_memory(syx: Path, extra=None):
    from tools.patchkit import port_resolve
    from tools.sharc_dev.sharcldr import LoadedMemory

    mem = LoadedMemory.from_stream(port_resolve.sections_of(syx)[7])
    if extra is None:
        return mem
    blocks = [dict(b) for b in mem.blocks]
    blocks.append(
        {
            "target_address": pk_sharc.SW_ALIAS_BASE + 2 * extra.base_sw,
            "byte_count": len(extra.code_boot),
            "fill": False,
            "payload_offset": len(mem.data),
            "payload_len": len(extra.code_boot),
            "index": len(blocks),
        }
    )
    return LoadedMemory(mem.data + extra.code_boot, blocks)


def render_voice(mem, v: int, pokes: dict[int, int], active: int = 1) -> dict:
    """Call the renderer for voice v the way the voice loop does; return the output, the probe and the registers."""
    from tools.sharc_dev import sharc_run as sr
    from tools.sharc_dev import sharc_trace as st

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
        "I6": IMG_STACK,
        "I7": IMG_STACK - 8,
    }
    for name, value in CALLEE_SAVED.items():
        regs[name] = value
    runner = sr.Runner(mem, 0x1C4ECF, regs=regs, breakpoints=[CALLER_RET])
    runner.state.explicit_memory_model = True
    words = {
        IMG_STACK - 4: CALLER_RET - 1,  # the return address the CJUMP pushed
        IMG_STACK: IMG_STACK + 0x10000,  # the caller's frame pointer
        rec: 0x300000,  # word +0, the sample pointer
        **{outp + 4 * k: 0x7F7F7F7F for k in range(32)},
        **pokes,
    }
    for a, w in words.items():
        st._dm_write(runner.state, a, 4, st.Const(w & 0xFFFFFFFF))
    st._dm_write(runner.state, rec + 0x1B8, 1, st.Const(active))
    runner.state.call_stack.append(CALLER_RET)
    res = runner.run(20000)

    def word(a: int) -> int:
        x = st._dm_read(runner.state, a, 4)
        return 0 if x is None else x.value & 0xFFFFFFFF

    return {
        "halt": res.halt.to_json(),
        "final_pc_sw": res.final_pc_sw,
        "out": [word(outp + 4 * k) for k in range(32)],
        "probe": [word(IMG_PROBE + 4 * k) for k in range(6)],
        "regs": {
            n: getattr(runner.state.uregs.get(st.UREG_CODES[n]), "value", None)
            for n in (*CALLEE_SAVED, "I6", "I7")
        },
    }


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
