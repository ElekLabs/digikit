"""Device data (docs/design/device-data.md): the facts that code repeats are read from `devices/*.toml`, and the rows that a module
reserves match the numbers the data give. `patchkit gen --check` (tests/patchkit gen checks in the Rust tests) keeps the generated
constants current; these tests keep the hand-written parts of the modules consistent with the data."""

from __future__ import annotations

import tomllib

import pytest
from support.paths import REPO_ROOT

from tools.device import dkproto
from tools.patchkit import modroots, pk_device, port_resolve

ROOT = REPO_ROOT
TYPES = tomllib.loads(
    (ROOT / "modules" / "core" / "machines" / "types.toml").read_text()
)


def module(mid: str) -> dict:
    return tomllib.loads((modroots.module_dir(mid) / "module.toml").read_text())


def test_the_store_geometry_is_the_device_files() -> None:
    for dev in pk_device.digikit_devices():
        s = pk_device.store_of(dev)
        card = pk_device.load(dev)["card"]["store"]
        assert s.first == card["first_sector"]
        assert s.first + s.sectors == card["end_sector"]
        assert s.header_b == card["header_b_sector"]
        assert (s.sel_a, s.sel_b) == (card["end_sector"] - 2, card["end_sector"] - 1)
        assert card["selection_sectors"] == 2
        # header B, the settings record (two copies of the sdk's size), the selection record
        assert card["settings_sectors"] == 2 * dkproto.SETTINGS_SECTORS
        assert s.header_b == card["end_sector"] - 2 - card["settings_sectors"] - 1
        assert s.set_a == card["end_sector"] - 2 - card["settings_sectors"]
        assert s.set_b == s.set_a + dkproto.SETTINGS_SECTORS
        assert s.set_b + dkproto.SETTINGS_SECTORS == s.sel_a
        # the store lies above the MmcFs write guard and ends with the card (finding 59)
        assert s.first > 0x457FFF
        assert s.end == pk_device.load(dev)["card"]["total_sectors"]
    # the two devices have different areas: the runtime reads its own from the device object
    assert pk_device.store_of("dt2").first != pk_device.store_of("dn2").first


@pytest.mark.skip(
    reason="this fork retires the run-time machine range (core/machines types.toml)"
)
def test_dynreg_reserves_one_machine_row_per_run_time_type() -> None:
    d = module("core/dynreg")
    for dev in pk_device.digikit_devices():
        t = TYPES[dev]
        want = [f"dyn{n}" for n in range(t["dynamic_first"], t["last"] + 1)]
        rows = [
            e["id"]
            for e in d["entry"]
            if e["registry"] == "machines" and dev in (e.get("devices") or [dev])
        ]
        assert rows == want, dev
        # and each row has its permanent number in types.toml
        for n in range(t["dynamic_first"], t["last"] + 1):
            assert t["alloc"][f"core/dynreg:dyn{n}"] == n


@pytest.mark.skip(
    reason="this fork retires the run-time machine range (core/machines types.toml)"
)
def test_dyndsp_reserves_one_voice_row_per_run_time_type() -> None:
    rows = [e for e in module("core/dyndsp")["entry"] if e["registry"] == "voices"]
    for dev in pk_device.digikit_devices():
        t = TYPES[dev]
        assert len(rows) == t["last"] - t["dynamic_first"] + 1, dev


def test_digikit_ids_and_names_are_unique_and_fit_the_info_reply() -> None:
    devs = [pk_device.load(d) for d in pk_device.digikit_devices()]
    ids = [d["digikit"]["id"] for d in devs]
    assert len(ids) == len(set(ids))
    assert len({d["device"]["syx_id"] for d in devs}) == len(devs)
    for d in devs:
        assert 1 <= len(d["digikit"]["name"]) <= 3


def test_port_resolve_reads_its_tables_from_the_device_files() -> None:
    # every firmware that has a port lockfile is flagged, and the other way round
    ports = {p.stem for p in (ROOT / "devices" / "ports").glob("*.toml")}
    flagged = {
        f"{p['device']}-{p['version']}"
        for p in port_resolve.load_profiles().values()
        if p["port"]
    }
    assert flagged == ports
    assert len(port_resolve.ported_firmware()) == len(ports)
    # the limits written into a port are the device's `[patch]`
    for pf in (ROOT / "devices" / "ports").glob("*.toml"):
        port = tomllib.loads(pf.read_text())
        patch = pk_device.load(port["port"]["device"])["patch"]
        assert port["limits"] == {k: patch[k] for k in port_resolve.LIMIT_KEYS}
