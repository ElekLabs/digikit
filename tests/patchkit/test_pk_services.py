"""Services of a development card: service detection, needed services of packed modules, bound-file staleness."""

import json
from pathlib import Path

import pytest

from tools.patchkit import modroots, pk_services

PACKED = (
    modroots.ROOT / "dist" / "modules" / "dn2"
)  # written by the Nx target `<id>:pack`


def packed(name: str) -> Path:
    path = PACKED / f"{name}.dkm"
    if not path.is_file():
        pytest.skip(f"{path} is not built (nx run <id>:pack)")
    return path


def test_services_come_from_the_manifests():
    services = pk_services.service_modules()
    assert services["machines"] == "core/services/machines"
    assert services["fx"] == "core/services/fx"
    assert pk_services.service_name("core/services/machines") == "machines"
    assert pk_services.service_name("test/dyn-dn2") is None


def test_store_path():
    assert pk_services.store_path("machines") == "services/machines.dks"


def test_machine_module_needs_machines_service():
    dkm = packed("dyn-dn2")
    assert pk_services.name_hash("machines") in pk_services.needed_hashes(
        dkm.read_bytes()
    )
    assert pk_services.needed_services([dkm]) == ["machines"]


def test_effect_module_needs_fx_service():
    assert pk_services.needed_services([packed("dyn-fx-gain")]) == ["fx"]


def test_needs_of_several_files_are_joined_once():
    files = [packed("dyn-dn2"), packed("sine"), packed("dyn-fx-gain")]
    assert pk_services.needed_services(files) == ["fx", "machines"]


def test_file_without_header_table_needs_nothing(tmp_path):
    stub = tmp_path / "stub.dkm"
    stub.write_bytes(b"\0" * 64)
    assert pk_services.needed_services([stub]) == []


def test_stale_binding_follows_the_sidecar(tmp_path):
    unbound, firmware, out = (
        tmp_path / "m.dks",
        tmp_path / "fw.syx",
        tmp_path / "m.dks.bound.dks",
    )
    unbound.write_bytes(b"packed")
    firmware.write_bytes(b"image")
    out.write_bytes(b"bound")
    assert "no .src.json" in pk_services.stale_binding(out, unbound, firmware)
    sidecar = out.with_suffix(".src.json")
    sidecar.write_text(
        json.dumps(
            {
                "unbound_sha256": pk_services._sha256(unbound),
                "firmware_sha256": pk_services._sha256(firmware),
            }
        )
    )
    assert pk_services.stale_binding(out, unbound, firmware) is None
    unbound.write_bytes(b"packed again")
    assert "another m.dks" in pk_services.stale_binding(out, unbound, firmware)
    unbound.write_bytes(b"packed")
    firmware.write_bytes(b"other image")
    assert "firmware" in pk_services.stale_binding(out, unbound, firmware)
