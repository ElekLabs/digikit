#!/usr/bin/env python3
"""Check the layering of the modules (docs/ARCHITECTURE.md): who may include, link and depend on what.

    uv run python -m tools.devtools.layercheck [--root DIR]        (mise run check:layers)

It reads manifests, sources and the built objects of the repository; it builds nothing; for the built objects of content modules it asks patchkit where the objects of this tree's sources are stored (`patchkit build --paths`).
Every violation prints as `file:line: rule: what is wrong. Instead: what to do.` The exit status is 1 when there is one.

Rules:

  roles     Each image module has `role` in [module], and the role fits its runtime/, services/ or image-modules/ folder.
  sources   Content modules (modules/ and tests/fixtures/modules) and apps include only
            their own files and the SDK (sdk/include, sdk/dsp/contract); a fixture may also include the source of another fixture (a variant of its base). Apps may also include dkrt.h (runtime) and dk_ui_core.h
            (core/ui). Only the runtime's platform file, the installer and adapters include the firmware bindings (dk_fw_* headers, the dkfw crate).
            C, ColdFire and SHARC assembly (`#include`, `.include`) and Rust (`dkfw`, path dependencies) are read.
  objects   The undefined symbols of the built objects of apps and content modules are SDK imports, dkrt_*, dk_ui_*,
            dk_app_* or the SHARC externs the SDK documents. A symbol never names a firmware anchor, and `[object.externs]`
            never maps one to an anchor.
  spans     No content module and no app uses `DK_SPAN_BEGIN` or `DK_SPAN_END` (<dk/core/trace.h>, dk_trace.inc). A span is a measurement the owner of a kind's
            dispatcher takes around its per-row call: the runtime's services and the image's stub (adapters) use them, and the runtime measures every module that way.
  apps      An app has no `[[op]]` and no `[[claim]]`: it never patches the firmware.
  self-contained
            No tracked file outside a user content module's own directory (modules/<kind>/AUTHOR/NAME) names its id
            (`AUTHOR/NAME`) or its path (`<kind>/AUTHOR/NAME`). Allowed: docs/**, any *.md, .agents/**, the fixtures
            (tests/fixtures/modules) and a module with `host = true` in [module] (the workbench speaks
            its protocol).
  arrows    Inside the runtime crate (runtime/core/src/rs/src), a service (services/NAME) names no other service folder and no core
            module except `crate::core_api` (the interface to the core: CoreApi, RealCore, FakeCore and the few core functions it re-exports); pure types
            come from the crate dkrt-abi. The core reaches a service only through `package/handlers.rs` (`exports`) and the generated table `package/gen_services.rs`. A service's own
            host tests (`tests.rs` or a `tests/` folder in the service) may name core modules: they drive the walkers and the planner over the service.
  service-crates
            A service crate (services/NAME, linked into the runtime object) depends only on dkrt-abi, dkrt-sdk and dkneeds, and on
            dkrt-test-support and dkrt-svc-rt (the glue of its own service file, which the tests of the file compile) as dev-dependencies; it names no firmware crate and no device id (only its `dks/` service file may name `dkfw`, in `dks/src/fw.rs`), and has no path into the runtime crate. The runtime
            names a service crate (`dkrt_svc_*`) only in `package/gen_services.rs`, `package/handlers.rs` and the host-test files under `package/handlers/`. Final state (R12-1): no service is a folder of the
            runtime crate's `services/`; every service (fx, machines, samples, lfo) is a crate.
  kind-free The runtime's own code (the runtime crate's and the abi crate's sources without their host tests, inline `#[cfg(test)] mod x { }` blocks included, `package/handlers.rs`, the host-test files in `package/handlers/` and the generated `package/gen_services.rs`) names no machine or effect: no builder
            call (the `provided` imports of the services machines and fx in sdk/sdk.toml, but for those with `wrapper = "fw"`: `dk_module_ctx_add_machine`, `dk_page_add_param` ...), no record kind of a machine or an effect (`KIND_MACHINE`, `KIND_PARAM`, `KIND_PAGE`,
            `KIND_ACTION`, `KIND_NATIVE_PAGE*`, `KIND_EFFECT`, `KINDS_MACHINES`, `KINDS_FX`), no table of an image adapter that only a service reads (`dynreg_info`, `machines_info`,
            `fxpages_info`, `mf_native_*`) and no state of the machines registry (`RegState`, `DKRT_REG`, `DKRT_ASSIGN`). The services own them: the machines and fx service files
            export the builder calls and write their records in `Records::commit`; the loader only runs the generic part (docs/plans/runtime-slimming.md, RS-C).
  platform  Inside the runtime crate, only `platform.rs` (the real platform, L1) and `fw.rs` at the crate's source root (the declared adapter of
            `dkrt_abi::fw::Fw`: it must contain `impl Fw for`) name the firmware crate dkfw; the rest of the core and the services use the `Platform`
            trait and the `Fw` trait (`dkrt-abi`). No runtime file but `platform.rs` compares the device id with a number
            (`device() == 1`): the code asks the device profile (`samples_in_store()`), whose values come from the device files. The runtime
            module patches the firmware only with the machine page hooks (finding 128: runtime-owned, with slots); the UI loop, main-OS task and MIDI
            receive hooks and the exception hook install are in the adapter `core/platform`.
  boundary-hook, boundary-table, boundary-abi
            The resident core (the runtime crate outside `package/`) reaches package code only through `hooks.rs`; package code reaches resident code only through the facade
            `package/resident.rs` and its folder, which call the resident table (`dkrt_abi::resident`) and name no resident module; a type that crosses is in the abi crate
            (docs/plans/resident-core-and-system-package.md, section 4). What the rule does not see: a `use package::...;` alias line of `lib.rs` (the crate-root name of the
            package's loader that `optional_imports!` expands to; RC2s-2 drops it with the package crate), items inside inline `mod name { }` blocks and impls (only the
            declarations at the top level of a file count as module items), a module named by `#[path]` outside `lib.rs` (its file name is read as its module name), and a path built
            by a macro. Host-test files and every item behind `#[cfg(test)]` are not read. `BOUNDARY_ENFORCED` switches `check()` on, `--boundary-report` lists the crossings at any time.
  imports   Every function of <dk/platform/ui.h> is an `[[import]]` of sdk/sdk.toml with `requires = "core/ui"`, and every import
            with `requires` is in the runtime's `optional` list, so the runtime can link it by name (or hand out its stub).

Exceptions are named below with their reason; a new one needs a reason in this file.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

from tools.patchkit.modroots import CORE_ROOTS, REPO_ROOTS

ROOT = Path(__file__).resolve().parents[2]

# The include search path of the module builds: the tool's own copy, whatever tree is checked.
PINS = ROOT / "tools/patchkit/toolchain/pins.toml"
# The moved folders keep their manifest roles.
FOLDER_ROLES = {
    "runtime/core": "runtime",
    "runtime/platform/expansion": "installer",
    "runtime/platform": "adapter",
    "runtime/apps": "app",
    "image-modules/adapters": "adapter",
    "image-modules/fixes": "adapter",
    "image-modules/points": "adapter",
    "image-modules/proofs": "adapter",
}
# Content modules that patch the firmware themselves (they have [[op]] and anchor externs by design: patchkit's own
# example of a hook; the test service takes a raw hook like a core service file does). They are exempt from the objects and apps rules.
FIRMWARE_PATCH_MODULES = {"test/registry-demo", "test/svc-test"}
# Headers of the core layers an app may include.
APP_HEADERS = {"dkrt.h", "dk_ui_core.h"}
ANCHOR = re.compile(r"^(cf|dsp|hook|pk)\.")
INC_C = re.compile(r'^\s*#\s*include\s*([<"])([^>"]+)[>"]')
INC_ASM = re.compile(r'^\s*\.include\s+"([^"]+)"')
RS_FW = re.compile(r"\bdkfw\s*::|\bextern\s+crate\s+dkfw\b")
EXTERN_DOC = re.compile(r"\.EXTERN\s+([A-Za-z_][A-Za-z0-9_]*)")
SOURCE_SUFFIXES = {".c", ".h", ".s", ".S", ".inc", ".rs"}
SPAN = re.compile(r"\bDK_SPAN_(?:BEGIN|END)\b")


@dataclass(frozen=True)
class Violation:
    where: str  # file:line
    rule: str
    message: str
    instead: str

    def __str__(self) -> str:
        return f"{self.where}: {self.rule}: {self.message}. Instead: {self.instead}."


@dataclass
class Module:
    dir: Path
    manifest: dict
    text: str

    @property
    def mod(self) -> dict:
        return self.manifest["module"]

    @property
    def id(self) -> str:
        return self.mod["id"]


def rel(root: Path, p: Path) -> str:
    try:
        return p.relative_to(root).as_posix()
    except ValueError:
        return p.as_posix()


def line_of(text: str, needle: str, default: int = 1) -> int:
    for n, line in enumerate(text.splitlines(), 1):
        if needle in line:
            return n
    return default


def load_modules(root: Path) -> list[Module]:
    out = []
    for base in REPO_ROOTS:
        for toml in sorted((root / base).rglob("module.toml")):
            text = toml.read_text(encoding="utf-8")
            try:
                manifest = tomllib.loads(text)
            except tomllib.TOMLDecodeError:
                continue  # patchkit reports a broken manifest
            if "module" in manifest and "id" in manifest["module"]:
                out.append(Module(toml.parent, manifest, text))
    return out


def classify(root: Path, m: Module) -> str:
    """Core or content according to its repository root."""
    r = rel(root, m.dir)
    if r.split("/", 1)[0] in CORE_ROOTS:
        return "core"
    return "content"


def check_roles(root: Path, mods: list[Module]) -> list[Violation]:
    out = []
    for m in mods:
        if classify(root, m) != "core":
            continue
        r = rel(root, m.dir)
        parts = r.split("/")
        folder = next((f for f in FOLDER_ROLES if r == f or r.startswith(f + "/")), r)
        want = FOLDER_ROLES.get(folder)
        # The neutral service crate contains a separate module at services/NAME/dks.
        if len(parts) == 3 and parts[0] == "services" and parts[2] == "dks":
            want = "service"
        where = f"{r}/module.toml:{line_of(m.text, 'category', 1)}"
        have = m.mod.get("role")
        if want is None:
            out.append(
                Violation(
                    where,
                    "roles",
                    f"{m.id} sits in {r}, which is no role folder",
                    "move it to a role folder under runtime/, services/ or image-modules/",
                )
            )
        elif have is None:
            out.append(
                Violation(
                    where,
                    "roles",
                    f"{m.id} has no `role` in [module]",
                    f'add role = "{want}" (the role of the folder {folder})',
                )
            )
        elif have != want:
            out.append(
                Violation(
                    f"{r}/module.toml:{line_of(m.text, 'role', 1)}",
                    "roles",
                    f'{m.id} has role = "{have}" but sits in {folder}/, whose role is "{want}"',
                    f'set role = "{want}" or move the module to the folder of role "{have}"',
                )
            )
    return out


class Resolver:
    """Finds the file an include names, with the search path of the module builds (the lists in
    tools/patchkit/toolchain/pins.toml, which `patchkit build` also reads)."""

    def __init__(self, root: Path):
        self.root = root
        pins = tomllib.loads(PINS.read_text(encoding="utf-8"))["include"]
        self.global_dirs = [
            root / d for d in dict.fromkeys([*pins["cf"], *pins["sharc"]])
        ]

    def find(self, name: str, file: Path, mod: Module, quoted: bool) -> Path | None:
        dirs = []
        if quoted:
            dirs += [file.parent]
        dirs += [mod.dir / "src", mod.dir / "build/gen", mod.dir, *self.global_dirs]
        for d in dirs:
            p = d / name
            if p.is_file():
                return p.resolve()
        return None


def sdk_symbols(root: Path) -> tuple[set[str], set[str]]:
    """(the SDK's import names, the SHARC externs that sdk/dsp/contract and sdk/templates document)."""
    imports: set[str] = set()
    sdk = root / "sdk/sdk.toml"
    if sdk.is_file():
        data = tomllib.loads(sdk.read_text(encoding="utf-8"))
        imports = {i["name"] for i in data.get("import", [])}
    externs: set[str] = set()
    for d in ("sdk/dsp/contract", "sdk/templates"):
        for p in sorted((root / d).rglob("*")) if (root / d).is_dir() else []:
            if p.is_file() and p.suffix in {".s", ".inc", ".asm"}:
                externs |= set(
                    EXTERN_DOC.findall(p.read_text(encoding="utf-8", errors="replace"))
                )
    return imports, externs


def module_sources(mod: Module) -> list[Path]:
    out = []
    for sub in ("src", "include"):
        d = mod.dir / sub
        if d.is_dir():
            out += [
                p
                for p in sorted(d.rglob("*"))
                if p.is_file() and p.suffix in SOURCE_SUFFIXES
            ]
    return out


def check_sources(root: Path, mods: list[Module]) -> list[Violation]:
    out = []
    res = Resolver(root)
    fw_dir = (root / "ports/bindings").resolve()
    runtime_inc = (root / "runtime/core/include").resolve()
    adapter_dirs = [
        (root / folder).resolve()
        for folder, role in FOLDER_ROLES.items()
        if role == "adapter"
    ]
    core_dirs = [(root / folder).resolve() for folder in CORE_ROOTS] + [fw_dir]
    runtime_sdk = (root / "runtime/core/sdk").resolve()
    service_sdk = (root / "runtime/core/svc-rt").resolve()
    all_mod_dirs = [(m.dir.resolve(), m) for m in mods]
    fixtures = (root / "tests/fixtures/modules").resolve()
    for m in mods:
        kind = classify(root, m)
        role = m.mod.get("role")
        is_app = kind == "core" and role == "app"
        if kind == "core" and not is_app:
            continue  # installer and adapters may use the firmware bindings; the runtime's own rule is `check_runtime_platform`
        who = (
            "an app"
            if is_app
            else "a service file"
            if role == "service"
            else "a content module"
        )
        own = m.dir.resolve()
        sdk = (root / "sdk").resolve()
        for f in module_sources(m):
            text = f.read_text(encoding="utf-8", errors="replace")
            for n, line in enumerate(text.splitlines(), 1):
                where = f"{rel(root, f)}:{n}"
                if f.suffix == ".rs":
                    if RS_FW.search(line):
                        out.append(
                            Violation(
                                where,
                                "sources",
                                f"{who} ({m.id}) uses the firmware crate dkfw",
                                "call the adapter's API (<dk/...> or an export of core/runtime); the firmware bindings belong to adapters",
                            )
                        )
                    continue
                inc = INC_C.match(line)
                name, quoted = None, False
                if inc:
                    name, quoted = inc.group(2), inc.group(1) == '"'
                else:
                    asm = INC_ASM.match(line)
                    if asm:
                        name, quoted = asm.group(1), True
                if name is None:
                    continue
                target = res.find(name, f, m, quoted)
                if target is None:
                    continue  # a system header, or a file patchkit reports
                if own in target.parents or target.parent == own:
                    continue
                if sdk in target.parents:
                    continue
                base = Path(name).name
                if fw_dir in target.parents:
                    out.append(
                        Violation(
                            where,
                            "sources",
                            f"{who} ({m.id}) includes the firmware binding {name}",
                            "use the SDK (<dk/...>) or move the firmware access into an adapter and call it by an exported function",
                        )
                    )
                elif runtime_inc in target.parents or any(
                    d in target.parents for d in adapter_dirs
                ):
                    if is_app and base in APP_HEADERS:
                        continue
                    out.append(
                        Violation(
                            where,
                            "sources",
                            f"{who} ({m.id}) includes {name}, a header of a core layer",
                            "include only <dk/...>"
                            + (
                                " (an app may add dkrt.h and dk_ui_core.h)"
                                if is_app
                                else ""
                            ),
                        )
                    )
                else:
                    foreign = next(
                        (o for d, o in all_mod_dirs if d in target.parents), None
                    )
                    if (
                        foreign is not None
                        and fixtures in m.dir.resolve().parents
                        and fixtures in foreign.dir.resolve().parents
                    ):
                        continue  # a fixture variant (file-write-b) includes the source of its base fixture
                    out.append(
                        Violation(
                            where,
                            "sources",
                            f"{who} ({m.id}) includes {name}"
                            + (
                                f" of module {foreign.id}"
                                if foreign
                                else " outside its own files and the SDK"
                            ),
                            "include only <dk/...> and the module's own files",
                        )
                    )
        # Rust dependencies
        for cargo in sorted(m.dir.rglob("Cargo.toml")):
            if "target" in cargo.relative_to(m.dir).parts:
                continue
            ctext = cargo.read_text(encoding="utf-8")
            try:
                data = tomllib.loads(ctext)
            except tomllib.TOMLDecodeError:
                continue
            for table in ("dependencies", "dev-dependencies", "build-dependencies"):
                for dep, spec in data.get(table, {}).items():
                    path = spec.get("path") if isinstance(spec, dict) else None
                    target = (cargo.parent / path).resolve() if path else None
                    bad = dep == "dkfw" or (
                        target is not None
                        and target != own
                        and own not in target.parents
                        and any(d in target.parents for d in core_dirs)
                        and target != runtime_sdk
                        and not (role == "service" and target == service_sdk)
                    )
                    if bad:
                        out.append(
                            Violation(
                                f"{rel(root, cargo)}:{line_of(ctext, dep)}",
                                "sources",
                                f"{who} ({m.id}) depends on the core crate {dep}",
                                "depend only on dkrt-sdk (the Rust view of the SDK); firmware bindings belong to adapters",
                            )
                        )
    return out


def check_spans(root: Path, mods: list[Module]) -> list[Violation]:
    """Content modules and apps do not measure spans: the dispatcher of each kind does (docs/plans/sdk.md section 9)."""
    out = []
    for m in mods:
        if classify(root, m) == "core" and m.mod.get("role") != "app":
            continue
        for f in module_sources(m):
            for n, line in enumerate(
                f.read_text(encoding="utf-8", errors="replace").splitlines(), 1
            ):
                if SPAN.search(line):
                    out.append(
                        Violation(
                            f"{rel(root, f)}:{n}",
                            "spans",
                            f"{m.id} uses DK_SPAN_*",
                            "leave the measurement to the service that owns the kind: its dispatcher already takes a span around each call of a module",
                        )
                    )
    return out


def stored_objects(root: Path) -> dict[str, dict[str, Path]] | None:
    """`patchkit build --paths`: the cache entry of each object for the sources of this tree (nothing is built), by module
    directory (the resolved path, as patchkit names it). None when ROOT is not this checkout (the tests' trees) or patchkit
    cannot answer; then a note says the objects of content modules are not checked."""
    from tools.patchkit import modroots, patchkit

    if root.resolve() != modroots.ROOT.resolve():
        return None
    try:
        done = patchkit._build_cmd(["build", "--paths"])
        if done.returncode != 0:
            lines = (done.stderr or done.stdout or "").strip().splitlines()
            raise RuntimeError(lines[0] if lines else f"exit status {done.returncode}")
        data = json.loads(done.stdout)
    except (
        OSError,
        subprocess.SubprocessError,
        patchkit.BuildError,
        RuntimeError,
        ValueError,
    ) as e:
        print(
            f"layercheck: the objects of content modules are not checked: patchkit build --paths failed: {e}",
            file=sys.stderr,
        )
        return None
    return {
        str((root / key).resolve()): {p: Path(f) for p, f in files.items()}
        for key, files in data.items()
    }


def object_path(
    root: Path, m: Module, rel_path: str, stored: dict[str, dict[str, Path]] | None
) -> Path | None:
    """Where the built object `rel_path` of the module is: in the module for `core/*`. For a content module it is the cache entry
    of this tree's sources from `stored` (None when not built for them), or, without `stored`, `out/cache/dko/<id>/<path>`
    (packages/patchkit/src/build/mod.rs)."""
    if classify(root, m) == "core":
        return m.dir / rel_path
    if stored is not None:
        # an entry of other sources (another worktree shares out/) is never read
        return stored.get(str(m.dir.resolve()), {}).get(rel_path)
    return root / "out/cache/dko" / m.id / rel_path


def check_committed_objects(root: Path, mods: list[Module]) -> list[Violation]:
    """Objects of content modules and fixtures are build output (the cache); only `core/*` objects are committed."""
    out = []
    for m in mods:
        if classify(root, m) == "core":
            continue
        for f in sorted(m.dir.rglob("*.dko.json")):
            out.append(
                Violation(
                    f"{rel(root, f)}:1",
                    "objects",
                    f"{m.id} is a content module and has an object file in its tree",
                    "delete it; `patchkit build` and `patchkit pack` keep the objects in out/cache/dko",
                )
            )
    return out


def check_objects(root: Path, mods: list[Module]) -> list[Violation]:
    out = []
    imports, sdk_externs = sdk_symbols(root)
    service_abi = root / "runtime/core/abi/src/core_imports.rs"
    service_imports = (
        set(
            re.findall(r"^\s*fn (\w+)\(", service_abi.read_text(encoding="utf-8"), re.M)
        )
        if service_abi.is_file()
        else set()
    )
    stored = stored_objects(root)
    for m in mods:
        kind = classify(root, m)
        is_app = kind == "core" and m.mod.get("role") == "app"
        if (kind == "core" and not is_app) or m.id in FIRMWARE_PATCH_MODULES:
            continue
        mrel = rel(root, m.dir)
        explicit_imports = {
            name
            for binding in m.manifest.get("service_import", [])
            for name in binding.get("names", [])
            if name not in service_imports
            and not name.startswith(("dkc_", "dk_service_"))
        }
        if m.mod.get("role") == "service":
            explicit_imports.update(service_imports)
        for obj in m.manifest.get("object", []):
            for sym, anchor in (obj.get("externs") or {}).items():
                out.append(
                    Violation(
                        f"{mrel}/module.toml:{line_of(m.text, sym)}",
                        "objects",
                        f"{m.id} maps its symbol {sym} to the firmware anchor {anchor}",
                        "call an SDK import or an adapter export; anchors are resolved only by adapters",
                    )
                )
            path = object_path(root, m, obj.get("path", ""), stored)
            if path is None:
                continue  # not built for these sources: nothing to read
            if not path.is_file():
                continue  # not built: nothing to read
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                continue
            for s in data.get("symbols", []):
                name = s["name"]
                if s.get("section") is not None:
                    continue
                ok = (
                    name in imports
                    or name in explicit_imports
                    or name in sdk_externs
                    or name.startswith(("dkrt_", "dk_ui_", "dk_app_", "dkm_"))
                )
                if ANCHOR.match(name) or not ok:
                    out.append(
                        Violation(
                            f"{rel(root, path)}:1",
                            "objects",
                            f"the object of {m.id} needs the symbol {name}, which is neither a declared import nor a permitted adapter export",
                            "use the SDK, a [[service_import]] declaration or an adapter export; a firmware anchor belongs to an adapter",
                        )
                    )
    return out


def check_apps(root: Path, mods: list[Module]) -> list[Violation]:
    out = []
    for m in mods:
        kind = classify(root, m)
        if kind == "core" and m.mod.get("role") == "app":
            for table in ("op", "claim"):
                if m.manifest.get(table):
                    out.append(
                        Violation(
                            f"{rel(root, m.dir)}/module.toml:{line_of(m.text, f'[[{table}]]')}",
                            "apps",
                            f"the app {m.id} has [[{table}]]: it patches the firmware itself",
                            "let an adapter own the hook and call the app through an optional symbol (dk_app_*)",
                        )
                    )
    return out


UI_FN = re.compile(r"^(?!typedef)[A-Za-z_][A-Za-z0-9_ \*]*?\b(dk_ui_[a-z_]+)\(", re.M)


def check_imports(root: Path) -> list[Violation]:
    """The imports of other modules: <dk/platform/ui.h> against sdk/sdk.toml against the runtime's `optional` list."""
    sdk, ui = root / "sdk/sdk.toml", root / "sdk/include/dk/platform/ui.h"
    if not sdk.is_file():
        return []
    sdk_text = sdk.read_text(encoding="utf-8")
    data = tomllib.loads(sdk_text)
    out = []
    # A service import without `kind` has its prototype generated into <dk/gen/imports.h>; the others are declared by hand in <dk/platform/ui.h>.
    ui_imports = {
        i["name"]
        for i in data.get("import", [])
        if i.get("requires") == "core/ui" and (i.get("kind") or not i.get("service"))
    }
    if ui.is_file():
        ui_text = ui.read_text(encoding="utf-8")
        declared = set(UI_FN.findall(ui_text))
        for n in sorted(declared - ui_imports):
            out.append(
                Violation(
                    f"sdk/include/dk/platform/ui.h:{line_of(ui_text, n + '(')}",
                    "imports",
                    f'{n} is declared in <dk/platform/ui.h> but is no [[import]] of sdk/sdk.toml with requires = "core/ui"',
                    "add the [[import]] (with sig and absent), so a content module can call it",
                )
            )
        for n in sorted(ui_imports - declared):
            name_line = 'name = "' + n + '"'
            out.append(
                Violation(
                    f"sdk/sdk.toml:{line_of(sdk_text, name_line)}",
                    "imports",
                    f"{n} is an import of core/ui but <dk/platform/ui.h> does not declare it",
                    "declare it in <dk/platform/ui.h> or remove the [[import]]",
                )
            )
    runtime = root / RT_MANIFEST
    if runtime.is_file():
        runtime_text = runtime.read_text(encoding="utf-8")
        optional: set[str] = set()
        for o in tomllib.loads(runtime_text).get("object", []):
            optional |= set(o.get("optional", []))
        needed = {i["name"] for i in data.get("import", []) if i.get("requires")}
        for n in sorted(needed - optional):
            out.append(
                Violation(
                    f"{RT_MANIFEST}:{line_of(runtime_text, 'optional')}",
                    "imports",
                    f"{n} is an import of another module but is not in the runtime's optional list",
                    "add it to `optional`, or patchkit cannot link the runtime when that module is missing",
                )
            )
    return out


# Files that may name any module: documentation, agent notes and the fixtures.
SC_ALLOWED_PREFIXES = (
    "docs/",
    ".agents/",
    "tests/fixtures/modules/",
)
# Runtime integration acceptance uses shipped modules as fixtures. Only this test
# and the runtimeAcceptance metadata may name them; runtime implementation may not.
# The device-literal baseline lists file paths, among them module directories.
SC_PATH_LIST = "tools/nx/device-literals.baseline.json"
SC_RUNTIME_TEST = "runtime/core/tests/capacity/inferred/inferred-needs.device.test.ts"
SC_RUNTIME_PROJECT = "runtime/core/tests/capacity/inferred/project.json"
SC_SKIP_DIRS = {".git", "out", "node_modules", "target", "firmware", ".venv", "dist"}
SC_HINT = (
    "move the test or model into the module's tests/ directory, use a fixture module (tests/fixtures/modules), "
    "or write a placeholder (<author/name>) in help text"
)


RT_SRC = "runtime/core/src/rs/src"
RT_CRATE_REF = re.compile(
    r"\bcrate::(?:\{([^}]*)\}|([A-Za-z_]\w*)(?:::([A-Za-z_]\w*))?)"
)
RT_BARE_SERVICES = re.compile(r"(?<![\w:])services::")
ARROW_SERVICE_HINT = (
    "call the core through CoreApi (crate::core_api) and take pure types from dkrt-abi; a service reaches another service through the core "
    "(a handler or a link function), never directly"
)
ARROW_CORE_HINT = "call the service through the Service trait (handlers.rs `Services`) or list its C entry point in `handlers::exports`"


def rust_without_comments(text: str) -> str:
    """The source with `//` comments blanked (line numbers kept)."""
    return re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), text)


