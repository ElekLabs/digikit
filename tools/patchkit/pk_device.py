"""Device facts for the patchkit tools, read from the device files (`devices/*.toml`) instead of being repeated in code.

    from tools.patchkit import pk_device
    s = pk_device.store_of("dt2")  # the module store's geometry of a device (absolute sectors): the area differs per device
    pk_device.runtime_devices()  # short names of the runtime targets (`[digikit]`)
    pk_device.store_devices()    # the ones of them that have a module store
    pk_device.dsp_devices()      # the ones of them that have a module store and a DSP

The store area is per device (docs/design/digikit-store.md): the runtime reads it from its device object (`devices/*.toml [card.store]`).
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]


@cache
def load_all(root: Path = ROOT) -> dict[str, dict]:
    """short name -> the parsed device file (files without a `[device]` table, the LED maps, are skipped)."""
    out = {}
    for f in sorted((root / "devices").glob("*.toml")):
        d = tomllib.loads(f.read_text())
        if "device" in d:
            out[d["device"]["short"]] = d
    return out


def load(short: str) -> dict:
    try:
        return load_all()[short]
    except KeyError:
        raise KeyError(f"no device file for {short!r}: {sorted(load_all())}") from None


def runtime_devices() -> list[str]:
    """Devices with a `[digikit]` section: the runtime runs there (it is one image: what its static arrays hold is the same on every one)."""
    return [s for s, d in load_all().items() if "digikit" in d]


def store_devices() -> list[str]:
    """Devices with a `[card.store]` and a `[digikit]` section: the runtime runs there and keeps its module store on the card."""
    return [
        s
        for s, d in load_all().items()
        if "digikit" in d and "store" in d.get("card", {})
    ]


def dsp_devices() -> list[str]:
    """The `store_devices` that have a DSP (`[capabilities] dsp`): the targets of the run-time machine, effect and DSP tools and tests."""
    return [s for s in store_devices() if load(s)["capabilities"]["dsp"] != "none"]


@dataclass(frozen=True)
class Store:
    first: int  # first card sector of the store
    sectors: int  # sectors in the area, with header copy B, the settings record and the selection record at the end
    sel_a: int  # selection record copy A (absolute)
    sel_b: int  # selection record copy B (absolute)
    header_b: int  # store header copy B (absolute); the store's files end before it
    set_a: int = 0  # settings record copy A (absolute first sector); copy B follows `set_copy_sectors` sectors later
    set_b: int = 0
    set_copy_sectors: int = 16  # sectors of one settings copy (sdk/sdk.toml [settings])

    @property
    def limit(self) -> int:
        """Sectors a store (header A, table, data) may use from `first`."""
        return self.header_b - self.first

    @property
    def end(self) -> int:
        """The first sector after the area."""
        return self.first + self.sectors


def store_of(short: str) -> Store:
    s = load(short)["card"]["store"]
    first, end = s["first_sector"], s["end_sector"]
    sel = s.get("selection_sectors", 2)
    settings = s.get(
        "settings_sectors", 32
    )  # two copies of sdk/sdk.toml [settings] sectors, before the selection record
    set_a = end - sel - settings
    return Store(
        first,
        end - first,
        end - sel,
        end - sel + 1,
        s["header_b_sector"],
        set_a,
        set_a + settings // 2,
        settings // 2,
    )


@dataclass(frozen=True)
class Limits:
    """A device's capacity limits (`[limits]` and `[digikit] dsp_area_bytes` of its file); see docs/plans/capacity-isolation.md."""

    modules: int  # module records (the selection record holds `sel_max` paths)
    machine_rows: int  # run-time machine types = machine rows = DSP voice rows
    arena_bytes: int  # the ColdFire module arena
    stage_bytes: int  # the staging buffer: the largest .dkm file
    fx_slots: int  # effects in the chain
    send_slots: int  # of them effects on a send bus (the device file's limit; `effective_send_slots` is what an image reports)
    param_rows: int  # parameter rows core/rows reserves
    page_rows: int  # page rows core/rows reserves (0 on a device without)
    dsp_area_bytes: int  # the DSP run-time area


@cache
def contract_max() -> dict[str, int]:
    """`[limits.max]` of `sdk/sdk.toml`: the contract maxima the device limits may not pass. `sel_max` is the selection record's path count
    (the same number as `modules`: the record format)."""
    m = tomllib.loads((ROOT / "sdk" / "sdk.toml").read_text())["limits"]["max"]
    return {**m, "sel_max": m["modules"]}


