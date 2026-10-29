"""Split-cursor sampler: independent fixed-point oracle and assembled-core checks.

Only assembled checks skip without the assembler. Sparse source maps also guard
large indices without allocating a large buffer.
"""

from dataclasses import replace

import numpy as np
import pytest
from support.paths import REPO_ROOT

from tools.patchkit import dsp_sample_ref as ref
from tools.patchkit import pk_sharc, pk_toolchains

ROOT = REPO_ROOT
F32 = np.float32
BASE = 0xB90000
SRC = 0x10001000
OUT = 0x08020000
STATE = OUT + 0x100
STACK = OUT + 0x300
SAVED = {f"R{n}": 0x7000 + n for n in (3, 5, 6, 7, 9, 10, 11, 13, 14, 15)}
SAVED.update({f"I{n}": 0x08021000 + 16 * n for n in (0, 1, 2, 3, 5)})
SAVED.update({"M1": 11, "M2": 12, "M3": 13, "B3": 0x08021050})
CONSTS = {"M5": 0, "M6": 1, "M7": 0xFFFFFFFF, "M14": 1}
REGS = {**SAVED, **CONSTS, "I6": STACK, "I7": STACK}
# Conservative instruction ceiling: <=40 per output + 100 entry/exit/caller.
# This is an instruction count, not a hardware-cycle or realtime claim.
CEILING = 32 * 40 + 100


def words(state):
    return [v & 0xFFFFFFFF for v in vars(state).values()]


def bits(values):
    return np.asarray(values, dtype=F32).view(np.uint32).tolist()


def oracle(source, state, blocks=2):
    """Combined arbitrary-precision fixed-point position, independent of split update."""
    st = replace(state)
    position = st.index * 65536 + st.fraction
    outputs = []
    for _ in range(blocks):
        out = np.zeros(32, dtype=F32)
        for n in range(32):
            index, fraction = divmod(position, 65536)
            if not st.start <= index < st.end:
                st.done = 1
            if st.done:
                break
            a = F32(source[index])
            b = F32(source[min(index + 1, st.end - 1)])
            # Explicit operation order and one float32 rounding per operation.
            difference = F32(b - a)
            product = F32(F32(fraction / 65536) * difference)
            out[n] = F32(a + product)
            position += st.rate
            if not st.start * 65536 <= position < st.end * 65536:
                st.done = 1
        outputs.append(out)
    st.index, st.fraction = divmod(position, 65536)
    return outputs, st


CASES = [
    ref.Cursor(0, 0, 0, 0, 65536),
    ref.Cursor(3, 3, 2, 0, -65536),
    ref.Cursor(0, 1, 0, 0, 65536),
    ref.Cursor(0, 1, 0, 65535, -1),
    ref.Cursor(0, 2, 0, 32768, 32768),
    ref.Cursor(0, 2, 1, 0, -32768),
    ref.Cursor(3, 103, 3, 65535, 100003),
    ref.Cursor(3, 103, 102, 1, -100003),
    ref.Cursor(0, 32, 0, 0, 65536),  # exact block-end exhaustion
    ref.Cursor(0, 32, 31, 0, -65536),
    ref.Cursor(0, 2, 0, 12345, 0),
    ref.Cursor(0, 2, 0, 0, 0x7FFFFFFF),
    ref.Cursor(0, 2, 1, 65535, -0x80000000),
    ref.Cursor(0, 2, 0, 0, 65536, 1),  # authoritative done
    ref.Cursor(3, 5, 2, 65535, 1),  # already outside
    ref.Cursor(2**24 + 3, 2**24 + 103, 2**24 + 3, 1, 32769),
    ref.Cursor(2**26 - 102, 2**26 - 1, 2**26 - 2, 65535, -65537),
]


def source_for(st):
    return {i: F32(((i % 17) - 8) / 11) for i in range(st.start, st.end)}


@pytest.mark.parametrize(
    "st", CASES + [ref.Cursor(2**30 - 102, 2**30 - 1, 2**30 - 2, 65535, -65537)]
)
def test_reference_blocks_against_combined_position(st):
    source = source_for(st)
    expected, final = oracle(source, st)
    current = st
    for want in expected:
        got, current = ref.linear_block(source, current)
        assert bits(got) == bits(want)
    assert current == final


def test_cursor_split_against_exact_integer_position():
    rng = np.random.default_rng(901)
    for _ in range(2000):
        index = int(rng.integers(-32768, 2**30))
        fraction = int(rng.integers(0, 65536))
        rate = int(rng.integers(-(2**31), 2**31))
        assert ref.cursor_step(index, fraction, rate) == divmod(
            index * 65536 + fraction + rate, 65536
        )


@pytest.fixture(scope="module")
def linked():
    if pk_toolchains.selache_dir() is None:
        pytest.skip("selache not built")
    text = "\n".join(
        [
            '#include "dsp/call.inc"',
            '#include "dsp/sample.inc"',
            ".SECTION/PM seg_pmco;",
            ".GLOBAL _sample_h;",
            "_sample_h:",
            f"R4 = {SRC};",
            f"R8 = {OUT};",
            f"R12 = {STATE};",
            "DK_CALL(_dk_sample_linear, _sample_after0);",
            f"R4 = {SRC};",
            f"R8 = {OUT + 32};",
            f"R12 = {STATE};",
            "DK_CALL(_dk_sample_linear, _sample_after1);",
            "RTS;",
            "",
        ]
    )
    _, result = pk_sharc.build(pk_sharc.expand_includes(text, [ROOT / "sdk"]), BASE)
    return result


def poke(source, st):
    dm = {SRC + i: bits([v])[0] for i, v in source.items()}
    dm.update({STATE + i: w for i, w in enumerate(words(st))})
    dm.update({OUT + i: 0x7FC00001 for i in range(64)})
    return dm


@pytest.mark.parametrize("st", CASES)
def test_assembled_two_blocks(linked, st):
    source = source_for(st)
    expected, final = oracle(source, st)
    r = pk_sharc.run_call(
        linked,
        "_sample_h",
        regs=REGS,
        dm_words=poke(source, st),
        reads={"out": (OUT, 64), "state": (STATE, 6)},
        read_regs=tuple(REGS),
    )
    assert r["halt"]["reason"] == "breakpoint", r["halt"]
    assert r["state"] == words(final)
    assert r["out"] == bits(np.concatenate(expected))
    assert {name: r["regs"][name] for name in REGS} == REGS
    assert r["instructions"] <= 2 * CEILING + 1


@pytest.mark.parametrize("st", [CASES[i] for i in (0, 2, 4, 5, 6, 7, 13, 15, 16)])
def test_assembled_source_read_guards(linked, st):
    source = source_for(st)
    for lo, count, control in (
        (SRC + st.start - 1, 1, False),
        (SRC + st.end, 1, False),
        (SRC + st.start, max(1, st.end - st.start), not st.done and st.start < st.end),
    ):
        sess = pk_sharc.CallSession([linked], 0xB9F000)
        for addr, word in poke(source, st).items():
            sess.poke(addr, word)
        sess.record_reads(lo, count)
        r = sess.call("_sample_h", REGS)
        assert r["halt"]["reason"] == "breakpoint", r["halt"]
        assert bool(sess.stores) == control
