//! The eDMA controller (MCF5441XRM chapter 19): shared register layout used
//! by every channel model in this crate (this file's own [`TxChannel`],
//! `dspi::Dspi2Link`, `ssi::Ssi0Dma`), plus the console-UART8-TX channel
//! (35) itself -- a direct port of `unicorn-reference-final:emu/edma.py`'s `TxChannel`.
//!
//! ## Register layout
//!
//! One 16 KiB slot at `0xFC04_4000` holds both the controller's own
//! registers (`0xFC04_4000`-`0xFC04_401F`, RM Table 19-3) and all 64
//! per-channel Transfer Control Descriptors (`TCDn` at
//! `0xFC04_5000 + n*0x20`, RM 19.4.17 p.384) -- the TCD area is `0x1000`
//! bytes into the same slot. `EdmaBank` (this file) owns that one
//! [`RegFile`]; every channel model that touches a TCD (this file's
//! `TxChannel`, `dspi::Dspi2Link`, `ssi::Ssi0Dma`) is handed `&mut RegFile`
//! rather than owning its own copy, exactly as they all read/write the same
//! guest memory region in Python (`self.m.uc.mem_read/mem_write`).
//!
//! `EDMA_SERQ`/`EDMA_CERQ` (RM 19.4.5/19.4.6, p.374-375) are the only two
//! eDMA control registers any model in this codebase reacts to (a write
//! arms/disarms a channel's request); every other TCD field is read
//! generically by field name (`SADDR`, `ATTR`, ... `CSR`), not hardcoded per
//! channel, matching the discipline `unicorn-reference-final:emu/edma.py`/`unicorn-reference-final:emu/dspi2.py`/`unicorn-reference-final:emu/ssi.py`
//! already follow (see their module docstrings: "duplicating firmware
//! struct layout is the mistake to avoid").
//!
//! `EDMA_SERQ`/`EDMA_CERQ`/`EDMA_CEEI`/`EDMA_CINT` are documented
//! **write-only: "Reads of this register return all zeroes"** (RM p.374,
//! p.375, p.377) -- real hardware, not exercised by any RD in the four
//! recorded traces (a read of these four bytes never appears), so this is
//! implemented from the manual with no oracle to check it against: an
//! `EdmaBank::read` of one of these four addresses returns 0, not the
//! `RegFile`'s echoed byte. Every other address in the slot (including every
//! TCD field this crate does not specifically interpret) is plain
//! `RegFile` passthrough, matching the Python oracle, which never
//! intercepts them at all.

use crate::regfile::RegFile;

pub const EDMA_BASE: u32 = 0xFC044000;
pub const SERQ: u32 = EDMA_BASE + 0x18;
pub const CERQ: u32 = EDMA_BASE + 0x19;
pub const SEEI: u32 = EDMA_BASE + 0x1A;
pub const CEEI: u32 = EDMA_BASE + 0x1B;
pub const CINT: u32 = EDMA_BASE + 0x1C;
/// Interrupt request latches (RM 19.4.13, p.380). Device Board supplies
/// W1C and CINT acknowledgement; Oracle keeps raw register passthrough.
pub const INTH: u32 = EDMA_BASE + 0x20;
pub const INTL: u32 = EDMA_BASE + 0x24;
pub const CERR: u32 = EDMA_BASE + 0x1D;
/// Enable request registers (RM 19.4.3 p.370-371): ERQH is channels 32..63 at +0x08, ERQL channels 0..31 at +0x0C.
pub const ERQH: u32 = EDMA_BASE + 0x08;
pub const ERQL: u32 = EDMA_BASE + 0x0C;
/// Error status (RM 19.4.2 p.369), error and interrupt latches (p.380-383).
pub const ES: u32 = EDMA_BASE + 0x04;
pub const ERRH: u32 = EDMA_BASE + 0x28;
pub const ERRL: u32 = EDMA_BASE + 0x2C;
pub const SSRT: u32 = EDMA_BASE + 0x1E;
pub const CDNE: u32 = EDMA_BASE + 0x1F;

