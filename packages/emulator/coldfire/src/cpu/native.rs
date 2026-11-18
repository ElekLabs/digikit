//! Native kernels: hot ColdFire functions translated to Rust ahead of time (`cfaot`), run by the host instead of the interpreter.
//!
//! A kernel executes one function with exactly the interpreter's results: registers, CCR (as `resolve_nzv` leaves it), EMAC state,
//! the memory writes and reads in order, the instruction count. It stops at an instruction boundary, with the state the interpreter
//! would have there, when the caller's budget ends, a bus access fails, or a write touches the function's own code. The interpreter
//! carries on from `pc`.
//!
//! Safety of the match: a kernel is keyed by its entry address *and* the FNV-1a hash of its code bytes, read through the bus the
//! first time `pc` reaches the entry and again after any CPU or DMA write overlapped the guarded range (`DecodeCache::guard`).
//! A different image has other bytes, so its kernels never run.
//!
//! The generated code (`kernels.rs`, from `CF_NATIVE_DIR`, firmware-derived and never committed) defines `KERNELS` and `run`.

use super::*;

/// One generated kernel.
pub struct KernelDesc {
    /// First instruction.
    pub entry: u32,
    /// Bytes from `entry` to the end of the last instruction.
    pub len: u32,
    /// FNV-1a 64 of those bytes.
    pub hash: u64,
    /// Byte offsets of the instructions, in order: the index of an instruction is the index the kernel passes to `ins`.
    pub insns: &'static [u16],
}

#[cfg(cf_native)]
include!(env!("CF_NATIVE_FILE"));

#[cfg(not(cf_native))]
pub static KERNELS: &[KernelDesc] = &[];
#[cfg(not(cf_native))]
fn run<B: Bus>(_: usize, _: &mut Cpu, _: &mut B, _: &[Insn], _: u64) -> u64 {
    unreachable!("no kernels")
}

/// FNV-1a 64 over `bytes`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Unchecked,
    Verified,
    /// The bytes at the entry are not the kernel's.
    Bad,
}

/// Counters, for the reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeStats {
    pub runs: u64,
    pub insns: u64,
    /// Runs that stopped before the function's end (budget, bus error, own-code write).
    pub early: u64,
    pub verified: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NativeState {
    enabled: bool,
    slots: Vec<Slot>,
    insns: Vec<Vec<Insn>>,
    lo: u32,
    span: u32,
    /// Set by a kernel that ended on a `Stop` of the core.
    stop: Option<Stop>,
    pub stats: NativeStats,
}

/// Not machine state: two cores that run the same code are equal whether or not one has kernels.
impl PartialEq for NativeState {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Cpu {
    /// Turn the generated kernels on. False when this build has none.
    pub fn native_install(&mut self) -> bool {
        if KERNELS.is_empty() {
            return false;
        }
        let lo = KERNELS.iter().map(|k| k.entry).min().unwrap_or(0);
        let hi = KERNELS.iter().map(|k| k.entry + k.len).max().unwrap_or(0);
        self.native = NativeState {
            enabled: true,
            slots: vec![Slot::Unchecked; KERNELS.len()],
            insns: vec![Vec::new(); KERNELS.len()],
            lo,
            span: hi - lo,
            stop: None,
            stats: NativeStats::default(),
        };
        self.decode_cache.guard_lo = lo;
        self.decode_cache.guard_hi = hi;
        self.decode_cache.guard_hit = false;
        true
    }

    /// Turn the kernels off (the guard stays; it costs two compares per write).
    pub fn native_disable(&mut self) {
        self.native.enabled = false;
    }

    pub fn native_stats(&self) -> NativeStats {
        self.native.stats
    }

    /// Whether `pc` may be the entry of a kernel (a cheap test for the caller's loop).
    #[inline(always)]
    pub fn native_wants(&self) -> bool {
        self.native.enabled && self.pc.wrapping_sub(self.native.lo) < self.native.span
    }

    /// Read the kernel's bytes and decode its instructions; false when they are not the kernel's.
    fn native_verify<B: Bus>(&mut self, bus: &mut B, id: usize) -> bool {
        let k = &KERNELS[id];
        let mut bytes = Vec::with_capacity(k.len as usize + 4);
        for off in (0..k.len + 4).step_by(2) {
            match bus.fetch16(k.entry.wrapping_add(off)) {
                Ok(w) => bytes.extend_from_slice(&w.to_be_bytes()),
                Err(_) if off >= k.len => bytes.extend_from_slice(&[0, 0]),
                Err(_) => return false,
            }
        }
        if fnv1a(&bytes[..k.len as usize]) != k.hash {
            return false;
        }
        let mut insns = Vec::with_capacity(k.insns.len());
        for &off in k.insns {
            let o = usize::from(off);
            let w = |n: usize| u16::from_be_bytes([bytes[o + 2 * n], bytes[o + 2 * n + 1]]);
            let words = [
                w(0),
                if o + 2 < bytes.len() { w(1) } else { 0 },
                if o + 4 < bytes.len() { w(2) } else { 0 },
            ];
            match decode(k.entry.wrapping_add(u32::from(off)), words) {
                Some(i) => insns.push(i),
                None => return false,
            }
        }
        self.native.insns[id] = insns;
        true
    }

