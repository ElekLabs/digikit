"""SHARC+ patchkit toolchain: Selache assembles, tools/patchkit/pk_sharc.py links, our decoder re-decodes, the core runs.

Skips when Selache is not built (uv run python -m tools.patchkit.pk_toolchains selache)."""

import struct

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.patchkit import (  # noqa: E402
    pk_sharc,  # noqa: E402
    pk_toolchains,  # noqa: E402
)

pytestmark = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None,
    reason="selache not built (tools/patchkit/pk_toolchains.py selache)",
)

SRC = ROOT / "tests" / "patchkit" / "pk_src"
BASE = 0xB90000  # short-word (PC) address in L2 code space
BUF = 0x08020000  # normal-word DM address in L2
HOOK = 0xB9F000
RET = 0xB9E000
FRAME = 0x08030100  # a C frame: [FRAME] = caller frame pointer, [FRAME-1] = return address - 1


def bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def val(x: int) -> float:
    return struct.unpack("<f", struct.pack("<I", x))[0]


def test_scale_buffer_runs_and_every_word_redecodes():
    obj, linked = pk_sharc.build(
        (SRC / "scale.s").read_text(), BASE, {"_fw_hook": HOOK}
    )
    assert linked.checked == len(obj.rows) == 12
    vals = [1.0, 2.0, -3.0, 0.5]
    out = pk_sharc.run_call(
        linked,
        "_scale_buf",
        regs={"R4": BUF, "R8": 4},
        dm_words={BUF + i: bits(v) for i, v in enumerate(vals)},
        reads={"out": (BUF, 5)},
        return_sw=HOOK,
    )
    assert out["halt"]["reason"] == "breakpoint" and out["halt"]["pc_sw"] == HOOK
    assert [val(w) for w in out["out"][:4]] == [v * 2.5 for v in vals]
    assert out["out"][4] is None  # one past the buffer untouched
    assert out["regs"]["R0"] == 7  # the local call `_bias` returned


def test_relocations_are_absolute_parcel_addresses():
    obj = pk_sharc.assemble((SRC / "scale.s").read_text())
    assert sorted((r.rtype, r.symbol) for r in obj.relocs) == [
        (11, "_bias"),
        (11, "_fw_hook"),
    ]
    linked = pk_sharc.link(obj, BASE, {"_fw_hook": HOOK})
    off = linked.symbols["_bias"] - BASE
    assert off == obj.symbols["_bias"][0]
    # CALL _bias: low 24 bits of the 48-bit word hold the absolute short-word address
    call = next(
        p
        for p, _ in obj.rows
        if any(r.parcel == p and r.symbol == "_bias" for r in obj.relocs)
    )
    assert (
        int.from_bytes(linked.code_selache[call * 2 + 3 : call * 2 + 6], "big")
        == linked.symbols["_bias"]
    )


def test_a_wrong_word_fails_the_decoder_check():
    obj = pk_sharc.assemble((SRC / "scale.s").read_text())
    linked = pk_sharc.link(obj, BASE, {"_fw_hook": HOOK})
    broken = bytearray(linked.code_selache)
    broken[0:2] = b"\xff\xff"  # not an instruction Selache selected
    bad = pk_sharc.Linked(
        BASE, bytes(broken), pk_sharc.sel.swap_parcels(bytes(broken)), linked.symbols
    )
    with pytest.raises(pk_sharc.PkSharcError, match="disagrees with our decoder"):
        pk_sharc.verify(obj, bad, {"_fw_hook": HOOK})


def test_unresolved_extern_is_an_error():
    obj = pk_sharc.assemble((SRC / "scale.s").read_text())
    with pytest.raises(pk_sharc.PkSharcError, match="unresolved symbol _fw_hook"):
        pk_sharc.link(obj, BASE, {})


# --- selcc evaluation: its output decodes and runs correctly on the core (byte-address pointers) ---


@pytest.fixture(scope="module")
def selcc_module():
    return pk_sharc.link(pk_sharc.compile_c((SRC / "scale_c.c").read_text()), BASE)


def c_call(linked, entry, regs, dm=None, reads=None):
    mem = {FRAME: 0, FRAME - 1: RET - 1, **(dm or {})}
    regs = {**regs, "I6": FRAME, "I7": FRAME - 2}
    return pk_sharc.run_call(
        linked,
        entry,
        regs=regs,
        dm_words=mem,
        reads=reads,
        return_sw=RET,
        max_steps=20000,
    )


def s32(x):
    return x - (1 << 32) if x & 0x80000000 else x


def test_selcc_integer_functions(selcc_module):
    assert (
        s32(c_call(selcc_module, "add3.", {"R4": 10, "R8": 3, "R12": 7})["regs"]["R0"])
        == 31
    )
    byte_buf = 0x20080000
    r = c_call(
        selcc_module,
        "sum.",
        {"R4": byte_buf, "R8": 5},
        {byte_buf + 4 * i: v & 0xFFFFFFFF for i, v in enumerate([1, 2, 3, 40, -5])},
    )
    assert s32(r["regs"]["R0"]) == 41
    for x, want in ((0, 11), (1, 22), (2, 33), (5, 44), (9, -1)):
        assert s32(c_call(selcc_module, "sw.", {"R4": x})["regs"]["R0"]) == want
    for x in (0, 0xF0F0, 0xFFFFFFFF):
        assert c_call(selcc_module, "popc.", {"R4": x})["regs"]["R0"] == bin(x).count(
            "1"
        )


def test_selcc_float_loop(selcc_module):
    byte_buf = 0x20080000
    vals = [1.0, 2.0, -3.0, 0.5]
    r = c_call(
        selcc_module,
        "scale.",
        {"R4": byte_buf, "R8": 4, "R12": bits(1.5)},
        {byte_buf + 4 * i: bits(v) for i, v in enumerate(vals)},
        {f"o{i}": (byte_buf + 4 * i, 1) for i in range(4)},
    )
    assert [val(r[f"o{i}"][0]) for i in range(4)] == [v * 1.5 for v in vals]


def test_negative_shift_hazard_follows_the_build():
    src = ".SECTION/PM seg_pmco;\nR0 = LSHIFT R1 BY -3;\n"
    if pk_sharc._negative_shift_patched(str(pk_sharc._tool("selas"))):
        assert [raw for _, raw in pk_sharc.assemble(src).rows] == ["023e7800fd01"]
    else:
        with pytest.raises(pk_sharc.PkSharcError, match="known Selache encode bug"):
            pk_sharc.assemble(src)
