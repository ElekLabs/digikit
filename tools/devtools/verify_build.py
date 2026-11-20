"""Build the windowless daemons (`digi-emud`) once, as the Nx target `digi-control:daemons` does.

    uv run python -m tools.devtools.verify_build daemons [--device dt2,dn2] [--env-out FILE]
    uv run python -m tools.devtools.verify_build cores [--device dt2,dn2]

Before this, every python-tests process that drove a daemon asked cargo for the binary itself
(tools/patchkit/pk_harness.py `daemon_binary`): the first build of a new generated core ran inside a
test file, in parallel with the other files and the other steps. The Nx target `digi-control:daemons` runs
this once, before the targets that use a daemon.

For each device that has its ready inputs it takes the daemon from a content-addressed store,
`<git common dir>/emud-cache/<device>/<key>/` (digi-emud, key.txt, depfile.d), shared by every worktree of the
clone. The key is `tools/devtools/emud_key.py key`. On a miss it builds the profile-guided daemon when a PGO
profile exists (`pgo-emud-<device>` variant; the launcher's `pgo-use` is never touched), else the plain daemon;
checks that the key did not change during the build; audits cargo's dependency file against the key; and inserts
the result with one rename. The last 4 used keys per device stay, and every key that
`dist/daemons.json` of any worktree of the clone points at (a restore refreshes the use time). `DIGI_EMUD_<DEV>=<stored digi-emud>` is written
to --env-out (JSON); a stored file is never rewritten, so the test processes run it and ask cargo for nothing.
`pk_harness.daemon_binary` takes that variable before anything else.

`daemons` also provides the core library of each device (`DIGI_SHARC_CORE_LIB_<DEV>` in the
--env-out file, and the library key in the daemon test key); a daemon has no static core. `cores` provides the generated SHARC core of each device as a library from the same kind of store
(`sharc-core-cache`, tools/devtools/core_store.py); both use the mechanics of tools/devtools/store.py.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

from tools.devtools import core_store, emud_key, stablebin, store  # noqa: E402
from tools.patchkit import pk_emud_pgo, pk_harness  # noqa: E402


def cargo_target_dir(binary: Path) -> Path:
    """The cargo target dir of `<target>[/<triple>]/release/<bin>`."""
    target = binary.parent.parent
    return target.parent if target.name == pk_emud_pgo.digi_env.TRIPLE else target


def daemon_pgo(device: str) -> tuple[object, str, str, bool]:
    """How the daemon of DEVICE is built: the usable profile (None: no), the profile state, its message, and whether a stale
    profile is applied anyway. A plain daemon has no profile and no stale one."""
    pgo = pk_emud_pgo.native_pgo
    gen = pk_emud_pgo.generated_core(device)
    profile, state, message = pgo.profile_for_tests(device, gen)
    stale = (
        profile is None
        and state != "off"
        and pgo.current_profile(device, gen) is not None
    )
    return profile, state, message, stale


def build_one(device: str, syx: Path) -> tuple[Path, Path]:
    """Build the daemon of DEVICE as `daemons` does: (release binary, cargo target dir)."""
    pgo = pk_emud_pgo.native_pgo
    profile, state, message, stale = daemon_pgo(device)
    pgo.report_profile(device, state, message, applied=stale)
    if profile is None and not stale:
        plain = pk_harness.daemon_binary(device, syx, pgo=False)
        return plain, cargo_target_dir(plain)
    return pk_emud_pgo.build(device), pk_emud_pgo.target_dir(device)


def store_root() -> Path:
    """`<git common dir>/emud-cache` (or DIGI_EMUD_STORE, for tests): shared by every worktree of the clone, never committed."""
    return store.store_root("emud-cache", "DIGI_EMUD_STORE")


# The store mechanics (insert by rename, eviction, keys named by worktrees) are in tools/devtools/store.py.
worktree_roots = store.worktree_roots
referenced_keys = store.referenced_keys
evict = store.evict


def ensure_daemon(device: str, syx: Path, root: Path) -> Path:
    """The stored daemon of DEVICE: restored when its key is in ROOT, else built, audited and inserted."""
    key = emud_key.key(device)
    device_dir = root / device
    final = device_dir / key
    binary = final / "digi-emud"
    if binary.is_file():
        # an entry written without files.txt (older code sharing the store) gets one from its depfile, or counts as a miss
        if emud_key.backfill_files(final) and store.mark_used(final):
            print(f"{device} daemon: restored {key[:12]}", flush=True)
            return binary
        print(
            f"{device} daemon: entry {key[:12]} has no usable files list: rebuilding",
            flush=True,
        )
        store.remove(final)
    built, target = build_one(device, syx)
    inputs = emud_key.inputs(device)
    after = emud_key.key_of(inputs)
    if after != key:
        raise pk_harness.HarnessError(
            f"the inputs of {device} changed during the build ({key} -> {after})"
        )
    depfile = built.with_suffix(".d")
    if not depfile.is_file():
        raise pk_harness.HarnessError(f"cargo wrote no {depfile}")
    missing = emud_key.audit(inputs, depfile.read_text(), target)
    if missing:
        lines = "".join(f"\n  {path}" for path in missing)
        raise pk_harness.HarnessError(
            f"{len(missing)} build inputs of {device} are not in the cache key:{lines}"
        )

    def fill(tmp: Path) -> None:
        with stablebin.build_lock(target):  # cargo may be rewriting BUILT
            shutil.copyfile(built, tmp / "digi-emud")
        (tmp / "digi-emud").chmod(0o755)
        # the repository files the build read: the daemon part of the key of a device test that does not run patchkit
        (tmp / "files.txt").write_text(
            "".join(
                f"{n}\n"
                for n in emud_key.repo_files_of_depfile(
                    depfile.read_text(), ROOT, target
                )
            )
        )
        shutil.copyfile(depfile, tmp / "depfile.d")

    if store.insert(device_dir, key, "digi-emud", fill):
        print(f"{device} daemon: built {key[:12]}", flush=True)
    else:  # another process inserted this key first
        print(
            f"{device} daemon: restored {key[:12]} (inserted by another process)",
            flush=True,
        )
    evict(device_dir, key, referenced_keys(device_dir, worktree_roots()))
    return binary


CORE_LIB_VAR = core_store.CORE_LIB_VAR


def core_env(devices: list[str]) -> tuple[dict[str, str], dict[str, str]]:
    """Provide the core libraries of DEVICES from the store: the variables `DIGI_SHARC_CORE_LIB_<DEV>` (store paths) that
    make a host load the library, and the store key of each device. A device whose daemon is profile-guided with a fresh
    profile gets the library built with that profile (a store entry of its own key; the header leaves the PGO flags out, so
    either library works with either daemon), else the plain one; `..._PLAIN` then names the plain entry when it is stored,
    so that the eviction pins both. A device whose library cannot be provided is an error."""
    env: dict[str, str] = {}
    keys: dict[str, str] = {}
    root = core_store.store_root()
    for device in devices:
        profile = daemon_pgo(device)[0] if device in pk_emud_pgo.FIRMWARE else None
        profdata = Path(str(profile)) if profile else None
        try:
            library = core_store.ensure_core(
                device,
                root,
                resolve=(lambda d=device, p=profdata: core_store.current(d, None, p))
                if profdata
                else None,
            )
        except (core_store.CoreStoreError, OSError, SystemExit) as exc:
            # no static fallback: a host without its core library cannot run the DSP
            reason = " ".join(str(exc).split()) or type(exc).__name__
            raise pk_harness.HarnessError(
                f"{device} core library unavailable: {reason}; build with: "
                f"uv run python -m tools.devtools.verify_build cores --device {device}"
            ) from exc
        env[CORE_LIB_VAR.format(device.upper())] = str(library)
        keys[device] = library.parent.name
        if profdata and (plain := core_store.restored_library(device, root)):
            env[CORE_LIB_VAR.format(device.upper()) + "_PLAIN"] = str(plain)
    return env, keys


def build_daemons(devices: list[str]) -> tuple[dict[str, str], dict[str, str]]:
    """Provide the daemons of DEVICES from the store: the paths for downstream consumers (with the core libraries, see
    `core_env`), and the test key of each device (from the files the build read, see emud_key.test_key_of, and the key of
    the device's core library)."""
    env: dict[str, str] = {}
    test_keys: dict[str, str] = {}
    root = store_root()
    for device in devices:
        cell = pk_harness.Cell(device, pk_emud_pgo.FIRMWARE[device])
        try:
            syx, _ready = pk_harness.firmware_inputs(cell)
        except pk_harness.HarnessError as exc:
            print(f"SKIP {device}: {exc}", file=sys.stderr, flush=True)
            continue
        binary = ensure_daemon(device, syx, root)
        env[f"DIGI_EMUD_{device.upper()}"] = str(binary)
        test_key = emud_key.test_key(device, binary.parent)
        core, keys = core_env([device])
        env.update(core)
        if device in keys:
            test_key = hashlib.sha256(
                f"{test_key}\ncore {keys[device]}".encode()
            ).hexdigest()
        test_keys[device] = test_key
    return env, test_keys


def build_cores(
    devices: list[str], env_out: Path | None = None, pgo: bool = False
) -> int:
    """`verify_build cores`: the core libraries of DEVICES from the store (tools/devtools/core_store.py). With PGO the
    library of a device with a fresh profile (`native_pgo` status ok or partial) is the one built with it."""
    try:
        env: dict[str, str] = {}
        profiles = pk_emud_pgo.native_pgo.fresh_profiles(devices) if pgo else {}
        for device in devices:
            # standalone: every library is built, whatever the daemon build of the device is
            root = core_store.store_root()
            profile = profiles.get(device)
            library = core_store.ensure_core(
                device,
                root,
                resolve=(lambda d=device, p=profile: core_store.current(d, None, p))
                if profile
                else None,
            )
            env[CORE_LIB_VAR.format(device.upper())] = str(library)
            print(f"{device} core: {library}", flush=True)
    except (core_store.CoreStoreError, OSError, SystemExit) as exc:
        print(f"verify_build: {exc}", file=sys.stderr)
        return 1
    if env_out:
        env_out.write_text(json.dumps(env, indent=2) + "\n")
    return 0


def check_pgo(devices: list[str]) -> int:
    """Check the actual library selected by a device test before it queues."""
    for device in devices:
        pgo = pk_emud_pgo.native_pgo
        profile, state, detail = pgo.profile_for_tests(
            device, pk_emud_pgo.generated_core(device)
        )
        if not profile:
            print(
                f"{device}: PGO profile unavailable ({state}). "
                + (
                    detail
                    or f"Check DIGI_EMU_PGO and host support. Train with: {pgo.train_hint(device)}"
                ),
                file=sys.stderr,
            )
            return 1
        var = CORE_LIB_VAR.format(device.upper())
        selected = os.environ.get(var)
        if not selected:
            manifest = ROOT / "dist" / "daemons.json"
            try:
                selected = json.loads(manifest.read_text()).get(var)
            except (OSError, ValueError) as exc:
                print(f"{device}: cannot read {manifest}: {exc}", file=sys.stderr)
                return 1
        expected = core_store.restored_library(device, profdata=Path(profile))
        if (
            not selected
            or expected is None
            or Path(selected).resolve() != expected.resolve()
        ):
            print(
                f"{device}: {var} selects {selected or 'no library'}; expected the "
                f"PGO core for this checkout and profile. Check overrides and refresh "
                f"dist/daemons.json with the digi-control:daemons target.",
                file=sys.stderr,
            )
            return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("what", choices=["daemons", "cores", "check-pgo"])
    ap.add_argument(
        "--device", default="dt2,dn2", help="comma-separated (default: dt2,dn2)"
    )
    ap.add_argument(
        "--pgo",
        action="store_true",
        help="cores: the library of a device with a fresh PGO profile is built with it (store entry of its own key)",
    )
    ap.add_argument(
        "--env-out",
        type=Path,
        help="write the pinned DIGI_EMUD_<DEV> and DIGI_SHARC_CORE_LIB_<DEV> variables here (JSON)",
    )
    args = ap.parse_args(argv)
    devices = [d for d in args.device.split(",") if d]
    if args.what == "cores":
        return build_cores(devices, args.env_out, args.pgo)
    unknown = sorted(set(devices) - set(pk_emud_pgo.FIRMWARE))
    if unknown:
        ap.error(f"unknown devices: {', '.join(unknown)}")
    if args.what == "check-pgo":
        return check_pgo(devices)
    try:
        env, test_keys = build_daemons(devices)
    except pk_harness.HarnessError as exc:
        print(f"verify_build: {exc}", file=sys.stderr)
        return 1
    if not env:
        print(
            "verify_build: no daemon built (missing stock firmware or ready inputs, see SKIP lines above); "
            "build ready inputs with mise run emu:ready -- <device>",
            file=sys.stderr,
        )
        return 1
    if args.env_out:
        args.env_out.write_text(json.dumps(env, indent=2) + "\n")
        # dist/bytes/daemon-test-<device>.key, the key only: the device tasks of a merge key hash the file of their own device instead
        # of the daemon path, which moves with every patchkit edit. dist/bytes/daemon-test.key lists all devices, one `device key` line each.
        key_file = args.env_out.parent / "bytes" / "daemon-test.key"
        key_file.parent.mkdir(parents=True, exist_ok=True)
        key_file.write_text("".join(f"{d} {k}\n" for d, k in sorted(test_keys.items())))
        for stale in key_file.parent.glob("daemon-test-*.key"):
            stale.unlink()
        for d, k in test_keys.items():
            (key_file.parent / f"daemon-test-{d}.key").write_text(f"{k}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
