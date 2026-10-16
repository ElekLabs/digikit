"""Opt-in FIR accelerator (FIR0): TCB chains, single-rate windows, status and interrupt.

Enabled with the other system peripherals by State.peripheral_model; reached
from periph._periph_read/_periph_write, so it is a hand-written twin of
native/sharc-rt/src/rt/fir.rs (tools/sharc_transpile.py BOUNDARY), like
iir.py, whose DMA helpers it shares. Public source: ADSP-2156x HWR chapter 38
(FIR Accelerator) and the FIR0 register addresses in its appendix (Table A-60).

What it models: legacy mode (FIR_CTL1.CH + 1 channels, TCBs of 13 words with
FIR_CHNPTR at TCB + 12) and Auto Configuration Mode (TCBs of 16 words with
FIR_CHNPTR at TCB + 15, until a null chain pointer); 32-bit IEEE floats;
single-rate filters of 1 to 1024 taps (one iteration) and windows of 1 to
1024 samples; coefficients stored C[N-1] first, read with FIR_COEFMOD and never
circular; a circular input buffer (FIR_INIDX, FIR_INMOD, FIR_INCNT as the
length, FIR_INBASE, in words) and output buffer, the same; the index
write-back to the TCB (legacy: + W, ACM: 0 and 0xffffffff); FIR_COEFCNT down
by the tap count; FIR_DMASTAT's WDONE and ACDONE and the FIR0_DMA source
(SID 118) through the SEC: per channel with FIR_CTL1.CCINTR (legacy) or per
channel unless FIR_CTL2.IMASK (ACM), else once when all channels are done
(legacy).

A run starts when FIR_CTL1 goes from disabled to EN | DMAEN, and completes at
once, as in iir.py, with the same difference from the HWR: WDONE and ACDONE
clear when the next run starts, not on a read of FIR_DMASTAT. DMA and MAC
timing, bursts, the debug registers, FIR_MACSTAT and FIR0_STAT, triggers and
priority are not modeled. Unsupported settings (fixed point, truncation
rounding, sample rate conversion, more than 1024 taps, Channel Auto Iterate,
HALT, a trigger wait) raise ValueError instead of guessing.

Unverified against silicon: the order of the float operations. The HWR says
four MACs take the taps four at a time and their sums are added at the end;
this model gives MAC k the taps k, k + 4, k + 8, ... in memory order (the
oldest sample first), rounds each product and each sum to float32 (round to
nearest even, denormals kept), starts each sum with its first product, and
adds the sums in order:
    acc[k] = sum over i = k mod 4 of C[N-1-i] x[n-(N-1)+i]
    y      = ((acc[0] + acc[1]) + acc[2]) + acc[3]
(fewer than four taps: only the sums that exist).
"""

from __future__ import annotations

from .iir import _bits, _f32, _load_tcb, _r, _set, _step, _word_read, _word_write
from .state import State
from .values import Const

FIR0 = 0x310C3000
FIR_CTL1 = FIR0 + 0x00
FIR_DMASTAT = FIR0 + 0x04
FIR_MACSTAT = FIR0 + 0x08
FIR_CTL2 = FIR0 + 0x40
FIR_INIDX = FIR0 + 0x44
FIR_INMOD = FIR0 + 0x48
FIR_INCNT = FIR0 + 0x4C
FIR_INBASE = FIR0 + 0x50
FIR_OUTIDX = FIR0 + 0x54
FIR_OUTMOD = FIR0 + 0x58
FIR_OUTCNT = FIR0 + 0x5C
FIR_OUTBASE = FIR0 + 0x60
FIR_COEFIDX = FIR0 + 0x64
FIR_COEFMOD = FIR0 + 0x68
FIR_COEFCNT = FIR0 + 0x6C
FIR_CHNPTR = FIR0 + 0x70
FIR_SCTL1 = FIR0 + 0x74
FIR_SCTL2 = FIR0 + 0x78
FIR_SGCTL = FIR0 + 0x7C
FIR_END = FIR0 + 0x80
SID_FIR0_DMA = 118