    /// The kernel at `pc`, verified, if any.
    fn native_find<B: Bus>(&mut self, bus: &mut B) -> Option<usize> {
        if !self.native.enabled || self.state != RunState::Running {
            return None;
        }
        let id = KERNELS.iter().position(|k| k.entry == self.pc)?;
        if self.decode_cache.guard_hit {
            self.decode_cache.guard_hit = false;
            self.native.slots.fill(Slot::Unchecked);
        }
        if self.native.slots[id] == Slot::Unchecked {
            let ok = self.native_verify(bus, id);
            self.native.slots[id] = if ok { Slot::Verified } else { Slot::Bad };
            // The fetches above are not CPU writes: a hit set by one of them cannot exist, but keep the flag clean.
            self.decode_cache.guard_hit = false;
            if ok {
                self.native.stats.verified += 1;
            }
        }
        (self.native.slots[id] == Slot::Verified).then_some(id)
    }

    /// Run the kernel that starts at `pc`, for at most `budget` instructions. `None`: no kernel here or it did not run (the caller steps).
    /// `Some(r)`: instructions ran; `r` is the `Stop` of the core when one ended the run (an exception taken inside is not one).
    pub fn run_native<B: Bus>(&mut self, bus: &mut B, budget: u64) -> Option<Result<(), Stop>> {
        let id = self.native_find(bus)?;
        let before = self.icount;
        self.last_exception = None;
        self.last_unimplemented = None;
        self.native.stop = None;
        let insns = std::mem::take(&mut self.native.insns[id]);
        let n = run(id, self, bus, &insns, budget);
        self.native.insns[id] = insns;
        debug_assert_eq!(self.icount - before, n);
        if n == 0 {
            return None;
        }
        self.native.stats.runs += 1;
        self.native.stats.insns += n;
        if self.native.stop.is_some() || self.pc.wrapping_sub(KERNELS[id].entry) < KERNELS[id].len {
            self.native.stats.early += 1;
        }
        Some(match self.native.stop.take() {
            Some(stop) => Err(stop),
            None => Ok(()),
        })
    }

    /// A memory write by a kernel: the interpreter's write path (decode cache and guard first).
    #[inline(always)]
    pub fn native_write32<B: Bus>(
        &mut self,
        bus: &mut B,
        addr: u32,
        v: u32,
    ) -> Result<(), BusError> {
        self.decode_cache.invalidate(addr, 4);
        bus.write32(addr, v)
    }

    /// Whether a write since the last check touched the guarded code.
    #[inline(always)]
    pub fn native_code_written(&self) -> bool {
        self.decode_cache.guard_hit
    }

    /// One instruction through the interpreter, from a kernel: `step` after the fetch. `Ok(true)` completed; `Ok(false)` an exception
    /// was taken (the state is the handler's); `Err` the core stopped. Adds to `icount` itself.
    pub fn native_exec<B: Bus>(&mut self, bus: &mut B, i: &Insn, pc: u32) -> Result<bool, Stop> {
        self.last_exception = None;
        self.last_unimplemented = None;
        let traps = i.traps();
        if traps != 0 {
            if traps & TRAP_PRIVILEGED != 0 && self.sr & sr::S == 0 {
                self.icount += 1;
                return self.exception(bus, vector::PRIVILEGE, pc).map(|()| false);
            }
            if traps & TRAP_FPU != 0 && self.fpu.is_none() {
                self.icount += 1;
                return self.exception(bus, vector::LINE_F, pc).map(|()| false);
            }
        }
        self.pc = pc.wrapping_add(i.len as u32);
        let r = self.execute(bus, i, pc);
        self.icount += 1;
        match r {
            Ok(()) => Ok(true),
            Err(Ok(e)) => self
                .exception(bus, e.vector(), if e.pc() == 0 { pc } else { e.pc() })
                .map(|()| false),
            Err(Err(stop)) => {
                if let Stop::Unimplemented(f) = stop {
                    self.pc = pc;
                    self.icount -= 1;
                    self.last_unimplemented = Some(f);
                }
                Err(stop)
            }
        }
    }

