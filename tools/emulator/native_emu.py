#!/usr/bin/env python3
"""Build the desktop emulator once, then run the binary.

With local ready inputs and a generated core (built as a core library into the shared store, tools/devtools/core_store.py: the
app and the daemon hold no generated core themselves), the app gets coupled audio; it
opens with no firmware loaded unless a FILE is given (the daemon loads the
default firmware). The private inputs stay out of the repository: every
path is overridable, and unavailable inputs leave the ordinary CF diagnostic
emulator available.

The UI dist and the wasm files come from Nx (`mise run workbench:inputs`).
The app is built once with cargo build and the binary is run directly;
--build-only stops after the build and prints the binary path. A stale PGO
profile is not used: the build falls back to plain with a warning. `mise run
perf:pgo` retrains it (or launch once with DIGI_EMU_PGO_TRAIN=1).
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
from tools import python_env  # noqa: E402
from tools.emulator import digi_env, emulator_config  # noqa: E402
from tools.firmware import device as device_data  # noqa: E402


def fast_regions_for(syx: pathlib.Path) -> str:
    """The verified fast regions for this firmware file, or "" if unknown."""
    return digi_env.fast_regions_for(syx)


def set_fast_regions(
    args: list[str], environ: dict[str, str] | os._Environ[str]
) -> emulator_config.Resolution | None:
    """Enable the fast tier for a coupled run unless the user chose otherwise.

    The rules are in `emulator_config.resolve` (explicit values win, DIGI_EMU_FAST=0 adds
    nothing, explicit regions or auto selection suppress fallback roots for that core).
    Returns the resolution of the first core, None when DIGI_EMU_FAST=0.
    """
    if environ.get("DIGI_EMU_FAST") == "0":
        return None
    syx = (
        pathlib.Path(args[args.index("--audio-profile-syx") + 1])
        if "--audio-profile-syx" in args
        else digi_env.firmware(environ)
    )
    # The addresses belong to one device's DSP image: the DN2 core reads SHARC_FAST_REGIONS and SHARC_BLOCKTRANS, the
    # DT2 core the lists with a _2 suffix (`GenCore::env_suffix`), so the suffix follows the firmware's device, in a launch
    # of one device and in one with `--also-coupled` (whose firmware is the fourth value).
    firmwares = [syx]
    if "--also-coupled" in args and (at := args.index("--also-coupled") + 4) < len(
        args
    ):
        firmwares.append(pathlib.Path(args[at]))
    first = None
    for firmware in firmwares:
        res = digi_env.resolve_tiers(None, firmware, None, environ)
        environ.update(res.add)
        first = first or res
    return first


def generated_core(
    args: list[str] | None = None, device: str | None = None
) -> pathlib.Path:
    """The generated core for a launch: SHARC_GEN_DIR_<DEVICE> (when DEVICE is given),
    else SHARC_GEN_DIR, else the one registered for the --dsp-image in ARGS
    (tools/sharc_dev/aot_gen.py), else the DN2 default."""
    image = None
    if args and "--dsp-image" in args:
        position = args.index("--dsp-image") + 1
        image = args[position] if position < len(args) else None
    return digi_env.generated_dir(image=image, device=device)


# The cargo features of a coupled host (digi-coupled, digi-control, the app).
COUPLED_FEATURES = "sharc,play"

# Launcher options that take a value: the value is not the firmware file (the first positional argument).
VALUE_OPTIONS = {"--card-image", "--card-folder", "--audio-buffer", "--mirror-dir"}


def other_core(device: str, dsp_image: pathlib.Path | str) -> pathlib.Path | None:
    """The generated core of DEVICE for its DSP image: SHARC_GEN_DIR_<DEVICE>, else the one registered for the image, else
    the DN2 default. Not `digi_env.generated_dir`: SHARC_GEN_DIR names the launch's own device and would win."""
    if os.environ.get(f"SHARC_GEN_DIR_{device.upper()}"):
        return pathlib.Path(os.environ[f"SHARC_GEN_DIR_{device.upper()}"])
    return digi_env.gen_for_image(dsp_image) or (
        digi_env.DEFAULT_GENERATED if device == "dn2" else None
    )