def check_runtime_arrows(root: Path) -> list[Violation]:
    """The arrows inside the runtime crate: services see only `core_api` of the core and no other service; the core sees services only in handlers.rs."""
    src = root / RT_SRC
    if not src.is_dir():
        return []
    out: list[Violation] = []
    for f in sorted(src.rglob("*.rs")):
        r = f.relative_to(src)
        text = rust_without_comments(f.read_text(encoding="utf-8"))
        own = r.parts[1] if r.parts[0] == "services" and len(r.parts) > 2 else None
        is_test = r.name == "tests.rs" or "tests" in r.parts
        if own is None and r.parts[0] == "services":
            continue  # services/mod.rs: the list of service folders
        for m in RT_CRATE_REF.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            where = f"{RT_SRC}/{r.as_posix()}:{line}"
            if m.group(1) is not None:
                names = [
                    (n.strip().split("::") + [""])[:2]
                    for n in m.group(1).split(",")
                    if n.strip()
                ]
            else:
                names = [[m.group(2), m.group(3) or ""]]
            for first, second in names:
                if own is not None:
                    if first == "services" and second != own:
                        out.append(
                            Violation(
                                where,
                                "arrows",
                                f"service {own} names {first}::{second or '*'}",
                                ARROW_SERVICE_HINT,
                            )
                        )
                    elif first not in ("services", "core_api") and not is_test:
                        out.append(
                            Violation(
                                where,
                                "arrows",
                                f"service {own} names core module crate::{first}",
                                ARROW_SERVICE_HINT,
                            )
                        )
                elif first == "services" and r.as_posix() not in (
                    "package/handlers.rs",
                    "package/gen_services.rs",
                ):
                    out.append(
                        Violation(
                            where,
                            "arrows",
                            f"the core names crate::services::{second or '*'}",
                            ARROW_CORE_HINT,
                        )
                    )
        if own is None and r.as_posix() not in (
            "package/handlers.rs",
            "package/gen_services.rs",
        ):
            for m in RT_BARE_SERVICES.finditer(text):
                line = text.count("\n", 0, m.start()) + 1
                out.append(
                    Violation(
                        f"{RT_SRC}/{r.as_posix()}:{line}",
                        "arrows",
                        "the core names services::",
                        ARROW_CORE_HINT,
                    )
                )
    return out


