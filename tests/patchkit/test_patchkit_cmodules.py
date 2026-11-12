"""C modules on the ColdFire V4e (finding 35): the libgcc helpers are ColdFire code, and the ISA checker refuses anything else.

Static: every committed ColdFire object decodes as ISA_A/B/C + EMAC; the toolchain's stock libgcc (68000 code) is refused;
the -mcpu=54418 libgcc archive passes; test/c-demo links its helpers locally (no undefined symbol, no exported helper).
Emulator: the packed c-demo is loaded by the boot hook and its 64-bit results equal Python's (dt2-1.17 and dn2-1.12)."""

import json
import subprocess
from pathlib import Path

import pytest
from support.paths import REPO_ROOT

from tools.coldfire import isacheck
from tools.firmware import config
from tools.patchkit import patchkit as front_end  # noqa: E402
from tools.patchkit import pk_cdemo, pk_dkrt, pk_toolchains

ROOT = REPO_ROOT
DEMO = ROOT / "tests/fixtures/modules/c-demo"
needs_gcc = pytest.mark.skipif(
    pk_toolchains.m68k_bin("gcc") is None, reason="m68k-elf toolchain not installed"
)


def test_every_committed_coldfire_object_is_plain_v4e():
    objs = isacheck.committed_objects(ROOT)
    assert objs
    for path in [*objs, *isacheck.cached_objects(ROOT)]:
        isacheck.check_file(path)  # raises IsaError with the refused forms


def test_the_checker_refuses_a_68020_form(tmp_path):
    # divs.l d1,d2:d0 (64-bit dividend) and bfextu: 68020 forms with no ColdFire encoding
    blob = bytes.fromhex("4c41 0c02 e9c0 1000 4e75".replace(" ", ""))
    obj = tmp_path / "bad.dko.json"
    obj.write_text(
        json.dumps(
            {
                "format": 1,
                "arch": "cf",
                "source_sha256": "0" * 64,
                "sections": [
                    {
                        "name": ".text",
                        "kind": "text",
                        "align": 4,
                        "size": len(blob),
                        "hex": blob.hex(),
                    }
                ],
                "symbols": [],
                "relocs": [],
            }
        )
    )
    with pytest.raises(isacheck.IsaError):
        isacheck.check_file(obj)


@needs_gcc
def test_the_stock_libgcc_is_refused_and_the_coldfire_one_passes():
    stock = Path(
        subprocess.run(
            [str(pk_toolchains.m68k_bin("gcc")), "-print-libgcc-file-name"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    )
    if stock.exists():
        with pytest.raises(isacheck.IsaError):
            isacheck.check_file(stock)
    lib = pk_toolchains.libgcc_cf()
    if lib is None:
        pytest.skip("pk_toolchains libgcc-cf not run")
    isacheck.check_file(lib)


@needs_gcc
def test_c_demo_object_is_current_and_self_contained():
    if pk_toolchains.libgcc_cf() is None:
        pytest.skip("pk_toolchains libgcc-cf not run")
    front_end.build_module(DEMO)
    built = ROOT / "out/cache/dko/test/c-demo/build/c-demo.dko.json"
    obj = json.loads(built.read_text())
    assert [s["name"] for s in obj["symbols"] if s["section"] is None] == []
    exported = {s["name"] for s in obj["symbols"] if s["global"]}
    assert exported == {"dk_module", "c_demo_in", "c_demo_n", "c_demo_out"}
    assert {r["type"] for r in obj["relocs"]} <= {"cf.32", "cf.pc32"}
    isacheck.check_file(built)


@pytest.mark.parametrize("image", ["dt2-1.17", "dn2-1.12"])
def test_boot_loads_c_demo_and_its_64_bit_results_are_right(image, tmp_path):
    if not config.stock(pk_dkrt.DEVICE_SYX[image]).exists():
        pytest.skip(f"{pk_dkrt.DEVICE_SYX[image]} missing")
    report = pk_cdemo.proof(image, tmp_path)
    failed = [c for c in report["checks"] if not c["ok"]]
    assert not failed, failed
