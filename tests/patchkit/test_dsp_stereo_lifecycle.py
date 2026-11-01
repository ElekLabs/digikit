"""The reset mirrors, the stages without a twin and the unsupported-context fallback of core/dsp-voice on DN2 1.12, on the concrete SHARC+ core.

The reset wrappers run the real stock resets of the image. The `_gs_bad_*` wrappers and the fallback of the twins jump to a logging stand-in (Rig(stub=True)):
they are about what the wrapper does around the stock call, not about the stock stage. Original firmware is supplied at runtime."""

import pytest
from support.dn2_stereo_rig import (
    BUFFERS,
    ENGINE,
    HEARTBEAT,
    STOCK,
    STUB_LOG,
    Rig,
    call,
    new_session,
    poke_words,
    words,
)

OTHER = 9
SLOT = 5
LBUF = BUFFERS + 0x80 * SLOT
NAN = 0xFFFFFFFF


@pytest.fixture(scope="module")
def rig():
    return Rig()


@pytest.fixture(scope="module")
def stub_rig():
    return Rig(stub=True)


def stamps(rig, session, slots):
    for v in range(16):
        session.poke(rig.data["_gs_stamp"] + 4 * v, HEARTBEAT if v in slots else 0)


def stub_log(session):
    return [session.peek(STUB_LOG + 4 * i) for i in range(4)]


# ---- reset mirrors ----


@pytest.mark.parametrize(
    "entry,stock,record,stride,rrec,size,cleared",
    [
        (
            "_gs_fo_reset",
            "_gs_fo_reset_stock",
            ENGINE + 0xE088,
            112,
            "_gs_fo_rec",
            28,
            (24, 25, 26, 27),
        ),
        (
            "_gs_uy_reset",
            "_gs_uy_reset_stock",
            ENGINE + 0x12148,
            32,
            "_gs_uy_rec",
            8,
            (6, 7),
        ),
    ],
    ids=["first-order", "uy"],
)
def test_the_reset_clears_the_stage_histories_of_r_too_and_only_of_its_slot(
    rig, entry, stock, record, stride, rrec, size, cleared
):
    mirrored, plain = new_session(rig), new_session(rig)
    for session in (mirrored, plain):
        for v in range(16):
            poke_words(session, record + stride * v, [0x3F000000 + v] * size)
    for v in range(16):
        poke_words(mirrored, rig.data[rrec] + stride * v, [0x3F800000 + v] * size)
    call(mirrored, rig.sym[entry], record + stride * SLOT, 0, 0, ())
    call(plain, STOCK[stock], record + stride * SLOT, 0, 0, ())
    assert [words(mirrored, record + stride * v, size) for v in range(16)] == [
        words(plain, record + stride * v, size) for v in range(16)
    ], "L is the stock reset"
    assert all(words(mirrored, record + stride * SLOT, size)[i] == 0 for i in cleared)
    for v in range(16):
        r = words(mirrored, rig.data[rrec] + stride * v, size)
        expected = [0x3F800000 + v] * size
        if v == SLOT:
            for i in cleared:
                expected[i] = 0
        assert r == expected, f"R record of slot {v}"


