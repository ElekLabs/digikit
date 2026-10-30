"""Finer routing for tools/devtools/verify.py: which steps and test files one changed path needs.

verify.py asks `fine(path)` first. A path this module knows gets its own steps and its own test
files; any other path falls through to the prefix rules of verify.py (RULES, PY_SUITES). The
functions read the working tree (module manifests, test files, scenario TOMLs); only
`mise_paths` takes texts, so the mise.toml routing can be tested without git.

Rules (docs/TESTING.md "What verify runs"):

- a module's own tests live in `modules/<group>/<name>/tests/` (test_*.py, scenarios/*.toml, TypeScript *.test.ts): found by globbing,
  selected by any change in the module (and by a change in a module it requires); a change in that tests/ directory alone selects
  only python-tests. Module ids come from the module.toml files, not from a list of authors.
- a file of a module under modules/ or tests/fixtures/modules/: python-tests and rust-tests; web only for the files the web
  gallery bundle reads (module.toml, build/, gallery/, registry tables, not src/, sdk/, tests/).
  Framework modules (FRAMEWORK: every patched image includes them) select all of tests/patchkit
  and tests/digikit. Any other module selects the test files that name it or a module that
  requires it (transitively) and the digikit test files of the scenarios that name it. No PCM step.
- a machine module (a DSP routine behind a `[[voice]]` or a voices `[[entry]]`, tools/patchkit/pk_conform.py), a module that one
  depends on or builds from, and core/dsp-voice (the wrapper every routine runs through) also select the machine conformance
  suite, CONFORMANCE (docs/TESTING.md "Machine conformance"). Its cases for every machine run in one file.
- an effect module (an `[[entry]] registry = "fx"` with a DSP object) and core/fx, which every effect requires, select the FX conformance
  suite, FX_CONFORMANCE (docs/TESTING.md "FX conformance").
- a module of tools/device: the unit tests that import it (and the importers of those) plus the
  digikit test files of the scenario kinds whose runners call it (DEVICE_KINDS).
- a scenario file of tests/digikit/scenarios: the test files that run the scenario's kind.
- `tests/digikit/test_*.py`, `tests/patchkit/test_*.py`: the file itself.
- a Rust file of packages/patchkit/src in a group of PATCHKIT_GROUPS: the steps and tests of the group (checks, build, flash,
  wasm, codegen); any other file of the crate keeps the wide route.
- a scenario runner of tools/patchkit (RUNNER_KINDS): the digikit tests of its kinds and of the runners that import it.
- `mise.toml`: the tasks that changed, routed like the tool their `run` names (`mise_paths`).
"""

from __future__ import annotations

import ast
import re
import shlex
import tomllib
from dataclasses import dataclass
from functools import cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

from tools.patchkit import modroots, pk_conform  # noqa: E402

# Modules that every patched image or every runtime build includes: a change selects the whole
# patchkit and digikit suites.
FRAMEWORK = frozenset(
    {
        "core/runtime",
        "core/dsp-voice",
        "core/dsp-link",
        "core/machines",
        "core/params",
        "core/dynreg",
        "core/dyndsp",
        "core/digikit-menu",
        "core/health",
        "core/input",
        "core/expansion",
    }
)
# tools/device module -> the digikit scenario kinds whose runners (tools/patchkit/pk_digikit_*.py)
# call it. A module of tools/device that is not listed here fails tests/devtools/test_verify.py.
DEVICE_KINDS: dict[str, tuple[str, ...]] = {
    "__init__": (),
    "dkupload": ("upload", "menu", "debug", "crash", "card", "settings"),
    "dksettings": ("settings",),
    "dkrun": ("menu",),
    "dkfit": (
        "menu",
        "debug",
    ),  # the fit rule: dktransfer and dkrun use it, the `fits` part of the debug scenarios
    "dkneeds": (
        "menu",
        "debug",
    ),  # the needs of a .dkm, from `patchkit needs`: dktransfer, dkrun, the `inferred-needs` and `fits` parts of the debug scenarios
    "dktransfer": ("menu",),
    "dkpackall": (),  # a dev task: the files the workbench reads
    "dkscreen": ("upload",),
    "dklog": ("debug", "crash", "card"),
    "dklog_kinds": ("debug", "crash", "card"),  # the event kinds dklog reads
    "dkproto": (
        "upload",
        "menu",
        "debug",
        "crash",
        "card",
    ),  # the protocol numbers dkupload uses
    "devrpc": ("upload",),
    "midisniff": ("upload", "menu", "debug", "crash", "card"),
    "mmon": (),
}
BROAD_TASK_GROUPS = ("verify", "build", "gen", "setup")
ALL_SUITES = ("tests/patchkit", "tests/digikit")
CONFORMANCE = "tests/modules/test_machine_conformance.py"
CONFORMANCE_WRAPPERS = frozenset({"core/dsp-voice"})
FX_CONFORMANCE = "tests/modules/test_fx_conformance.py"
MODULE_TESTS_GLOB = (
    "modules/*/*/tests"  # a module's own tests: test_*.py, scenarios/*.toml, *.test.ts
)


