"""The R twins of core/dsp-voice on DN2 1.12 (docs/design/dn2-stereo.md, section 2), in the style of test_dsp_stereo_shadow_replay.py.

A twin wrapper (`_gs_fo`, `_gs_uy`, `_gs_gain`, `_gs_c5`) is called the way the engine calls the stock stage: a linked call, R4 = the L record, R8 = R12 = the voice
buffer, the arguments on the stack. For a stamped slot it runs the stock function on L, then on a private R record and the R buffer. Reference channels run the
stock function on independent concrete cores with private state; the control words of each block are the same for all three. Negative controls: a shared
history, and the stage's control words copied after the L call, give a different R. Original firmware is supplied at runtime, never copied into a fixture."""

import random
from dataclasses import dataclass, field

import pytest
from support.dn2_stereo_rig import (
    BACK,
    BUFFERS,
    ENGINE,
    FRAME,
    HEARTBEAT,
    REGS,
    STOCK,
    Rig,
    bits,
    call,
    new_session,
    poke_words,
    words,
)

SLOT = 5
LBUF = BUFFERS + 0x80 * SLOT
SCRATCH = BUFFERS + 0x1000  # the L scratch block of case 5


@pytest.fixture(scope="module")
def rig():
    return Rig()


@dataclass
class Stage:
    name: str
    entry: str
    stock: str
    record: int  # L record of slot v: record + stride * v (byte address)
    stride: int
    size: int  # words
    controls: tuple  # shared control words, rewritten before every block
    histories: tuple  # words the stage keeps per channel
    initial: dict  # word -> float, the L record at the start (and the R record's histories are zero)
    args: tuple = (32,)
    scratch: bool = False  # R12 is a scratch output block and a blend follows (case 5)
    blocks: int = 4
    controls_per_block: tuple = ()  # gain: the first stack argument of each block (1 restarts the attack)
    extra: dict = field(default_factory=dict)

    def args_of(self, block):
        if not self.controls_per_block:
            return self.args
        return (
            self.controls_per_block[block % len(self.controls_per_block)],
            *self.args[1:],
        )

    def l_record(self, v=SLOT):
        return self.record + self.stride * v

    def r_record(self, rig, v=SLOT):
        return rig.data[self.extra["rrec"]] + self.stride * v


STAGES = {
    "first-order": Stage(
        "first-order",
        "_gs_fo",
        "_gs_fo_stock",
        ENGINE + 0xE088,
        112,
        28,
        (21, 22, 23),
        (24, 25),
        {21: 0.7, 22: 0.2, 23: 0.1, 24: 0.1, 25: -0.05, 26: 0.3, 27: -0.2},
        extra={
            "rrec": "_gs_fo_rec",
            "values": {21: (0.7, -0.05), 22: (0.2, 0.01), 23: (0.1, 0.02)},
        },
    ),
    "uy": Stage(
        "uy",
        "_gs_uy",
        "_gs_uy_stock",
        ENGINE + 0x12148,
        32,
        8,
        (2, 3, 4, 5),
        (6, 7),
        {2: 0.6, 3: 0.3, 4: 0.2, 5: 0.4, 6: 0.02, 7: -0.03},
        extra={
            "rrec": "_gs_uy_rec",
            "values": {2: (0.6, 0.02), 3: (0.3, -0.01), 4: (0.2, 0.0), 5: (0.4, 0.03)},
        },
    ),
}