/// Registers documented "Reads of this register return all zeroes" (RM
/// p.374 EDMA_SERQ, p.375 EDMA_CERQ/EDMA_CEEI header, p.377 EDMA_CINT).
/// `EDMA_SEEI`/`EDMA_SSRT`/`EDMA_CERR`/`EDMA_CDNE` are the same family of
/// write-only command register (RM 19.4.7-19.4.12) but their read behaviour
/// is not transcribed here since none is exercised by any trace; they stay
/// plain RAM like everything else this crate does not specifically model.
pub const READ_ZERO: [u32; 4] = [SERQ, CERQ, CEEI, CINT];

pub const TCD_BASE: u32 = 0xFC045000;
/// TCDn's offset within `EdmaBank`'s one 16 KiB slot (`TCD_BASE - EDMA_BASE`).
pub const TCD_SLOT_OFFSET: usize = (TCD_BASE - EDMA_BASE) as usize;

// TCDn field offsets, relative to that channel's 32-byte descriptor
// (RM Figure 19-24 p.384, the ColdFire field order -- CITER at +0x14,
// BITER at +0x1C, NOT the Kinetis order).
pub const SADDR: usize = 0x00;
pub const ATTR: usize = 0x04;
pub const SOFF: usize = 0x06;
pub const NBYTES: usize = 0x08;
pub const SLAST: usize = 0x0C;
pub const DADDR: usize = 0x10;
pub const CITER: usize = 0x14;
pub const DOFF: usize = 0x16;
pub const DLAST: usize = 0x18;
pub const BITER: usize = 0x1C;
pub const CSR: usize = 0x1E;

// TCDn_CSR bits (RM Figure 19-36 / Table 19-31, p.390-391).
pub const CSR_DONE: u16 = 0x0080;
pub const CSR_ACTIVE: u16 = 0x0040;
pub const CSR_MAJOR_E_LINK: u16 = 0x0020;
pub const CSR_E_SG: u16 = 0x0010;
pub const CSR_D_REQ: u16 = 0x0008;
pub const CSR_INT_HALF: u16 = 0x0004;
pub const CSR_INT_MAJOR: u16 = 0x0002;
pub const CSR_START: u16 = 0x0001;

/// A detached eDMA TCD image.  Peripheral helpers use this instead of a
/// `RegFile` so their transfer effects are pure and can be applied by a
/// machine owner after its guest-memory operation succeeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcdSnapshot {
    pub saddr: u32,
    pub attr: u16,
    pub soff: i16,
    pub nbytes: u32,
    pub slast: i32,
    pub daddr: u32,
    /// The count with E_LINK removed.
    pub citer: u16,
    pub doff: i16,
    pub dlast: i32,
    /// The count with E_LINK removed.
    pub biter: u16,
    pub csr: u16,
}

/// -> the byte offset of channel `chan`'s TCD, within `EdmaBank`'s slot.
pub const fn tcd_offset(chan: usize) -> usize {
    TCD_SLOT_OFFSET + chan * 0x20
}

/// Sign-extend the low `bits` bits of `value` (`unicorn-reference-final:emu/edma.py`/`unicorn-reference-final:emu/dspi2.py`'s
/// `_signed`; `edma.py`'s `TxChannel.run` inlines the 16-bit case).
pub fn signed(value: u32, bits: u32) -> i64 {
    let sign = 1i64 << (bits - 1);
    let v = (value as i64) & ((1i64 << bits) - 1);
    if v & sign != 0 { v - (1i64 << bits) } else { v }
}

/// One eDMA channel's live TCD fields, generically read/written by name
/// (RM 19.4.17) -- the same fields every model in this crate reads instead
/// of hardcoding a layout. Backed by whichever `RegFile` slot the channel's
/// TCD physically lives in (always `EdmaBank`'s, for every channel this
/// crate touches).
pub struct TcdView<'a> {
    regs: &'a mut RegFile,
    base: usize,
}