def second_core(
    device: str, *, enabled_by_default: bool = False
) -> tuple[list[str], pathlib.Path | None]:
    """The other device's ready inputs for a second coupled engine in the same process (`--also-coupled`) and its generated
    core (the launch provides its library, `core_library_env`). Desktop launches enable it by default; daemons opt in with
    DIGI_EMU_SECOND_CORE=1. DIGI_EMU_SECOND_CORE=0 disables it. The profile of the build is device `both`.
    Nothing (a one-device launch) when it is off or the other device lacks a ready set or a core."""
    if (
        os.environ.get("DIGI_EMU_SECOND_CORE", "1" if enabled_by_default else "0")
        != "1"
    ):
        return [], None
    other = "dt2" if device == "dn2" else "dn2"
    paths = digi_env.ready_inputs(device=other)
    missing = [label for label, path in paths.items() if not path.is_file()]
    gen = other_core(other, paths["DSP image"])
    if missing or gen is None or not gen.is_dir():
        print(
            f"second engine: no {other.upper()} inputs (missing {', '.join(missing) or 'generated core'}); one device only",
            file=sys.stderr,
        )
        return [], None
    digi_env.check_generated(gen)
    print(f"second engine: {gen} ({other.upper()})", file=sys.stderr)
    return [
        "--also-coupled",
        str(paths["DSP image"]),
        str(paths["DSP state"]),
        str(paths["CF snapshot"]),
        str(paths["firmware"]),
    ], gen


def default_audio_args(
    args: list[str] | None = None,
    *,
    second_core_by_default: bool = False,
    load_default: bool = True,
) -> list[str]:
    """Add local ready audio inputs to a normal launcher command line.

    load_default=False leaves out the default firmware as the first positional:
    the app opens empty and uses --audio-profile-syx only for its audio profile.
    """
    args = [] if args is None else list(args)
    positional = next(
        (
            value
            for index, value in enumerate(args)
            if not value.startswith("--")
            and (index == 0 or args[index - 1] not in VALUE_OPTIONS)
        ),
        None,
    )
    syx = pathlib.Path(positional) if positional else digi_env.firmware()
    device = digi_env.device_for_syx(syx) or "dn2"
    if device not in digi_env.READY_DIRS:
        # A device with no DSP (no ready inputs, no generated core): ColdFire-only build; its audio, if any, is the peer's.
        return args
    paths = digi_env.ready_inputs(device=device)
    # A patched build of the stock firmware (a runtime image) boots from the positional file while its audio state is the stock firmware's (the device whose snapshot is built from the stock firmware alone does the same): the
    # snapshot is only valid for the firmware it was taken from (the daemon refuses another one: "snapshot was taken from different firmware").
    if positional and device != "dn2" and not ready_is_of_another_firmware(paths, syx):
        paths["firmware"] = syx
    card = (
        pathlib.Path(args[args.index("--card-image") + 1])
        if "--card-image" in args and args.index("--card-image") + 1 < len(args)
        else None
    )
    if device != "dn2" and (card is not None or not paths["CF snapshot"].is_file()):
        paths = ready_for(device, syx, card, paths)
    missing = [label for label, path in paths.items() if not path.is_file()]
    generated = digi_env.generated_dir(image=paths["DSP image"], device=device)
    own_var = f"SHARC_GEN_DIR_{device.upper()}"
    explicit = own_var if os.environ.get(own_var) else "SHARC_GEN_DIR"
    reason = explicit if os.environ.get(explicit) else "registered core"
    if not generated.is_dir():
        missing.append(f"generated {device.upper()} core (set {own_var})")
    if missing:
        print(
            "Native audio is not set up (missing "
            + ", ".join(missing)
            + "); launching the CF-only diagnostic emulator. "
            "Use --cf-only to silence this notice.",
            file=sys.stderr,
        )
        return []
    digi_env.check_generated(generated)
    print(f"native core: {generated} ({reason})", file=sys.stderr)
    also, _second = second_core(device, enabled_by_default=second_core_by_default)
    args.extend(also)
    if positional is None and load_default:
        args.insert(0, str(paths["firmware"]))
    args.extend(
        [
            "--auto-coupled",
            "--audio-profile-syx",
            str(paths["firmware"]),
            "--dsp-image",
            str(paths["DSP image"]),
            "--dsp-state",
            str(paths["DSP state"]),
            "--cf-snapshot",
            str(paths["CF snapshot"]),
        ]
    )
    # 0.3 s of prefill: the paced app held DN2 and DT2 at 0 underruns with it (finding 18 "Desktop app
    # real time"); pass --audio-buffer 1 on a slower machine, or 0 for the lowest latency.
    if "--audio-buffer" not in args:
        args.extend(["--audio-buffer", "0.3"])
    return args


def ready_is_of_another_firmware(
    paths: dict[str, pathlib.Path], syx: pathlib.Path
) -> bool:
    """True when the ready directory records (`SOURCE.json`, written by ready.py) another firmware than `syx`. A directory that records nothing is not known to differ."""
    try:
        recorded = json.loads(
            (paths["CF snapshot"].parent / "SOURCE.json").read_text()
        )["syx_sha256"]
    except (OSError, KeyError, ValueError):
        return False
    return recorded != hashlib.sha256(syx.read_bytes()).hexdigest()


