"""tom/chord on the concrete SHARC+ core through core/dsp-voice, against the float32 model (tools/patchkit/chord_ref.py)."""

import numpy as np
import pytest
from support.machine_rig import MachineRig
from support.pluck_rig import SKIP_NO_SELACHE

from tools.patchkit import chord_ref

pytestmark = SKIP_NO_SELACHE

IDS = ("ch_loop", "ch_dtun", "ch_int2", "ch_int3", "ch_int4", "ch_sprd")
LEFT = 0x8422B9E8  # sample memory (DDR), where the firmware put a slot's first sample
RIGHT = 0x8423B9EA  # a stereo slot's right channel (an upper half)
OFF = 0x4000


def note(n: int) -> np.ndarray:
    """A decaying two-partial tone, a stand-in for a one-shot instrument note (n frames)."""
    t = np.arange(n)
    x = (np.sin(2 * np.pi * t / 61) + 0.4 * np.sin(2 * np.pi * t / 23)) * np.exp(-t / (n / 3))
    return np.round(x / np.max(np.abs(x)) * 0.8 * 32767).astype(np.int16)


class Rig(MachineRig):
    def __init__(self, left: np.ndarray, right: np.ndarray | None = None, addr: int = LEFT) -> None:
        super().__init__("tom/chord", "dt2")
        st = self.sess._st
        for base, ch in ((addr, left), (RIGHT, right)):
            if ch is None:
                continue
            pad = base & 3
            raw = bytes(pad) + np.asarray(ch, "<i2").tobytes() + bytes(8)
            raw += bytes(-len(raw) % 4)
            for k in range(len(raw) // 4):
                w = int.from_bytes(raw[4 * k : 4 * k + 4], "little")
                assert st._dm_write(self.sess.runner.state, base - pad + 4 * k, 4, st.Const(w))
        self.rec_word(self.voice, 0, addr)
        self.rec_word(self.voice, 0x188, len(left))
        self.rec_word(self.voice + 1, 0, RIGHT if right is not None else addr)


def run(left, right, note_, params, n, addr=LEFT):
    rig = Rig(left, right, addr)
    rig.set_note(note_, 100)
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, params, strict=True))})
    rig.trigger()
    m = chord_ref.Chord(left, right)
    m.note_on()
    gl, gr, wl, wr = [], [], [], []
    for _ in range(n):
        b = rig.block()
        assert b.returned, b.halt
        gl.append(np.array(b.samples, np.float32))
        gr.append(np.array(b.right, np.float32))
        left_, right_, _ = m.block(note_ << 8, params)
        wl.append(left_)
        wr.append(right_)
    return np.concatenate(gl), np.concatenate(gr), np.concatenate(wl), np.concatenate(wr), m


S = note(1500)
CASES = {
    "one-shot": (S, None, 60, [OFF, OFF, OFF, OFF, OFF, 0], LEFT),
    "stereo": (S, (S // 2).astype(np.int16), 67, [OFF, 0x5000, 0x4800, OFF, OFF, 0x7F00], LEFT),
    "forward-loop": (S, None, 64, [0x6000, OFF, 0x4400, 0x3C00, OFF, 0x4000], LEFT),
    "ping-pong": (S, (-S).astype(np.int16), 55, [0x2000, 0x3000, OFF, 0x4800, OFF, 0x6000], LEFT),
    "odd-start": (S, None, 48, [0x4100, OFF, 0x4C00, OFF, 0x5000, 0x2000], LEFT + 2),
    "unison-4": (S, None, 60, [0x3F00, 0x6800, 0x4100, 0x4100, 0x4100, 0x7F00], LEFT),
}


@pytest.mark.parametrize("name", CASES)
def test_the_routine_matches_the_model(name):
    left, right, note_, params, addr = CASES[name]
    gl, gr, wl, wr, _ = run(left, right, note_, params, 80, addr)
    assert np.max(np.abs(wl)) > 0.02
    np.testing.assert_allclose(gl, wl, atol=1e-6)
    np.testing.assert_allclose(gr, wr, atol=1e-6)


def test_a_one_shot_ends_at_the_sample_end():
    _, _, wl, _, m = run(S, None, 60, [OFF, OFF, OFF, OFF, OFF, 0], 60)
    assert not any(m.alive) and not np.any(wl[1600:])


@pytest.mark.parametrize(("word", "want"), [(0x4000, (0, -1)), (0x4100, (1, 0)), (0x3F00, (-1, 0)), (0x7F00, (1, 998)), (0, (-1, 998))])
def test_loop_knob(word, want):
    assert chord_ref.loop(word, 1000) == want


def test_ping_pong_turns_at_the_ends():
    m = chord_ref.Chord(S)
    m.note_on()
    ups, downs = 0, 0
    for _ in range(200):
        before = m.back[0]
        m.block(60 << 8, [0x3F00, OFF, OFF, OFF, OFF, 0])
        ups += before and not m.back[0]
        downs += m.back[0] and not before
    assert m.alive[0] and ups >= 2 and downs >= 2


def test_cost_per_block():
    rig = Rig(S, (S // 2).astype(np.int16))
    rig.set_note(60, 100)
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, [0x3F00, 0x5000, 0x4400, 0x4700, 0x4C00, 0x5000], strict=True))})
    rig.trigger()
    counts = [rig.block().instructions for _ in range(4)]
    assert max(counts) < 20000, counts
