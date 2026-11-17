"""The core library store: the generated SHARC core of a device as a library, built once per key.

    uv run python -m tools.devtools.verify_build cores [--device dn2,dt2]

The store is `<git common dir>/sharc-core-cache/<image>/<key>/` (the library, `key.txt`, `header.json`),
shared by every worktree of the clone (the mechanics are in tools/devtools/store.py; `DIGI_SHARC_CORE_STORE`
moves it, tests only). The image is the device id of the generated core (`dn2`, `dt2`). An entry is a hit only
with all three files.

Key and build come from one source: `build_plan` returns the exact argv and environment of the cargo call, and
`key_inputs` hashes the parts of that plan that change the library (the argv without the target dir, the rustc
flag variables after `digi_env.rust_env` and the PGO flags, the cargo profile variables). The key scheme is the
daemon key's (tools/devtools/emud_key.py `key_of`): a fixed list of source globs (CORE_SOURCE_GLOBS: the
`sharc-core-lib`, `sharc-gen` and `sharc-rt` crates, Cargo.toml, Cargo.lock, rust-toolchain, .cargo) and the
recipe scripts (CORE_RECIPE_FILES), every file of the gen dir, the PGO profile content (or `plain`), the pinned
toolchain and `rustc -vV`. Like the daemon key it is known before the first build, so a lookup needs no build.
After a build, `build_core` audits cargo's dependency file against the key (`emud_key.audit`): a file the build
read that the key does not cover fails the build and nothing is stored. Add the path to CORE_SOURCE_GLOBS.

The last 4 used keys per image stay, plus every key that `dist/daemons.json` of any worktree names and every
entry used within the last hour. The build is `build_core`; the crate `sharc-core-lib` is not in the tree yet.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

from tools.devtools import emud_key, store  # noqa: E402

CRATE = "packages/emulator/sharc-core-lib"
LIB_STEM = "sharc_core_lib"
PROFILE = "release"
FEATURES: tuple[str, ...] = ()
STORE_NAME = "sharc-core-cache"
RUSTFLAG_VARS = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")

# git pathspecs (glob magic) of the repository files cargo reads for the library.
CORE_SOURCE_GLOBS = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".cargo/**",
    "packages/emulator/sharc-rt/**",
    "packages/emulator/sharc-gen/**",
    f"{CRATE}/**",
)
# The scripts that decide how the build runs.
CORE_RECIPE_FILES = (
    "tools/devtools/core_store.py",
    "tools/devtools/store.py",
    "tools/emulator/digi_env.py",
    "tools/emulator/native_pgo.py",
    "tools/emulator/native_target.py",
)


class CoreStoreError(RuntimeError):
    pass


@dataclass
class Plan:
    """The cargo call that builds the library: argv, environment, the library it writes, the gen dir, the PGO profile."""

    argv: list[str]
    env: dict[str, str]
    library: Path
    gen: Path
    profdata: Path | None


def lib_name() -> str:
    """The file name of the library on this host."""
    suffix = {"darwin": ".dylib", "win32": ".dll"}.get(sys.platform, ".so")
    return ("" if os.name == "nt" else "lib") + LIB_STEM + suffix


def store_root() -> Path:
    return store.store_root(STORE_NAME, "DIGI_SHARC_CORE_STORE")


def build_plan(device: str, environ: Mapping[str, str] | None = None) -> Plan:
    """The one description of the build of DEVICE: `key_inputs` hashes it and `build_core` runs it.

    The one place to point at the real crate: change CRATE and LIB_STEM."""
    from tools.emulator import digi_env, native_pgo, native_target

    environ = os.environ if environ is None else environ
    gen = digi_env.generated_dir(environ, device=device)
    target = native_target.target_dir(gen, "core-lib")
    env = {**digi_env.rust_env(environ), "SHARC_GEN_DIR": str(gen)}
    profile = native_pgo.current_profile(device, gen)
    profdata = Path(profile) if profile else None
    if profdata and not any(env.get(k) for k in RUSTFLAG_VARS):
        env["RUSTFLAGS"] = native_pgo.profile_use_flags(str(profdata))
    argv = [
        "rustup", "run", digi_env.TOOLCHAIN, "cargo", "build",
        f"--{PROFILE}", "--locked", "--manifest-path", f"{CRATE}/Cargo.toml",
        "--target-dir", str(target),
    ]  # fmt: skip
    if FEATURES:
        argv += ["--features", ",".join(FEATURES)]
    return Plan(argv, env, target / PROFILE / lib_name(), gen, profdata)


def key_inputs(
    plan: Plan,
    triple: str,
    rustc: str,
    root: Path = ROOT,
) -> emud_key.EmudInputs:
    """What the library depends on, in the daemon key's form: the argv without the target dir, the rustc flag and cargo
    profile variables of the plan's environment (the profile path replaced by its content hash), the repository files, the gen
    dir, the PGO profile, `rustc -vV`."""
    argv = list(plan.argv)
    if "--target-dir" in argv:
        at = argv.index("--target-dir")
        del argv[at : at + 2]  # a path of this tree, not an input
    flags = {"argv": " ".join(argv), "triple": triple, "profile": PROFILE}
    for name, value in sorted(plan.env.items()):
        if name in RUSTFLAG_VARS or name.startswith("CARGO_PROFILE_"):
            if plan.profdata:
                value = value.replace(str(plan.profdata), "<profdata>")
            flags[f"env:{name}"] = value
    flags["pgo"] = "profdata" if plan.profdata else "plain"
    names = emud_key.repo_files(root, (*CORE_SOURCE_GLOBS, *CORE_RECIPE_FILES))
    return emud_key.EmudInputs(names, {"gen": plan.gen}, plan.profdata, rustc, flags)


def current(device: str) -> tuple[Plan, emud_key.EmudInputs]:
    from tools.emulator import digi_env

    plan = build_plan(device)
    return plan, key_inputs(plan, digi_env.TRIPLE, emud_key.rustc_version())


def core_key(device: str) -> str:
    return emud_key.key_of(current(device)[1])


def build_core(plan: Plan, inputs: emud_key.EmudInputs) -> Path:
    """Run PLAN's cargo call, audit the dependency file against the key; the built file (cargo's own path, never stored)."""
    if not (ROOT / CRATE / "Cargo.toml").is_file():
        raise CoreStoreError(
            f"the crate {CRATE} does not exist yet; the build would be: {' '.join(plan.argv)}"
        )
    from tools.devtools import benchlock

    with benchlock.hold("shared", "sharc core library"):
        done = subprocess.run(plan.argv, cwd=ROOT, env=plan.env, check=False)
    if done.returncode != 0 or not plan.library.is_file():
        raise CoreStoreError(
            f"building the core library failed ({done.returncode}): {' '.join(plan.argv)}"
        )
    depfile = plan.library.with_suffix(".d")
    if not depfile.is_file():
        raise CoreStoreError(f"cargo wrote no {depfile}")
    missing = emud_key.audit(inputs, depfile.read_text(), plan.library.parent.parent)
    if missing:
        lines = "".join(f"\n  {p}" for p in missing)
        raise CoreStoreError(
            f"{len(missing)} build inputs of the core library are not in the cache key:{lines}"
        )
    return plan.library


def whole(entry: Path) -> bool:
    """An entry is usable only with its library, `key.txt` and `header.json`."""
    return all((entry / n).is_file() for n in (lib_name(), "key.txt", "header.json"))


def ensure_core(
    device: str,
    root: Path,
    *,
    resolve: Callable[[], tuple[Plan, emud_key.EmudInputs]] | None = None,
    build: Callable[[Plan, emud_key.EmudInputs], Path] = build_core,
    worktrees: list[Path] | None = None,
) -> Path:
    """The stored library of DEVICE: restored when its key is in ROOT, else built and inserted.

    RESOLVE and BUILD are the seams of the tests (a stand-in artifact); WORKTREES lists the trees whose
    `dist/daemons.json` pin keys against eviction (default: every worktree of the clone)."""
    resolve = resolve or (lambda: current(device))
    plan, inputs = resolve()
    key = emud_key.key_of(inputs)
    group = root / device
    final = group / key
    if whole(final) and store.mark_used(final):
        print(f"{device} core: restored {key[:12]}", flush=True)
        return final / lib_name()
    if final.exists():  # a partial entry: not a hit
        store.remove(final)
    built = build(plan, inputs)
    if emud_key.key_of(resolve()[1]) != key:
        raise CoreStoreError(f"the inputs of {device} changed during the build")

    def fill(tmp: Path) -> None:
        shutil.copyfile(built, tmp / lib_name())
        (tmp / lib_name()).chmod(0o755)
        header = {
            "image": device,
            "key": key,
            "rustc": inputs.rustc,
            "flags": inputs.flags,
        }
        (tmp / "header.json").write_text(json.dumps(header, indent=1) + "\n")

    if store.insert(group, key, "header.json", fill):
        print(f"{device} core: built {key[:12]}", flush=True)
    else:
        print(
            f"{device} core: restored {key[:12]} (inserted by another process)",
            flush=True,
        )
    trees = store.worktree_roots() if worktrees is None else worktrees
    for gone in store.evict(group, key, store.referenced_keys(group, trees)):
        print(f"{device} core: evicted {gone[:12]}", flush=True)
    return final / lib_name()
