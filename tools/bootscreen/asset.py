"""Bounded experimental DKBS pack/inspect tool; see docs/design/bootscreen-assets.md."""

from __future__ import annotations

import argparse
import base64
import json
import struct
import zlib
from collections import Counter
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PIL import Image

from tools.control.digi_client import oled_png

WIDTH, HEIGHT = 128, 64
FRAME_BYTES = 1024
MAX_TICKS = 300
MAX_BYTES = 65536
HEADER = struct.Struct(">4sHHII")
RECORD = struct.Struct(">HHB3s")


@dataclass(frozen=True)
class Frame:
    ticks: int
    pixels: bytes


@dataclass(frozen=True)
class Asset:
    frames: tuple[Frame, ...]
    codecs: tuple[int, ...]
    file_bytes: int

    def stats(self) -> dict[str, Any]:
        ticks = sum(frame.ticks for frame in self.frames)
        return {
            "width": WIDTH,
            "height": HEIGHT,
            "nominal_hz": 30,
            "records": len(self.frames),
            "ticks": ticks,
            "bytes": self.file_bytes,
            "raw_tick_bytes": ticks * FRAME_BYTES,
            "codecs": dict(
                Counter("raw" if c == 0 else "xor-run" for c in self.codecs)
            ),
        }


def _integer(value: object, low: int, high: int, name: str) -> int:
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"{name} must be an integer in {low}..{high}")
    return value


