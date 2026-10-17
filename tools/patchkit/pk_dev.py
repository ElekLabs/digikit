#!/usr/bin/env python3
"""The patchkit developer loop: watch a module, rebuild, patch, inject into a running emulator, run its quick tests.

    mise run patch:dev -- --modules fx/my-drive[,test/cf-demo] [--device dt2] [--firmware 1.17] [--once] [--no-tests]

One daemon stays up on the stock firmware's ready inputs (DT2 with a one-sample card). Each time a file under a
module's `src/` or its `module.toml` changes, the loop runs: `pk_build` (src -> .dko), `patchkit patch` (from the stock
image), `firmware.load` into the daemon (a DSP-only change is injected hot in about 0.1 s; a MAIN change cold-boots the
ColdFire with DSP audio, cached by MAIN hash), then the quick tests of the modules (`modules/*/*/tests/*.toml`, the
`[test] quick = true` ones) from the ready state. One summary line per iteration:

    12:01:09 OK   build 1.9 s  patch 1.1 s  load 0.2 s (hot-dsp)  tests 2/2 6.8 s   edit->running 3.3 s  edit->tested 10.1 s

`edit->running` is from the file's modification time to the end of the load: the patched firmware is running and
ready for input. A failing step prints its error and the loop waits for the next edit. `--once` does one iteration and
exits (no watching). Output images are firmware-derived and stay under out/patch/.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

from tools import python_env  # noqa: E402
from tools.patchkit import pk_build  # noqa: E402
from tools.patchkit import pk_harness as h  # noqa: E402

POLL_S = 0.25


def watched(modules: list[str]) -> dict[Path, float]:
    """mtime of every file the loop reacts to: `src/**` and `module.toml` of the modules."""
    seen: dict[Path, float] = {}
    for mod in modules:
        base = h.ROOT / "modules" / mod
        files = [base / "module.toml", *sorted((base / "src").rglob("*"))]
        for f in files:
            if f.is_file():
                seen[f] = f.stat().st_mtime
    return seen


class Dev:
    """The loop's state: the cell, the daemon session, the last image."""

    def __init__(self, cell: h.Cell, tests: bool = True):
        self.cell, self.tests = cell, tests
        self.goldens = h.Goldens()
        self.session = h.Session(cell, self.goldens, run_name=f"dev-{cell.device}")
        self.opened = False
        self.out = (
            ROOT / "out" / "patch" / f"dev-{cell.device}-{cell.firmware}.ungated.syx"
        )

    def start(self) -> None:
        # the daemon starts on the stock firmware; the first iteration loads the patched image
        self.session.open(image=self.session.syx)
        self.opened = True

    def close(self) -> None:
        self.session.close()

    def build(self) -> float:
        started = time.monotonic()
        for mod in self.cell.modules:
            pk_build.build_module(h.ROOT / "modules" / mod, check=False)
        return time.monotonic() - started

    def patch(self) -> float:
        started = time.monotonic()
        r = subprocess.run(
            [
                sys.executable,
                "-m",
                "tools.patchkit.patchkit",
                "patch",
                "--firmware",
                str(self.session.syx),
                "--modules",
                ",".join(self.cell.modules),
                # Developer-loop images go to the emulator, not the unit; the gate would stop DSP modules.
                "--no-encoding-gate",
                "-o",
                str(self.out),
            ],
            cwd=ROOT,
            env=python_env(),
            capture_output=True,
            text=True,
        )
        if r.returncode:
            raise h.HarnessError((r.stderr or r.stdout).strip()[-1200:])
        return time.monotonic() - started

    def iterate(self, edited_at: float | None = None) -> str:
        """One build, patch, inject, test round; the summary line. `edited_at`: wall time of the edit."""
        t0 = time.time()
        edited_at = edited_at or t0
        stamp = time.strftime("%H:%M:%S")
        try:
            tb = self.build()
            tp = self.patch()
            t = time.monotonic()
            report = self.session.load(self.out)
            tl = time.monotonic() - t
        except (pk_build.BuildError, Exception) as e:  # noqa: BLE001 - one line per failure, the loop goes on
            kind = "load" if isinstance(e, h.LoadFailure) else "build/patch"
            return f"{stamp} FAIL {kind}: {e}"
        running = time.time() - edited_at
        line = f"{stamp} OK   build {tb:.1f} s  patch {tp:.1f} s  load {tl:.1f} s ({report.get('start_used', '?')})"
        if not self.tests:
            return f"{line}   edit->running {running:.1f} s"
        t = time.monotonic()
        failed: list[str] = []
        names = list(
            h.module_tests(self.cell.modules, self.cell.device, quick_only=True)
        )
        for name, steps in names:
            if not self.session.run_segment(name, steps):
                failed.append(name)
        tt = time.monotonic() - t
        passed = len(names) - len(failed)
        tested = time.time() - edited_at
        verdict = "OK  " if not failed else "FAIL"
        line = (
            f"{stamp} {verdict} build {tb:.1f} s  patch {tp:.1f} s  load {tl:.1f} s "
            f"({report.get('start_used', '?')})  tests {passed}/{len(names)} {tt:.1f} s   "
            f"edit->running {running:.1f} s  edit->tested {tested:.1f} s"
        )
        for c in self.session.checks:
            if not c.ok:
                line += f"\n    {c.name}: {c.detail}"
        self.session.checks.clear()
        return line


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--modules", required=True, help="a,b (ids under modules/)")
    ap.add_argument("--device", default="dt2", choices=("dt2", "dn2"))
    ap.add_argument("--firmware", help="default: 1.17 (dt2), 1.12 (dn2)")
    ap.add_argument("--once", action="store_true")
    ap.add_argument("--no-tests", action="store_true")
    a = ap.parse_args(argv)
    fw = a.firmware or {"dt2": "1.17", "dn2": "1.12"}[a.device]
    cell = h.Cell(a.device, fw, [m for m in a.modules.split(",") if m])
    for m in cell.modules:
        if not (h.ROOT / "modules" / m / "module.toml").is_file():
            raise SystemExit(f"no module {m!r} under modules/")
    dev = Dev(cell, tests=not a.no_tests)
    try:
        started = time.monotonic()
        dev.start()
        print(
            f"daemon up in {time.monotonic() - started:.1f} s ({cell.name})", flush=True
        )
        print(dev.iterate(), flush=True)
        if a.once:
            return 0
        seen = watched(cell.modules)
        print(
            "watching; edit a file under modules/<id>/src/ or module.toml (ctrl-c to stop)",
            flush=True,
        )
        while True:
            time.sleep(POLL_S)
            now = watched(cell.modules)
            if now != seen:
                edited = max(
                    (m for f, m in now.items() if seen.get(f) != m),
                    default=time.time(),
                )
                time.sleep(0.2)  # let an editor finish writing
                seen = watched(cell.modules)
                print(dev.iterate(edited_at=edited), flush=True)
    except KeyboardInterrupt:
        return 0
    except h.HarnessError as e:
        print(f"setup: {e}", file=sys.stderr)
        return 2
    finally:
        dev.close()


if __name__ == "__main__":
    os.chdir(ROOT)
    sys.exit(main(sys.argv[1:]))
