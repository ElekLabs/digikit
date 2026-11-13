#!/usr/bin/env python3
"""The standard DSP performance report: where the CPU goes, not only which tier.

For each configuration (tools/devtools/fastcov.py names) it runs the coupled scenario
twice -- once clean (DSP CPU per audio second, PCM hash, tier split, kernel
calls) and once with the diagnostics on (SHARC_FAST_HIST and SHARC_FAST_COV:
interpreted instructions per PC, calls and instructions per region) -- and
ranks the DSP functions by an estimated cost:

    cost(f) = interpreted insns(f) * ns_interp
            + region calls(f)      * ns_call      (the engine round trip)
            + kernel insns(f)      * ns_kernel

The three constants are measured, not assumed: `--calibrate` runs a set of
tier configurations and fits CPU = a*interp + b*calls + k*kernel by
least squares (out/native/perf/calibration.json). Runs are unlocked unless
`--timed` (then the exclusive benchmark lock is taken, for milestone numbers).

    uv run python -m tools.devtools.perf [--config NAME ...] [--seconds 1.5] [--timed]
    uv run python -m tools.devtools.perf --calibrate
    mise run perf -- --config static-blocks+follow --seconds 1.5

Configurations are those of tools/devtools/fastcov.py (`default`, `static-blocks`,
`static-blocks+follow`, `fast-auto:256,64`, ...) and `hybrid-top:N`: the default
config (AOT on) plus fast regions armed only at the static block starts of the
top N functions of the cost ranking (a fast-only diagnostic run, cached). The firmware comes from
DIGI_EMU_SYX like fastcov. Each run writes out/runs/perf/<stamp>/ (latest symlink) with report.md
and perf.json.
"""

from __future__ import annotations

import argparse
import collections
import datetime
import json
import os
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
from tools import python_env  # noqa: E402
from tools.devtools import fastcov  # noqa: E402
from tools.emulator import coupled, digi_env  # noqa: E402
from tools.firmware import device as device_files  # noqa: E402

OUT = ROOT / "out" / "native" / "perf"
CALIB = OUT / "calibration.json"
# Used until `--calibrate` has run: the profile of 2026-10-07 (docs/findings/18-dsp-performance.md).
DEFAULT_COSTS = {
    "interp": 119.0,
    "call": 100.0,
    "kernel": 0.5,
    "aot": 4.3,
    "audio": 0.0,
}
CALIBRATION_CONFIGS = [
    "static-blocks",
    "static-blocks+follow",
    "static-insns",
    "static-insns+follow",
    "fast-auto:256,64",
]


def load_costs() -> dict[str, float]:
    if CALIB.is_file():
        return {**DEFAULT_COSTS, **json.loads(CALIB.read_text())["costs"]}
    return dict(DEFAULT_COSTS)