def ready_for(
    device: str,
    syx: pathlib.Path,
    card: pathlib.Path | None,
    paths: dict[str, pathlib.Path],
) -> dict[str, pathlib.Path]:
    """The ready inputs of a device whose snapshot depends on the card (DT2:
    the sample list is in the restored RAM), built from the .syx alone by
    tools/emulator/ready.py on first use and reused after."""
    command = [
        sys.executable,
        "-m",
        "tools.emulator.ready",
        device,
        "--syx",
        str(syx),
    ]
    if card is not None:
        command += ["--card-image", str(card)]
    print(
        f"Preparing the {device.upper()} audio state for this card (first time only)...",
        file=sys.stderr,
    )
    done = subprocess.run(
        command, cwd=ROOT, env=python_env(), capture_output=True, text=True
    )
    sys.stderr.write(done.stderr[-2000:])
    ready_dir = next(
        (
            line.partition("=")[2]
            for line in done.stdout.splitlines()
            if line.startswith("ready_dir=")
        ),
        None,
    )
    if done.returncode or ready_dir is None:
        print(
            f"tools/emulator/ready.py {device} failed:\n{done.stdout[-2000:]}",
            file=sys.stderr,
        )
        return paths
    base = pathlib.Path(ready_dir)
    return {
        **paths,
        "DSP image": base / "digi-audio-dn2-image.bin",
        "DSP state": base / "digi-audio-m5.snap.dsp",
        "CF snapshot": base / "digi-audio-m5.snap",
    }


def target_dir_for(gen: pathlib.Path | None = None, variant: str = "") -> str:
    """The shared cargo target dir (tools/emulator/native_target.py) of a build against the generated core GEN. The app, the
    daemon and the other hosts hold no generated core, so they all use the no-core dir (the default).
    A build with other cargo features needs its own `variant` (it cannot reuse the compiled crates of the plain build)."""
    from tools.emulator import native_target

    # "" means no generated core, whatever SHARC_GEN_DIR says.
    return str(
        native_target.target_dir(gen if gen is not None else "", variant=variant)
    )


# Every workbench daemon has the run-time ColdFire profile (`cf.profile {enable}`, off until asked). The feature changes the
# emulator crates, so these builds keep their own target dirs (variant suffix) and leave the plain ones as they are.
CF_PROFILE = "cf-profile"
CF_PROFILE_VARIANT = "cfprof"


def has_peer_audio(data: device_data.Device) -> bool:
    """The device's MAIN mixes the voices of a peer CPU (`[emulator.peer_audio]` in its device file)."""
    return "peer_audio" in data.emulator


def cargo_command(args: list[str], pgo: object | None = None) -> list[str]:
    """The cargo command line; pgo is a native_pgo.LaunchPlan for the PGO build.

    The app holds no generated core, so it builds into the no-core target dir.
    """
    command = [
        "cargo",
        "run",
        "--release",
        "--locked",
        "--manifest-path",
        "apps/desktop/src-tauri/Cargo.toml",
    ]
    if "--coupled" in args or "--auto-coupled" in args:
        command.extend(["--features", f"{COUPLED_FEATURES},{CF_PROFILE}"])
        if pgo is not None:
            command = [*pgo.prefix, *command, *pgo.cargo_args]  # type: ignore[attr-defined]
        else:
            command.extend(["--target-dir", target_dir_for(variant=CF_PROFILE_VARIANT)])
    else:
        # A device whose audio comes from its peer CPU needs the audio output in a ColdFire-only build (no generated core).
        # It has its own target dir: switching devices does not rebuild the shared nogen one.
        features = [CF_PROFILE]
        variant = CF_PROFILE_VARIANT
        if has_peer_audio(launch_device(syx_from_args(args))):
            features.insert(0, "play")
            variant = f"play-{CF_PROFILE_VARIANT}"
        command.extend(["--features", ",".join(features)])
        command.extend(["--target-dir", target_dir_for(None, variant=variant)])
    return [*command, "--", *args]


def device_for_args(args: list[str]) -> str:
    """The PGO profile device of a coupled launch: `both` when it runs a second engine (`--also-coupled`), else the
    device of its firmware file (`dn2`, `dt2`)."""
    if "--also-coupled" in args:
        return "both"
    if "--audio-profile-syx" in args:
        position = args.index("--audio-profile-syx") + 1
        syx = pathlib.Path(args[position]) if position < len(args) else None
    else:
        syx = digi_env.firmware()
    return (digi_env.device_for_syx(syx) if syx else None) or "dn2"


