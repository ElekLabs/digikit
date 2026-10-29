"""tom/para on the concrete SHARC+ core through core/dsp-voice, against the float32 model (tools/patchkit/para_ref.py)."""

import numpy as np
import pytest
from support.machine_rig import MachineRig
from support.pluck_rig import SKIP_NO_SELACHE

from tools.patchkit import para_ref

pytestmark = SKIP_NO_SELACHE

IDS = ("pa_wave", "pa_dtun", "pa_atk", "pa_dec", "pa_jump", "pa_sprd")
SAMPLE = 0x8422B9E8  # sample memory (DDR), where the firmware put a slot's first sample
SERUM = para_ref.serum_table(8)


class Rig(MachineRig):
    def __init__(self, table: np.ndarray, addr: int = SAMPLE) -> None:
        super().__init__("tom/para", "dt2")
        pad = addr & 3
        raw = bytes(pad) + np.asarray(table, "<i2").tobytes() + bytes(8)
        raw += bytes(-len(raw) % 4)
        base = addr - pad
        st = self.sess._st  # sample memory is byte-addressed: CallSession.poke maps a normal-word address
        for k in range(len(raw) // 4):
            w = int.from_bytes(raw[4 * k : 4 * k + 4], "little")
            assert st._dm_write(self.sess.runner.state, base + 4 * k, 4, st.Const(w))
        self.rec_word(self.voice, 0, addr)
        self.rec_word(self.voice, 0x188, len(table))


def play(table: np.ndarray, params: list[int], events: dict[int, tuple], n: int, addr: int = SAMPLE, smooth: bool = True):
    """EVENTS: block -> ("on", note) or ("off",). The routine and the model, block for block: (got L, got R, want L, want R)."""
    rig = Rig(table, addr)
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, params, strict=True))})
    m = para_ref.Para(table)
    if not smooth:
        rig.hw(0x7C + 0x30 * rig.track, 1)  # the track's flags word (core/trackflags), frame byte 0xf8 + 0x60 t: bit 0
        m.smooth = False
    gl, gr, wl, wr = [], [], [], []
    for b in range(n):
        ev = events.get(b)
        released = False
        if ev and ev[0] == "on":
            notes = ev[2] if len(ev) > 2 else 0
            rig.hw(0x7A + 0x30 * rig.track, notes & 0xFFFF)  # core/polynotes' notes 2-4, frame byte 0xf4 + 0x60 t
            rig.hw(0x7B + 0x30 * rig.track, notes >> 16)
            rig.set_note(ev[1], 100)
            rig.trigger()
            m.note_on_notes(int(ev[1] * 256), notes)
        elif ev:
            rig.release()
            released = True
        blk = rig.block()
        assert blk.returned, blk.halt
        gl.append(np.array(blk.samples, np.float32))
        gr.append(np.array(blk.right, np.float32))
        left, right, _ = m.block(params, released)
        wl.append(left)
        wr.append(right)
    return np.concatenate(gl), np.concatenate(gr), np.concatenate(wl), np.concatenate(wr)


CHORD = {0: ("on", 48), 5: ("on", 55), 10: ("on", 60), 15: ("on", 64), 20: ("off",), 25: ("on", 67)}
# trigs with notes 2-4 (0x40 + the interval): four notes, three (steals two slots), notes that are none, a single note
CHORD_TRIGS = {0: ("on", 48, 0x4B4744), 6: ("on", 55, 0x4744), 12: ("on", 60, 0x00FF40), 20: ("off",), 25: ("on", 67)}
CASES = {
    "chord": (SERUM, SAMPLE, [0x2000, 0x5000, 0x1000, 0x3000, 0x4000, 0x7F00], CHORD),
    "jump-up": (SERUM, SAMPLE, [0x3000, 0x4800, 0, 0x2000, 0x4200, 0x4000], CHORD),
    "jump-down-wraps": (SERUM, SAMPLE + 2, [0x1000, 0x3000, 0x2000, 0x1000, 0x3D00, 0x6000], CHORD),
    "short-decay": (SERUM, SAMPLE, [0x7F00, 0x4000, 0, 0, 0x4000, 0], {0: ("on", 60), 3: ("off",), 8: ("on", 62)}),
    "single-cycle": (para_ref.serum_table(1, 601), SAMPLE, [0, 0x5000, 0x800, 0x4000, 0x4800, 0x7F00], CHORD),
    "chord-trigs": (SERUM, SAMPLE, [0x2000, 0x5000, 0x1000, 0x3000, 0x4200, 0x7F00], CHORD_TRIGS),
}


