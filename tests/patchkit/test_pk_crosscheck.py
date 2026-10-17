"""Operand cross-check: Selache's listing and our decoder must name the same operands for every instruction.

The Selache-dependent tests skip when Selache is not built (uv run python -m tools.patchkit.pk_toolchains selache)."""

from __future__ import annotations

import json
import tomllib
from pathlib import Path

import pytest

from tools.patchkit import pk_build, pk_crosscheck, pk_sharc, pk_toolchains

ROOT = pk_toolchains.ROOT
needs_selache = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)


def test_selache_line_operands():
    ops = pk_crosscheck.selache_operands("if lt jump (pc,-0x3) (db)")
    assert (
        ops.cond == 0x01 and ops.flags == {"db"} and ops.imms == (-3,) and not ops.regs
    )
    ops = pk_crosscheck.selache_operands("jump (m13,i12)")
    assert ops.regs == {pk_crosscheck.UREG_CODE["m13"], pk_crosscheck.UREG_CODE["i12"]}
    assert ops.cond == pk_crosscheck.ALWAYS and not ops.flags
    ops = pk_crosscheck.selache_operands("r2=r2-r1")
    assert ops.imms == () and ops.regs == {1, 2}
    assert pk_crosscheck.selache_operands("r3=r3-1").imms == (1,)
    assert pk_crosscheck.selache_operands("dm (-0x5,i6)=r0").imms == (-5,)


def test_unreadable_selache_line_is_an_error():
    with pytest.raises(pk_crosscheck.CrossCheckError):
        pk_crosscheck.selache_operands("??? 0x311c 0x013e")
    with pytest.raises(pk_crosscheck.CrossCheckError):
        pk_crosscheck.selache_operands("r0=frobnicate r1")


def test_native_9a_reads_selache_nocompress_jump_as_i8_delayed():
    from tools.patchkit import pk_encoding_gate as gate

    ins = gate.decode_listing("083e2c000000")
    n = pk_crosscheck.native_operands(ins.type_name or "", ins.fields)
    s = pk_crosscheck.selache_operands("jump (m13,i12)")
    assert n.flags == {"db"} and pk_crosscheck.UREG_CODE["i8"] in n.regs
    assert pk_crosscheck.compare(s, n) is not None


def test_immediates_compare_modulo_the_field_width():
    s = pk_crosscheck.selache_operands("r1=-0x4")
    n = pk_crosscheck.NativeOperands(
        frozenset({1}), ((0xFFFC, 16),), pk_crosscheck.ALWAYS, frozenset()
    )
    assert pk_crosscheck.compare(s, n) is None
    n2 = pk_crosscheck.NativeOperands(
        frozenset({1}), ((0x7FFC, 16),), pk_crosscheck.ALWAYS, frozenset()
    )
    assert pk_crosscheck.compare(s, n2) is not None


def test_statements_keep_source_lines_and_drop_labels_and_directives():
    pre = '#line 1 "m.s"\n.SECTION/PM seg_pmco;\n\nx:\n    R0 = 1;\n.NOCOMPRESS;\ny: JUMP x;\n    R1 =\n      R2;\n'
    assert pk_crosscheck.statements(pre) == [
        (4, "R0 = 1"),
        (6, "JUMP x"),
        (7, "R1 = R2"),
    ]


@needs_selache
def test_nocompress_indirect_jump_is_refused_with_its_source_line():
    src = ".SECTION/PM seg_pmco;\n_f:\n    .NOCOMPRESS;\n    JUMP (M13, I12);\n"
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_sharc.assemble(src)
    msg = str(e.value)
    assert "line 4: `JUMP (M13, I12)`" in msg
    assert "083e2c000000" in msg and "db" in msg


@needs_selache
def test_compressed_indirect_jump_assembles_as_stock_9b():
    obj = pk_sharc.assemble(".SECTION/PM seg_pmco;\n_f:\n    JUMP (M13, I12);\n")
    assert [raw for _, raw in obj.rows] == ["083f283f"]


@needs_selache
def test_pk_build_names_the_source_file_and_line(tmp_path):
    (tmp_path / "dsp").mkdir()
    a = tmp_path / "dsp" / "a.s"
    b = tmp_path / "dsp" / "b.s"
    a.write_text(".SECTION/PM seg_pmco;\n_f:\n    NOP;\n")
    b.write_text("// b\n    .NOCOMPRESS;\n    CALL (M13, I12);\n")
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_build.sharc_object([a, b])
    assert f"{b}:3: `CALL (M13, I12)`" in str(e.value)