CTL1_EN = 1 << 0
CTL1_CH_SHIFT = 1
CTL1_CH_MASK = 0x1F
CTL1_DMAEN = 1 << 8
CTL1_CAI = 1 << 9
CTL1_CCINTR = 1 << 11
CTL1_FXD = 1 << 12
CTL1_RND = 1 << 14
CTL1_HALT = 1 << 30
CTL1_ACM = 1 << 31
CTL2_TAPLEN_MASK = 0xFFF
CTL2_WINDOW_SHIFT = 14
CTL2_WINDOW_MASK = 0x3FF
CTL2_IMASK = 1 << 24
CTL2_TWAIT = 1 << 28
CTL2_SRCEN = 1 << 29
SGCTL_FXD = 1 << 12          # the same field positions as FIR_CTL1's, per channel
SGCTL_RND = 1 << 14
DMASTAT_WDONE = 1 << 5
DMASTAT_ACDONE = 1 << 6
DMASTAT_STICKY = DMASTAT_WDONE | DMASTAT_ACDONE
MAX_TAPS = 1024              # the hard filter length: more takes multi-iteration
MACS = 4
MAX_ACM_CHANNELS = 4096      # a guard against a chain that never ends

# TCB words in load order, down from the chain pointer (HWR Tables 38-6 and 38-7).
LEGACY_TCB = (
    FIR_CTL2, FIR_INIDX, FIR_INMOD, FIR_INCNT, FIR_INBASE, FIR_OUTIDX,
    FIR_OUTMOD, FIR_OUTCNT, FIR_OUTBASE, FIR_COEFIDX, FIR_COEFMOD,
    FIR_COEFCNT, FIR_CHNPTR,
)
ACM_TCB = LEGACY_TCB[:12] + (FIR_SGCTL, FIR_SCTL2, FIR_SCTL1, FIR_CHNPTR)


def _mmr(state: State, address: int) -> int:
    value = state.mmrs.get(address)
    if value is None:
        return 0
    if not isinstance(value, Const):
        raise ValueError("unknown FIR register %#x" % address)
    return value.value & 0xFFFFFFFF


def _coefficients(state: State, taps: int) -> list[float]:
    """The channel's TAPS coefficients in memory order (C[N-1] first); FIR_COEFCNT
    counts down by TAPS."""
    index = _mmr(state, FIR_COEFIDX) & 0x3FFFFFFF
    modifier = _mmr(state, FIR_COEFMOD)
    out = []
    for _ in range(taps):
        out.append(_f32(_word_read(state, index)))
        index = _step(index, modifier, 0, 0)
    _set(state, FIR_COEFCNT, (_mmr(state, FIR_COEFCNT) - taps) & 0xFFFF)
    return out


def _window(state: State, coefs: list[float], layout, pointer: int, acm: bool) -> None:
    """One window of the channel: W outputs from N - 1 + W inputs, then the
    index write-back to the TCB in memory."""
    window = ((_mmr(state, FIR_CTL2) >> CTL2_WINDOW_SHIFT) & CTL2_WINDOW_MASK) + 1
    taps = len(coefs)
    inmod, inlen = _mmr(state, FIR_INMOD), _mmr(state, FIR_INCNT) & 0xFFFF
    inbase = _mmr(state, FIR_INBASE) & 0x3FFFFFFF
    inidx = _mmr(state, FIR_INIDX) & 0x3FFFFFFF
    outidx = _mmr(state, FIR_OUTIDX) & 0x3FFFFFFF
    xs, at, back = [], inidx, inidx
    for k in range(taps - 1 + window):
        xs.append(_f32(_word_read(state, at)))
        at = _step(at, inmod, inlen, inbase)
        if k == window - 1:
            back = at                                   # FIR_INIDX + W
    for j in range(window):
        acc = []
        for i in range(taps):
            p = _r(coefs[i] * xs[j + i])
            if i < MACS:
                acc.append(p)
            else:
                acc[i % MACS] = _r(acc[i % MACS] + p)
        y = acc[0]
        for k in range(1, len(acc)):
            y = _r(y + acc[k])
        _word_write(state, outidx, _bits(y))
        outidx = _step(outidx, _mmr(state, FIR_OUTMOD), _mmr(state, FIR_OUTCNT) & 0xFFFF,
                       _mmr(state, FIR_OUTBASE) & 0x3FFFFFFF)
    if acm:
        back, outidx = 0x00000000, 0xFFFFFFFF
    _word_write(state, pointer - layout.index(FIR_INIDX), back)
    _word_write(state, pointer - layout.index(FIR_OUTIDX), outidx)
    _set(state, FIR_INIDX, back)
    _set(state, FIR_OUTIDX, outidx)


