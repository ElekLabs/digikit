"""A free-running daemon with the digikit runtime, for live checks of the desktop pages (tools/devtools/memory_live.mjs, ui/preview/live.vite.config.ts).

    uv run python -m tools.devtools.emu_live --device dn2 --run-dir DIR

Starts the device's `digi-emud` (the PGO build the patchkit harness uses; `DIGI_EMUD_<DEV>` names another) on the stock image with the runtime
patched in, runs it free, prints one JSON line `{"pid", "socket", "device"}` and waits. It ends when SIGTERM or SIGINT arrives (stop it by the
PID it printed: `kill PID`), and stops the daemon with it. The caller gives the run directory; nothing is written elsewhere.
"""

from __future__ import annotations

import argparse
import json
import signal
import sys
import time
from pathlib import Path

from tools.control.digi_client import Emu
from tools.firmware import config
from tools.patchkit import pk_device
from tools.patchkit import pk_harness as H

DEVICES = pk_device.acceptance_firmware()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--device", choices=sorted(DEVICES), default="dn2")
    ap.add_argument("--run-dir", type=Path, required=True)
    ap.add_argument(
        "--settle",
        type=float,
        default=12.0,
        help="seconds to run before reporting (the runtime loads its modules)",
    )
    a = ap.parse_args()
    spec = H.FIRMWARE[(a.device, DEVICES[a.device])]
    syx = config.stock(spec["syx"])
    a.run_dir.mkdir(parents=True, exist_ok=True)
    emu = Emu.spawn_coupled(
        syx,
        H.ROOT / spec["ready"],
        "--mirror-dir",
        str(a.run_dir / "mirror"),
        timeout=300,
        binary=H.daemon_binary(a.device, syx),
        run_dir=a.run_dir / "run",
    )
    stop = []
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, lambda *_: stop.append(1))
    try:
        if (
            a.device == "dt2"
        ):  # the DT2 boots on a card built from a folder; the DN2 on none
            from tools.emulator import card_check

            samples = a.run_dir / "samples"
            card_check.make_samples(samples)
            card = emu.call("card.load", dir=str(samples))["card"]
            emu.load(syx, runtime=True, start="auto", card=card)
        else:
            emu.load(syx, runtime=True, start="auto")
        emu.call("run.mode", mode="free")
        time.sleep(a.settle)
        print(
            json.dumps(
                {
                    "pid": emu._proc.pid if emu._proc else None,
                    "socket": emu.path,
                    "device": a.device,
                }
            ),
            flush=True,
        )
        while not stop:
            time.sleep(0.5)
    finally:
        emu.__exit__(None, None, None)
    return 0


if __name__ == "__main__":
    sys.exit(main())
