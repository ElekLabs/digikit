"""The opt-in IIR accelerator, IIR0 (tools/sharc_dev/sharc_core/iir.py; ADSP-2156x HWR chapter 39).

Each case lays out TCBs, coefficients and data in a RAM window, starts IIR0 by a guest
write of IIR_CTL1 (as code on the DSP would), and checks the output against a plain
float32 biquad in the model's documented order, plus the write-back, the status and
the SEC source.
"""

from __future__ import annotations

import struct
from pathlib import Path

import pytest

from tools.sharc_dev import sharcldr
from tools.sharc_dev.sharc_core import iir
from tools.sharc_dev.sharc_core.memory import _dm_read, _dm_write
from tools.sharc_dev.sharc_core.periph import SEC_CCTL, SEC_GCTL, SEC_SCTL
from tools.sharc_dev.sharc_core.state import State
from tools.sharc_dev.sharc_core.values import Const

RAM = 0x28100000
SIZE = 0x4000
TCB0, TCB1, TCB2 = RAM + 0x000, RAM + 0x080, RAM + 0x100
COEF0, COEF1 = RAM + 0x200, RAM + 0x400
IN0, IN1 = RAM + 0x800, RAM + 0xC00
OUT0, OUT1 = RAM + 0x1000, RAM + 0x1400
SID = iir.SID_IIR0_DMA


def _state() -> State:
    memory = sharcldr.LoadedMemory(
        bytes(SIZE),
        [
            dict(
                target_address=RAM,
                byte_count=SIZE,
                fill=False,
                payload_offset=0,
                payload_len=SIZE,
            )
        ],
    )
    state = State(pc_sw=0, concrete=memory, assume_nw32=True, peripheral_model=True)
    state.mmrs[SEC_GCTL] = Const(1)
    state.mmrs[SEC_CCTL] = Const(1)
    state.mmrs[SEC_SCTL + 8 * SID] = Const(5)  # IEN | SEN
    return state


def _w(byte: int) -> int:
    """A byte address as the accelerator takes it: a word address."""
    return byte >> 2


def _f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


def _poke(state: State, byte: int, words) -> None:
    for k, value in enumerate(words):
        raw = (
            struct.pack("<f", value)
            if isinstance(value, float)
            else struct.pack("<I", value & 0xFFFFFFFF)
        )
        for j, b in enumerate(raw):
            state.overlay[byte + 4 * k + j] = b


def _peek(state: State, byte: int, n: int, fmt: str = "f") -> list:
    raw = bytes(state.overlay.get(byte + k, 0) for k in range(4 * n))
    return list(struct.unpack("<%d%s" % (n, fmt), raw))


def _ctl2(biquads: int, window: int, imask: bool = False) -> int:
    return (
        (biquads - 1)
        | ((window - 1) << iir.CTL2_WINDOW_SHIFT)
        | (iir.CTL2_IMASK if imask else 0)
    )


def _legacy_tcb(state, tcb, nxt, ctl2, coef, inp, out, inlen=0, inbase=None):
    """TCB + 0 CHNPTR ... TCB + 12 CTL2 (HWR Table 39-5); NXT is the next TCB's start."""
    _poke(
        state,
        tcb,
        [
            _w(nxt) + 12,  # CHNPTR: the next TCB + 12
            0,
            1,
            _w(coef),  # COEFLEN, COEFMOD, COEFIDX
            _w(out),
            0,
            1,
            _w(out),  # OUTBASE, OUTLEN, OUTMOD, OUTIDX
            _w(inp if inbase is None else inbase),
            inlen,
            1,
            _w(inp),  # INBASE, INLEN, INMOD, INIDX
            ctl2,
        ],
    )


def _acm_tcb(state, tcb, nxt, ctl2, coef, inp, out, sgctl=0):
    """TCB + 0 CHNPTR, SCTL1, SCTL2, SGCTL, COEFLEN ... TCB + 15 CTL2 (HWR Table 39-6)."""
    _poke(
        state,
        tcb,
        [
            (_w(nxt) + 15) if nxt else 0,
            0,
            0,
            sgctl,
            0,
            1,
            _w(coef),
            _w(out),
            0,
            1,
            _w(out),
            _w(inp),
            0,
            1,
            _w(inp),
            ctl2,
        ],
    )


def _biquads(state, at, sections):
    words = []
    for a0, a1, b1, a2, b2, d2, d1 in sections:
        words += [
            float(a0),
            float(a1),
            float(b1),
            float(a2),
            float(b2),
            float(d2),
            float(d1),
        ]
    _poke(state, at, words)


