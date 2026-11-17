"""The core library store: the generated SHARC core of a device as a library, built once per key.

    uv run python -m tools.devtools.verify_build cores [--device dn2,dt2]

The store is `<git common dir>/sharc-core-cache/<image>/<key>/` (the library, `key.txt`, `header.json`),
shared by every worktree of the clone (the mechanics are in tools/devtools/store.py; `DIGI_SHARC_CORE_STORE`
moves it, tests only). The image is the device id of the generated core (`dn2`, `dt2`).

The key is a sha256 over: every file of the generated core directory (out/native/<dev>-audio/gen),
the sources of packages/emulator/sharc-rt, `rustc -vV` of the pinned toolchain, the target triple, the
CPU and other rustc flags of the environment, the cargo profile, and the PGO profile hash (or `plain`).
The last 4 used keys per image stay, and every key that `dist/daemons.json` of any worktree names.

The build is `build_core`: one function, `build_command` names the cargo call. It builds the crate
`sharc-core-lib` (not in the tree yet: until then a miss stops with a message naming the command).
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

from tools.devtools import store  # noqa: E402

RT_DIR = "packages/emulator/sharc-rt"
CRATE = "packages/emulator/sharc-core-lib"
LIB_STEM = "sharc_core_lib"
PROFILE = "release"
KEY_VERSION = 1
STORE_NAME = "sharc-core-cache"
RUSTFLAG_VARS = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")


class CoreStoreError(RuntimeError):
    pass


def lib_name() -> str:
    """The file name of the library on this host."""
    suffix = {"darwin": ".dylib", "win32": ".dll"}.get(sys.platform, ".so")
    return ("" if os.name == "nt" else "lib") + LIB_STEM + suffix


def store_root() -> Path:
    return store.store_root(STORE_NAME, "DIGI_SHARC_CORE_STORE")


def sha256_file(path: Path) -> str:
    with open(path, "rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def tree_hashes(folder: Path) -> list[tuple[str, str]]:
    """(relative posix name, sha256) of every file below FOLDER, sorted."""
    if not folder.is_dir():
        raise CoreStoreError(f"directory {folder} is missing")
    return sorted(
        (p.relative_to(folder).as_posix(), sha256_file(p))
        for p in folder.rglob("*")
        if p.is_file() and "target" not in p.relative_to(folder).parts
    )


def key_of(record: Mapping[str, object]) -> str:
    """sha256 over the canonical serialisation of a key record (see `key_record`)."""
    text = json.dumps(record, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(text.encode()).hexdigest()


def key_record(
    gen: Path,
    rt: Path,
    rustc: str,
    triple: str,
    flags: str,
    profdata: Path | None,
) -> dict[str, object]:
    """What a library depends on: the gen dir files, the sharc-rt sources, the toolchain, the flags, the profile."""
    return {
        "version": KEY_VERSION,
        "gen": tree_hashes(gen),
        "sharc_rt": tree_hashes(rt),
        "rustc": rustc,
        "triple": triple,
        "profile": PROFILE,
        "flags": flags,
        "pgo": sha256_file(profdata) if profdata else "plain",
    }


def rustc_version() -> str:
    from tools.devtools import emud_key

    return emud_key.rustc_version()


def current_inputs(
    device: str, environ: Mapping[str, str] | None = None
) -> tuple[dict[str, object], Path]:
    """The key record of the core of DEVICE as the build would run it, and its gen dir."""
    from tools.emulator import digi_env, native_pgo

    environ = os.environ if environ is None else environ
    gen = digi_env.generated_dir(environ, device=device)
    profile = native_pgo.current_profile(device, gen)
    profdata = Path(profile) if profile else None
    flags = " ".join(f"{k}={environ[k]}" for k in RUSTFLAG_VARS if environ.get(k))
    if profdata:
        flags = (
            flags + " " + native_pgo.profile_use_flags("<profdata sha256>")
        ).strip()
    record = key_record(
        gen, ROOT / RT_DIR, rustc_version(), digi_env.TRIPLE, flags, profdata
    )
    return record, gen


def core_key(device: str, environ: Mapping[str, str] | None = None) -> str:
    return key_of(current_inputs(device, environ)[0])


def build_command(device: str, gen: Path) -> tuple[list[str], dict[str, str], Path]:
    """The cargo call that builds the library of DEVICE: (argv, environment, the library it writes).

    The one place to point at the real crate: change CRATE and LIB_STEM here."""
    from tools.emulator import digi_env, native_pgo, native_target

    target = native_target.target_dir(gen, "core-lib")
    env = {**digi_env.rust_env(), "SHARC_GEN_DIR": str(gen)}
    profile = native_pgo.current_profile(device, gen)
    if profile and not any(env.get(k) for k in RUSTFLAG_VARS):
        env["RUSTFLAGS"] = native_pgo.profile_use_flags(profile)
    argv = [
        "rustup", "run", digi_env.TOOLCHAIN, "cargo", "build",
        f"--{PROFILE}", "--locked", "--manifest-path", f"{CRATE}/Cargo.toml",
        "--target-dir", str(target),
    ]  # fmt: skip
    return argv, env, target / PROFILE / lib_name()


def build_core(device: str, gen: Path) -> Path:
    """Build the library of DEVICE with `build_command`; the built file (cargo's own path, never stored)."""
    argv, env, library = build_command(device, gen)
    if not (ROOT / CRATE / "Cargo.toml").is_file():
        raise CoreStoreError(
            f"the crate {CRATE} does not exist yet; the build would be: {' '.join(argv)}"
        )
    from tools.devtools import benchlock

    with benchlock.hold("shared", f"sharc core library {device}"):
        done = subprocess.run(argv, cwd=ROOT, env=env, check=False)
    if done.returncode != 0 or not library.is_file():
        raise CoreStoreError(
            f"{device}: building the core library failed ({done.returncode}): {' '.join(argv)}"
        )
    return library


def ensure_core(
    device: str,
    root: Path,
    *,
    inputs: Callable[[], tuple[dict[str, object], Path]] | None = None,
    build: Callable[[str, Path], Path] = build_core,
    worktrees: list[Path] | None = None,
) -> Path:
    """The stored library of DEVICE: restored when its key is in ROOT, else built and inserted.

    INPUTS and BUILD are the seams of the tests (a stand-in artifact); WORKTREES lists the trees whose
    `dist/daemons.json` pin keys against eviction (default: every worktree of the clone)."""
    record, gen = (inputs or (lambda: current_inputs(device)))()
    key = key_of(record)
    group = root / device
    final = group / key
    library = final / lib_name()
    if library.is_file():
        (final / "key.txt").touch()
        print(f"{device} core: restored {key[:12]}", flush=True)
        return library
    built = build(device, gen)
    if key_of((inputs or (lambda: current_inputs(device)))()[0]) != key:
        raise CoreStoreError(f"the inputs of {device} changed during the build")

    def fill(tmp: Path) -> None:
        shutil.copyfile(built, tmp / lib_name())
        (tmp / lib_name()).chmod(0o755)
        header = {
            "image": device,
            "key": key,
            **{k: record[k] for k in ("rustc", "triple", "profile", "flags", "pgo")},
        }
        (tmp / "header.json").write_text(json.dumps(header, indent=1) + "\n")

    if store.insert(group, key, lib_name(), fill):
        print(f"{device} core: built {key[:12]}", flush=True)
    else:
        print(
            f"{device} core: restored {key[:12]} (inserted by another process)",
            flush=True,
        )
    trees = store.worktree_roots() if worktrees is None else worktrees
    before = {
        p.name for p in group.iterdir() if p.is_dir() and not p.name.startswith(".")
    }
    store.evict(group, key, store.referenced_keys(group, trees))
    after = {
        p.name for p in group.iterdir() if p.is_dir() and not p.name.startswith(".")
    }
    for gone in sorted(before - after):
        print(f"{device} core: evicted {gone[:12]}", flush=True)
    return library
