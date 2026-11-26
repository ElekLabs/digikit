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
`sharc-core-lib`, `sharc-gen` and `sharc-rt` crates, Cargo.toml, rust-toolchain, .cargo), the core's reachable Cargo.lock package records, every file of
the gen dir, the PGO profile content (or `plain`), the pinned toolchain (in the argv) and `rustc -vV`. The tooling scripts
are not in the key: what they decide (argv, environment, flags, target) is in the plan, so an edit of a script that leaves
the plan alone keeps every stored library (the DN2 one takes 13 to 20 minutes). The library file and `header.json` (made
from the library and the gen dir) are all that an entry holds. Like the daemon key it is known before the first build, so a
lookup needs no build.
After a build, `build_core` audits cargo's dependency file against the key (`emud_key.audit`): a file the build
read that the key does not cover fails the build and nothing is stored. Add the path to CORE_SOURCE_GLOBS.

The last 4 used keys per image stay, plus every key that `dist/daemons.json` of any worktree names and every
entry used within the last hour. The build is `build_core`.
"""

from __future__ import annotations

import ctypes
import json
import os
import re
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass, replace
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

from tools.devtools import cargo_lock, emud_key, store  # noqa: E402

CRATE = "packages/emulator/sharc-core-lib"
CRATE_NAME = "sharc-core-lib"
LIB_STEM = "sharc_core_lib"
PROFILE = "release"
FEATURES: tuple[str, ...] = ()
STORE_NAME = "sharc-core-cache"
INSTRUMENT_STORE_NAME = "sharc-core-pgo-instrument"
LOCK_CLOSURE = "cargo-lock-closure"
RUSTFLAG_VARS = ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS")
# Compiler wrappers change the built code too; CARGO_TARGET_<TRIPLE>_* (rustflags, linker, runner) is matched by prefix.
WRAPPER_VARS = ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")

# git pathspecs (glob magic) of the repository files cargo reads for the library.
CORE_SOURCE_GLOBS = (
    "Cargo.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".cargo/**",
    "packages/emulator/sharc-rt/**",
    "packages/emulator/sharc-gen/**",
    f"{CRATE}/**",
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
    target: Path | None = (
        None  # the cargo target dir (the audit allows the files under it)
    )
    instrument: Path | None = (
        None  # the `-Cprofile-generate` directory of an instrumented build
    )


def lib_name() -> str:
    """The file name of the library on this host."""
    suffix = {"darwin": ".dylib", "win32": ".dll"}.get(sys.platform, ".so")
    return ("" if os.name == "nt" else "lib") + LIB_STEM + suffix


def store_root() -> Path:
    return store.store_root(STORE_NAME, "DIGI_SHARC_CORE_STORE")


def instrument_store_root() -> Path:
    """The store of the instrumented libraries of a PGO training (`native_pgo.instrumented_library`)."""
    return store.store_root(INSTRUMENT_STORE_NAME, "DIGI_SHARC_PGO_INSTRUMENT_STORE")


def absolute(value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else ROOT / path


def device_gen(device: str, environ: Mapping[str, str] | None = None) -> Path:
    """The generated core of DEVICE, whose image must start with `<dev>-` (the `IMAGE` const of its `image.rs`).

    SHARC_GEN_DIR_<DEV> wins and is refused with a message when it is another device's core. The generic SHARC_GEN_DIR is
    used only when its image is this device's; otherwise it is ignored. Else `out/native/<dev>-audio/gen`."""
    environ = os.environ if environ is None else environ
    own = environ.get(f"SHARC_GEN_DIR_{device.upper()}")
    if own:
        gen = absolute(own)
        if not gen_is(gen, device):
            raise CoreStoreError(
                f"SHARC_GEN_DIR_{device.upper()}={own} is not a {device.upper()} core (image {gen_image_or_none(gen)})"
            )
        return gen
    generic = environ.get("SHARC_GEN_DIR")
    if generic and gen_is(absolute(generic), device):
        return absolute(generic)
    return ROOT / "out" / "native" / f"{device}-audio" / "gen"


def gen_image_or_none(gen: Path) -> str | None:
    try:
        return gen_image(gen)
    except CoreStoreError:
        return None


def gen_is(gen: Path, device: str) -> bool:
    image = gen_image_or_none(gen)
    return image is not None and image.startswith(f"{device}-")