@dataclass(frozen=True)
class Fine:
    """The steps and pytest paths (files or directories) one path needs."""

    label: str
    steps: tuple[str, ...]
    tests: tuple[str, ...]


@cache
def manifests() -> dict[str, tuple[str, ...]]:
    """Module id -> the modules it depends on: those it `requires` and those whose files its
    build sources name (`sources = ["../../x/y/..."]`). Modules and ids: tools/patchkit/modroots.py."""
    out: dict[str, tuple[str, ...]] = {}
    for mid, moddir in modroots.manifests().items():
        data = tomllib.loads((moddir / "module.toml").read_text(encoding="utf-8"))
        deps = list(data.get("module", {}).get("requires", []))
        for table in data.get("object", []):
            for source in table.get("sources", []):
                other = modroots.module_id(moddir / source)
                if other is not None and other != mid:
                    deps.append(other)
        out[mid] = tuple(dict.fromkeys(deps))
    return out


def dependents(ids: set[str]) -> set[str]:
    """IDS and every module that depends on one of them (manifests), directly or not."""
    mods = manifests()
    found = set(ids)
    grew = True
    while grew:
        grew = False
        for mid, requires in mods.items():
            if mid not in found and found.intersection(requires):
                found.add(mid)
                grew = True
    return found


def _production_uploads() -> dict[str, str]:
    """Module id -> its coupled lifecycle acceptance file, for the shipped modules (under `modules/`) that are run-time
    uploads: `dynamic`, a service or a `host` module. The file is `tests/regression/modules/<leaf>.test.ts`, `<leaf>` being
    the last part of the id; a module without that file has no lifecycle cell. tests/regression/modules/acceptance.ts
    scans the manifests by the same rule."""
    out: dict[str, str] = {}
    for mid, moddir in sorted(modroots.manifests().items()):
        if not moddir.is_relative_to(ROOT / "modules"):
            continue
        meta = tomllib.loads((moddir / "module.toml").read_text(encoding="utf-8")).get(
            "module", {}
        )
        if not (
            meta.get("dynamic") or meta.get("host") or meta.get("type") == "service"
        ):
            continue
        rel = f"tests/regression/modules/{mid.split('/')[-1]}.test.ts"
        if (ROOT / rel).is_file():
            out[mid] = rel
    return out


# These production uploads have coupled lifecycle acceptance, in addition to their static owners.
RUNTIME_UPLOADS = _production_uploads()
ACCEPTANCE_DIRS = {
    "stock": "dt2",
    "runtime-fixture": "runtime",
    "module-load": "modules",
}
# Runtime test files by the fixture module they exercise; the gate packs a native artifact for each
# module (DIGI_RUNTIME_MODULES). Paths are relative to tests/regression.
RUNTIME_MODULES: dict[str, tuple[str, ...]] = {
    "test/input-probe": ("runtime/input-adapter.test.ts",),
    "digikit/remote": ("runtime/remote-input.test.ts",),
    "test/svc-echo": ("runtime/service-commands.test.ts",),
    "test/dyn-fx-hang": ("runtime/fault-trace.test.ts",),
    "test/dyn-dsp-hang": ("runtime/fault-trace.test.ts",),
    "test/dyn-fx-nan": ("runtime/fault-trace.test.ts",),
    "test/dyn-fx-gain-send": ("runtime/fault-trace.test.ts",),
    "test/dyn-crash": ("runtime/fault-trace.test.ts",),
}
# Host sources only these runtime test files read.
RUNTIME_SOURCES: dict[str, tuple[str, ...]] = {
    path: ("runtime/samples-live.test.ts", "runtime/samples-store-live.test.ts")
    for path in (
        "packages/dk-session/src/dksample.ts",
        "packages/dk-session/src/rpcfiles.ts",
        "packages/dk-session/src/samples.ts",
    )
}
ACCEPTANCE_SHARED = (
    "packages/test-support/src/",
    "packages/control-client/src/",
    "packages/emulator/",
    "devices/",
    "tests/regression/support.ts",
)
RUNTIME_SHARED = (
    "sdk/",
    "packages/dk-session/src/",
    "packages/patchkit/",
)