impl<'a> TcdView<'a> {
    pub fn new(regs: &'a mut RegFile, chan: usize) -> Self {
        Self {
            regs,
            base: tcd_offset(chan),
        }
    }
    pub fn saddr(&self) -> u32 {
        self.regs.u32_at(self.base + SADDR)
    }
    pub fn set_saddr(&mut self, v: u32) {
        self.regs.set_u32_at(self.base + SADDR, v)
    }
    pub fn daddr(&self) -> u32 {
        self.regs.u32_at(self.base + DADDR)
    }
    pub fn set_daddr(&mut self, v: u32) {
        self.regs.set_u32_at(self.base + DADDR, v)
    }
    pub fn attr(&self) -> u16 {
        self.regs.u16_at(self.base + ATTR)
    }
    pub fn soff(&self) -> i64 {
        signed(self.regs.u16_at(self.base + SOFF) as u32, 16)
    }
    pub fn doff(&self) -> i64 {
        signed(self.regs.u16_at(self.base + DOFF) as u32, 16)
    }
    pub fn nbytes(&self) -> u32 {
        self.regs.u32_at(self.base + NBYTES)
    }
    pub fn slast(&self) -> i64 {
        signed(self.regs.u32_at(self.base + SLAST), 32)
    }
    pub fn dlast(&self) -> i64 {
        signed(self.regs.u32_at(self.base + DLAST), 32)
    }
    pub fn dlast_raw(&self) -> u32 {
        self.regs.u32_at(self.base + DLAST)
    }
    /// Bit 15 of CITER/BITER is E_LINK, not part of the count (`& 0x7FFF`,
    /// matching every Python model's own masking).
    pub fn citer(&self) -> u16 {
        self.regs.u16_at(self.base + CITER) & 0x7FFF
    }
    pub fn citer_raw(&self) -> u16 {
        self.regs.u16_at(self.base + CITER)
    }
    pub fn biter(&self) -> u16 {
        self.regs.u16_at(self.base + BITER) & 0x7FFF
    }
    pub fn biter_raw(&self) -> u16 {
        self.regs.u16_at(self.base + BITER)
    }
    pub fn set_citer(&mut self, v: u16) {
        self.regs.set_u16_at(self.base + CITER, v)
    }
    pub fn csr(&self) -> u16 {
        self.regs.u16_at(self.base + CSR)
    }
    pub fn smod_mask(&self) -> u32 {
        let smod = (self.attr() >> 11) & 0x1F;
        if smod == 0 { 0 } else { (1u32 << smod) - 1 }
    }
    pub fn dmod_mask(&self) -> u32 {
        let dmod = (self.attr() >> 3) & 0x1F;
        if dmod == 0 { 0 } else { (1u32 << dmod) - 1 }
    }
    /// Read a full 32-byte descriptor (for a scatter/gather reload's source
    /// pointer verification, or `RegFile::load_page` cross-checks).
    pub fn base_offset(&self) -> usize {
        self.base
    }

    /// Copy the live descriptor into a detached value for a pure peripheral
    /// transfer helper. E_LINK is intentionally not carried in its counts;
    /// the oracle masks it before its eSDHC channel-59 bookkeeping too.
    pub fn snapshot(&self) -> TcdSnapshot {
        TcdSnapshot {
            saddr: self.saddr(),
            attr: self.attr(),
            soff: self.soff() as i16,
            nbytes: self.nbytes(),
            slast: self.slast() as i32,
            daddr: self.daddr(),
            citer: self.citer(),
            doff: self.doff() as i16,
            dlast: self.dlast() as i32,
            biter: self.biter(),
            csr: self.csr(),
        }
    }

    /// Apply only the three writes the eSDHC oracle performs on completion:
    /// the moving address, reloaded CITER, and DONE-set CSR. The remaining
    /// snapshot fields are inputs, not writebacks (or extra HWR events).
    pub fn apply_dma59_writeback(&mut self, tcd: TcdSnapshot, card_to_guest: bool) {
        if card_to_guest {
            self.set_daddr(tcd.daddr);
        } else {
            self.set_saddr(tcd.saddr);
        }
        self.set_citer(tcd.citer);
        self.regs.set_u16_at(self.base + CSR, tcd.csr);
    }
}

