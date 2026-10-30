"""Assembled synthetic DN2 subset; no claim about the stock default route.

The full subset uses the existing approximate RECIPS seed model for 1c06ba.
Bit equality is within this model, not hardware reciprocal exactness. Without
that option, 1c0706 forks on an unknown AV predicate after RECIPS. The separate
recurrence case uses the default core. No instruction semantics are modified.

Original firmware is supplied at runtime, never copied into a fixture. Reference
channels execute independently on separate concrete cores with private state.
"""

import hashlib
import struct
from collections import Counter

import pytest
from support.paths import REPO_ROOT

from tools.patchkit import pk_sharc
from tools.sharc_dev.sharc_core.addressing import normal_word_to_byte
from tools.sharc_dev.sharc_core.memory import _canonical_dm_address
from tools.sharc_dev.sharcldr import LoadedMemory

SOURCE = REPO_ROOT / "tests/fixtures/dsp/stereo-shadow-replay.s"
IMAGE = REPO_ROOT / "out/sections/dn2-1.12/section_7_BLOB.bin"
BACK, STACK = 0xB9F000, 0x08044000
CONTROL, FIRST, UY = 0x08040000, 0x08040200, 0x08040400
LEFT, RIGHT = 0x08041000, 0x08041100
REGS = {
    "I6": STACK,
    "I7": STACK,
    "M5": 0,
    "M6": 1,
    "M7": 0xFFFFFFFF,
    "M14": 1,
    "MODE1": 0,
}
REGS.update({f"L{i}": 0 for i in range(16)})


def bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


@pytest.fixture(scope="module")
def rig():
    if not IMAGE.exists():
        pytest.skip("requires original DN2 1.12 section 7")
    data = IMAGE.read_bytes()
    assert (
        hashlib.sha256(data).hexdigest()
        == "336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2"
    )
    original = LoadedMemory.from_stream(data)
    source = pk_sharc.expand_includes(SOURCE.read_text(), [REPO_ROOT / "sdk"])
    linked = pk_sharc.link(
        pk_sharc.assemble(source),
        0xB90000,
        {
            "_control": 0x1CDB56,
            "_first": 0xB80C39,
            "_uy": 0xB81368,
        },
    )
    return linked, original


def fresh(rig, approximate=True):
    linked, memory = rig
    session = pk_sharc.CallSession(
        [linked], BACK, base_memory=memory, approx_recips=approximate
    )
    session.record_stores()
    return session


def call(session, entry):
    result = session.call(entry, REGS, max_steps=12000)
    assert result["halt"]["reason"] == "breakpoint", result
    assert result["final_pc_sw"] == BACK
    # Canonical byte-address watchpoints bound every executed store, including
    # nested stock frames. Preparation may copy controls/coefficients only.
    if entry == "_prepare":
        allowed = {CONTROL + 0x100 + i for i in range(14)}
        allowed |= {FIRST + 0x100 + i for i in range(21, 24)}
        allowed |= {UY + 0x100 + i for i in range(2, 6)}
    else:
        offset = 0x100 if entry in ("_right", "_right_recurrences") else 0
        allowed = {LEFT + offset + i for i in range(32)}
        allowed |= {CONTROL + offset + i for i in (4, 5, 6, 11, 12, 13)}
        allowed |= {FIRST + offset + i for i in (24, 25)}
        allowed |= {UY + offset + i for i in (6, 7)}
        allowed |= set(range(STACK - 64, STACK + 1))
    byte_addresses = {
        _canonical_dm_address(
            session.runner.state, normal_word_to_byte(a), 4, for_write=True
        ): a
        for a in allowed
    }
    assert all(
        event.width == 4 and event.address in byte_addresses for event in session.stores
    ), [(hex(e.address), e.width) for e in session.stores]
    if entry in ("_left", "_right"):
        counts = Counter(byte_addresses[event.address] for event in session.stores)
        assert [counts[LEFT + offset + i] for i in range(32)] == [4] * 32
        assert [counts[CONTROL + offset + i] for i in (4, 5, 6, 11, 12, 13)] == [2] * 6
    return result


def initialize(session):
    for base, count in ((CONTROL, 14), (FIRST, 28), (UY, 8)):
        for i in range(count):
            session.poke(base + i, 0)
    for i, value in {0: 0.8, 1: 0.3, 2: 0.6, 5: 0.2, 12: 0.1}.items():
        session.poke(CONTROL + i, bits(value))
    for i, value in {21: 0.7, 22: 0.2, 23: 0.1}.items():
        session.poke(FIRST + i, bits(value))
    for i, value in {2: 0.6, 3: 0.3, 4: 0.2, 5: 0.4}.items():
        session.poke(UY + i, bits(value))


