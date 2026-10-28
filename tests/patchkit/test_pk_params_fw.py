"""core/params as a module: the hook routine on the native core, and the table-row expressions of module.toml against
the Rust generator's row builder (a1cf1df^:native/patchkit/src/params.rs, removed in a1cf1df).

The expressions are evaluated by a small stand-in for patchkit's expression language (integers, symbols, `$field`,
`image.u8|u16|u32(...)`, + - * / & | << >> and parentheses): lane a's evaluator is the real one, this keeps the
manifest honest until it lands."""

import tomllib

import pytest
from support.paths import REPO_ROOT

ROOT = REPO_ROOT

from tools.firmware import config  # noqa: E402
from tools.patchkit import (  # noqa: E402
    pk_cfunit,  # noqa: E402
    pk_toolchains,  # noqa: E402
)
from tools.patchkit.pk_expr import evaluate as eval_expr  # noqa: E402

SYX = config.stock("Digitakt_II_OS1.17.syx")
SRC = ROOT / "modules" / "core" / "params" / "src" / "cf" / "dt2" / "params.s"
MANIFEST = ROOT / "modules" / "core" / "params" / "module.toml"
FIRST = 0x113
ROWS = 0x47EC0000
ROWSIZE = 32  # name, short, template, slot, min, max, default, formatter

needs = [
    pytest.mark.skipif(
        pk_toolchains.m68k_bin("gcc") is None, reason="m68k toolchain not installed"
    ),
]


def stock_table(count):
    """A stock parameter table of `count` 60-byte rows with distinctive words."""
    return b"".join(
        ((r * 0x01010101 + w * 0x00070001) & 0xFFFFFFFF).to_bytes(4, "big")
        for r in range(count)
        for w in range(15)
    )


def rust_row(table, d, slot_mirror=(25, 26, 27, 28, 31, 32, 33, 34)):
    """params.rs `generate`: the appended row, word by word."""

    def word(row, off):
        at = row * 60 + off
        return int.from_bytes(table[at : at + 4], "big")

    t = d["template"]
    cc = 0xFFFF if d["slot"] == 3 else 0x10 + d["slot"]
    return [
        word(t, 0),
        slot_mirror[d["slot"]],
        d["min"],
        d["max"],
        d["default"],
        word(t, 0x14),
        cc << 16 | (word(t, 0x18) & 0xFFFF),
        0x80 + d["slot"],
        word(t, 0x20),
        word(t, 0x24),
        d["name"],
        word(t, 0x2C),
        d["short"],
        d["formatter"] or word(t, 0x34),
        word(t, 0x38),
    ]


def test_table_row_expressions_match_the_rust_row_builder():
    m = tomllib.loads(MANIFEST.read_text())
    sec = next(s for s in m["section"] if s["name"] == "pf_table")
    table = stock_table(FIRST)
    T1 = 0x40200000

    def image(addr, n):
        assert addr >= T1 and addr + n <= T1 + len(table)
        return table[addr - T1 : addr - T1 + n]

    for template, slot, formatter in [
        (t, s, f)
        for t in (0x00, 0xD1, 0x112)
        for s in range(8)
        for f in (0, 0x400F1234)
    ]:
        d = dict(
            template=template,
            slot=slot,
            min=0,
            max=0x7F00,
            default=0x4000,
            name=0x47F00010,
            short=0x47F00020,
            formatter=formatter,
        )
        got = [
            eval_expr(e, {"cf.params.t1": T1}, d, image) & 0xFFFFFFFF
            for e in sec["append"]["row"]
        ]
        assert got == rust_row(table, d), (template, slot, formatter)
    assert (
        eval_expr(
            sec["from"]["len"],
            {"cf.params.t2": 0x2000},
            image=lambda a, n: FIRST.to_bytes(4, "big")[:n] if a == 0x2006 else b"",
        )
        == FIRST * 60
    )


def test_bound_and_reference_writes():
    m = tomllib.loads(MANIFEST.read_text())
    refs = next(o for o in m["op"] if o.get("anchors") == "cf.params.ref.*")
    bounds = next(o for o in m["op"] if o.get("anchors") == "cf.params.bound.*")
    # `this` stands for the matched anchor: the stand-in binds it to the anchor's address
    img = {
        0x1000: (0x4020F59C + 120).to_bytes(4, "big"),
        0x2002: FIRST.to_bytes(4, "big"),
    }
    old = int.from_bytes(img[0x1000], "big")
    v = eval_expr(
        refs["value"].replace("this", "0x1000"),
        {"pf_table": 0x47F00000, "cf.params.t1": 0x4020F59C},
        image=lambda a, n: img[a][:n],
    )
    assert v == 0x47F00000 + (old - 0x4020F59C)
    v = eval_expr(
        bounds["value"].replace("this", "0x2000"),
        {"params.count": 3},
        image=lambda a, n: img[a][:n],
    )
    assert v == FIRST + 3


@pytest.fixture(scope="module")
def unit():
    return pk_cfunit.CfUnit(
        SYX,
        [SRC],
        ["_pf_t_t2"],
        {
            "_pf_first": FIRST,
            "_pf_count": 3,
            "_pf_rows": ROWS,
            "_pf_rowsize": ROWSIZE,
            "_pe_count": 0,
            "_pe_rows": ROWS + 0x1000,
            "_pe_rowsize": 24,
        },
        ["pf_t2"],
    )


@pytest.mark.skipif(
    not SYX.exists(), reason="firmware/Digitakt_II_OS1.17.syx not present"
)
def test_added_codes_become_their_template(unit):
    templates = [0xD1, 0x20, 0x112]
    rows = b"".join(
        (0).to_bytes(4, "big") * 2 + t.to_bytes(4, "big") + (0).to_bytes(4, "big") * 5
        for t in templates
    )
    codes = [0, 0x112, FIRST, FIRST + 1, FIRST + 2, FIRST + 3, 0x1000]
    res = unit.run_cases(
        {ROWS: rows},
        [{"routine": "pf_t2", "d": {0: c, 1: 5}, "a": {0: 9}} for c in codes],
    )
    for c, r in zip(codes, res, strict=True):
        want = templates[c - FIRST] if FIRST <= c < FIRST + 3 else c
        assert r.stub == "_pf_t_t2" and r.d[0] == want
        assert r.d[1] == 5 and r.a[0] == 9


def test_row_size_of_the_manifest_matches_the_assembly():
    m = tomllib.loads(MANIFEST.read_text())
    fields = m["registry"][0]["field"]
    assert all(f["kind"] in ("u32", "cstr") for f in fields)
    assert len(fields) * 4 == ROWSIZE
    assert [f["name"] for f in fields].index("template") * 4 == 8