/// One eDMA channel whose destination is a fixed peripheral register and
/// whose major loop runs eagerly on `EDMA_SERQ`, run entirely inside a
/// register write with the CPU's own instruction stream paused -- a direct
/// port of `unicorn-reference-final:emu/edma.py`'s `TxChannel` (see that module's docstring for the
/// hardware reasoning: eDMA channel 35, the UART8 console TX ring).
///
/// The major loop's *source* bytes (`0x4FE1B000`'s ring, outside this
/// crate's own register file) are not something a peripheral-only replay
/// can read independently -- there is no guest DDR model here, only the
/// peripheral slots the trace hooks. A live machine (a future
/// `packages/emulator/machine`, with a real memory bus) calls [`TxChannel::run`]
/// directly with the source bytes it read itself; a trace replay instead
/// takes them from the trace's own merged `HRD` record for this exact call
/// (`unicorn-reference-final:emu/edma.py`'s `TxChannel.run read` -- `docs/plans/native-emulator.md` P4,
/// "host memory-write records HWR" mentions this class of record; the read
/// counterpart is `HRD`, recorded "only for peripheral modules... the DMA
/// source data a model replayed in isolation needs as input",
/// `scratchpad/p4-mmio-recorder.md`). Either way the bytes are handed to
/// `run`, which does not care where they came from -- only the register
/// side effects (SADDR advance, CITER reload) are computed here, and those
/// *are* checked, against `HWR`/`RD`, by a replay.
pub struct TxChannel {
    pub chan: usize,
    pub vector: u16,
    /// Queued major-loop completions, delivered from a PC-triggered hook in
    /// Python (`at(wait_loop, ...)`) that this peripheral-only crate cannot
    /// reproduce (see `bin/replay.rs`'s module docs, "PC-triggered
    /// completions"): a replay counts these, it does not predict their
    /// instruction-count boundary.
    pub pending: u64,
    pub bytes: u64,
    pub transfers: u64,
}

impl TxChannel {
    pub fn new(chan: usize, vector: u16) -> Self {
        Self {
            chan,
            vector,
            pending: 0,
            bytes: 0,
            transfers: 0,
        }
    }

    /// Run the whole major loop now, given the source bytes it moved
    /// (`citer * 1` bytes, one byte per minor loop -- `unicorn-reference-final:emu/edma.py`'s `TxChannel`
    /// always uses `NBYTES` bytes per minor loop, but the console driver's
    /// own TCD always programs `NBYTES=1`; `source` may be longer, only the
    /// first `citer * nbytes` bytes are used). -> `false` (no-op) if CITER
    /// is 0, matching Python's early return.
    pub fn run(&mut self, regs: &mut RegFile, source: &[u8]) -> bool {
        let mut tcd = TcdView::new(regs, self.chan);
        let citer = tcd.citer();
        if citer == 0 {
            return false;
        }
        let nbytes = tcd.nbytes();
        let want = citer as usize * nbytes as usize;
        debug_assert!(
            source.len() >= want,
            "TxChannel::run: source has {} bytes, TCD wants {}",
            source.len(),
            want
        );
        let biter = tcd.biter();
        let mut src = tcd.saddr();
        let soff = tcd.soff();
        let mask = tcd.smod_mask();
        let base = if mask != 0 { src & !mask } else { 0 };
        for _ in 0..want {
            src = ((src as i64 + soff) & 0xFFFF_FFFF) as u32;
            if mask != 0 {
                src = base | (src & mask);
            }
        }
        src = ((src as i64 + tcd.slast()) & 0xFFFF_FFFF) as u32;
        if mask != 0 {
            src = base | (src & mask);
        }
        tcd.set_saddr(src);
        tcd.set_citer(biter); // major-loop completion reloads CITER
        self.bytes += want as u64;
        self.transfers += 1;
        self.pending += 1;
        true
    }

    /// One queued completion delivered (from `deliver()`'s replay-side
    /// counterpart -- see the struct docs). -> whether one was available.
    pub fn consume_pending(&mut self) -> bool {
        if self.pending == 0 {
            return false;
        }
        self.pending -= 1;
        true
    }
}

impl TxChannel {
    pub fn snap_save(&self, w: &mut crate::snap::Writer) {
        w.u64(self.pending);
        w.u64(self.bytes);
        w.u64(self.transfers);
    }
    pub fn snap_load(&mut self, r: &mut crate::snap::Reader) -> crate::snap::Result<()> {
        self.pending = r.u64()?;
        self.bytes = r.u64()?;
        self.transfers = r.u64()?;
        Ok(())
    }
}

/// EDMA_ES bits (RM Table 19-4 p.369). Bus errors (SBE, DBE) and configuration errors are different bits: an access that no slave answers
/// is a bus error, a misaligned address or offset is a configuration error.
pub mod es {
    pub const VLD: u32 = 1 << 31;
    pub const SAE: u32 = 1 << 7;
    pub const SOE: u32 = 1 << 6;
    pub const DAE: u32 = 1 << 5;
    pub const DOE: u32 = 1 << 4;
    pub const NCE: u32 = 1 << 3;
    pub const SGE: u32 = 1 << 2;
    pub const SBE: u32 = 1 << 1;
    pub const DBE: u32 = 1 << 0;
}