@cache
def acceptance_python(step: str) -> set[str]:
    """Dependencies of consumed preparation/client entry points, using the existing import graph."""
    from tools.devtools import changed_tests

    # The graph omits function-local imports. These consumed call paths need stablebin from
    # patchkit.binary and sharc_disasm (imported inside the functions of the SHARC runner pk_sharc.run_call).
    roots = {
        "tools",
        "tools.patchkit.patchkit",
        "tools.devtools.stablebin",
        "tools.sharc_dev.sharc_disasm",
    }
    if step == "runtime-fixture":
        roots.update(
            {
                "tools.device.dktransfer",
                "tools.control.digi_client",
                "tools.emulator.card_overlay",
            }
        )
    graph = changed_tests.graph()
    found, todo = set(roots), list(roots)
    while todo:
        for dependency in graph.find_modules_directly_imported_by(todo.pop()):
            if dependency not in found:
                found.add(dependency)
                todo.append(dependency)
    return {changed_tests.path_of(module) for module in found}


@cache
def runtime_framework() -> set[str]:
    """Modules included by the actual runtime patch sets, including manifest dependencies."""
    required: set[str] = set(FRAMEWORK)
    for filename in ("digitakt-ii.toml", "digitone-ii.toml"):
        data = tomllib.loads((ROOT / "devices" / filename).read_text())
        for roots in data["patch_sets"].values():
            required.update(roots)
    todo = list(required)
    while todo:
        for mid in manifests().get(todo.pop(), ()):
            if mid not in required:
                required.add(mid)
                todo.append(mid)
    return required


def module_smoke_cells(ids: set[str]) -> list[str] | None:
    """Existing matrix cells consuming these leaves; None preserves unsupported cell settings."""
    affected = dependents(ids)
    matrix = tomllib.loads((ROOT / "tests/patchkit/smoke/matrix.toml").read_text())
    selected = []
    for cell in matrix["cell"]:
        modules = cell.get("modules", [])
        if any(mid not in manifests() for mid in modules):
            return None  # an unknown matrix module's dependencies cannot be proven unrelated
        if affected.intersection(modules):
            # --cell preserves the normal full-cell execution, but cannot encode custom settings.
            if set(cell) - {"device", "firmware", "modules"}:
                return None
            selected.append(f"{cell['device']}-{cell['firmware']}:" + ",".join(modules))
    return selected


def acceptance_files(step: str, paths: list[str]) -> list[str]:
    """Native Vitest filenames affected by PATHS, relative to tests/regression.

    Framework/upload dependencies are partly implicit in patched firmware, so shared code keeps
    cross-device coverage. Known static leaves retain their existing owners without unrelated uploads.
    """
    directory = ACCEPTANCE_DIRS[step]
    config = "tests/regression/vitest.config.ts"
    all_files = sorted(
        p.relative_to(ROOT / "tests/regression").as_posix()
        for p in (ROOT / "tests/regression" / directory).rglob("*.test.ts")
    )
    selected: set[str] = set()
    for path in paths:
        if path.endswith(".md") or path in {
            "devices/digitakt-ii-leds.toml",
            "devices/digitone-ii-leds.toml",
        }:
            continue
        if path in RUNTIME_SOURCES:
            if step == "runtime-fixture":
                selected.update(RUNTIME_SOURCES[path])
            continue
        own = f"tests/regression/{directory}/"
        if path.startswith(own):
            rel = path.removeprefix("tests/regression/")
            if path.endswith(".test.ts") and rel in all_files:
                selected.add(rel)
            else:
                # Helpers may serve several devices and cases; deleted cases keep their owner suite.
                selected.update(all_files)
        elif (
            path == config
            or path
            in ("tests/regression/package.json", "tests/regression/tsconfig.json")
            or path.startswith(ACCEPTANCE_SHARED)
            or (
                step != "stock"
                and path.startswith(RUNTIME_SHARED)
                and not patchkit_no_smoke(path)
            )
            or (step != "stock" and path == "tools/emulator/fast_regions.json")
            or (
                step != "stock"
                and path.startswith("tools/")
                and path in acceptance_python(step)
            )
        ):
            selected.update(all_files)
        elif step != "stock" and path.startswith(
            tuple(r + "/" for r in modroots.REPO_ROOTS)
        ):
            mid = modroots.module_id(path)
            if mid is None:
                selected.update(all_files)
            else:
                hit = dependents({mid})
                if hit & runtime_framework():
                    selected.update(all_files)
                elif step == "module-load":
                    selected.update(
                        f.removeprefix("tests/regression/")
                        for m, f in RUNTIME_UPLOADS.items()
                        if m in hit
                    )
                else:
                    for module, files in RUNTIME_MODULES.items():
                        if module in hit:
                            selected.update(files)
        elif step == "stock" and path.startswith("tests/regression/goldens/"):
            selected.update(all_files)
    return sorted(selected)