def _reference(xs, sections):
    """The model's documented order, each product and sum rounded to float32."""
    st = [[_f32(s[5]), _f32(s[6])] for s in sections]
    co = [[_f32(c) for c in s[:5]] for s in sections]
    out = []
    for x in xs:
        x = _f32(x)
        for (a0, a1, b1, a2, b2), d in zip(co, st, strict=True):
            y = _f32(_f32(a0 * x) + d[1])
            d[1] = _f32(_f32(_f32(a1 * x) + _f32(b1 * y)) + d[0])
            d[0] = _f32(_f32(a2 * x) + _f32(b2 * y))
            x = y
        out.append(x)
    return out, st


def _start(state, ctl1, chnptr):
    assert _dm_write(state, iir.IIR_CHNPTR, 4, Const(chnptr))
    assert _dm_write(state, iir.IIR_CTL1, 4, Const(ctl1))


def _pending(state) -> bool:
    return bool(state.mmrs.get(SEC_SCTL + 8 * SID + 4, Const(0)).value & 0x100)


LOWPASS = (0.25, 0.0, 0.75, 0.0, 0.0, 0.0, 0.0)  # y = 0.25 x + 0.75 y[-1]
PEAK = (1.1, -1.6, 1.5, 0.62, -0.7, 0.0, 0.0)


def test_one_channel_one_window_matches_the_biquad_and_writes_back_the_indices():
    state = _state()
    xs = [1.0] + [0.0] * 15 + [0.5] * 16
    _poke(state, IN0, [float(x) for x in xs])
    _biquads(state, COEF0, [LOWPASS])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 32), COEF0, IN0, OUT0)
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN, _w(TCB0) + 12)
    want, _ = _reference(xs, [LOWPASS])
    assert _peek(state, OUT0, 32) == want
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [_w(IN0) + 32]  # INIDX + W
    assert _peek(state, TCB0 + 4 * 7, 1, "I") == [_w(OUT0) + 32]  # OUTIDX + W
    stat = _dm_read(state, iir.IIR_DMASTAT, 4).value
    assert stat & iir.DMASTAT_ACDONE and stat & iir.DMASTAT_WDONE
    assert _pending(state)  # all channels done: SID 120


def test_two_cascaded_biquads_and_two_channels_with_save_state_continue_across_runs():
    state = _state()
    xs0 = [float(k % 5) - 2.0 for k in range(16)]
    xs1 = [0.1 * k for k in range(16)]
    _poke(state, IN0, xs0 + xs0)
    _poke(state, IN1, xs1 + xs1)
    sec0, sec1 = [LOWPASS, PEAK], [PEAK]
    _biquads(state, COEF0, sec0)
    _biquads(state, COEF1, sec1)
    _legacy_tcb(state, TCB0, TCB1, _ctl2(2, 16), COEF0, IN0, OUT0)
    _legacy_tcb(state, TCB1, TCB0, _ctl2(1, 16), COEF1, IN1, OUT1)
    ctl1 = iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_SS | (1 << iir.CTL1_CH_SHIFT)
    _start(state, ctl1, _w(TCB0) + 12)
    _dm_write(state, iir.IIR_CTL1, 4, Const(0))  # off, then a second run
    _dm_write(state, iir.IIR_CHNPTR, 4, Const(_w(TCB0) + 12))
    _dm_write(state, iir.IIR_CTL1, 4, Const(ctl1))
    want0, st0 = _reference(xs0 + xs0, sec0)
    want1, st1 = _reference(xs1 + xs1, sec1)
    assert _peek(state, OUT0, 32) == want0  # one 32-sample run's worth
    assert _peek(state, OUT1, 32) == want1
    assert _peek(state, COEF0 + 4 * 5, 2) == st0[0]  # Dk2, Dk1 saved after A0..B2
    assert _peek(state, COEF0 + 4 * 12, 2) == st0[1]


def test_without_save_state_a_second_run_starts_from_the_stored_states_again():
    state = _state()
    xs = [1.0] * 8
    _poke(state, IN0, xs + xs)
    _biquads(state, COEF0, [LOWPASS])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 8), COEF0, IN0, OUT0)
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN, _w(TCB0) + 12)
    _dm_write(state, iir.IIR_CTL1, 4, Const(0))
    _dm_write(state, iir.IIR_CHNPTR, 4, Const(_w(TCB0) + 12))
    _dm_write(state, iir.IIR_CTL1, 4, Const(iir.CTL1_EN | iir.CTL1_DMAEN))
    first, _ = _reference(xs, [LOWPASS])
    assert _peek(state, OUT0 + 32, 8) == first  # the same as the first window