/// Transfer size in bytes of a TCDn_ATTR SSIZE/DSIZE code (RM Table 19-21 p.385): 0 is 8-bit, 1 16-bit, 2 32-bit, 4 16-byte. The other
/// codes are reserved and cause a configuration error.
pub fn transfer_size(code: u16) -> Option<u32> {
    match code & 7 {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        4 => Some(16),
        _ => None,
    }
}

/// Guest memory as a software-started transfer sees it: one byte at a time, `None`/`false` when no slave answers (a bus error).
pub trait SoftBus {
    fn read_byte(&mut self, addr: u32) -> Option<u8>;
    fn write_byte(&mut self, addr: u32, value: u8) -> bool;
}

/// The descriptor fields a software-started transfer needs, raw as the guest wrote them (`citer`, `biter` keep their E_LINK bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoftTcd {
    pub saddr: u32,
    pub attr: u16,
    pub soff: i16,
    pub nbytes: u32,
    pub slast: i32,
    pub daddr: u32,
    pub citer: u16,
    pub doff: i16,
    /// DLAST_SGA: an offset added to the destination, or with E_SG the address of the next descriptor.
    pub dlast_sga: u32,
    pub biter: u16,
    pub csr: u16,
}

/// What a software start did, for the owner to write back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoftOutcome {
    /// The channel stopped with an error: `es` has the error bits (the owner adds VLD and ERRCHN). The descriptor keeps the state at the
    /// point of the fault (RM p.368): current addresses and iteration count, no DONE.
    Fault {
        es: u32,
        saddr: u32,
        daddr: u32,
        citer: u16,
    },
    /// The major loop finished. `next` is the descriptor loaded when E_SG is set (it replaces the channel's TCD, so `saddr`, `daddr`,
    /// `citer` and `csr` are only for the case without it). `interrupt` asks for the channel's INTH/INTL bit, `clear_request` for
    /// its ERQ bit to clear (D_REQ). `es` is a configuration error found at major loop completion (an unaligned scatter/gather address):
    /// the data moved, the descriptor was not loaded.
    Done {
        saddr: u32,
        daddr: u32,
        citer: u16,
        csr: u16,
        next: Option<[u8; 32]>,
        interrupt: bool,
        clear_request: bool,
        es: u32,
    },
}

/// Most bytes a software-started transfer may move in one call.
pub const MAX_SOFT_BYTES: u64 = 1 << 26;