def _dsp_objects() -> list[tuple[Path, dict]]:
    out = []
    for manifest in sorted((ROOT / "modules").rglob("module.toml")):
        for o in tomllib.loads(manifest.read_text()).get("object", []):
            srcs = o.get("sources") or []
            if (
                srcs
                and all(s.endswith((".s", ".c")) for s in srcs)
                and any("dsp" in Path(s).parts for s in srcs)
            ):
                out.append((manifest.parent, o))
    return out


@needs_selache
@pytest.mark.parametrize(
    "moddir,obj",
    _dsp_objects(),
    ids=lambda x: x["path"] if isinstance(x, dict) else x.name,
)
def test_committed_module_objects_pass_the_cross_check_unchanged(
    moddir: Path, obj: dict
):
    built = pk_build.sharc_object([moddir / s for s in obj["sources"]])
    committed = json.loads((moddir / obj["path"]).read_text())
    for doc in (built, committed):
        doc.pop("toolchain")  # Selache's binary hashes, not the object
    assert built == committed


@needs_selache
def test_pm_transfer_that_selache_encodes_as_dm_is_refused():
    # Selache writes `PM(I12, M14) = R2` as DM(I4, M6) = R2 (59be81000000); its listing agrees with our decoder, the
    # source does not.
    src = ".SECTION/PM seg_pmco;\n_f:\n    PM(I12, M14) = R2;\n"
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_sharc.assemble(src)
    assert "line 3: `PM(I12, M14) = R2`" in str(e.value)
    assert "I12" in str(e.value) and "I4" in str(e.value)


@needs_selache
def test_low_modify_registers_pass_despite_the_listing():
    # seldump lists Type 3a's M0-M3 as M4-M7 and a 16-bit Type 3c with M0-M3 as a 32-bit word; the source and our
    # decoder agree, so these assemble. (A 3c with M0-M3 followed by another instruction is still refused by
    # pk_sharc's extent check, where seldump's linear listing merges the two.)
    src = ".SECTION/PM seg_pmco;\n_f:\n    F0 = F1 * F2, R3 = DM(I0, M1);\n    DM(I1, M2) = R2;\n"
    obj = pk_sharc.assemble(src)
    assert [raw for _, raw in obj.rows] == ["507e01930012", "92b2"]


# ---- Type 16a/16b `DM|PM(Ia, Mb) = data`: compared role by role (Selache's C stores the return address of every call
# with `DM(I7, M7) = label - 1`, the stock call sequence's 16a word)


def _strict(raw: str, text: str) -> str | None:
    from tools.patchkit import pk_encoding_gate as gate

    ins = gate.decode_listing(raw)
    assert ins.type_name in ("16a", "16b")
    return pk_crosscheck.strict_check(ins.type_name, dict(ins.fields), text)


def test_16a_stock_call_return_store_agrees():
    # stock 1.17 SW 0x1c4b30: the second delay slot of `CALL 0x1c0d68 (DB)`
    assert _strict("9fc0001c4b32", "dm (i7,m7)=0x1c4b32") is None
    assert _strict("9fc0ffffffff", "dm (i7,m7)=0xffffffff") is None


def test_16b_stock_store_agrees():
    # stock 1.17 SW 0xb82223
    assert _strict("9fc40020", "dm (i7,m7)=0x20") is None


def test_16a_operands_are_compared_by_role():
    assert "index" in (_strict("9fc0001c4b32", "dm (i6,m7)=0x1c4b32") or "")
    assert "modify" in (_strict("9fc0001c4b32", "dm (i7,m6)=0x1c4b32") or "")
    assert "immediate" in (_strict("9fc0001c4b32", "dm (i7,m7)=0x1c4b33") or "")
    assert "memory" in (_strict("9fc0001c4b32", "pm (i15,m15)=0x1c4b32") or "")
    # I and M swapped (a pre-modify spelling) is not the same instruction
    assert _strict("9fc0001c4b32", "dm (m7,i7)=0x1c4b32") is not None


def test_16b_where_seldump_reads_another_modify_register_is_refused():
    # stock word 99840000: our decoder (PRM layout) DM(I4, M6) = 0; seldump lists M4
    assert "modify" in (_strict("99840000", "dm (i4,m4)=0x0") or "")


def test_16a_width_bits_fail_closed():
    # sl/by set (a short-word or byte store): not cross-checked
    raw = f"{int('9fc0001c4b32', 16) | (1 << 33):012x}"
    assert "not cross-checked" in (_strict(raw, "dm (i7,m7)=0x1c4b32") or "")


# ---- fail-closed paths: unhandled forms, statement counts, relocation addends


def test_unhandled_form_fails_closed():
    with pytest.raises(pk_crosscheck.CrossCheckError, match="not cross-checked yet"):
        pk_crosscheck.native_operands("26a", {})