BLEND = (
    ENGINE + 0x13E9C + 0x38 * SLOT
)  # the blend's 14-word ramp record (I0 of the engine at the case 5 call)
BLEND_ENTRY, BLEND_STOP = 0x1C9C12, 0x1C98B9
CASE5 = {9: 0.02, 10: -0.03} | {
    11 + 7 * k + d: v
    for k in range(6)
    for d, v in ((0, (0.6, 0.1, 0.15, 0.2, 0.1, 0.08)[k]), (2, 0.0001))
}
CASE5_VALUES = {
    11 + 7 * k + d: (v * (1 + 0.1 * k), v * 0.05)
    for k in range(6)
    for d, v in ((0, (0.6, 0.1, 0.15, 0.2, 0.1, 0.08)[k]), (2, 0.0001))
}
STAGES["gain"] = Stage(
    "gain",
    "_gs_gain",
    "_gs_gain_stock",
    ENGINE + 0x13508,
    44,
    11,
    (),
    (),
    {},
    args=(0, 0, 32),
    blocks=9,
    controls_per_block=(1, 0, 0, 0, 0, 1, 0, 0, 0),
    extra={"rrec": "_gs_gain_rec", "words": (7, 1, 7, bits(0.5), 7, 1, 0, 5, 0, 0, 0)},
)
STAGES["case5"] = Stage(
    "case5",
    "_gs_c5",
    "_gs_c5_stock",
    ENGINE + 0x127C8,
    212,
    53,
    tuple(CASE5_VALUES),
    (9, 10),
    CASE5,
    scratch=True,
    extra={"rrec": "_gs_c5_rec", "values": CASE5_VALUES},
)


def init_record(session, address, size, initial, words_=None):
    poke_words(session, address, list(words_) if words_ else [0] * size)
    for i, v in initial.items():
        session.poke(address + 4 * i, bits(v))


def run_blend(session, buffer, scratch=SCRATCH):
    """The caller's continuation after the case 5 call: the stock blend 0x1c9c12 to 0x1c98b9 on the buffer and the scratch block."""
    session.poke(ENGINE + 0x13808, scratch)
    session.symbols["_blend"] = BLEND_ENTRY
    session.runner.breakpoints = frozenset((BACK, BLEND_STOP))
    regs = {
        **REGS,
        "I0": BLEND,
        "I1": ENGINE,
        "I5": buffer,
        "R9": 32,
        "I6": FRAME,
        "I7": FRAME - 8,
    }
    result = session.call("_blend", regs, max_steps=4000)
    assert (
        result["halt"]["reason"] == "breakpoint" and result["final_pc_sw"] == BLEND_STOP
    ), result


def block_input(rng, scale=0.4):
    return [bits(rng.uniform(-scale, scale)) for _ in range(32)]


def stage_regs(stage):
    """The registers of the caller that the stage's call keeps (case 5: the engine's I0, I1, I5 and R9 at the call, read by the blend that follows)."""
    return {"I0": BLEND, "I1": ENGINE, "I5": LBUF, "R9": 32} if stage.scratch else {}


def setup_blend(session):
    poke_words(session, BLEND, [0] * 14)
    for i, v in {0: 1.0, 2: -0.01, 7: 0.0, 9: 0.01}.items():
        session.poke(BLEND + 4 * i, bits(v))