def png_frame(path: Path, threshold: int = 128) -> bytes:
    """Composite over black, integer luma, then native column-major MSB-first bits."""
    _integer(threshold, 0, 255, "threshold")
    with Image.open(path) as image:
        if (
            image.format != "PNG"
            or image.get_format_mimetype() == "image/apng"
            or getattr(image, "n_frames", 1) != 1
        ):
            raise ValueError(f"{path}: expected a single-frame PNG (no APNG)")
        if image.size != (WIDTH, HEIGHT):
            raise ValueError(
                f"{path}: expected 128x64, got {image.width}x{image.height}"
            )
        if image.mode not in {"1", "L", "LA", "P", "RGB", "RGBA"}:
            raise ValueError(
                f"{path}: unsupported mode {image.mode}; export an 8-bit PNG"
            )
        rgba = image.convert("RGBA")
        rgb = Image.alpha_composite(
            Image.new("RGBA", image.size, (0, 0, 0, 255)), rgba
        ).convert("RGB")
        pixels = rgb.tobytes()
    result = bytearray(FRAME_BYTES)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            at = 3 * (y * WIDTH + x)
            r, g, b = pixels[at : at + 3]
            if (77 * r + 150 * g + 29 * b + 128) >> 8 > threshold:
                result[8 * x + y // 8] |= 1 << (7 - y % 8)
    return bytes(result)


def load_manifest(path: Path) -> tuple[Frame, ...]:
    source = json.loads(path.read_text())
    if not isinstance(source, dict) or set(source) not in (
        {"version", "frames"},
        {"version", "threshold", "frames"},
    ):
        raise ValueError(
            "manifest requires exactly version, frames and optional threshold"
        )
    _integer(source["version"], 1, 1, "version")
    threshold = _integer(source.get("threshold", 128), 0, 255, "threshold")
    frames = source["frames"]
    if not isinstance(frames, list) or not 1 <= len(frames) <= MAX_TICKS:
        raise ValueError("frames must be a list of 1..300 records")
    specs: list[tuple[Path, int]] = []
    total = 0
    for frame in frames:
        if not isinstance(frame, dict) or set(frame) != {"file", "ticks"}:
            raise ValueError("each frame requires exactly file and ticks")
        if not isinstance(frame["file"], str) or not frame["file"]:
            raise ValueError("frame file must be a nonempty string")
        ticks = _integer(frame["ticks"], 1, MAX_TICKS, "ticks")
        total += ticks
        if total > MAX_TICKS:
            raise ValueError("total ticks must be in 1..300")
        specs.append((path.parent / frame["file"], ticks))
    return tuple(Frame(ticks, png_frame(file, threshold)) for file, ticks in specs)


def _runs(data: bytes) -> bytes:
    result = bytearray()
    at = 0
    while at < len(data):
        end = at + 1
        while end < min(at + 128, len(data)) and data[end] == data[at]:
            end += 1
        if end - at >= 2:
            result.extend((0x80 | (end - at - 1), data[at]))
        else:
            while end < min(at + 128, len(data)):
                if end + 1 < len(data) and data[end] == data[end + 1]:
                    break
                end += 1
            result.append(end - at - 1)
            result.extend(data[at:end])
        at = end
    return bytes(result)


def _unruns(payload: bytes) -> bytes:
    result = bytearray()
    at = 0
    while at < len(payload):
        ctrl = payload[at]
        count = (ctrl & 127) + 1
        at += 1
        take = 1 if ctrl & 128 else count
        if len(result) + count > FRAME_BYTES or at + take > len(payload):
            raise ValueError("XOR-RUN expansion exceeds frame or payload is truncated")
        if ctrl & 128:
            result.extend(bytes([payload[at]]) * count)
        else:
            result.extend(payload[at : at + count])
        at += take
    if len(result) != FRAME_BYTES:
        raise ValueError("XOR-RUN must expand to exactly 1024 bytes")
    return bytes(result)


def encode(frames: Sequence[Frame]) -> bytes:
    if not 1 <= len(frames) <= MAX_TICKS:
        raise ValueError("record count must be in 1..300")
    merged: list[Frame] = []
    total = 0
    for frame in frames:
        ticks = _integer(frame.ticks, 1, MAX_TICKS, "ticks")
        if not isinstance(frame.pixels, bytes) or len(frame.pixels) != FRAME_BYTES:
            raise ValueError("native frame must be exactly 1024 bytes")
        total += ticks
        if total > MAX_TICKS:
            raise ValueError("total ticks must be in 1..300")
        if merged and merged[-1].pixels == frame.pixels:
            merged[-1] = Frame(merged[-1].ticks + ticks, frame.pixels)
        else:
            merged.append(frame)
    body = bytearray()
    previous = bytes(FRAME_BYTES)
    for frame in merged:
        packed = _runs(
            bytes(a ^ b for a, b in zip(frame.pixels, previous, strict=True))
        )
        codec, payload = (1, packed) if len(packed) < FRAME_BYTES else (0, frame.pixels)
        body.extend(RECORD.pack(frame.ticks, len(payload), codec, bytes(3)))
        body.extend(payload)
        if HEADER.size + len(body) > MAX_BYTES:
            raise ValueError(
                f"asset needs at least {HEADER.size + len(body)} bytes; maximum is {MAX_BYTES}"
            )
        previous = frame.pixels
    return HEADER.pack(b"DKBS", 1, len(merged), total, zlib.crc32(body)) + body


def decode(data: bytes) -> Asset:
    if len(data) > MAX_BYTES:
        raise ValueError(f"asset is {len(data)} bytes; maximum is {MAX_BYTES}")
    if len(data) < HEADER.size:
        raise ValueError("truncated DKBS header")
    magic, version, count, ticks, crc = HEADER.unpack_from(data)
    if magic != b"DKBS" or version != 1:
        raise ValueError("unsupported DKBS magic/version")
    _integer(count, 1, MAX_TICKS, "record count")
    _integer(ticks, 1, MAX_TICKS, "total ticks")
    if zlib.crc32(data[HEADER.size :]) != crc:
        raise ValueError("body CRC32 mismatch")
    at, total = HEADER.size, 0
    frames: list[Frame] = []
    codecs: list[int] = []
    previous = bytes(FRAME_BYTES)
    for _ in range(count):
        if at + RECORD.size > len(data):
            raise ValueError("truncated record header")
        hold, size, codec, reserved = RECORD.unpack_from(data, at)
        _integer(hold, 1, MAX_TICKS, "hold ticks")
        total += hold
        if total > ticks:
            raise ValueError("record holds exceed declared total ticks")
        if reserved != bytes(3) or codec not in (0, 1):
            raise ValueError("invalid codec or nonzero reserved bytes")
        if size == 0 or (codec == 0 and size != FRAME_BYTES):
            raise ValueError("invalid payload length")
        at += RECORD.size
        if at + size > len(data):
            raise ValueError("truncated record payload")
        payload = data[at : at + size]
        pixels = (
            payload
            if codec == 0
            else bytes(a ^ b for a, b in zip(_unruns(payload), previous, strict=True))
        )
        frames.append(Frame(hold, pixels))
        codecs.append(codec)
        previous = pixels
        at += size
    if at != len(data) or total != ticks:
        raise ValueError("trailing bytes or mismatched total ticks")
    return Asset(tuple(frames), tuple(codecs), len(data))


def native_to_oled(pixels: bytes) -> bytes:
    """Reverse byte and bit order per column for the existing OLED PNG helper."""
    if len(pixels) != FRAME_BYTES:
        raise ValueError("native frame must be exactly 1024 bytes")
    result = bytearray(FRAME_BYTES)
    for x in range(WIDTH):
        for page in range(8):
            value = pixels[8 * x + page]
            result[8 * x + 7 - page] = int(f"{value:08b}"[::-1], 2)
    return bytes(result)


def write_preview(asset: Asset, path: Path) -> None:
    frames = [
        {
            "ticks": frame.ticks,
            "src": "data:image/png;base64,"
            + base64.b64encode(oled_png(native_to_oled(frame.pixels), scale=1)).decode(
                "ascii"
            ),
        }
        for frame in asset.frames
    ]
    path.write_text(
        """<!doctype html><meta charset="utf-8"><title>Experimental DKBS preview</title>
<style>body{background:#222;color:white;font:16px sans-serif}canvas{width:512px;height:256px;image-rendering:pixelated;background:black}</style>
<p>Experimental host preview · 30 nominal ticks/s</p><canvas width="128" height="64"></canvas>
<p><button>Pause</button> <input type="range" min="0" step="1" value="0"><output></output></p>
<script>
const frames=FRAMES, total=frames.reduce((n,f)=>n+f.ticks,0), canvas=document.querySelector('canvas'), ctx=canvas.getContext('2d');
const slider=document.querySelector('input'), label=document.querySelector('output'), button=document.querySelector('button');
slider.max=total-1; let tick=0,playing=true,last;
const images=frames.map(f=>{const i=new Image();i.src=f.src;return i});
function draw(){let n=tick,k=0;while(n>=frames[k].ticks)n-=frames[k++].ticks;ctx.clearRect(0,0,128,64);if(images[k].complete)ctx.drawImage(images[k],0,0);slider.value=tick;label.value=`${tick+1}/${total}`}
button.onclick=()=>{playing=!playing;button.textContent=playing?'Pause':'Play';last=undefined};
slider.oninput=()=>{tick=Number(slider.value);draw();last=undefined};
function animate(now){if(last===undefined)last=now;const steps=Math.floor((now-last)*30/1000);if(playing&&steps){tick=(tick+steps)%total;last+=steps*1000/30}draw();requestAnimationFrame(animate)}
requestAnimationFrame(animate);
</script>""".replace("FRAMES", json.dumps(frames))
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("pack")
    pack.add_argument("manifest", type=Path)
    pack.add_argument("-o", "--output", type=Path, required=True)
    pack.add_argument("--preview", type=Path)
    inspect = commands.add_parser("inspect")
    inspect.add_argument("asset", type=Path)
    inspect.add_argument("--preview", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "pack":
            data = encode(load_manifest(args.manifest))
            asset = decode(data)
            args.output.write_bytes(data)
        else:
            with args.asset.open("rb") as source:
                asset = decode(source.read(MAX_BYTES + 1))
        if args.preview:
            write_preview(asset, args.preview)
        print(json.dumps(asset.stats(), sort_keys=True))
    except (OSError, ValueError) as error:
        parser.exit(2, f"error: {error}\n")


if __name__ == "__main__":
    main()