def solve(a: list[list[float]], b: list[float]) -> list[float]:
    """Gaussian elimination with partial pivoting (n x n)."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(m[r][c]))
        m[c], m[p] = m[p], m[c]
        if abs(m[c][c]) < 1e-12:
            raise ValueError("singular fit")
        for r in range(n):
            if r != c:
                f = m[r][c] / m[c][c]
                m[r] = [x - f * y for x, y in zip(m[r], m[c], strict=True)]
    return [m[i][n] / m[i][i] for i in range(n)]


def fit(samples: list[dict], kernel_ns: float = 0.5) -> dict[str, float]:
    """Least squares of cpu_ns - k*kernel = a*interp + b*calls.

    The kernel cost cannot be fitted: kernel instructions are about the same
    in every configuration, so they are confounded with the constant. It is
    the measured gate figure (docs/findings/18-dsp-performance.md, 0.435-0.5 ns/insn). A
    per-audio-second constant is not fitted: it is collinear with the others on
    these runs and came out negative.
    """
    keys = ["interp", "call"]
    rows = [[s[k] for k in keys] for s in samples]
    ys = [s["cpu_ns"] - kernel_ns * s["kernel"] for s in samples]
    n = len(keys)
    sc = [max(abs(r[i]) for r in rows) or 1.0 for i in range(n)]
    ata = [
        [sum(r[i] * r[j] for r in rows) / (sc[i] * sc[j]) for j in range(n)]
        for i in range(n)
    ]
    aty = [
        sum(r[i] * y for r, y in zip(rows, ys, strict=True)) / sc[i] for i in range(n)
    ]
    x = solve(ata, aty)
    out = {k: x[i] / sc[i] for i, k in enumerate(keys)}
    out["kernel"] = kernel_ns
    out["aot"] = DEFAULT_COSTS["aot"]
    return out


def counts(sc: dict) -> dict:
    """Per audio second counts and CPU of a scenario report."""
    d = sc["dsp_instructions"]
    audio = sc.get("audio_seconds") or 1.0
    return {
        "cpu_ns": sc["cpu"]["dsp_thread_s"] * 1e9,
        "audio": audio,
        "interp": d.get("interpreter", 0),
        "call": d.get("fast_calls", 0),
        "kernel": d.get("fast_tier", 0),
        "aot": d.get("aot_block", 0),
        "idle": d.get("idle_skip", 0),
        "executed": d.get("instructions", 0) - d.get("idle_skip", 0),
        "pcm": (sc.get("pcm") or {}).get("sha256"),
        "ok": (sc.get("gates") or {}).get("ok"),
    }


def timed_run(binary, scenario, seconds, env, label, out, timed, timeout):
    fastcov.NO_LOCK = not timed
    report, errfile, code = fastcov.run_once(
        binary, scenario, seconds, env, label, True, out, timeout
    )
    sc = fastcov.scenario_report(report, scenario)
    if sc is None:
        raise SystemExit(f"perf: run of {label} failed (exit {code}), see {errfile}")
    return {**counts(sc), "report": str(report)}


def diag_run(binary, scenario, seconds, env, label, out, timeout):
    report, errfile, code = fastcov.run_once(
        binary,
        scenario,
        seconds,
        {**env, **fastcov.DIAG_ENV},
        label,
        False,
        out,
        timeout,
    )
    return fastcov.Diag(errfile.read_text(errors="replace"))


def rank(diag, funcs, costs: dict[str, float], audio: float):
    """Per-function estimated cost (ms per audio second) from a diagnostic run."""
    rows: dict[int, dict] = collections.defaultdict(
        lambda: {"name": "", "interp": 0, "calls": 0, "kernel": 0}
    )
    for pc, n in diag.hist.items():
        entry, name = funcs.of(pc)
        rows[entry]["name"] = name
        rows[entry]["interp"] += n
    for run in diag.runs:
        entry, name = funcs.of(run["root"])
        rows[entry]["name"] = name
        rows[entry]["calls"] += run["calls"]
        rows[entry]["kernel"] += run["insns"]
    out = []
    for entry, r in rows.items():
        ns = (
            r["interp"] * costs["interp"]
            + r["calls"] * costs["call"]
            + r["kernel"] * costs["kernel"]
        )
        out.append({"entry": entry, **r, "ms_per_audio_s": ns / 1e6 / audio})
    out.sort(key=lambda r: -r["ms_per_audio_s"])
    return out


def cumulative(rows, key) -> str:
    """Share of the total of KEY in the top 10 / 20 / 50 rows (sorted by KEY)."""
    vals = sorted((r[key] for r in rows), reverse=True)
    total = sum(vals) or 1
    return (
        ", ".join(f"top {n}: {100 * sum(vals[:n]) / total:.1f}%" for n in (10, 20, 50))
        + f" (of {len(vals)} functions)"
    )


def region_rank(sc: dict, funcs) -> list[dict]:
    """Wall time per function from a `--profile-regions` run (fast and AOT tiers)."""
    rows: dict[int, dict] = collections.defaultdict(
        lambda: {"name": "", "fast_ns": 0, "aot_ns": 0, "insns": 0, "calls": 0}
    )
    for r in (sc.get("region_profile") or {}).get("rows", []):
        entry, name = funcs.of(r["entry_pc_u32"])
        e = rows[entry]
        e["name"] = name
        e["fast_ns" if r["tier"] == "fast" else "aot_ns"] += r["wall_ns"]
        e["insns"] += r["retired_instructions"]
        e["calls"] += r["calls"]
    out = [{"entry": k, **v, "ns": v["fast_ns"] + v["aot_ns"]} for k, v in rows.items()]
    out.sort(key=lambda r: -r["ns"])
    return out


def report_config(label, t, ranking, costs, top) -> str:
    audio = t["audio"]
    ex = max(t["executed"], 1)
    lines = [
        f"## {label}",
        "",
        f"- DSP CPU per audio s: **{t['cpu_ns'] / 1e9 / audio:.3f}**, PCM `{str(t['pcm'])[:16]}`, gate ok={t['ok']}",
        f"- instructions executed per audio s: {ex / audio / 1e6:.0f}M "
        f"(kernel {100 * t['kernel'] / ex:.1f}%, AOT {100 * t['aot'] / ex:.1f}%, "
        f"interpreter {100 * t['interp'] / ex:.2f}%; idle skipped {t['idle'] / audio / 1e6:.0f}M)",
        f"- region calls (engine round trips) per audio s: {t['call'] / audio / 1e6:.2f}M "
        f"({t['kernel'] / max(t['call'], 1):.1f} kernel insns per call)",
        f"- call path (residual): {max(t['cpu_ns'] - t['interp'] * costs['interp'] - t['kernel'] * costs['kernel'] - t['aot'] * costs['aot'], 0) / max(t['call'], 1):.0f} ns per region call "
        f"(CPU minus interpreted, kernel and AOT time, over all calls)",
        f"- model: {t['cpu_ns'] / 1e9 / audio:.3f} measured vs "
        f"{(t['interp'] * costs['interp'] + t['call'] * costs['call'] + t['kernel'] * costs['kernel'] + t['aot'] * costs['aot'] + t['audio'] * costs['audio']) / 1e9 / audio:.3f} "
        f"estimated CPU s per audio s (ns/insn interp {costs['interp']:.0f}, "
        f"ns/call {costs['call']:.0f}, ns/kernel insn {costs['kernel']:.2f}, ns/AOT insn {costs['aot']:.2f})",
        "",
        "| function | interp insns/s | calls/s | kernel insns/s | est. ms per audio s | share |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    total = sum(r["ms_per_audio_s"] for r in ranking) or 1.0
    if ranking:
        lines.insert(
            -2,
            "- cumulative share of executed insns (interpreted + kernel): "
            + cumulative([{**r, "x": r["interp"] + r["kernel"]} for r in ranking], "x"),
        )
        lines.insert(
            -2,
            "- cumulative share of the estimated cost: "
            + cumulative(ranking, "ms_per_audio_s"),
        )
    for r in ranking[:top]:
        lines.append(
            f"| {r['entry']:#x} {r['name']} | {r['interp'] / audio / 1e6:.2f}M | {r['calls'] / audio / 1e6:.3f}M "
            f"| {r['kernel'] / audio / 1e6:.1f}M | {r['ms_per_audio_s']:.0f} | {100 * r['ms_per_audio_s'] / total:.1f}% |"
        )
    return "\n".join(lines) + "\n"


def refusal_reasons(diag, funcs, rows) -> dict[str, str]:
    """Why the fast tier left each function interpreted (fastcov's reason text)."""
    if diag is None:
        return {}
    out = {}
    for r in rows:
        mine = [(n, pc) for pc, n in diag.hist.items() if funcs.of(pc)[0] == r["entry"]]
        pc = max(mine)[1] if mine else r["entry"]
        out[f"{r['entry']:#x}"] = (
            f"hottest interpreted pc {pc:#x}: {diag.reason_of(pc, funcs)}"
        )
    return out


def hybrid_entries(funcs, entries: list[int], image: str) -> list[int]:
    """Every static basic-block start (and the entry) inside the given functions."""
    db = funcs.db
    if db is None:
        raise SystemExit(f"perf: hybrid-top needs out/sharcdb/{image}.sqlite")
    pcs: set[int] = set()
    for entry in entries:
        row = db.execute(
            "SELECT end_sw FROM functions WHERE image=? AND entry_sw=?", (image, entry)
        ).fetchone()
        end = row[0] if row else entry + 1
        pcs.add(entry)
        for (pc,) in db.execute(
            "SELECT start_sw FROM bblocks WHERE image=? AND start_sw>=? AND start_sw<?",
            (image, entry, end),
        ):
            pcs.add(pc)
    return sorted(pcs)


def top_functions(binary, scenario, seconds, out, timeout, image, funcs, costs, n):
    """Entries of the N costliest functions: from a fast-only diagnostic run.

    The ranking (static-insns+follow, generated lowering, AOT off) is cached in
    out/native/perf/ranking-<image>-<seconds>.json and reused by later runs.
    """
    cache = OUT / f"ranking-{image}-{seconds}.json"
    if cache.is_file():
        ranking = json.loads(cache.read_text())
    else:
        _, env = fastcov.parse_config("static-insns+follow")
        diag = diag_run(binary, scenario, seconds, env, "ranking", out, timeout)
        ranking = rank(diag, funcs, costs, max(seconds, 1e-9))
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text(json.dumps(ranking))
    return [r["entry"] for r in ranking[:n]]


def hybrid_config(spec, binary, scenario, seconds, out, timeout, image, funcs, costs):
    """`hybrid-top:N`: the default config (AOT on) plus fast regions armed only at
    the static block starts of the top N functions by the cost ranking, with the
    generated lowering and helpers (the defaults) on."""
    n = int(spec.partition(":")[2])
    top = top_functions(binary, scenario, seconds, out, timeout, image, funcs, costs, n)
    pcs = hybrid_entries(funcs, top, image)
    path = out / f"hybrid-top{n}-entries.txt"
    path.write_text(",".join(f"{pc:#x}" for pc in pcs))
    print(f"  {spec}: {len(top)} functions, {len(pcs)} entries")
    return spec, {"SHARC_FAST_REGIONS": f"@{path}"}


def calibrate(binary, scenario, seconds, out, timeout, image) -> None:
    samples = []
    for spec in CALIBRATION_CONFIGS:
        label, env = fastcov.parse_config(spec)
        t = timed_run(binary, scenario, seconds, env, label, out, False, timeout)
        samples.append(t)
        print(
            f"  {label}: cpu {t['cpu_ns'] / 1e9 / t['audio']:.2f} s/audio s, "
            f"interp {t['interp'] / 1e6:.0f}M calls {t['call'] / 1e6:.1f}M kernel {t['kernel'] / 1e6:.0f}M aot {t['aot'] / 1e6:.0f}M"
        )
    costs = fit(samples)
    resid = []
    for s in samples:
        est = sum(costs[k] * s[k] for k in costs)
        resid.append(est / s["cpu_ns"])
    CALIB.parent.mkdir(parents=True, exist_ok=True)
    CALIB.write_text(
        json.dumps(
            {"costs": costs, "est_over_measured": resid, "seconds": seconds}, indent=1
        )
    )
    print("fitted ns:", {k: round(v, 2) for k, v in costs.items()})
    print("estimate / measured per run:", [round(x, 2) for x in resid])


def use_dt2(args) -> None:
    """Point the coupled-run arguments at the DT2 inputs (a card's ready state)."""
    from tools.emulator import digi_env

    syx = digi_env.firmware_for("dt2")
    base = digi_env.READY_DIRS["dt2"]
    ready = pathlib.Path(args.ready) if args.ready else base
    inputs = {
        "firmware": syx,
        "DSP image": base / "digi-audio-dn2-image.bin",
        "DSP state": ready / "digi-audio-m5.snap.dsp",
        "CF snapshot": ready / "digi-audio-m5.snap",
    }
    extra = ["--scenarios", str(args.scenarios or ROOT / "tests/scenarios/dt2.json")]
    if args.card_image:
        extra += ["--card-image", args.card_image]
    coupled.inputs = lambda: inputs
    original = coupled.run_args
    coupled.run_args = lambda binary, ins, ex: original(binary, ins, [*extra, *ex])
    args.scenario = [
        "dt2-sample-trig" if n == "trig1-taps" else n for n in args.scenario
    ]
    if args.image == "dn2-1.11":
        args.image = "dt2-1.16"
    fastcov.STATIC_IMAGE = args.image


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument(
        "--scenario",
        nargs="+",
        action="extend",
        default=None,
        metavar="NAME",
        help="scenario name(s), default trig1-taps; each is run and reported in turn",
    )
    ap.add_argument(
        "--scenarios",
        default=None,
        metavar="FILE",
        help="scenario file holding the names (default: the device's file)",
    )
    ap.add_argument(
        "--seconds",
        type=float,
        default=1.5,
        help="audio seconds per run; 0 runs each scenario for its own length",
    )
    ap.add_argument("--config", action="append", default=None, metavar="NAME")
    ap.add_argument(
        "--timed", action="store_true", help="take the exclusive benchmark lock"
    )
    ap.add_argument("--calibrate", action="store_true")
    ap.add_argument(
        "--no-diag", action="store_true", help="skip the per-function ranking run"
    )
    ap.add_argument(
        "--rank-seconds",
        type=float,
        default=3.0,
        help="length of the run that ranks functions for hybrid-top:N",
    )
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument("--image", default="dn2-1.11")
    ap.add_argument("--gen", choices=["shared", "prod"], default="shared")
    ap.add_argument(
        "--refresh-core", action="store_true", help="regenerate the shared core first"
    )
    ap.add_argument("--device", choices=device_files.shorts(), default="dn2")
    ap.add_argument(
        "--card-image",
        default=None,
        help="DT2 card image (see tools/emulator/dt2_sample_check.py)",
    )
    ap.add_argument(
        "--ready", default=None, help="DT2 ready dir holding the .snap and .snap.dsp"
    )
    ap.add_argument(
        "--regions",
        action="store_true",
        help="per-function wall time of the fast and AOT tiers (needs a build with "
        "DIGI_COUPLED_FEATURES=coupled-audio,region-profile)",
    )
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--timeout", type=float, default=2400)
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    args.scenario = args.scenario or ["trig1-taps"]
    if args.seconds <= 0:  # 0: each scenario's own length
        args.seconds = None

    # The shared core (`mise run gen:core`): current semantics, no firmware-derived
    # blocks, so no AOT configs; `--gen prod` keeps the prod generated core.
    if args.gen == "shared":
        core = ROOT / "out" / "native" / "shared-core" / "gen"
        if not (core / "core_g.rs").is_file() or args.refresh_core:
            import subprocess

            subprocess.run(
                [sys.executable, "-m", "tools.sharc_dev.sharc_transpile", "--strict", "--out", str(core),
                 "--work", str(core.parent / "work")],
                cwd=ROOT, env=python_env(), check=True, stdout=subprocess.DEVNULL,
            )  # fmt: skip
        digi_env.check_generated(core)
        os.environ["SHARC_GEN_DIR"] = str(core)
    if args.device == "dt2":
        use_dt2(args)
    elif args.scenarios:
        file = str(pathlib.Path(args.scenarios).resolve())
        base = coupled.run_args
        coupled.run_args = lambda binary, ins, ex: base(
            binary, ins, ["--scenarios", file, *ex]
        )
    if args.regions:
        wrapped = coupled.run_args
        coupled.run_args = lambda binary, ins, ex: wrapped(
            binary, ins, [*ex, "--profile-regions"]
        )
    binary = coupled.ensure_binary(args.no_build)
    stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%d-%H%M%S")
    # out/runs/perf/<stamp>/: report.md and perf.json here, the runs' own
    # reports and stderr in logs/.
    report_dir = pathlib.Path(args.out) if args.out else digi_env.run_dir("perf", stamp)
    out = report_dir / "logs"
    out.mkdir(parents=True, exist_ok=True)
    if args.calibrate:
        calibrate(binary, args.scenario[0], args.seconds, out, args.timeout, args.image)
        return 0
    costs = load_costs()
    funcs = fastcov.Functions(args.image)
    first = args.scenario[0]
    configs = [
        hybrid_config(
            c, binary, first, args.rank_seconds, out, args.timeout,
            args.image, funcs, costs,
        )
        if c.startswith("hybrid-top:")
        else fastcov.parse_config(c)
        for c in (args.config or ["static-blocks+follow"])
    ]  # fmt: skip
    md = [
        f"# DSP performance, {', '.join(args.scenario)}, {args.seconds} s, {'timed (locked)' if args.timed else 'unlocked'}",
        f"firmware {os.environ.get('DIGI_EMU_SYX', 'default')}, {stamp}",
        "",
    ]
    results = []
    for scenario in args.scenario:
        if len(args.scenario) > 1:
            md.append(f"# scenario {scenario}\n")
            out = report_dir / "logs" / scenario
            out.mkdir(parents=True, exist_ok=True)
        for label, env in configs:
            t = timed_run(
                binary,
                scenario,
                args.seconds,
                env,
                label,
                out,
                args.timed,
                args.timeout,
            )
            ranking = []
            if args.regions:
                sc = fastcov.scenario_report(pathlib.Path(t["report"]), scenario)
                rr = region_rank(sc, funcs)
                tot = sum(r["ns"] for r in rr) or 1
                md.append(f"### region profile (fast + AOT wall time) for {label}\n")
                md.append(
                    "cumulative share of that time: " + cumulative(rr, "ns") + "\n"
                )
                md.append(
                    "| function | fast ms/audio s | AOT ms/audio s | insns/s | calls/s | share |\n|---|---:|---:|---:|---:|---:|"
                )
                for r in rr[: args.top]:
                    md.append(
                        f"| {r['entry']:#x} {r['name']} | {r['fast_ns'] / 1e6 / t['audio']:.0f} | {r['aot_ns'] / 1e6 / t['audio']:.0f} "
                        f"| {r['insns'] / t['audio'] / 1e6:.1f}M | {r['calls'] / t['audio'] / 1e6:.3f}M | {100 * r['ns'] / tot:.1f}% |"
                    )
                md.append("")
            diag = None
            if not args.no_diag:
                diag = diag_run(
                    binary, scenario, args.seconds, env, label, out, args.timeout
                )
                ranking = rank(diag, funcs, costs, t["audio"])
            md.append(report_config(label, t, ranking, costs, args.top))
            results.append(
                {
                    "scenario": scenario,
                    "config": label,
                    "env": env,
                    "run": t,
                    "ranking": ranking[:50],
                    "reasons": refusal_reasons(diag, funcs, ranking[:20]),
                }
            )
    text = "\n".join(md)
    (report_dir / "report.md").write_text(text)
    (report_dir / "perf.json").write_text(json.dumps(results, indent=1))
    print(text)
    print(f"report: {report_dir / 'report.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
