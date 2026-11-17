#!/usr/bin/env python3
"""Build and run the coupled-run scenario matrix; write one report and a summary.

``coupled-run`` (packages/emulator/coupled) executes the desktop app's coupled run loop
headless. This tool builds it once per DSP configuration, runs every scenario
under the exclusive benchmark lock, repeats the runs, and writes
``out/native/runs/<timestamp>/{report.json,summary.md}`` plus the raw per-run
JSON in ``raw/``.

Configurations (each is a generated-core key plus run-time environment):

    prod          out/native/dn2-audio/gen, SHARC_FAST_REGIONS from the launcher's
                  per-firmware table (what the desktop app runs)
    generic       out/native/dn2-audio/gen-generic, interpreter only
    generic-auto  generic plus SHARC_FAST_AUTO=256,64 (run-time region selection)

The interpreter-only configurations cost about 25-55 CPU seconds per audio
second, so their defaults are short; the whole default matrix takes about 11
minutes. ``--pgo PROFDATA`` builds every configuration with that profile.

    uv run python -m tools.emulator.coupled_matrix                 # full default matrix
    uv run python -m tools.emulator.coupled_matrix --configs prod --scenarios idle --repeats 1
    uv run python -m tools.emulator.coupled_matrix --list
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime
import json
import os
import pathlib
import platform
import statistics
import subprocess
import time
from collections.abc import Callable

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools.devtools import benchlock  # noqa: E402
from tools.emulator import (  # noqa: E402
    digi_env,  # noqa: E402
    native_target,  # noqa: E402
)
from tools.firmware import config  # noqa: E402

TOOLCHAIN = "1.98.1"
TRIPLE = "aarch64-apple-darwin"
PGO_FLAGS = "-Cprofile-use={profdata}"
READY = digi_env.READY
SCENARIO_FILE = ROOT / "tests" / "scenarios" / "dn2.json"
RUNS_DIR = ROOT / "out" / "native" / "runs"
PROD_GEN = ROOT / "out" / "native" / "dn2-audio" / "gen"
GENERIC_GEN = ROOT / "out" / "native" / "dn2-audio" / "gen-generic"
DEFAULT_SYX = config.stock("Digitone_II_OS1.12.syx")
# Environment the runner reads; cleared first so a run depends on its config only.
CLEARED_ENV = (
    "SHARC_FAST_REGIONS",
    "SHARC_FAST_REGIONS_2",
    "SHARC_BLOCKTRANS_2",
    "SHARC_FAST_FALLBACK_REGIONS_2",
    "SHARC_FAST_AUTO",
    "SHARC_FAST_LOG",
    "SHARC_FAST_HIST",
    "DSP_IDLE_SKIP",
    "DN2_PROFILE_LINK",
    "DIGI_EMU_PCM_F32LE",
    "NOTE_EVENTS",
)
LOAD_WARN = 1.5  # 1-minute load average above this before a timed run: noisy


@dataclasses.dataclass(frozen=True)
class Config:
    name: str
    gen: pathlib.Path
    fast: str  # "launcher": the launcher's region table; "": none; else the list
    auto: str  # SHARC_FAST_AUTO value, "" for none
    seconds: float  # default audio seconds per scenario
    repeats: int  # default repeats per scenario


CONFIGS: dict[str, Config] = {
    c.name: c
    for c in (
        Config("prod", PROD_GEN, "launcher", "", 10.0, 3),
        Config("generic", GENERIC_GEN, "", "", 1.5, 1),
        Config("generic-auto", GENERIC_GEN, "", "256,64", 1.5, 1),
    )
}
SCENARIOS = ("idle", "trig1-hold", "kb-chord16")
# Short runs with a recorded PCM hash: on every config / on prod only.
VALIDATION_ALL = ("exactness-100m",)
VALIDATION_PROD = ("trig1-noqa-40m",)


def launcher_fast_regions(syx: pathlib.Path) -> str:
    """The launcher's verified fast regions for this firmware ("" if unknown)."""
    import runpy

    launcher = runpy.run_path(
        str(ROOT / "tools" / "emulator" / "native_emu.py"), run_name="launcher"
    )
    return str(launcher["fast_regions_for"](syx))