@cache
def _ids_by_dir() -> dict[str, str]:
    """`category/name` of a module directory -> its id (a path form in a test names the module too)."""
    out: dict[str, str] = {}
    for mid, moddir in modroots.manifests().items():
        for root in modroots.REPO_ROOTS:
            if moddir.is_relative_to(ROOT / root):
                out[moddir.relative_to(ROOT / root).as_posix()] = mid
    return out


@cache
def _id_patterns() -> tuple[re.Pattern[str], re.Pattern[str]]:
    """(ID_RE, SPLIT_ID_RE) built from the module manifests: the ids and the `category/name` directory forms.
    No author or category is named here; a new module is found by its module.toml."""
    names = set(manifests()) | set(_ids_by_dir())
    names = {n for n in names if "/" in n}
    firsts = sorted({n.split("/", 1)[0] for n in names})
    alt = "|".join(re.escape(n) for n in sorted(names, key=lambda n: (-len(n), n)))
    first = "|".join(re.escape(f) for f in firsts) or "(?!)"
    # a path built from parts: ROOT / "modules" / "machines" / "pluck"
    split = re.compile(rf"""["']({first})["']\s*[/,]\s*["']([a-z0-9][a-z0-9-]*)["']""")
    full = re.compile(rf"(?<![\w-])(?:{alt})(?![\w-])" if alt else "(?!)")
    return full, split


def refs(text: str) -> set[str]:
    """Module ids named in TEXT (comment lines of a TOML file are not read)."""
    lines = (line for line in text.splitlines() if not line.lstrip().startswith("#"))
    body = "\n".join(lines)
    id_re, split_re = _id_patterns()
    found = set(id_re.findall(body)) | {f"{a}/{b}" for a, b in split_re.findall(body)}
    ids = manifests()
    by_dir = _ids_by_dir()
    return {m if m in ids else by_dir[m] for m in found if m in ids or m in by_dir}


# The scenario runners (tools/patchkit/pk_digikit_*.py) name modules in code, not only in the scenario
# TOMLs (test/dyn-guard-nan in the debug runner). A module a runner names selects the kinds the runner
# serves; a runner not listed here serves every kind.
RUNNER_KINDS: dict[str, tuple[str, ...]] = {
    "pk_digikit_debug.py": ("card", "debug", "crash"),
    "pk_digikit_upload.py": ("upload",),
    "pk_digikit_pages.py": ("pages",),
    "pk_digikit_fx.py": ("fx",),
    "pk_digikit_fxsend.py": ("fxsend",),
    "pk_digikit_fxroute.py": ("fxroute",),
    "pk_digikit_fxroutes.py": ("fxroutes",),
    "pk_digikit_settings.py": ("settings",),
    "pk_digikit_fxpage.py": ("fxpage",),
    "pk_digikit_fxpersist.py": ("fxpersist",),
    "pk_digikit_fxshift.py": ("fxshift",),
    "pk_digikit_capacity.py": ("debug",),
    "pk_digikit_memaudit.py": ("debug",),
    "pk_digikit_cost.py": ("debug",),
    "pk_dkmenu.py": ("menu",),
}


def code_refs(text: str) -> set[str]:
    """Module ids in the string literals of Python TEXT, docstrings left out (examples in a
    docstring name modules the code never builds)."""
    try:
        tree = ast.parse(text)
    except SyntaxError:
        return refs(text)
    skip: set[int] = set()
    for node in ast.walk(tree):
        if isinstance(
            node, ast.Module | ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef
        ):
            first = node.body[0] if node.body else None
            if isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant):
                skip.add(id(first.value))
    found: set[str] = set()
    for node in ast.walk(tree):
        if (
            isinstance(node, ast.Constant)
            and isinstance(node.value, str)
            and id(node) not in skip
        ):
            found |= refs(node.value)
    return found


@cache
def pk_refs(name: str) -> frozenset[str]:
    """Module ids named in tools/patchkit/NAME.py (empty when there is no such file)."""
    path = ROOT / "tools/patchkit" / f"{name}.py"
    return (
        frozenset(refs(path.read_text(encoding="utf-8")))
        if path.is_file()
        else frozenset()
    )


