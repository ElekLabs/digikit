#!/usr/bin/env python3
"""Press every panel code once and record what it does: a contact sheet of the screens and the LED changes.

    uv run python -m tools.emulator.panel_sweep firmware/Syntakt_OS1.42.syx --codes 1-49 --out out/runs/panel-sweep

One cold boot (`panel_drive --save-state`), then one short run per code from that state: dismiss the boot dialog with `--dismiss`,
tap the code, wait, write the screen. The LED records of the front-panel stream (DIGI_PANEL_TX_LOG) are decoded for the scheme that
OS 1.42 uses: `0x20 + n` records carry four LEDs each, two bits per LED (the state number, 3 = idle). A run resumes a state that
already holds the LED states, so a LED that was lit before the press shows no change. Writes `sheet-<first>.png` (12 screens each) and
prints one line per code. Needs a release build of the `panel_drive` example (`cargo build --release -p elektron-native-boot --example
panel_drive`, target dir from `tools/emulator/native_target.py`); `--bin` names another one.
"""

import argparse
import os
import pathlib
import subprocess
import sys

from tools.emulator import native_target, panel_tx

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent


def codes_of(text: str) -> list[int]:
    out: list[int] = []
    for part in text.split(","):
        lo, _, hi = part.partition("-")
        out += range(int(lo), int(hi or lo) + 1)
    return out


def led_states(path: pathlib.Path) -> list[tuple[int, int, int]]:
    """(icount, led, state) for every state byte of `0x20 + n` records in the capture."""
    out = []
    stream = panel_tx.load(str(path))
    i = 0
    while i < len(stream):
        t, c = stream[i]
        if 0x10 <= c <= 0x17:
            i += 10
        elif 0x20 <= c <= 0x2A and i + 1 < len(stream):
            for k in range(4):
                out.append((t, (c - 0x20) * 4 + k, stream[i + 1][1] >> (2 * k) & 3))
            i += 2
        else:
            i += (
                1 + panel_tx.LEN.get(c, 0) + (2 if c in (0xB0, 0xB1, 0xB2, 0xB3) else 0)
            )
    return out


def pbm_to_image(path: pathlib.Path):
    from PIL import Image

    text = path.read_text().split()
    w, h = int(text[1]), int(text[2])
    image = Image.new("L", (w, h))
    image.putdata([255 if b == "1" else 0 for b in text[3:]])
    return image


def sheet(frames: list[tuple[int, pathlib.Path]], out: pathlib.Path) -> None:
    from PIL import Image, ImageDraw

    cols, tile = 4, (256, 128)
    for start in range(0, len(frames), 12):
        part = frames[start : start + 12]
        rows = (len(part) + cols - 1) // cols
        page = Image.new(
            "RGB", (cols * (tile[0] + 4), rows * (tile[1] + 4)), (40, 40, 40)
        )
        for n, (code, path) in enumerate(part):
            image = pbm_to_image(path).resize(tile, Image.NEAREST).convert("RGB")
            ImageDraw.Draw(image).text((2, 2), f"code {code}", fill=(255, 0, 0))
            page.paste(image, ((n % cols) * (tile[0] + 4), (n // cols) * (tile[1] + 4)))
        page.save(out / f"sheet-{part[0][0]}.png")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("syx")
    ap.add_argument("--codes", default="1-48", help="codes to press, e.g. 1-12,17")
    ap.add_argument(
        "--dismiss",
        type=int,
        default=0,
        help="a code to tap first (the NO key closes the boot dialog)",
    )
    ap.add_argument("--wait", default="20M", help="instructions to run after the tap")
    ap.add_argument("--out", default="out/runs/panel-sweep")
    ap.add_argument("--bin", default=None)
    ap.add_argument("--max", default="800M", help="boot budget")
    a = ap.parse_args()
    out = (ROOT / a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    binary = a.bin or str(
        native_target.target_dir() / "release" / "examples" / "panel_drive"
    )
    state = out / "boot.state"
    base = [binary, a.syx, "--out", str(out)]
    lead = f"tap:{a.dismiss},wait:10M," if a.dismiss else ""
    boot = subprocess.run(
        base
        + [
            "--max",
            a.max,
            "--after",
            "20M",
            "--save-state",
            str(state),
            "--steps",
            "wait:1M",
        ],
        capture_output=True,
        text=True,
    )
    if boot.returncode:
        sys.exit(boot.stdout + boot.stderr)
    frames = []
    failed = 0
    for code in codes_of(a.codes):
        log = out / f"code{code}.log"
        env = {**os.environ, "DIGI_PANEL_TX_LOG": str(log)}
        steps = f"{lead}tap:{code},wait:{a.wait},frame:code{code}"
        run = subprocess.run(
            base + ["--state", str(state), "--steps", steps],
            capture_output=True,
            text=True,
            env=env,
        )
        if run.returncode:
            print(f"{code:3d}  {run.stdout.strip()[-120:]}")
            failed += 1
            continue
        frames.append((code, out / f"code{code}.pbm"))
        marks = [m for m in panel_tx.marks(str(log)) if m[1] == code]
        if not marks:
            continue
        press = marks[0][0]
        changes = [f"{led}:{state}" for t, led, state in led_states(log) if t >= press]
        print(f"{code:3d}  leds {' '.join(changes) or '-'}")
    sheet(frames, out)
    print(f"sheets in {out}")
    if failed:
        sys.exit(f"{failed} code run(s) failed")


if __name__ == "__main__":
    main()