def effective_fx_slots(lim: Limits, plan: dict[str, Any]) -> int:
    """The effect slots an image reports in CAPACITY: the device's `fx_slots` when the image has `core/dynfx` (its free effect rows), else 0 (an image that cannot
    load an effect, such as every Digitone II image until F4 step 5, reports no slot)."""
    return (
        lim.fx_slots
        if any(str(m).startswith("core/dynfx@") for m in plan["modules"])
        else 0
    )


def effective_send_slots(short: str, plan: dict[str, Any]) -> int:
    """The send slots an image of device SHORT reports in CAPACITY: the device's `send_slots` when the image has `core/fxpages` (the send page) and the device file has a send page
    (`[fx] track_page`), else 0 (every Digitone II image until FX routing step 6)."""
    has_page = any(str(m).startswith("core/fxpages@") for m in plan["modules"])
    if not has_page or load(short).get("fx", {}).get("track_page", 0) == 0:
        return 0
    return limits(short).send_slots


def limits(short: str) -> Limits:
    """The capacity limits of device SHORT, read from its device file (generated into the runtime by `patchkit gen`)."""
    d = load(short)
    return Limits(
        **{
            k: d["limits"][k]
            for k in Limits.__dataclass_fields__
            if k != "dsp_area_bytes"
        },
        dsp_area_bytes=d["digikit"]["dsp_area_bytes"],
    )


def shared_limits() -> Limits:
    """The limits every digikit device agrees on, except the per-device ones (`dsp_area_bytes`, `fx_slots`, `page_rows`), which are
    taken from the first DSP device. Raises when a shared limit differs: the runtime is one image for all devices. The module count and
    the arena and staging sizes are the runtime's static arrays and are checked on every runtime target; the machine and parameter rows
    exist only where there is a DSP, and are checked on those."""
    dsp = dsp_devices()
    first = limits(dsp[0])
    for key, devs in (
        ("modules", runtime_devices()),
        ("arena_bytes", runtime_devices()),
        ("stage_bytes", runtime_devices()),
        ("machine_rows", dsp),
        ("param_rows", dsp),
    ):
        want = getattr(first, key)
        for d in devs:
            have = load(d)["limits"][key]
            if have != want:
                raise RuntimeError(
                    f"[limits] {key} differs between {dsp[0]} and {d}; the runtime is one image for all devices"
                )
    return first


def peek_ranges(short: str) -> list[tuple[int, int]]:
    """RAM ranges `[lo, hi)` the PEEK command of the upload protocol may read (`[digikit] peek`)."""
    return [(lo, hi) for lo, hi in load(short)["digikit"]["peek"]]


def keep_range(short: str) -> tuple[int, int]:
    """`(base, bytes)` of the RAM the crash record lives in and the emulator keeps across a soft reset (`[digikit] keep_base`, `keep_bytes`)."""
    k = load(short)["digikit"]
    return k["keep_base"], k["keep_bytes"]


def working_project_sector(short: str) -> int:
    """First sector of the working project record of device SHORT (`[[card.regions]]` role `working-project`): the card above it is the firmware's
    (project slots follow at the first project pool sector)."""
    found = [
        r["sector"]
        for r in load(short)["card"].get("regions", [])
        if r["role"] == "working-project"
    ]
    if len(found) != 1:
        raise RuntimeError(
            f"{short}: expected one working-project region, found {len(found)}"
        )
    return found[0]


def port_images(short: str) -> tuple[str, ...]:
    """Image keys (`dt2-1.17`) of the firmware of a device that has a port lockfile, in device-file order."""
    return tuple(
        f"{short}-{fw['version']}"
        for fw in load(short).get("firmware", [])
        if fw.get("port")
    )


def acceptance_firmware() -> dict[str, str]:
    """short name -> the firmware version the device's tests and tools run (`[acceptance] firmware`)."""
    return {
        s: d["acceptance"]["firmware"]
        for s, d in load_all().items()
        if "acceptance" in d
    }


def port_firmware() -> dict[tuple[str, str], str]:
    """(short name, version) -> .syx file name of every firmware that has a port lockfile."""
    return {
        (s, fw["version"]): fw["filename"]
        for s, d in load_all().items()
        for fw in d.get("firmware", [])
        if fw.get("port")
    }


def ready_dir(short: str, version: str) -> str:
    """The ready-snapshot directory of a firmware: `[acceptance] ready_dir` for the acceptance firmware, else
    `out/ready-<short>-<version without dots>`."""
    acc = load(short).get("acceptance", {})
    if acc.get("firmware") == version and "ready_dir" in acc:
        return acc["ready_dir"]
    return f"out/ready-{short}-{version.replace('.', '')}"