def runner_kinds(hit: set[str]) -> set[str]:
    """The scenario kinds whose runner code names a module of HIT."""
    kinds: set[str] = set()
    runners = [
        *(ROOT / "tools/patchkit").glob("pk_digikit_*.py"),
        ROOT / "tools/patchkit/pk_dkmenu.py",
    ]
    for path in sorted(runners):
        if pk_refs(path.stem) & hit:
            kinds |= set(RUNNER_KINDS.get(path.name, tuple(kind_files()) + ("pages",)))
    return kinds


def module_test_files(mid: str | None = None) -> list[str]:
    """The pytest files of the modules' own tests (`modules/<group>/<name>/tests/**/test_*.py`), repo-relative:
    those of module MID, or of every module. Found by globbing; no list names them."""
    base = modroots.module_dir(mid) / "tests" if mid else None
    found = (
        sorted(base.rglob("test_*.py"))
        if base is not None
        else sorted(ROOT.glob(f"{MODULE_TESTS_GLOB}/**/test_*.py"))
    )
    return [
        p.relative_to(ROOT).as_posix() for p in found if "node_modules" not in p.parts
    ]


def module_scenario_files(mid: str) -> list[str]:
    """The scenario files of module MID (`tests/scenarios/*.toml` in its directory), repo-relative."""
    base = modroots.module_dir(mid) / "tests/scenarios"
    return [p.relative_to(ROOT).as_posix() for p in sorted(base.glob("*.toml"))]


@cache
def test_files() -> tuple[str, ...]:
    """Every tests/**/test_*.py and every module-owned test_*.py, repo-relative (node_modules skipped)."""
    return tuple(
        rel
        for rel in (
            *(
                p.relative_to(ROOT).as_posix()
                for p in sorted((ROOT / "tests").rglob("test_*.py"))
            ),
            *module_test_files(),
        )
        if "node_modules" not in rel
    )


@cache
def test_refs() -> dict[str, frozenset[str]]:
    """Pytest file -> the module ids it names."""
    out: dict[str, frozenset[str]] = {}
    for rel in test_files():
        if rel.startswith("tests/devtools/"):  # the routing tests name modules as data
            continue
        text = (ROOT / rel).read_text(encoding="utf-8")
        found = refs(text).union(
            *(pk_refs(n) for n in set(re.findall(r"\bpk_[a-z0-9_]+\b", text)))
        )
        if found:
            out[rel] = frozenset(found)
    return out


@cache
def kind_files() -> dict[str, tuple[str, ...]]:
    """Scenario kind -> the tests/digikit files that run it (their `cases((kinds), device)` call)."""
    out: dict[str, list[str]] = {}
    for path in sorted((ROOT / "tests/digikit").glob("test_*.py")):
        match = re.search(r"cases\(\(([^)]*)\)", path.read_text(encoding="utf-8"))
        if not match:
            continue
        for kind in re.findall(r'"([a-z]+)"', match.group(1)):
            out.setdefault(kind, []).append(path.relative_to(ROOT).as_posix())
    return {kind: tuple(files) for kind, files in out.items()}


@cache
def scenario_refs() -> dict[str, tuple[str, frozenset[str]]]:
    """Scenario file -> (kind, module ids it names). A module's own scenarios (modules/*/*/tests/scenarios) belong to
    their module whether or not the file names it."""
    out: dict[str, tuple[str, frozenset[str]]] = {}
    paths = [
        *sorted((ROOT / "tests/digikit/scenarios").glob("*.toml")),
        *sorted(ROOT.glob(f"{MODULE_TESTS_GLOB}/scenarios/*.toml")),
    ]
    for path in paths:
        text = path.read_text(encoding="utf-8")
        kind = tomllib.loads(text).get("scenario", {}).get("kind", "")
        ids = refs(text)
        owner = modroots.module_id(path)
        if owner is not None and path.is_relative_to(ROOT / "modules"):
            ids.add(owner)
        out[path.relative_to(ROOT).as_posix()] = (kind, frozenset(ids))
    return out


def scenario_tests(kinds: set[str]) -> list[str]:
    """The digikit test files that run scenarios of KINDS."""
    files = {f for k in kinds for f in kind_files().get(k, ())}
    return sorted(files)


def own_module_tests(mid: str) -> list[str]:
    """Pytest paths of the tests inside module MID's directory: its test_*.py and the digikit test files that run
    the kinds of its scenarios."""
    kinds = {
        scenario_refs()[f][0]
        for f in module_scenario_files(mid)
        if f in scenario_refs()
    }
    return [*module_test_files(mid), *scenario_tests(kinds)]


