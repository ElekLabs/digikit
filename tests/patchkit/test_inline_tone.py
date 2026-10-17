"""test/inline-tone: the sine routine on core/dsp-inline (concrete SHARC+ core) and the two H1 hardware images.

The routine is assembled the way patch:build assembles the objects of test/inline-tone-96k (TONE_MODE 0) and
test/inline-tone-48k (TONE_MODE 1): the files of the object's `sources`, joined into one translation unit. It is run
through the hook itself: core/dsp-inline is linked at a test base with its firmware addresses stood in by L2 test
pages (as in test_dsp_inline.py), the routine is registered for the ITone type, and the run starts at `_di_entry` with
the renderer's registers and frame. The hook's two continuations (SW 0x1c524a, 0x1c5264) are breakpoints.

The owner-image test patches stock 1.17 with each variant and runs the four checks an image must pass before it goes
to the owner: the container checksums (../elektron-firmware-tool), the SHARC+ boot stream (sharc_bootcheck against
stock), the encoding gate, and sections 2, 4, 5 and 8 byte-identical to stock. Skips without Selache, the stock image,
the 1.17 sharcdb or the firmware tool."""

from __future__ import annotations

import math
import struct
import subprocess
import tomllib
from pathlib import Path

import pytest

from tools.patchkit import pk_sharc, pk_toolchains

ROOT = pk_toolchains.ROOT
MODULES = ROOT / "modules"
HOOK_SRC = MODULES / "core" / "dsp-inline" / "src" / "dsp" / "dsp-inline.s"
TYPES = MODULES / "core" / "machines" / "types.toml"

pytestmark = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)

CORE_BASE, TONE_BASE, TAIL_BASE = 0xB90000, 0xB92000, 0xB94000
RESUME, DECIMATE, AFTER = 0x1C4F18, 0x1C524A, 0x1C5264
RETURN_SW = 0xB9F100
DISPLACED = bytes.fromhex("5499a801408c")
TYPE_CACHE, FRAME, RECORDS, LATCH, OUT, TABLE, PHASE, STACK = (
    0x20091000,
    0x20092000,
    0x20093000,
    0x20097000,
    0x20098000,
    0x2009A000,
    0x2009B000,
    0x2009C000,
)
REC = 0x1D8
VARIANT = {0: "test/inline-tone-96k", 1: "test/inline-tone-48k"}
FS = {0: 96000, 1: 48000}
NS = {0: 64, 1: 32}
TUNE_REST = 0x4000


def itone_type() -> int:
    return int(
        tomllib.loads(TYPES.read_text())["dt2"]["alloc"]["test/inline-tone:itone"]
    )


def nw(byte: int) -> int:
    return byte // 4


def f32(w: int) -> float:
    return float(struct.unpack("<f", struct.pack("<I", w & 0xFFFFFFFF))[0])


def sharc_jump(target: int) -> bytes:
    return struct.pack("<HHH", 0x063E, (target >> 16) & 0xFF, target & 0xFFFF)


def variant_source(mode: int) -> str:
    """The object's sources joined as tools/patchkit/pk_build.py joins them."""
    moddir = MODULES / VARIANT[mode]
    manifest = tomllib.loads((moddir / "module.toml").read_text())
    (obj,) = manifest["object"]
    return "\n".join((moddir / s).read_text() for s in obj["sources"])


def tune_hw(t: int) -> int:
    """Frame halfword of track t's machine slot 0 (TUNE): byte 0xda + 0x60 t."""
    return (0xDA + 0x60 * t) // 2