    /// Longword shift of `v` by `count` (0-63), result only: what `shift` returns first.
    #[inline(always)]
    pub fn native_shift_l(v: u32, count: u32, left: bool, arith: bool) -> u32 {
        if left {
            if count >= 32 { 0 } else { v << count }
        } else if arith {
            ((v as i32) >> count.min(31)) as u32
        } else if count >= 32 {
            0
        } else {
            v >> count
        }
    }

    /// Longword shift with flags, as the interpreter's shift instructions leave them.
    #[inline(always)]
    pub fn native_shift_flags(&mut self, v: u32, count: u32, left: bool, arith: bool) -> u32 {
        let (res, last, vflag) = self.shift(v, count, left, arith, Size::L);
        self.flags_shift(count, last, vflag, res, Size::L);
        res
    }

    /// `MOVCLR`'s read of accumulator `a` and its clearing.
    #[inline(always)]
    pub fn native_movclr(&mut self, a: usize) -> u32 {
        let v = self.acc_to_reg(a);
        self.emac.acc[a] = 0;
        self.set_ext16(a, 0);
        self.set_pav(a, false);
        v
    }

    #[inline(always)]
    pub fn native_set_nz(&mut self, v: u32) {
        self.set_nz(v, Size::L);
    }
    #[inline(always)]
    pub fn native_flags_add(&mut self, src: u32, dst: u32, r: u32) {
        self.flags_add(src, dst, r, Size::L);
    }
    #[inline(always)]
    pub fn native_flags_sub(&mut self, src: u32, dst: u32, r: u32, cmp: bool) {
        self.flags_sub(src, dst, r, Size::L, cmp);
    }
    #[inline(always)]
    pub fn native_cond(&mut self, c: u8) -> bool {
        self.cond(c)
    }
    #[inline(always)]
    pub fn native_mac(&mut self, ry: u32, rx: u32, word: bool, sf: u8, acc: usize, sub: bool) {
        self.mac_op(ry, rx, word, sf, acc, sub);
    }
    #[inline(always)]
    pub fn native_move_to_acc(&mut self, a: usize, v: u32) {
        self.move_to_acc(a, v);
    }
    #[inline(always)]
    pub fn native_acc_to_reg(&self, a: usize) -> u32 {
        self.acc_to_reg(a)
    }
    #[inline(always)]
    pub fn native_mask(&self) -> u32 {
        self.emac.mask
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Parity: the kernel against the interpreter from the same state.

/// A bus that buffers writes (reads see them) and logs every access in order.
pub struct JournalBus<'a, B: Bus> {
    inner: &'a mut B,
    overlay: std::collections::HashMap<u32, u8>,
    pub log: Vec<(bool, u8, u32, u32)>,
    /// Writes in order, for replaying onto the real bus.
    writes: Vec<(u8, u32, u32)>,
}

impl<'a, B: Bus> JournalBus<'a, B> {
    pub fn new(inner: &'a mut B) -> Self {
        Self {
            inner,
            overlay: Default::default(),
            log: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn get(&mut self, addr: u32, n: u8) -> Result<u32, BusError> {
        let mut v = 0u32;
        let all_overlay =
            (0..u32::from(n)).all(|k| self.overlay.contains_key(&addr.wrapping_add(k)));
        if all_overlay {
            for k in 0..u32::from(n) {
                v = v << 8 | u32::from(self.overlay[&addr.wrapping_add(k)]);
            }
            return Ok(v);
        }
        // Mixed or none: build from the inner bus byte by byte (reads are logged once, at the caller).
        for k in 0..u32::from(n) {
            let a = addr.wrapping_add(k);
            let b = match self.overlay.get(&a) {
                Some(b) => *b,
                None => self.inner.read8(a)?,
            };
            v = v << 8 | u32::from(b);
        }
        Ok(v)
    }

    /// Buffer a write. Whether the bus accepts it is known only at commit, so the bytes are first read from the bus: a failing read stands
    /// for a failing write (a check is for memory, not for device registers, whose reads can have effects).
    fn put(&mut self, addr: u32, n: u8, v: u32) -> Result<(), BusError> {
        for k in 0..u32::from(n) {
            let a = addr.wrapping_add(k);
            if !self.overlay.contains_key(&a) {
                self.inner
                    .read8(a)
                    .map_err(|e| BusError { write: true, ..e })?;
            }
        }
        for k in 0..u32::from(n) {
            let sh = 8 * (u32::from(n) - 1 - k);
            self.overlay.insert(addr.wrapping_add(k), (v >> sh) as u8);
        }
        self.writes.push((n, addr, v));
        self.log.push((true, n, addr, v));
        Ok(())
    }

    /// Replay the writes onto the inner bus.
    pub fn commit(self) -> Result<(), BusError> {
        for (n, a, v) in self.writes {
            match n {
                1 => self.inner.write8(a, v as u8)?,
                2 => self.inner.write16(a, v as u16)?,
                _ => self.inner.write32(a, v)?,
            }
        }
        Ok(())
    }
}

impl<B: Bus> Bus for JournalBus<'_, B> {
    fn read8(&mut self, a: u32) -> Result<u8, BusError> {
        let v = self.get(a, 1)?;
        self.log.push((false, 1, a, v));
        Ok(v as u8)
    }
    fn read16(&mut self, a: u32) -> Result<u16, BusError> {
        let v = self.get(a, 2)?;
        self.log.push((false, 2, a, v));
        Ok(v as u16)
    }
    fn read32(&mut self, a: u32) -> Result<u32, BusError> {
        let v = self.get(a, 4)?;
        self.log.push((false, 4, a, v));
        Ok(v)
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), BusError> {
        self.put(a, 1, u32::from(v))
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), BusError> {
        self.put(a, 2, u32::from(v))
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), BusError> {
        self.put(a, 4, v)
    }
    fn fetch16(&mut self, a: u32) -> Result<u16, BusError> {
        self.inner.fetch16(a)
    }
}

/// The architectural state a comparison looks at (CCR resolved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arch {
    pub d: [u32; 8],
    pub a: [u32; 8],
    pub pc: u32,
    pub sr: u16,
    pub emac: Emac,
    pub icount: u64,
    pub state: RunState,
    pub last_exception: Option<u8>,
}

impl Cpu {
    pub fn native_arch(&mut self) -> Arch {
        self.resolve_nzv();
        Arch {
            d: self.d,
            a: self.a,
            pc: self.pc,
            sr: self.sr,
            emac: self.emac.clone(),
            icount: self.icount,
            state: self.state,
            last_exception: self.last_exception,
        }
    }

