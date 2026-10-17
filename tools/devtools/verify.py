"""One command for the exactness gate, scaled to what changed.

    uv run python -m tools.devtools.verify                  # steps for the uncommitted changes
    uv run python -m tools.devtools.verify --base BRANCH    # a lane: its commits since BRANCH plus uncommitted changes
    uv run python -m tools.devtools.verify --full           # every step, with the full 134-scenario tier sweep
    uv run python -m tools.devtools.verify --full --dt2     # plus the full DT2 FX tier sweep
    uv run python -m tools.devtools.verify --only pins,check
    uv run python -m tools.devtools.verify --rerun-failed   # only the steps that failed in the last run
    uv run python -m tools.devtools.verify --jobs 1        # sequential (default: half the CPUs in parallel)
    uv run python -m tools.devtools.verify --explain        # show the routing and the plan, run nothing

Scenario renders go through the content-addressed render cache (tools/devtools/render_cache.py). Each
cache hit is also re-rendered from scratch with probability DIGI_RENDER_CACHE_CHECK (verify
default 0.03; 0 = off) and compared by PCM sha256 and peeks; a mismatch fails the step as a
stale cache. verify.py sets the variable for all steps once, in main(). --no-cache sets
DIGI_RENDER_CACHE=0 so everything is rendered (the release/publication gate).

python-tests runs one pytest process per file, in parallel (DIGI_PY_JOBS, default
max(2, cpus // 4); DIGI_PY_TIMEOUT per file in seconds, default 3600), through tools/devtools/test_outcomes.py; a run that passed no test fails. The
tier steps cut their (default/no-aot x file) groups into small chunks of scenarios and run
them from one shared queue, longest first, on DIGI_TIER_JOBS workers (default cpus - 4).
python-tests runs the PY_GATE files, the PY_SUITES whose trigger prefixes match a changed file,
every changed tests/**/test_*.py (except tests/scenarios/, which has its own steps), and the
test files that import a changed tests/support module. With
--full it runs PY_GATE plus every suite, not all of tests/ (`mise run test` runs all of tests/).
rust-tests runs cargo test --workspace --release. --explain takes the same options as a run, then
prints each changed file with its matched prefix and steps, the node plan with dependencies and
the python-tests files with their suite, without running anything or creating a run directory.

Prints the chosen steps, then one PASS/FAIL line per step as each finishes (independent steps
run in parallel after the builds they need; each step's output is printed whole); each step's output is in
out/runs/verify/<stamp>/<step>.log. Exits 1 when any step failed.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import threading
import time
from collections.abc import Callable
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

from tools import python_env  # noqa: E402
from tools.devtools import (  # noqa: E402
    benchlock,  # noqa: E402
    fastcov,  # noqa: E402
)
from tools.emulator import (  # noqa: E402
    coupled,  # noqa: E402
    digi_env,  # noqa: E402
)

PY = ["uv", "run", "python"]
FX_FILES = ("tests/scenarios/dn2-trackfx.json", "tests/scenarios/dn2-sendfx.json")
DT2_FX_FILES = ("tests/scenarios/dt2-trackfx.json", "tests/scenarios/dt2-sendfx.json")
# python-tests always runs PY_GATE. A suite of PY_SUITES, (name, trigger prefixes, test
# paths), runs when a changed file starts with one of its prefixes (all suites with
# --full). Test paths may be globs or directories; one that matches nothing is an error.
PY_GATE = (
    "tests/sharc/integration/test_sharc_golden.py",
    "tests/sharc/semantics/test_sharc_denormal.py",
    "tests/sharc/generation/test_sharc_rsgen_density.py",
    "tests/devtools/test_lint.py",
    "tests/devtools/test_types.py",
    "tests/devtools/test_pathcheck_repo.py",
    "tests/patchkit/test_patchkit.py",
    "tests/patchkit/test_patchkit_expansion.py",
)
PY_SUITES: tuple[tuple[str, tuple[str, ...], tuple[str, ...]], ...] = (
    (
        "patchkit",
        (
            "packages/patchkit/",
            "modules/",
            "devices/ports/",
            "tools/patchkit/pk_",
            "tools/patchkit/port_resolve.py",
            "tools/patchkit/patchkit.py",
            "tools/patchkit/pluck_ref.py",
            "tests/patchkit/",
        ),
        ("tests/patchkit",),
    ),
    (
        # the PLAITS host port, bit exact against upstream Plaits (the oracle needs ../eurorack/stmlib or
        # PLAITS_STMLIB; python-tests sets PLAITS_ORACLE_REQUIRED=1, so a missing oracle fails instead of skipping)
        "plaits",
        ("modules/machines/plaits/", "tests/plaits/", "tests/support/plaits_"),
        ("tests/plaits",),
    ),
    (
        "control-client",
        ("tools/control/digi_client.py", "packages/emulator/control/"),
        ("tests/control",),
    ),
    (
        "devtools",
        (
            "tools/devtools/verify.py",
            "tools/devtools/rust_ws.py",
            "tools/patchkit/pk_proofs.py",
            "tools/devtools/benchlock.py",
            "tools/devtools/test_outcomes.py",
        ),
        (
            "tests/devtools/test_verify.py",
            "tests/devtools/test_test_outcomes.py",
            "tests/devtools/test_benchlock.py",
            "tests/patchkit/test_pk_proofs.py",
        ),
    ),
    (
        "launcher",
        (
            "tools/emulator/native_emu.py",
            "tools/emulator/native_pgo.py",
            "tools/emulator/native_target.py",
            "tools/emulator/coupled.py",
            "tools/emulator/digi_env.py",
        ),
        (
            "tests/emulator/test_native_emu_launcher.py",
            "tests/emulator/test_native_pgo.py",
            "tests/emulator/test_native_target.py",
            "tests/emulator/test_coupled_tool.py",
            "tests/emulator/test_gen_fingerprint.py",
        ),
    ),
)
STEPS = (
    "build",
    "desktop",
    "web",
    "pins",
    "check",
    "check-no-aot",
    "tier-quick",
    "tier-sweep",
    "python-tests",
    "rust-tests",
    "card",
    "dt2-tone",
    "dt2-check",
    "dt2-tier-quick",
    "dt2-tier-sweep",
)
# Every seventh FX scenario of each file: about 20 of 134, at least one per FX family.
TIER_QUICK_EVERY = 7
EVERYTHING = tuple(s for s in STEPS if s not in ("tier-sweep", "dt2-tier-sweep"))
FALLBACK = ("build", "desktop", "pins", "check", "python-tests")
# The first rule whose prefix matches a changed path decides that path's steps.
RULES: tuple[tuple[tuple[str, ...], tuple[str, ...]], ...] = (
    (("docs/", "README", "AGENTS.md", "CLAUDE.md", "tests/scenarios/README"), ()),
    # GitHub settings files (templates, CODEOWNERS): only the path check reads them.
    ((".github/",), ("python-tests",)),
    (
        (
            "tests/scenarios/dt2-",
            "tests/scenarios/test_dt2",
            "tools/emulator/gen_dt2fx.py",
            "tools/emulator/dt2fx.py",
        ),
        ("dt2-check", "dt2-tier-quick", "python-tests"),
    ),
    (
        (
            ".oxlintrc.json",
            ".prettierrc.json",
            ".prettierignore",
            ".editorconfig",
            "packages/control-client/",
            "packages/test-support/",
            "tests/regression/",
        ),
        ("web",),
    ),
    (("apps/web/",), ("web",)),
    (("apps/desktop/",), ("desktop", "web")),
    (
        (
            "tools/sharc_dev/sharc_core/",
            "tools/sharc_dev/sharc_transpile/",
            "tools/sharc_dev/sharc_transpile_infer.py",
            "tools/sharc_dev/sharc_rsgen.py",
            "packages/emulator/sharc-gen/",
            "packages/emulator/sharc-rt/",
        ),
        EVERYTHING,
    ),
    (
        (
            "packages/emulator/sharc/src/fast/",
            "packages/emulator/sharc-fast-cl/",
            "packages/emulator/sharc-fast-wasm/",
        ),
        ("build", "pins", "check", "check-no-aot", "tier-quick", "rust-tests"),
    ),
    (
        ("packages/emulator/sharc/",),
        (
            "build",
            "desktop",
            "pins",
            "check",
            "check-no-aot",
            "tier-quick",
            "rust-tests",
            "dt2-tone",
        ),
    ),
    (
        (
            "packages/patchkit/",
            "modules/",
            "devices/ports/",
            "tools/patchkit/pk_",
            "tools/patchkit/port_resolve.py",
            "tools/patchkit/patchkit.py",
            "tests/patchkit/",
        ),
        ("rust-tests", "python-tests", "web"),
    ),
    (
        ("packages/emulator/control/",),
        ("desktop", "rust-tests", "card", "python-tests"),
    ),
    (
        (
            "packages/emulator/plusdrive/",
            "packages/emulator/card/",
            "tools/emulator/card_check.py",
        ),
        ("desktop", "rust-tests", "card"),
    ),
    (
        ("packages/emulator/",),
        ("build", "desktop", "pins", "check", "rust-tests", "card", "dt2-tone"),
    ),
    (
        (
            "tests/scenarios/",
            "tools/emulator/fxcheck.py",
            "tools/emulator/audiocheck.py",
            "tools/emulator/scenario_dsl.py",
        ),
        ("check", "python-tests"),
    ),
    (
        (
            "tools/emulator/coupled.py",
            "tools/emulator/digi_env.py",
            "tools/emulator/native_emu.py",
            "tools/emulator/native_pgo.py",
            "tools/emulator/native_target.py",
            "tools/devtools/fastcov.py",
        ),
        ("build", "pins", "python-tests"),
    ),
    (("tools/", "tests/", "pyproject.toml"), ("python-tests",)),
    (
        ("Cargo.toml", "Cargo.lock", "mise.toml", "package.json", "pnpm-lock.yaml"),
        EVERYTHING,
    ),
)


def git_lines(*args: str) -> list[str]:
    out = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return [line for line in out.stdout.splitlines() if line]


def changed_files(base: str) -> list[str]:
    """Files changed since the merge base with BASE, committed or not, plus untracked files."""
    since = git_lines("merge-base", base, "HEAD")[0] if base != "HEAD" else "HEAD"
    files = set(git_lines("diff", "--name-only", since))
    files |= set(git_lines("ls-files", "--others", "--exclude-standard"))
    return sorted(files)


def route(path: str) -> tuple[str, tuple[str, ...]]:
    """How one changed path is routed: (matched prefix or "docs"/"fallback", steps)."""
    if path.endswith(".md"):
        return "docs", ()  # documentation selects no steps
    for prefixes, steps in RULES:
        for prefix in prefixes:
            if path.startswith(prefix):
                return prefix, steps
    return "fallback", FALLBACK


def is_changed_test(path: str) -> bool:
    """A changed test file that python-tests runs itself (scenario tests have steps)."""
    name = path.rsplit("/", 1)[-1]
    return (
        path.startswith("tests/")
        and not path.startswith("tests/scenarios/")
        and name.startswith("test_")
        and name.endswith(".py")
        and (ROOT / path).is_file()
    )


def resolve_tests(paths: tuple[str, ...]) -> list[str]:
    """Repo-relative test paths for PATHS (globs and directories); a name that matches
    nothing is a registry error, so a rename cannot drop a suite silently."""
    found: list[str] = []
    for pattern in paths:
        if any(c in pattern for c in "*?["):
            hits = sorted(ROOT.glob(pattern))
        else:
            hits = [ROOT / pattern] if (ROOT / pattern).exists() else []
        if not hits:
            raise SystemExit(f"verify: PY_SUITES entry matches nothing: {pattern}")
        found += [hit.relative_to(ROOT).as_posix() for hit in hits]
    return found


def support_users(path: str) -> list[str]:
    """The test files that import the changed helper module tests/support/NAME.py."""
    if not (path.startswith("tests/support/") and path.endswith(".py")):
        return []
    module = "support." + path.removeprefix("tests/support/").removesuffix(".py")
    tests = sorted((ROOT / "tests").rglob("test_*.py"))
    users = [t for t in tests if module in t.read_text(encoding="utf-8")]
    return [t.relative_to(ROOT).as_posix() for t in users]


def python_test_plan(files: list[str], full: bool) -> list[tuple[str, str]]:
    """(pytest path, source) in run order, without duplicates: the gate files, the
    suites a changed file triggers (all of them when FULL), changed test files, then
    the tests that import a changed tests/support/ module."""
    plan_: list[tuple[str, str]] = [(path, "gate") for path in PY_GATE]
    for name, triggers, tests in PY_SUITES:
        if full or any(f.startswith(triggers) for f in files):
            plan_ += [(path, name) for path in resolve_tests(tests)]
    plan_ += [(f, "changed") for f in files if is_changed_test(f)]
    plan_ += [(t, "support") for f in files for t in support_users(f)]
    seen: set[str] = set()
    return [(p, s) for p, s in plan_ if not (p in seen or seen.add(p))]


def python_tests(files: list[str], full: bool) -> list[str]:
    """The ordered, de-duplicated pytest paths for the python-tests step."""
    return [path for path, _ in python_test_plan(files, full)]


def steps_for(files: list[str]) -> tuple[str, ...]:
    """The steps the changed files need, in gate order."""
    wanted: set[str] = set()
    for path in files:
        wanted.update(route(path)[1])
        if is_changed_test(path) or (
            not path.endswith(".md")
            and any(path.startswith(triggers) for _, triggers, _ in PY_SUITES)
        ):
            wanted.add("python-tests")
    return tuple(s for s in STEPS if s in wanted)


def cache_env(
    base: dict[str, str] | None = None, no_cache: bool = False
) -> dict[str, str]:
    """``base`` (default os.environ) with the render-cache settings of the
    scenario-rendering steps: spot-check probability (default 0.03) and, with
    ``no_cache``, DIGI_RENDER_CACHE=0."""
    env = dict(os.environ if base is None else base)
    env.setdefault("DIGI_RENDER_CACHE_CHECK", "0.03")
    if no_cache:
        env["DIGI_RENDER_CACHE"] = "0"
    return env


def no_aot_env() -> dict[str, str]:
    """The fast-tier-only configuration (fastcov `static-insns+follow`)."""
    _, env = fastcov.parse_config("static-insns+follow")
    return {**os.environ, **env}


def pytest_ran(text: str) -> str | None:
    """A pytest run that only skipped is not a pass."""
    lines = text.strip().splitlines()
    last = lines[-1] if lines else ""
    return None if " passed" in last else f"nothing passed ({last})"


def python_tests_passed(out_dir: Path) -> str | None:
    """A python-tests run whose files passed no test at all is not a pass."""
    summary = out_dir / "summary.tsv"
    if not summary.exists():
        return f"no {summary.name}"
    rows = summary.read_text().splitlines()[1:]
    passed = 0
    for row in rows:
        cols = row.split("\t")
        if len(cols) > 4 and cols[4].isdigit():
            passed += int(cols[4])
    return None if passed > 0 else "nothing passed (0 tests passed)"


def python_tests_env(base: dict[str, str] | None = None) -> dict[str, str]:
    """The python-tests environment: the PLAITS bit-exact oracle (tests/plaits) must run, not skip, unless the
    caller sets PLAITS_ORACLE_REQUIRED=0 itself."""
    env = dict(os.environ if base is None else base)
    env.setdefault("PLAITS_ORACLE_REQUIRED", "1")
    return env


def python_tests_command(out: Path, files: list[str]) -> list[str]:
    """The per-file runner for the python-tests step (one pytest process per file)."""
    cmd = [
        *PY,
        "-m",
        "tools.devtools.test_outcomes",
        "run",
        "--check",
        "--out",
        str(out),
    ]
    cmd = [*cmd, "--timeout", str(python_timeout())]
    return [*cmd, "--jobs", str(python_jobs()), *files]


class Gate:
    """Collects each step's output lines in a per-thread buffer, so parallel steps
    print whole and in order when they finish."""

    def __init__(self, out: Path, py_tests: list[str] | None = None) -> None:
        self.out = out
        self.py_tests = py_tests if py_tests is not None else list(PY_GATE)
        self.failed: list[str] = []
        self.local = threading.local()
        self.lock = threading.Lock()

    def say(self, line: str) -> None:
        buf = getattr(self.local, "lines", None)
        if buf is None:
            print(line, flush=True)
        else:
            buf.append(line)

    def report(
        self, name: str, problem: str | None, seconds: float, text: str = ""
    ) -> None:
        if problem is None:
            self.say(f"PASS {name} ({seconds:.0f} s)")
            return
        with self.lock:
            self.failed.append(name)
        tail = " | ".join(text.strip().splitlines()[-3:])
        self.say(f"FAIL {name} ({seconds:.0f} s): {problem}: {tail}")

    def run(
        self,
        name: str,
        cmd: list[str],
        env: dict[str, str] | None = None,
        check: Callable[[str], str | None] | None = None,
        cwd: Path = ROOT,
    ) -> None:
        """Run one step, log its output and print its line."""
        start = time.monotonic()
        with benchlock.hold("shared", f"verify {name}"):
            proc = subprocess.run(
                cmd, cwd=cwd, env=python_env(env), capture_output=True, text=True
            )
        text = proc.stdout + proc.stderr
        (self.out / f"{name}.log").write_text(text)
        if proc.returncode:
            problem: str | None = f"exit {proc.returncode}"
        else:
            problem = check(text) if check else None
        self.report(name, problem, time.monotonic() - start, text)


def tier_jobs_budget() -> int:
    """Coupled-run processes for all tier groups together: DIGI_TIER_JOBS, else
    cpus - 4 (the performance cores of a 16-core machine)."""
    env = os.environ.get("DIGI_TIER_JOBS")
    if env:
        jobs = int(env)
        if jobs < 1:
            raise SystemExit("verify: DIGI_TIER_JOBS must be at least 1")
        return jobs
    return max(1, (os.cpu_count() or 1) - 4)


def python_jobs() -> int:
    """Test files run at once in python-tests: DIGI_PY_JOBS, else max(2, cpus // 4)."""
    env = os.environ.get("DIGI_PY_JOBS")
    if env:
        return max(1, int(env))
    return max(2, (os.cpu_count() or 1) // 4)


def python_timeout() -> int:
    """Per-file timeout for python-tests: catches hangs, not slow files (test_pluck.py
    takes ~450 s alone and more than 900 s under verify:full load)."""
    env = os.environ.get("DIGI_PY_TIMEOUT")
    if env:
        seconds = int(env)
        if seconds < 1:
            raise SystemExit("verify: DIGI_PY_TIMEOUT must be at least 1")
        return seconds
    return 3600


# no-AOT renders cost about 2x per simulated second (measured 1.9-2.0 on DN2 FX, 2026-10-14); weights chunk costs
TIER_NO_AOT_COST = 2.0


@dataclass(frozen=True)
class Chunk:
    """Consecutive scenarios of one (label, file) group, run by one coupled process."""

    label: str
    path: str
    names: tuple[str, ...]
    cost: float
    index: int


def plan_chunks(
    groups: list[tuple[str, str, list[str], dict[str, float], float]], budget: int
) -> list[Chunk]:
    """Cut each group (label, path, names, seconds by name, cost factor) into consecutive
    chunks of about total cost / (budget * 4), at least one scenario each; longest first."""
    costs = [
        [factor * seconds[n] for n in names] for _, _, names, seconds, factor in groups
    ]
    target = sum(sum(c) for c in costs) / (budget * 4)
    chunks: list[Chunk] = []
    for (label, path, names, _, _), group_costs in zip(groups, costs, strict=True):
        index = 0
        taken: list[str] = []
        cost = 0.0
        for name, c in zip(names, group_costs, strict=True):
            taken.append(name)
            cost += c
            if cost >= target:
                chunks.append(Chunk(label, path, tuple(taken), cost, index))
                index += 1
                taken, cost = [], 0.0
        if taken:
            chunks.append(Chunk(label, path, tuple(taken), cost, index))
    chunks.sort(key=lambda c: -c.cost)
    return chunks


def tier(gate: Gate, name: str, every: int, device: str = "dn2") -> None:
    """FX scenarios must give the same PCM in the default and no-AOT configurations.

    DT2 runs its scenarios on the one-sample card; its no-AOT configuration is the
    interpreter alone (`SHARC_AOT=0`, about 80 s per audio second), as DT2 has no
    fast regions."""
    start = time.monotonic()
    shas: dict[str, dict[str, str]] = {}
    dt2 = device == "dt2"
    no_aot = {**os.environ, "SHARC_AOT": "0"} if dt2 else no_aot_env()
    envs: dict[str, dict[str, str] | None] = {"default": None, "no-aot": no_aot}
    groups: list[tuple[str, str, list[str], dict[str, float], float]] = []
    for label in envs:
        for path in DT2_FX_FILES if dt2 else FX_FILES:
            seconds = coupled.scenario_seconds(path)
            names = sorted(seconds)[::every]
            factor = TIER_NO_AOT_COST if label == "no-aot" else 1.0
            groups.append((label, path, names, seconds, factor))
    budget = tier_jobs_budget()
    chunks = plan_chunks(groups, budget)

    def report_of(chunk: Chunk) -> Path:
        stem = Path(chunk.path).stem
        return (
            gate.out
            / f"{name}-{chunk.label}-{stem}"
            / f"chunk-{chunk.index:03d}"
            / "report.json"
        )

    def run_chunk(chunk: Chunk) -> tuple[subprocess.CompletedProcess[str], float]:
        report = report_of(chunk)
        report.parent.mkdir(parents=True, exist_ok=True)
        cmd = [
            *PY,
            "-m",
            "tools.emulator.coupled",
            "scenario",
            "--scenarios",
            chunk.path,
        ]
        cmd += [*chunk.names, "--no-build", "--jobs", "1", "--out", str(report)]
        if dt2:
            cmd += ["--device", "dt2", "--dt2-card"]
        with benchlock.hold("shared", f"verify {name}"):
            proc = subprocess.run(
                cmd,
                cwd=ROOT,
                env=python_env(envs[chunk.label]),
                capture_output=True,
                text=True,
            )
        return proc, time.monotonic() - start

    with ThreadPoolExecutor(max_workers=budget) as pool:
        done = list(pool.map(run_chunk, chunks))
    ended: dict[tuple[str, str], float] = {}
    for chunk, (_, end) in zip(chunks, done, strict=True):
        key = (chunk.label, chunk.path)
        ended[key] = max(ended.get(key, 0.0), end)
    for label, path, names, _, _ in groups:
        count = sum(1 for c in chunks if (c.label, c.path) == (label, path))
        gate.say(
            f"     {label} {Path(path).stem}: {len(names)} scenarios in "
            f"{count} chunks, {ended.get((label, path), 0.0):.0f} s"
        )
    gate.say(f"     budget {budget} jobs, {len(chunks)} chunks")
    for chunk, (proc, _) in zip(chunks, done, strict=True):
        report = report_of(chunk)
        text = proc.stdout + proc.stderr
        (report.parent / "run.log").write_text(text)
        # Exit 2 is a failed gate in an unpinned FX scenario; only the PCM matters here.
        if proc.returncode not in (0, 2) or not report.exists():
            problem = (
                f"{chunk.label} {chunk.path} chunk {chunk.index}: "
                f"exit {proc.returncode}"
            )
            gate.report(name, problem, time.monotonic() - start, text)
            return
        for result in coupled.scenario_results(json.loads(report.read_text())):
            shas.setdefault(result["name"], {})[chunk.label] = result["sha256"]
    for label, path, names, _, _ in groups:
        for n in names:
            if label not in shas.get(n, {}):
                problem = f"{label} {path}: no result for {n}"
                gate.report(name, problem, time.monotonic() - start)
                return
    differ = [n for n, s in shas.items() if s.get("default") != s.get("no-aot")]
    problem = (
        f"{len(differ)} of {len(shas)} differ: {', '.join(differ[:5])}"
        if differ
        else None
    )
    gate.report(name, problem, time.monotonic() - start)
    if not differ:
        gate.say(f"     {len(shas)} of {len(shas)} scenarios equal")


def desktop_check(gate: Gate) -> None:
    """The desktop console builds and type-checks, and the app crate compiles with it."""
    gate.run("desktop-ui", ["pnpm", "--filter", "@digi/desktop", "run", "build:ui"])
    gate.run(
        "desktop-ui-types", ["pnpm", "--filter", "@digi/desktop", "run", "check:ui"]
    )
    env = digi_env.rust_env()
    env["SHARC_GEN_DIR"] = str(digi_env.generated_dir())
    cmd = ["cargo", "check", "--manifest-path", "apps/desktop/src-tauri/Cargo.toml"]
    cmd += ["--target-dir", str(ROOT / "out" / "native" / "target" / "check-desktop")]
    gate.run("desktop", cmd, env=env)


def web_check(gate: Gate) -> None:
    web = ROOT / "apps" / "web"
    gate.run(
        "patchkit-wasm", [*PY, "-m", "tools.emulator.native_wasm", "--patchkit-only"]
    )
    gate.run("web-lint", ["pnpm", "exec", "oxlint"])
    gate.run("web-format", ["pnpm", "exec", "prettier", "--check", "."])
    gate.run("web-tests", ["node", "--test", "tests/*.test.mjs"], cwd=web)
    gate.run("web-types", ["npx", "astro", "check"], cwd=web)


def default_step_jobs() -> int:
    return max(1, (os.cpu_count() or 2) // 2)


# Steps that need a built binary: DN2 steps the DN2 coupled-run, DT2 steps the DT2 one.
# The build nodes run first; the others are independent of each other.
DN2_BINARY = ("pins", "check", "check-no-aot", "tier-quick", "tier-sweep")
DT2_BINARY = ("dt2-tone", "dt2-check", "dt2-tier-quick", "dt2-tier-sweep")
# Longest first, so the pool is not left waiting on one long step at the end.
PRIORITY = (
    "build",
    "build-dt2",
    "tier-sweep",
    "dt2-tier-sweep",
    "dt2-tier-quick",
    "check-no-aot",
    "check",
    "tier-quick",
    "dt2-check",
    "pins",
)


def plan(steps: tuple[str, ...]) -> tuple[list[str], dict[str, tuple[str, ...]]]:
    """The nodes to run (steps plus the builds they need) and each node's dependencies."""
    nodes = list(steps)
    if any(s in DN2_BINARY for s in steps) and "build" not in nodes:
        nodes.append("build")
    if any(s in DT2_BINARY for s in steps) and "build-dt2" not in nodes:
        nodes.append("build-dt2")
    deps: dict[str, tuple[str, ...]] = {}
    for node in nodes:
        if node in DN2_BINARY:
            deps[node] = ("build",)
        elif node in DT2_BINARY:
            deps[node] = ("build-dt2",)
        else:
            deps[node] = ()
    # "build" also builds DT2 when only DT2 steps need it, via its own node; when the
    # user picked "build" and DT2 steps, build-dt2 is added above.
    order = {n: i for i, n in enumerate(PRIORITY)}
    nodes.sort(
        key=lambda n: (order.get(n, len(PRIORITY)), STEPS.index(n) if n in STEPS else 0)
    )
    return nodes, deps


def rust_tests_command() -> list[str]:
    # cargo test --workspace --release (rust_ws.py); digiemu-desktop is excluded when
    # apps/desktop/ui/dist is absent
    return [*PY, "-m", "tools.devtools.rust_ws", "test"]


def explain(
    steps: tuple[str, ...],
    files: list[str],
    py_plan: list[tuple[str, str]],
    per_file: bool,
) -> None:
    """Print the routing, the node plan and the python-tests files; run nothing."""
    if per_file:
        print("changed files:")
        for path in files:
            label, wanted = route(path)
            print(f"  {path} -> {label} -> {', '.join(wanted) or 'no steps'}")
    print(f"steps: {', '.join(steps) or 'nothing to run'}")
    nodes, deps = plan(steps)
    print("plan:")
    for node in nodes:
        print(f"  {node} <- {', '.join(deps[node]) or 'no dependencies'}")
    if "python-tests" in steps:
        print("python-tests files:")
        for path, source in py_plan:
            print(f"  {path} ({source})")


def run_step(gate: Gate, step: str) -> None:
    scenario_tests = [
        *PY,
        "-m",
        "pytest",
        "tests/scenarios",
        "--slow",
        "-q",
        "--no-build",
    ]
    jobs = str(coupled.default_jobs())
    if step == "build":
        gate.run("build", [*PY, "-m", "tools.emulator.coupled", "build"])
    elif step == "build-dt2":
        gate.run(
            "build-dt2",
            [*PY, "-m", "tools.emulator.coupled", "build", "--device", "dt2"],
        )
    elif step == "desktop":
        desktop_check(gate)
    elif step == "web":
        web_check(gate)
    elif step == "pins":
        cmd = [
            *PY,
            "-m",
            "tools.emulator.coupled",
            "scenario",
            "--all",
            "--no-build",
            "--jobs",
            jobs,
        ]
        gate.run("pins", cmd)
    elif step == "check":
        gate.run("check", scenario_tests, check=pytest_ran)
    elif step == "check-no-aot":
        gate.run("check-no-aot", scenario_tests, env=no_aot_env(), check=pytest_ran)
    elif step == "tier-quick":
        tier(gate, "tier-quick", TIER_QUICK_EVERY)
    elif step == "tier-sweep":
        tier(gate, "tier-sweep", 1)
    elif step == "python-tests":
        out = gate.out / "python-tests"
        gate.run(
            "python-tests",
            python_tests_command(out, gate.py_tests),
            env=python_tests_env(),
            check=lambda _text: python_tests_passed(out),
        )
    elif step == "rust-tests":
        gate.run("rust-tests", rust_tests_command())
    elif step == "card":
        gate.run("card", [*PY, "-m", "tools.emulator.card_check"])
    elif step == "dt2-tone":
        gate.run("dt2-tone", [*PY, "-m", "tools.emulator.dt2_sample_check"])
    elif step == "dt2-check":
        gate.run("dt2-check", [*scenario_tests, "--device", "dt2"], check=pytest_ran)
    elif step == "dt2-tier-quick":
        tier(gate, "dt2-tier-quick", TIER_QUICK_EVERY, "dt2")
    elif step == "dt2-tier-sweep":
        tier(gate, "dt2-tier-sweep", 1, "dt2")


def run_steps(gate: Gate, steps: tuple[str, ...], jobs: int) -> list[tuple[str, float]]:
    """Run the steps in a pool of JOBS threads, after the builds they need.

    Each step's output is buffered and printed whole when it finishes. A step whose
    build failed is skipped. Returns (node, seconds) in completion order."""
    nodes, deps = plan(steps)
    done: set[str] = set()
    failed_nodes: set[str] = set()
    pending = list(nodes)
    running: dict[object, tuple[str, float]] = {}
    times: list[tuple[str, float]] = []

    def work(node: str) -> list[str]:
        gate.local.lines = []
        try:
            run_step(gate, node)
        except BaseException as exc:  # a crashed step is a failed step
            with gate.lock:
                gate.failed.append(node)
            gate.say(f"FAIL {node}: {type(exc).__name__}: {exc}")
        return gate.local.lines

    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        while pending or running:
            for node in list(pending):
                if any(d in failed_nodes for d in deps[node]):
                    pending.remove(node)
                    failed_nodes.add(node)
                    print(f"SKIP {node}: its build failed", flush=True)
                    with gate.lock:
                        gate.failed.append(node)
                elif all(d in done for d in deps[node]) and len(running) < max(1, jobs):
                    pending.remove(node)
                    running[pool.submit(work, node)] = (node, time.monotonic())
            if not running:
                continue
            finished, _ = wait(list(running), return_when=FIRST_COMPLETED)
            for fut in finished:
                node, began = running.pop(fut)
                lines = fut.result()
                print("\n".join(lines), flush=True)
                times.append((node, time.monotonic() - began))
                if any(line.startswith("FAIL") for line in lines):
                    failed_nodes.add(node)
                else:
                    done.add(node)
    return times


def load_last_result(verify_dir: Path, exclude: Path | None) -> dict | None:
    """The newest (by directory name) result.json under VERIFY_DIR, skipping EXCLUDE."""
    if not verify_dir.is_dir():
        return None
    for run in sorted(verify_dir.iterdir(), key=lambda p: p.name, reverse=True):
        if exclude is not None and run.resolve() == exclude.resolve():
            continue
        path = run / "result.json"
        if run.is_dir() and not run.is_symlink() and path.is_file():
            return json.loads(path.read_text())
    return None


def rerun_plan(result: dict) -> tuple[tuple[str, ...], list[str]]:
    """(steps, python-tests files) to repeat the steps that failed in RESULT."""
    failed = tuple(result.get("failed", []))
    unknown = sorted(set(failed) - set(STEPS))
    if unknown:
        raise SystemExit(f"verify: unknown steps in last result: {', '.join(unknown)}")
    steps = tuple(s for s in STEPS if s in failed)
    return steps, list(result.get("py_tests", []))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description="Run the exactness gate for what changed, one line per step."
    )
    ap.add_argument(
        "--base",
        default="HEAD",
        help="compare with the merge base of this ref (default: HEAD)",
    )
    ap.add_argument("--full", action="store_true", help="every step, full tier sweep")
    ap.add_argument("--only", help=f"comma-separated steps from: {', '.join(STEPS)}")
    ap.add_argument(
        "--dt2",
        action="store_true",
        help="with --full: the full DT2 FX tier sweep (about 35 min more)",
    )
    ap.add_argument(
        "--jobs",
        type=int,
        default=0,
        help="steps run at once (default: half the CPUs); 1 is sequential",
    )
    ap.add_argument(
        "--explain",
        action="store_true",
        help="print the routing, the plan and the python-tests files; run nothing",
    )
    ap.add_argument(
        "--rerun-failed",
        action="store_true",
        help="only the steps that failed in the most recent run",
    )
    ap.add_argument(
        "--no-cache",
        action="store_true",
        help="render every scenario (DIGI_RENDER_CACHE=0): the release gate",
    )
    args = ap.parse_args(argv)
    if args.rerun_failed and (args.only or args.full):
        ap.error("--rerun-failed cannot be combined with --only or --full")
    files: list[str] = []
    full, dt2 = args.full, args.dt2
    rerun_tests: list[str] | None = None
    if args.rerun_failed:
        last = load_last_result(Path(digi_env.RUNS_ROOT) / "verify", None)
        if last is None:
            print("verify: no previous run with a result.json")
            return 2
        if not last.get("failed"):
            print("verify: the last run had no failed steps; nothing to do")
            return 0
        steps, rerun_tests = rerun_plan(last)
        full, dt2 = bool(last.get("full")), bool(last.get("dt2"))
        old, new = str(last.get("head", "?")), git_lines("rev-parse", "HEAD")[0]
        if old != new:
            print(
                f"verify: code changed since the failed run ({old[:10]} -> "
                f"{new[:10]}); re-running the failed steps on the current tree"
            )
        why = "--rerun-failed"
    elif args.only:
        steps = tuple(args.only.split(","))
        unknown = sorted(set(steps) - set(STEPS))
        if unknown:
            ap.error(f"unknown steps: {', '.join(unknown)}")
        why = "--only"
        if "python-tests" in steps:
            files = changed_files(args.base)
    elif args.full:
        skip = {"tier-quick", "dt2-tier-quick" if args.dt2 else "dt2-tier-sweep"}
        steps = tuple(s for s in STEPS if s not in skip)
        why = "--full"
    else:
        files = changed_files(args.base)
        steps = steps_for(files)
        why = f"{len(files)} changed files since {args.base}"
    if rerun_tests is not None:
        py_plan = [(path, "rerun") for path in rerun_tests]
    else:
        py_plan = python_test_plan(files, args.full)
    if args.explain:
        explain(
            steps,
            files,
            py_plan,
            per_file=not (args.only or args.full or args.rerun_failed),
        )
        return 0
    # Every step, and no_aot_env(), inherits os.environ: set the cache env once.
    os.environ.update(cache_env(None, args.no_cache))
    out = Path(digi_env.run_dir("verify"))
    out.mkdir(parents=True, exist_ok=True)
    print(
        f"verify: {', '.join(steps) or 'nothing to run'} ({why}); logs in {out}",
        flush=True,
    )
    gate = Gate(out, [path for path, _ in py_plan])
    jobs = args.jobs if args.jobs else default_step_jobs()
    start = time.monotonic()
    times = run_steps(gate, steps, jobs)
    for name, seconds in times:
        print(f"  {name:<16} {seconds:6.0f} s", flush=True)
    print(f"verify: wall {time.monotonic() - start:.0f} s with {jobs} jobs", flush=True)
    result = {
        "steps": list(steps),
        "failed": [s for s in gate.failed if s in STEPS],
        "full": full,
        "dt2": dt2,
        "base": args.base,
        "head": git_lines("rev-parse", "HEAD")[0],
        "py_tests": gate.py_tests,
    }
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    if gate.failed:
        print(f"verify: FAILED: {', '.join(gate.failed)}", flush=True)
        return 1
    print("verify: all passed", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
