"""The opt-in FIR accelerator, FIR0 (tools/sharc_core/fir.py; ADSP-2156x HWR chapter 38).

Each case lays out TCBs, coefficients and data in a RAM window, starts FIR0 by a guest
write of FIR_CTL1 (as code on the DSP would), and checks the output: against an exact
convolution where the values make every sum exact (the buffer layout of HWR "System
Memory Storage"), else against a float32 FIR in the model's documented order; plus the
write-back, the status and the SEC source.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

import sharcldr
from sharc_core import fir
from sharc_core.memory import _dm_read, _dm_write
from sharc_core.periph import SEC_CCTL, SEC_GCTL, SEC_SCTL
from sharc_core.state import State
from sharc_core.values import Const

RAM = 0x28100000
SIZE = 0x4000
TCB0, TCB1 = RAM + 0x000, RAM + 0x080
COEF0, COEF1 = RAM + 0x200, RAM + 0x400
IN0, IN1 = RAM + 0x800, RAM + 0xC00
OUT0, OUT1 = RAM + 0x1000, RAM + 0x1400
SID = fir.SID_FIR0_DMA


def _state() -> State:
    memory = sharcldr.LoadedMemory(
        bytes(SIZE),
        [dict(target_address=RAM, byte_count=SIZE, fill=False, payload_offset=0, payload_len=SIZE)],
    )
    state = State(pc_sw=0, concrete=memory, assume_nw32=True, peripheral_model=True)
    state.mmrs[SEC_GCTL] = Const(1)
    state.mmrs[SEC_CCTL] = Const(1)
    state.mmrs[SEC_SCTL + 8 * SID] = Const(5)            # IEN | SEN
    return state


def _w(byte: int) -> int:
    """A byte address as the accelerator takes it: a word address."""
    return byte >> 2


def _f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


def _poke(state: State, byte: int, words) -> None:
    for k, value in enumerate(words):
        raw = struct.pack("<f", value) if isinstance(value, float) else struct.pack("<I", value & 0xFFFFFFFF)
        for j, b in enumerate(raw):
            state.overlay[byte + 4 * k + j] = b


def _peek(state: State, byte: int, n: int, fmt: str = "f") -> list:
    raw = bytes(state.overlay.get(byte + k, 0) for k in range(4 * n))
    return list(struct.unpack("<%d%s" % (n, fmt), raw))


def _ctl2(taps: int, window: int, imask: bool = False) -> int:
    return (taps - 1) | ((window - 1) << fir.CTL2_WINDOW_SHIFT) | (fir.CTL2_IMASK if imask else 0)


def _legacy_tcb(state, tcb, nxt, ctl2, coef, inp, out, incnt=0, inbase=None, coefcnt=0):
    """TCB + 0 CHNPTR ... TCB + 12 CTL2 (HWR Table 38-6); NXT is the next TCB's start."""
    _poke(state, tcb, [
        _w(nxt) + 12,                                   # CHNPTR: the next TCB + 12
        coefcnt, 1, _w(coef),                           # COEFCNT, COEFMOD, COEFIDX
        _w(out), 0, 1, _w(out),                         # OUTBASE, OUTCNT, OUTMOD, OUTIDX
        _w(inp if inbase is None else inbase), incnt, 1, _w(inp),   # INBASE, INCNT, INMOD, INIDX
        ctl2,
    ])


def _acm_tcb(state, tcb, nxt, ctl2, coef, inp, out, sgctl=0):
    """TCB + 0 CHNPTR, SCTL1, SCTL2, SGCTL, COEFCNT ... TCB + 15 CTL2 (HWR Table 38-7)."""
    _poke(state, tcb, [
        (_w(nxt) + 15) if nxt else 0, 0, 0, sgctl,
        0, 1, _w(coef),
        _w(out), 0, 1, _w(out),
        _w(inp), 0, 1, _w(inp),
        ctl2,
    ])


def _coefs(state, at, c):
    """C[0..N-1] stored as the HWR asks: C[N-1] first."""
    _poke(state, at, [float(v) for v in reversed(c)])


def _reference(c, history, xs):
    """The model's documented order: four sums over the taps k, k + 4, ... in memory
    order (oldest sample first), each product and sum rounded to float32."""
    mem = [_f32(v) for v in reversed(c)]
    data = [_f32(v) for v in list(history) + list(xs)]
    out = []
    for j in range(len(xs)):
        acc = []
        for i, cv in enumerate(mem):
            p = _f32(cv * data[j + i])
            if i < 4:
                acc.append(p)
            else:
                acc[i % 4] = _f32(acc[i % 4] + p)
        y = acc[0]
        for a in acc[1:]:
            y = _f32(y + a)
        out.append(y)
    return out


def _start(state, ctl1, chnptr):
    assert _dm_write(state, fir.FIR_CHNPTR, 4, Const(chnptr))
    assert _dm_write(state, fir.FIR_CTL1, 4, Const(ctl1))


