#!/usr/bin/env python3
"""Where the fast tier stops: DSP CPU, the tier split and what the interpreter runs.

Runs a coupled scenario (packages/emulator/coupled, `coupled-run`) under one or more
tier configurations and reports, for each:

- DSP thread CPU per audio second, the PCM SHA-256 (a changed hash means the
  configuration is not exact), and the instruction split (fast tier / AOT
  blocks / interpreter / idle skip);
- with a diagnostic run (SHARC_FAST_LOG=summary, SHARC_FAST_COV, SHARC_FAST_HIST;
  not timed): the interpreted instructions ranked by the reason no fast region
  covered them (a refused root, an instruction a region stopped at, a lowering
  denial), by function (the address map: `tools.devtools.addrnames`) and the instructions
  that ran between a kernel exit and the next kernel call, by exit PC.

    uv run python -m tools.devtools.fastcov [--scenario trig1-taps] [--seconds S]
        [--config NAME ...] [--diag yes|no|only] [--timeout S] [--top N]
        [--image dn2-1.12] [--no-build] [--out DIR]
    mise run perf:cov -- --config default --config fast-auto:256,64

Configurations (``--config``, repeatable; default ``default fast-auto``):

    default             the shipped tiers: AOT blocks plus the verified fast regions
    fast-verified       SHARC_AOT=0, only the verified regions and cluster roots
    fast-auto[:T,M[,1]] SHARC_AOT=0, no verified regions, SHARC_FAST_AUTO=T,M[,blocks]
                        (fast tier only: the firmware-free case; default 256,64)
    <config>+flat       SHARC_FAST_FLATAUTO=1 over any config (the default now; explicit)
    env:SHARC_FAST_FLATAUTO=0   turns the flat fallback off for an A/B
    env:SHARC_FAST_DYNFLAT=0    turns the dynamic-window flat rebuild off (on by default)
    env:SHARC_FAST_CL_OPT=speed Cranelift opt level of the fast tier back to `speed` (default `none`)
    static-funcs[:IMAGE] / static-blocks[:IMAGE]   SHARC_AOT=0, region entries = every function
                        entry / basic-block start of the program database, chosen at load
    fast-auto+verified[:T,M[,1]]   the same plus the verified regions
    aot-auto[:T,M[,1]]  AOT plus the verified regions plus run-time selection
    env:K=V[;K=V...]    anything else (applied over the shipped defaults)

SHARC_AOT=0 turns off the generated blocks only; the fast tier keeps running
(packages/emulator/sharc/src/lib.rs block_plan). Timed runs take the exclusive benchmark
lock (tools/devtools/benchlock.py). The firmware comes from DIGI_EMU_SYX (default: the
repo-root Digitone II OS 1.12 .syx); the build is the coupled-run build of
tools/emulator/coupled.py (the prod generated-core target dir of this tree).
"""

from __future__ import annotations

import argparse
import bisect
import collections
import datetime
import json
import os
import pathlib
import re
import sqlite3
import subprocess
import sys
from collections.abc import Iterable

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
from tools.devtools import benchlock  # noqa: E402
from tools.devtools.addrnames import AddrNames, AddrNamesError  # noqa: E402
from tools.emulator import coupled, digi_env  # noqa: E402

OUT_DIR = ROOT / "out" / "native" / "fastcov"
DB_DIR = ROOT / "out" / "sharcdb"
# The `fastcov` diagnostic of tools/emulator/digi_env.py (stderr only, no files).
DIAG_ENV = digi_env.DIAG["fastcov"][2]("", pathlib.Path())
NO_LOCK = False
NO_FAST = {"SHARC_FAST_REGIONS": "", "SHARC_BLOCKTRANS": ""}


STATIC_IMAGE = "dn2-1.12"