def module_tests(mid: str) -> list[str]:
    """Pytest paths for a change in module MID (see the module docstring)."""
    hit = dependents({mid})
    if hit & FRAMEWORK:
        return [
            *ALL_SUITES,
            *module_test_files(),
            *([CONFORMANCE] if mid in CONFORMANCE_WRAPPERS else []),
        ]
    # the module's own tests (and those of the modules that depend on it), then the tests that name it
    tests = list(dict.fromkeys(t for m in sorted(hit) for t in own_module_tests(m)))
    tests += [t for t, ids in test_refs().items() if ids & hit and t not in tests]
    if any(pk_conform.is_machine_module(m) for m in hit) and CONFORMANCE not in tests:
        tests.append(CONFORMANCE)
    if any(pk_conform.is_fx_module(m) for m in hit) and FX_CONFORMANCE not in tests:
        tests.append(FX_CONFORMANCE)
    kinds = {
        kind for kind, ids in scenario_refs().values() if ids & hit
    } | runner_kinds(hit)
    tests += [t for t in scenario_tests(kinds) if t not in tests]
    return tests


def web_reads(rel: str) -> bool:
    """Whether the web gallery bundle can read RELATIVE (to the module directory): the manifest,
    built objects, registry tables and the gallery files, not sources, the SDK, smoke tests."""
    top = rel.split("/", 1)[0]
    return top not in ("src", "sdk", "tests") and not rel.endswith(".md")


def module_fine(path: str) -> Fine | None:
    root = next((r for r in modroots.REPO_ROOTS if path.startswith(r + "/")), None)
    if root is None or "/" not in path[len(root) + 1 :]:
        return None
    if path.endswith(".md"):
        return Fine("modules docs", (), ())
    mid = modroots.module_id(path)
    inside = (ROOT / path).relative_to(modroots.module_dir(mid)).parts if mid else ()
    if inside[:1] == ("tests",):
        # a module's own test or scenario: only the tests read it (no Rust, no web bundle)
        steps = ("python-tests", "host") if path.endswith(".ts") else ("python-tests",)
        return Fine(f"module {mid} tests", steps, tuple(own_module_tests(mid)))
    if mid is None:
        # a file outside any module, or a module that is gone: stay broad
        return Fine(
            "modules (unknown module)",
            ("python-tests", "rust-tests", "web"),
            ALL_SUITES,
        )
    steps = ["python-tests", "rust-tests"]
    if web_reads((ROOT / path).relative_to(modroots.module_dir(mid)).as_posix()):
        steps.append("web")
    label = f"module {mid}" + (" (framework)" if mid in FRAMEWORK else "")
    return Fine(label, tuple(steps), tuple(module_tests(mid)))


def importers(stem: str) -> list[str]:
    """tools/device modules that import tools.device.STEM, and the tests that import it."""
    pat = re.compile(
        rf"^\s*(?:from\s+tools\.device\s+import\s+[^\n]*\b{stem}\b"
        rf"|from\s+tools\.device\.{stem}\s+import|import\s+tools\.device\.{stem}\b)",
        re.M,
    )
    users = [
        p.relative_to(ROOT).as_posix() for p in sorted(ROOT.glob("tools/device/*.py"))
    ]
    return [
        rel
        for rel in [*users, *test_files()]
        if rel != f"tools/device/{stem}.py"
        and pat.search((ROOT / rel).read_text(encoding="utf-8"))
    ]


def device_fine(path: str) -> Fine | None:
    if not (path.startswith("tools/device/") and path.endswith(".py")):
        return None
    stem = path.rsplit("/", 1)[-1].removesuffix(".py")
    seen, todo = {stem}, [stem]
    tests: list[str] = []
    while todo:  # the importers of the importers, inside tools/device
        cur = todo.pop()
        own = ROOT / f"tests/digikit/test_{cur}.py"
        if own.is_file():
            tests.append(own.relative_to(ROOT).as_posix())
        for user in importers(cur):
            if user.startswith("tests/"):
                tests.append(user)
            else:
                name = user.rsplit("/", 1)[-1].removesuffix(".py")
                if name not in seen:
                    seen.add(name)
                    todo.append(name)
    kinds = {k for s in seen for k in DEVICE_KINDS.get(s, ())}
    if kinds or stem not in DEVICE_KINDS:
        tests += scenario_tests(kinds) if stem in DEVICE_KINDS else list(ALL_SUITES)
    ordered = list(dict.fromkeys(tests))
    return Fine(f"tools/device/{stem}", ("python-tests",), tuple(ordered))


