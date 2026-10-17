"""The encoding gate refuses added DSP instructions whose encoding shape is absent from stock code."""

import json
import tomllib

import pytest

from tools.patchkit import pk_encoding_gate as gate
from tools.patchkit import pk_toolchains

ROOT = pk_toolchains.ROOT
STOCK_DB = ROOT / "out" / "sharcdb" / "dt2-1.17.sqlite"
needs_db = pytest.mark.skipif(
    not STOCK_DB.exists(), reason="needs out/sharcdb/dt2-1.17.sqlite"
)


class Words:
    """Loader-memory stand-in holding one listing-order instruction at short word 0."""

    def __init__(self, listing_hex: str):
        # Listing order is most significant parcel first; memory holds each parcel little endian.
        self.data = b"".join(
            bytes.fromhex(listing_hex[i : i + 4])[::-1]
            for i in range(0, len(listing_hex), 4)
        )

    def read_sw(self, pc_sw: int, size: int = 6) -> bytes | None:
        lo = 2 * pc_sw
        return self.data[lo : lo + size] if lo + size <= len(self.data) else None


def run(listing_hex: str, allowed: set[str]) -> list[gate.Violation]:
    mem = Words(listing_hex)
    return gate.check_code(mem, [(0, len(mem.data) // 2)], allowed)


@pytest.fixture(scope="module")
def stock() -> set[str]:
    return gate.stock_shapes(STOCK_DB)


@needs_db
def test_selache_old_negative_shift_is_rejected(stock):
    got = run("023e0000e816", stock)
    assert [(v.pc, v.raw) for v in got] == [(0, "023e0000e816")]
    assert got[0].shape.startswith("6b_shiftimm")


@needs_db
def test_stock_negative_shift_is_allowed(stock):
    assert run("023e7800e816", stock) == []


@needs_db
def test_short_and_fsub_forms_are_allowed(stock):
    assert run("01840262", stock) == []
    assert run("01882710", stock) == []


def test_unknown_shape_reports_pc_and_raw():
    got = run("01840262" + "01882710", set())
    assert [(v.pc, v.raw) for v in got] == [(0, "01840262"), (2, "01882710")]
    assert all(v.shape for v in got)


def test_register_and_immediate_values_do_not_change_the_shape():
    a = gate.shape(gate.decode_listing("023e00000410"))  # R1 = LSHIFT R0 BY 4
    b = gate.shape(gate.decode_listing("023e0000072f"))  # other regs, amount 7
    assert a == b
    assert a != gate.shape(gate.decode_listing("023e7800e816"))  # negative amount


def test_proven_toml_entries_are_documented():
    doc = tomllib.loads(gate.PROVEN.read_text())
    for e in doc.get("proven", []):
        assert e["shape"] and e["evidence"] and e["note"]


def test_proven_shapes_extend_the_allowed_set(tmp_path):
    f = tmp_path / "p.toml"
    f.write_text('[[proven]]\nshape = "x:1"\nevidence = "img 2026-10-17"\nnote = "n"\n')
    assert gate.proven_shapes(f) == {"x:1"}


@pytest.mark.slow
@needs_db
def test_check_image_on_pluck_image_runs():
    syx = ROOT / "out" / "patch" / "pluck-1.17.syx"
    plan = syx.with_suffix(".plan.json")
    if not syx.exists() or not plan.exists():
        pytest.skip("needs out/patch/pluck-1.17.syx")
    assert isinstance(gate.check_image(syx, plan), list)


@needs_db
def test_check_image_on_xslice_image_has_no_dsp_code():
    syx = ROOT / "out" / "patch" / "diag-xslice-1.17.syx"
    plan = syx.with_suffix(".plan.json")
    if not syx.exists() or not plan.exists():
        pytest.skip("needs out/patch/diag-xslice-1.17.syx")
    assert gate.check_image(syx, plan) == []
    assert not [h for h in json.loads(plan.read_text())["hooks"] if h["cpu"] == "sharc"]


def test_cmd_patch_gate_deletes_output_and_exits_1(tmp_path, monkeypatch, capsys):
    from tools.patchkit import patchkit

    out = tmp_path / "o.syx"
    out.write_bytes(b"x")
    viol = [gate.Violation(0xB80000, "023e0000e816", "6b_shiftimm:x")]
    monkeypatch.setattr(gate, "check_image", lambda *a, **k: viol)
    assert patchkit.encoding_gate(out, tmp_path / "p.json", True) == 1
    assert not out.exists()
    assert "0xb80000" in capsys.readouterr().err


def test_cmd_patch_no_gate_flag_keeps_output_and_warns(tmp_path, monkeypatch, capsys):
    from tools.patchkit import patchkit

    out = tmp_path / "o.syx"
    out.write_bytes(b"x")
    viol = [gate.Violation(0xB80000, "023e0000e816", "6b_shiftimm:x")]
    monkeypatch.setattr(gate, "check_image", lambda *a, **k: viol)
    assert patchkit.encoding_gate(out, tmp_path / "p.json", False) == 0
    assert out.exists()
    err = capsys.readouterr().err
    assert "WARNING" in err and "023e0000e816" in err