# The resident/package boundary (docs/plans/resident-core-and-system-package.md, section 4; RC2s-1 S8). Resident code is the runtime crate outside `package/`; package code is `package/`.
# Resident code reaches package code only through the hooks module `hooks.rs` (the struct the package fills at entry); package code reaches resident code only through
# `package/resident.rs` (the rows of the resident table) and the abi crate. Types that cross the boundary live in dkrt-abi.
# `check()` runs the rules when BOUNDARY_ENFORCED is True; `--boundary-report` lists the crossings any time.
BOUNDARY_ENFORCED = True
BOUNDARY_HOOKS_FILE = "hooks.rs"
BOUNDARY_TABLE_FILE = "package/resident.rs"
BOUNDARY_HOOK_HINT = "call the package through a hook of `hooks.rs` (a function pointer the package fills at entry; null keeps the stock behaviour)"
BOUNDARY_TABLE_HINT = "reach the resident code through a row of the resident table (`package/resident.rs`); the package names no resident module itself"
BOUNDARY_ABI_HINT = "move the type to the abi crate (runtime/core/abi) and name it as `dkrt_abi::...` on both sides"
BOUNDARY_USE = re.compile(r"\buse\s+([^;]+);", re.S)
BOUNDARY_PATH = re.compile(r"(?<![\w:])(crate|package|self|super)((?:::\w+)+)")