class Rig:
    def __init__(self, mode: int) -> None:
        self.mode = mode
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
        self.tone = pk_sharc.link(
            pk_sharc.assemble(variant_source(mode)),
            TONE_BASE,
            {
                "_di_frame_hw": self.core.symbols["_di_frame_hw"],
                "_di_trig": self.core.symbols["_di_trig"],
                "_it_phase": PHASE,
            },
        )
        tail = DISPLACED + sharc_jump(RESUME)
        self.tail = pk_sharc.Linked(TAIL_BASE, tail, tail, {})
        self.dm: dict[int, int] = {}
        for base, n in (
            (TYPE_CACHE, 8),
            (FRAME, 256 * 2),
            (RECORDS, 32 * REC // 4),
            (LATCH, 8),
            (OUT, 32 * 32),
            (TABLE, 34),
            (PHASE, 32),
        ):
            for k in range(n):
                self.dm[nw(base) + k] = 0
        self.type = itone_type()
        self.dm[nw(TABLE)] = self.type
        self.dm[nw(TABLE) + 1] = self.tone.symbols["_it_render"]

    def set_type(self, track: int, t: int) -> None:
        word = nw(TYPE_CACHE) + track // 2
        sh = 16 * (track % 2)
        self.dm[word] = (self.dm[word] & ~(0xFFFF << sh) & 0xFFFFFFFF) | (t << sh)

    def set_hw(self, k: int, value: int) -> None:
        word = nw(FRAME) + k // 2
        sh = 16 * (k % 2)
        self.dm[word] = (self.dm[word] & ~(0xFFFF << sh) & 0xFFFFFFFF) | (value << sh)

    def note(self, track: int, note: int, tune: int = TUNE_REST) -> None:
        self.set_type(track, self.type)
        self.set_hw(1 + track, note << 8)
        self.set_hw(tune_hw(track), tune)

    def trig(self, v: int, on: bool = True) -> None:
        word = nw(LATCH) + v // 4
        sh = 8 * (v % 4)
        self.dm[word] = (self.dm[word] & ~(0xFF << sh) & 0xFFFFFFFF) | (
            (1 if on else 0) << sh
        )

    def block(self, v: int) -> dict:
        """One render block of voice v through the hook; the phase word carries over to the next call."""
        rec, outp = RECORDS + v * REC, OUT + v * 0x80
        for off, val in {-4: 32, -6: rec, -7: outp, -5: 0xDEAD}.items():
            self.dm[nw(STACK) + off] = val
        regs = {
            "R0": 64,
            "R4": rec,
            "I4": rec,
            "I6": STACK,
            "I7": STACK - 104,
            "M5": 0,
            "M6": 1,
            "M7": -1,
            "M13": 0,
            "M14": 1,
            "M15": -1,
        }
        out: dict = pk_sharc.run_call(
            self.core,
            "_di_entry",
            regs=regs,
            dm_words=dict(self.dm),
            reads={
                "work": (nw(rec + 4), 64),
                "out": (nw(outp), 32),
                "phase": (nw(PHASE), 32),
            },
            return_sw=RETURN_SW,
            others=(self.tone, self.tail),
            read_regs=("I6", "I7", "M5", "M6", "M7", "M13", "M14", "M15"),
            max_steps=20000,
            extra_breaks=(RESUME, DECIMATE, AFTER),
        )
        want = AFTER if self.mode else DECIMATE
        assert out["halt"]["reason"] == "breakpoint", out["halt"]
        assert out["final_pc_sw"] == want, hex(out["final_pc_sw"])
        for k, w in enumerate(out["phase"]):
            self.dm[nw(PHASE) + k] = w
        out["samples"] = [f32(w) for w in (out["out"] if self.mode else out["work"])]
        return out


def increment(before: int, after: int, mode: int) -> int:
    d = (after - before) & 0xFFFFFFFF
    assert d % NS[mode] == 0
    return d // NS[mode]


def cents(f: float, ref: float) -> float:
    return 1200 * math.log2(f / ref)


def note_hz(n: float) -> float:
    return 440 * 2 ** ((n - 69) / 12)


def assert_sine(samples: list[float], p0: int, inc: int) -> None:
    for k, y in enumerate(samples):
        want = 0.125 * math.sin(2 * math.pi * ((p0 + k * inc) & 0xFFFFFFFF) / 2**32)
        assert abs(y - want) < 2e-6, (k, y, want)


@pytest.mark.parametrize("mode", [0, 1])
def test_tone_pitch_note60_tune64_is_261_626_hz(mode):
    rig = Rig(mode)
    v = 6  # track 3
    rig.note(3, 60)
    rig.trig(v)
    a = rig.block(v)
    inc = increment(0, a["phase"][v], mode)
    f = inc * FS[mode] / 2**32
    assert abs(cents(f, 261.6256)) < 1, f
    assert abs(cents(f, note_hz(60))) < 0.01, f
    assert_sine(a["samples"], 0, inc)
    # the next block continues the phase (no trigger), and only voice v's phase word moved
    rig.trig(v, False)
    b = rig.block(v)
    assert_sine(b["samples"], a["phase"][v], inc)
    assert increment(a["phase"][v], b["phase"][v], mode) == inc
    assert [w for k, w in enumerate(b["phase"]) if k != v] == [0] * 31
    # the ABI registers the hook relies on are as they came in
    regs = b["regs"]
    assert (regs["I6"], regs["I7"]) == (STACK, STACK - 104 - (8 if mode else 0))
    assert (regs["M5"], regs["M6"], regs["M7"]) == (0, 1, 0xFFFFFFFF)
    assert (regs["M13"], regs["M14"], regs["M15"]) == (0, 1, 0xFFFFFFFF)
    # the other buffer is untouched: mode 0 leaves the output to the decimator, mode 1 skips the work buffer
    assert (b["work"] if mode else b["out"]) == [0] * (64 if mode else 32)


@pytest.mark.parametrize("mode", [0, 1])
def test_tone_pitch_follows_tune_word(mode):
    rig = Rig(mode)
    v = 1  # track 0
    rig.note(0, 60, tune=0x4100)
    rig.trig(v)
    inc = increment(0, rig.block(v)["phase"][v], mode)
    f = inc * FS[mode] / 2**32
    assert abs(cents(f, note_hz(61))) < 1, f
    # and down: tune 0x3e00 is two semitones below note 60; note 72 at rest is an octave up
    rig.note(0, 60, tune=0x3E00)
    rig.dm[nw(PHASE) + v] = 0
    f = increment(0, rig.block(v)["phase"][v], mode) * FS[mode] / 2**32
    assert abs(cents(f, note_hz(58))) < 1, f
    rig.note(0, 72)
    rig.dm[nw(PHASE) + v] = 0
    f = increment(0, rig.block(v)["phase"][v], mode) * FS[mode] / 2**32
    assert abs(cents(f, note_hz(72))) < 1, f


@pytest.mark.parametrize("mode", [0, 1])
def test_tone_restarts_on_trigger_byte(mode):
    rig = Rig(mode)
    v = 31  # track 15
    rig.note(15, 69)
    rig.dm[nw(PHASE) + v] = 0x40000000  # a quarter period in: sin = 1
    a = rig.block(v)  # no trigger: the phase carries on
    assert a["samples"][0] == pytest.approx(0.125, abs=2e-6)
    inc = increment(0x40000000, a["phase"][v], mode)
    assert abs(cents(inc * FS[mode] / 2**32, 440.0)) < 1
    rig.trig(v)  # note-on: the block starts at phase 0
    b = rig.block(v)
    assert b["samples"][0] == 0.0
    assert b["phase"][v] == NS[mode] * inc
    assert_sine(b["samples"], 0, inc)
    # the trigger byte of another voice (the track's other voice, 30) does not restart voice 31
    rig.trig(v, False)
    rig.trig(30)
    c = rig.block(v)
    assert c["samples"][0] != 0.0
    assert c["phase"][v] == 2 * NS[mode] * inc


# ---- the H1 images: stock 1.17 + the variant, and the four owner-image checks

STOCK = ROOT / "firmware" / "Digitakt_II_OS1.17.syx"
STOCK_DB = ROOT / "out" / "sharcdb" / "dt2-1.17.sqlite"
FWTOOL = ROOT.parent / "elektron-firmware-tool" / "elektron-firmware-tool"
UNCHANGED = (2, 4, 5, 8)


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


needs_image = pytest.mark.skipif(
    not STOCK.exists() or not STOCK_DB.exists(),
    reason="needs firmware/Digitakt_II_OS1.17.syx and out/sharcdb/dt2-1.17.sqlite",
)
H1 = {1: "h1b-tone-48k", 0: "h1a-tone-96k"}


@pytest.fixture(scope="module")
def h1_images(tmp_path_factory) -> dict[int, tuple[Path, Path]]:
    """Stock 1.17 patched with each routine module (its requires pull in the machine and core/dsp-inline)."""
    d = tmp_path_factory.mktemp("h1")
    images = {}
    for mode, name in H1.items():
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
def test_h1_images_pass_all_owner_image_checks(h1_images, tmp_path):
    import json

    from tools.patchkit import pk_encoding_gate as gate
    from tools.sharc_dev import sharc_bootcheck as boot

    stock_sections = sections(FWTOOL, STOCK, tmp_path / "stock")
    for mode, (out, plan) in h1_images.items():
        name = H1[mode]
        doc = json.loads(plan.read_text())
        mods = {str(m).split("@")[0] for m in doc["modules"]}
        assert {VARIANT[mode], "test/inline-tone", "core/dsp-inline"} <= mods, mods
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
        # 3. the encoding gate, over code that is really there
        assert gate.code_ranges(doc), "no DSP code ranges in the plan"
        assert gate.check_image(out, plan) == [], name
        # 4. bootloader, updater, metadata and section 8 byte-identical to stock
        got = sections(FWTOOL, out, tmp_path / name)
        for sid in UNCHANGED:
            assert got[sid] == stock_sections[sid], f"{name}: section {sid} changed"
        assert got[7] != stock_sections[7], f"{name}: the DSP blob is unchanged"


# The H1 image's own DSP code (tests/support/dsp_inline_image.py): the stock renderer called for an ITONE voice runs
# the hook and the routine as linked into the image and returns to the voice loop.
IMG_FRAME, IMG_LATCH = 0x2558DC, 0x24F0CC


@needs_image
@pytest.mark.parametrize("mode", [1, 0])
def test_h1_image_renders_the_sine_through_the_stock_renderer(h1_images, mode):
    import json

    from support.dsp_inline_image import (
        CALLEE_SAVED,
        CALLER_RET,
        IMG_RECORDS,
        IMG_STACK,
        IMG_TYPES,
        image_memory,
        render_voice,
    )

    out, plan = h1_images[mode]
    doc = json.loads(plan.read_text())
    phase = next(a for k, a in doc["symbols"].items() if k.endswith(":_it_phase"))
    v, t = 21, 10
    hw = {1 + t: 60 << 8, tune_hw(t): TUNE_REST}
    pokes: dict[int, int] = {IMG_TYPES + 4 * (t >> 1): itone_type() << (16 * (t & 1))}
    for k, value in hw.items():
        a = IMG_FRAME + 4 * (k // 2)
        pokes[a] = pokes.get(a, 0) | (value << (16 * (k % 2)))
    pokes[IMG_LATCH + (v & ~3)] = 1 << (8 * (v & 3))
    pokes[phase + 4 * v] = 0x12345678  # restarted by the trigger
    rec = IMG_RECORDS + v * REC
    got = render_voice(
        image_memory(out),
        v,
        pokes,
        reads={"work": (rec + 4, 64), "phase": (phase, 32)},
    )
    assert got["halt"]["reason"] == "breakpoint", got["halt"]
    assert got["final_pc_sw"] == CALLER_RET
    inc = increment(0, got["phase"][v], mode)
    assert abs(cents(inc * FS[mode] / 2**32, note_hz(60))) < 0.01
    if mode:
        assert_sine([f32(w) for w in got["out"]], 0, inc)
    else:
        assert_sine([f32(w) for w in got["work"]], 0, inc)
        assert all(w != 0x7F7F7F7F for w in got["out"]), (
            "the decimator wrote the output"
        )
    # the renderer's epilogue restored the caller's registers and frame
    assert {n: got["regs"][n] for n in CALLEE_SAVED} == {
        n: x & 0xFFFFFFFF for n, x in CALLEE_SAVED.items()
    }
    assert (got["regs"]["I6"], got["regs"]["I7"]) == (IMG_STACK + 0x10000, IMG_STACK)
