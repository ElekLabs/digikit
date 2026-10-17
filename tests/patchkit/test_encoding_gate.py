"""The encoding gate refuses added DSP instructions whose encoding shape is absent from stock code."""

import json
import tomllib
from pathlib import Path

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
    hooks = json.loads(plan.read_text()).get("hooks", [])
    assert not [h for h in hooks if h["cpu"] == "sharc"]


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


def test_gate_exception_deletes_output_and_exits_1(tmp_path, monkeypatch):
    from tools.patchkit import patchkit

    out = tmp_path / "o.syx"
    out.write_bytes(b"x")

    def boom(*a, **k):
        raise KeyError("no such form")

    monkeypatch.setattr(gate, "check_image", boom)
    assert patchkit.encoding_gate(out, tmp_path / "p.json", True) == 1
    assert not out.exists()


class FakeApply:
    """Stands in for the Rust patch step: records its args and writes the output."""

    def __init__(self):
        self.args: list[str] = []

    def __call__(self, args, capture=False):
        import subprocess

        self.args = args
        Path(args[args.index("-o") + 1]).write_bytes(b"img")
        return subprocess.CompletedProcess(args, 0)


def cmd_patch_env(tmp_path, monkeypatch, checker):
    from tools.patchkit import patchkit

    fake = FakeApply()
    calls: list[tuple] = []

    def check(*a, **k):
        calls.append(a)
        return checker()

    monkeypatch.setattr(patchkit, "run", fake)
    monkeypatch.setattr(gate, "check_image", check)
    return patchkit, fake, calls


ARGS = ["--firmware", "fw.syx", "--modules", "m"]


def test_cmd_patch_runs_the_gate_by_default(tmp_path, monkeypatch):
    patchkit, fake, calls = cmd_patch_env(tmp_path, monkeypatch, lambda: [])
    out = tmp_path / "o.syx"
    assert patchkit.cmd_patch([*ARGS, "-o", str(out)]) == 0
    assert len(calls) == 1 and out.exists()


def test_cmd_patch_failing_gate_returns_1_and_removes_output(tmp_path, monkeypatch):
    viol = [gate.Violation(1, "0000", "x:0")]
    patchkit, fake, calls = cmd_patch_env(tmp_path, monkeypatch, lambda: viol)
    out = tmp_path / "o.syx"
    assert patchkit.cmd_patch([*ARGS, "-o", str(out)]) == 1
    assert not out.exists()


def test_cmd_patch_bypass_strips_flag_and_names_output_ungated(
    tmp_path, monkeypatch, capsys
):
    viol = [gate.Violation(1, "0000", "x:0")]
    patchkit, fake, calls = cmd_patch_env(tmp_path, monkeypatch, lambda: viol)
    out = tmp_path / "o.syx"
    assert patchkit.cmd_patch([*ARGS, "--no-encoding-gate", "-o", str(out)]) == 0
    assert "--no-encoding-gate" not in fake.args
    assert not out.exists()
    final = tmp_path / "o.ungated.syx"
    assert final.exists()
    assert str(final) in capsys.readouterr().out


def test_inject_default_ignores_ungated_images(tmp_path, monkeypatch):
    import os

    from tools.control import digi_client
    from tools.patchkit import patchkit

    d = tmp_path / "out" / "patch"
    d.mkdir(parents=True)
    good, bad = d / "a.syx", d / "b.ungated.syx"
    good.write_bytes(b"g")
    bad.write_bytes(b"b")
    os.utime(good, (1, 1))
    os.utime(bad, (2, 2))  # newer
    seen: list[list[str]] = []
    monkeypatch.setattr(patchkit, "ROOT", tmp_path)

    def fake_main(a: list[str]) -> int:
        seen.append(a)
        return 0

    monkeypatch.setattr(digi_client, "main", fake_main)
    assert patchkit.cmd_inject([]) == 0
    assert seen[0][1] == str(good)
