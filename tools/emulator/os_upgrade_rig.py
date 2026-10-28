#!/usr/bin/env python3
"""Bounded emulator rig for the MAIN OS upgrade receive path (finding 75).

Boots a stock image from a ready snapshot (zero DSP), opens SETTINGS > SYSTEM > OS UPGRADE with panel taps, streams a
`.syx` over the emulated USB MIDI input (`midi.usb.in`, the firmware's SysEx assembler), and records per step: the
receiver state words, `midi.usb.out` bytes, the OLED frame. Optional YES at the confirm dialog, then the run goes on
until the flash write ends in the soft reset.

Every run is bounded by `--limit` (guest instructions in total). It starts one private `digi-emud` and stops only that
PID. Example:

    uv run python -m tools.emulator.os_upgrade_rig dt2 --binary BIN --ready out/ready-pk-dt2-117 \
        --syx firmware/Digitakt_II_OS1.17.syx --out out/runs/os-upgrade --yes --limit 12000000000

With `--patchkit BIN` the rig does not send the stream itself: after the panel steps it runs `patchkit flash STOCK --emulator SOCKET`, which sends
the patched image through the shared sender, compares the emulated flash with the image, waits for the restart and reads INFO. `--restore-after`
then opens OS UPGRADE again on the patched image and runs `patchkit flash --restore` (the stock file back).

With `--exec CMD` the rig runs CMD instead (arguments split like a shell, `{socket}` replaced by the control socket, `DIGI_FW_EMU_SOCKET` set), for
example the Firmware page's own flash logic headless: the Vitest file apps/web/tests/firmware-emulator.test.ts, which reads the socket from
`DIGI_FW_EMU_SOCKET` (`pnpm --filter @digi/web exec vitest run --root . --config ../../vitest.host.config.ts --project web-transfer FILE`).
`--exec-restore CMD` then opens OS UPGRADE again and runs that.

State words (MAIN OS receive state, finding 75): byte 0 = state (0 idle, 1 header seen, 2 receiving, 3 complete),
+8 = announced message count, +0xc = messages taken, +0x10 = next sequence number.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shlex
import subprocess
import sys
import time

from tools.control.digi_client import Emu

ROOT = pathlib.Path(__file__).resolve().parents[2]
YES, NO, DOWN = 10, 12, 14
SETTINGS = 8
# receive state object (FUN_40120bcc returns it) per image; the DT2 value is 1.17, the DN2 value 1.12
STATE = {"dt2": 0x403A0ED0, "dn2": 0x4038BEC0}
SYX = {"dt2": "Digitakt_II_OS1.17.syx", "dn2": "Digitone_II_OS1.12.syx"}


def split_messages(stream: bytes) -> list[bytes]:
    out, i = [], 0
    while i < len(stream):
        j = stream.index(0xF7, i)
        out.append(stream[i : j + 1])
        i = j + 1
    return out


class Rig:
    def __init__(self, emu: Emu, device: str, out: pathlib.Path, limit: int) -> None:
        self.emu, self.device, self.out, self.limit = emu, device, out, limit
        self.used = 0
        self.log: list[dict] = []

    def step(self, n: int) -> None:
        if self.used + n > self.limit:
            raise SystemExit(f"limit {self.limit} instructions reached")
        self.emu.step(n)
        self.used += n

    def tap(self, code: int, gap: int = 30_000_000) -> None:
        self.emu.tap(code, hold=2_000_000)
        self.step(2_000_000 + gap)

    def frame(self, name: str) -> str:
        self.out.mkdir(parents=True, exist_ok=True)
        (self.out / f"{name}.png").write_bytes(self.emu.frame_png())
        return self.emu.frame_hash()

    def state(self) -> dict:
        base = STATE[self.device]
        word = self.emu.peek32(base) or 0
        return {
            "state": word >> 24,
            "total": self.emu.peek32(base + 8),
            "taken": self.emu.peek32(base + 0xC),
            "next_seq": self.emu.peek32(base + 0x10),
        }

    def open_os_upgrade(self) -> None:
        self.tap(NO)
        self.tap(SETTINGS)
        for _ in range(7):
            self.tap(DOWN, 15_000_000)
        self.tap(YES, 40_000_000)  # SYSTEM
        self.tap(DOWN, 20_000_000)  # OS UPGRADE
        self.tap(YES, 60_000_000)
        self.step(100_000_000)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("device", choices=["dt2", "dn2"])
    p.add_argument(
        "--binary", required=True, help="digi-emud binary (an immutable copy)"
    )
    p.add_argument("--ready", required=True, help="ready dir with digi-audio-m5.snap")
    p.add_argument(
        "--syx", help="stream to send (default: the stock file of the device)"
    )
    p.add_argument("--out", required=True)
    p.add_argument(
        "--limit", type=int, default=12_000_000_000, help="guest instructions in total"
    )
    p.add_argument(
        "--messages", type=int, help="send only the first N messages (a cut transfer)"
    )
    p.add_argument("--chunk", type=int, help="USB bytes per call (default 64)")
    p.add_argument(
        "--period", type=int, help="instructions between USB calls (default 132000)"
    )
    p.add_argument(
        "--poll",
        type=int,
        default=20_000_000,
        help="instructions between state samples",
    )
    p.add_argument(
        "--idle",
        type=int,
        default=25,
        help="stop after this many polls without a state change",
    )
    p.add_argument("--yes", action="store_true", help="press YES at the confirm dialog")
    p.add_argument(
        "--no-open",
        action="store_true",
        help="do not open the OS UPGRADE view (RPC-less control run)",
    )
    p.add_argument(
        "--patchkit",
        help="patchkit binary: flash with `patchkit flash --emulator` instead of sending the stream here",
    )
    p.add_argument(
        "--restore-after",
        action="store_true",
        help="with --patchkit: after the flash, restore the stock file the same way",
    )
    p.add_argument(
        "--exec",
        dest="exec_cmd",
        help="command to run at the OS UPGRADE screen instead of sending the stream here; {socket} is the control socket",
    )
    p.add_argument(
        "--exec-restore",
        dest="exec_restore",
        help="with --exec: open OS UPGRADE again after it and run this command",
    )
    a = p.parse_args(argv)

    syx = pathlib.Path(a.syx or ROOT / "firmware" / SYX[a.device])
    msgs = split_messages(syx.read_bytes())
    if a.messages is not None:
        msgs = msgs[: a.messages]
    out = pathlib.Path(a.out)
    snap = pathlib.Path(a.ready) / "digi-audio-m5.snap"
    emu = Emu.spawn(
        "--dsp",
        "zero",
        "--cf-snapshot",
        str(snap),
        binary=a.binary,
        run_dir=out / "run",
    )
    t0 = time.monotonic()
    try:
        emu.call(
            "firmware.load",
            syx=str((ROOT / "firmware" / SYX[a.device]).resolve()),
            start="snapshot",
            force=True,
            auto_no=True,
        )
        rig = Rig(emu, a.device, out, a.limit)
        rig.step(300_000_000)
        if not a.no_open:
            rig.open_os_upgrade()
        print("waiting screen", rig.frame("1-waiting"), rig.state())
        if a.exec_cmd:
            runs = [("exec", a.exec_cmd)] + (
                [("exec-restore", a.exec_restore)] if a.exec_restore else []
            )
            env = {**os.environ, "DIGI_FW_EMU_SOCKET": emu.path}
            for n, (name, command) in enumerate(runs):
                if n:
                    rig.open_os_upgrade()
                    print("waiting screen", rig.frame(f"{n + 1}-waiting-{name}"))
                argv_run = [
                    part.format(socket=emu.path) for part in shlex.split(command)
                ]
                t1 = time.monotonic()
                done = subprocess.run(argv_run, cwd=ROOT, env=env)
                print(
                    name,
                    "exit",
                    done.returncode,
                    "wall",
                    round(time.monotonic() - t1, 1),
                    "s",
                )
                print("after", name, rig.frame(f"{n + 1}-after-{name}"))
                if done.returncode:
                    return done.returncode
            return 0
        if a.patchkit:
            stock = ROOT / "firmware" / SYX[a.device]
            runs = [("flash", [])] + (
                [("restore", ["--restore"])] if a.restore_after else []
            )
            for n, (name, extra) in enumerate(runs):
                if n:
                    rig.open_os_upgrade()
                    print("waiting screen", rig.frame(f"{n + 1}-waiting-{name}"))
                cmd = [
                    a.patchkit,
                    "flash",
                    str(stock),
                    "--emulator",
                    emu.path,
                    "--yes",
                    *extra,
                ]
                t1 = time.monotonic()
                done = subprocess.run(cmd, cwd=ROOT)
                print(
                    name,
                    "exit",
                    done.returncode,
                    "wall",
                    round(time.monotonic() - t1, 1),
                    "s",
                )
                print("after", name, rig.frame(f"{n + 1}-after-{name}"))
                if done.returncode:
                    return done.returncode
            return 0
        emu.usb_out()
        emu.usb_in(b"".join(msgs), chunk=a.chunk, period=a.period)
        sent_at = rig.used
        last = None
        out_bytes = b""
        done_at = None
        idle = 0
        events: list[dict] = []
        while rig.used < a.limit:
            rig.step(a.poll)
            st = rig.state()
            o = emu.usb_out()
            out_bytes += o
            if st != last or o:
                events.append({"instr": rig.used - sent_at, "usb_out": len(o), **st})
                last = st
                idle = 0
            else:
                idle += 1
                if idle >= a.idle:
                    print("no progress for", idle * a.poll, "instructions", st)
                    break
            if st["state"] == 3 and done_at is None:
                done_at = rig.used - sent_at
                print("stream complete", st, "instructions since first send", done_at)
                break
        rig.step(a.poll * 5)
        print(
            "after stream",
            rig.frame("2-after-stream"),
            rig.state(),
            "usb_out bytes",
            len(out_bytes),
        )
        if a.yes and done_at is not None:
            rig.tap(YES, 100_000_000)
            print("after YES", rig.frame("3-after-yes"))
            seen = set()
            while rig.used < a.limit:
                rig.step(100_000_000)
                try:
                    h = rig.frame("4-latest")
                except Exception as err:  # the soft reset after the flash write leaves no frame for a while
                    print("no frame at", rig.used, err)
                    break
                if h not in seen:
                    seen.add(h)
                    (out / f"5-screen-{len(seen)}.png").write_bytes(emu.frame_png())
                    print("screen", len(seen), h, "at", rig.used)
                if len(seen) >= 12:
                    break
        (out / "events.json").write_text(
            json.dumps({"messages": len(msgs), "events": events}, indent=1)
        )
        print(
            "wall",
            round(time.monotonic() - t0, 1),
            "s; guest",
            rig.used,
            "instructions; usb_out",
            out_bytes[:64].hex(),
        )
    finally:
        emu.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