def words(session, base, count):
    return [session.peek(base + i) for i in range(count)]


def test_multiblock_shadow_matches_isolated_mono(rig):
    stereo, left, right = (fresh(rig) for _ in range(3))
    for session in (stereo, left, right):
        initialize(session)
    # Initialize private right histories explicitly; preparation must retain them.
    for base, indices in ((FIRST + 0x100, (24, 25)), (UY + 0x100, (6, 7))):
        for i in indices:
            stereo.poke(base + i, 0)
    first_snapshot = None
    for iteration, block in enumerate((0, 1, 2, 0)):
        if iteration == 3:
            # Explicit reset of BOTH audio histories and shared controls. The
            # same block must reproduce the first execution exactly.
            for session in (stereo, left, right):
                initialize(session)
            for base, indices in ((FIRST + 0x100, (24, 25)), (UY + 0x100, (6, 7))):
                for i in indices:
                    stereo.poke(base + i, 0)
        for session in (stereo, left, right):
            session.poke(CONTROL, bits(0.8 - 0.1 * block))
            session.poke(FIRST + 21, bits(0.7 - 0.05 * block))
            session.poke(UY + 2, bits(0.6 + 0.02 * block))
        for i in range(32):
            left_bits, right_bits = (
                bits((i - 11) / 64 + block / 32),
                bits((7 - i) / 83 - block / 64),
            )
            stereo.poke(LEFT + i, left_bits)
            stereo.poke(RIGHT + i, right_bits)
            left.poke(LEFT + i, left_bits)
            right.poke(LEFT + i, right_bits)
        call(stereo, "_prepare")
        call(stereo, "_left")
        authoritative = words(stereo, CONTROL, 14)
        call(stereo, "_right")
        call(left, "_left")
        call(right, "_left")
        assert words(stereo, LEFT, 32) == words(left, LEFT, 32)
        assert words(stereo, RIGHT, 32) == words(right, LEFT, 32)
        assert words(stereo, CONTROL, 14) == authoritative == words(left, CONTROL, 14)
        assert words(stereo, CONTROL + 0x100, 14) == words(right, CONTROL, 14)
        assert words(stereo, LEFT, 32) != words(stereo, RIGHT, 32)
        for base in (FIRST, UY):
            # Ignore unaudited padding: only coefficients/histories are used.
            indices = range(21, 26) if base == FIRST else range(2, 8)
            assert [stereo.peek(base + i) for i in indices] == [
                left.peek(base + i) for i in indices
            ]
            assert [stereo.peek(base + 0x100 + i) for i in indices] == [
                right.peek(base + i) for i in indices
            ]

        snapshot = (
            words(stereo, LEFT, 32),
            words(stereo, RIGHT, 32),
            words(stereo, CONTROL, 14),
            words(stereo, CONTROL + 0x100, 14),
            [
                stereo.peek(base + offset + i)
                for base, indices in ((FIRST, (24, 25)), (UY, (6, 7)))
                for offset in (0, 0x100)
                for i in indices
            ],
        )
        if iteration == 0:
            first_snapshot = snapshot
        elif iteration == 3:
            assert snapshot == first_snapshot
    assert stereo.runner.state.approx_recips_used


def test_recurrences_default_core_matches_independent_channels(rig):
    stereo, left, right = (fresh(rig, approximate=False) for _ in range(3))
    for session in (stereo, left, right):
        initialize(session)
    for base, indices in ((FIRST + 0x100, (24, 25)), (UY + 0x100, (6, 7))):
        for i in indices:
            stereo.poke(base + i, 0)
    for block in range(3):
        for session in (stereo, left, right):
            session.poke(FIRST + 21, bits(0.7 - block / 32))
            session.poke(UY + 2, bits(0.6 + block / 64))
        for i in range(32):
            left_bits, right_bits = (
                bits((i - 13) / 64 + block / 32),
                bits((11 - i) / 89),
            )
            stereo.poke(LEFT + i, left_bits)
            stereo.poke(RIGHT + i, right_bits)
            left.poke(LEFT + i, left_bits)
            right.poke(LEFT + i, right_bits)
        call(stereo, "_prepare")
        call(stereo, "_recurrences")
        call(stereo, "_right_recurrences")
        call(left, "_recurrences")
        call(right, "_recurrences")
        for reference, output, offset in ((left, LEFT, 0), (right, RIGHT, 0x100)):
            assert words(stereo, output, 32) == words(reference, LEFT, 32)
            for base, indices in ((FIRST, (24, 25)), (UY, (6, 7))):
                assert [stereo.peek(base + offset + i) for i in indices] == [
                    reference.peek(base + i) for i in indices
                ]
    assert not stereo.runner.state.approx_recips_used


