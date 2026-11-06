"""Host-only asset boundaries and pixel layout, using synthetic images/bytes."""

import io
import json
import random
import struct
import zlib

import pytest
from PIL import Image

from tools.bootscreen.asset import (
    Frame,
    decode,
    encode,
    load_manifest,
    native_to_oled,
    png_frame,
)
from tools.control.digi_client import oled_png


def binary(records, *, ticks=None, count=None):
    body = b"".join(
        struct.pack(">HHB3s", hold, len(payload), codec, reserved) + payload
        for hold, codec, reserved, payload in records
    )
    return (
        struct.pack(
            ">4sHHII",
            b"DKBS",
            1,
            len(records) if count is None else count,
            sum(r[0] for r in records) if ticks is None else ticks,
            zlib.crc32(body),
        )
        + body
    )


def test_asymmetric_native_corners_and_oled_preview(tmp_path):
    image = Image.new("RGB", (128, 64))
    points = [(0, 0), (1, 63), (127, 1), (125, 62), (17, 19)]
    for point in points:
        image.putpixel(point, (255, 255, 255))
    path = tmp_path / "corners.png"
    image.save(path)
    packed = png_frame(path)
    assert packed[0] == 0x80
    assert packed[15] == 1
    assert packed[1016] == 0x40
    assert packed[1007] == 2
    preview = Image.open(io.BytesIO(oled_png(native_to_oled(packed), scale=1)))
    assert {
        (x, y) for y in range(64) for x in range(128) if preview.getpixel((x, y))
    } == set(points)


def test_threshold_integer_luma_alpha_and_grayscale(tmp_path):
    image = Image.new("RGBA", (128, 64))
    values = [
        (255, 255, 255, 0),
        (255, 255, 255, 128),
        (255, 255, 255, 129),
        (255, 0, 0, 255),
        (0, 255, 0, 255),
        (0, 0, 255, 255),
    ]
    for x, value in enumerate(values):
        image.putpixel((x, 0), value)
    path = tmp_path / "alpha.png"
    image.save(path)
    packed = png_frame(path)
    assert [bool(packed[8 * x] & 128) for x in range(len(values))] == [
        False,
        False,
        True,
        False,
        True,
        False,
    ]
    gray = Image.new("L", (128, 64), 128)
    gray.save(path)
    assert png_frame(path) == bytes(1024)
    assert png_frame(path, 127) == bytes([255]) * 1024


def test_merge_holds_and_raw_fallback():
    noise = random.Random(19).randbytes(1024)
    result = decode(encode([Frame(2, noise), Frame(3, noise)]))
    assert result.frames == (Frame(5, noise),)
    assert result.codecs == (0,)
    assert result.stats()["raw_tick_bytes"] == 5120


def test_sparse_xor_sequence_roundtrip():
    frames = []
    for x in range(128):
        pixels = bytearray(1024)
        pixels[x * 8 + x % 8] = 1 << (x % 8)
        frames.append(Frame(1, bytes(pixels)))
    result = decode(encode(frames))
    assert result.frames == tuple(frames)
    assert set(result.codecs) == {1}
    assert result.file_bytes < 128 * 1024


def test_raw_and_delta_share_the_same_previous_frame():
    random_source = random.Random(41)
    first = random_source.randbytes(1024)
    second = bytearray(first)
    second[513] ^= 0x80
    frames = [
        Frame(1, first),
        Frame(2, bytes(second)),
        Frame(3, random_source.randbytes(1024)),
    ]
    result = decode(encode(frames))
    assert result.frames == tuple(frames)
    assert result.codecs == (0, 1, 0)


def test_payload_truncation_with_valid_crc():
    body = struct.pack(">HHB3s", 1, 1024, 0, bytes(3)) + b"short"
    packed = struct.pack(">4sHHII", b"DKBS", 1, 1, 1, zlib.crc32(body)) + body
    with pytest.raises(ValueError, match="truncated record payload"):
        decode(packed)


def test_literal_xor_payload_larger_than_raw_is_valid():
    pixels = bytes(range(256)) * 4
    payload = b"".join(
        b"\x7f" + pixels[start : start + 128] for start in range(0, 1024, 128)
    )
    result = decode(binary([(7, 1, bytes(3), payload)]))
    assert result.frames == (Frame(7, pixels),)
    assert result.codecs == (1,)


def test_crc_and_every_truncation():
    packed = encode([Frame(1, bytes(1024))])
    for cut in range(len(packed)):
        with pytest.raises(ValueError):
            decode(packed[:cut])
    damaged = bytearray(packed)
    damaged[-1] ^= 1
    with pytest.raises(ValueError, match="CRC"):
        decode(bytes(damaged))