def run_chain(rig, stage, blocks=None, seed=1):
    """Stereo core with the wrapper, left and right reference cores with the stock function. Returns the three cores' observations per block."""
    blocks = blocks or stage.blocks
    rng = random.Random(seed)
    stereo, left, right = (new_session(rig) for _ in range(3))
    lrec, rrec = stage.l_record(), stage.r_record(rig)
    stock_pc = STOCK[stage.stock]
    out = SCRATCH if stage.scratch else LBUF
    regs = stage_regs(stage)
    for session in (stereo, left, right):
        init_record(session, lrec, stage.size, stage.initial, stage.extra.get("words"))
        setup_blend(session)
    # R state of the stereo core: what the adapter's clear leaves (histories), unwritten (NaN) words elsewhere
    poke_words(stereo, rrec, [0xFFFFFFFF] * stage.size)
    for i in stage.histories:
        stereo.poke(rrec + 4 * i, 0)
    # the right reference's own record: histories zero, controls as the left ones
    for i in stage.histories:
        right.poke(lrec + 4 * i, 0)
    stereo.poke(rig.data["_gs_stamp"] + 4 * SLOT, HEARTBEAT)
    rbuf = rig.data["_gs_rbuf"] + 128 * SLOT
    history = []
    for block in range(blocks):
        l_in, r_in = block_input(rng), block_input(rng, 0.3)
        for session in (stereo, left, right):
            for i in stage.controls:
                base, spread = stage.extra["values"][i]
                session.poke(lrec + 4 * i, bits(base + spread * block))
        poke_words(stereo, LBUF, l_in)
        poke_words(left, LBUF, l_in)
        poke_words(stereo, rbuf, r_in)
        poke_words(right, LBUF, r_in)
        before_l_ctrl = words(stereo, lrec, stage.size)
        args = stage.args_of(block)
        call(stereo, rig.sym[stage.entry], lrec, LBUF, out, args, **regs)
        call(left, stock_pc, lrec, LBUF, out, args, **regs)
        call(right, stock_pc, lrec, LBUF, out, args, **regs)
        if stage.scratch:
            for session in (stereo, left, right):
                run_blend(session, LBUF)
        history.append(
            {
                "l": words(stereo, LBUF, 32),
                "l_ref": words(left, LBUF, 32),
                "r": words(stereo, rbuf, 32),
                "r_ref": words(right, LBUF, 32),
                "l_rec": words(stereo, lrec, stage.size),
                "l_rec_ref": words(left, lrec, stage.size),
                "r_rec": words(stereo, rrec, stage.size),
                "r_rec_ref": words(right, lrec, stage.size),
                "l_in": l_in,
                "r_in": r_in,
                "ctrl": before_l_ctrl,
            }
        )
    return history, stereo


@pytest.mark.parametrize("name", STAGES)
def test_the_twin_matches_two_isolated_mono_channels(rig, name):
    stage = STAGES[name]
    history, stereo = run_chain(rig, stage)
    for h in history:
        assert h["l"] == h["l_ref"], "L is the stock path"
        assert h["l_rec"] == h["l_rec_ref"]
        assert h["r"] == h["r_ref"], "R is the stock stage on independent state"
        words_checked = (
            range(stage.size)
            if stage.name == "gain"
            else (*stage.controls, *stage.histories)
        )
        for i in words_checked:
            assert h["r_rec"][i] == h["r_rec_ref"][i], f"R record word {i}"
        assert h["l"] != h["r"]
        assert h["r"] != h["r_in"] and h["l"] != h["l_in"]
        assert all((w & 0x7F800000) != 0x7F800000 for w in h["l"] + h["r"])
    # the histories are independent: both are live and different
    assert not stage.histories or any(
        history[-1]["l_rec"][i] != history[-1]["r_rec"][i] for i in stage.histories
    )


@pytest.mark.parametrize("name", STAGES)
def test_a_slot_that_is_not_stamped_is_the_plain_stock_call(rig, name):
    stage = STAGES[name]
    stereo, plain = new_session(rig), new_session(rig)
    lrec, rrec = stage.l_record(), stage.r_record(rig)
    for session in (stereo, plain):
        init_record(session, lrec, stage.size, stage.initial)
        poke_words(session, LBUF, block_input(random.Random(3)))
    poke_words(stereo, rrec, [0x12345678] * stage.size)
    stereo.poke(rig.data["_gs_stamp"] + 4 * SLOT, HEARTBEAT - 1)
    stereo.poke(rig.data["_gs_rbuf"] + 128 * SLOT, 0x12345678)
    out = SCRATCH if stage.scratch else LBUF
    call(stereo, rig.sym[stage.entry], lrec, LBUF, out, stage.args, **stage_regs(stage))
    call(plain, STOCK[stage.stock], lrec, LBUF, out, stage.args, **stage_regs(stage))
    assert words(stereo, LBUF, 32) == words(plain, LBUF, 32)
    assert words(stereo, lrec, stage.size) == words(plain, lrec, stage.size)
    assert words(stereo, rrec, stage.size) == [0x12345678] * stage.size, (
        "R state untouched"
    )
    assert stereo.peek(rig.data["_gs_rbuf"] + 128 * SLOT) == 0x12345678