# What a training takes (measured on a lane, 2026-11-20). Warm: host build and instrumented library are in the
# lane and the store, so only the runs and the merge are left. Cold: the instrumented library and host builds
# (DN2 library 11 min, DT2 library 2.5 min, host builds 2 to 3 min each) are added; a full cold `perf:pgo` took 29 min.
TRAIN_ESTIMATE = {
    "dn2": "about 10 s warm, 13 s after an engine edit, 15 min with a cold library and host build",
    "dt2": "about 1.5 min warm, 5 min with a cold library and host build",
    "both": "about 4 min warm, 8 min with a cold two-core host build",
}


def train_profile(device: str = "dn2") -> str:
    """Run `native_pgo.py train`; returns "" on success, else why it failed."""
    try:
        done = subprocess.run(
            [
                sys.executable,
                "-m",
                "tools.emulator.native_pgo",
                "train",
                *(["--device", device] if device != "dn2" else []),
            ],
            env=python_env(),
            cwd=ROOT,
            check=False,
        )
    except OSError as exc:
        return f"could not start training: {exc}"
    return f"training failed (exit {done.returncode})" if done.returncode else ""


PGO_REPORT: dict[str, str] = {}


def pgo_env() -> dict[str, str]:
    """DIGI_PGO_STATUS and DIGI_PGO_DETAIL for the child, from the last pgo_plan; empty when unknown."""
    if not PGO_REPORT:
        return {}
    return {
        "DIGI_PGO_STATUS": PGO_REPORT["status"],
        "DIGI_PGO_DETAIL": PGO_REPORT["detail"],
    }


def note_pgo(status: str, detail: str) -> None:
    """Remember the PGO state of this launch (partial counts as ok; an ok detail is a path, so it is dropped)."""
    if status in ("ok", "partial"):
        status, detail = "ok", ""
    PGO_REPORT.update(status=status, detail=detail)


def pgo_plan(args: list[str]) -> object | None:
    """Profile-use plan for the coupled build, or None for the ordinary build.

    Each single-core device has its own profile (`native_pgo.py train --device dt2`): a DN2
    profile gives a DT2 build only the shared-engine 3%, a DT2 profile 19%. A launch with a second engine (`--also-coupled`,
    the desktop default) has the profile of device `both` (`train --device both`), built in a target dir of its
    own; `mise run perf:pgo` trains it after dn2 and dt2, so a missing or stale `both` profile only prints
    the notice and builds a plain host. Desktop DSP libraries resolve their profiles separately. The launcher never trains on its own. A stale or missing profile falls back
    to the ordinary build (its own target dir, not the launcher's pgo-use). DIGI_EMU_PGO_TRAIN=1 trains a stale or missing
    profile here when the training inputs exist (`mise run perf:pgo` is the usual
    way); DIGI_EMU_PGO=0 skips PGO. Any failure falls back to the ordinary
    build with a WARNING line: launching is never blocked.
    """
    PGO_REPORT.clear()
    if "--coupled" not in args and "--auto-coupled" not in args:
        return None
    try:
        from tools.emulator import native_pgo
    except ImportError:
        return None
    environ = os.environ
    device = device_for_args(args)
    status, detail = native_pgo.pgo_state(environ, device)
    if status == "off":
        return None
    if status in ("stale", "missing"):
        if environ.get("DIGI_EMU_PGO_TRAIN") == "1":
            blocker = native_pgo.training_blocker(environ, device)
            if blocker:
                detail = f"{detail}; cannot train: {blocker}"
            else:
                print(
                    f"native PGO: profile {status} ({detail}); training ({TRAIN_ESTIMATE[device]}, "
                    "DIGI_EMU_PGO_TRAIN=1)",
                    file=sys.stderr,
                    flush=True,
                )
                failure = train_profile(device)
                if failure:
                    status, detail = "failed", failure
                else:
                    status, detail = native_pgo.pgo_state(environ, device)
        elif status == "stale":
            note_pgo(status, detail)
            print(native_pgo.stale_notice(detail, device), file=sys.stderr)
            return None
        else:
            note_pgo(status, detail)
            print(native_pgo.fallback_notice(status, detail, device), file=sys.stderr)
            return None
    if status == "partial":
        profile = native_pgo.current_profile(
            device,
            native_pgo.device_generated(device, environ),
            native_pgo.second_generated(environ) if device == "both" else None,
        )
        if profile:
            note_pgo(status, detail)
            print(native_pgo.PARTIAL_NOTICE, file=sys.stderr)
            return native_pgo.plan_for(profile, environ, device)
        status, detail = "missing", "profile file is gone"
    if status != "ok":
        note_pgo(status, detail)
        print(native_pgo.fallback_notice(status, detail, device), file=sys.stderr)
        return None
    note_pgo(status, detail)
    print(
        f"native PGO: using {device.upper() + ' ' if device != 'dn2' else ''}profile "
        + pathlib.Path(detail).name,
        file=sys.stderr,
    )
    return native_pgo.plan_for(detail, environ, device)


