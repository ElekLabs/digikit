"""tom/parasmp on the concrete SHARC+ core through core/dsp-voice, against the float32 model (tools/patchkit/parasmp_ref.py)."""

import numpy as np
import pytest
from support.machine_rig import MachineRig
from support.pluck_rig import SKIP_NO_SELACHE

from tools.patchkit import parasmp_ref

pytestmark = SKIP_NO_SELACHE

IDS = ("ps_loop", "ps_dtun", "ps_atk", "ps_dec", "ps_strt", "ps_sprd")
LEFT = 0x8422B9E8
RIGHT = 0x8423B9EA
OFF = 0x4000


def note(n: int) -> np.ndarray:
    t = np.arange(n)
    x = (np.sin(2 * np.pi * t / 61) + 0.4 * np.sin(2 * np.pi * t / 23)) * np.exp(-t / (n / 3))
    return np.round(x / np.max(np.abs(x)) * 0.8 * 32767).astype(np.int16)


class Rig(MachineRig):
    def __init__(self, left: np.ndarray, right: np.ndarray | None = None, addr: int = LEFT) -> None:
        super().__init__("tom/parasmp", "dt2")
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


def play(left, right, params, events, n, addr=LEFT):
    rig = Rig(left, right, addr)
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, params, strict=True))})
    m = parasmp_ref.ParaSmp(left, right)
    gl, gr, wl, wr = [], [], [], []
    for b in range(n):
        ev = events.get(b)
        released = False
        if ev and ev[0] == "on":
            rig.set_note(ev[1], 100)
            rig.trigger()
            m.note_on(ev[1] << 8)
        elif ev:
            rig.release()
            released = True
        blk = rig.block()
        assert blk.returned, blk.halt
        gl.append(np.array(blk.samples, np.float32))
        gr.append(np.array(blk.right, np.float32))
        left_, right_, _ = m.block(params, released)
        wl.append(left_)
        wr.append(right_)
    return np.concatenate(gl), np.concatenate(gr), np.concatenate(wl), np.concatenate(wr), m


S = note(3000)
CHORD = {0: ("on", 48), 4: ("on", 55), 8: ("on", 60), 12: ("on", 64), 20: ("off",), 26: ("on", 67)}
CASES = {
    "one-shot": (S, None, [OFF, 0x5000, 0x1000, 0x3000, 0, 0x7F00], CHORD, LEFT),
    "stereo-forward": (S, (S // 2).astype(np.int16), [0x6000, 0x4800, 0, 0x2000, 0x1000, 0x4000], CHORD, LEFT),
    "ping-pong-start": (S, None, [0x2800, 0x3000, 0x800, 0x4000, 0x3000, 0x6000], CHORD, LEFT + 2),
    "short": (S, None, [OFF, OFF, 0, 0, 0x7000, 0], {0: ("on", 60), 3: ("off",), 8: ("on", 62)}, LEFT),
}


@pytest.mark.parametrize("name", CASES)
def test_the_routine_matches_the_model(name):
    left, right, params, events, addr = CASES[name]
    gl, gr, wl, wr, _ = play(left, right, params, events, 40, addr)
    assert np.max(np.abs(wl)) > 0.01
    np.testing.assert_allclose(gl, wl, atol=1e-6)
    np.testing.assert_allclose(gr, wr, atol=1e-6)


def test_each_trig_takes_the_next_of_four_slots():
    m = parasmp_ref.ParaSmp(S)
    assert [m.note_on(n << 8) for n in (48, 52, 55, 59, 62)] == [0, 1, 2, 3, 0]


def test_a_slot_starts_at_strt_and_a_one_shot_ends():
    m = parasmp_ref.ParaSmp(S)
    m.note_on(60 << 8)
    m.block([OFF, OFF, 0, 0x7F00, 0x4000, 0])
    assert m.pos[0] == pytest.approx(len(S) * 0x4000 / 0x7F00 + 32, abs=1)
    for _ in range(60):
        _, _, done = m.block([OFF, OFF, 0, 0x7F00, 0x4000, 0])
    assert done and m.stg[0] == parasmp_ref.OFF


def test_the_envelope_holds_until_the_note_off_on_a_loop():
    m = parasmp_ref.ParaSmp(S)
    m.note_on(60 << 8)
    params = [0x4100, OFF, 0x2000, 0x2000, 0, 0]
    for _ in range(300):
        m.block(params)
    assert m.stg[0] == parasmp_ref.HELD and m.lvl[0] == 1.0
    m.block(params, released=True)
    assert m.stg[0] == parasmp_ref.DECAY


def test_cost_per_block():
    rig = Rig(S, (S // 2).astype(np.int16))
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, [0x3000, 0x5000, 0x1000, 0x3000, 0, 0x5000], strict=True))})
    counts = []
    for n in (48, 52, 55, 59, 0, 0):
        if n:
            rig.set_note(n, 100)
            rig.trigger()
        counts.append(rig.block().instructions)
    assert max(counts) < 22000, counts