SCRATCH_R = SCRATCH + 0x200
RREC = 0x20190000  # a separate record for the variants


def variant_r(rig, stage, history, mode):
    """R by hand with the stock function: `early` copies the control words from the L record before the L call (what the twin does, so it must give the twin's R),
    `late` copies them after the L call, `shared` runs R on the L record."""
    session = new_session(rig)
    lrec = stage.l_record()
    rrec = lrec if mode == "shared" else RREC
    regs = stage_regs(stage)
    init_record(session, lrec, stage.size, stage.initial, stage.extra.get("words"))
    if mode != "shared":
        init_record(session, rrec, stage.size, {}, [0] * stage.size)
    setup_blend(session)
    out = SCRATCH if stage.scratch else LBUF
    outputs = []
    for block, h in enumerate(history):
        for i in stage.controls:
            base, spread = stage.extra["values"][i]
            session.poke(lrec + 4 * i, bits(base + spread * block))
        poke_words(session, LBUF, h["l_in"])
        copy = [i for i in range(stage.size) if i not in stage.histories]
        if mode == "early":
            for i in copy:
                session.poke(rrec + 4 * i, session.peek(lrec + 4 * i))
        call(session, STOCK[stage.stock], lrec, LBUF, out, stage.args_of(block), **regs)
        if stage.scratch:
            run_blend(session, LBUF)
        if mode == "late":
            for i in copy:
                session.poke(rrec + 4 * i, session.peek(lrec + 4 * i))
        poke_words(session, LBUF, h["r_in"])
        call(
            session,
            STOCK[stage.stock],
            rrec,
            LBUF,
            SCRATCH_R if stage.scratch else LBUF,
            stage.args_of(block),
            **regs,
        )
        if stage.scratch:
            run_blend(session, LBUF, SCRATCH_R)
        outputs.append(words(session, LBUF, 32))
    return outputs


@pytest.mark.parametrize("name", STAGES)
def test_the_harness_reproduces_the_twin_with_the_early_capture(rig, name):
    """Positive control for the two negative controls below."""
    stage = STAGES[name]
    history, _ = run_chain(rig, stage)
    assert variant_r(rig, stage, history, "early") == [h["r"] for h in history]


@pytest.mark.parametrize("name", STAGES)
def test_a_shared_history_gives_a_different_r(rig, name):
    stage = STAGES[name]
    history, _ = run_chain(rig, stage)
    assert variant_r(rig, stage, history, "shared") != [h["r"] for h in history]


def test_controls_copied_after_the_l_call_give_a_different_r_for_the_gain_stage(rig):
    """The L call advances the stage's control words once per block; an R pass that starts from the advanced words is a different gain trajectory."""
    stage = STAGES["gain"]
    history, _ = run_chain(rig, stage)
    assert variant_r(rig, stage, history, "late") != [h["r"] for h in history]


def test_the_callee_saved_registers_survive_the_stereo_path(rig):
    """`call` checks them for every stage above; this one holds the identity under hostile context registers too."""
    stage = STAGES["first-order"]
    session = new_session(rig)
    init_record(session, stage.l_record(), stage.size, stage.initial)
    session.poke(rig.data["_gs_stamp"] + 4 * SLOT, HEARTBEAT)
    for i in stage.histories:
        session.poke(stage.r_record(rig) + 4 * i, 0)
    poke_words(session, LBUF, block_input(random.Random(2)))
    call(
        session,
        rig.sym[stage.entry],
        stage.l_record(),
        LBUF,
        LBUF,
        stage.args,
        M6=1,
        M5=0,
        M14=1,
        M7=0xFFFFFFFF,
    )