def build_plan(
    device: str,
    environ: Mapping[str, str] | None = None,
    gen: Path | None = None,
    profdata: Path | None = None,
    instrument: Path | None = None,
) -> Plan:
    """The one description of the build of DEVICE: `key_inputs` hashes it and `build_core` runs it.

    The command is the documented one (packages/emulator/README.md "Core libraries"): `cargo build --release --locked -p
    sharc-core-lib` with SHARC_GEN_DIR set, in the target dir of that core (as tools/nx/rust-env.mjs --gen picks it, so the
    compiled `sharc-gen` of the host builds is reused). A plain build has no PGO flags (the header leaves the PGO flags
    out, so a PGO host accepts it and the other way round). With PROFDATA the library is built with `-Cprofile-use=PROFDATA`
    (a store entry: the profile content is part of its key); with INSTRUMENT (a directory for `.profraw` files) it is built
    with `-Cprofile-generate=INSTRUMENT` for a training run (stored in the instrument store, `instrument_store_root`). Both use `--target <triple>`, so the flags skip
    build scripts, and a target dir of their own. GEN overrides the device's generated core (the launcher knows it)."""
    from tools.emulator import digi_env, native_pgo, native_target

    environ = os.environ if environ is None else environ
    gen = device_gen(device, environ) if gen is None else gen
    flags = None
    variant = ""
    if profdata is not None:
        flags, variant = native_pgo.profile_use_flags(str(profdata)), "pgo-lib-use"
    elif instrument is not None:
        flags, variant = f"-Cprofile-generate={instrument}", "pgo-lib-instrument"
    target = Path(native_target.target_dir(gen, variant))
    env = {**digi_env.rust_env(environ), "SHARC_GEN_DIR": str(gen)}
    if flags:
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        env["RUSTFLAGS"] = flags
    argv = [
        "rustup", "run", digi_env.TOOLCHAIN, "cargo", "build",
        f"--{PROFILE}", "--locked", "-p", CRATE_NAME,
        *(["--target", digi_env.TRIPLE] if flags else []),
        "--target-dir", str(target),
    ]  # fmt: skip
    if FEATURES:
        argv += ["--features", ",".join(FEATURES)]
    built = target / digi_env.TRIPLE if flags else target
    return Plan(
        argv, env, built / PROFILE / lib_name(), gen, profdata, target, instrument
    )


def key_inputs(
    plan: Plan,
    triple: str,
    rustc: str,
    root: Path = ROOT,
) -> emud_key.EmudInputs:
    """What the library depends on, in the daemon key's form: the argv without the target dir, the rustc flag and cargo
    profile variables of the plan's environment (the profile path replaced by its content hash), the repository files, the gen
    dir, the reachable lockfile records, the PGO profile, `rustc -vV`."""
    argv = list(plan.argv)
    if "--target-dir" in argv:
        at = argv.index("--target-dir")
        del argv[at : at + 2]  # a path of this tree, not an input
    flags = {"argv": " ".join(argv), "triple": triple, "profile": PROFILE}
    for name, value in sorted(plan.env.items()):
        if (
            name in RUSTFLAG_VARS
            or name in WRAPPER_VARS
            or name.startswith(("CARGO_PROFILE_", "CARGO_TARGET_"))
        ):
            if plan.profdata:
                value = value.replace(str(plan.profdata), "<profdata>")
            if plan.instrument:
                value = value.replace(str(plan.instrument), "<raw>")  # a lane path
            flags[f"env:{name}"] = value
    flags["pgo"] = (
        "profdata" if plan.profdata else "instrument" if plan.instrument else "plain"
    )
    flags[LOCK_CLOSURE] = lock_closure(root / "Cargo.lock")
    names = emud_key.repo_files(root, CORE_SOURCE_GLOBS)
    return emud_key.EmudInputs(names, {"gen": plan.gen}, plan.profdata, rustc, flags)


def lock_closure(lockfile: Path) -> str:
    try:
        return cargo_lock.closure_hash(lockfile, CRATE_NAME)
    except (KeyError, ValueError) as exc:
        raise CoreStoreError(f"cannot resolve the core lockfile inputs: {exc}") from exc


def legacy_library(group: Path, inputs: emud_key.EmudInputs) -> Path | None:
    """Adopt a pre-projection entry only for an exact match of the old inputs."""
    manifest = inputs.files.get("Cargo.toml")
    if manifest is None or LOCK_CLOSURE not in inputs.flags:
        return None
    lockfile = manifest.with_name("Cargo.lock")
    if lock_closure(lockfile) != inputs.flags[LOCK_CLOSURE]:
        raise CoreStoreError("the core lockfile dependencies changed during lookup")
    legacy = replace(
        inputs,
        files={**inputs.files, "Cargo.lock": lockfile},
        flags={
            name: value for name, value in inputs.flags.items() if name != LOCK_CLOSURE
        },
    )
    entry = group / emud_key.key_of(legacy)
    return entry / lib_name() if whole(entry) else None