def digikit_fine(path: str) -> Fine | None:
    if path.startswith("tests/digikit/scenarios/") and path.endswith(".toml"):
        kind = scenario_refs().get(path, ("", frozenset()))[0]
        if not kind:  # deleted or unreadable: the whole suite
            return Fine(
                "tests/digikit/scenarios", ("python-tests",), ("tests/digikit",)
            )
        return Fine(
            f"scenario kind {kind}", ("python-tests",), tuple(scenario_tests({kind}))
        )
    name = path.rsplit("/", 1)[-1]
    for suite in ALL_SUITES:
        if (
            path.startswith(suite + "/")
            and path.count("/") == 2
            and name.startswith("test_")
            and name.endswith(".py")
            and (ROOT / path).is_file()
        ):
            return Fine(f"{suite}/{name}", ("python-tests",), (path,))
    return None


PATCHKIT_SRC = "packages/patchkit/src/"
# packages/patchkit/src groups: (label, files, steps, test files). A file of the crate in no group keeps the
# wide route. `smoke` in the steps keeps the smoke matrix; the groups without it skip it (patchkit_no_smoke).
PATCHKIT_GROUPS: tuple[
    tuple[str, tuple[str, ...], tuple[str, ...], tuple[str, ...]], ...
] = (
    (
        "patchkit/checks",
        ("build/sharc_legal.rs", "dspscan.rs"),
        ("rust-tests", "python-tests"),
        (
            "tests/patchkit/test_dsp_guard.py",
            "tests/patchkit/test_dsp_guard_contract.py",
            "tests/patchkit/test_pk_dsp_budget.py",
            "tests/patchkit/test_pk_sharc.py",
        ),
    ),
    (
        "patchkit/build",
        ("build_cli.rs", "dko.rs", "link.rs", "abigen.rs", "modgen.rs", "explain.rs"),
        ("rust-tests", "python-tests", "smoke"),
        (
            "tests/patchkit/test_patchkit_cmodules.py",
            "tests/patchkit/test_patchkit_rust.py",
            "tests/patchkit/test_patchkit_dkrt.py",
            "tests/patchkit/test_patchkit_registry.py",
            "tests/patchkit/test_patchkit_expansion.py",
            "tests/patchkit/test_patchkit_dkrt_machine.py",
            "tests/patchkit/test_patchkit_dkrt_boot.py",
            "tests/patchkit/test_pk_cf.py",
            "tests/patchkit/test_dsplink.py",
            "tests/patchkit/test_dyn_fx.py",
            "tests/patchkit/test_health.py",
            "tests/patchkit/test_fx*.py",
            "tests/patchkit/test_digikit_menu.py",
            "tests/patchkit/test_modcheck_needs.py",
            "tests/modules",
        ),
    ),
    (
        "patchkit/flash",
        ("flash.rs", "flash_rpc.rs", "flash_cli.rs"),
        ("rust-tests", "web"),
        (),
    ),
    ("patchkit/wasm", ("wasm.rs",), ("rust-tests", "web"), ()),
    (
        "patchkit/codegen",
        ("sdkgen.rs", "devgen.rs", "fwgen.rs", "desc.rs", "devdata.rs"),
        ("rust-tests", "python-tests"),
        (
            "tests/patchkit/test_pk_gen_include.py",
            "tests/patchkit/test_pk_new.py",
            "tests/patchkit/test_pk_conform.py",
            "tests/patchkit/test_pk_types.py",
        ),
    ),
)
# Files of build/ outside the checks group that the build group takes; build/mod.rs stays on the wide route.
PATCHKIT_BUILD_DIR_WIDE = ("build/mod.rs",)


def patchkit_fine(path: str) -> Fine | None:
    if not (path.startswith(PATCHKIT_SRC) and path.endswith(".rs")):
        return None
    rel = path.removeprefix(PATCHKIT_SRC)
    for label, files, steps, tests in PATCHKIT_GROUPS:
        in_build = (
            label == "patchkit/build"
            and rel.startswith("build/")
            and rel not in PATCHKIT_BUILD_DIR_WIDE
            and not any(rel in g[1] for g in PATCHKIT_GROUPS if g[0] != label)
        )
        if rel in files or in_build:
            found = [
                p.relative_to(ROOT).as_posix()
                for t in tests
                for p in sorted(ROOT.glob(t))
            ]
            return Fine(label, steps, tuple(found))
    return None


def patchkit_no_smoke(path: str) -> str | None:
    """The label of PATH's patchkit group when it needs no smoke matrix, else None."""
    found = patchkit_fine(path)
    return found.label if found and "smoke" not in found.steps else None


