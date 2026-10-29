"""The diagnostic's actual consumer, through the machine JUMP/CJUMP bridge.

Sparse packed input covers both ends of the 480000-frame view. These checks use
its real note-on/render code and retain the adapter frame/keep-register guards.
"""

import struct

import pytest
from support.paths import REPO_ROOT

from tools.patchkit import pk_sharc

HERE = REPO_ROOT / "modules/core/adapters/dsp-ddr-proof/src/dsp"
STATE, INPUT, PARAMS, OUT, READY, OBSERVE = [0x08020000 + n * 0x100 for n in range(6)]
SOURCE = 0x10200000
RETURN = 0xB9F000
REGS = {
    **{f"I{i}": 0x20098000 + i * 64 for i in range(8, 16)},
    **{f"M{i}": i * 17 for i in range(16)},
    **{f"L{i}": 0 for i in range(8)},
    "M0": 0,
    "M5": 0,
    "M6": 1,
    "M7": 0xFFFFFFFF,
    "M13": 0,
    "M14": 1,
    "M15": 0xFFFFFFFF,
    "I6": 0x2009C000,
    "I7": 0x2009C000,
    "I13": RETURN,
    "MODE1": 0x8000,
}
KEEP = (
    "I6",
    "I7",
    *[f"I{i}" for i in range(8, 16)],
    "M0",
    *[f"M{i}" for i in range(5, 16)],
    *[f"L{i}" for i in range(8)],
    "MODE1",
)


def sample(i):
    return ((i * 137) % 65536) - 32768


def bits(i):
    return struct.unpack("<I", struct.pack("<f", sample(i) / 32768))[0]


@pytest.fixture(scope="module")
def consumer():
    text = "\n".join(
        [
            '#include "dk_machine.inc"',
            '#include "dsp/call.inc"',
            '#include "dsp/sample.inc"',
            '#include "dsp/sample_pcm16.inc"',
            '#include "playback.inc"',
        ]
    )
    externs = {"_dd_ready": READY}
    externs.update(
        {
            f"_dd_play_{name}": OBSERVE + i
            for i, name in enumerate(("state", "notes", "blocks", "done", "output"))
        }
    )
    return pk_sharc.build(
        pk_sharc.expand_includes(
            text, [REPO_ROOT / "sdk/asm", REPO_ROOT / "sdk", HERE]
        ),
        0xB90000,
        externs,
    )[1]


def voice(consumer, ready=1, reverse=False):
    sess = pk_sharc.CallSession([consumer], RETURN)
    for i in [*range(0, 128, 2), *range(479872, 480000, 2)]:
        sess.poke(
            SOURCE + i // 2, (sample(i) & 65535) | ((sample(i + 1) & 65535) << 16)
        )
    sess.poke(READY, ready)
    sess.poke(PARAMS, 256 if reverse else 0)
    sess.poke(INPUT + 3, OUT)
    for i in range(5):
        sess.poke(OBSERVE + i, 0)
    sess.proof_seeded = False
    return sess


def call(sess, entry):
    # The DN2 adapter enters with I7==I6, an exclusive boundary. Keep a guard
    # there; the bridge must reserve space before the kernel's postmodify pushes.
    sess.poke(REGS["I7"] // 4, 0xABCD1234)
    regs = REGS if not sess.proof_seeded else {}
    r = sess.call(
        entry, {**regs, "R4": STATE, "R8": INPUT, "R12": PARAMS}, read_regs=KEEP
    )
    sess.proof_seeded = True
    assert r["halt"]["reason"] == "breakpoint", r["halt"]
    assert all(r["regs"][n] == REGS[n] for n in KEEP)
    assert sess.peek(REGS["I7"] // 4) == 0xABCD1234
    assert r["instructions"] <= (2600 if "note_on" in entry else 2400)
    return r


@pytest.mark.parametrize("ready", [0, 2, 0xFFFFFFFF])
def test_unready_note_is_silent_done_and_does_not_adopt_later_ready(consumer, ready):
    sess = voice(consumer, ready)
    call(sess, "_dd_play_note_on")
    sess.poke(READY, 1)
    sess.record_reads(SOURCE, 240000)
    r = call(sess, "_dd_play_render")
    assert not sess.stores
    assert [sess.peek(OUT + i) for i in range(32)] == [0] * 32
    assert r["regs"]["R0"] == 1
    assert sess.peek(STATE + 1) == 0


@pytest.mark.parametrize("reverse", [False, True])
def test_full_view_direction_final_block_done_and_retrigger(consumer, reverse):
    sess = voice(consumer, reverse=reverse)
    for _ in range(2):
        call(sess, "_dd_play_note_on")
        assert sess.peek(STATE + 1) == 480000
        assert sess.peek(STATE + 2) == (479999 if reverse else 0)
        assert sess.peek(STATE + 4) == ((-65536 if reverse else 65536) & 0xFFFFFFFF)
        sess.poke(READY, 0)  # Admission is latched; the owned source is immutable.
        sess.poke(PARAMS, 0 if reverse else 256)
        r = call(sess, "_dd_play_render")
        indices = range(479999, 479967, -1) if reverse else range(32)
        assert [sess.peek(OUT + i) for i in range(32)] == [bits(i) for i in indices]
        assert r["regs"]["R0"] == 0
        sess.poke(STATE + 2, 31 if reverse else 479968)
        r = call(sess, "_dd_play_render")
        indices = range(31, -1, -1) if reverse else range(479968, 480000)
        assert [sess.peek(OUT + i) for i in range(32)] == [bits(i) for i in indices]
        assert sess.peek(STATE + 2) == (0xFFFFFFFF if reverse else 480000)
        assert sess.peek(STATE + 5) == 1
        assert r["regs"]["R0"] == 1
        sess.record_reads(SOURCE, 240000)
        assert call(sess, "_dd_play_render")["regs"]["R0"] == 1
        assert not sess.stores
        assert [sess.peek(OUT + i) for i in range(32)] == [0] * 32
        sess.poke(READY, 1)
        sess.poke(PARAMS, 256 if reverse else 0)
