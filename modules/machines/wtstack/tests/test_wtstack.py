"""tom/wtstack on the concrete SHARC+ core through core/dsp-voice, against the float32 model (tools/patchkit/wtstack_ref.py)."""

import numpy as np
import pytest
from support.machine_rig import MachineRig
from support.pluck_rig import SKIP_NO_SELACHE

from tools.patchkit import wtstack_ref

pytestmark = SKIP_NO_SELACHE

IDS = ("ws_wave", "ws_dtun", "ws_int2", "ws_int3", "ws_int4", "ws_sprd")
SAMPLE = 0x8422B9E8  # sample memory (DDR), where the firmware put a slot's first sample


class Rig(MachineRig):
    def __init__(self, table: np.ndarray | None, addr: int = SAMPLE) -> None:
        super().__init__("tom/wtstack", "dt2")
        if table is None:
            return
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

    def play(self, note: float, params: list[int]) -> None:
        self.set_note(note, 100)
        self.set_params({"tune": 0x4000, **dict(zip(IDS, params, strict=True))})
        self.trigger()

    def run(self, n: int) -> tuple[np.ndarray, np.ndarray, list[int]]:
        ls, rs, counts = [], [], []
        for _ in range(n):
            b = self.block()
            assert b.returned, b.halt
            ls.append(np.array(b.samples, np.float32))
            rs.append(np.array(b.right, np.float32))
            counts.append(b.instructions)
        return np.concatenate(ls), np.concatenate(rs), counts


def model_run(table: np.ndarray, note: float, params: list[int], n: int) -> tuple[np.ndarray, np.ndarray]:
    m = wtstack_ref.WtStack(table)
    out = [m.block(int(note * 256), params) for _ in range(n)]
    return np.concatenate([o[0] for o in out]), np.concatenate([o[1] for o in out])


SERUM = wtstack_ref.serum_table(8)
OFF = 0x4000
CASES = {
    "one-voice": (SERUM, SAMPLE, 60, [0, OFF, OFF, OFF, OFF, 0x7F00]),
    "stack": (SERUM, SAMPLE, 48, [0x3000, 0x5800, 0x4700, 0x3400, 0x4C00, 0x5000]),
    "last-frame": (SERUM, SAMPLE, 72, [0x7F00, 0x2000, 0x3400, OFF, OFF, 0x7F00]),
    "odd-start": (SERUM, SAMPLE + 2, 55, [0x5A00, 0x4800, 0x4C00, 0x4300, OFF, 0x2000]),
    "unison-4": (SERUM, SAMPLE, 50, [0x2000, 0x6800, 0x4100, 0x4100, 0x4100, 0x7F00]),
    "single-cycle": (wtstack_ref.serum_table(1, 601), SAMPLE, 40, [0x4000, 0x4000, 0x4C00, OFF, OFF, 0x7F00]),
}


@pytest.mark.parametrize("name", CASES)
def test_the_routine_matches_the_model(name):
    table, addr, note, params = CASES[name]
    rig = Rig(table, addr)
    rig.play(note, params)
    gl, gr, _ = rig.run(40)
    wl, wr = model_run(table, note, params, 40)
    assert np.max(np.abs(wl)) > 0.05
    np.testing.assert_allclose(gl, wl, atol=2e-5)
    np.testing.assert_allclose(gr, wr, atol=2e-5)


def test_no_sample_is_silent():
    rig = Rig(None)
    rig.play(60, [0, OFF, 0x4C00, OFF, OFF, 0x7F00])
    gl, gr, _ = rig.run(4)
    assert not np.any(gl) and not np.any(gr)


def test_one_voice_is_centred_and_the_spread_pans_two_apart():
    l1, r1 = model_run(SERUM, 60, [0, OFF, OFF, OFF, OFF, 0x7F00], 20)
    np.testing.assert_array_equal(l1, r1)
    l2, r2 = model_run(SERUM, 60, [0, OFF, 0x4C00, OFF, OFF, 0x7F00], 20)
    assert np.max(np.abs(l2)) > 0.1 and np.max(np.abs(r2)) > 0.1
    assert abs(np.corrcoef(l2, r2)[0, 1]) < 0.5


def test_note_69_is_440_hz_on_the_first_frame():
    L, _ = model_run(SERUM, 69, [0, OFF, OFF, OFF, OFF, 0], 300)
    x = L[3200:]
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x))))
    f = np.argmax(spec) * 48000 / len(x)
    assert abs(f - 440) < 48000 / len(x)


def test_full_detune_is_a_semitone_each_way():
    m = wtstack_ref.WtStack(SERUM)
    _, vs = m.voices(69 << 8, [0, 0x7F00, 0x4000 + (13 << 8), OFF, OFF, 0])
    ratio = vs[1][1] / vs[0][1]
    assert abs(ratio - 2 ** (10 / 12)) < 1e-5  # +1 and 12 - 1 semitones


def test_int_steps_run_minus_24_off_uni_plus_24():
    words = range(0x2800, 0x5A00, 0x100)
    got = [None if w == OFF else wtstack_ref.interval(w) // 256 for w in words]
    assert got == [*range(-24, 0), None, *range(0, 25)]


def test_four_in_unison_are_four_pitches_and_four_pans():
    m = wtstack_ref.WtStack(SERUM)
    _, vs = m.voices(60 << 8, [0, 0x6000, 0x4100, 0x4100, 0x4100, 0x7F00])
    assert len({v[1] for v in vs}) == 4
    assert len({(v[2], v[3]) for v in vs}) == 4


def test_cost_per_block():
    rig = Rig(SERUM)
    rig.play(60, [0x3000, 0x5000, 0x4700, 0x3400, 0x4C00, 0x5000])
    _, _, counts = rig.run(4)
    assert max(counts) < 10500, counts