def _channel(state: State, layout, pointer: int, acm: bool) -> None:
    """The channel whose TCB is loaded: its settings checked, its coefficients
    and one window."""
    ctl2 = _mmr(state, FIR_CTL2)
    if ctl2 & CTL2_SRCEN:
        raise ValueError("FIR sample rate conversion is not modeled")
    if acm and ctl2 & CTL2_TWAIT:
        raise ValueError("a FIR trigger wait is not modeled")
    if acm and _mmr(state, FIR_SGCTL) & (SGCTL_FXD | SGCTL_RND):
        raise ValueError("only floating-point round-to-nearest FIR processing is modeled")
    taps = (ctl2 & CTL2_TAPLEN_MASK) + 1
    if taps > MAX_TAPS:
        raise ValueError("FIR multi-iteration (more than %d taps) is not modeled" % MAX_TAPS)
    _window(state, _coefficients(state, taps), layout, pointer, acm)


def _channel_done(state: State, interrupt: bool) -> int:
    """WDONE; 1 when the channel raises FIR0_DMA."""
    _set(state, FIR_DMASTAT, _mmr(state, FIR_DMASTAT) | DMASTAT_WDONE)
    return 1 if interrupt else 0


def _run(state: State) -> int:
    """The whole chain; how many times it raises FIR0_DMA (periph raises them)."""
    ctl1 = _mmr(state, FIR_CTL1)
    if ctl1 & (CTL1_FXD | CTL1_RND):
        raise ValueError("only floating-point round-to-nearest FIR processing is modeled")
    if ctl1 & (CTL1_CAI | CTL1_HALT):
        raise ValueError("FIR Channel Auto Iterate and HALT are not modeled")
    _set(state, FIR_DMASTAT, _mmr(state, FIR_DMASTAT) & ~DMASTAT_STICKY)
    pointer = _mmr(state, FIR_CHNPTR) & 0x3FFFFFFF
    raises = 0
    if ctl1 & CTL1_ACM:
        for _ in range(MAX_ACM_CHANNELS):
            if not pointer:
                break
            _load_tcb(state, pointer, ACM_TCB)
            _channel(state, ACM_TCB, pointer, True)
            raises += _channel_done(state, not _mmr(state, FIR_CTL2) & CTL2_IMASK)
            pointer = _mmr(state, FIR_CHNPTR) & 0x3FFFFFFF
        else:
            raise ValueError("a FIR ACM chain longer than %d TCBs" % MAX_ACM_CHANNELS)
        _set(state, FIR_DMASTAT, _mmr(state, FIR_DMASTAT) | DMASTAT_ACDONE)
        return raises
    channels = ((ctl1 >> CTL1_CH_SHIFT) & CTL1_CH_MASK) + 1
    each = bool(ctl1 & CTL1_CCINTR)
    for _ in range(channels):
        _load_tcb(state, pointer, LEGACY_TCB)
        _channel(state, LEGACY_TCB, pointer, False)
        raises += _channel_done(state, each)
        pointer = _mmr(state, FIR_CHNPTR) & 0x3FFFFFFF
    _set(state, FIR_DMASTAT, _mmr(state, FIR_DMASTAT) | DMASTAT_ACDONE)
    return raises if each else 1


def _fir_read(state: State, address: int) -> Const | None:
    """A FIR register's value; None outside FIR0."""
    if not FIR0 <= address < FIR_END:
        return None
    return Const(_mmr(state, address))


def _fir_write(state: State, address: int, value: int) -> int:
    """A FIR register write: 0 outside FIR0, else 1 + how many times the write
    raises FIR0_DMA (periph raises them). FIR_CTL1 from disabled to EN | DMAEN
    runs the chain; FIR_DMASTAT and FIR_MACSTAT are read-only."""
    if not FIR0 <= address < FIR_END:
        return 0
    if address in (FIR_DMASTAT, FIR_MACSTAT):
        return 1
    old = _mmr(state, FIR_CTL1)
    _set(state, address, value)
    want = CTL1_EN | CTL1_DMAEN
    if address == FIR_CTL1 and value & want == want and old & want != want:
        return 1 + _run(state)
    return 1