def _rerun(state, ctl1, chnptr):
    _dm_write(state, fir.FIR_CTL1, 4, Const(0))
    _start(state, ctl1, chnptr)


def _pending(state) -> bool:
    return bool(state.mmrs.get(SEC_SCTL + 8 * SID + 4, Const(0)).value & 0x100)


GO = fir.CTL1_EN | fir.CTL1_DMAEN


def test_single_rate_is_the_convolution_over_the_documented_buffer_layout():
    state = _state()
    c = [1.0, 2.0, 3.0, 4.0, 5.0]                       # y[n] = sum c[k] x[n-k]: integers, exact
    history = [7.0, -1.0, 2.0, 3.0]                     # x[n-4] .. x[n-1]
    xs = [1.0, 0.0, -2.0, 5.0, 4.0, 0.0, 1.0, 3.0]
    _poke(state, IN0, history + xs)
    _coefs(state, COEF0, c)
    _legacy_tcb(state, TCB0, TCB0, _ctl2(5, 8), COEF0, IN0, OUT0, coefcnt=5)
    _start(state, GO, _w(TCB0) + 12)
    data = history + xs
    want = [sum(c[k] * data[4 + j - k] for k in range(5)) for j in range(8)]
    assert _peek(state, OUT0, 8) == want
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [_w(IN0) + 8]        # INIDX + W
    assert _peek(state, TCB0 + 4 * 7, 1, "I") == [_w(OUT0) + 8]        # OUTIDX + W
    assert _dm_read(state, fir.FIR_COEFCNT, 4).value == 0              # 5 - 5 taps
    stat = _dm_read(state, fir.FIR_DMASTAT, 4).value
    assert stat & fir.DMASTAT_ACDONE and stat & fir.DMASTAT_WDONE
    assert _pending(state)                                             # all channels done: SID 118


@pytest.mark.parametrize("taps", [1, 2, 3, 4, 11, 64])
def test_rounding_follows_the_four_mac_order(taps):
    state = _state()
    c = [_f32(0.3 / (k + 1) - 0.07 * (k % 3)) for k in range(taps)]
    data = [_f32(((k * 37) % 23) / 7.0 - 1.4) for k in range(taps - 1 + 16)]
    _poke(state, IN0, data)
    _coefs(state, COEF0, c)
    _legacy_tcb(state, TCB0, TCB0, _ctl2(taps, 16), COEF0, IN0, OUT0)
    _start(state, GO, _w(TCB0) + 12)
    assert _peek(state, OUT0, 16) == _reference(c, data[:taps - 1], data[taps - 1:])


def test_a_circular_input_streams_across_runs():
    state = _state()
    c = [1.0, 10.0, 100.0]                               # 3 taps, windows of 4, a ring of 8
    ring = [float(k) for k in range(8)]
    _poke(state, IN0, ring)
    _coefs(state, COEF0, c)
    _legacy_tcb(state, TCB0, TCB0, _ctl2(3, 4), COEF0, IN0, OUT0, incnt=8, inbase=IN0)
    _start(state, GO, _w(TCB0) + 12)
    _poke(state, IN0, [8.0, 9.0])                        # the next block's newest samples, wrapped
    _rerun(state, GO, _w(TCB0) + 12)
    xs = [float(k) for k in range(10)]
    want = [xs[n] + 10 * xs[n - 1] + 100 * xs[n - 2] for n in range(2, 10)]
    assert _peek(state, OUT0, 8) == want
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [_w(IN0) + 0]        # 0 + 4 + 4 in a ring of 8


def test_two_channels_interrupt_each_with_ccintr():
    state = _state()
    _poke(state, IN0, [1.0, 2.0, 3.0, 4.0])
    _poke(state, IN1, [5.0, 6.0, 7.0, 8.0])
    _coefs(state, COEF0, [2.0])
    _coefs(state, COEF1, [1.0, -1.0])
    _legacy_tcb(state, TCB0, TCB1, _ctl2(1, 4), COEF0, IN0, OUT0)
    _legacy_tcb(state, TCB1, TCB0, _ctl2(2, 3), COEF1, IN1, OUT1)
    _start(state, GO | fir.CTL1_CCINTR | (1 << fir.CTL1_CH_SHIFT), _w(TCB0) + 12)
    assert _peek(state, OUT0, 4) == [2.0, 4.0, 6.0, 8.0]
    assert _peek(state, OUT1, 3) == [1.0, 1.0, 1.0]                    # x[n] - x[n-1]
    assert _pending(state)


