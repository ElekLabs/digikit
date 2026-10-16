"""Opt-in IIR accelerator (IIR0): TCB chains, biquad windows, status and interrupt.

Enabled with the other system peripherals by State.peripheral_model; reached
from periph._periph_read/_periph_write, so it is a hand-written twin of
native/sharc-rt/src/rt/iir.rs (tools/sharc_transpile.py BOUNDARY), like
periph.py. Public source: ADSP-2156x HWR chapter 39 (IIR Accelerator) and
the IIR0 register addresses in its appendix (Table A-62).

What it models: legacy mode (IIR_CTL1.CH + 1 channels, TCBs of 13 words with
IIR_CHNPTR at TCB + 12) and Auto Configuration Mode (TCBs of 16 words with
IIR_CHNPTR at TCB + 15, until a null chain pointer); 32-bit IEEE floats; the
transposed direct form II biquad of HWR Figure 39-2, up to 64 a channel, a
window of 1 to 1024 samples; circular input, output and coefficient buffers
(index, modifier, length, base, in words); the index write-back to the TCB
(legacy: + W, ACM: 0 and 0xffffffff); Save State (IIR_CTL1.SS, IIR_SGCTL.SS)
writing each biquad's Dk2, Dk1 back after its coefficients; IIR_DMASTAT's
WDONE and ACDONE and the IIR0_DMA source (SID 120)
through the SEC: per channel with IIR_CTL1.CCINTR (legacy) or per channel
unless IIR_CTL2.IMASK (ACM), else once when all channels are done (legacy).

A run starts when IIR_CTL1 goes from disabled to EN | DMAEN, and completes at
once: the core sees the finished state on its next access. WDONE and ACDONE
are sticky; the HWR clears them on a read of IIR_DMASTAT, and here they clear
when the next run starts instead (core reads have no side effects in either
twin), which code that polls after starting a run cannot tell apart. DMA and MAC timing,
burst reads, the debug registers, IIR_MACSTAT, triggers and priority are not
modeled. Unsupported settings (40-bit mode, truncation rounding, Channel Auto
Iterate, HALT, a trigger wait) raise ValueError instead of guessing.

Unverified against silicon: the order of the float operations inside a biquad.
The HWR gives the signal flow, not the order; this model follows the figure
term by term, each product and each sum rounded to float32 (round to nearest
even), denormals kept:
    y   = A0 x + Dk1
    Dk1 = (A1 x + B1 y) + Dk2
    Dk2 = A2 x + B2 y
with the HWR's names (Ak = b, Bk = -a, stored A0, A1, B1, A2, B2, Dk2, Dk1).
"""

from __future__ import annotations

from .floats import _f32_from_bits, _float32_bits
from .state import State
from .values import Const

IIR0 = 0x310C4000
IIR_CTL1 = IIR0 + 0x00
IIR_DMASTAT = IIR0 + 0x04
IIR_MACSTAT = IIR0 + 0x08
IIR_CTL2 = IIR0 + 0x40
IIR_INIDX = IIR0 + 0x44
IIR_INMOD = IIR0 + 0x48
IIR_INLEN = IIR0 + 0x4C
IIR_INBASE = IIR0 + 0x50
IIR_OUTIDX = IIR0 + 0x54
IIR_OUTMOD = IIR0 + 0x58
IIR_OUTLEN = IIR0 + 0x5C
IIR_OUTBASE = IIR0 + 0x60
IIR_COEFIDX = IIR0 + 0x64
IIR_COEFMOD = IIR0 + 0x68
IIR_COEFLEN = IIR0 + 0x6C
IIR_CHNPTR = IIR0 + 0x70
IIR_SCTL1 = IIR0 + 0x74
IIR_SCTL2 = IIR0 + 0x78
IIR_SGCTL = IIR0 + 0x7C
IIR_END = IIR0 + 0x80
SID_IIR0_DMA = 120

CTL1_EN = 1 << 0
CTL1_CH_SHIFT = 1
CTL1_CH_MASK = 0x1F
CTL1_DMAEN = 1 << 8
CTL1_CAI = 1 << 9
CTL1_SS = 1 << 10
CTL1_CCINTR = 1 << 11
CTL1_FORTYBIT = 1 << 12
CTL1_RND = 1 << 14
CTL1_HALT = 1 << 30
CTL1_ACM = 1 << 31
CTL2_BIQUADS_MASK = 0x3F
CTL2_WINDOW_SHIFT = 14
CTL2_WINDOW_MASK = 0x3FF
CTL2_IMASK = 1 << 24
CTL2_TWAIT = 1 << 28
SGCTL_SS = 1 << 10          # the same field positions as IIR_CTL1's, per channel
SGCTL_FORTYBIT = 1 << 12
SGCTL_RND = 1 << 14
DMASTAT_WDONE = 1 << 5
DMASTAT_ACDONE = 1 << 6
DMASTAT_STICKY = DMASTAT_WDONE | DMASTAT_ACDONE
MAX_CHANNELS = 32
MAX_ACM_CHANNELS = 4096      # a guard against a chain that never ends
WORDS_PER_BIQUAD = 7