@needs_selache
def test_unhandled_form_in_a_source_is_refused_with_its_line():
    src = ".SECTION/PM seg_pmco;\n_f:\n    R2 <-> S3;\n"
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_sharc.assemble(src)
    assert "line 3: `R2 <-> S3`" in str(e.value)
    assert "5a_swap is not cross-checked yet" in str(e.value)


@needs_selache
def test_statement_count_mismatch_is_an_error(monkeypatch):
    # The source check (and the listing quirks it vouches for) needs one statement per instruction; a source that
    # does not map is refused, not silently checked against the listing only.
    real = pk_crosscheck.statements
    monkeypatch.setattr(pk_crosscheck, "statements", lambda pre: real(pre)[:-1])
    src = ".SECTION/PM seg_pmco;\n_f:\n    R0 = 1;\n    R1 = 2;\n"
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_sharc.assemble(src)
    assert "1 source statements but 2 instructions" in str(e.value)


def test_statement_count_mismatch_raises_in_check():
    from tools.sharc_dev import sharc_selache as sel

    cmp = sel.compare_external_bytes(bytes.fromhex("00010001"))  # two NOPs
    with pytest.raises(pk_crosscheck.CrossCheckError, match="1 source statements"):
        pk_crosscheck.check(b"", "seg_pmco", bytes.fromhex("00010001"), [(3, "NOP")])
    assert len(cmp.instructions) == 2


def test_symbol_refs_read_bare_names_constants_and_refuse_other_expressions():
    names = {"_x", ".L_ret_1", "plaits_track."}
    refs = pk_crosscheck.symbol_refs
    assert refs("R0 = _x", names) == [("_x", 0)]
    assert refs("R0 = _x + 8", names) == [("_x", 8)]
    assert refs("DM(I7, M7) = _x - 1", names) == [("_x", -1)]
    assert refs("DM (I7,M7)=.L_ret_1-1", names) == [(".L_ret_1", -1)]
    assert refs("CJUMP plaits_track. (DB)", names) == [("plaits_track.", 0)]
    assert refs("R0 = _x + 4 * 2", names) == [("_x", None)]
    assert refs("R0 = 4 + _x", names) == [("_x", None)]
    assert refs("R0 = _xy + 4", names) == []


def test_check_addends_maps_statement_to_relocation():
    names = {"_x"}
    ok = pk_crosscheck.check_addends([(3, "R0 = _x + 8")], [0], [(0, "_x", 8)], names)
    assert ok == []
    dropped = pk_crosscheck.check_addends(
        [(3, "R0 = _x + 8")], [0], [(0, "_x", 0)], names
    )
    assert len(dropped) == 1 and "_x+0" in dropped[0] and "_x+8" in dropped[0]
    norel = pk_crosscheck.check_addends([(3, "JUMP _x + 2")], [0], [], names)
    assert len(norel) == 1 and "without a relocation" in norel[0]


def test_check_addends_unmapped_counts_relocations_per_addend():
    names = {".L_r", "t."}
    stmts = [(5, "DM (I7,M7)=.L_r-1"), (9, "R1 = t.")]
    assert pk_crosscheck.check_addends_unmapped(stmts, [(0, ".L_r", -1)], names) == []
    bad = pk_crosscheck.check_addends_unmapped(stmts, [(0, ".L_r", 0)], names)
    assert len(bad) == 1 and ".L_r-1" in bad[0]


@needs_selache
def test_dropped_addend_is_refused_at_build_time():
    # Selache relocates `R0 = _x + 8` as `_x` (field 0): the image would load _x, not _x + 8.
    src = ".SECTION/PM seg_pmco;\n.EXTERN _x;\n_f:\n    R0 = _x + 8;\n"
    with pytest.raises(pk_sharc.PkSharcError) as e:
        pk_sharc.assemble(src)
    assert "line 4: `R0 = _x + 8`" in str(e.value) and "_x+8" in str(e.value)


@needs_selache
def test_kept_addends_assemble():
    # the forms where Selache keeps the constant: a jump target and a 16a store of a return address
    src = (
        ".SECTION/PM seg_pmco;\n.EXTERN _f;\n_a:\n    JUMP _f + 2;\n"
        "    DM(I7, M7) = _b - 1;\n_b: NOP;\n"
    )
    obj = pk_sharc.assemble(src)
    assert sorted((r.symbol, r.addend) for r in obj.relocs) == [("_b", -1), ("_f", 2)]


@needs_selache
def test_compiled_c_addends_are_checked_against_its_assembly():
    # selcc computes array offsets in registers and stores return addresses as `label - 1` (kept): it builds.
    obj = pk_sharc.compile_c(
        "int t[8];\nint g(void);\nint *f(void){return &t[3];}\nint h(void){return g() + 1;}\n"
    )
    assert any(r.addend == -1 for r in obj.relocs)