def static_entries(what: str, image: str) -> list[int]:
    """Function entries ("funcs") or basic-block starts ("blocks") of IMAGE."""
    db = sqlite3.connect(f"file:{DB_DIR / (image + '.sqlite')}?mode=ro", uri=True)
    if what == "insns":
        rows = db.execute("SELECT sw FROM insn WHERE image=? AND aligned=1", (image,))
    elif what == "funcs":
        rows = db.execute("SELECT entry_sw FROM functions WHERE image=?", (image,))
    else:
        rows = db.execute(
            "SELECT start_sw FROM bblocks WHERE image=? UNION "
            "SELECT entry_sw FROM functions WHERE image=?",
            (image, image),
        )
    return sorted(r[0] for r in rows)


def write_entries(what: str, image: str) -> str:
    """The entry list as SHARC_FAST_REGIONS: inline when short, else @FILE."""
    text = ",".join(f"{pc:#x}" for pc in static_entries(what, image))
    if len(text) < 60000:
        return text
    path = OUT_DIR / f"entries-{image}-{what}.txt"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return f"@{path}"


def parse_config(spec: str) -> tuple[str, dict[str, str]]:
    """(label, env overrides) for a --config value."""
    if spec.endswith("+aot"):
        # The generated AOT blocks stay on (the prod generated core): the
        # configuration's fast regions take their entries, AOT runs elsewhere.
        _, env = parse_config(spec.removesuffix("+aot"))
        env = {k: v for k, v in env.items() if k != "SHARC_AOT"}
        return spec, env
    if "+env:" in spec:
        # `<config>+env:KEY=VALUE`: one more environment variable (repeatable).
        head, tail = spec.rsplit("+env:", 1)
        _, env = parse_config(head)
        key, _, value = tail.partition("=")
        return spec, {**env, key: value}
    if spec.endswith("+gen"):
        # The generated lowering of tools/sharc_dev/sharc_lowergen (lane F) for every form it has.
        _, env = parse_config(spec.removesuffix("+gen"))
        return spec, {**env, "SHARC_FAST_GEN": "all"}
    if spec.endswith("+flat"):
        # Accesses with no address range, and regions whose static windows keep
        # declining, are lowered as flat guarded accesses (cfg::flat_auto).
        _, env = parse_config(spec.removesuffix("+flat"))
        return spec, {**env, "SHARC_FAST_FLATAUTO": "1"}
    if spec.endswith("+follow"):
        # Also make the instruction after each stop instruction an entry.
        _, env = parse_config(spec.removesuffix("+follow"))
        return spec, {**env, "SHARC_FAST_FOLLOW": "1"}
    kind, _, arg = spec.partition(":")
    if kind == "env":
        env = dict(kv.split("=", 1) for kv in arg.split(";") if kv)
        return spec, env
    auto = arg or "256,64"
    if kind == "default":
        return spec, {}
    if kind == "fast-verified":
        return spec, {"SHARC_AOT": "0"}
    if kind == "fast-auto":
        return spec, {"SHARC_AOT": "0", **NO_FAST, "SHARC_FAST_AUTO": auto}
    if kind in ("cluster-blocks", "cluster-insns"):
        # The same entries built as block-translator clusters (guarded dynamic
        # windows, call/return sequences): SHARC_BLOCKTRANS instead of REGIONS.
        entries = write_entries(kind.split("-")[1], arg or STATIC_IMAGE)
        return spec, {
            "SHARC_AOT": "0",
            "SHARC_BLOCKTRANS": entries,
            "SHARC_FAST_REGIONS": "",
        }
    if kind in ("static-funcs", "static-blocks", "static-insns"):
        # Region entries chosen at load from the program database: function
        # entries, or every basic-block start. No run-time selection or drop.
        return spec, {
            "SHARC_AOT": "0",
            "SHARC_BLOCKTRANS": "",
            "SHARC_FAST_REGIONS": ",".join(
                f"{pc:#x}"
                for pc in static_entries(kind.split("-")[1], arg or STATIC_IMAGE)
            ),
        }
    if kind == "fast-auto+verified":
        return spec, {"SHARC_AOT": "0", "SHARC_FAST_AUTO": auto}
    if kind == "aot-auto":
        return spec, {"SHARC_FAST_AUTO": auto}
    raise SystemExit(f"fastcov: unknown config {spec!r} (see --help)")