PGO_MISMATCH_WARNING = (
    "WARNING: PGO profile does not match this binary (crate hashes differ); "
    "live DSP will run ~35% slower. Run uv run python -m tools.emulator.native_pgo train."
)
PGO_MISMATCH_WARNING_DT2 = PGO_MISMATCH_WARNING.replace("train.", "train --device dt2.")
PGO_MISMATCH_WARNING_BOTH = PGO_MISMATCH_WARNING.replace(
    "train.", "train --device both."
)


def build_command(command: list[str]) -> list[str]:
    """The `cargo run` command as a `cargo build` of the same binary."""
    cut = command.index("--")
    head = command[:cut]
    head[head.index("cargo") + 1] = "build"
    return head


def verify_pgo(
    command: list[str],
    env: dict[str, str],
    device: str = "dn2",
    *,
    binary: pathlib.Path | None = None,
    before: int | None = None,
) -> None:
    """Build, then warn loudly if the profile's crate hashes miss the binary.

    A failed build stops the launch; a failed check only warns. The check runs
    only when the build changed `binary`; `before` is its mtime_ns before the
    build, None when it did not exist.
    """
    built = subprocess.run(build_command(command), cwd=ROOT, env=env)
    if built is not None and built.returncode:
        raise SystemExit(built.returncode)
    if (
        binary is not None
        and before is not None
        and binary.exists()
        and binary.stat().st_mtime_ns == before
    ):
        return
    try:
        done = subprocess.run(
            [
                sys.executable,
                "-m",
                "tools.emulator.native_pgo",
                "verify",
                "--device",
                device,
                *(
                    ["--binary", str(app_binary(command, True))]
                    if "--target-dir" in command
                    else []
                ),
            ],
            env=python_env(),
            cwd=ROOT,
            check=False,
        )
    except (OSError, ValueError):
        return
    if done.returncode == 1:
        warning = {
            "dt2": PGO_MISMATCH_WARNING_DT2,
            "both": PGO_MISMATCH_WARNING_BOTH,
        }.get(device, PGO_MISMATCH_WARNING)
        print(warning, file=sys.stderr, flush=True)
        note_pgo("mismatch", "profile does not match this build")


def daemon_command(args: list[str]) -> list[str]:
    """`cargo run` for the windowless daemon `digi-emud` (packages/emulator/control).

    Same target dir as the desktop app. No PGO: the profile is trained on the desktop binary.
    """
    command = [
        "cargo",
        "run",
        "--release",
        "--locked",
        "--manifest-path",
        "packages/emulator/control/Cargo.toml",
        "--bin",
        "digi-emud",
    ]
    if "--coupled" in args or "--auto-coupled" in args:
        command.extend(
            [
                "--features",
                f"{COUPLED_FEATURES},{CF_PROFILE}",
                "--target-dir",
                target_dir_for(variant=CF_PROFILE_VARIANT),
            ]
        )
    else:
        command.extend(
            [
                "--features",
                CF_PROFILE,
                "--target-dir",
                target_dir_for(None, variant=CF_PROFILE_VARIANT),
            ]
        )
    return [*command, "--", *args]


def app_binary(command: list[str], plan: object | None) -> pathlib.Path:
    """The binary the build command produces (digiemu-desktop, or digi-emud)."""
    target = pathlib.Path(command[command.index("--target-dir") + 1])
    name = (
        command[command.index("--bin") + 1]
        if "--bin" in command[: command.index("--")]
        else "digiemu-desktop"
    )
    if plan is not None:
        return target / digi_env.TRIPLE / "release" / name
    return target / "release" / name


def plus_drive_files(root: pathlib.Path) -> list[pathlib.Path]:
    """Every regular file under `root` (relative paths, sorted); symbolic links and
    .DS_Store are left out, as the builder leaves them out."""
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        here = pathlib.Path(dirpath)
        for name in sorted(filenames):
            path = here / name
            if name != ".DS_Store" and not path.is_symlink():
                found.append(path.relative_to(root))
    return found


