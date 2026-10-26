"""Alternating A/B timing of coupled-run binaries (DSP thread CPU per audio second).

Each pair runs A then B (order swapped on odd pairs) under one exclusive
benchlock hold, so nothing else heavy runs in between. The binaries are
`coupled-run` builds of any generated core (copy them away first: a target dir
is rebuilt in place). Default config: the fast tier and the AOT core, as
`tools/emulator/coupled.py scenario` sets it (digi_env.fast_env). Every run checks the
scenario's PCM hash against the file's pin (or against the first run).

    uv run python -m tools.devtools.coupled_ab A=BIN_A B=BIN_B -s trig1-taps -s tf-br-127 \\
        [--pairs 6] [--scenarios tests/regression/scenarios/dn2-trackfx.ts] [--out OUT.json]
    uv run python -m tools.devtools.coupled_ab A=BIN --pairs 5 -s trig1-taps     # one binary: N runs
    uv run python -m tools.devtools.coupled_ab A=BIN B=BIN --patch B=X.patch ...  # one binary, image B patched

The report prints, per scenario and binary, the median DSP CPU per audio s
(IQR), the CF thread CPU, the real-time factor and the PCM hash; with two
binaries also the paired ratio B/A (median, min, max). The machine load average
is recorded at every run (a quiet machine is not required, a flat one helps).
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.devtools import benchlock  # noqa: E402
from tools.emulator import coupled  # noqa: E402

# Set by --dt2-card: the sample card's ready inputs and its `--card-image` argument.
CARD: tuple[dict[str, pathlib.Path], list[str]] | None = None
# Set by --seconds: cut every run to this many audio seconds.
SECONDS: str | None = None
# Set by --patch LABEL=FILE: a DSP patch file (coupled-run --dsp-patch) for the runs of that label, so one binary can run two images.
PATCHES: dict[str, str] = {}
# Set by --env: extra environment for the runs (interpreter-only: SHARC_AOT=0 ...).
ENV: dict[str, str] = {}


def one_run(
    binary: str,
    scenario: str,
    scenarios: str | None,
    device: str,
    patch: str | None = None,
) -> dict:
    """One headless run; the scenario's numbers from its report."""
    with tempfile.TemporaryDirectory() as tmp:
        out = pathlib.Path(tmp) / "report.json"
        extra = ["--scenario", scenario, "--out", str(out)]
        if SECONDS:
            extra += ["--seconds", SECONDS]
        if patch:
            extra += ["--dsp-patch", patch]
        if scenarios:
            extra = ["--scenarios", str(coupled.scenario_path(scenarios))] + extra
        ins, card = CARD or (coupled.inputs(device), [])
        argv = coupled.run_args(binary, ins, [*card, *extra])
        load = os.getloadavg()[0]
        done = subprocess.run(
            argv,
            cwd=ROOT,
            env=coupled.run_env(ENV, syx=ins["firmware"]),
            capture_output=True,
            text=True,
        )
        if not out.is_file():
            sys.exit("no report from %s:\n%s" % (binary, done.stderr[-2000:]))
        report = json.loads(out.read_text())
    sc = next(s for s in report["scenarios"] if s["name"] == scenario)
    d = sc["dsp_instructions"]
    cpu = sc["cpu"]
    pcm = sc.get("pcm") or {}
    return {
        "dsp_cpu": cpu["dsp_thread_per_audio_s"],
        "cf_cpu": cpu["cf_thread_per_audio_s"],
        "rtf": sc.get("real_time_factor"),
        "pcm": pcm.get("sha256"),
        "ok": (sc.get("gates") or {}).get("ok"),
        "load": load,
        "block_entries": d.get("block_entries"),
        "block_traps": d.get("block_traps"),
        "aot_block": d.get("aot_block"),
        "interpreter": d.get("interpreter"),
        "audio_s": d.get("audio_s") or sc.get("audio_seconds"),
        "exit_code": done.returncode,
    }