@pytest.mark.parametrize(
    "fault",
    [
        "copy-advanced-controls",
        "reset-right-history",
        "reset-controls-between-occurrences",
    ],
)
def test_shadow_corruption_is_observable(rig, fault):
    candidate, reference = fresh(rig), fresh(rig)
    for session in (candidate, reference):
        initialize(session)
        for base, indices in ((FIRST + 0x100, (24, 25)), (UY + 0x100, (6, 7))):
            for i in indices:
                session.poke(base + i, 0)
    differences = []
    for block in range(2):
        for session in (candidate, reference):
            session.poke(CONTROL, bits(0.9 - block / 8))
            for i in range(32):
                session.poke(LEFT + i, bits((i - 10) / 64))
                session.poke(RIGHT + i, bits((5 - i) / 73))
            call(session, "_prepare")
            if session is candidate:
                pre_controls = words(session, CONTROL + 0x100, 14)
            call(session, "_left")
        if fault == "copy-advanced-controls":
            # The shadow must start from the pre-left snapshot, not its end.
            call(candidate, "_prepare")
        elif fault == "reset-right-history":
            for base, indices in ((FIRST + 0x100, (24, 25)), (UY + 0x100, (6, 7))):
                for i in indices:
                    candidate.poke(base + i, 0)
        call(reference, "_right")
        if fault == "reset-controls-between-occurrences":
            # Pause after U/Y, before the repeated control occurrence. Reusing
            # the preblock snapshot here loses the first occurrence's writes.
            boundary = rig[0].symbols["_ru"]
            candidate.runner.breakpoints = frozenset((BACK, boundary))
            result = candidate.call("_right", REGS, max_steps=12000)
            assert result["final_pc_sw"] == boundary
            for i, word in enumerate(pre_controls):
                candidate.poke(CONTROL + 0x100 + i, word)
            candidate.runner.breakpoints = frozenset((BACK,))
            result = candidate.runner.run(12000)
            assert result.halt.reason == "breakpoint"
            assert result.final_pc_sw == BACK
        else:
            call(candidate, "_right")
        differences.append(
            (words(candidate, RIGHT, 32), words(candidate, CONTROL, 14))
            != (words(reference, RIGHT, 32), words(reference, CONTROL, 14))
        )
    assert any(differences)


def test_base_image_overlay_and_decode_cache_isolation():
    caller = pk_sharc.build(
        ".SECTION/PM seg_pmco;\n.GLOBAL _entry;\n_entry:\n CALL _callee;\n RTS;",
        0xB90000,
        {"_callee": 0xB91000},
    )[1]
    callees = [
        pk_sharc.build(
            f".SECTION/PM seg_pmco;\n.GLOBAL _callee;\n_callee:\n R0 = {value};\n RTS;",
            0xB91000,
        )[1]
        for value in (7, 19)
    ]
    sessions = []
    for callee in callees:
        data = callee.code_boot + struct.pack("<I", 0x12345678)
        original = LoadedMemory(
            data,
            [
                {
                    "index": 0,
                    "target_address": pk_sharc.SW_ALIAS_BASE + 2 * callee.base_sw,
                    "byte_count": len(callee.code_boot),
                    "fill": False,
                    "payload_offset": 0,
                    "payload_len": len(callee.code_boot),
                },
                {
                    "index": 1,
                    "target_address": LEFT * 4,
                    "byte_count": 4,
                    "fill": False,
                    "payload_offset": len(callee.code_boot),
                    "payload_len": 4,
                },
            ],
        )
        session = pk_sharc.CallSession([caller], BACK, base_memory=original)
        assert session.peek(LEFT) == 0x12345678
        result = session.call("_entry", {}, max_steps=10)
        assert result["halt"]["reason"] == "breakpoint", result
        sessions.append(session)
        # Appended linked code must override the original bytes at the same PC.
        overlaid = pk_sharc.CallSession(
            [caller, callees[1]], BACK, base_memory=original
        )
        assert overlaid.call("_entry", {}, max_steps=10)["regs"]["R0"] == 19
        assert (
            original.read(
                pk_sharc.SW_ALIAS_BASE + 2 * callee.base_sw, len(callee.code_boot)
            )
            == callee.code_boot
        )
    assert [
        session.call("_entry", {}, max_steps=10)["regs"]["R0"] for session in sessions
    ] == [7, 19]
    assert sessions[0].runner._cache is not sessions[1].runner._cache
