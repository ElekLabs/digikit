"""ui/boot-splash: the image generator, the ColdFire draw function and the module's writes.

- tools/patchkit/pk_splash.py turns a 128x64 1-bit PNG into the 1024-byte table of the firmware's Bitmap layout
  (setPixel 0x401124fc on 1.17): column-major, byte 8x + k holds Bitmap rows 8k .. 8k+7 of column x, MSB = the lower
  Bitmap row, a set bit = a lit pixel. On the glass Bitmap row r is screen row 63 - r: Bitmap row 0 is the BOTTOM
  (the stock full-screen intro image reads upright only that way, and so does the UI framebuffer under the emulator's
  panel model; docs/findings/03-ui-and-panel.md, "The boot intro on DT2 1.17"). A known pattern pins it: the screen's top-left pixel is
  byte 7's LSB, its bottom-left pixel byte 0's MSB.
- `splash_draw(int frame, int nframes, Bitmap *bmp)` runs on the emulated ColdFire core (tools/patchkit/pk_cfcall.py)
  with the stock draw functions' convention (cdecl, bmp at 12(sp), d0/d1/a0/a1 scratch, d2-d7/a2-a6 preserved): it
  copies exactly the 1024 bytes into bmp->bits of a stock 128x64 Bitmap, writes nothing outside them, and writes
  nothing at all when the Bitmap is not one.
- The committed table is the generator's output for the committed art with the polarity recorded in its header;
  the default is the inverted polarity (the owner's decision, 2026-10-18: dark screen, the figure lit).

The ColdFire tests skip without the m68k toolchain or the stock image."""

from __future__ import annotations

import re
import struct
import tomllib
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

from tools.firmware import config
from tools.patchkit import pk_cfcall, pk_link, pk_splash, pk_toolchains

ROOT: Path = REPO_ROOT
MOD = ROOT / "modules" / "ui" / "boot-splash"
ART = MOD / "art" / "abigor-128x64.png"
TABLE = MOD / "src" / "cf" / "splash_bits.s"
DRAW = MOD / "src" / "cf" / "splash.s"
SYX = config.stock("Digitakt_II_OS1.17.syx")
BASE = 0x47E8B000
BMP = 0x47EC0000  # the Bitmap object (0x1c bytes)
BITS = 0x47EC1000  # its storage: 1024 bytes, with guard bytes on both sides
GUARD = 256
BITMAP_VTABLE = 0x4021CFF8  # 1.17's Bitmap vtable (the ctor 0x401124b6 stores it at +0)

needs_cf = pytest.mark.skipif(
    pk_toolchains.m68k_bin("gcc") is None or not SYX.exists(),
    reason="m68k toolchain or firmware/Digitakt_II_OS1.17.syx missing",
)


def blank() -> list[list[bool]]:
    return [[False] * pk_splash.W for _ in range(pk_splash.H)]


# ---- the generator ---------------------------------------------------------------------------------------------------


def test_top_left_pixel_is_byte_7_lsb() -> None:
    px = blank()
    px[0][0] = True  # the screen's top-left
    data = pk_splash.encode(px)
    assert len(data) == 1024
    assert data[7] == 0x01 and sum(data) == 1


def test_known_pattern_pins_the_layout() -> None:
    cases = {  # screen (x, y), y = 0 the top row -> (byte, bit)
        (0, 63): (0, 0x80),  # bottom-left: byte 0's MSB (Bitmap row 0)
        (0, 56): (0, 0x01),  # Bitmap row 7: byte 0's LSB
        (0, 55): (1, 0x80),  # Bitmap row 8: byte 1's MSB
        (0, 1): (7, 0x02),
        (1, 63): (8, 0x80),  # column 1 starts at byte 8
        (127, 0): (1023, 0x01),  # top-right: the last byte's LSB
        (5, 30): (8 * 5 + 4, 0x40),  # Bitmap row 33
    }
    for (x, y), (i, bit) in cases.items():
        px = blank()
        px[y][x] = True
        data = pk_splash.encode(px)
        assert data[i] == bit, (x, y)
        assert sum(bin(b).count("1") for b in data) == 1, (x, y)
    # the same as the firmware's setPixel(x, r): word storage[x*2 + (r >> 5)] (big-endian), bit 0x80000000 >> (r & 31),
    # with Bitmap row r = 63 - y
    px = blank()
    px[45][77] = True
    words = struct.unpack(">256I", pk_splash.encode(px))
    r = 63 - 45
    assert words[77 * 2 + (r >> 5)] == 0x80000000 >> (r & 31)


def test_png_round_trip(tmp_path: Path) -> None:
    import random

    rng = random.Random(1234)
    px = [[rng.random() < 0.5 for _ in range(128)] for _ in range(64)]
    px[0][0], px[0][1], px[1][0] = True, False, False
    png = tmp_path / "t.png"
    pk_splash.save_png(px, png)
    data = pk_splash.encode(pk_splash.load_png(png, invert=False))
    assert pk_splash.decode(data) == px
    inv = pk_splash.encode(pk_splash.load_png(png, invert=True))
    assert bytes(b ^ 0xFF for b in inv) == data
    assert data[0] & 0x80 and not inv[0] & 0x80
    # bytes -> PNG -> bytes
    back = tmp_path / "back.png"
    pk_splash.save_png(pk_splash.decode(data), back)
    assert pk_splash.encode(pk_splash.load_png(back, invert=False)) == data


def test_wrong_size_is_refused(tmp_path: Path) -> None:
    from PIL import Image

    p = tmp_path / "big.png"
    Image.new("1", (128, 65)).save(p)
    with pytest.raises(pk_splash.SplashError):
        pk_splash.load_png(p, invert=False)
    with pytest.raises(pk_splash.SplashError):
        pk_splash.decode(b"\0" * 1023)