# TCB words in load order: the chain pointer register points at the last
# word, and the accelerator reads down from it (HWR Tables 39-5 and 39-6).
LEGACY_TCB = (
    IIR_CTL2, IIR_INIDX, IIR_INMOD, IIR_INLEN, IIR_INBASE, IIR_OUTIDX,
    IIR_OUTMOD, IIR_OUTLEN, IIR_OUTBASE, IIR_COEFIDX, IIR_COEFMOD,
    IIR_COEFLEN, IIR_CHNPTR,
)
ACM_TCB = LEGACY_TCB[:12] + (IIR_SGCTL, IIR_SCTL2, IIR_SCTL1, IIR_CHNPTR)

def _f32(bits: int) -> float:
    return _f32_from_bits(bits & 0xFFFFFFFF)


def _bits(value: float) -> int:
    """VALUE's float32 pattern (to nearest even), +-infinity on overflow."""
    return _float32_bits(value)[0]


def _r(value: float) -> float:
    """VALUE rounded to float32."""
    return _f32(_bits(value))


def _mmr(state: State, address: int) -> int:
    value = state.mmrs.get(address)
    if value is None:
        return 0
    if not isinstance(value, Const):
        raise ValueError("unknown IIR register %#x" % address)
    return value.value & 0xFFFFFFFF


def _set(state: State, address: int, value: int) -> None:
    state.mmrs[address] = Const(value & 0xFFFFFFFF)


def _word_read(state: State, word: int) -> int:
    """The 32-bit word at word address WORD (byte address WORD << 2)."""
    address = (word & 0x3FFFFFFF) << 2
    backing = state.concrete
    raw = 0
    for k in range(4):
        here = address + k
        if here in state.overlay:
            byte = state.overlay[here]
        else:
            data = backing.read(here, 1) if backing is not None else None
            if data is None:
                raise ValueError("IIR DMA read outside known memory %#x" % here)
            byte = data[0]
        raw |= byte << (8 * k)
    return raw


def _word_write(state: State, word: int, value: int) -> None:
    address = (word & 0x3FFFFFFF) << 2
    for k in range(4):
        state.overlay[address + k] = (value >> (8 * k)) & 0xFF


def _step(index: int, modifier: int, length: int, base: int) -> int:
    """The next index of a circular buffer (in words); length 0 is linear."""
    modifier &= 0xFFFF
    if modifier & 0x8000:
        modifier -= 0x10000
    nxt = index + modifier
    if length:
        if nxt >= base + length:
            nxt -= length
        elif nxt < base:
            nxt += length
    return nxt & 0x3FFFFFFF


def _load_tcb(state: State, pointer: int, layout) -> None:
    """The TCB at POINTER into the registers, read down from its last word."""
    for k, reg in enumerate(layout):
        _set(state, reg, _word_read(state, pointer - k))


def _coefficients(state: State) -> list[list[float]]:
    """The channel's biquads (A0, A1, B1, A2, B2, Dk2, Dk1) from its buffer, with
    the word address each Dk2 came from (index 7) for Save State."""
    count = (_mmr(state, IIR_CTL2) & CTL2_BIQUADS_MASK) + 1
    index = _mmr(state, IIR_COEFIDX) & 0x3FFFFFFF
    modifier, length = _mmr(state, IIR_COEFMOD), _mmr(state, IIR_COEFLEN) & 0xFFFF
    base = index
    out = []
    for _ in range(count):
        words, places = [], []
        for _ in range(WORDS_PER_BIQUAD):
            places.append(index)
            words.append(_f32(_word_read(state, index)))
            index = _step(index, modifier, length, base)
        out.append(words + [places[5], places[6]])
    return out


def _save_state(state: State, biquads: list[list[float]]) -> None:
    for bq in biquads:
        _word_write(state, bq[7], _bits(bq[5]))
        _word_write(state, bq[8], _bits(bq[6]))


def _window(state: State, biquads: list[list[float]], layout, pointer: int, acm: bool) -> None:
    """One window of the channel: W samples through every biquad, then the
    index write-back to the TCB in memory."""
    window = ((_mmr(state, IIR_CTL2) >> CTL2_WINDOW_SHIFT) & CTL2_WINDOW_MASK) + 1
    inidx, outidx = _mmr(state, IIR_INIDX) & 0x3FFFFFFF, _mmr(state, IIR_OUTIDX) & 0x3FFFFFFF
    for _ in range(window):
        x = _f32(_word_read(state, inidx))
        for bq in biquads:
            a0, a1, b1, a2, b2, d2, d1 = bq[:7]
            y = _r(_r(a0 * x) + d1)
            bq[6] = _r(_r(_r(a1 * x) + _r(b1 * y)) + d2)
            bq[5] = _r(_r(a2 * x) + _r(b2 * y))
            x = y
        _word_write(state, outidx, _bits(x))
        inidx = _step(inidx, _mmr(state, IIR_INMOD), _mmr(state, IIR_INLEN) & 0xFFFF, _mmr(state, IIR_INBASE) & 0x3FFFFFFF)
        outidx = _step(outidx, _mmr(state, IIR_OUTMOD), _mmr(state, IIR_OUTLEN) & 0xFFFF, _mmr(state, IIR_OUTBASE) & 0x3FFFFFFF)
    if acm:
        inidx, outidx = 0x00000000, 0xFFFFFFFF
    _word_write(state, pointer - layout.index(IIR_INIDX), inidx)
    _word_write(state, pointer - layout.index(IIR_OUTIDX), outidx)
    _set(state, IIR_INIDX, inidx)
    _set(state, IIR_OUTIDX, outidx)