def norm(text: str) -> str:
    """A reason with its numbers and addresses folded (so counts group)."""
    text = re.sub(r"0x[0-9a-fA-F]+", "#", text)
    text = re.sub(r"\b\d+\b", "#", text)
    return text.strip()[:110]


class Diag:
    """What one diagnostic run's stderr says."""

    def __init__(self, text: str):
        self.hist: dict[int, int] = {}
        self.hist_total = 0
        self.refused: dict[int, str] = {}
        self.deny: dict[int, str] = {}  # at -> "form: why"
        self.stop: dict[int, str] = {}
        self.runs: list[dict] = []
        self.marks: list[tuple[int, str]] = []
        self.tier_line = ""
        self.engine_lines: list[str] = []
        for line in text.splitlines():
            if line.startswith("hist: 0x"):
                _, pc, n = line.split()
                self.hist[int(pc, 16)] = int(n)
            elif line.startswith("hist: "):
                self.hist_total = int(line.split()[1])
            elif line.startswith("cov refused "):
                m = re.match(r"cov refused root=(0x[0-9a-f]+) why=(.*)", line)
                if m:
                    self.refused[int(m[1], 16)] = m[2]
            elif line.startswith(("cov deny ", "cov stop ")):
                m = re.match(
                    r"cov (deny|stop) root=0x[0-9a-f]+ at=(0x[0-9a-f]+) form=(\S+) why=(.*)",
                    line,
                )
                if m:
                    dest = self.deny if m[1] == "deny" else self.stop
                    dest[int(m[2], 16)] = f"{m[3]}: {m[4]}"
            elif line.startswith("cov run "):
                self.runs.append(self.parse_run(line))
            elif line.startswith("fast tier ["):
                self.tier_line = line
            elif line.startswith("fast engine:"):
                self.engine_lines.append(line)

    @staticmethod
    def parse_run(line: str) -> dict:
        fields = dict(re.findall(r"(\w+)=(\S+)", line))
        exits = []
        for item in (fields.get("exits") or "").split(";"):
            parts = item.split(":")
            if len(parts) == 5:
                exits.append(
                    (
                        int(parts[0], 16),
                        parts[1],
                        int(parts[2]),
                        int(parts[3]),
                        int(parts[4]),
                    )
                )
        return {
            "root": int(fields["root"], 16),
            "calls": int(fields.get("calls", 0)),
            "insns": int(fields.get("insns", 0)),
            "exits": exits,
        }

    def reason_of(self, pc: int, funcs: Functions | None = None) -> str:
        if pc in self.deny:
            return "deny " + norm(self.deny[pc])
        if pc in self.stop:
            return "stop " + norm(self.stop[pc])
        if pc in self.refused:
            return "refused " + norm(self.refused[pc])
        if funcs is not None:
            # After the nearest earlier stop/denial/refusal in the same function:
            # the code a region could not cover, run by the interpreter up to
            # the end of the function unless another region starts later.
            if not self.marks:
                for table, kind in (
                    (self.deny, "deny"),
                    (self.stop, "stop"),
                    (self.refused, "refused"),
                ):
                    for at, why in table.items():
                        self.marks.append((at, f"{kind} {norm(why)}"))
                self.marks.sort()
            entry = funcs.of(pc)[0]
            i = bisect.bisect_right(self.marks, (pc, "\U0010ffff")) - 1
            if i >= 0 and self.marks[i][0] >= entry:
                return "after " + self.marks[i][1]
        return "no stop recorded in the function (cold, dropped or never selected)"


class Functions:
    """PC -> function through the address map (the firmware names of the SHARC program database), if there is one."""

    def __init__(self, image: str):
        self.image = image
        try:
            self.names: AddrNames | None = AddrNames.for_stock(image)
        except AddrNamesError as e:
            print(f"fastcov: no function names ({e})", file=sys.stderr)
            self.names = None
        self.found: dict[int, tuple[int, str]] = {}

    def prefetch(self, pcs: Iterable[int]) -> None:
        """Name many PCs with one lookup process (`of` asks for one PC at a time)."""
        if self.names is None:
            return
        todo = [pc for pc in dict.fromkeys(pcs) if pc not in self.found]
        for pc, hit in zip(todo, self.names.resolve(todo, "sharc"), strict=True):
            self.found[pc] = (
                (hit["sym_addr"], hit["name"]) if hit["name"] else (pc, "?")
            )

    def of(self, pc: int) -> tuple[int, str]:
        if self.names is None:
            return (pc, "?")
        if pc not in self.found:
            self.prefetch([pc])
        return self.found[pc]