    fn native_restore(&mut self, s: &Arch) {
        self.d = s.d;
        self.a = s.a;
        self.pc = s.pc;
        self.sr = s.sr;
        self.pending_nzv = PendingNzv::None;
        self.emac = s.emac.clone();
        self.icount = s.icount;
        self.state = s.state;
        self.last_exception = s.last_exception;
    }

    /// Run the kernel at `pc` and the interpreter from the same state, both on a journal of `bus`, and compare registers, CCR, EMAC,
    /// the instruction count and every read and write in order. On equality the interpreter's result is kept and its writes go to
    /// `bus`. `Ok(None)`: no kernel ran here (nothing changed). `Ok(Some(n))`: `n` instructions agreed. `Err`: the first difference
    /// (the state is then the interpreter's, its writes not applied).
    pub fn check_native<B: Bus>(
        &mut self,
        bus: &mut B,
        budget: u64,
    ) -> Result<Option<u64>, String> {
        if self.native_find(bus).is_none() {
            return Ok(None);
        }
        let start = self.native_arch();
        let (end_native, log_native, n) = {
            let mut j = JournalBus::new(bus);
            let _ = self.run_native(&mut j, budget);
            let n = self.icount - start.icount;
            (self.native_arch(), j.log, n)
        };
        if n == 0 {
            self.native_restore(&start);
            return Ok(None);
        }
        self.native_restore(&start);
        let mut j = JournalBus::new(bus);
        for _ in 0..n {
            if self.step(&mut j).is_err() {
                break;
            }
        }
        let end_interp = self.native_arch();
        let log_interp = std::mem::take(&mut j.log);
        let mut why = Vec::new();
        let (mut en, ei) = (end_native.clone(), end_interp.clone());
        // `last_exception` is that of the last instruction only; both sides report the same instruction.
        en.last_exception = ei.last_exception;
        if en != ei {
            why.push(format!("state\n  native {en:x?}\n  interp {ei:x?}"));
        }
        if log_native != log_interp {
            let k = log_native
                .iter()
                .zip(&log_interp)
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| log_native.len().min(log_interp.len()));
            why.push(format!(
                "bus log differs at event {k} (native {} events, interp {}): native {:x?} interp {:x?}",
                log_native.len(),
                log_interp.len(),
                log_native.get(k),
                log_interp.get(k)
            ));
        }
        if !why.is_empty() {
            return Err(format!("entry {:#x}: {}", start.pc, why.join("; ")));
        }
        j.commit().map_err(|e| format!("commit {e:?}"))?;
        Ok(Some(n))
    }
}