def test_asm_output_parses_back() -> None:
    px = blank()
    px[3][9] = True
    data = pk_splash.encode(px)
    text = pk_splash.to_asm(data, "x.png", "inverted")
    assert pk_splash.parse_asm(text) == (data, "x.png", "inverted")
    assert ".globl splash_bits" in text


# ---- the committed module --------------------------------------------------------------------------------------------


def test_committed_table_is_the_generator_output_for_the_committed_art() -> None:
    data, source, polarity = pk_splash.parse_asm(TABLE.read_text())
    assert source == "art/abigor-128x64.png"
    assert (MOD / source).resolve() == ART.resolve()
    want = pk_splash.encode(pk_splash.load_png(ART, invert=polarity == "inverted"))
    assert data == want, "stale: rerun the generator (modules/ui/boot-splash/README.md)"


def test_default_polarity_is_inverted() -> None:
    # owner decision 2026-10-18: dark screen, Abigor in lit pixels. The art is black on white, so inverted lights the
    # figure: far fewer lit pixels than dark ones.
    data, _source, polarity = pk_splash.parse_asm(TABLE.read_text())
    assert polarity == "inverted"
    lit = sum(bin(b).count("1") for b in data)
    assert lit < 128 * 64 // 2


def test_manifest_points_all_five_scenes_at_splash_draw_for_45_frames() -> None:
    m = tomllib.loads((MOD / "module.toml").read_text())
    assert m["module"]["ports"] == ["dt2-1.17"]
    ops = [o for o in m["op"] if o["anchor"] == "cf.intro.scenes"]
    assert len(ops) == len(m["op"]) == 5
    assert sorted(o["offset"] for o in ops) == [0x00, 0x10, 0x20, 0x30, 0x40]
    for o in ops:
        assert o["op"] == "write"
        assert o["hex"] == "00000000" + "%08x" % 45
        assert o["reloc"] == [{"offset": 0, "type": "cf.32", "symbol": "splash_draw"}]
    rule = next(r for r in m["anchor_rule"] if r["name"] == "cf.intro.scenes")
    assert rule["kind"] == "cf_words" and len(rule["words"]) == 20
    assert [c["resource"] for c in m["claim"]] == ["cf.intro.scenes"]
    obj = m["object"][0]
    assert obj["place"] == "cf.expansion"
    assert not [o for o in m["op"] if o["op"] in ("hook", "retarget")]


def test_the_draw_source_makes_no_calls() -> None:
    code = "\n".join(line.split("|", 1)[0] for line in DRAW.read_text().splitlines())
    assert not re.search(r"\b(jsr|bsr|jmp|trap)\b", code)


# ---- splash_draw on the emulated ColdFire core -----------------------------------------------------------------------


def bitmap(w: int, h: int, stride: int, bits: int) -> bytes:
    return struct.pack(">IIIIII", BITMAP_VTABLE, w, h, stride, bits, 0) + b"\0\0\0\0"


@needs_cf
def test_splash_draw_copies_exactly_1024_bytes_with_the_stock_convention() -> None:
    _obj, linked, _hist = pk_link.build([DRAW, TABLE], BASE, {})
    assert linked.bss_size == 0
    draw = linked.symbols["splash_draw"]
    table = linked.image[
        linked.symbols["splash_bits"] - BASE : linked.symbols["splash_bits"]
        - BASE
        + 1024
    ]
    assert table == pk_splash.parse_asm(TABLE.read_text())[0]
    fill = b"\xa5" * (GUARD + 1024 + GUARD)
    region = (BITS - GUARD, 2 * GUARD + 1024)
    good = bitmap(128, 64, 2, BITS)
    bad = [
        bitmap(127, 64, 2, BITS),
        bitmap(128, 32, 1, BITS),
        bitmap(128, 64, 3, BITS),
        bitmap(256, 64, 2, BITS),
        bitmap(128, 64, 2, 0),
    ]
    calls: list[dict[str, object]] = []
    # the stock caller (0x400d1992) passes frame, nframes, &bitmap; frames 0, 1, 44 of 45 and odd values
    for frame, n in [(0, 45), (1, 45), (44, 45), (0x7FFFFFFF, -1)]:
        calls.append(
            {"pc": draw, "args": [frame, n, BMP], "reads": [region, (BMP, 28)]}
        )
    for _b in bad:
        calls.append(
            {
                "pc": draw,
                "args": [0, 45, BMP + 0x100],
                "reads": [region, (BMP + 0x100, 28)],
            }
        )
    # one machine run per Bitmap state: fresh guard bytes for each good call, the bad Bitmaps beside it
    pokes = {BASE: linked.image, BMP: good, BITS - GUARD: fill}
    m = pk_cfcall.CfMachine(SYX)
    for c in calls[:4]:
        r = m.run(pokes, [c])[0]
        assert r.ok, r.error
        assert not r.clobbered, (
            r.clobbered
        )  # d2-d7/a2-a6 and the stack pointer as on entry
        got = r.reads[0]
        assert got[:GUARD] == b"\xa5" * GUARD
        assert got[GUARD + 1024 :] == b"\xa5" * GUARD
        assert got[GUARD : GUARD + 1024] == table
        assert r.reads[1] == good  # the Bitmap object itself is not written
        assert r.insns < 2000
    for b, c in zip(bad, calls[4:], strict=True):
        r = m.run({**pokes, BMP + 0x100: b}, [c])[0]
        assert r.ok, r.error
        assert not r.clobbered, r.clobbered
        assert r.reads[0] == fill, b.hex()  # nothing written
        assert r.reads[1] == b