def runner_users(stem: str) -> list[str] | None:
    """The modules of tools/patchkit that import the runner STEM. None when the dispatcher
    pk_digikit_scen uses it outside its one-line wrapper for the kind."""
    wrapper = re.compile(
        rf"    from tools\.patchkit import {stem} as \w+\n\n    \w+\.run_\w+\(c\)\n"
    )
    uses = re.compile(
        rf"from tools\.patchkit import [^\n]*\b{stem}\b|from tools\.patchkit\.{stem} import"
    )
    users: list[str] = []
    for path in sorted((ROOT / "tools/patchkit").glob("*.py")):
        if path.stem == stem:
            continue
        text = path.read_text(encoding="utf-8")
        if path.stem == "pk_digikit_scen":
            text = wrapper.sub("", text)
        if uses.search(text):
            if path.stem == "pk_digikit_scen":
                return None
            users.append(path.stem)
    return users


def runner_fine(path: str) -> Fine | None:
    """A scenario runner (RUNNER_KINDS) selects the digikit tests of the kinds it serves and of the
    runners that import it. A user that is no listed runner, a kind no test runs: the wide route."""
    name = path.rsplit("/", 1)[-1]
    if path != f"tools/patchkit/{name}" or name not in RUNNER_KINDS:
        return None
    stem = name.removesuffix(".py")
    seen, todo = {stem}, [stem]
    kinds: set[str] = set()
    while todo:
        cur = todo.pop()
        if f"{cur}.py" not in RUNNER_KINDS:
            return None
        kinds |= set(RUNNER_KINDS[f"{cur}.py"])
        users = runner_users(cur)
        if users is None:
            return None
        todo += [u for u in users if u not in seen]
        seen.update(users)
    if not kinds <= set(kind_files()):
        return None
    return Fine(
        f"scenario runner {name}", ("python-tests",), tuple(scenario_tests(kinds))
    )


def fine(path: str) -> Fine | None:
    """The finer route of PATH, or None to use the prefix rules of verify.py."""
    return (
        module_fine(path)
        or device_fine(path)
        or digikit_fine(path)
        or patchkit_fine(path)
        or runner_fine(path)
    )


# ---- mise.toml ---------------------------------------------------------------------------------


def run_lines(task: dict) -> list[str]:
    run = task.get("run", [])
    return [run] if isinstance(run, str) else [str(x) for x in run]


def paths_of_run(lines: list[str]) -> list[str] | None:
    """The repo paths a task's run lines name: `-m a.b.c` (a/b/c.py or a package), `node FILE`,
    test paths of `-m pytest`, pnpm and npx (the web workspace). None when a line names none."""
    found: list[str] = []
    for line in lines:
        try:
            words = shlex.split(line)
        except ValueError:
            return None
        mine: list[str] = []
        for i, word in enumerate(words):
            if word == "-m" and i + 1 < len(words) and words[i + 1] != "pytest":
                base = words[i + 1].replace(".", "/")
                for cand in (f"{base}.py", f"{base}/__init__.py"):
                    if (ROOT / cand).is_file():
                        mine.append(cand)
                        break
            elif word in ("pnpm", "npx"):
                mine.append("apps/web/package.json")
            elif (ROOT / word).exists() and "/" in word:
                mine.append(word)
        if not mine:
            return None
        found += mine
    return found


def mise_paths(
    old: str | None, new: str | None
) -> tuple[list[str], dict[str, str]] | None:
    """Repo paths to route in place of a changed mise.toml, with the task each came from; None
    when the change is broad: a section other than [tasks], a verify/build/gen/setup task, a task
    key other than run/depends/description/alias, a run that names no known file, a bad TOML."""
    if old is None or new is None:
        return None
    try:
        before, after = tomllib.loads(old), tomllib.loads(new)
    except tomllib.TOMLDecodeError:
        return None
    tasks_old, tasks_new = before.pop("tasks", {}), after.pop("tasks", {})
    if before != after:
        return None
    paths: list[str] = []
    origin: dict[str, str] = {}
    for name in sorted(set(tasks_old) | set(tasks_new)):
        if tasks_old.get(name) == tasks_new.get(name):
            continue
        if name.split(":", 1)[0] in BROAD_TASK_GROUPS:
            return None
        task = tasks_new.get(name)
        if task is None:  # a removed task: only the path check reads it
            continue
        other = {
            k: v
            for k, v in task.items()
            if k not in ("run", "depends", "description", "alias")
        }
        if other != {
            k: v for k, v in (tasks_old.get(name) or {}).items() if k in other
        }:
            return None
        lines = run_lines(task)
        for dep in task.get("depends", []):
            lines += run_lines(tasks_new.get(dep, {}))
        named = paths_of_run(lines)
        if named is None:
            return None
        for path in named:
            origin.setdefault(path, f"mise.toml task {name}")
            paths.append(path)
    return list(dict.fromkeys(paths)), origin