/// Runs a software-started (SSRT) transfer whole: the model has no bus contention. Checks the configuration as RM 19.4.2 lists it (reserved
/// transfer sizes, alignment of addresses and offsets, NBYTES a multiple of both sizes), then moves `nbytes` per minor loop for the
/// iteration count. NBYTES = 0 is a 4 GiB transfer (RM p.386), above `MAX_SOFT_BYTES`: refused with NCE. The major loop adds SLAST to the
/// source and, without E_SG, DLAST_SGA to the destination; with E_SG, DLAST_SGA is the address of the next descriptor (32-byte aligned,
/// RM p.388) and is read from memory.
pub fn software_start(t: &SoftTcd, bus: &mut impl SoftBus) -> SoftOutcome {
    let count_mask = if t.citer & 0x8000 != 0 { 0x1ff } else { 0x7fff };
    let loops = u32::from(t.citer & count_mask);
    let config = |es: u32| SoftOutcome::Fault {
        es,
        saddr: t.saddr,
        daddr: t.daddr,
        citer: t.citer,
    };
    // Every configuration error that applies is reported together (RM 19.4.2 p.366-367, p.370).
    let mut bits = 0;
    // CITER = 0, or CITER[E_LINK] != BITER[E_LINK] (p.370).
    if loops == 0 || (t.citer ^ t.biter) & 0x8000 != 0 {
        bits |= es::NCE;
    }
    let ssize = transfer_size(t.attr >> 8);
    let dsize = transfer_size(t.attr);
    // A reserved size code is a configuration error; the manual names no bit, so it is the address error of its side.
    if ssize.is_none() {
        bits |= es::SAE;
    }
    if dsize.is_none() {
        bits |= es::DAE;
    }
    let nbytes = if t.nbytes == 0 {
        1u64 << 32
    } else {
        u64::from(t.nbytes)
    };
    if let (Some(ssize), Some(dsize)) = (ssize, dsize) {
        if !nbytes.is_multiple_of(u64::from(ssize)) || !nbytes.is_multiple_of(u64::from(dsize)) {
            bits |= es::NCE;
        }
        if !t.saddr.is_multiple_of(ssize) {
            bits |= es::SAE;
        }
        if !(t.soff.unsigned_abs() as u32).is_multiple_of(ssize) {
            bits |= es::SOE;
        }
        if !t.daddr.is_multiple_of(dsize) {
            bits |= es::DAE;
        }
        if !(t.doff.unsigned_abs() as u32).is_multiple_of(dsize) {
            bits |= es::DOE;
        }
    }
    // The model has no bus contention and runs the transfer inside one host call, so a transfer above MAX_SOFT_BYTES (NBYTES = 0 is
    // 4 GiB) is refused as an NBYTES/CITER configuration error instead of hanging the host: nothing moves, the TCD is unchanged. No
    // firmware transfer is near this size (the largest is a few KiB).
    if nbytes.saturating_mul(u64::from(loops)) > MAX_SOFT_BYTES {
        bits |= es::NCE;
    }
    if bits != 0 {
        return config(bits);
    }
    let (Some(ssize), Some(dsize)) = (ssize, dsize) else {
        return config(es::SAE);
    };
    let (mut src, mut dst) = (t.saddr, t.daddr);
    // A chunk is the larger transfer size: whole elements on both sides.
    let chunk = ssize.max(dsize);
    let mut buf = [0u8; 16];
    for done in 0..loops {
        let fault = |es: u32, src: u32, dst: u32| SoftOutcome::Fault {
            es,
            saddr: src,
            daddr: dst,
            citer: (t.citer & !count_mask) | (loops - done) as u16,
        };
        let mut left = nbytes;
        while left > 0 {
            for i in 0..chunk / ssize {
                for k in 0..ssize {
                    match bus.read_byte(src.wrapping_add(k)) {
                        Some(v) => buf[(i * ssize + k) as usize] = v,
                        None => return fault(es::SBE, src, dst),
                    }
                }
                src = src.wrapping_add_signed(i32::from(t.soff));
            }
            for i in 0..chunk / dsize {
                for k in 0..dsize {
                    if !bus.write_byte(dst.wrapping_add(k), buf[(i * dsize + k) as usize]) {
                        return fault(es::DBE, src, dst);
                    }
                }
                dst = dst.wrapping_add_signed(i32::from(t.doff));
            }
            left -= u64::from(chunk);
        }
    }
    let biter_count = u32::from(t.biter & if t.biter & 0x8000 != 0 { 0x1ff } else { 0x7fff });
    let interrupt = t.csr & CSR_INT_MAJOR != 0 || (t.csr & CSR_INT_HALF != 0 && biter_count >= 2);
    let clear_request = t.csr & CSR_D_REQ != 0;
    let csr = (t.csr & !(CSR_START | CSR_ACTIVE)) | CSR_DONE;
    src = src.wrapping_add_signed(t.slast);
    let mut outcome = SoftOutcome::Done {
        saddr: src,
        daddr: dst,
        citer: t.biter,
        csr,
        next: None,
        interrupt,
        clear_request,
        es: 0,
    };
    if t.csr & CSR_E_SG == 0 {
        if let SoftOutcome::Done { daddr, .. } = &mut outcome {
            *daddr = dst.wrapping_add_signed(t.dlast_sga as i32);
        }
        return outcome;
    }
    if !t.dlast_sga.is_multiple_of(32) {
        if let SoftOutcome::Done { es, .. } = &mut outcome {
            *es = es::SGE;
        }
        return outcome;
    }
    let mut raw = [0u8; 32];
    for (i, byte) in raw.iter_mut().enumerate() {
        match bus.read_byte(t.dlast_sga.wrapping_add(i as u32)) {
            Some(v) => *byte = v,
            None => {
                return SoftOutcome::Fault {
                    es: es::SBE,
                    saddr: src,
                    daddr: dst,
                    citer: t.biter,
                };
            }
        }
    }
    if let SoftOutcome::Done { next, .. } = &mut outcome {
        *next = Some(raw);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 256 bytes of memory at 0x1000; every other address is a bus error.
    struct Mem(Vec<u8>);

    impl SoftBus for Mem {
        fn read_byte(&mut self, addr: u32) -> Option<u8> {
            self.0.get(addr.checked_sub(0x1000)? as usize).copied()
        }
        fn write_byte(&mut self, addr: u32, value: u8) -> bool {
            match addr
                .checked_sub(0x1000)
                .and_then(|a| self.0.get_mut(a as usize))
            {
                Some(b) => {
                    *b = value;
                    true
                }
                None => false,
            }
        }
    }

    fn mem() -> Mem {
        Mem((0..=255).collect())
    }

    /// 2 loops of 8 bytes, 32-bit elements, 0x1000 to 0x1080.
    fn tcd() -> SoftTcd {
        SoftTcd {
            saddr: 0x1000,
            attr: 0x0202,
            soff: 4,
            nbytes: 8,
            slast: -16,
            daddr: 0x1080,
            citer: 2,
            doff: 4,
            dlast_sga: 0,
            biter: 2,
            csr: 0,
        }
    }

    #[test]
    fn an_unanswered_access_is_a_bus_error_with_the_tcd_at_the_fault_point() {
        let mut m = mem();
        let t = SoftTcd {
            daddr: 0x10fc,
            ..tcd()
        };
        // The second 32-bit write of the first loop leaves the mapped bytes: DBE (bit 0), not the alignment error DAE.
        assert_eq!(
            software_start(&t, &mut m),
            SoftOutcome::Fault {
                es: es::DBE,
                saddr: 0x1008,
                daddr: 0x1100,
                citer: 2
            }
        );
        let t = SoftTcd {
            saddr: 0x2000,
            ..tcd()
        };
        assert_eq!(
            software_start(&t, &mut m),
            SoftOutcome::Fault {
                es: es::SBE,
                saddr: 0x2000,
                daddr: 0x1080,
                citer: 2
            }
        );
        // NBYTES = 0 is a 4 GiB transfer: refused at once (NCE, nothing moves), even with SOFF = DOFF = 0 in mapped memory.
        let before = m.0.clone();
        let t = SoftTcd {
            nbytes: 0,
            soff: 0,
            doff: 0,
            ..tcd()
        };
        let start = std::time::Instant::now();
        assert!(matches!(
            software_start(&t, &mut m),
            SoftOutcome::Fault { es: es::NCE, .. }
        ));
        assert!(start.elapsed().as_secs() < 1);
        assert_eq!(m.0, before);
    }

    #[test]
    fn every_configuration_error_that_applies_is_reported() {
        let mut m = mem();
        let run = |t: SoftTcd, m: &mut Mem| match software_start(&t, m) {
            SoftOutcome::Fault { es, .. } => es,
            other => panic!("{other:?}"),
        };
        // CITER = 0 and an E_LINK mismatch are NBYTES/CITER errors (RM p.370).
        assert_eq!(run(SoftTcd { citer: 0, ..tcd() }, &mut m), es::NCE);
        assert_eq!(
            run(
                SoftTcd {
                    citer: 0x8002,
                    ..tcd()
                },
                &mut m
            ),
            es::NCE
        );
        // A misaligned address and offset on the same side are both reported.
        assert_eq!(
            run(
                SoftTcd {
                    saddr: 0x1002,
                    soff: 2,
                    ..tcd()
                },
                &mut m
            ),
            es::SAE | es::SOE
        );
        assert_eq!(
            run(
                SoftTcd {
                    daddr: 0x1081,
                    doff: 1,
                    nbytes: 6,
                    ..tcd()
                },
                &mut m
            ),
            es::DAE | es::DOE | es::NCE
        );
        assert_eq!(
            run(
                SoftTcd {
                    attr: 0x0303,
                    ..tcd()
                },
                &mut m
            ),
            es::SAE | es::DAE
        );
    }

    #[test]
    fn configuration_errors_are_recorded_and_move_nothing() {
        let before = mem().0;
        let cases = [
            (
                SoftTcd {
                    attr: 0x0302,
                    ..tcd()
                },
                es::SAE,
            ), // reserved SSIZE 3
            (
                SoftTcd {
                    attr: 0x0205,
                    ..tcd()
                },
                es::DAE,
            ), // reserved DSIZE 5
            (SoftTcd { nbytes: 6, ..tcd() }, es::NCE),
            (
                SoftTcd {
                    saddr: 0x1002,
                    ..tcd()
                },
                es::SAE,
            ),
            (SoftTcd { soff: 2, ..tcd() }, es::SOE),
            (
                SoftTcd {
                    daddr: 0x1082,
                    ..tcd()
                },
                es::DAE,
            ),
            (SoftTcd { doff: 6, ..tcd() }, es::DOE),
        ];
        for (t, want) in cases {
            let mut m = mem();
            let SoftOutcome::Fault {
                es,
                saddr,
                daddr,
                citer,
            } = software_start(&t, &mut m)
            else {
                panic!("{t:?} must fault");
            };
            assert_eq!(es, want, "{t:?}");
            assert_eq!((saddr, daddr, citer), (t.saddr, t.daddr, t.citer));
            assert_eq!(m.0, before);
        }
        // 16-byte elements and unequal sizes are legal.
        let mut m = mem();
        let t = SoftTcd {
            attr: 0x0401,
            nbytes: 16,
            soff: 16,
            doff: 2,
            citer: 1,
            biter: 1,
            slast: 0,
            ..tcd()
        };
        assert!(matches!(
            software_start(&t, &mut m),
            SoftOutcome::Done { .. }
        ));
        assert_eq!(m.0[0x80..0x84], [0, 1, 2, 3]);
        assert_eq!(m.0[0x8e..0x90], [14, 15]);
    }

    #[test]
    fn the_major_loop_ends_with_interrupt_request_clear_and_descriptor_load() {
        // Plain end: SLAST and DLAST_SGA are address offsets; INT_HALF needs BITER >= 2.
        let mut m = mem();
        let t = SoftTcd {
            dlast_sga: 0x20,
            csr: CSR_START | CSR_D_REQ | CSR_INT_HALF,
            ..tcd()
        };
        let SoftOutcome::Done {
            saddr,
            daddr,
            citer,
            csr,
            next,
            interrupt,
            clear_request,
            es,
        } = software_start(&t, &mut m)
        else {
            panic!("done");
        };
        assert_eq!((saddr, daddr, citer), (0x1000, 0x1080 + 16 + 0x20, 2));
        assert_eq!(csr, CSR_D_REQ | CSR_INT_HALF | CSR_DONE);
        assert!(next.is_none() && interrupt && clear_request);
        assert_eq!(es, 0);
        assert_eq!(m.0[0x80..0x90], (0..16).collect::<Vec<u8>>()[..]);
        let t = SoftTcd {
            csr: CSR_INT_HALF,
            biter: 1,
            citer: 1,
            ..tcd()
        };
        let SoftOutcome::Done { interrupt, .. } = software_start(&t, &mut mem()) else {
            panic!("done");
        };
        assert!(!interrupt, "no half-way point with one iteration");
        // E_SG: DLAST_SGA names the next descriptor; DADDR is not adjusted.
        let t = SoftTcd {
            dlast_sga: 0x1040,
            csr: CSR_E_SG | CSR_INT_MAJOR,
            ..tcd()
        };
        let SoftOutcome::Done {
            daddr,
            next,
            interrupt,
            ..
        } = software_start(&t, &mut m)
        else {
            panic!("done");
        };
        assert_eq!(daddr, 0x1090);
        assert_eq!(next.unwrap()[..4], [0x40, 0x41, 0x42, 0x43]);
        assert!(interrupt);
        // An unaligned or unreadable descriptor address is an error.
        let t = SoftTcd {
            dlast_sga: 0x1044,
            csr: CSR_E_SG,
            ..tcd()
        };
        let SoftOutcome::Done { es, next, .. } = software_start(&t, &mut m) else {
            panic!("done");
        };
        assert_eq!((es, next), (self::es::SGE, None));
        let t = SoftTcd {
            dlast_sga: 0x3000,
            csr: CSR_E_SG,
            ..tcd()
        };
        assert!(matches!(
            software_start(&t, &mut m),
            SoftOutcome::Fault {
                es: self::es::SBE,
                ..
            }
        ));
    }
}
