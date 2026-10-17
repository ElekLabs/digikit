"""machines/plaits (VA model) on core/dsp-inline: the compiled C on the concrete SHARC+ core and the two H2 images.

The rig (tests/support/plaits_rig.py) links core/dsp-inline at test addresses and the PLAITS object of
machines/plaits-48k (mode 1, 48 kHz into the voice output) or machines/plaits-96k (mode 0, 96 kHz into the work
buffer), built as pk_build builds it (the entry shim plus the C voice wrapper, VA engine and half-band compiled by
selcc), and runs `_di_entry` block after block on the same memory. The reference is the same C built for the host
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
    port: Port, mode: int, setting: dict[str, Any], blocks: int
) -> tuple[float, float, float, list[int]]:
    """Track 0's even voice, triggered at block 0, against the host wrapper: (max error, RMS error, signal RMS,
    instructions per block)."""
    rig = PlaitsRig(mode)
    hv = port.voice()
    tune = setting.get("tune", 0x4000)
    core, host = [], []
    for b in range(blocks):
        slots = {**setting["first"], **(setting["second"] if b >= blocks // 2 else {})}
        rig.controls(0, setting["note"], tune=tune, **slots)
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


@pytest.mark.slow
@pytest.mark.parametrize("mode", [1, 0])
def test_extreme_parameters_compiled_core(port: Port, mode: int) -> None:
    """All {0, 0x7f00}^3 of HARMONICS/TIMBRE/MORPH at notes 0 and 127 and TUNE 0, 0x4000, 0x7f00: finite output,
    within MAX_ERR of the host build, every block within the instruction bound."""
    rig = PlaitsRig(mode)
    rig.active(0)
    worst = 0
    for note in (0, 127):
        for tune in (0, 0x4000, 0x7F00):
            for h in (0, 0x7F00):
                for t in (0, 0x7F00):
                    for m in (0, 0x7F00):
                        slots = {"harmonics": h, "timbre": t, "morph": m}
                        rig.controls(0, note, tune=tune, **slots)
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
                            worst = max(worst, rig.instructions[-1])
                            assert (
                                rig.instructions[-1] <= MAX_BLOCK_INSTRUCTIONS[mode]
                            ), (where, rig.instructions[-1])
    print(f"mode {mode}: worst block {worst} instructions")