def test_channel_complete_interrupts_and_a_circular_input():
    state = _state()
    _poke(state, IN0, [1.0, 2.0, 3.0, 4.0])
    _biquads(state, COEF0, [(1.0, 0, 0, 0, 0, 0, 0)])  # a wire
    # index at the third word of a 4-word circular buffer: 3, 4, then 1, 2, 3, 4
    _legacy_tcb(
        state, TCB0, TCB0, _ctl2(1, 6), COEF0, IN0 + 8, OUT0, inlen=4, inbase=IN0
    )
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_CCINTR, _w(TCB0) + 12)
    assert _peek(state, OUT0, 6) == [3.0, 4.0, 1.0, 2.0, 3.0, 4.0]
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [
        _w(IN0) + 0
    ]  # wrapped: + 6 in a 4-word ring
    assert _pending(state)


def test_acm_runs_a_chain_to_its_null_pointer_and_honours_the_interrupt_mask():
    state = _state()
    xs = [float(k) for k in range(8)]
    _poke(state, IN0, xs)
    _poke(state, IN1, xs)
    _biquads(state, COEF0, [LOWPASS])
    _biquads(state, COEF1, [PEAK])
    _acm_tcb(state, TCB0, TCB1, _ctl2(1, 8, imask=True), COEF0, IN0, OUT0)
    _acm_tcb(
        state, TCB1, 0, _ctl2(1, 8, imask=True), COEF1, IN1, OUT1, sgctl=iir.SGCTL_SS
    )
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_ACM, _w(TCB0) + 15)
    assert _peek(state, OUT0, 8) == _reference(xs, [LOWPASS])[0]
    assert _peek(state, OUT1, 8) == _reference(xs, [PEAK])[0]
    assert _peek(state, TCB0 + 4 * 14, 1, "I") == [0]  # ACM: INIDX 0, OUTIDX all ones
    assert _peek(state, TCB0 + 4 * 10, 1, "I") == [0xFFFFFFFF]
    assert (
        _peek(state, COEF1 + 4 * 5, 2) == _reference(xs, [PEAK])[1][0]
    )  # SGCTL.SS: channel 2 only
    assert _peek(state, COEF0 + 4 * 5, 2) == [0.0, 0.0]
    assert not _pending(state)  # IMASK on both
    assert _dm_read(state, iir.IIR_DMASTAT, 4).value & iir.DMASTAT_ACDONE


def _acm_blocks(sgctl: int, blocks: int = 3, window: int = 8):
    """Two ACM channels run once a block; between runs the core re-arms each TCB's
    INIDX and OUTIDX to the next block's buffers, which the accelerator set to 0 and
    0xFFFFFFFF after the last window (HWR chapter 39, ACM)."""
    state = _state()
    xs0 = [float(k % 7) - 3.0 for k in range(window * blocks)]
    xs1 = [0.05 * k for k in range(window * blocks)]
    _poke(state, IN0, xs0)
    _poke(state, IN1, xs1)
    _biquads(state, COEF0, [LOWPASS, PEAK])
    _biquads(state, COEF1, [PEAK])
    ctl2 = (_ctl2(2, window, imask=True), _ctl2(1, window, imask=True))
    _acm_tcb(state, TCB0, TCB1, ctl2[0], COEF0, IN0, OUT0, sgctl=sgctl)
    _acm_tcb(state, TCB1, 0, ctl2[1], COEF1, IN1, OUT1, sgctl=sgctl)
    ctl1 = iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_ACM
    for b in range(blocks):
        if b:
            for tcb, inp, out in ((TCB0, IN0, OUT0), (TCB1, IN1, OUT1)):
                assert _peek(state, tcb + 4 * 14, 1, "I") == [0]
                assert _peek(state, tcb + 4 * 10, 1, "I") == [0xFFFFFFFF]
                _poke(state, tcb + 4 * 14, [_w(inp + 4 * window * b)])
                _poke(state, tcb + 4 * 10, [_w(out + 4 * window * b)])
            _dm_write(state, iir.IIR_CTL1, 4, Const(0))
        _start(state, ctl1, _w(TCB0) + 15)
    return state, xs0, xs1


def test_acm_channels_with_save_state_continue_block_after_block_once_re_armed():
    blocks, window = 3, 8
    state, xs0, xs1 = _acm_blocks(iir.SGCTL_SS, blocks, window)
    assert _peek(state, OUT0, blocks * window) == _reference(xs0, [LOWPASS, PEAK])[0]
    assert _peek(state, OUT1, blocks * window) == _reference(xs1, [PEAK])[0]