def expand_use_tree(tree: str) -> list[list[str]]:
    """The paths of a `use` tree: `a::{b, c::D}` gives [a, b] and [a, c, D]; `self` and `*` end a path."""
    tree = tree.strip()
    brace = tree.find("{")
    if brace < 0:
        return [[x.strip() for x in tree.split(" as ")[0].split("::") if x.strip()]]
    prefix = [x.strip() for x in tree[:brace].split("::") if x.strip()]
    depth, start, parts = 0, brace + 1, []
    for i in range(brace + 1, len(tree)):
        c = tree[i]
        if c == "{":
            depth += 1
        elif c == "}":
            if depth == 0:
                parts.append(tree[start:i])
                break
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(tree[start:i])
            start = i + 1
    out: list[list[str]] = []
    for part in parts:
        if part.strip():
            out += [prefix + p for p in expand_use_tree(part)]
    return out


BOUNDARY_TYPE_KINDS = {"struct", "enum", "type", "trait", "union"}
BOUNDARY_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:(?:const|unsafe|async)[ \t]+)*(?:extern[ \t]+\"[^\"]*\"[ \t]+)?"
    r"(struct|enum|type|trait|union|fn|const|static)[ \t]+(?:mut[ \t]+)?([A-Za-z_]\w*)",
    re.M,
)
BOUNDARY_MOD_LINE = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+\w+[ \t]*;", re.M
)
# `use package::loader;` in lib.rs: the crate-root name `optional_imports!` expands to, not a call.
BOUNDARY_ALIAS_LINE = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?use[ \t]+package::\w+[ \t]*;", re.M
)
BOUNDARY_PATH_MOD = re.compile(
    r"#\[path[ \t]*=[ \t]*\"[^\"]+\"\]\s*(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+(\w+)[ \t]*;"
)