def config_env(
    config: Config, syx: pathlib.Path, base: dict[str, str] | None = None
) -> dict[str, str]:
    """The environment of a run: ``base`` minus the tuning knobs, plus the config's."""
    env = dict(os.environ if base is None else base)
    for name in CLEARED_ENV:
        env.pop(name, None)
    env["SHARC_GEN_DIR"] = str(config.gen)
    fast = launcher_fast_regions(syx) if config.fast == "launcher" else config.fast
    if fast:
        env["SHARC_FAST_REGIONS"] = fast
    if config.auto:
        env["SHARC_FAST_AUTO"] = config.auto
    # The core of a device reads the lists with its device's suffix (DT2 `_2`).
    return digi_env.slot_env(digi_env.device_for_syx(syx) or "dn2", env)


def target_dir(config: Config, pgo: str | None) -> pathlib.Path:
    return native_target.target_dir(config.gen, "pgo-use" if pgo else "")


def binary_path(config: Config, pgo: str | None) -> pathlib.Path:
    base = target_dir(config, pgo)
    # RUSTFLAGS builds use --target, so the binary sits one level deeper.
    return (base / TRIPLE if pgo else base) / "release" / "coupled-run"


def build_command(
    config: Config, pgo: str | None, profile_regions: bool = False
) -> list[str]:
    return [
        "rustup",
        "run",
        TOOLCHAIN,
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "digi-coupled",
        "--features",
        "sharc,play,region-profile" if profile_regions else "sharc,play",
        "--bin",
        "coupled-run",
        *(["--target", TRIPLE] if pgo else []),
        "--target-dir",
        str(target_dir(config, pgo)),
    ]


def build(
    config: Config, pgo: str | None, syx: pathlib.Path, profile_regions: bool = False
) -> pathlib.Path:
    env = config_env(config, syx)
    if pgo:
        env["RUSTFLAGS"] = PGO_FLAGS.format(profdata=pgo)
    start = time.monotonic()
    with benchlock.hold("shared", f"coupled_matrix build {config.name}"):
        proc = subprocess.run(
            build_command(config, pgo, profile_regions),
            cwd=ROOT,
            env=env,
            capture_output=True,
            text=True,
        )
    if proc.returncode:
        raise SystemExit(f"build of {config.name} failed:\n{proc.stderr[-3000:]}")
    path = binary_path(config, pgo)
    print(
        f"built {config.name} in {time.monotonic() - start:.0f} s: {path}", flush=True
    )
    return path


@dataclasses.dataclass(frozen=True)
class Job:
    config: str
    scenario: str
    repeat: int
    seconds: float | None  # None: the scenario's own end (validation)
    profile_regions: bool = False


def plan(
    configs: list[Config],
    scenarios: list[str],
    repeats: int | None,
    seconds: float | None,
    validate: bool,
    profile_regions: bool = False,
) -> list[Job]:
    """Jobs in run order: validation first, then repeats outermost so slow drift
    (thermal, other load) is spread over all configurations and scenarios."""
    jobs: list[Job] = []
    if validate:
        for c in configs:
            names = VALIDATION_ALL + (VALIDATION_PROD if c.name == "prod" else ())
            jobs += [Job(c.name, n, 0, None, profile_regions) for n in names]
    rounds = max((repeats or c.repeats) for c in configs) if configs else 0
    for r in range(1, rounds + 1):
        for c in configs:
            if r > (repeats or c.repeats):
                continue
            for s in scenarios:
                jobs.append(Job(c.name, s, r, seconds or c.seconds, profile_regions))
    return jobs


def runner_command(
    binary: pathlib.Path, job: Job, syx: pathlib.Path, out: pathlib.Path
) -> list[str]:
    cmd = [
        str(binary),
        "--syx",
        str(syx),
        "--dsp-image",
        str(READY / "digi-audio-dn2-image.bin"),
        "--dsp-state",
        str(READY / "digi-audio-m5.snap.dsp"),
        "--cf-snapshot",
        str(READY / "digi-audio-m5.snap"),
        "--scenarios",
        str(SCENARIO_FILE),
        "--scenario",
        job.scenario,
        "--out",
        str(out),
    ]
    if job.seconds is not None:
        cmd += ["--seconds", f"{job.seconds:g}"]
    if job.profile_regions:
        cmd += ["--profile-regions"]
    return cmd