def current(
    device: str, gen: Path | None = None, profdata: Path | None = None
) -> tuple[Plan, emud_key.EmudInputs]:
    from tools.emulator import digi_env

    plan = build_plan(device, gen=gen, profdata=profdata)
    return plan, key_inputs(plan, digi_env.TRIPLE, emud_key.rustc_version())


def core_key(device: str, profdata: Path | None = None) -> str:
    return emud_key.key_of(current(device, profdata=profdata)[1])


def build_core(
    plan: Plan, inputs: emud_key.EmudInputs, log: Path | None = None
) -> Path:
    """Run PLAN's cargo call, audit the dependency file against the key; the built file (cargo's own path, never stored).
    With LOG the output of cargo goes to that file, and a failure carries its last 30 lines."""
    if not (ROOT / CRATE / "Cargo.toml").is_file():
        raise CoreStoreError(
            f"the crate {CRATE} does not exist yet; the build would be: {' '.join(plan.argv)}"
        )
    from tools.devtools import benchlock
    from tools.emulator import digi_env

    digi_env.check_generated(
        plan.gen
    )  # refuses a stale generated core, as tools/nx/rust-env.mjs --gen does
    if log is not None:
        log.parent.mkdir(parents=True, exist_ok=True)
        with benchlock.hold("shared", "sharc core library"), log.open("w") as handle:
            done = subprocess.run(
                plan.argv,
                cwd=ROOT,
                env=plan.env,
                check=False,
                stdout=handle,
                stderr=subprocess.STDOUT,
            )
    else:
        with benchlock.hold("shared", "sharc core library"):
            done = subprocess.run(plan.argv, cwd=ROOT, env=plan.env, check=False)
    if done.returncode != 0 or not plan.library.is_file():
        tail = ""
        if log is not None:
            tail = "\n" + "".join(log.read_text().splitlines(keepends=True)[-30:])
        raise CoreStoreError(
            f"building the core library failed ({done.returncode}): {' '.join(plan.argv)}{tail}"
        )
    depfile = plan.library.with_suffix(".d")
    if not depfile.is_file():
        raise CoreStoreError(f"cargo wrote no {depfile}")
    missing = emud_key.audit(
        inputs, depfile.read_text(), plan.target or plan.library.parent.parent
    )
    if missing:
        lines = "".join(f"\n  {p}" for p in missing)
        raise CoreStoreError(
            f"{len(missing)} build inputs of the core library are not in the cache key:{lines}"
        )
    return plan.library


def read_header(library: Path) -> tuple[dict[str, object] | None, str | None]:
    """The header of a built library and the image name, as the JSON of its `sharc_core_header_json` symbol: (header, None),
    or (None, the reason) for a library without the symbol (an older build) or one that cannot be loaded."""
    try:
        lib = ctypes.CDLL(str(library))
    except OSError as exc:
        return None, f"cannot load {library}: {exc}"
    try:
        function = lib.sharc_core_header_json
    except AttributeError:
        return (
            None,
            "the library has no sharc_core_header_json symbol (built from older sources)",
        )
    function.restype = ctypes.c_char_p
    try:
        return json.loads((function() or b"").decode()), None
    except ValueError as exc:
        return None, f"sharc_core_header_json is not JSON: {exc}"


def gen_image(gen: Path) -> str:
    """The DSP image name of a generated core: the `IMAGE` const of its `image.rs` (`GenCore.image` of the library)."""
    try:
        text = (gen / "image.rs").read_text()
    except OSError as exc:
        raise CoreStoreError(f"{gen}: no image.rs: {exc}") from exc
    found = re.search(r'pub const IMAGE: &str = "([^"]*)"', text)
    if not found:
        raise CoreStoreError(f"{gen}/image.rs has no IMAGE const")
    return found[1]


def whole(entry: Path) -> bool:
    """An entry is usable only with its library, `key.txt` and `header.json`."""
    return all((entry / n).is_file() for n in (lib_name(), "key.txt", "header.json"))


