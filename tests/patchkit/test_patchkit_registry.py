"""The run-time registry (core/dynreg, the runtime's registry.rs, `patchkit pack` of a dynamic module): machines and
parameters that a `.dkm` adds at run time (docs/design/digikit-registry.md, finding 30).

The committed objects are current; the packer lowers a dynamic module's typed sections into `register` records
deterministically; the emulator proofs are the boot-time scenarios of tests/digikit (the firmware's own list getter and rows with the module
selected at boot, the PCM of the machine). The hot-load proofs that were here (digikit-hot-reload-v1:tools/patchkit/pk_registry.py: mailbox load,
refuse, unload, the panel frames) are archived with hot reload (tag digikit-hot-reload-v1)."""

import json

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.patchkit import pk_build, pk_dkrt, pk_toolchains  # noqa: E402
from tools.patchkit.pk_dkrt_machine import machine_hash  # noqa: E402

DYNREG = ROOT / "modules/core/dynreg"
RUNTIME = ROOT / "modules/core/runtime"


def test_committed_objects_are_current_and_plain_coldfire():
    for dev in ("dt2", "dn2"):
        obj = json.loads((DYNREG / f"build/dynreg-{dev}.dko.json").read_text())
        assert {r["type"] for r in obj["relocs"]} <= {"cf.32", "cf.pc32", "cf.pc16"}
    pk_build.build_module(DYNREG, check=True)
    if pk_toolchains.codegen_gcc_ready():
        pk_build.build_module(RUNTIME, check=True)


@pytest.mark.skip(
    reason="this fork retires the run-time machine range (core/machines types.toml)"
)
def test_pack_lowers_the_typed_sections_deterministically(tmp_path):
    tools = pk_dkrt.Tools()
    a, b = tmp_path / "a.dkm", tmp_path / "b.dkm"
    for out in (a, b):
        tools.patchkit_run("pack", "test/dyn-xslice", "--device", "dt2", "-o", out)
    assert a.read_bytes() == b.read_bytes()
    info = tools.patchkit_run("dkm-info", a)
    assert "device 1 (1 params, 1 machines, 0 pages)" in info
    # the type field is the machine key: the hash of "<module id>:<machine id>" (D7); the runtime assigns the number at load
    assert (
        f'machine type h{machine_hash("test/dyn-xslice", "xslice_d"):06x}+16 "XSlice-D" "XSD" clone_of 6'
        in info
    )
    # a run-time module needs its device, and a machine module that is not dynamic cannot be packed without code
    r = tools.run(
        tools.patchkit,
        "--root",
        ROOT,
        "pack",
        "test/dyn-xslice",
        "-o",
        tmp_path / "x.dkm",
    )
    assert r.returncode != 0 and "--device" in r.stderr
    # the Digitone II module: machine (own_dsp), four parameters and one page, packed deterministically
    c, d = tmp_path / "c.dkm", tmp_path / "d.dkm"
    for out in (c, d):
        tools.patchkit_run("pack", "test/dyn-dn2", "--device", "dn2", "-o", out)
    assert c.read_bytes() == d.read_bytes()
    info = tools.patchkit_run("dkm-info", c)
    assert "device 2 (4 params, 1 machines, 1 pages)" in info
    h = f"h{machine_hash('test/dyn-dn2', 'dyn_pluck'):06x}+16"
    assert f'machine type {h} "DynSyn" "DSY" clone_of 0 level 10 own_dsp 1' in info
    assert f'page of type {h} "DYN" "Dyn Synth"' in info
    # a Digitakt II module has no page, and a Digitone II one is refused on the Digitakt
    r = tools.run(
        tools.patchkit,
        "--root",
        ROOT,
        "pack",
        "test/dyn-dn2",
        "--device",
        "dt2",
        "-o",
        tmp_path / "z.dkm",
    )
    assert r.returncode != 0
    r = tools.run(
        tools.patchkit,
        "--root",
        ROOT,
        "pack",
        "test/xslice",
        "--device",
        "dt2",
        "-o",
        tmp_path / "y.dkm",
    )
    assert r.returncode != 0