def _channel_done(state: State, interrupt: bool) -> int:
    """WDONE; 1 when the channel raises IIR0_DMA."""
    _set(state, IIR_DMASTAT, _mmr(state, IIR_DMASTAT) | DMASTAT_WDONE)
    return 1 if interrupt else 0


def _run(state: State) -> int:
    """The whole chain; how many times it raises IIR0_DMA (periph raises them)."""
    ctl1 = _mmr(state, IIR_CTL1)
    if ctl1 & (CTL1_FORTYBIT | CTL1_RND):
        raise ValueError("only 32-bit round-to-nearest IIR processing is modeled")
    if ctl1 & (CTL1_CAI | CTL1_HALT):
        raise ValueError("IIR Channel Auto Iterate and HALT are not modeled")
    _set(state, IIR_DMASTAT, _mmr(state, IIR_DMASTAT) & ~DMASTAT_STICKY)
    pointer = _mmr(state, IIR_CHNPTR) & 0x3FFFFFFF
    raises = 0
    if ctl1 & CTL1_ACM:
        for _ in range(MAX_ACM_CHANNELS):
            if not pointer:
                break
            _load_tcb(state, pointer, ACM_TCB)
            if _mmr(state, IIR_SGCTL) & (SGCTL_FORTYBIT | SGCTL_RND):
                raise ValueError("only 32-bit round-to-nearest IIR processing is modeled")
            if _mmr(state, IIR_CTL2) & CTL2_TWAIT:
                raise ValueError("an IIR trigger wait is not modeled")
            biquads = _coefficients(state)
            _window(state, biquads, ACM_TCB, pointer, True)
            if _mmr(state, IIR_SGCTL) & SGCTL_SS:
                _save_state(state, biquads)
            raises += _channel_done(state, not _mmr(state, IIR_CTL2) & CTL2_IMASK)
            pointer = _mmr(state, IIR_CHNPTR) & 0x3FFFFFFF
        else:
            raise ValueError("an IIR ACM chain longer than %d TCBs" % MAX_ACM_CHANNELS)
        _set(state, IIR_DMASTAT, _mmr(state, IIR_DMASTAT) | DMASTAT_ACDONE)
        return raises
    channels = ((ctl1 >> CTL1_CH_SHIFT) & CTL1_CH_MASK) + 1
    first = pointer
    loaded = []
    for _ in range(channels):                       # every channel's coefficients first
        _load_tcb(state, pointer, LEGACY_TCB)
        loaded.append(_coefficients(state))
        pointer = _mmr(state, IIR_CHNPTR) & 0x3FFFFFFF
    pointer = first
    each = bool(ctl1 & CTL1_CCINTR)
    for c in range(channels):
        _load_tcb(state, pointer, LEGACY_TCB)
        _window(state, loaded[c], LEGACY_TCB, pointer, False)
        raises += _channel_done(state, each)
        pointer = _mmr(state, IIR_CHNPTR) & 0x3FFFFFFF
    if ctl1 & CTL1_SS:
        for biquads in loaded:
            _save_state(state, biquads)
    _set(state, IIR_DMASTAT, _mmr(state, IIR_DMASTAT) | DMASTAT_ACDONE)
    return raises if each else 1


def _iir_read(state: State, address: int) -> Const | None:
    """An IIR register's value; None outside IIR0."""
    if not IIR0 <= address < IIR_END:
        return None
    return Const(_mmr(state, address))


def _iir_write(state: State, address: int, value: int) -> int:
    """An IIR register write: 0 outside IIR0, else 1 + how many times the write
    raises IIR0_DMA (periph raises them). IIR_CTL1 from disabled to EN | DMAEN
    runs the chain; IIR_DMASTAT and IIR_MACSTAT are read-only."""
    if not IIR0 <= address < IIR_END:
        return 0
    if address in (IIR_DMASTAT, IIR_MACSTAT):
        return 1
    old = _mmr(state, IIR_CTL1)
    _set(state, address, value)
    want = CTL1_EN | CTL1_DMAEN
    if address == IIR_CTL1 and value & want == want and old & want != want:
        return 1 + _run(state)
    return 1
