"""Device data (docs/design/device-data.md): the facts that code repeats are read from `devices/*.toml`, and the rows that a module
reserves match the numbers the data give. `patchkit gen --check` (tests/patchkit gen checks in the Rust tests) keeps the generated
constants current; these tests keep the hand-written parts of the modules consistent with the data."""

from __future__ import annotations

import tomllib

from support.paths import REPO_ROOT

from tools.device import dkproto
from tools.patchkit import modroots, pk_device, port_resolve

ROOT = REPO_ROOT
TYPES = tomllib.loads(
    (ROOT / "image-modules" / "points" / "machines" / "types.toml").read_text()
)


def module(mid: str) -> dict:
    return tomllib.loads((modroots.module_dir(mid) / "module.toml").read_text())


def test_the_store_geometry_is_the_device_files() -> None:
    tail_devices = ("dt2", "dn2")
    for dev in pk_device.store_devices():
        s = pk_device.store_of(dev)
        card = pk_device.load(dev)["card"]
        store = card["store"]
        assert s.first == store["first_sector"]
        assert s.first + s.sectors == store["end_sector"]
        assert s.header_b == store["header_b_sector"]
        assert (s.sel_a, s.sel_b) == (store["end_sector"] - 2, store["end_sector"] - 1)
        assert store["selection_sectors"] == 2
        # header B, the settings record (two copies of the sdk's size), the selection record
        assert store["settings_sectors"] == 2 * dkproto.SETTINGS_SECTORS
        assert s.header_b == store["end_sector"] - 2 - store["settings_sectors"] - 1
        assert s.set_a == store["end_sector"] - 2 - store["settings_sectors"]
        assert s.set_b == s.set_a + dkproto.SETTINGS_SECTORS
        assert s.set_b + dkproto.SETTINGS_SECTORS == s.sel_a
        # The project pool ends at the MmcFs boundary (findings 59 and 154).
        projects = card["pools"]["projects"]
        stock_end = max(
            projects["base_sector"] + projects["slots"] * projects["stride_sectors"],
            card.get("path_fs", {}).get("end_sector", 0),
            *(
                row["end"]
                for row in card.get("guard", {}).get("row", [])
                if row["who"] == "stock"
            ),
        )
        assert stock_end <= s.first < s.end <= card["total_sectors"], dev
        if dev in tail_devices:
            assert s.end == card["total_sectors"], dev
    # the tail stores have different starts: the runtime reads its own from the device object
    assert len({pk_device.store_of(dev).first for dev in tail_devices}) == len(
        tail_devices
    )


def test_dynreg_reserves_one_machine_row_per_run_time_type() -> None:
    d = module("core/dynreg")
    for dev in pk_device.dsp_devices():
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


def test_dyndsp_reserves_one_voice_row_per_run_time_type() -> None:
    rows = [e for e in module("core/dyndsp")["entry"] if e["registry"] == "voices"]
    for dev in pk_device.dsp_devices():
        t = TYPES[dev]
        assert len(rows) == t["last"] - t["dynamic_first"] + 1, dev


def test_digikit_ids_and_names_are_unique_and_fit_the_info_reply() -> None:
    devs = [pk_device.load(d) for d in pk_device.store_devices()]
    ids = [d["digikit"]["id"] for d in devs]
    assert len(ids) == len(set(ids))
    assert len({d["device"]["syx_id"] for d in devs}) == len(devs)
    for d in devs:
        assert 1 <= len(d["digikit"]["name"]) <= 3


def test_port_resolve_reads_its_tables_from_the_device_files() -> None:
    # every firmware that has a port lockfile is flagged, and the other way round
    ports = {p.stem for p in (ROOT / "ports").glob("*.toml")}
    flagged = {
        f"{p['device']}-{p['version']}"
        for p in port_resolve.load_profiles().values()
        if p["port"]
    }
    assert flagged == ports
    assert len(port_resolve.ported_firmware()) == len(ports)
    # the limits written into a port are the device's `[patch]` (a device without a DSP has no dsp_* limits)
    for pf in (ROOT / "ports").glob("*.toml"):
        port = tomllib.loads(pf.read_text())
        patch = pk_device.load(port["port"]["device"])["patch"]
        assert port["limits"] == {
            k: patch[k] for k in port_resolve.LIMIT_KEYS if k in patch
        }


def test_the_harness_inputs_come_from_the_device_files() -> None:
    ports = {p.stem for p in (ROOT / "ports").glob("*.toml")}
    assert {f"{s}-{v}" for s, v in pk_device.port_firmware()} == ports
    for short, version in pk_device.acceptance_firmware().items():
        acc = pk_device.load(short)["acceptance"]
        assert pk_device.ready_dir(short, version) == acc["ready_dir"]
    # a port firmware that is not the acceptance firmware gets the conventional ready directory
    other = next(
        (s, v)
        for s, v in pk_device.port_firmware()
        if pk_device.acceptance_firmware().get(s) != v
    )
    assert (
        pk_device.ready_dir(*other)
        == f"out/ready-{other[0]}-{other[1].replace('.', '')}"
    )


def test_the_dsp_section_is_the_container_role_and_needs_a_dsp_image() -> None:
    profiles = port_resolve.load_profiles()
    for prof in profiles.values():
        if prof["dsp"]:
            assert port_resolve.dsp_section(prof) == prof["container"]["dsp"]
    assert port_resolve.dsp_section({"dsp": None, "container": {"dsp": 7}}) is None