def run_once(binary, scenario, seconds, env, label, timed, out, timeout):
    """One coupled-run: (report JSON path, stderr path, return code)."""
    safe = re.sub(r"[^A-Za-z0-9_.+-]+", "_", label)
    tag = "timed" if timed else "diag"
    report = out / f"{safe}.{tag}.json"
    errfile = out / f"{safe}.{tag}.err"
    extra = ["--scenario", scenario, "--out", str(report)]
    if seconds is not None:
        extra += ["--seconds", str(seconds)]
    argv = coupled.run_args(binary, coupled.inputs(), extra)
    report.unlink(missing_ok=True)
    mode = "exclusive" if timed and not NO_LOCK else "shared"
    with benchlock.hold(mode, f"fastcov {label} {tag}"), errfile.open("wb") as err:
        proc = subprocess.Popen(
            argv,
            cwd=ROOT,
            env=coupled.run_env(env),
            stdout=subprocess.DEVNULL,
            stderr=err,
        )
        waited = 0.0
        while True:
            try:
                code = proc.wait(timeout=2)
                break
            except subprocess.TimeoutExpired:
                waited += 2
            # A panicking DSP thread leaves the ColdFire side spinning forever.
            if b"panicked at" in errfile.read_bytes():
                proc.kill()
                proc.wait()
                code = -6
                break
            if waited > timeout:
                proc.kill()
                proc.wait()
                code = -9
                break
    return report, errfile, code


def scenario_report(report: pathlib.Path, scenario: str) -> dict | None:
    if not report.is_file():
        return None
    data = json.loads(report.read_text())
    for sc in data.get("scenarios", []):
        if sc.get("name") == scenario:
            return sc
    return None


def fmt_split(d: dict) -> str:
    total = d.get("instructions", 0)
    active = total - d.get("idle_skip", 0)

    def pct(n: int) -> float:
        return 100.0 * n / max(active, 1)

    return (
        f"insns {total / 1e9:.2f}G (idle skip {d.get('idle_skip', 0) / 1e9:.2f}G, "
        f"executed {active / 1e9:.2f}G): fast {pct(d.get('fast_tier', 0)):.1f}% "
        f"aot {pct(d.get('aot_block', 0)):.1f}% interp {pct(d.get('interpreter', 0)):.2f}% "
        f"other {d.get('other', 0)}; fast calls {d.get('fast_calls', 0)} "
        f"built {d.get('fast_built_total', 0)} refused {d.get('fast_refused_total', 0)}"
    )