@pytest.mark.parametrize(
    "entry,record,stride",
    [("_gs_fo_reset", ENGINE + 0xE088, 112), ("_gs_uy_reset", ENGINE + 0x12148, 32)],
)
@pytest.mark.parametrize("where", ["below", "past the last slot", "unaligned"])
def test_a_reset_of_a_record_that_is_no_slot_only_runs_the_stock_reset(
    stub_rig, entry, record, stride, where
):
    session = new_session(stub_rig)
    for name, size in (("_gs_fo_rec", 1792), ("_gs_uy_rec", 512)):
        poke_words(session, stub_rig.data[name], [0x55555555] * (size // 4))
    address = {
        "below": record - 64,
        "past the last slot": record + 16 * stride,
        "unaligned": record + 3 * stride + 4,
    }[where]
    call(session, stub_rig.sym[entry], address, 0, 0, ())
    assert stub_log(session)[3] == 1 and stub_log(session)[0] == address
    for name, size in (("_gs_fo_rec", 1792), ("_gs_uy_rec", 512)):
        assert words(session, stub_rig.data[name], size // 4) == [0x55555555] * (
            size // 4
        ), name


# ---- stages without an R twin ----

# (wrapper, stock function, counter word in `_gs_cnt`)
BAD = [
    ("_gs_bad_case1", "_gs_case1_stock", 4),
    ("_gs_bad_case2", "_gs_case2_stock", 5),
    ("_gs_bad_case3", "_gs_case3_stock", 6),
    ("_gs_bad_ring4", "_gs_ring_stock", 7),
    ("_gs_bad_ring6", "_gs_ring_stock", 8),
]


def seen(rig, session, slots):
    for v in range(16):
        session.poke(rig.data["_gs_seen"] + 4 * v, HEARTBEAT if v in slots else 0)


def counters(rig, session):
    return words(session, rig.data["_gs_cnt"], 32)


@pytest.mark.parametrize("entry,stock,index", BAD)
def test_a_stage_without_a_twin_clears_the_stamp_of_its_slot_and_calls_the_stock_function(
    stub_rig, entry, stock, index
):
    session = new_session(stub_rig)
    stamps(stub_rig, session, {SLOT, OTHER})
    seen(stub_rig, session, {SLOT, OTHER})
    session.poke(stub_rig.data["_gs_nbad"], 0)
    call(session, stub_rig.sym[entry], 0x1234, LBUF, LBUF, ())
    log = stub_log(session)
    assert log[:3] == [0x1234, LBUF, LBUF] and log[3] == 1, (
        "the stock function got its arguments, once"
    )
    assert session.peek(stub_rig.data["_gs_stamp"] + 4 * SLOT) == 0, (
        "this slot plays R = L from here"
    )
    assert session.peek(stub_rig.data["_gs_stamp"] + 4 * OTHER) == HEARTBEAT, (
        "no other slot is touched"
    )
    assert session.peek(stub_rig.data["_gs_nbad"]) == 1
    assert counters(stub_rig, session) == [int(i == index) for i in range(32)]
    assert session.peek(stub_rig.data["_gs_hacc"]) == 3
    assert [session.peek(stub_rig.data["_gs_seen"] + 4 * v) for v in range(16)] == [
        HEARTBEAT if v in (SLOT, OTHER) else 0 for v in range(16)
    ], "the stereo latch is never cleared by a stage"


@pytest.mark.parametrize("entry,stock,index", BAD)
def test_a_stage_without_a_twin_counts_a_stereo_slot_whose_stamp_an_earlier_stage_cleared(
    stub_rig, entry, stock, index
):
    """Both stages are active on the slot, only the first clears the stamp: the second is visible in its own counter."""
    session = new_session(stub_rig)
    stamps(stub_rig, session, {OTHER})
    seen(stub_rig, session, {SLOT, OTHER})
    session.poke(stub_rig.data["_gs_nbad"], 0)
    call(session, stub_rig.sym[entry], 0x1234, LBUF, LBUF, ())
    assert stub_log(session)[3] == 1
    assert session.peek(stub_rig.data["_gs_nbad"]) == 0, "no stamp was left to clear"
    assert counters(stub_rig, session)[index] == 1
    assert session.peek(stub_rig.data["_gs_hacc"]) == 3


@pytest.mark.parametrize("entry,stock,index", BAD)
def test_a_stage_without_a_twin_leaves_a_mono_slot_alone(
    stub_rig, entry, stock, index
):
    session = new_session(stub_rig)
    stamps(stub_rig, session, {OTHER})
    seen(stub_rig, session, {OTHER})
    session.poke(stub_rig.data["_gs_stamp"] + 4 * SLOT, HEARTBEAT - 1)
    session.poke(stub_rig.data["_gs_seen"] + 4 * SLOT, HEARTBEAT - 1)
    session.poke(stub_rig.data["_gs_nbad"], 0)
    call(session, stub_rig.sym[entry], 0x1234, LBUF, LBUF, ())
    assert stub_log(session)[3] == 1
    assert session.peek(stub_rig.data["_gs_stamp"] + 4 * SLOT) == HEARTBEAT - 1
    assert session.peek(stub_rig.data["_gs_nbad"]) == 0
    assert counters(stub_rig, session) == [0] * 32
    assert session.peek(stub_rig.data["_gs_hacc"]) == 0


def test_every_slot_is_found_by_its_voice_buffer(stub_rig):
    for slot in range(16):
        session = new_session(stub_rig)
        stamps(stub_rig, session, set(range(16)))
        seen(stub_rig, session, set(range(16)))
        call(session, stub_rig.sym["_gs_bad_ring4"], 0, BUFFERS + 0x80 * slot, 0, ())
        assert [
            session.peek(stub_rig.data["_gs_stamp"] + 4 * v) for v in range(16)
        ] == [0 if v == slot else HEARTBEAT for v in range(16)]


# ---- twins in a context the twin does not support ----

# (twin, arguments, counter word in `_gs_cnt`)
TWINS = [
    ("_gs_fo", (32,), 9),
    ("_gs_uy", (32,), 10),
    ("_gs_c5", (32,), 12),
    ("_gs_gain", (0, 0, 32), 11),
    ("_gs_held_early", (32,), 0),
    ("_gs_held_late", (32,), 1),
    ("_gs_shaper_early", (32,), 2),
    ("_gs_shaper_late", (32,), 3),
]


@pytest.mark.parametrize("entry,args,index", TWINS)
@pytest.mark.parametrize(
    "context",
    ["circular-l4", "circular-l3", "circular-l5", "block-16"],
)
def test_an_unsupported_context_clears_the_stamp_and_runs_the_stock_stage_once(
    stub_rig, entry, args, index, context
):
    session = new_session(stub_rig)
    stamps(stub_rig, session, {SLOT})
    regs = {
        "circular-l4": {"L4": 64},
        "circular-l3": {"L3": 64},
        "circular-l5": {"L5": 8},
        "block-16": {},
    }[context]
    call_args = tuple(
        16 if (context == "block-16" and i == len(args) - 1) else a
        for i, a in enumerate(args)
    )
    call(session, stub_rig.sym[entry], 0x1234, LBUF, LBUF, call_args, **regs)
    assert stub_log(session)[3] == 1 and stub_log(session)[:3] == [0x1234, LBUF, LBUF]
    assert session.peek(stub_rig.data["_gs_stamp"] + 4 * SLOT) == 0
    assert counters(stub_rig, session) == [int(i == index) for i in range(32)]
    assert session.peek(stub_rig.data["_gs_nbad"]) == 1
    assert session.peek(stub_rig.data["_gs_hacc"]) == 3