@pytest.mark.parametrize(
    "record",
    [
        (0, 0, bytes(3), bytes(1024)),
        (301, 0, bytes(3), bytes(1024)),
        (1, 2, bytes(3), bytes(1024)),
        (1, 0, b"\x01\0\0", bytes(1024)),
        (1, 0, bytes(3), bytes(1023)),
        (1, 1, bytes(3), b""),
        (1, 1, bytes(3), bytes(1025)),
        (1, 1, bytes(3), b"\xff\0" * 9),  # expansion beyond 1024
        (1, 1, bytes(3), b"\xff\0" * 7),  # under-expansion
        (1, 1, bytes(3), b"\x7f\0"),  # truncated literal
        (1, 1, bytes(3), b"\xff"),  # truncated repeat
    ],
)
def test_malformed_record_bounds(record):
    with pytest.raises(ValueError):
        decode(binary([record]))


@pytest.mark.parametrize(
    "count,ticks", [(0, 1), (301, 1), (1, 0), (1, 301), (2, 1), (1, 2)]
)
def test_header_and_hold_totals(count, ticks):
    with pytest.raises(ValueError):
        decode(binary([(1, 0, bytes(3), bytes(1024))], count=count, ticks=ticks))


def test_trailing_reserved_magic_version_and_repeated_frames():
    packed = binary([(1, 0, bytes(3), bytes(1024))])
    body = packed[16:] + b"trailing"
    with pytest.raises(ValueError, match="trailing"):
        decode(packed[:12] + struct.pack(">I", zlib.crc32(body)) + body)
    for bad in [b"NOPE" + packed[4:], packed[:4] + b"\0\2" + packed[6:]]:
        with pytest.raises(ValueError, match="magic/version"):
            decode(bad)
    repeated = decode(binary([(1, 0, bytes(3), bytes(1024))] * 2))
    assert repeated.frames == (Frame(1, bytes(1024)), Frame(1, bytes(1024)))


def test_asset_cap_and_encoder_bounds():
    noise = random.Random(29)
    with pytest.raises(ValueError, match="maximum is 65536"):
        encode([Frame(1, noise.randbytes(1024)) for _ in range(64)])
    with pytest.raises(ValueError, match="maximum is 65536"):
        decode(bytes(65537))
    for frames in [
        [],
        [Frame(True, bytes(1024))],
        [Frame(301, bytes(1024))],
        [Frame(200, bytes(1024)), Frame(101, bytes(1024))],
        [Frame(1, b"short")],
    ]:
        with pytest.raises(ValueError):
            encode(frames)


def test_manifest_relative_path_and_default_threshold(tmp_path):
    images = tmp_path / "images"
    images.mkdir()
    Image.new("RGB", (128, 64), "white").save(images / "one.png")
    manifest = tmp_path / "source.json"
    manifest.write_text(
        json.dumps({"version": 1, "frames": [{"file": "images/one.png", "ticks": 7}]})
    )
    assert load_manifest(manifest) == (Frame(7, bytes([255]) * 1024),)


@pytest.mark.parametrize(
    "source",
    [
        [],
        {},
        {"version": True, "frames": []},
        {"version": 2, "frames": []},
        {"version": 1, "frames": [], "extra": 0},
        {"version": 1, "frames": []},
        {"version": 1, "frames": "bad"},
        {"version": 1, "frames": [{}]},
        {"version": 1, "frames": [{"file": "x", "ticks": True}]},
        {"version": 1, "frames": [{"file": "x", "ticks": 0}]},
        {"version": 1, "frames": [{"file": "x", "ticks": 1, "extra": 0}]},
        {"version": 1, "frames": [{"file": 1, "ticks": 1}]},
        {"version": 1, "frames": [{"file": "", "ticks": 1}]},
        {"version": 1, "frames": [{"file": "x", "ticks": 200}] * 2},
        {"version": 1, "frames": [{"file": "x", "ticks": 1}] * 301},
        *[
            {"version": 1, "threshold": value, "frames": [{"file": "x", "ticks": 1}]}
            for value in [True, -1, 256, 1.5]
        ],
    ],
)
def test_source_validation_before_image_loading(tmp_path, source):
    manifest = tmp_path / "source.json"
    manifest.write_text(json.dumps(source))
    with pytest.raises(ValueError):
        load_manifest(manifest)


def test_reject_non_png_wrong_size_and_animation(tmp_path):
    path = tmp_path / "image.png"
    Image.new("RGB", (128, 64)).save(path, format="BMP")
    with pytest.raises(ValueError, match="single-frame PNG"):
        png_frame(path)
    Image.new("RGB", (64, 128)).save(path)
    with pytest.raises(ValueError, match="128x64"):
        png_frame(path)
    Image.new("RGB", (128, 64)).save(
        path,
        save_all=True,
        append_images=[Image.new("RGB", (128, 64), "white")],
        duration=100,
    )
    with pytest.raises(ValueError, match="single-frame PNG"):
        png_frame(path)


def test_reject_16bit_grayscale_instead_of_clipping(tmp_path):
    path = tmp_path / "16bit.png"
    Image.new("I;16", (128, 64), 32768).save(path)
    with pytest.raises(ValueError, match="export an 8-bit PNG"):
        png_frame(path)