def print_diag(diag: Diag, funcs: Functions, top: int) -> None:
    lost = sum(diag.hist.values())
    total = diag.hist_total or lost
    print(f"  interpreter: {total} insns over the run; the top-PC table covers {lost}")
    if diag.tier_line:
        print("  " + diag.tier_line[:2000])
    for line in diag.engine_lines:
        print("  " + line)
    funcs.prefetch([*diag.hist, *(at for run in diag.runs for at, *_ in run["exits"])])
    by_reason: collections.Counter[str] = collections.Counter()
    by_func: collections.Counter[tuple[int, str]] = collections.Counter()
    for pc, n in diag.hist.items():
        by_reason[diag.reason_of(pc, funcs)] += n
        by_func[funcs.of(pc)] += n
    print(f"  interpreted insns by reason (top {top}):")
    for reason, n in by_reason.most_common(top):
        print(f"    {n:>12} {100 * n / max(total, 1):5.1f}%  {reason}")
    print(f"  interpreted insns by function (top {top}):")
    for (entry, name), n in by_func.most_common(top):
        print(f"    {n:>12} {100 * n / max(total, 1):5.1f}%  {entry:#x} {name}")
    gaps: collections.Counter[tuple[int, str]] = collections.Counter()
    exits: collections.Counter[tuple[int, str]] = collections.Counter()
    for run in diag.runs:
        for at, kind, count, _retired, gap in run["exits"]:
            gaps[(at, kind)] += gap
            exits[(at, kind)] += count
    print(
        "  insns between a kernel exit and the next kernel call, by exit "
        f"(capped at 4096 each; total {sum(gaps.values())}), top {top}:"
    )
    for (at, kind), g in gaps.most_common(top):
        why = diag.stop.get(at) or diag.deny.get(at) or ""
        print(
            f"    {g:>12} {exits[(at, kind)]:>10} exits  {at:#x} {kind:<6} "
            f"{funcs.of(at)[1]}  {norm(why)}"
        )
    print(f"  kernel exits by count (top {top}); total {sum(exits.values())}:")
    for (at, kind), n in exits.most_common(top):
        why = diag.stop.get(at) or diag.deny.get(at) or ""
        print(f"    {n:>12} {at:#x} {kind:<6} {funcs.of(at)[1]}  {norm(why)}")
    hottest = sorted(diag.hist.items(), key=lambda kv: -kv[1])[:top]
    print(
        "  hottest interpreted PCs: " + ", ".join(f"{pc:#x}:{n}" for pc, n in hottest)
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--scenario", default="trig1-taps")
    ap.add_argument("--seconds", type=float, default=None)
    ap.add_argument("--config", action="append", default=None, metavar="NAME")
    ap.add_argument("--diag", choices=["yes", "no", "only"], default="yes")
    ap.add_argument("--timeout", type=float, default=2400, help="per run, seconds")
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument(
        "--image", default="dn2-1.12", help="sharcdb image for function names"
    )
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument(
        "--no-lock",
        action="store_true",
        help="timed runs share the lock (coverage runs)",
    )
    ap.add_argument(
        "--out",
        default=None,
        help="output directory (default out/runs/fastcov/<stamp>/)",
    )
    args = ap.parse_args()
    global NO_LOCK
    NO_LOCK = args.no_lock

    configs = [parse_config(c) for c in (args.config or ["default", "fast-auto"])]
    binary = coupled.ensure_binary(args.no_build)
    stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%d-%H%M%S")
    out = pathlib.Path(args.out) if args.out else digi_env.run_dir("fastcov", stamp)
    out.mkdir(parents=True, exist_ok=True)
    funcs = Functions(args.image)
    syx = os.environ.get("DIGI_EMU_SYX", "default")
    print(f"fastcov: {args.scenario} firmware {syx} -> {out}")
    summary = []
    for label, env in configs:
        print(f"== {label}  env {str(env or '(shipped)')[:160]}")
        row: dict = {"config": label, "env": env}
        if args.diag != "only":
            report, errfile, code = run_once(
                binary, args.scenario, args.seconds, env, label, True, out, args.timeout
            )
            sc = scenario_report(report, args.scenario)
            if sc is None:
                print(f"  timed run failed (exit {code}); see {errfile}")
            else:
                cpu = sc["cpu"]["dsp_thread_per_audio_s"]
                sha = (sc.get("pcm") or {}).get("sha256")
                gates_ok = (sc.get("gates") or {}).get("ok")
                print(
                    f"  dsp cpu/audio s {cpu:.3f}  rtf {sc.get('real_time_factor', 0):.2f}  "
                    f"pcm {str(sha)[:16]}  ok={gates_ok}"
                )
                print("  " + fmt_split(sc.get("dsp_instructions") or {}))
                row.update(
                    dsp_cpu_per_audio_s=cpu, pcm=sha, split=sc.get("dsp_instructions")
                )
        if args.diag != "no":
            report, errfile, code = run_once(
                binary,
                args.scenario,
                args.seconds,
                {**env, **DIAG_ENV},
                label,
                False,
                out,
                args.timeout,
            )
            print(f"  diagnostic run (not timed): exit {code}, stderr {errfile}")
            print_diag(Diag(errfile.read_text(errors="replace")), funcs, args.top)
        summary.append(row)
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    print(f"fastcov: summary {out / 'summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