@pytest.mark.parametrize("name", CASES)
def test_the_routine_matches_the_model(name):
    table, addr, params, events = CASES[name]
    gl, gr, wl, wr = play(table, addr=addr, params=params, events=events, n=40)
    assert np.max(np.abs(wl)) > 0.05
    np.testing.assert_allclose(gl, wl, atol=2e-5)
    np.testing.assert_allclose(gr, wr, atol=2e-5)


def test_each_trig_takes_the_next_of_four_slots():
    m = para_ref.Para(SERUM)
    assert [m.note_on(n << 8) for n in (48, 52, 55, 59, 62, 64)] == [0, 1, 2, 3, 0, 1]
    assert m.pit == [62 << 8, 64 << 8, 55 << 8, 59 << 8]


def test_a_trigs_notes_take_the_next_slots_round_robin():
    m = para_ref.Para(SERUM)
    assert m.note_on_notes(48 << 8, 0x4B4744) == [0, 1, 2, 3]
    assert m.note_on_notes(55 << 8, 0x3944) == [0, 1, 2]
    assert m.note_on_notes(60 << 8, 0xFF4000) == [3]
    assert m.pit == [55 << 8, 59 << 8, 48 << 8, 60 << 8]


def test_the_envelope_holds_until_the_note_off_then_decays_and_stops():
    m = para_ref.Para(SERUM)
    params = [0, 0x4000, 0x2000, 0x2000, 0x4000, 0]  # ATK 32 (about 20 ms), DEC 32
    m.note_on(60 << 8)
    for _ in range(200):  # 133 ms
        _, _, done = m.block(params)
    assert m.lvl[0] == 1.0 and m.stg[0] == para_ref.HELD and not done
    m.block(params, released=True)
    assert m.stg[0] == para_ref.DECAY
    t60 = para_ref.gen.decay_s(32)
    blocks = 0
    while not done:
        _, _, done = m.block(params)
        blocks += 1
    assert blocks * 32 / 48000 == pytest.approx(t60 * 4 / 3, rel=0.05)  # -80 dB at 4/3 of the time to -60 dB


def test_attack_rises_linearly_over_its_time():
    m = para_ref.Para(SERUM)
    params = [0, 0x4000, 64 << 8, 0x4000, 0x4000, 0]
    m.note_on(60 << 8)
    n = round(para_ref.gen.attack_s(64) * 48000)
    env = np.concatenate([m.envelope(0, params, 32) for _ in range(n // 32 + 2)])
    assert env[n // 2] == pytest.approx(0.5, abs=0.01)
    assert env[n + 1] == 1.0


@pytest.mark.parametrize(("jump", "want"), [(0x4000, [3, 3, 3, 3]), (0x4200, [3, 5, 7, 1]), (0x3E00, [3, 1, 7, 5]), (0x4D00, [3, 0, 5, 2])])
def test_jump_offsets_each_slot_by_frames_and_wraps(jump, want):
    m = para_ref.Para(SERUM)  # 8 frames; WAVE 3/7 of the way: frame 3
    _, nf, fab, _ = m.frames([3 * 0x7F00 // 7 + 1, 0, 0, 0, jump, 0])
    assert nf == 8 and [a for a, _ in fab] == want


def test_cost_per_block():
    rig = Rig(SERUM)
    rig.set_params({"tune": 0x4000, **dict(zip(IDS, [0x3000, 0x5000, 0x1000, 0x3000, 0x4200, 0x5000], strict=True))})
    counts = []
    for note in (48, 52, 55, 59, 0, 0):
        if note:
            rig.set_note(note, 100)
            rig.trigger()
        blk = rig.block()
        counts.append(blk.instructions)
    assert max(counts) < 13000, counts


def test_smoothing_off_plays_the_nearer_frame():
    params = [0x3A00, 0x4000, 0, 0x4000, 0x4400, 0x7F00]  # WAVE between frames 3 and 4, nearer 4; JUMP +4 wraps
    gl, gr, wl, wr = play(SERUM, params, CHORD, 30, smooth=False)
    np.testing.assert_allclose(gl, wl, atol=2e-5)
    np.testing.assert_allclose(gr, wr, atol=2e-5)
    _, _, sl, _ = play(SERUM, params, CHORD, 30)
    assert np.max(np.abs(wl - sl)) > 1e-3