def plusdrive_card(
    folder: pathlib.Path, syx: pathlib.Path, env: dict[str, str], device: str = "dt2"
) -> pathlib.Path:
    """Build (or reuse) a +Drive card image from a `plus_drive/<device>/` folder.

    The folder contract (docs/findings/14-plus-drive-format.md, "plus_drive/"): files and
    folders map to the path filesystem as they are; a `.wav` becomes a native
    sample named without the extension (24 -> 16 bit, 48 kHz); any other file is
    copied byte for byte (a file that is a native sample stays one);
    `.mmcfs/{projects,sounds,kits}/NNNN.bin` are raw MmcFs slots;
    `.mmcfs/boot/SSSSSS.bin` are the project regions outside the pools (a boot
    copy replaces the generated boot project); `digikit/` goes to the module
    store (docs/design/digikit-store.md). DN2 has no path filesystem: only
    `.mmcfs/` and `digikit/` apply. The card layout of the device comes from
    devices/<device>.toml `[card]`.

    Uses the native `plusdrive build` (packages/emulator/plusdrive). The image lives in
    out/plusdrive/native/<key12>/<device>.img, keyed by the SHA-256 of the firmware,
    the plusdrive binary and every file's relative path and bytes, so unchanged
    inputs reuse the image.
    """
    if not folder.is_dir():
        raise SystemExit(f"{folder} is not a directory")
    files = plus_drive_files(folder)
    if not files:
        raise SystemExit(f"{folder} holds no files")
    manifest = ROOT / "packages" / "emulator" / "plusdrive" / "Cargo.toml"
    # Share the CF-only target dir with the desktop app: one dependency build.
    target = target_dir_for(None)
    done = subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--locked",
            "--manifest-path",
            str(manifest),
            "--bin",
            "plusdrive",
            "--target-dir",
            target,
        ],
        cwd=ROOT,
        env=env,
    )
    if done.returncode:
        raise SystemExit(done.returncode)
    binary = pathlib.Path(target) / "release" / "plusdrive"
    h = hashlib.sha256()
    h.update(b"firmware\0" + hashlib.sha256(syx.read_bytes()).digest())
    h.update(b"plusdrive\0" + hashlib.sha256(binary.read_bytes()).digest())
    h.update(b"device\0" + device.encode() + b"\0")
    for rel in files:
        h.update(b"file\0" + rel.as_posix().encode() + b"\0")
        h.update(hashlib.sha256((folder / rel).read_bytes()).digest())
    key = h.hexdigest()[:12]
    image = ROOT / "out" / "plusdrive" / "native" / key / f"{device}.img"
    if image.is_file():
        print(f"+Drive card: reusing {image} ({len(files)} file(s))", file=sys.stderr)
        return image
    image.parent.mkdir(parents=True, exist_ok=True)
    # One partial file per process: parallel verify steps can build the same
    # key at once, and os.replace lets the last one win atomically.
    partial = image.with_name(f"{device}.{os.getpid()}.part")
    done = subprocess.run(
        [
            str(binary),
            "build",
            str(folder),
            "-o",
            str(partial),
            "--device",
            device,
            "--syx",
            str(syx),
        ],
        cwd=ROOT,
        env=env,
    )
    if done.returncode:
        partial.unlink(missing_ok=True)
        raise SystemExit(done.returncode)
    os.replace(partial, image)
    print(f"+Drive card: built {image} ({len(files)} file(s))", file=sys.stderr)
    return image


def take_option(args: list[str], name: str) -> tuple[str | None, list[str]]:
    """The value of `--name VALUE` and `args` without it."""
    if name not in args:
        return None, args
    i = args.index(name)
    if i + 1 >= len(args):
        raise SystemExit(f"{name} needs a directory")
    return args[i + 1], args[:i] + args[i + 2 :]


def syx_from_args(args: list[str]) -> pathlib.Path:
    """The firmware file the launcher will run: the first positional argument,
    else the default."""
    syx_arg = next(
        (
            v
            for j, v in enumerate(args)
            if not v.startswith("--") and (j == 0 or args[j - 1] not in VALUE_OPTIONS)
        ),
        None,
    )
    return pathlib.Path(syx_arg) if syx_arg else digi_env.firmware(os.environ)


def launch_device(syx: pathlib.Path) -> device_data.Device:
    """The device data of a firmware file, by hash (a patched image by its patch record).

    A file that no device file lists is an error: the launcher never guesses a device.
    """
    short = digi_env.device_for_syx(syx)
    if short is None:
        raise SystemExit(
            f"{syx}: cannot tell which device this firmware is for (the file is unreadable, or no device file in "
            "devices/ lists its sha256)"
        )
    return device_data.by_short(short)