def iqr(values: list[float]) -> float:
    if len(values) < 4:
        return max(values) - min(values)
    q = statistics.quantiles(values, n=4)
    return q[2] - q[0]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("binaries", nargs="+", metavar="LABEL=BINARY")
    ap.add_argument("-s", "--scenario", action="append", required=True)
    ap.add_argument("--scenarios", help="scenario file (default: the device's)")
    ap.add_argument("--device", default="dn2")
    ap.add_argument(
        "--dt2-card",
        action="store_true",
        help="DT2: run on the one-sample card of tools/emulator/dt2_sample_check.py",
    )
    ap.add_argument("--pairs", type=int, default=6)
    ap.add_argument("--seconds", help="audio seconds per run (default: the scenario's)")
    ap.add_argument(
        "--patch",
        action="append",
        default=[],
        metavar="LABEL=FILE",
        help="a DSP patch file for the runs of LABEL (tools/patchkit/pk_dsp_diff.py writes one from two images)",
    )
    ap.add_argument(
        "--env", action="append", default=[], metavar="K=V", help="run environment"
    )
    ap.add_argument("--out")
    args = ap.parse_args()
    global SECONDS
    SECONDS = args.seconds
    ENV.update(e.partition("=")[::2] for e in args.env)
    for item in args.patch:
        label, _, path = item.partition("=")
        if not pathlib.Path(path).is_file():
            sys.exit("expected LABEL=FILE, got %r" % item)
        PATCHES[label] = str(pathlib.Path(path).resolve())
    if args.dt2_card:
        from tools.emulator import dt2_sample_check

        global CARD
        CARD = dt2_sample_check.card_inputs()
    bins = []
    for item in args.binaries:
        label, _, path = item.partition("=")
        if not path or not pathlib.Path(path).is_file():
            sys.exit("expected LABEL=BINARY, got %r" % item)
        bins.append((label, path))
    runs: dict[tuple[str, str], list[dict]] = {}
    with benchlock.hold("exclusive", "coupled_ab"):
        for scenario in args.scenario:
            for i in range(args.pairs):
                order = bins if i % 2 == 0 else list(reversed(bins))
                for label, path in order:
                    r = one_run(
                        path, scenario, args.scenarios, args.device, PATCHES.get(label)
                    )
                    r["pair"] = i
                    runs.setdefault((scenario, label), []).append(r)
                    print(
                        "%-12s %-3s pair %d  dsp %.3f  pcm %s  load %.1f"
                        % (
                            scenario,
                            label,
                            i,
                            r["dsp_cpu"],
                            (r["pcm"] or "-")[:16],
                            r["load"],
                        ),
                        flush=True,
                    )
    print()
    for scenario in args.scenario:
        for label, _ in bins:
            rs = runs[(scenario, label)]
            d = [r["dsp_cpu"] for r in rs]
            pcms = sorted({(r["pcm"] or "-")[:16] for r in rs})
            ns = [
                r["dsp_cpu"] * r["audio_s"] * 1e9 / r["interpreter"]
                for r in rs
                if r.get("interpreter") and r.get("audio_s")
            ]
            if ns:
                print(
                    "%-12s %-3s ns per DSP instruction  median %.1f  min %.1f  max %.1f"
                    % (scenario, label, statistics.median(ns), min(ns), max(ns))
                )
            print(
                "%-12s %-3s dsp cpu/audio s  median %.3f  IQR %.3f  min %.3f  max %.3f  n=%d  pcm %s"
                % (
                    scenario,
                    label,
                    statistics.median(d),
                    iqr(d),
                    min(d),
                    max(d),
                    len(d),
                    ",".join(pcms),
                )
            )
        if len(bins) == 2:
            a, b = bins[0][0], bins[1][0]
            ra, rb = runs[(scenario, a)], runs[(scenario, b)]
            ratios = [y["dsp_cpu"] / x["dsp_cpu"] for x, y in zip(ra, rb, strict=True)]
            print(
                "%-12s %s/%s paired ratio  median %.3f  min %.3f  max %.3f"
                % (scenario, b, a, statistics.median(ratios), min(ratios), max(ratios))
            )
    if args.out:
        pathlib.Path(args.out).write_text(
            json.dumps(
                {
                    "when": time.strftime("%F %T"),
                    "runs": {"%s/%s" % k: v for k, v in runs.items()},
                },
                indent=1,
            )
        )


if __name__ == "__main__":
    main()
