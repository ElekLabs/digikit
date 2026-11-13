#!/usr/bin/env python3
"""patchkit: check one module with its conformance suite and own tests.

    uv run python -m tools.patchkit.pk_modcheck NAME [--device dt2|dn2] [--trace]      (mise run module:check NAME)

NAME is a module id (`<author/name>`) or its last part (`<name>`). An effect module runs tests/modules/test_fx_conformance.py. A machine module (tools/patchkit/pk_conform.py) runs the conformance
suite, tests/modules/test_machine_conformance.py, for its devices (docs/TESTING.md "Machine conformance"). The same process includes the module's own tests: `modules/<group>/<name>/tests/**/test_*.py` and its scenarios in `tests/scenarios/*.toml` (only those run, at every tier). One pytest process runs those Python tests. Module-owned Nx `test-device-all` targets (full inputs) then run their declared TypeScript device acceptance; any failing stage stops the check before size/fit.
Own tests may build or launch an emulator. A service/raw module with no Python tests has its objects checked against their sources, then runs its owned device target if present. Size and fit follow only after both stages pass. This command does not prove every module loads in patched firmware.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

from tools import python_env  # noqa: E402
from tools.patchkit import modroots, pk_conform  # noqa: E402

CONFORMANCE = "tests/modules/test_machine_conformance.py"
FX_CONFORMANCE = "tests/modules/test_fx_conformance.py"


USES_KEYS = ("machine_rows", "param_rows", "page_rows", "record_bytes")


def write_uses(toml_path: Path, uses: dict[str, dict[str, int]]) -> bool:
    """Write the `[uses.<device>]` tables of a module.toml like a lock file (docs/plans/sdk.md 6.1): USES maps a device to the counts of
    USES_KEYS, committed rows read from that device's emulated run. The file changes only when a count differs, and then only the table lines:
    other text, comments and the order of the tables stay. Returns True when the file was written."""
    text = toml_path.read_text(encoding="utf-8")
    for dev, counts in uses.items():
        text = _write_table(text, dev, counts)
    if text == toml_path.read_text(encoding="utf-8"):
        return False
    toml_path.write_text(text, encoding="utf-8")
    return True


def _write_table(text: str, dev: str, uses: dict[str, int]) -> str:
    head = f"[uses.{dev}]"
    want = [f"{k} = {int(uses[k])}" for k in USES_KEYS]
    lines = text.split("\n")
    start = next((i for i, ln in enumerate(lines) if ln.strip() == head), None)
    if start is None:
        # a new table follows the last `[uses.*]` table, else goes to the end
        last = [i for i, ln in enumerate(lines) if ln.strip().startswith("[uses.")]
        if not last:
            return text.rstrip("\n") + f"\n\n{head}\n" + "\n".join(want) + "\n"
        at = last[-1] + 1
        while at < len(lines) and not lines[at].lstrip().startswith("["):
            at += 1
        while at > 0 and lines[at - 1].strip() == "":
            at -= 1
        return "\n".join(lines[:at] + ["", head, *want] + lines[at:])
    end = start + 1
    while end < len(lines) and not lines[end].lstrip().startswith("["):
        end += 1
    body = lines[start + 1 : end]
    have = {
        m.group(1): m.group(2)
        for ln in body
        if (m := re.match(r"\s*(\w+)\s*=\s*(\d+)\s*(?:#.*)?$", ln))
    }
    if all(have.get(k) == str(int(uses[k])) for k in USES_KEYS):
        return text
    # keep comments and blank lines of the table; the count lines take their place, missing ones follow the last count
    out, seen = [], set()
    for ln in body:
        m = re.match(r"\s*(\w+)\s*=", ln)
        if m and m.group(1) in USES_KEYS:
            seen.add(m.group(1))
            out.append(f"{m.group(1)} = {int(uses[m.group(1)])}")
        else:
            out.append(ln)
    missing = [w for w in want if w.split(" ")[0] not in seen]
    trail = 0
    while out and out[-1].strip() == "":
        out.pop()
        trail += 1
    out += missing + [""] * trail
    return "\n".join(lines[: start + 1] + out + lines[end:])


def _daemon(device: str) -> str:
    """The plain emulator daemon of DEVICE: `DIGI_EMUD_<DEV>`, else resolved as `run_device_targets` does."""
    given = os.environ.get(f"DIGI_EMUD_{device.upper()}")
    if given:
        return given
    from tools.devtools import benchlock
    from tools.patchkit import pk_harness

    version = {"dt2": "1.17", "dn2": "1.12"}[device]
    with benchlock.hold("shared", f"module:check daemon resolution {device}"):
        syx, _ready = pk_harness.firmware_inputs(pk_harness.Cell(device, version))
        return str(pk_harness.daemon_binary(device, syx, pgo=None))


def commit_stage(mid: str, devices: list[str]) -> bool:
    """For a module with `[[machine]]` or `[[fx]]`: load it on the emulated device, read the rows it committed (tools/devtools/commit_records.ts, decoded by
    `patchkit records`), write `[uses.<device>]` into its module.toml and `build/gen/gallery.json` from them (docs/plans/sdk.md 6.1 and 6.4). The
    conformance suite that runs next reads that gallery.json. A tool or a service has no rows to commit and is left alone: returns False."""
    moddir = modroots.module_dir(mid)
    manifest_path = moddir / "module.toml"
    original = manifest_path.read_text(encoding="utf-8")
    if "[[machine]]" not in original and "[[fx]]" not in original:
        return False  # declares nothing through builder calls: no rows to measure
    # The measuring build takes the device limits as its `[uses]`: the loader refuses a module whose tables are absent, zero or lower than what its
    # init declares, so a new or changed module could not be measured with its own numbers. The measured rows replace them below.
    write_uses(manifest_path, {d: seed_uses(d) for d in devices})
    try:
        per = _measure(mid, devices)
    except BaseException:
        manifest_path.write_text(original, encoding="utf-8")
        raise
    if not per:
        manifest_path.write_text(original, encoding="utf-8")
        return False
    kept = tomllib.loads(original).get("uses", {})
    write_uses(
        manifest_path,
        {
            d: {k: int(kept[d].get(k, 0)) for k in USES_KEYS}
            for d in devices
            if d not in per and d in kept
        },
    )
    return _write_measured(mid, moddir, per, original)


def seed_uses(dev: str) -> dict[str, int]:
    """The `[uses.<dev>]` a module takes while it is measured: every row the device has."""
    from tools.patchkit import pk_device

    lim = pk_device.limits(dev)
    return {
        "machine_rows": lim.machine_rows,
        "param_rows": lim.param_rows,
        "page_rows": lim.page_rows,
        "record_bytes": 0,  # the loader does not check it; the measured size replaces it
    }


def _measure(mid: str, devices: list[str]) -> dict[str, dict]:
    """Pack MID for each device and read the rows its init committed on the emulated device. {} when no device packs it."""
    from tools.patchkit import patchkit, pk_committed

    per: dict[str, dict] = {}
    with tempfile.TemporaryDirectory(prefix="pk-commit-") as tmp:
        for dev in devices:
            out = Path(tmp) / f"{mid.rsplit('/', 1)[-1]}-{dev}.dkm"
            r = patchkit.run(
                ["pack", mid, "--device", dev, "-o", str(out)], capture=True
            )
            if r.returncode:
                continue  # not packed for this device (ports); the size lines say why
            per[dev] = pk_committed.committed(
                mid, dev, out, str(patchkit.binary()), _daemon(dev)
            )
    return per


def _write_measured(
    mid: str, moddir: Path, per: dict[str, dict], original: str
) -> bool:
    """Write the measured `[uses.<device>]` and `build/gen/gallery.json` of MID."""
    from tools.patchkit import pk_committed, pk_device

    manifest = tomllib.loads((moddir / "module.toml").read_text(encoding="utf-8"))
    ids = [m["id"] for m in manifest.get("machine", [])]
    counts = pk_committed.uses(per)
    write_uses(moddir / "module.toml", counts)
    changed = (moddir / "module.toml").read_text(encoding="utf-8") != original
    models = {d: pk_device.load(d)["modules"]["knobs"] for d in per}
    text = (
        json.dumps(
            pk_committed.gallery(
                mid, ids, per, models, [f["id"] for f in manifest.get("fx", [])]
            ),
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    gallery = moddir / "build/gen/gallery.json"
    if not gallery.is_file() or gallery.read_text(encoding="utf-8") != text:
        gallery.parent.mkdir(parents=True, exist_ok=True)
        gallery.write_text(text, encoding="utf-8")
    print(
        f"{mid}: committed rows {counts}; module.toml [uses.<device>] {'written' if changed else 'unchanged'}",
        flush=True,
    )
    return True


def resolve(name: str) -> str:
    mods = list(modroots.manifests())
    if name in mods:
        return name
    hit = [m for m in mods if m.split("/", 1)[1] == name]
    if len(hit) != 1:
        raise SystemExit(
            f"{name!r}: {'no such module' if not hit else 'ambiguous: ' + ', '.join(hit)}"
        )
    return hit[0]


def own_tests(mid: str) -> list[str]:
    """Test files of the module: `tests/**/test_*.py` in its folder."""
    found = sorted((modroots.module_dir(mid) / "tests").rglob("test_*.py"))
    return [p.relative_to(ROOT).as_posix() for p in found]


def _scenario_tests(kinds: set[str]) -> list[str]:
    """The tests/digikit test files that run scenarios of KINDS (their `cases((kinds), device)` call)."""
    files: set[str] = set()
    for path in sorted((ROOT / "tests/digikit").glob("test_*.py")):
        match = re.search(r"cases\(\(([^)]*)\)", path.read_text(encoding="utf-8"))
        if match and kinds.intersection(re.findall(r'"([a-z]+)"', match.group(1))):
            files.add(path.relative_to(ROOT).as_posix())
    return sorted(files)


def plan(mid: str, device: str | None) -> tuple[list[str], dict[str, str]]:
    """(pytest paths, extra environment) for module MID."""
    paths: list[str] = []
    env: dict[str, str] = {}
    if pk_conform.is_machine_module(mid):
        paths.append(CONFORMANCE)
        env["DIGI_CONFORM_MODULES"] = mid
        if device:
            env["DIGI_CONFORM_DEVICE"] = device
    if pk_conform.is_fx_module(mid):
        paths.append(FX_CONFORMANCE)
        env["DIGI_CONFORM_MODULES"] = mid
    paths += own_tests(mid)
    # the module's own scenarios (tests/scenarios/*.toml): the digikit test files of their kinds, limited to them, all tiers
    scen = sorted(
        p.stem for p in (modroots.module_dir(mid) / "tests/scenarios").glob("*.toml")
    )
    if scen:
        kinds = {
            tomllib.loads(p.read_text(encoding="utf-8"))
            .get("scenario", {})
            .get("kind", "")
            for p in (modroots.module_dir(mid) / "tests/scenarios").glob("*.toml")
        }
        paths += [t for t in _scenario_tests(kinds) if t not in paths]
        env["DIGIKIT_SCENARIOS"] = ",".join(scen)
        env["DIGIKIT_TIER"] = "full"
    return paths, env


def own_device_targets(mid: str) -> list[str]:
    """Nx device acceptance targets at the module root and its nested test-only owners."""
    from tools.devtools import nx_graph

    nodes = nx_graph.nx_graph()["nodes"]
    node = nodes.get(mid)
    if node is None:
        return []
    data = node["data"]
    owners = [mid] if "test-device" in data.get("targets", {}) else []
    root = data.get("root")
    if root:
        for name, candidate in nodes.items():
            candidate = candidate["data"]
            if (
                "test-only" in candidate.get("tags", [])
                and candidate.get("root", "").startswith(root + "/")
                and "test-device" in candidate.get("targets", {})
            ):
                owners.append(name)
    roots: dict[str, str] = {}
    for owner in sorted(owners):
        owner_root = nodes[owner]["data"].get("root")
        if owner_root and owner_root in roots:
            raise SystemExit(
                f"{mid}: ambiguous device acceptance root {owner_root}: {roots[owner_root]}, {owner}"
            )
        if owner_root:
            roots[owner_root] = owner
    return [f"{owner}:test-device-all" for owner in sorted(owners)]


def run_device_targets(mid: str, targets: list[str]) -> int:
    """Resolve both-device inputs once, then schedule all owned suites serially through Nx."""
    from tools.devtools import benchlock
    from tools.patchkit import pk_harness

    binaries = {}
    # The plain daemon build needs the same shared protection as the legacy harness path.
    # Nested shared resolution is supported; release this span before the target owns its work.
    with benchlock.hold("shared", f"module:check {mid} daemon resolution"):
        for device, version in (("dt2", "1.17"), ("dn2", "1.12")):
            syx, _ready = pk_harness.firmware_inputs(pk_harness.Cell(device, version))
            binaries[f"DIGI_EMUD_{device.upper()}"] = str(
                pk_harness.daemon_binary(device, syx, pgo=None)
            )
    print(f"{mid}: device acceptance {', '.join(targets)} (DT2 and DN2)", flush=True)
    # The launcher sets no-daemon and the job budget and takes the shared lock, so the hold above must end first.
    return subprocess.run(
        [
            "pnpm",
            "nx:pilot",
            "run-many",
            "-t",
            "test-device-all",
            "--projects",
            ",".join(t.rsplit(":", 1)[0] for t in targets),
            "--parallel=1",
        ],
        cwd=ROOT,
        env=python_env({**os.environ, **binaries}),
    ).returncode


def device_caps(dev: str) -> dict[str, dict[str, int]]:
    """CAPACITY of an empty device DEV as its device file sets the limits (nothing used): what a module must fit to load on it alone."""
    from tools.patchkit import pk_device

    lim = pk_device.limits(dev)
    sel = pk_device.contract_max()["modules"]

    def cap(limit: int, largest: int = 0) -> dict[str, int]:
        return {"limit": limit, "used": 0, "largest": largest}

    return {
        "MODULES": cap(lim.modules),
        "SELECTION": cap(sel),
        "ARENA": cap(lim.arena_bytes),
        "STAGE": cap(lim.stage_bytes, lim.stage_bytes),
        "DSP_AREA": cap(lim.dsp_area_bytes),
        "MACHINE_ROWS": cap(lim.machine_rows),
        "VOICE_ROWS": cap(lim.machine_rows),
        "PARAM_ROWS": cap(lim.param_rows),
        "PAGE_ROWS": cap(lim.page_rows),
        "FX_SLOTS": cap(lim.fx_slots),
    }


def needs_lines(dkm: bytes, dev: str) -> list[str]:
    """What a load of a packed module takes, inferred from the file (docs/design/digikit-dkm.md: `patchkit needs`, packages/dkneeds), and whether it
    fits the empty device DEV (the fit rule, `dkneeds.fit`)."""
    from tools.device import dkneeds

    try:
        n = dkneeds.needs_of(dkm)
    except dkneeds.NeedsError as e:
        return [
            f"{dev}: needs    none: {e}",
            f"{dev}: fits     NO: the loader refuses the file",
        ]
    shown = ", ".join(
        f"{label} {n[k]}"
        for label, k in (
            ("CPU memory", "ARENA"),
            ("CPU memory while the DSP code is sent", "ARENA_XFER"),
            ("DSP memory", "DSP_AREA"),
            ("machines", "MACHINE_ROWS"),
            ("voices", "VOICE_ROWS"),
            ("params", "PARAM_ROWS"),
            ("pages", "PAGE_ROWS"),
        )
        if n[k]
    )
    caps = device_caps(dev)
    short = dkneeds.fit(caps, [n], empty=True)["each"][0]
    verdict = (
        "fits an empty device"
        if short is None
        else f"DOES NOT FIT even an empty device: {short['describe']}"
    )
    return [
        f"{dev}: needs    {shown or 'nothing but its code'}",
        f"{dev}: fits     {verdict}",
    ]


def size_lines(mid: str, devices: list[str], trace: bool = False) -> list[str]:
    """The `size` line of `patchkit pack` for module MID, once per device (design rule D5), then what a load of it takes (inferred from the file) and whether it fits an
    empty device. A module that does not pack (no ColdFire object, or several, e.g. a patched-in module) gets a "not packed" line with the reason."""
    from tools.patchkit import patchkit

    lines: list[str] = []
    with tempfile.TemporaryDirectory(prefix="pk-modcheck-") as tmp:
        for dev in devices:
            out = Path(tmp) / f"{mid.rsplit('/', 1)[-1]}-{dev}.dkm"
            r = patchkit.run(
                [
                    "pack",
                    mid,
                    "--device",
                    dev,
                    "-o",
                    str(out),
                    *(["--trace"] if trace else []),
                ],
                capture=True,
            )
            if r.returncode:
                why = (r.stderr or r.stdout).strip().splitlines()
                lines.append(
                    f"{dev}: size     not packed ({why[-1] if why else 'pack failed'})"
                )
                continue
            lines += [
                f"{dev}: size     {ln.split(': ', 1)[-1]}"
                for ln in r.stdout.splitlines()
                if ln.startswith("size ")
            ]
            lines += needs_lines(out.read_bytes(), dev)
    return lines


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("name")
    ap.add_argument("--device", choices=pk_conform.DEVICES)
    ap.add_argument(
        "--trace",
        action="store_true",
        help="pack the size lines with DK_TRACE defined (patchkit pack --trace); the conformance and own tests run the module as stored",
    )
    a = ap.parse_args()
    mid = resolve(a.name)
    commit_stage(mid, [a.device] if a.device else list(pk_conform.DEVICES))
    paths, env = plan(mid, a.device)
    targets = own_device_targets(mid)
    kind = (
        "machine module"
        if CONFORMANCE in paths
        else "effect module"
        if FX_CONFORMANCE in paths
        else "module (no machine or effect DSP: no conformance suite)"
    )
    print(f"{mid}: {kind}; pytest {' '.join(paths) or '(nothing to run)'}", flush=True)
    devices = [a.device] if a.device else list(pk_conform.DEVICES)
    if not paths:
        from tools.patchkit import patchkit

        try:
            patchkit.build_module(modroots.module_dir(mid), check=True)
        except patchkit.BuildError as e:
            print(f"{mid}: {e}", file=sys.stderr)
            return 1
    else:
        cmd = [
            sys.executable,
            "-m",
            "pytest",
            "-q",
            "-p",
            "no:cacheprovider",
            "-rf",
            *paths,
        ]
        rc = subprocess.run(
            cmd, cwd=ROOT, env=python_env({**os.environ, **env})
        ).returncode
        if rc:
            return rc
    if targets:
        rc = run_device_targets(mid, targets)
        if rc:
            return rc
    for line in size_lines(mid, devices, a.trace):
        print(line, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