def test_acm_runs_a_chain_to_its_null_pointer_and_honours_the_interrupt_mask():
    state = _state()
    xs = [float(k) for k in range(10)]
    _poke(state, IN0, xs)
    _poke(state, IN1, xs)
    _coefs(state, COEF0, [0.5, 0.5])
    _coefs(state, COEF1, [1.0, 0.0, -1.0])
    _acm_tcb(state, TCB0, TCB1, _ctl2(2, 8, imask=True), COEF0, IN0, OUT0)
    _acm_tcb(state, TCB1, 0, _ctl2(3, 8, imask=True), COEF1, IN1, OUT1)
    _start(state, GO | fir.CTL1_ACM, _w(TCB0) + 15)
    assert _peek(state, OUT0, 8) == [0.5 + k for k in range(8)]
    assert _peek(state, OUT1, 8) == [2.0] * 8
    assert _peek(state, TCB0 + 4 * 14, 1, "I") == [0]                  # ACM: INIDX 0, OUTIDX all ones
    assert _peek(state, TCB0 + 4 * 10, 1, "I") == [0xFFFFFFFF]
    assert not _pending(state)                                         # IMASK on both
    assert _dm_read(state, fir.FIR_DMASTAT, 4).value & fir.DMASTAT_ACDONE


def test_writing_ctl1_again_while_enabled_does_not_rerun():
    state = _state()
    _poke(state, IN0, [1.0] * 4)
    _coefs(state, COEF0, [2.0])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 4), COEF0, IN0, OUT0)
    _start(state, GO, _w(TCB0) + 12)
    _dm_write(state, fir.FIR_CTL1, 4, Const(GO | fir.CTL1_CCINTR))
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [_w(IN0) + 4]        # one run only


@pytest.mark.parametrize("ctl1, ctl2", [
    (fir.CTL1_FXD, _ctl2(1, 1)),
    (fir.CTL1_RND, _ctl2(1, 1)),
    (fir.CTL1_CAI, _ctl2(1, 1)),
    (0, _ctl2(1, 1) | fir.CTL2_SRCEN),
    (0, _ctl2(1025, 1)),
])
def test_unmodeled_settings_refuse_instead_of_guessing(ctl1, ctl2):
    state = _state()
    _legacy_tcb(state, TCB0, TCB0, ctl2, COEF0, IN0, OUT0)
    with pytest.raises(ValueError):
        _start(state, GO | ctl1, _w(TCB0) + 12)


def test_registers_read_back_and_status_is_read_only():
    state = _state()
    assert _dm_write(state, fir.FIR_SCTL2, 4, Const(0x1234))
    assert _dm_read(state, fir.FIR_SCTL2, 4) == Const(0x1234)
    assert _dm_write(state, fir.FIR_DMASTAT, 4, Const(0xFFFF))
    assert _dm_read(state, fir.FIR_DMASTAT, 4) == Const(0)


def _native(state: State):
    import os

    import sharc_transpile_run as nr

    library = os.environ.get("SHARC_NATIVE_LIB", nr.DEFAULT_LIB)
    if not Path(library).exists():
        pytest.skip("no native library built")
    core = nr.NativeCore(nr.pack_image(state.concrete), library)
    nr.to_native(core, state)
    return core


def _both(state, core, address, value):
    assert _dm_write(state, address, 4, Const(value))
    assert core.poke(address, value.to_bytes(4, "little"), 4) == 1


@pytest.mark.parametrize("mode", ["legacy", "acm"])
def test_native_twin_matches_the_reference(mode) -> None:
    import sharc_diff as sd

    state = _state()
    data = [_f32(((k * 37) % 23) / 7.0 - 1.4) for k in range(40)]
    _poke(state, IN0, data)
    _poke(state, IN1, [0.5 * x for x in data])
    _coefs(state, COEF0, [_f32(0.3 / (k + 1)) for k in range(11)])
    _coefs(state, COEF1, [0.25, -0.5, 0.25])
    if mode == "legacy":
        _legacy_tcb(state, TCB0, TCB1, _ctl2(11, 24), COEF0, IN0, OUT0, coefcnt=3)
        _legacy_tcb(state, TCB1, TCB0, _ctl2(3, 24), COEF1, IN1, OUT1, incnt=16, inbase=IN1)
        ctl1, pointer = GO | fir.CTL1_CCINTR | (1 << fir.CTL1_CH_SHIFT), _w(TCB0) + 12
    else:
        _acm_tcb(state, TCB0, TCB1, _ctl2(11, 24), COEF0, IN0, OUT0)
        _acm_tcb(state, TCB1, 0, _ctl2(3, 24, imask=True), COEF1, IN1, OUT1)
        ctl1, pointer = GO | fir.CTL1_ACM, _w(TCB0) + 15
    core = _native(state)
    _both(state, core, fir.FIR_CHNPTR, pointer)
    _both(state, core, fir.FIR_CTL1, ctl1)
    assert core.peek(OUT0 + 4 * 23, 4) == struct.unpack("<I", struct.pack("<f", _peek(state, OUT0 + 4 * 23, 1)[0]))[0]
    assert core.peek(fir.FIR_DMASTAT, 4) == _dm_read(state, fir.FIR_DMASTAT, 4).value
    assert sd.compare_states(sd.export_state(state), core.export_state()) == []
