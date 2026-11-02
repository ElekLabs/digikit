"""Build the windowless daemons (`digi-emud`) once for a verify run.

    uv run python -m tools.devtools.verify_build daemons [--device dt2,dn2] [--env-out FILE]

Before this, every python-tests process that drove a daemon asked cargo for the binary itself
(tools/patchkit/pk_harness.py `daemon_binary`): the first build of a new generated core ran inside a
test file, in parallel with the other files and the other steps. verify now runs this once, before the
steps that use a daemon (`build-daemons` in tools/devtools/verify.py), after the coupled-run builds, never in
parallel with them.

For each device that has its ready inputs it builds the profile-guided daemon when a PGO profile exists
(`pgo-emud-<device>` variant; the launcher's `pgo-use` is never touched), else the plain daemon (the target dir
of the device's generated core, shared with the coupled-run build). A PGO daemon is copied to an immutable
file (tools/devtools/stablebin.py) and `DIGI_EMUD_<DEV>=<copy>` is written to --env-out (JSON), so the test
processes run that copy and ask cargo for nothing; `pk_harness.daemon_binary` takes that variable before
anything else, so a device with a pinned PGO daemon never uses the plain one and it is not built (it used to
be, first, for every device). The plain daemon is published through the same environment keys without copying: it belongs to
the selected stock image's generated core and downstream steps wait for this build node.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

import tools  # noqa: E402
from tools.devtools import stablebin  # noqa: E402
from tools.patchkit import pk_emud_pgo, pk_harness  # noqa: E402

NX_OUT = ROOT / "out" / "nx"


def cargo_target_dir(binary: Path) -> Path:
    """The cargo target dir of `<target>[/<triple>]/release/<bin>`."""
    target = binary.parent.parent
    return target.parent if target.name == pk_emud_pgo.digi_env.TRIPLE else target


def build_one(device: str, syx: Path) -> tuple[Path, Path]:
    """Build the daemon of DEVICE as `daemons` does: (release binary, cargo target dir)."""
    if pk_emud_pgo.native_pgo.current_profile(device) is None:
        plain = pk_harness.daemon_binary(device, syx, pgo=False)
        return plain, cargo_target_dir(plain)
    return pk_emud_pgo.build(device), pk_emud_pgo.target_dir(device)


def build_daemon(device: str, out: Path) -> int:
    """Build one daemon and write OUT/digi-emud, OUT/key.txt and OUT/depfile.d, then audit its inputs.

    This is the body of the Nx `daemon-<device>` target (tools/nx/cargo.mjs); the key is computed before and
    after the build and must not change.
    """
    from tools.devtools import emud_key

    cell = pk_harness.Cell(device, pk_emud_pgo.FIRMWARE[device])
    syx, _ready = pk_harness.firmware_inputs(cell)
    before = emud_key.key(device)
    binary, target = build_one(device, syx)
    inputs = emud_key.inputs(device)
    after = emud_key.key_of(inputs)
    if before != after:
        print(
            f"verify_build: the inputs of {device} changed during the build ({before} -> {after})",
            file=sys.stderr,
        )
        return 1
    depfile = binary.with_suffix(".d")
    if not depfile.is_file():
        print(f"verify_build: cargo wrote no {depfile}", file=sys.stderr)
        return 1
    missing = emud_key.audit(inputs, depfile.read_text(), target)
    if missing:
        print(
            f"verify_build: {len(missing)} build inputs of {device} are not in the cache key:",
            file=sys.stderr,
        )
        for path in missing:
            print(f"  {path}", file=sys.stderr)
        return 1
    out.mkdir(parents=True, exist_ok=True)
    partial = out / f".digi-emud.{os.getpid()}.part"
    shutil.copyfile(binary, partial)
    partial.chmod(0o755)
    os.replace(partial, out / "digi-emud")
    (out / "key.txt").write_text(after + "\n")
    shutil.copyfile(depfile, out / "depfile.d")
    print(f"{device} daemon: {out / 'digi-emud'} key {after[:12]}", flush=True)
    return 0


def nx_daemon(device: str) -> Path:
    """The daemon of DEVICE through the Nx target (built or restored from the shared cache)."""
    command = ["pnpm", "nx:pilot", "run", f"digi-control:daemon-{device}"]
    done = subprocess.run(command, cwd=ROOT, env=tools.python_env())
    binary = NX_OUT / "emud" / device / "digi-emud"
    if done.returncode or not binary.is_file():
        raise pk_harness.HarnessError(
            f"nx daemon-{device} failed (exit {done.returncode})"
        )
    return binary


def build_daemons(devices: list[str], via_nx: bool = False) -> dict[str, str]:
    """Build the daemons of DEVICES; publish each built path for downstream consumers."""
    env: dict[str, str] = {}
    for device in devices:
        cell = pk_harness.Cell(device, pk_emud_pgo.FIRMWARE[device])
        try:
            syx, _ready = pk_harness.firmware_inputs(cell)
        except pk_harness.HarnessError as exc:
            print(f"SKIP {device}: {exc}", flush=True)
            continue
        if via_nx:
            binary = nx_daemon(device)
            target = NX_OUT / "emud-stable" / device
            with stablebin.build_lock(target):
                copy = stablebin.stable_copy(binary, target)
            print(f"{device} nx: {copy}", flush=True)
            env[f"DIGI_EMUD_{device.upper()}"] = str(copy)
            continue
        if pk_emud_pgo.native_pgo.current_profile(device) is None:
            plain = pk_harness.daemon_binary(device, syx, pgo=False)
            env[f"DIGI_EMUD_{device.upper()}"] = str(plain)
            print(f"{device} plain: {plain}", flush=True)
            print(f"{device} pgo: no profile; the harness uses the plain daemon")
            continue
        pgo_path = pk_emud_pgo.build(device)
        target = pgo_path.parent.parent
        with stablebin.build_lock(target):
            copy = stablebin.stable_copy(pgo_path, target)
        print(f"{device} pgo: {copy}", flush=True)
        env[f"DIGI_EMUD_{device.upper()}"] = str(copy)
    return env


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("what", choices=["daemons", "daemon"])
    ap.add_argument(
        "--device", default="dt2,dn2", help="comma-separated (default: dt2,dn2)"
    )
    ap.add_argument(
        "--via-nx",
        action="store_true",
        help="daemons: take each daemon from the Nx target daemon-<device> (shared cache)",
    )
    ap.add_argument(
        "--out", type=Path, help="daemon: directory for digi-emud, key.txt, depfile.d"
    )
    ap.add_argument(
        "--env-out",
        type=Path,
        help="write the pinned DIGI_EMUD_<DEV> variables here (JSON)",
    )
    args = ap.parse_args(argv)
    devices = [d for d in args.device.split(",") if d]
    unknown = sorted(set(devices) - set(pk_emud_pgo.FIRMWARE))
    if unknown:
        ap.error(f"unknown devices: {', '.join(unknown)}")
    if args.what == "daemon":
        if len(devices) != 1 or not args.out:
            ap.error("daemon needs one --device and --out")
        try:
            return build_daemon(devices[0], args.out)
        except pk_harness.HarnessError as exc:
            print(f"verify_build: {exc}", file=sys.stderr)
            return 1
    try:
        env = build_daemons(devices, args.via_nx)
    except pk_harness.HarnessError as exc:
        print(f"verify_build: {exc}", file=sys.stderr)
        return 1
    if args.env_out:
        args.env_out.write_text(json.dumps(env, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