def core_library_env(
    args: list[str], plan: object | None = None, *, desktop: bool = False
) -> dict[str, str]:
    """`DIGI_SHARC_CORE_LIB_<DEV>` of both devices for a coupled launch, from the core library store
    (tools/devtools/core_store.py: a hit takes about 0.2 s, a miss builds the library once), whichever device the firmware
    is and with or without `--also-coupled`, so choosing a firmware of the other device in the app boots it coupled. A
    profile-guided build works with either library (the header leaves the PGO flags out). Nothing for a launch without
    coupled audio. When one device's library cannot be built the launch goes on with the other and one line names the
    missing one and the build command; the launch stops when no library can be provided. An explicit
    `DIGI_SHARC_CORE_LIB_<DEV>` wins. With a PGO PLAN (a fresh profile) the library of each device the profile covers is the
    one built with that profile. Desktop tries each device's fresh combined profile, then its fresh individual
    profile, then its plain library."""
    if not ("--coupled" in args or "--auto-coupled" in args):
        return {}
    from tools.devtools import core_store
    from tools.emulator import native_pgo

    profiles = native_pgo.library_profiles(plan)  # type: ignore[arg-type]
    firmware = (
        pathlib.Path(args[args.index("--audio-profile-syx") + 1])
        if "--audio-profile-syx" in args
        else syx_from_args(args)
    )
    own = digi_env.device_for_syx(firmware)
    second: dict[str, str] = {}
    if "--also-coupled" in args:
        at = args.index("--also-coupled")
        if at + 4 < len(args) and (
            other := digi_env.device_for_syx(pathlib.Path(args[at + 4]))
        ):
            second[other] = args[at + 1]
    env: dict[str, str] = {}
    missing: dict[str, str] = {}
    devices = device_data.dsp_shorts()  # a device without a DSP has no core library
    states: dict[str, str] = {}
    cores = {}
    for device in devices:
        if device == own:
            cores[device] = generated_core(args, device)
        else:
            image = (
                second.get(device) or digi_env.ready_inputs(device=device)["DSP image"]
            )
            cores[device] = other_core(device, image)
    desktop_profiles: dict[str, list[pathlib.Path]] = {}
    if desktop:
        # Key each profile against the exact library the firmware loader will open.
        profile_env = dict(os.environ)
        for device, gen in cores.items():
            if gen is not None:
                profile_env[f"SHARC_GEN_DIR_{device.upper()}"] = str(gen)
        profile_env["SHARC_GEN_DIR"] = str(cores[digi_env.SLOT_DEVICES[0]])
        desktop_profiles = native_pgo.fresh_profile_candidates(devices, profile_env)
    for device, gen in cores.items():
        candidates: list[pathlib.Path | None] = (
            [*desktop_profiles.get(device, []), None]
            if desktop
            else [profiles.get(device)]
        )
        for i, profile in enumerate(candidates):
            try:
                library = core_store.library_env(
                    {device: gen}, profiles={device: profile} if profile else {}
                )
            except core_store.CoreStoreError as exc:
                if i + 1 < len(candidates):
                    fallback = "the device profile" if candidates[i + 1] else "plain"
                    print(
                        f"{device.upper()} profile-guided DSP library failed: {exc}; trying {fallback}",
                        file=sys.stderr,
                    )
                else:
                    states[device] = "unavailable"
                    missing[device] = str(exc)
                continue
            env.update(library)
            if not library:
                states[device] = "external"
            else:
                states[device] = "ok" if profile else "plain"
            break
    if len(missing) == len(devices):
        raise SystemExit(f"core library: {'; '.join(missing.values())}")
    for device, reason in missing.items():
        print(
            f"{device.upper()} core library unavailable ({reason}): a {device.upper()} firmware will not run its DSP; "
            f"build with: uv run python -m tools.devtools.verify_build cores --device {device}",
            file=sys.stderr,
        )
    if (
        desktop
        and os.environ.get("DIGI_EMU_PGO") != "0"
        and native_pgo.supported_host()
    ):
        env["DIGI_DSP_PGO"] = json.dumps(states)
    return env