def module_path(rel: Path) -> list[str]:
    """The module path of a source file of the runtime crate: `package/files.rs` is [package, files], `a/mod.rs` is [a], `lib.rs` is []."""
    parts = list(rel.with_suffix("").parts)
    if parts and parts[-1] in ("mod", "lib"):
        parts.pop()
    return parts


def resolve_relative(segs: list[str], mod: list[str]) -> list[str]:
    """Resolve a path from `crate`, `self`, `super` (any chain) or a bare `package` to segments below the crate root; [] when it climbs out of the crate."""
    if not segs:
        return []
    if segs[0] == "crate":
        return segs[1:]
    if segs[0] == "package":
        return segs
    if segs[0] in ("self", "super"):
        base, i = list(mod), 0
        while i < len(segs) and segs[i] in ("self", "super"):
            if segs[i] == "super":
                if not base:
                    return []
                base.pop()
            i += 1
        return base + segs[i:]
    return []


def crate_paths(text: str, mod: list[str] | None = None) -> list[tuple[int, list[str]]]:
    """Every path of `text` that starts with `crate`, `self`, `super` or `package`, as (line, segments below the crate root), from `use` trees and inline paths. A bare `package::` counts as `crate::package::`; `self` and `super` chains resolve against the module path `mod` of the file."""
    mod = mod or []
    found: list[tuple[int, list[str]]] = []

    def add(line: int, path: list[str]) -> None:
        while path and path[-1] in ("self", "*"):
            path = path[:-1]
        if path and path[0] in ("crate", "package", "self", "super"):
            segs = resolve_relative(path, mod)
            if segs:
                found.append((line, segs))

    for m in BOUNDARY_USE.finditer(text):
        line = text.count("\n", 0, m.start()) + 1
        for path in expand_use_tree(m.group(1)):
            add(line, path)
    inline = BOUNDARY_USE.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)
    for m in BOUNDARY_PATH.finditer(inline):
        add(
            inline.count("\n", 0, m.start()) + 1,
            [m.group(1), *[x for x in m.group(2).split("::") if x]],
        )
    return found


BOUNDARY_CFG_TEST = re.compile(r"^\s*#\[cfg\((?:all\()?test[,)]")
BOUNDARY_STRING = re.compile(r'"(?:\\.|[^"\\])*"')
BOUNDARY_CHAR = re.compile(r"'(?:\\.|[^'\\])'")
BOUNDARY_ATTR_LINE = re.compile(r"^\s*#\[")


def braces_of(line: str) -> tuple[int, int]:
    """The `{` and `}` of a line outside string and char literals."""
    line = BOUNDARY_CHAR.sub("''", BOUNDARY_STRING.sub('""', line))
    return line.count("{"), line.count("}")


def without_test_items(lines: list[str]) -> list[str]:
    """The lines with every item behind `#[cfg(test)]` or `#[cfg(all(test, ..))]` blanked: an inline module, a function, an impl, a `use`, a static, also when the attribute
    and the item share a line (line numbers kept)."""
    out = list(lines)
    i = 0
    while i < len(lines):
        if not BOUNDARY_CFG_TEST.match(lines[i]):
            i += 1
            continue
        start = i
        # the item starts after the attribute on this line, or on a following line that is no attribute
        first = lines[i].split("]", 1)[1] if "]" in lines[i] else ""
        j = i
        text = first
        while not text.strip() or BOUNDARY_ATTR_LINE.match(text):
            j += 1
            if j >= len(lines):
                break
            text = lines[j]
        depth, seen = 0, False
        end = j
        while end < len(lines):
            opens, closes = braces_of(lines[end] if end != start else first)
            depth += opens - closes
            seen = seen or opens > 0
            body = lines[end] if end != start else first
            if (seen and depth <= 0) or (
                not seen and depth == 0 and body.rstrip().endswith(";")
            ):
                break
            end += 1
        for k in range(start, min(end, len(lines) - 1) + 1):
            out[k] = ""
        i = end + 1
    return out


def module_level(text: str) -> str:
    """The text with the lines blanked that lie inside braces (the declarations at the top level of a file stay; line numbers are kept)."""
    out, depth = [], 0
    for line in text.split("\n"):
        out.append(line if depth == 0 else "")
        opens, closes = braces_of(line)
        depth += opens - closes
    return "\n".join(out)


def declared_items(src: Path) -> dict[tuple[str, ...], str]:
    """The items declared at the top level of the runtime crate's files (test files and every item behind `#[cfg(test)]` left out; a method, an inline module's item and an item of an
    `extern` block are not module items): module path plus name -> struct, enum, type, trait, union, fn, const or static."""
    items: dict[tuple[str, ...], str] = {}
    for f in sorted(src.rglob("*.rs")):
        r = f.relative_to(src)
        if is_runtime_test_file(r):
            continue
        lines = f.read_text(encoding="utf-8").split("\n")
        text = module_level(rust_without_comments("\n".join(without_test_items(lines))))
        mod = module_path(r)
        for m in BOUNDARY_DECL.finditer(text):
            items.setdefault((*mod, m.group(2)), m.group(1))
    return items


def names_a_type(segs: list[str], items: dict[tuple[str, ...], str]) -> bool:
    """True when the path names a type or something of it (an associated function, an enum variant): some prefix resolves to a type declaration. A path whose prefixes resolve to no declaration (a re-export) is looked up by the name of its segments in the whole crate when that name is declared once."""
    hit = False
    for i in range(1, len(segs) + 1):
        kind = items.get(tuple(segs[:i]))
        if kind in BOUNDARY_TYPE_KINDS:
            return True
        hit = hit or kind is not None
    if hit:
        return False
    for seg in segs:
        kinds = {k for key, k in items.items() if key[-1] == seg}
        if kinds & BOUNDARY_TYPE_KINDS and not kinds - BOUNDARY_TYPE_KINDS:
            return True
    return False


