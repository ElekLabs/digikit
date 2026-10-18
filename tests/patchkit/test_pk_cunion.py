"""tools/patchkit/pk_cunion.py: C built by selcc must not name a union member other than the first (selcc lays union
members out like struct members, docs/findings/20-patch-toolchains.md). pk_sharc.compile_c refuses such a unit."""

from __future__ import annotations

from pathlib import Path

import pytest

from tools.patchkit import pk_cunion, pk_sharc, pk_toolchains

ROOT = Path(__file__).resolve().parents[2]

REFUSED = {
    "local": "float f(int i) { union { int i; float f; } u; u.i = i; return u.f; }",
    "pointer": "typedef union { int a[4]; float b; } U;\nfloat g(U *p) { return p->b; }",
    "array": "static union { unsigned int u; float f; } t[2];\nfloat h(int k) { return t[k].f; }",
    "designated": "union V { int a; float b; };\nunion V v = { .b = 1.0f };",
    "anonymous later struct": (
        "typedef union { int w; struct { float x, y; }; } A;\nfloat k(A *a) { return a->y; }"
    ),
    "attribute before the tag": (
        "union __attribute__((aligned(8))) U { int a; float b; };\nfloat n(union U *u) { return u->b; }"
    ),
    "attribute, no tag": (
        "static union __attribute__((packed)) { int a; float b; } w;\nfloat q(void) { return w.b; }"
    ),
    "declspec and alignas": (
        "union __declspec(align(8)) _Alignas(8) U { int a; float b; };\nfloat r(union U *u) { return u->b; }"
    ),
    "nested member": (
        "typedef struct { float p; } S;\ntypedef union { int n; S s; } E;\n"
        "typedef struct { E e; } Voice;\nfloat m(Voice *v) { return v->e.s.p; }"
    ),
}

ACCEPTED = {
    "first member": "float f(int i) { union { int i; float f; } u; u.i = i; return *(float *)(void *)&u; }",
    "pointer cast": (
        "typedef struct { float a; } VA;\ntypedef struct { float b; } WS;\n"
        "typedef union { VA va; WS ws; } E;\n#define PL_WS(e) ((WS *)(void *)(e))\n"
        "float g(E *e) { return PL_WS(e)->b + e->va.a; }"
    ),
    "structs only": "typedef struct { int i; float f; } S;\nfloat h(S *s) { return s->f + 1.5f; }",
    "union type use": "union U;\nfloat k(union U *u) { return 0.0f; }",
    "floats and strings": 'const char *s = "u.f"; float z = 1.0f + .5f;',
    "attribute, first member": (
        "float f(int i) { union __attribute__((aligned(4))) { int i; float f; } u; u.i = i; return u.i; }"
    ),
    "union returned by value": (
        "union U { int a; float b; };\nunion U make(void) { union U u; u.a = 1; return u; }"
    ),
    "qualified use": "union U;\nfloat k(union U const *u) { return 0.0f; }",
}

# Union forms the check cannot parse: refused even without a member access (fail closed).
UNPARSED = {
    "unknown specifier": "union PACKED U { int a; float b; } v;\nint f(void) { return v.a; }",
    "attribute without argument": "union __attribute__ U { int a; float b; } v;",
    "no tag, no body": "union;",
}


@pytest.mark.parametrize("name", sorted(REFUSED))
def test_later_union_member_is_refused(name: str) -> None:
    found = pk_cunion.check(REFUSED[name])
    assert found, name
    assert all(v.member in ("f", "b", "y", "s") for v in found), found


@pytest.mark.parametrize("name", sorted(ACCEPTED))
def test_first_member_and_pointer_casts_are_accepted(name: str) -> None:
    assert pk_cunion.check(ACCEPTED[name]) == [], name


@pytest.mark.parametrize("name", sorted(UNPARSED))
def test_unparsed_union_is_refused(name: str) -> None:
    found = pk_cunion.check(UNPARSED[name])
    assert [v.member for v in found][:1] == [pk_cunion.UNPARSED], (name, found)
    assert "cannot parse" in str(found[0])


def test_violation_reports_the_line() -> None:
    (v,) = pk_cunion.check(REFUSED["pointer"])
    assert v.line == 2 and v.member == "b" and "p->b" in str(v)


def test_the_plaits_sources_pass() -> None:
    """Every C file the PLAITS object builds names only first union members (they use PL_* pointer casts)."""
    dsp = ROOT / "modules" / "machines" / "plaits" / "src" / "dsp"
    for c in sorted(dsp.rglob("*.c")):
        text = c.read_text()
        for h in sorted(dsp.rglob("*.h")):
            text += "\n" + h.read_text()  # headers too (the unit includes them)
        assert pk_cunion.check(text) == [], c.name


@pytest.mark.skipif(pk_toolchains.selache_dir() is None, reason="selache not built")
def test_compile_c_refuses_a_later_union_member() -> None:
    with pytest.raises(
        pk_sharc.PkSharcError, match="union member other than the first"
    ):
        pk_sharc.compile_c(REFUSED["local"])
    obj = pk_sharc.compile_c(ACCEPTED["first member"])
    assert obj.section