def main(args: list[str] | None = None) -> None:
    args = sys.argv[1:] if args is None else args
    force_cf = "--cf-only" in args
    build_only = "--build-only" in args
    daemon = "--daemon" in args
    no_sync = "--no-sync" in args
    args = [
        arg
        for arg in args
        if arg not in ("--cf-only", "--build-only", "--daemon", "--no-sync")
    ]
    plus_drive, args = take_option(args, "--plus-drive")
    samples, args = take_option(args, "--samples")
    if plus_drive is not None and samples is not None:
        raise SystemExit("--plus-drive and --samples cannot be combined")
    folder = pathlib.Path(plus_drive or samples) if (plus_drive or samples) else None
    if folder is not None and "--card-image" in args:
        raise SystemExit("--plus-drive/--samples and --card-image cannot be combined")
    syx = syx_from_args(args)
    data = launch_device(syx)
    device = data.short
    # A device that is not a runtime target (no `[digikit]`) boots stock; a device without a DSP runs the ColdFire-only build.
    if not data.runtime_target:
        if "--runtime" in args:
            raise SystemExit(f"{data.name} is not a digikit runtime target: --runtime")
        if "--no-runtime" not in args:
            print(
                f"{data.name} is not a digikit runtime target: starting the stock firmware (--no-runtime)",
                file=sys.stderr,
            )
            args = [*args, "--no-runtime"]
    if folder is not None and not data.has_card():
        raise SystemExit(
            f"{data.name} has no card the launcher can seed: --plus-drive/--samples"
        )
    if folder is not None:
        if plus_drive is None and samples is None:
            print(f"+Drive: {folder}", file=sys.stderr)
        env = digi_env.rust_env(dict(os.environ))
        # A patched image gets the card of its stock base (the same card as `inject` keeps).
        card = plusdrive_card(folder, digi_env.base_firmware(syx), env, device)
        first_positional = any(
            not v.startswith("--") and (j == 0 or args[j - 1] not in VALUE_OPTIONS)
            for j, v in enumerate(args)
        )
        args = [
            *([] if first_positional else [str(syx)]),
            *args,
            "--card-image",
            str(card),
            # The app exports what the firmware wrote to the card back into the folder on exit
            # (a log line says how many files); --no-sync leaves the folder alone.
            "--card-folder",
            str(folder.resolve()),
            *(["--no-card-sync"] if no_sync else []),
        ]
        if not no_sync:
            print(
                f"+Drive: changes on the card are exported to {folder} on exit (--no-sync turns it off)",
                file=sys.stderr,
            )
    # One folder per device, chosen by the app (Rust: the controller): plus_drive/<device>/ or the folder the user chose in the Card
    # panel (kept per device in the app's data folder). On every load the controller builds the card from the loaded device's folder
    # when it holds something the card takes, and mirrors the module store to it while the emulator runs (the Card panel: files,
    # "Sync now"). --plus-drive/--samples name the folder of the first load instead (a card built here from it). --no-sync turns
    # the mirror and the export on exit off.
    if data.has_card() and "--device-folders" not in args:
        args = [
            *args,
            "--device-folders",
            *(["--no-card-sync"] if no_sync and folder is None else []),
        ]
    if folder is not None and not no_sync and "--mirror-dir" not in args:
        args = [*args, "--mirror-dir", str(folder.resolve())]
    if force_cf and ("--coupled" in args or "--auto-coupled" in args):
        raise SystemExit("--cf-only cannot be combined with coupled audio options")
    if not data.has_dsp() and ("--coupled" in args or "--auto-coupled" in args):
        raise SystemExit(f"{data.name} has no DSP: coupled audio options do not apply")
    if (
        data.has_dsp()
        and not force_cf
        and "--coupled" not in args
        and "--auto-coupled" not in args
    ):
        args = default_audio_args(
            args, second_core_by_default=not daemon, load_default=daemon
        )
    fast = None
    if "--coupled" in args or "--auto-coupled" in args:
        generated = generated_core(args, device)
        if not generated.is_dir():
            raise SystemExit(
                "--coupled needs a local generated core: SHARC_GEN_DIR_<DEV> (or SHARC_GEN_DIR) names it"
            )
        digi_env.check_generated(generated)
        fast = set_fast_regions(args, os.environ)
    # The daemon has no window: no PGO.
    plan = None if daemon else pgo_plan(args)
    env = digi_env.rust_env({**os.environ, **(plan.env if plan is not None else {})})  # type: ignore[attr-defined]
    command = daemon_command(args) if daemon else cargo_command(args, plan)
    binary = app_binary(command, plan)
    before = binary.stat().st_mtime_ns if binary.exists() else None
    if plan is not None:
        verify_pgo(command, env, device_for_args(args), binary=binary, before=before)
    else:
        done = subprocess.run(build_command(command), cwd=ROOT, env=env)
        if done is not None and done.returncode:
            raise SystemExit(done.returncode)
    if build_only:
        print(binary)
        return
    # Devices whose MAIN mixes the voices of a peer CPU play them (`[emulator.peer_audio]`); the other devices ignore it.
    if not daemon and has_peer_audio(data):
        env.setdefault("DIGI_EMU_PEER_AUDIO", "0" if force_cf else "1")
    env.update(core_library_env(args, plan, desktop=not daemon))
    if not daemon and data.has_dsp():
        env.update(pgo_env())
        # Workbench default from config/emulator.json; an explicit value, including an empty one, still wins.
        context = digi_env.resolve_tiers(
            "workbench", digi_env.firmware(env), "", env, lists=False
        )
        env.update(context.add)
        print(emulator_config.startup_line("workbench", env, fast, context), flush=True)
    subprocess.run([str(binary), *args], cwd=ROOT, check=True, env=env)


if __name__ == "__main__":
    main()