def reexported_names(lib: Path) -> set[str]:
    """The names `lib.rs` brings into the crate root with `use` (`pub use storage::{card, store}` makes `crate::card` and `crate::store`): resident names too. A `use package::...` alias names package code and is left out."""
    if not lib.is_file():
        return set()
    text = rust_without_comments(lib.read_text(encoding="utf-8"))
    names: set[str] = set()
    for m in BOUNDARY_USE.finditer(text):
        for path in expand_use_tree(m.group(1)):
            if (
                path
                and path[0] not in ("package", "crate", "self", "super")
                and not path[0].startswith("dkrt")
                and path[0] not in ("core", "std", "alloc")
            ):
                last = path[-1]
                if last not in ("*", "self"):
                    names.add(last)
    return names


def check_resident_boundary(root: Path) -> list[Violation]:
    """The resident/package boundary: hooks one way, the resident table the other, shared types in the abi crate. Host-test files and inline test modules are not read.
    Exemptions: `hooks.rs` may call package functions and `package/resident.rs` may name resident functions, but neither may name a type of the other side (resident types in the table's signatures come from the abi crate); `lib.rs` is read except its `mod` lines."""
    src = root / RT_SRC
    if not src.is_dir():
        return []
    items = declared_items(src)
    resident = {
        p.stem for p in src.glob("*.rs") if p.stem not in ("lib", "package")
    } | {p.name for p in src.iterdir() if p.is_dir() and p.name != "package"}
    resident |= {k[0] for k in items if len(k) == 1}
    resident |= reexported_names(src / "lib.rs")
    lib = src / "lib.rs"
    if lib.is_file():
        resident |= set(BOUNDARY_PATH_MOD.findall(lib.read_text(encoding="utf-8")))
    out: list[Violation] = []
    for f in sorted(src.rglob("*.rs")):
        r = f.relative_to(src)
        if is_runtime_test_file(r):
            continue
        rp = r.as_posix()
        in_package = r.parts[0] == "package"
        lines = f.read_text(encoding="utf-8").split("\n")
        text = rust_without_comments("\n".join(without_test_items(lines)))
        if rp == "lib.rs":
            for line_re in (BOUNDARY_MOD_LINE, BOUNDARY_ALIAS_LINE):
                text = line_re.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)
        seen: set[tuple[int, str]] = set()
        for line, segs in crate_paths(text, module_path(r)):
            first = segs[0]
            is_type = names_a_type(segs, items)
            if in_package:
                if first not in resident:
                    continue
                crossing = f"package code names crate::{'::'.join(segs)}"
                allowed = rp == BOUNDARY_TABLE_FILE and not is_type
                target = "package"
            else:
                if first != "package":
                    continue
                crossing = f"resident code names crate::{'::'.join(segs)}"
                allowed = rp == BOUNDARY_HOOKS_FILE and not is_type
                target = "resident"
            if allowed or (line, crossing) in seen:
                continue
            seen.add((line, crossing))
            if is_type:
                rule, hint = "boundary-abi", BOUNDARY_ABI_HINT
            elif target == "package":
                rule, hint = "boundary-table", BOUNDARY_TABLE_HINT
            else:
                rule, hint = "boundary-hook", BOUNDARY_HOOK_HINT
            out.append(Violation(f"{RT_SRC}/{rp}:{line}", rule, crossing, hint))
    return out


# L1 files of the runtime crate: the real platform and the adapter of `dkrt_abi::fw::Fw` (`fw.rs` at the source root), the only places that name the
# firmware crate. The adapter file must implement the interface it is declared for.
RT_PLATFORM_FILE = "platform.rs"
RT_ADAPTER_FILE = "fw.rs"
RT_ADAPTER_IMPL = re.compile(r"\bimpl\s+(?:dkrt_abi::fw::)?Fw\s+for\b")
RT_DEVICE_COMPARE = re.compile(
    r"\b(?:device|device_id)\s*\(\s*\)\s*[!=]=\s*\d|\bprofile\s*\(\s*\)\s*\.\s*id\s*[!=]=\s*\d"
)


# The firmware hooks that the runtime module patches in itself: none. Every hook (UI loop, main-OS task entry, MIDI receive) belongs to the platform adapter; the machine
# page hooks are the machines service file's (services/machines/dks).
RT_OWN_HOOKS: set[str] = set()
RT_MANIFEST = "runtime/core/module.toml"


def check_runtime_platform(root: Path) -> list[Violation]:
    """The runtime has no firmware calls outside its L1 files (`platform.rs`, an adapter's `fw.rs`), branches on no device id and patches only its own hooks."""
    src = root / RT_SRC
    if not src.is_dir():
        return []
    out: list[Violation] = []
    manifest = root / RT_MANIFEST
    if manifest.is_file():
        text = manifest.read_text(encoding="utf-8")
        for op in tomllib.loads(text).get("op", []):
            hook = op.get("hook", "")
            if hook and hook not in RT_OWN_HOOKS:
                out.append(
                    Violation(
                        f"{RT_MANIFEST}:{line_of(text, hook)}",
                        "platform",
                        f"the runtime installs the firmware hook {hook} itself",
                        "put the hook and its trampoline in the platform adapter (runtime/platform/platform); the runtime exports the function it calls",
                    )
                )
    for f in sorted(src.rglob("*.rs")):
        r = f.relative_to(src)
        l1 = r.as_posix() in (RT_ADAPTER_FILE, RT_PLATFORM_FILE)
        text = rust_without_comments(f.read_text(encoding="utf-8"))
        if r.name == RT_ADAPTER_FILE and r.as_posix() != RT_ADAPTER_FILE:
            out.append(
                Violation(
                    f"{RT_SRC}/{r.as_posix()}:1",
                    "platform",
                    "an adapter file fw.rs below the source root of the runtime",
                    "the only firmware adapter is src/fw.rs (the implementation of dkrt_abi::fw::Fw)",
                )
            )
        if r.as_posix() == RT_ADAPTER_FILE and not RT_ADAPTER_IMPL.search(text):
            out.append(
                Violation(
                    f"{RT_SRC}/{r.as_posix()}:1",
                    "platform",
                    "fw.rs does not implement dkrt_abi::fw::Fw",
                    "fw.rs is exempt from the dkfw rule only as the declared adapter of that interface",
                )
            )
        for n, line in enumerate(text.splitlines(), 1):
            where = f"{RT_SRC}/{r.as_posix()}:{n}"
            if not l1 and RS_FW.search(line):
                out.append(
                    Violation(
                        where,
                        "platform",
                        "the runtime names the firmware crate dkfw outside platform.rs",
                        "call it through the Platform trait or the Fw trait (dkrt-abi); the implementation of Fw is src/fw.rs",
                    )
                )
            if r.as_posix() != RT_PLATFORM_FILE and RT_DEVICE_COMPARE.search(line):
                out.append(
                    Violation(
                        where,
                        "platform",
                        "the runtime compares the device id outside platform.rs",
                        "ask the device profile (Platform::samples_in_store, sample_path_max ...); add the fact to the device files and `patchkit gen`",
                    )
                )
    return out


SERVICE_CRATES = "services"
# dkrt-svc-rows is the parameter row writers that machines and fx share (no service names the other); dkrt-svc-bitmap the firmware tile objects of lfo
SERVICE_DEPS = {"dkrt-abi", "dkrt-sdk", "dkneeds", "dkrt-svc-rows", "dkrt-svc-bitmap"}
SERVICE_DEV_DEPS = SERVICE_DEPS | {"dkrt-test-support", "dkrt-svc-rt"}
# `dkrt_svc_rt` is the support crate of a service file (runtime/core/svc-rt), not a service
SERVICE_CRATE_REF = re.compile(r"\bdkrt_svc_(?!rt\b)\w+")
SERVICE_RUNTIME_FILES = ("package/handlers.rs", "package/gen_services.rs")
# The services that are folders of the runtime crate (`services/NAME`): none since R12-1; every service is a crate (`services/NAME`).
RT_LOCAL_SERVICES: set[str] = set()
SERVICE_RUNTIME_REF = re.compile(r"\bdkrt::|\bextern\s+crate\s+dkrt\b")
RUNTIME_MANIFEST = "runtime/core/src/rs/Cargo.toml"
DEP_TABLES = ("dependencies", "build-dependencies", "dev-dependencies")


