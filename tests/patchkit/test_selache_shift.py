"""Selache must encode negative shift immediates the way the stock toolchain does (data_hi bits 27-31 = 0xF)."""

import json
import sqlite3

import pytest

from tools.patchkit import pk_sharc, pk_toolchains

STOCK_DB = pk_toolchains.ROOT / "out" / "sharcdb" / "dt2-1.17.sqlite"

CASES = {
    "R1 = LSHIFT R6 BY -24;": "023e7800e816",
    "R2 = LSHIFT R15 BY -1;": "023e7800ff2f",  # stock 0xb8088e
    "R7 = ASHIFT R6 BY -4;": "023e7801fc76",  # stock 0x1c0e57
    "R1 = R1 OR LSHIFT R0 BY -8;": "023e7808f810",  # stock 0xb883b3
    "R1 = LSHIFT R0 BY 4;": "023e00000410",  # positive: unchanged
}

pytestmark_needs_selache = pytest.mark.skipif(
    pk_toolchains.selache_dir() is None, reason="needs the Selache build"
)


@pytestmark_needs_selache
@pytest.mark.parametrize("src,word", list(CASES.items()))
def test_shift_immediates_match_stock_encoding(src, word):
    obj = pk_sharc.assemble(f".SECTION/PM seg_pmco;\n{src}\n")
    assert [raw for _, raw in obj.rows] == [word]


@pytest.mark.skipif(not STOCK_DB.exists(), reason="needs out/sharcdb/dt2-1.17.sqlite")
def test_stock_rule_negative_shift_sets_data_hi():
    con = sqlite3.connect(STOCK_DB)
    rows = con.execute(
        "SELECT raw, fields FROM insn WHERE form = '6b_shiftimm' AND mnemonic LIKE '%shift(%'"
    ).fetchall()
    assert len(rows) > 100
    for raw, fields in rows:
        f = json.loads(fields)
        word = int(raw, 16)
        amount = (f["shiftimm[15:0]"] >> 8) & 0xFF
        assert (word >> 27) & 0x1F == (0xF if amount & 0x80 else 0), raw