def test_acm_channels_without_save_state_restart_every_block():
    blocks, window = 3, 8
    state, xs0, _ = _acm_blocks(0, blocks, window)
    for b in range(blocks):  # each block from the stored (zero) states again
        part = xs0[b * window : (b + 1) * window]
        assert (
            _peek(state, OUT0 + 4 * window * b, window)
            == _reference(part, [LOWPASS, PEAK])[0]
        )
    assert _peek(state, OUT0, blocks * window) != _reference(xs0, [LOWPASS, PEAK])[0]


def test_reads_leave_the_done_bits_set():
    state = _state()
    _biquads(state, COEF0, [LOWPASS])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 1), COEF0, IN0, OUT0)
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN, _w(TCB0) + 12)
    for _ in range(2):  # the model clears them at the next run
        assert (
            _dm_read(state, iir.IIR_DMASTAT, 4).value & iir.DMASTAT_STICKY
            == iir.DMASTAT_STICKY
        )


def test_writing_ctl1_again_while_enabled_does_not_rerun():
    state = _state()
    _poke(state, IN0, [1.0] * 4)
    _biquads(state, COEF0, [(2.0, 0, 0, 0, 0, 0, 0)])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 4), COEF0, IN0, OUT0)
    _start(state, iir.CTL1_EN | iir.CTL1_DMAEN, _w(TCB0) + 12)
    _dm_write(
        state, iir.IIR_CTL1, 4, Const(iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_CCINTR)
    )
    assert _peek(state, TCB0 + 4 * 11, 1, "I") == [_w(IN0) + 4]  # one run only


@pytest.mark.parametrize("ctl1", [iir.CTL1_FORTYBIT, iir.CTL1_RND, iir.CTL1_CAI])
def test_unmodeled_settings_refuse_instead_of_guessing(ctl1):
    state = _state()
    _biquads(state, COEF0, [LOWPASS])
    _legacy_tcb(state, TCB0, TCB0, _ctl2(1, 1), COEF0, IN0, OUT0)
    with pytest.raises(ValueError):
        _start(state, iir.CTL1_EN | iir.CTL1_DMAEN | ctl1, _w(TCB0) + 12)


def test_registers_read_back_and_status_is_read_only():
    state = _state()
    assert _dm_write(state, iir.IIR_SCTL1, 4, Const(0x1234))
    assert _dm_read(state, iir.IIR_SCTL1, 4) == Const(0x1234)
    assert _dm_write(state, iir.IIR_DMASTAT, 4, Const(0xFFFF))
    assert _dm_read(state, iir.IIR_DMASTAT, 4) == Const(0)


def _native(state: State):
    import os

    from tools.sharc_dev import sharc_transpile_run as nr

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
    from tools.sharc_dev import sharc_diff as sd

    state = _state()
    xs = [float(k % 7) - 3.0 for k in range(24)]
    _poke(state, IN0, xs)
    _poke(state, IN1, [0.5 * x for x in xs])
    _biquads(state, COEF0, [LOWPASS, PEAK])
    _biquads(state, COEF1, [PEAK])
    if mode == "legacy":
        _legacy_tcb(state, TCB0, TCB1, _ctl2(2, 24), COEF0, IN0, OUT0)
        _legacy_tcb(
            state, TCB1, TCB0, _ctl2(1, 24), COEF1, IN1, OUT1, inlen=8, inbase=IN1
        )
        ctl1, pointer = (
            iir.CTL1_EN
            | iir.CTL1_DMAEN
            | iir.CTL1_SS
            | iir.CTL1_CCINTR
            | (1 << iir.CTL1_CH_SHIFT),
            _w(TCB0) + 12,
        )
    else:
        _acm_tcb(state, TCB0, TCB1, _ctl2(2, 24), COEF0, IN0, OUT0, sgctl=iir.SGCTL_SS)
        _acm_tcb(state, TCB1, 0, _ctl2(1, 24, imask=True), COEF1, IN1, OUT1)
        ctl1, pointer = iir.CTL1_EN | iir.CTL1_DMAEN | iir.CTL1_ACM, _w(TCB0) + 15
    core = _native(state)
    _both(state, core, iir.IIR_CHNPTR, pointer)
    _both(state, core, iir.IIR_CTL1, ctl1)
    assert (
        core.peek(OUT0 + 4 * 23, 4)
        == struct.unpack("<I", struct.pack("<f", _peek(state, OUT0 + 4 * 23, 1)[0]))[0]
    )
    assert core.peek(iir.IIR_DMASTAT, 4) == _dm_read(state, iir.IIR_DMASTAT, 4).value
    assert sd.compare_states(sd.export_state(state), core.export_state()) == []