def manifest_deps(data: dict) -> list[tuple[str, str, str, object]]:
    """(table label, key, crate name, spec) of every dependency of a manifest: the three tables and the same tables under `[target.CFG.*]`.
    A renamed dependency (`alias = { package = "dkrt-abi" }`) has its real crate name, not its key."""
    out: list[tuple[str, str, str, object]] = []

    def take(label: str, table: dict) -> None:
        for key, spec in table.items():
            real = spec.get("package", key) if isinstance(spec, dict) else key
            out.append((label, key, real, spec))

    for t in DEP_TABLES:
        take(t, data.get(t, {}))
    for cfg, tables in data.get("target", {}).items():
        for t in DEP_TABLES:
            take(f"target.{cfg}.{t}", tables.get(t, {}))
    return out


def check_test_only_deps(
    root: Path, manifest: Path, data: dict, text: str
) -> list[Violation]:
    """dkrt-test-support and the `fake` feature of dkrt-link are for tests: dev-dependencies only."""
    out = []
    for label, key, real, spec in manifest_deps(data):
        if label.endswith("dev-dependencies"):
            continue
        fake = (
            real == "dkrt-link"
            and isinstance(spec, dict)
            and "fake" in spec.get("features", [])
        )
        if real == "dkrt-test-support" or fake:
            what = (
                "dkrt-test-support"
                if real == "dkrt-test-support"
                else "the fake feature of dkrt-link"
            )
            out.append(
                Violation(
                    f"{rel(root, manifest)}:{line_of(text, key)}",
                    "service-crates",
                    f"{what} is a [{label}] entry",
                    "test fakes may be dev-dependencies of the runtime crate and of services only; they never reach the runtime object",
                )
            )
    return out


def check_service_crates(root: Path) -> list[Violation]:
    """Service crates reach the runtime only through dkrt-abi, and the runtime reaches them only through its service table and `handlers::exports`."""
    out: list[Violation] = []
    base = root / SERVICE_CRATES
    for manifest in sorted(base.glob("*/Cargo.toml")) if base.is_dir() else []:
        text = manifest.read_text(encoding="utf-8")
        data = tomllib.loads(text)
        for label, key, real, _ in manifest_deps(data):
            allowed = (
                SERVICE_DEV_DEPS if label.endswith("dev-dependencies") else SERVICE_DEPS
            )
            if real not in allowed:
                alias = f" (as {key})" if key != real else ""
                out.append(
                    Violation(
                        f"{rel(root, manifest)}:{line_of(text, key)}",
                        "service-crates",
                        f"the service crate depends on {real}{alias} ([{label}])",
                        f"a service crate depends only on {', '.join(sorted(allowed))}; reach the core through CoreApi (dkrt-abi), and put test fakes in dkrt-test-support",
                    )
                )
        out.extend(check_test_only_deps(root, manifest, data, text))
        for f in sorted(manifest.parent.rglob("*.rs")):
            if "target" in f.relative_to(manifest.parent).parts:
                continue
            for n, line in enumerate(
                rust_without_comments(f.read_text(encoding="utf-8")).splitlines(), 1
            ):
                where = f"{rel(root, f)}:{n}"
                # the service file (`dks/`) is where a service names firmware (`dks/src/fw.rs`); the neutral crate does not
                if (
                    RS_FW.search(line)
                    and "dks" not in f.relative_to(manifest.parent).parts
                ):
                    out.append(
                        Violation(
                            where,
                            "service-crates",
                            "the service crate names the firmware crate dkfw",
                            "use the Platform trait or CoreApi (dkrt-abi)",
                        )
                    )
                if RT_DEVICE_COMPARE.search(line):
                    out.append(
                        Violation(
                            where,
                            "service-crates",
                            "the service crate compares the device id with a number",
                            "ask the device profile through CoreApi or Platform",
                        )
                    )
                if SERVICE_RUNTIME_REF.search(line):
                    out.append(
                        Violation(
                            where,
                            "service-crates",
                            "the service crate names the runtime crate",
                            "take the core as a `&C` argument with `C: CoreApi`",
                        )
                    )
    rt_manifest = root / RUNTIME_MANIFEST
    if rt_manifest.is_file():
        rt_text = rt_manifest.read_text(encoding="utf-8")
        out.extend(
            check_test_only_deps(root, rt_manifest, tomllib.loads(rt_text), rt_text)
        )
    src = root / RT_SRC
    local = src / "services"
    for d in sorted(local.iterdir()) if local.is_dir() else []:
        if d.is_dir() and d.name not in RT_LOCAL_SERVICES:
            out.append(
                Violation(
                    rel(root, d),
                    "service-crates",
                    f"the service {d.name} is a folder of the runtime crate",
                    f"no service is a folder of the runtime crate; put a service in a crate services/{d.name}",
                )
            )
    mod_rs = local / "mod.rs"
    if mod_rs.is_file():
        mod_text = rust_without_comments(mod_rs.read_text(encoding="utf-8"))
        for n, line in enumerate(mod_text.splitlines(), 1):
            m = re.match(r"\s*pub\s+mod\s+(\w+)", line)
            if m and m.group(1) not in RT_LOCAL_SERVICES:
                out.append(
                    Violation(
                        f"{RT_SRC}/services/mod.rs:{n}",
                        "service-crates",
                        f"services/mod.rs declares the service {m.group(1)}",
                        "no service is a folder of the runtime crate; every service is a crate in services",
                    )
                )
    for f in sorted(src.rglob("*.rs")) if src.is_dir() else []:
        r = f.relative_to(src).as_posix()
        if r in SERVICE_RUNTIME_FILES or r.startswith("package/handlers/"):
            continue
        for n, line in enumerate(
            rust_without_comments(f.read_text(encoding="utf-8")).splitlines(), 1
        ):
            if SERVICE_CRATE_REF.search(line):
                out.append(
                    Violation(
                        f"{RT_SRC}/{r}:{n}",
                        "service-crates",
                        "the core names a service crate outside handlers.rs and gen_services.rs",
                        ARROW_CORE_HINT,
                    )
                )
    return out


def kind_free_builders(root: Path) -> list[str]:
    """The builder calls of the services machines and fx: their `provided` imports in the sdk/sdk.toml of `root` (the core does not name them).
    An import with `wrapper = "fw"` (`dk_param_get`, `dk_param_set`) reads a value, it builds nothing."""
    sdk = tomllib.loads((root / "sdk/sdk.toml").read_text(encoding="utf-8"))
    return [
        i["name"]
        for i in sdk["import"]
        if i.get("provided")
        and i.get("service") in ("machines", "fx")
        and i.get("wrapper") != "fw"
    ]


def rt_kind_free(root: Path) -> re.Pattern[str]:
    """What the core does not name any more after RS-C (the machines and fx content): builder calls (from sdk/sdk.toml), their record kinds, the adapters' tables and the registry state.
    The kind names stay literal."""
    return re.compile(
        r"\b(?:"
        + "|".join(kind_free_builders(root))
        + r"|KIND_MACHINE|KIND_PARAM|KIND_PAGE|KIND_ACTION|KIND_NATIVE_PAGE|KIND_NATIVE_PAGE_V2|KIND_EFFECT|KINDS_MACHINES|KINDS_FX"
        r"|dynreg_info|machines_info|fxpages_info|mf_native_\w+|RegState|DKRT_REG|DKRT_ASSIGN)\b"
    )