def ensure_core(
    device: str,
    root: Path,
    *,
    resolve: Callable[[], tuple[Plan, emud_key.EmudInputs]] | None = None,
    build: Callable[[Plan, emud_key.EmudInputs], Path] = build_core,
    header: Callable[[Path], tuple[dict[str, object] | None, str | None]] = read_header,
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
    kind = " (profile-guided)" if plan.profdata else ""
    if whole(final) and store.mark_used(final):
        print(f"{device} core: restored {key[:12]}{kind}", flush=True)
        return final / lib_name()
    if final.exists():  # a partial entry: not a hit
        store.remove(final)
    legacy = legacy_library(group, inputs)
    if legacy is not None and not store.mark_used(legacy.parent):
        legacy = None
    built = legacy or build(plan, inputs)
    if emud_key.key_of(resolve()[1]) != key:
        raise CoreStoreError(f"the inputs of {device} changed during the build")

    def fill(tmp: Path) -> None:
        shutil.copyfile(built, tmp / lib_name())
        (tmp / lib_name()).chmod(0o755)
        read, reason = header(tmp / lib_name())
        record = {
            "device": device,
            "image": gen_image(plan.gen),
            "key": key,
            "pgo": plan.profdata is not None,
            "build_flags": inputs.flags,
            "header": read,
            **({"header_unavailable": reason} if read is None else {}),
        }
        (tmp / "header.json").write_text(json.dumps(record, indent=1) + "\n")

    if store.insert(group, key, "header.json", fill):
        status = "restored" if legacy else "built"
        print(f"{device} core: {status} {key[:12]}{kind}", flush=True)
    else:
        print(
            f"{device} core: restored {key[:12]} (inserted by another process)",
            flush=True,
        )
    trees = store.worktree_roots() if worktrees is None else worktrees
    for gone in store.evict(group, key, store.referenced_keys(group, trees)):
        print(f"{device} core: evicted {gone[:12]}", flush=True)
    return final / lib_name()


CORE_LIB_VAR = "DIGI_SHARC_CORE_LIB_{}"


def library_env(
    devices: Mapping[str, Path | None] | list[str],
    root: Path | None = None,
    environ: Mapping[str, str] | None = None,
    profiles: Mapping[str, Path] | None = None,
) -> dict[str, str]:
    """`DIGI_SHARC_CORE_LIB_<DEV>` for DEVICES (`dn2`, `dt2`) from the store: a miss builds the library. A host holds no
    generated core, so every launcher of a coupled run calls this. DEVICES is a list, or a map from a device to the generated
    core to build from (None: the device's own, see `device_gen`). A variable already set in ENVIRON (default: the process
    environment) wins and nothing is built for that device. PROFILES maps a device to a PGO profile: its library is then the
    one built with `-Cprofile-use=<profile>` (a store entry of its own key); a device without one gets the plain library.
    A library that cannot be built raises CoreStoreError with the build command in the message."""
    environ = os.environ if environ is None else environ
    wanted = dict.fromkeys(devices) if isinstance(devices, list) else dict(devices)
    root = store_root() if root is None else root
    env: dict[str, str] = {}
    for device, gen in wanted.items():
        var = CORE_LIB_VAR.format(device.upper())
        if environ.get(var):
            continue
        profile = (profiles or {}).get(device)
        try:
            library = ensure_core(
                device,
                root,
                resolve=(lambda d=device, g=gen, p=profile: current(d, g, p))
                if gen or profile
                else None,
            )
        except (CoreStoreError, OSError, SystemExit) as exc:
            reason = " ".join(str(exc).split()) or type(exc).__name__
            raise CoreStoreError(
                f"{device} core library unavailable: {reason}; build with: "
                f"uv run python -m tools.devtools.verify_build cores --device {device}"
            ) from exc
        env[var] = str(library)
    return env


def restored_library(
    device: str, root: Path | None = None, profdata: Path | None = None
) -> Path | None:
    """The stored library of DEVICE for the current inputs (and PROFDATA: the PGO one), or None: restore only, nothing is
    built."""
    try:
        _, inputs = current(device, profdata=profdata)
        group = (store_root() if root is None else root) / device
        entry = group / emud_key.key_of(inputs)
        return entry / lib_name() if whole(entry) else legacy_library(group, inputs)
    except (CoreStoreError, OSError, SystemExit):
        return None


def main(argv: list[str] | None = None) -> int:
    """`core_store key --device dn2`: the store key of the library of a device (what `verify_build cores` would store it
    under), for the Nx inputs of the coupled-run targets (tools/nx/coupled-inputs.mjs). Prints `none` when the key cannot be
    computed (no generated core)."""
    import argparse

    ap = argparse.ArgumentParser(description=(main.__doc__ or "").splitlines()[0])
    ap.add_argument("what", choices=["key"])
    ap.add_argument("--device", required=True)
    args = ap.parse_args(argv)
    try:
        print(core_key(args.device))
    except (CoreStoreError, OSError, SystemExit):
        print("none")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
