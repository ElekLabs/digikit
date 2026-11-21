"""`cf_call` finds a function through its caller's `jsr`; `offset` moves the anchor into that function (synthetic image, no database)."""

import hashlib

import pytest

from tools.patchkit import port_resolve as pr

BASE = 0x4000_0000


class FakeCf(pr.CfFacts):
    def __init__(self, main: bytes, callers: list[int]):
        self.name = "fake"
        self.main = main
        self.base = BASE
        self.callers = callers

    def funcs_by_hash(self, h):
        return self.callers


def image() -> bytes:
    main = bytearray(0x100)
    main[0x10:0x16] = b"\x4e\xb9" + (BASE + 0x80).to_bytes(
        4, "big"
    )  # jsr 0x40000080 at caller + 0
    main[0x80:0x90] = bytes(range(0x80, 0x90))  # the callee
    return bytes(main)


RULE = {"name": "t", "kind": "cf_call", "hash": "h" * 32, "call": 0, "len": 4}


def test_offset_is_added_to_the_callee():
    cf = FakeCf(image(), [BASE + 0x10])
    a = pr.resolve_cf_anchor(cf, {**RULE, "offset": 6})
    assert (a["addr"], a["src"], a["len"]) == (BASE + 0x86, BASE + 0x10, 4)
    assert a["sha256"] == hashlib.sha256(bytes([0x86, 0x87, 0x88, 0x89])).hexdigest()
    assert pr.resolve_cf_anchor(cf, RULE)["addr"] == BASE + 0x80


def test_no_jsr_at_the_call_offset_is_an_error():
    cf = FakeCf(image(), [BASE + 0x10])
    with pytest.raises(pr.ResolveError, match="no jsr"):
        pr.resolve_cf_anchor(cf, {**RULE, "call": 2})