def execute(
    binary: pathlib.Path,
    job: Job,
    syx: pathlib.Path,
    env: dict[str, str],
    raw_dir: pathlib.Path,
    timeout: float,
) -> dict:
    """One timed run under the exclusive lock; the scenario's report or an error."""
    out = raw_dir / f"{job.config}-{job.scenario}-{job.repeat}.json"
    label = f"coupled_matrix {job.config}/{job.scenario}#{job.repeat}"
    with benchlock.hold("exclusive", label) as held:
        load = os.getloadavg()[0]
        start = time.monotonic()
        try:
            proc = subprocess.run(
                runner_command(binary, job, syx, out),
                cwd=ROOT,
                env=env,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            return {"error": f"timed out after {timeout:.0f} s", "load1": load}
        elapsed = time.monotonic() - start
    info = {"load1": load, "lock_wait_s": round(held.waited, 1), "process_s": elapsed}
    try:
        report = json.loads(out.read_text())["scenarios"][0]
    except (OSError, ValueError, KeyError, IndexError):
        return {
            **info,
            "error": f"exit {proc.returncode}: {proc.stderr.strip()[-500:]}",
        }
    report["run"] = {**info, "exit_code": proc.returncode}
    return report


def median(values: list[float]) -> float:
    return statistics.median(values) if values else float("nan")


def metric(report: dict, path: str) -> float:
    cur: object = report
    for key in path.split("."):
        cur = cur.get(key) if isinstance(cur, dict) else None
    return float(cur) if isinstance(cur, (int, float)) else float("nan")


METRICS = {
    "audio_seconds": "audio_seconds",
    "wall_seconds": "wall_seconds",
    "real_time_factor": "real_time_factor",
    "dsp_cpu_per_audio_s": "cpu.dsp_thread_per_audio_s",
    "cf_cpu_per_audio_s": "cpu.cf_thread_per_audio_s",
    "over_deadline_pct": None,  # derived
    "period_p50_ms": "dsp_period_wall.p50_ms",
    "period_p99_ms": "dsp_period_wall.p99_ms",
    "period_max_ms": "dsp_period_wall.max_ms",
    "dsp_idle_skip_pct": "dsp_instructions.idle_skip_pct",
    "dsp_interpreter_pct": "dsp_instructions.interpreter_pct",
    "dsp_aot_pct": "dsp_instructions.aot_block_pct",
    "dsp_fast_pct": "dsp_instructions.fast_tier_pct",
    "cf_interpreted_pct": "cf_instructions.interpreted_pct",
    "cf_idle_ff_pct": "cf_instructions.idle_fast_forward_pct",
    "pcm_rms": "pcm.rms",
    "pcm_peak": "pcm.peak",
}


def metrics_of(report: dict) -> dict[str, float]:
    out = {}
    for name, path in METRICS.items():
        if path is None:
            out[name] = 100.0 * metric(report, "dsp_period_wall.over_deadline_fraction")
        else:
            out[name] = metric(report, path)
    return out


def aggregate(results: list[dict]) -> dict[str, dict]:
    """Per ``config/scenario``: medians over the repeats and the gate verdicts."""
    groups: dict[str, list[dict]] = {}
    for item in results:
        groups.setdefault(f"{item['config']}/{item['scenario']}", []).append(item)
    out: dict[str, dict] = {}
    for key, items in groups.items():
        good = [i for i in items if "report" in i]
        reports = [i["report"] for i in good]
        per = [metrics_of(r) for r in reports]
        walls = [m["wall_seconds"] for m in per]
        out[key] = {
            "runs": len(items),
            "failed": len(items) - len(good),
            "median": {name: median([m[name] for m in per]) for name in METRICS},
            "wall_seconds_all": walls,
            "dsp_cpu_per_audio_s_all": [m["dsp_cpu_per_audio_s"] for m in per],
            "pcm_sha256": sorted({r["pcm"]["sha256"] for r in reports}),
            "gates_ok": all(r["gates"]["ok"] for r in reports) if reports else False,
            "errors": [i["error"] for i in items if "error" in i],
        }
    return out


def fmt(value: float, digits: int = 2) -> str:
    return "-" if value != value else f"{value:.{digits}f}"


def render_summary(report: dict) -> str:
    """The markdown summary of a matrix report (``report.json``'s content)."""
    meta = report["meta"]
    agg = report["aggregate"]
    lines = [
        f"# coupled-run matrix {meta['timestamp']}",
        "",
        f"git {meta['git']}; {meta['host']}; rustc {meta['rustc']}; "
        f"pgo {meta['pgo'] or 'off'}; total {meta['total_seconds']:.0f} s.",
        "",
        "Medians over the repeats. `dsp cpu/s` and `cf cpu/s` are exact thread CPU "
        "seconds (CLOCK_THREAD_CPUTIME_ID) per audio second; real time needs "
        "`rtf` >= 1. A period is over the deadline when the DSP's wall time for it "
        "(SPI + DSP + SPORT) exceeds 0.667 ms.",
        "",
        "| config | scenario | n | audio s | wall s | rtf | dsp cpu/s | cf cpu/s "
        "| periods over | p50 / p99 / max ms | dsp idle/interp/aot/fast % "
        "| cf interp/idle % | pcm rms | gates |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for key, a in agg.items():
        config, scenario = key.split("/", 1)
        if scenario in VALIDATION_ALL + VALIDATION_PROD:
            continue
        m = a["median"]
        lines.append(
            f"| {config} | {scenario} | {a['runs'] - a['failed']}/{a['runs']} "
            f"| {fmt(m['audio_seconds'], 1)} | {fmt(m['wall_seconds'], 1)} "
            f"| {fmt(m['real_time_factor'])} | {fmt(m['dsp_cpu_per_audio_s'])} "
            f"| {fmt(m['cf_cpu_per_audio_s'])} | {fmt(m['over_deadline_pct'], 1)}% "
            f"| {fmt(m['period_p50_ms'])} / {fmt(m['period_p99_ms'])} / "
            f"{fmt(m['period_max_ms'], 1)} "
            f"| {fmt(m['dsp_idle_skip_pct'], 1)} / {fmt(m['dsp_interpreter_pct'], 1)} / "
            f"{fmt(m['dsp_aot_pct'], 1)} / {fmt(m['dsp_fast_pct'], 1)} "
            f"| {fmt(m['cf_interpreted_pct'], 1)} / {fmt(m['cf_idle_ff_pct'], 1)} "
            f"| {fmt(m['pcm_rms'], 4)} | {'ok' if a['gates_ok'] else 'FAIL'} |"
        )
    lines += ["", "## Validation (recorded PCM hashes)", ""]
    lines += ["| config | scenario | hash | gates |", "|---|---|---|---|"]
    for key, a in agg.items():
        config, scenario = key.split("/", 1)
        if scenario in VALIDATION_ALL + VALIDATION_PROD:
            hashes = ", ".join(h[:12] for h in a["pcm_sha256"]) or "-"
            lines.append(
                f"| {config} | {scenario} | {hashes} "
                f"| {'ok' if a['gates_ok'] else 'FAIL'} |"
            )
    spread = [
        f"- {key}: wall {', '.join(fmt(w, 1) for w in a['wall_seconds_all'])} s; "
        f"dsp cpu/s {', '.join(fmt(c) for c in a['dsp_cpu_per_audio_s_all'])}"
        for key, a in agg.items()
        if len(a["wall_seconds_all"]) > 1
    ]
    if spread:
        lines += ["", "## Repeat spread", "", *spread]
    notes = [
        f"- {r['config']}/{r['scenario']}#{r['repeat']}: load1 {r['load1']:.1f} "
        "before the run (other work may have perturbed it)"
        for r in report["runs"]
        if (r.get("load1") or 0) > LOAD_WARN
    ] + [f"- {k}: {e}" for k, a in agg.items() for e in a["errors"]]
    if notes:
        lines += ["", "## Notes", "", *notes]
    return "\n".join(lines) + "\n"


def git_state() -> str:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=ROOT, capture_output=True, text=True
        ).stdout.strip()

    dirty = bool(git("status", "--porcelain", "--untracked-files=no"))
    return git("rev-parse", "--short", "HEAD") + ("+dirty" if dirty else "")


def host_description() -> str:
    brand = subprocess.run(
        ["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True
    ).stdout.strip()
    return f"{brand or platform.processor() or platform.machine()}, {os.cpu_count()} cpus, {platform.platform()}"


def rustc_version() -> str:
    return subprocess.run(
        ["rustup", "run", TOOLCHAIN, "rustc", "--version"],
        capture_output=True,
        text=True,
    ).stdout.strip()


def run_matrix(
    configs: list[Config],
    scenarios: list[str],
    repeats: int | None,
    seconds: float | None,
    validate: bool,
    pgo: str | None,
    syx: pathlib.Path,
    out_dir: pathlib.Path,
    timeout: float,
    builder: Callable[[Config, str | None, pathlib.Path, bool], pathlib.Path] = build,
    runner: Callable[..., dict] = execute,
    profile_regions: bool = False,
) -> dict:
    """Build, run every job, write ``report.json`` and ``summary.md`` in out_dir."""
    started = time.monotonic()
    raw = out_dir / "raw"
    raw.mkdir(parents=True, exist_ok=True)
    binaries = {c.name: builder(c, pgo, syx, profile_regions) for c in configs}
    envs = {c.name: config_env(c, syx) for c in configs}
    jobs = plan(configs, scenarios, repeats, seconds, validate, profile_regions)
    results: list[dict] = []
    for index, job in enumerate(jobs, 1):
        print(
            f"[{index}/{len(jobs)}] {job.config} {job.scenario} #{job.repeat}",
            flush=True,
        )
        got = runner(binaries[job.config], job, syx, envs[job.config], raw, timeout)
        item = {"config": job.config, "scenario": job.scenario, "repeat": job.repeat}
        item["load1"] = got.get("load1")
        if "error" in got:
            item["error"] = got["error"]
        else:
            item["report"] = got
        results.append(item)
        report_line = got.get("pcm") and (
            f"  rtf {got['real_time_factor']:.2f} dsp {got['cpu']['dsp_thread_per_audio_s']:.2f} "
            f"gates {'ok' if got['gates']['ok'] else 'FAIL'}"
        )
        print(report_line or f"  {got.get('error', 'no pcm tap')}", flush=True)
    report = {
        "meta": {
            "timestamp": out_dir.name,
            "git": git_state(),
            "host": host_description(),
            "rustc": rustc_version(),
            "pgo": pgo,
            "profile_regions": profile_regions,
            "syx": str(syx),
            "total_seconds": time.monotonic() - started,
            "configs": {
                c.name: {
                    "gen": str(c.gen),
                    "binary": str(binaries[c.name]),
                    "env": {
                        k: envs[c.name][k]
                        for k in ("SHARC_FAST_REGIONS", "SHARC_FAST_AUTO")
                        if k in envs[c.name]
                    },
                    "seconds": seconds or c.seconds,
                    "repeats": repeats or c.repeats,
                }
                for c in configs
            },
        },
        "runs": results,
    }
    report["aggregate"] = aggregate(results)
    (out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out_dir / "summary.md").write_text(render_summary(report))
    return report


def scenario_names() -> list[str]:
    """Every scenario the runner's scenario file defines."""
    return [s["name"] for s in json.loads(SCENARIO_FILE.read_text())["scenarios"]]


def parse_list(text: str, allowed: tuple[str, ...] | list[str], what: str) -> list[str]:
    names = [n.strip() for n in text.split(",") if n.strip()]
    bad = [n for n in names if n not in allowed]
    if bad:
        raise SystemExit(f"unknown {what}: {', '.join(bad)}")
    return names


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--configs", default=",".join(CONFIGS), help="comma list")
    ap.add_argument("--scenarios", default=",".join(SCENARIOS), help="comma list")
    ap.add_argument("--repeats", type=int, help="override every config's repeats")
    ap.add_argument(
        "--seconds", type=float, help="override every config's audio seconds"
    )
    ap.add_argument("--pgo", metavar="PROFDATA", help="build with this profile")
    ap.add_argument(
        "--profile-regions",
        action="store_true",
        help="pass --profile-regions to coupled-run (adds dispatch timing overhead)",
    )
    ap.add_argument("--no-validate", action="store_true", help="skip the hash checks")
    ap.add_argument("--syx", type=pathlib.Path, default=DEFAULT_SYX)
    ap.add_argument("--out-dir", type=pathlib.Path)
    ap.add_argument("--timeout", type=float, default=1800, help="per run, seconds")
    ap.add_argument(
        "--list", action="store_true", help="print the planned jobs and exit"
    )
    args = ap.parse_args(argv)

    configs = [CONFIGS[n] for n in parse_list(args.configs, tuple(CONFIGS), "config")]
    scenarios = parse_list(args.scenarios, scenario_names(), "scenario")
    jobs = plan(
        configs,
        scenarios,
        args.repeats,
        args.seconds,
        not args.no_validate,
        args.profile_regions,
    )
    if args.list:
        for job in jobs:
            print(job)
        return 0
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = args.out_dir or RUNS_DIR / stamp
    if out_dir.exists() and any(out_dir.iterdir()):
        raise SystemExit(f"{out_dir} is not empty")
    report = run_matrix(
        configs,
        scenarios,
        args.repeats,
        args.seconds,
        not args.no_validate,
        str(pathlib.Path(args.pgo).resolve()) if args.pgo else None,
        args.syx,
        out_dir,
        args.timeout,
        profile_regions=args.profile_regions,
    )
    print((out_dir / "summary.md").read_text())
    print(f"wrote {out_dir}/report.json and summary.md")
    failed = [k for k, a in report["aggregate"].items() if not a["gates_ok"]]
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