KIND_FREE_EXEMPT = ("package/handlers.rs", "package/gen_services.rs")
KIND_FREE_HINT = "the machines or the fx service owns it (services/machines, fx): the core runs the generic part and calls the service through `Service` or `CoreApi`"


def is_runtime_test_file(r: Path) -> bool:
    """A host-test file of the runtime crate: `tests.rs`, `*_tests.rs`, a `tests/` folder, the fakes and the test lock."""
    return (
        "tests" in r.parts
        or r.name == "tests.rs"
        or r.name.endswith("_tests.rs")
        or r.name.startswith("fake_")
        or r.name == "test_lock.rs"
    )


def without_test_modules(lines: list[str]) -> list[str]:
    """The lines with the body of each inline `#[cfg(test)] mod name { ... }` block blanked (braces matched; line numbers kept). `#[cfg(test)] mod name;` (a file) blanks nothing:
    the file is a test file or is scanned on its own."""
    out = list(lines)
    i = 0
    while i < len(lines):
        if (
            lines[i].strip() == "#[cfg(test)]"
            and i + 1 < len(lines)
            and re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{", lines[i + 1])
        ):
            depth = 0
            j = i + 1
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                out[j] = ""
                if depth <= 0:
                    break
                j += 1
            out[i] = ""
            i = j
        i += 1
    return out


# The host-test files of `handlers/` (its other files, if any, are scanned).
KIND_FREE_TEST_FILES = (
    "package/handlers/tests.rs",
    "package/handlers/lfo_tests.rs",
    "package/handlers/fx_fixture.rs",
    "package/handlers/dks_decls.rs",
)
# The abi crate is the core's boundary: it too names no machine or effect.
RT_ABI_SRC = "runtime/core/abi/src"


def check_core_kind_free(root: Path) -> list[Violation]:
    """The core names no machine or effect (rule `kind-free`): the runtime crate and the abi crate."""
    out: list[Violation] = []
    pattern = rt_kind_free(root)
    for base in (RT_SRC, RT_ABI_SRC):
        src = root / base
        if not src.is_dir():
            continue
        for f in sorted(src.rglob("*.rs")):
            r = f.relative_to(src)
            if base == RT_SRC and (
                r.as_posix() in KIND_FREE_EXEMPT
                or r.as_posix() in KIND_FREE_TEST_FILES
                or is_runtime_test_file(r)
            ):
                continue
            if base == RT_ABI_SRC and r.as_posix() == "keys.rs":
                continue  # the header table's key groups (`dkneeds::KIND_MACHINE`, `KIND_EFFECT`): the .dkm format the core parses for the planner and the commit
            if base == RT_ABI_SRC and (
                r.name.startswith("gen_") or r.name == "tests.rs" or "tests" in r.parts
            ):
                continue
            lines = without_test_modules(
                rust_without_comments(f.read_text(encoding="utf-8")).splitlines()
            )
            for n, line in enumerate(lines, 1):
                m = pattern.search(line)
                if m:
                    out.append(
                        Violation(
                            f"{base}/{r.as_posix()}:{n}",
                            "kind-free",
                            f"the core names {m.group(0)}",
                            KIND_FREE_HINT,
                        )
                    )
    return out


def tracked_files(root: Path) -> list[str]:
    """Tracked files (`git ls-files`); outside a git repository, every file except build output."""
    try:
        done = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            capture_output=True,
            check=True,
        )
        names = [n for n in done.stdout.decode("utf-8", "replace").split("\0") if n]
        if names:
            return [n for n in names if (root / n).is_file()]
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass
    out = []
    for p in sorted(root.rglob("*")):
        r = p.relative_to(root)
        if p.is_file() and not SC_SKIP_DIRS & set(r.parts):
            out.append(r.as_posix())
    return out


def user_modules(root: Path, mods: list[Module]) -> list[tuple[Module, str]]:
    """(module, relative directory) of every user content module that is not a `host` module."""
    out = []
    for m in mods:
        r = rel(root, m.dir)
        if r.startswith("modules/") and not m.mod.get("host", False):
            out.append((m, r))
    return out


def sc_hits(root: Path, mods: list[Module]) -> list[tuple[str, int, Module, str]]:
    """(file, line, module, module dir) for every mention of a user module outside its directory."""
    users = user_modules(root, mods)
    pats = []
    for m, d in users:
        names = {m.id, d.removeprefix("modules/")}
        pats.append(
            (
                m,
                d,
                re.compile(
                    r"(?<![\w.-])(?:"
                    + "|".join(re.escape(n) for n in sorted(names))
                    + r")(?![\w-])"
                ),
            )
        )
    out = []
    for f in tracked_files(root):
        if (
            f.endswith(".md")
            or f.startswith(SC_ALLOWED_PREFIXES)
            or f in (SC_RUNTIME_TEST, SC_PATH_LIST)
        ):
            continue
        data = (root / f).read_bytes()
        if b"\0" in data[:4096]:
            continue
        text = data.decode("utf-8", "replace")
        if f == SC_RUNTIME_PROJECT:
            acceptance = re.search(r'"runtimeAcceptance"\s*:\s*', text)
            if acceptance:
                start = acceptance.end()
                _, end = json.JSONDecoder().raw_decode(text, start)
                # Preserve line numbers and check all fields outside acceptance.
                text = (
                    text[:start] + re.sub(r"[^\n]", " ", text[start:end]) + text[end:]
                )
        for m, d, pat in pats:
            if f.startswith(d + "/"):
                continue
            for n, line in enumerate(text.splitlines(), 1):
                if pat.search(line):
                    out.append((f, n, m, d))
    return out


def check_self_contained(root: Path, mods: list[Module]) -> list[Violation]:
    return [
        Violation(f"{f}:{n}", "self-contained", f"{m.id} named outside {d}", SC_HINT)
        for f, n, m, d in sc_hits(root, mods)
    ]


def check(root: Path = ROOT, boundary_enforced: bool | None = None) -> list[Violation]:
    mods = load_modules(root)
    return [
        *check_roles(root, mods),
        *check_sources(root, mods),
        *check_committed_objects(root, mods),
        *check_objects(root, mods),
        *check_apps(root, mods),
        *check_spans(root, mods),
        *check_imports(root),
        *check_runtime_arrows(root),
        *check_runtime_platform(root),
        *(
            check_resident_boundary(root)
            if (BOUNDARY_ENFORCED if boundary_enforced is None else boundary_enforced)
            else []
        ),
        *check_service_crates(root),
        *check_core_kind_free(root),
        *check_self_contained(root, mods),
    ]


def boundary_report(root: Path) -> int:
    found = check_resident_boundary(root)
    for v in found:
        print(v)
    by_rule: dict[str, int] = {}
    by_file: dict[tuple[str, str], int] = {}
    for v in found:
        by_rule[v.rule] = by_rule.get(v.rule, 0) + 1
        key = (v.rule, v.where.rsplit(":", 1)[0])
        by_file[key] = by_file.get(key, 0) + 1
    print("\nper rule:")
    for k, n in sorted(by_rule.items()):
        print(f"  {k}: {n}")
    print("per file:")
    for (rule, file), n in sorted(
        by_file.items(), key=lambda kv: (kv[0][0], -kv[1], kv[0][1])
    ):
        print(f"  {rule} {n:4d} {file}")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "--root",
        type=Path,
        default=ROOT,
        help="repository root (default: this repository)",
    )
    ap.add_argument(
        "--boundary-report",
        action="store_true",
        help="list the crossings of the resident/package boundary (rules boundary-hook, boundary-table, boundary-abi) with counts per rule and file, and exit 0",
    )
    args = ap.parse_args(argv)
    if args.boundary_report:
        return boundary_report(args.root.resolve())
    found = check(args.root.resolve())
    for v in found:
        print(v)
    if found:
        print(
            f"layercheck: {len(found)} violation(s); the layers are in docs/ARCHITECTURE.md",
            file=sys.stderr,
        )
        return 1
    print("layercheck: the layering holds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
