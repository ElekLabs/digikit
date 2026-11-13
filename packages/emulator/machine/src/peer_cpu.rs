//! The second ColdFire CPU of a two-CPU board (the peer, finding 128) on a second instance of the ColdFire core.
//!
//! The peer has its own memory map: SDRAM 0x40000000..0x48000000 (private), SRAM 0x80000000..0x80010000, its own peripherals (INTC0,
//! DTIM0, PIT1, eDMA, GPIO, DSPI) and FlexBus chip select 0 at address 0, which is the 128 KB window MAIN sees at 0x10000000. The window
//! is not copied: every peer access to it goes to the `Window` the caller passes in (MAIN's board), so both CPUs see one memory. The two
//! CPUs run in lock step slices on one thread, which makes a run a function of its inputs.
//!
//! Only data transport and CPU-side peripherals are modelled. The FPGA is not: the second bitstream is never loaded or interpreted, and
//! the peer's poll of the FPGA configuration status (a GPIO input) is answered by `inputs`.

use std::collections::BTreeMap;

use coldfire::{Bus, BusError, Cpu, Stop};

/// Size of the shared window (FlexBus CS0 mask 0x10001).
pub const WINDOW_BYTES: u32 = 0x2_0000;
pub const SDRAM_BASE: u32 = 0x4000_0000;
pub const SDRAM_BYTES: usize = 0x0800_0000;
pub const SRAM_BASE: u32 = 0x8000_0000;
pub const SRAM_BYTES: usize = 0x1_0000;
/// Where the program (section 7) is loaded by the stage-1 loader.
pub const PROGRAM_BASE: u32 = 0x4000_0400;
const GPIO_BASE: u32 = 0xEC09_4000;
const INTC0: u32 = 0xFC04_8000;
const EDMA_BASE: u32 = 0xFC04_4000;
const EDMA_SSRT: u32 = EDMA_BASE + 0x1E;
/// The peer's output pin to MAIN: PPDSDR (set) and PCLRR (clear) bytes of GPIO port 6, bit 2.
const PIN_SET: u32 = GPIO_BASE + 0x1e;
const PIN_CLEAR: u32 = GPIO_BASE + 0x2a;
const PIN_BIT: u8 = 4;
const DTIM0: u32 = 0xFC07_0000;
const PIT1: u32 = 0xFC08_4000;
/// Frame tick: DTIM0 raises INTC0 source 32 (vector 96).
pub const FRAME_VECTOR: u32 = 96;

/// The memory of the other CPU's FlexBus window as the peer sees it.
pub trait Window {
    fn read(&mut self, offset: u32, size: u8) -> Option<u32>;
    fn write(&mut self, offset: u32, size: u8, value: u32) -> bool;
}

/// A plain buffer as a window (tests and probes).
impl Window for Vec<u8> {
    fn read(&mut self, offset: u32, size: u8) -> Option<u32> {
        let o = offset as usize;
        let s = self.get(o..o + size as usize)?;
        Some(s.iter().fold(0, |v, b| v << 8 | u32::from(*b)))
    }
    fn write(&mut self, offset: u32, size: u8, value: u32) -> bool {
        let o = offset as usize;
        let Some(s) = self.get_mut(o..o + size as usize) else {
            return false;
        };
        for (i, b) in s.iter_mut().enumerate() {
            *b = (value >> (8 * (size as usize - 1 - i))) as u8;
        }
        true
    }
}

/// One access to a device register, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoAccess {
    pub pc: u32,
    pub addr: u32,
    pub size: u8,
    pub write: bool,
    pub value: u32,
}

/// The peer's private memory and peripherals (everything but the window).
pub struct PeerMem {
    sdram: Vec<u8>,
    sram: Vec<u8>,
    /// Device registers by byte address (reads of a never-written byte are 0).
    io: BTreeMap<u32, u8>,
    /// Bits forced to 1 on a byte read (pin inputs the model stubs, e.g. the FPGA status).
    pub inputs: BTreeMap<u32, u8>,
    /// First accesses to each distinct (address, direction); bounded.
    pub log: Vec<IoAccess>,
    pub log_limit: usize,
    seen: BTreeMap<(u32, bool), u32>,
    /// Accesses to addresses nothing maps (reads give 0, writes are dropped).
    pub faults: Vec<IoAccess>,
    /// The PC of the instruction being executed (for the log).
    pub pc: u32,
    /// Peer instruction count (copy of the core's, for time-based registers).
    pub icount: u64,
    /// eDMA channels started by software since the last service: done at the next bus access.
    dma_start: Vec<usize>,
    pub dma_bytes: u64,
    /// PIT1 reload start (peer instruction count): PIF is set once PMR + 1 ticks (one per instruction) have passed.
    pit_start: u64,
    /// The peer's output pin to MAIN (GPIO port 6, bit 2: set through PPDSDR 0xEC09401E, cleared through PCLRR 0xEC09402A) and its changes
    /// since the last `take_pin_edges`. MAIN's EPORT IRQ4 sees them through the bridge.
    pin: bool,
    pin_edges: u32,
    /// `frame_level` as of the last write to the interrupt controller (read on every step while a frame waits).
    frame_level_cache: Option<u8>,
}

impl PeerMem {
    fn new() -> Self {
        Self {
            sdram: vec![0; SDRAM_BYTES],
            sram: vec![0; SRAM_BYTES],
            io: BTreeMap::new(),
            inputs: BTreeMap::new(),
            log: Vec::new(),
            log_limit: 0,
            seen: BTreeMap::new(),
            faults: Vec::new(),
            pc: 0,
            icount: 0,
            dma_start: Vec::new(),
            dma_bytes: 0,
            pit_start: 0,
            pin: false,
            pin_edges: 0,
            frame_level_cache: None,
        }
    }

    pub fn sdram(&self) -> &[u8] {
        &self.sdram
    }

    pub fn sram(&self) -> &[u8] {
        &self.sram
    }

    fn mem(&self, addr: u32, len: u32) -> Option<&[u8]> {
        let end = addr.checked_add(len)?;
        if addr >= SDRAM_BASE && end <= SDRAM_BASE + SDRAM_BYTES as u32 {
            let o = (addr - SDRAM_BASE) as usize;
            Some(&self.sdram[o..o + len as usize])
        } else if addr >= SRAM_BASE && end <= SRAM_BASE + SRAM_BYTES as u32 {
            let o = (addr - SRAM_BASE) as usize;
            Some(&self.sram[o..o + len as usize])
        } else {
            None
        }
    }

    fn mem_mut(&mut self, addr: u32, len: u32) -> Option<&mut [u8]> {
        let end = addr.checked_add(len)?;
        if addr >= SDRAM_BASE && end <= SDRAM_BASE + SDRAM_BYTES as u32 {
            let o = (addr - SDRAM_BASE) as usize;
            Some(&mut self.sdram[o..o + len as usize])
        } else if addr >= SRAM_BASE && end <= SRAM_BASE + SRAM_BYTES as u32 {
            let o = (addr - SRAM_BASE) as usize;
            Some(&mut self.sram[o..o + len as usize])
        } else {
            None
        }
    }

    fn is_io(addr: u32) -> bool {
        addr >= 0xEC00_0000
    }

    fn note(&mut self, addr: u32, size: u8, write: bool, value: u32) {
        let n = self.seen.entry((addr, write)).or_insert(0);
        *n += 1;
        if *n <= 2 && self.log.len() < self.log_limit {
            self.log.push(IoAccess {
                pc: self.pc,
                addr,
                size,
                write,
                value,
            });
        }
    }

    /// PIT1 PCSR as the guest reads it: PIF (bit 2) is set when the enabled timer has run PMR + 1 ticks since its start.
    fn pit_pcsr(&self) -> u32 {
        let pcsr = u32::from(self.io_byte(PIT1)) << 8 | u32::from(self.io_byte(PIT1 + 1));
        let pmr = u32::from(self.io_byte(PIT1 + 2)) << 8 | u32::from(self.io_byte(PIT1 + 3));
        let due = pcsr & 1 != 0 && self.icount.wrapping_sub(self.pit_start) > u64::from(pmr);
        (pcsr & !4) | if due { 4 } else { 0 }
    }

    fn io_read(&mut self, addr: u32, size: u8) -> u32 {
        if addr == PIT1 && size == 2 {
            let v = self.pit_pcsr();
            self.note(addr, size, false, v);
            return v;
        }
        let mut v = 0u32;
        for i in 0..u32::from(size) {
            let a = addr + i;
            let mut b = self.io.get(&a).copied().unwrap_or(0);
            b |= self.inputs.get(&a).copied().unwrap_or(0);
            v = v << 8 | u32::from(b);
        }
        self.note(addr, size, false, v);
        v
    }

    fn io_write(&mut self, addr: u32, size: u8, value: u32) {
        self.io_write_bytes(addr, size, value);
        if (INTC0..INTC0 + 0x80).contains(&addr) {
            self.frame_level_cache = self.compute_frame_level();
        }
    }

    fn io_write_bytes(&mut self, addr: u32, size: u8, value: u32) {
        self.note(addr, size, true, value);
        if addr == PIT1 && size == 2 {
            // Writing PIF = 1 clears it and, with RLD, restarts the count; enabling starts it.
            let was = self.io_byte(PIT1 + 1) & 1 != 0;
            if value & 4 != 0 || (value & 1 != 0 && !was) {
                self.pit_start = self.icount;
            }
            self.io.insert(PIT1, (value >> 8) as u8);
            self.io.insert(PIT1 + 1, (value as u8) & !4);
            return;
        }
        for i in 0..u32::from(size) {
            let a = addr + i;
            let b = (value >> (8 * (u32::from(size) - 1 - i))) as u8;
            let pin = match a {
                PIN_SET if b & PIN_BIT != 0 => Some(true),
                PIN_CLEAR if b & PIN_BIT == 0 => Some(false),
                _ => None,
            };
            if let Some(level) = pin
                && level != self.pin
            {
                self.pin = level;
                self.pin_edges += 1;
            }
            if a == EDMA_SSRT {
                self.dma_start.push(usize::from(b & 0x3f));
            }
            // DTIM0 DTER (+3) is write-one-to-clear; INTC SIMR/CIMR (+0x1c/+0x1d) set and clear mask bits.
            if a == DTIM0 + 3 {
                let old = self.io.get(&a).copied().unwrap_or(0);
                self.io.insert(a, old & !b);
                continue;
            }
            if a == INTC0 + 0x1c || a == INTC0 + 0x1d {
                let src = u32::from(b & 0x7f);
                let set = a == INTC0 + 0x1c;
                if src == 0x40 {
                    let (h, l) = (INTC0 + 8..INTC0 + 12, INTC0 + 12..INTC0 + 16);
                    for r in h.chain(l) {
                        self.io.insert(r, if set { 0xff } else { 0 });
                    }
                } else {
                    let (byte, bit) = intc_mask_bit(src);
                    let old = self.io.get(&byte).copied().unwrap_or(0);
                    self.io
                        .insert(byte, if set { old | bit } else { old & !bit });
                }
                continue;
            }
            self.io.insert(a, b);
        }
    }

    /// Device register bytes as the guest would read them (no side effects), for tools.
    pub fn io_peek(&self, addr: u32, len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| self.io_byte(addr + i)).collect()
    }

    fn io_byte(&self, addr: u32) -> u8 {
        self.io.get(&addr).copied().unwrap_or(0)
    }

    /// DTIM0 capture event (DTER bit 0): MAIN's doorbell edge reaches the peer's DTIN0 input through the bridge. The handler reads and clears it.
    pub fn dtim0_event(&mut self) {
        let a = DTIM0 + 3;
        let old = self.io_byte(a);
        self.io.insert(a, old | 1);
    }

    /// The interrupt level of the frame vector when the peer has it enabled (ICR set, source unmasked).
    fn frame_level(&self) -> Option<u8> {
        self.frame_level_cache
    }

    fn compute_frame_level(&self) -> Option<u8> {
        let src = FRAME_VECTOR - 64;
        let icr = self.io_byte(INTC0 + 0x40 + src);
        let level = icr & 7;
        // the source's own mask bit, or IMRL bit 0 (mask all)
        let (byte, bit) = intc_mask_bit(src + 64);
        let (all_byte, all_bit) = intc_mask_bit(0);
        let masked = self.io_byte(byte) & bit != 0 || self.io_byte(all_byte) & all_bit != 0;
        (level != 0 && !masked).then_some(level)
    }

    pub fn take_pin_edges(&mut self) -> u32 {
        std::mem::take(&mut self.pin_edges)
    }
}

/// The IMRH/IMRL byte and bit that mask INTC0 source `src` (0..=63; sources 32..=63 are in IMRH, IMRL bit 0 is mask-all).
fn intc_mask_bit(src: u32) -> (u32, u8) {
    let reg = if src >= 32 { INTC0 + 8 } else { INTC0 + 12 };
    (reg + (3 - (src & 31) / 8), 1u8 << (src & 7))
}

/// The bus the core sees: the private memory plus the shared window.
struct PeerBus<'a, W: Window> {
    mem: &'a mut PeerMem,
    window: &'a mut W,
}

impl<W: Window> PeerBus<'_, W> {
    fn read(&mut self, addr: u32, size: u8) -> u32 {
        if addr < WINDOW_BYTES {
            return self.window.read(addr, size).unwrap_or(0);
        }
        if let Some(s) = self.mem.mem(addr, u32::from(size)) {
            return s.iter().fold(0, |v, b| v << 8 | u32::from(*b));
        }
        if PeerMem::is_io(addr) {
            return self.mem.io_read(addr, size);
        }
        self.fault(addr, size, false, 0);
        0
    }

    fn write(&mut self, addr: u32, size: u8, value: u32) {
        if addr < WINDOW_BYTES {
            if !self.window.write(addr, size, value) {
                self.fault(addr, size, true, value);
            }
            return;
        }
        if let Some(s) = self.mem.mem_mut(addr, u32::from(size)) {
            for (i, b) in s.iter_mut().enumerate() {
                *b = (value >> (8 * (size as usize - 1 - i))) as u8;
            }
            return;
        }
        if PeerMem::is_io(addr) {
            self.mem.io_write(addr, size, value);
            self.service_dma();
            return;
        }
        self.fault(addr, size, true, value);
    }

    fn fault(&mut self, addr: u32, size: u8, write: bool, value: u32) {
        if self.mem.faults.len() < 64 {
            self.mem.faults.push(IoAccess {
                pc: self.mem.pc,
                addr,
                size,
                write,
                value,
            });
        }
    }

    /// Software-started eDMA channels run their whole transfer at once (the model has no bus contention): reads TCD, copies
    /// `nbytes * citer` bytes, then sets DONE and clears START.
    fn service_dma(&mut self) {
        while let Some(ch) = self.mem.dma_start.pop() {
            let tcd = EDMA_BASE + 0x1000 + 0x20 * ch as u32;
            let g32 = |m: &PeerMem, o: u32| {
                (0..4).fold(0u32, |v, i| v << 8 | u32::from(m.io_byte(tcd + o + i)))
            };
            let g16 = |m: &PeerMem, o: u32| {
                (0..2).fold(0u32, |v, i| v << 8 | u32::from(m.io_byte(tcd + o + i)))
            };
            let (mut src, mut dst) = (g32(self.mem, 0), g32(self.mem, 0x10));
            let (soff, doff) = (
                g16(self.mem, 6) as u16 as i16,
                g16(self.mem, 0x16) as u16 as i16,
            );
            let attr = g16(self.mem, 4);
            // Transfer sizes are coded 0..5 for 1, 2, 4, 8, 16 and 32 bytes (RM 19.4.17).
            let (ssize, dsize) = (1u32 << ((attr >> 8) & 7).min(5), 1u32 << (attr & 7).min(5));
            let nbytes = g32(self.mem, 8);
            // With channel linking enabled (bit 15) the iteration count has 9 bits; the link makes every minor loop request the next one,
            // so the whole major loop runs from one start.
            let count = |raw: u32| {
                if raw & 0x8000 != 0 {
                    raw & 0x1ff
                } else {
                    raw & 0x7fff
                }
            };
            let mut citer = count(g16(self.mem, 0x14));
            let biter = g16(self.mem, 0x1c);
            let slast = g32(self.mem, 0xc) as i32;
            let dlast = g32(self.mem, 0x18) as i32;
            if nbytes == 0 || citer == 0 || ssize != dsize || !nbytes.is_multiple_of(ssize) {
                continue;
            }
            while citer > 0 {
                for _ in 0..nbytes / ssize {
                    for k in 0..ssize {
                        let v = self.read(src.wrapping_add(k), 1);
                        self.write(dst.wrapping_add(k), 1, v);
                    }
                    src = src.wrapping_add_signed(i32::from(soff));
                    dst = dst.wrapping_add_signed(i32::from(doff));
                }
                self.mem.dma_bytes += u64::from(nbytes);
                citer -= 1;
            }
            src = src.wrapping_add_signed(slast);
            dst = dst.wrapping_add_signed(dlast);
            let put = |m: &mut PeerMem, o: u32, v: u32, n: u32| {
                for i in 0..n {
                    m.io.insert(tcd + o + i, (v >> (8 * (n - 1 - i))) as u8);
                }
            };
            put(self.mem, 0, src, 4);
            put(self.mem, 0x10, dst, 4);
            put(self.mem, 0x14, biter, 2);
            // CSR is a 16-bit register at +0x1e: its low byte (+0x1f) holds START (bit 0) and DONE (bit 7).
            let csr = (g16(self.mem, 0x1e) as u8 & !1) | 0x80;
            put(self.mem, 0x1f, u32::from(csr), 1);
            // The channel's interrupt bit (INTL/INTH), when INT_MAJOR is set.
        }
    }
}

macro_rules! bus_impl {
    () => {
        fn read8(&mut self, a: u32) -> Result<u8, BusError> {
            Ok(self.read(a, 1) as u8)
        }
        fn read16(&mut self, a: u32) -> Result<u16, BusError> {
            Ok(self.read(a, 2) as u16)
        }
        fn read32(&mut self, a: u32) -> Result<u32, BusError> {
            Ok(self.read(a, 4))
        }
        fn write8(&mut self, a: u32, v: u8) -> Result<(), BusError> {
            self.write(a, 1, u32::from(v));
            Ok(())
        }
        fn write16(&mut self, a: u32, v: u16) -> Result<(), BusError> {
            self.write(a, 2, u32::from(v));
            Ok(())
        }
        fn write32(&mut self, a: u32, v: u32) -> Result<(), BusError> {
            self.write(a, 4, v);
            Ok(())
        }
    };
}

impl<W: Window> Bus for PeerBus<'_, W> {
    bus_impl!();
}

/// Why a peer slice ended early.
#[derive(Debug, PartialEq, Eq)]
pub enum PeerStop {
    Halted,
    Unimplemented(String),
}

pub struct PeerCpu {
    pub cpu: Cpu,
    pub mem: PeerMem,
    /// Frame interrupts raised and delivered.
    pub frames_raised: u64,
    pub frames_taken: u64,
    frame_pending: bool,
    /// Instructions run inside the frame handler (from the interrupt to the RTE that restores the mask), summed and the largest.
    pub isr_instructions: u64,
    pub isr_max: u64,
    isr: Option<(u16, u64)>,
    /// Where the startup clear loop was found: after that only this address is tested (the scan reads memory at every step).
    clear_pc: Option<u32>,
    /// Instructions inside the frame handler by 64-byte code bucket (`DIGI_PEER_PROFILE=1`), for finding where its time goes.
    pub isr_profile: Option<BTreeMap<u32, u64>>,
}

impl Default for PeerCpu {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerCpu {
    pub fn new() -> Self {
        let mut mem = PeerMem::new();
        // The FPGA configuration status the peer polls after loading its bitstream: GPIO 0xEC09401C bit 0 reads 1 (DONE). Stub of one
        // status bit only; no FPGA logic.
        mem.inputs.insert(GPIO_BASE + 0x1c, 1);
        // DONE of the same configuration (bit 7 of the next byte), and the configuration DSPI's transfer-complete and end-of-queue flags: the
        // bits the peer polls after it pushed the bitstream words (the words are not interpreted).
        mem.inputs.insert(GPIO_BASE + 0x1d, 0x80);
        mem.inputs.insert(0xFC03_C02C, 0x90);
        Self {
            cpu: Cpu::new(),
            mem,
            frames_raised: 0,
            frames_taken: 0,
            frame_pending: false,
            isr_instructions: 0,
            isr_max: 0,
            isr: None,
            clear_pc: None,
            isr_profile: Self::profile_requested().then(BTreeMap::new),
        }
    }

    fn profile_requested() -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::env::var_os("DIGI_PEER_PROFILE").is_some_and(|v| v != "0")
        }
    }

    /// Loads the program at `PROGRAM_BASE` and starts at its entry (the first long of the image), as the stage-1 loader does with
    /// `jmp *(0x40000400)`. The stage-1 stack lives in SRAM; the program reads one stack argument at entry and switches to its own stack.
    /// MAIN supplies the image, so a short or oversized one is an error, not a panic.
    pub fn start(&mut self, program: &[u8]) -> Result<(), String> {
        let o = (PROGRAM_BASE - SDRAM_BASE) as usize;
        let Some(entry) = program.first_chunk::<4>().map(|e| u32::from_be_bytes(*e)) else {
            return Err(format!(
                "peer program image is {} bytes: it needs at least the 4-byte entry",
                program.len()
            ));
        };
        let Some(dest) = self.mem.sdram.get_mut(o..o + program.len()) else {
            return Err(format!(
                "peer program image of {} bytes does not fit in the peer SDRAM",
                program.len()
            ));
        };
        dest.copy_from_slice(program);
        self.cpu = Cpu::new();
        self.clear_pc = None;
        self.cpu.pc = entry;
        self.cpu.a[7] = SRAM_BASE + SRAM_BYTES as u32 - 0x40;
        self.cpu.sr = 0x2700;
        // The stage-1 loader raised the pin once before it waited for MAIN (MAIN's IRQ4 #1, queued by the mailbox model when the stream
        // ended); the program's first toggle therefore clears it.
        self.mem.pin = true;
        Ok(())
    }

    /// A frame tick (DTIM0 event): the program's ISR runs when it has enabled the interrupt.
    pub fn frame_tick<W: Window>(&mut self, _window: &mut W) {
        self.mem.dtim0_event();
        self.frames_raised += 1;
        self.frame_pending = true;
    }

    /// Runs `n` instructions (the count of the core, so an accelerated loop counts what it replaces).
    pub fn run<W: Window>(&mut self, window: &mut W, n: u64) -> Result<(), PeerStop> {
        let end = self.cpu.icount + n;
        while self.cpu.icount < end {
            if self.frame_pending
                && let Some(level) = self.mem.frame_level()
                && ((self.cpu.sr >> 8) & 7) < u16::from(level)
            {
                let mut bus = PeerBus {
                    mem: &mut self.mem,
                    window,
                };
                let taken = self.cpu.take_interrupt(
                    &mut bus,
                    FRAME_VECTOR as u8,
                    Some(level),
                    coldfire::InterruptPolicy::Device,
                );
                match taken {
                    Ok(true) => {
                        self.frame_pending = false;
                        self.frames_taken += 1;
                        self.isr = Some((u16::from(level), self.cpu.icount));
                    }
                    Ok(false) => {}
                    Err(_) => return Err(PeerStop::Halted),
                }
            }
            if self.clear_loop(end) {
                continue;
            }
            self.mem.pc = self.cpu.pc;
            self.mem.icount = self.cpu.icount;
            if self.isr.is_some()
                && let Some(profile) = self.isr_profile.as_mut()
            {
                *profile.entry(self.cpu.pc & !0x3f).or_default() += 1;
            }
            let mut bus = PeerBus {
                mem: &mut self.mem,
                window,
            };
            match self.cpu.step(&mut bus) {
                Ok(()) => {}
                Err(Stop::Halted) => return Err(PeerStop::Halted),
                Err(Stop::Unimplemented(f)) => {
                    return Err(PeerStop::Unimplemented(format!("{f:?}")));
                }
            }
            if let Some((level, start)) = self.isr
                && ((self.cpu.sr >> 8) & 7) < level
            {
                let cost = self.cpu.icount - start;
                self.isr_instructions += cost;
                self.isr_max = self.isr_max.max(cost);
                self.isr = None;
            }
        }
        Ok(())
    }

    /// The byte-clear loop of the program's startup (`clr.b (a0)+; cmp.l a0,d0; bne.b` over 64 MB of SDRAM that is zero already) costs three
    /// instructions per byte. When the core stands on it, clear all but the last iteration in one go and count the instructions it
    /// replaces; the last iteration runs on the core, so the flags are its own. Recognised by the three instruction words, not by address.
    fn clear_loop(&mut self, end: u64) -> bool {
        const LOOP: [u8; 6] = [0x42, 0x18, 0xb0, 0x88, 0x66, 0xfa];
        let (a0, d0, pc) = (self.cpu.a[0], self.cpu.d[0], self.cpu.pc);
        if self.clear_pc.is_some_and(|known| known != pc) {
            return false;
        }
        if self.mem.mem(pc, 6) != Some(&LOOP[..]) || a0 >= d0 || self.mem.mem(a0, d0 - a0).is_none()
        {
            return false;
        }
        self.clear_pc = Some(pc);
        let k = u64::from(d0 - a0 - 1).min((end - self.cpu.icount) / 3);
        if k == 0 {
            return false;
        }
        if let Some(bytes) = self.mem.mem_mut(a0, k as u32) {
            bytes.fill(0);
        }
        self.cpu.a[0] = a0 + k as u32;
        self.cpu.icount += 3 * k;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program image: the entry long, then `code` at `PROGRAM_BASE + 4`.
    fn image(code: &[u8]) -> Vec<u8> {
        let mut v = (PROGRAM_BASE + 4).to_be_bytes().to_vec();
        v.extend_from_slice(code);
        v
    }

    // ColdFire has no immediate-to-absolute-long move: each helper loads D0 and stores it (two instructions).
    fn movel_imm_abs(imm: u32, abs: u32) -> Vec<u8> {
        let mut v = vec![0x20, 0x3c];
        v.extend_from_slice(&imm.to_be_bytes());
        v.extend_from_slice(&[0x23, 0xc0]);
        v.extend_from_slice(&abs.to_be_bytes());
        v
    }

    fn movew_imm_abs(imm: u16, abs: u32) -> Vec<u8> {
        let mut v = vec![0x30, 0x3c];
        v.extend_from_slice(&imm.to_be_bytes());
        v.extend_from_slice(&[0x33, 0xc0]);
        v.extend_from_slice(&abs.to_be_bytes());
        v
    }

    fn moveb_imm_abs(imm: u8, abs: u32) -> Vec<u8> {
        let mut v = vec![0x10, 0x3c, 0x00, imm, 0x13, 0xc0];
        v.extend_from_slice(&abs.to_be_bytes());
        v
    }

    #[test]
    fn the_byte_clear_loop_ends_where_the_core_would_with_the_same_count() {
        let (start, count) = (0x4010_0000u32, 5000u32);
        let mut code = vec![0x20, 0x7c];
        code.extend_from_slice(&start.to_be_bytes()); // movea.l #start, a0
        code.extend_from_slice(&[0x20, 0x3c]);
        code.extend_from_slice(&(start + count).to_be_bytes()); // move.l #end, d0
        code.extend_from_slice(&[0x42, 0x18, 0xb0, 0x88, 0x66, 0xfa]); // clr.b (a0)+; cmp.l a0,d0; bne.b
        code.extend_from_slice(&[0x60, 0xfe]); // bra.b .
        let mut peer = PeerCpu::new();
        peer.start(&image(&code)).unwrap();
        let o = (start - SDRAM_BASE) as usize;
        peer.mem.sdram[o..o + count as usize + 4].fill(0xff);
        let mut window = vec![0u8; WINDOW_BYTES as usize];
        // Two setup instructions, three per byte, then the branch is the next instruction.
        peer.run(&mut window, 2 + 3 * u64::from(count)).unwrap();
        assert_eq!(peer.cpu.icount, 2 + 3 * u64::from(count));
        assert_eq!(peer.cpu.a[0], start + count);
        assert!(
            peer.mem.sdram[o..o + count as usize]
                .iter()
                .all(|b| *b == 0)
        );
        assert_eq!(
            peer.mem.sdram[o + count as usize],
            0xff,
            "one byte past the range is untouched"
        );
        let spin = PROGRAM_BASE + 4 + 6 + 6 + 6;
        assert_eq!(peer.cpu.pc, spin);
    }

    #[test]
    fn software_started_dma_copies_the_major_loop_into_the_shared_window() {
        let ch47 = 0xFC04_55E0;
        let mut code = vec![];
        code.extend(movel_imm_abs(SRAM_BASE + 0x100, ch47)); // SADDR
        code.extend(movew_imm_abs(0x0202, ch47 + 4)); // ATTR: 32-bit both
        code.extend(movew_imm_abs(4, ch47 + 6)); // SOFF
        code.extend(movel_imm_abs(4, ch47 + 8)); // NBYTES
        code.extend(movel_imm_abs(0x100, ch47 + 0x10)); // DADDR: window + 0x100
        code.extend(movew_imm_abs(0x8008, ch47 + 0x14)); // CITER: linked, 8 loops
        code.extend(movew_imm_abs(4, ch47 + 0x16)); // DOFF
        code.extend(movew_imm_abs(0x8008, ch47 + 0x1c)); // BITER
        code.extend(moveb_imm_abs(47, EDMA_SSRT)); // start channel 47
        code.extend_from_slice(&[0x60, 0xfe]);
        let mut peer = PeerCpu::new();
        peer.start(&image(&code)).unwrap();
        for i in 0..32u8 {
            peer.mem.sram[0x100 + usize::from(i)] = i + 1;
        }
        let mut window = vec![0u8; WINDOW_BYTES as usize];
        peer.run(&mut window, 40).unwrap();
        let expect: Vec<u8> = (1..=32).collect();
        assert_eq!(&window[0x100..0x120], &expect[..]);
        assert_eq!(window[0x120], 0);
        assert_eq!(peer.mem.dma_bytes, 32);
        // DONE (CSR bit 7) is set and the iteration count is reloaded from BITER.
        assert_eq!(peer.mem.io_peek(ch47 + 0x1f, 1)[0] & 0x81, 0x80);
        assert_eq!(peer.mem.io_peek(ch47 + 0x14, 2), [0x80, 0x08]);
    }

    #[test]
    fn the_frame_source_mask_is_the_same_bit_for_the_writer_and_the_reader() {
        let mut mem = PeerMem::new();
        mem.io_write(INTC0 + 0x40 + 32, 1, 3); // ICR32 level 3
        mem.io_write(INTC0 + 0x1d, 1, 32); // CIMR: unmask source 32
        assert_eq!(mem.frame_level(), Some(3));
        mem.io_write(INTC0 + 0x1c, 1, 32); // SIMR: mask source 32
        assert_eq!(mem.frame_level(), None, "masked through SIMR");
        mem.io_write(INTC0 + 8, 4, 0xFFFF_FFFE); // IMRH: source 32 unmasked, the rest masked
        assert_eq!(mem.frame_level(), Some(3), "unmasked through IMRH");
        mem.io_write(INTC0 + 8, 4, 0x0000_0001); // IMRH: only source 32 masked
        assert_eq!(mem.frame_level(), None);
    }

    #[test]
    fn a_short_or_oversized_image_is_an_error_not_a_panic() {
        let mut peer = PeerCpu::new();
        assert!(
            peer.start(&[0x40, 0x00])
                .unwrap_err()
                .contains("4-byte entry")
        );
        assert!(peer.start(&[]).is_err());
        let big = vec![0u8; SDRAM_BYTES];
        assert!(peer.start(&big).unwrap_err().contains("does not fit"));
        peer.start(&[0x40, 0x00, 0x04, 0x04]).unwrap();
    }

    #[test]
    fn the_output_pin_edges_are_level_changes_and_start_high() {
        let mut code = vec![];
        code.extend(moveb_imm_abs(0xfb, PIN_CLEAR)); // pin low: an edge (stage 1 left it high)
        code.extend(moveb_imm_abs(0xfb, PIN_CLEAR)); // still low: no edge
        code.extend(moveb_imm_abs(4, PIN_SET)); // high: an edge
        code.extend_from_slice(&[0x60, 0xfe]);
        let mut peer = PeerCpu::new();
        peer.start(&image(&code)).unwrap();
        let mut window = vec![0u8; WINDOW_BYTES as usize];
        peer.run(&mut window, 6).unwrap();
        assert_eq!(peer.mem.take_pin_edges(), 2);
        assert_eq!(peer.mem.take_pin_edges(), 0);
    }

    #[test]
    fn the_frame_tick_runs_the_installed_handler_and_it_costs_what_it_runs() {
        // Handler at HANDLER: clear DTER (byte 1 to DTIM0+3), then rte. Main code: VBR := table, vector 96 := HANDLER, ICR32 := 3, unmask
        // source 32 with CIMR, enable interrupts, spin.
        let table = SRAM_BASE + 0x400;
        let handler = PROGRAM_BASE + 0x200;
        let mut code = vec![];
        code.extend(movel_imm_abs(handler, table + 4 * FRAME_VECTOR));
        code.extend_from_slice(&[0x20, 0x3c]); // move.l #table, d0
        code.extend_from_slice(&table.to_be_bytes());
        code.extend_from_slice(&[0x4e, 0x7b, 0x08, 0x01]); // movec d0, vbr
        code.extend(moveb_imm_abs(3, INTC0 + 0x40 + 32)); // ICR32 level 3
        code.extend(moveb_imm_abs(32, INTC0 + 0x1d)); // CIMR: unmask source 32
        code.extend_from_slice(&[0x46, 0xfc, 0x20, 0x00]); // move.w #0x2000, sr
        code.extend_from_slice(&[0x60, 0xfe]);
        let mut image = image(&code);
        image.resize(0x200, 0);
        // The handler saves nothing: moveq #1, d0 ; move.b d0, DTER (write one clears) ; rte.
        image.extend_from_slice(&[0x70, 0x01, 0x13, 0xc0, 0xfc, 0x07, 0x00, 0x03, 0x4e, 0x73]);
        let mut peer = PeerCpu::new();
        peer.start(&image).unwrap();
        let mut window = vec![0u8; WINDOW_BYTES as usize];
        peer.run(&mut window, 30).unwrap();
        assert_eq!(peer.frames_taken, 0, "no tick yet");
        peer.frame_tick(&mut window);
        assert_eq!(
            peer.mem.io_peek(DTIM0 + 3, 1),
            [1],
            "the tick sets the capture flag"
        );
        peer.run(&mut window, 20).unwrap();
        assert_eq!(peer.frames_taken, 1);
        assert_eq!(
            peer.mem.io_peek(DTIM0 + 3, 1),
            [0],
            "the handler cleared it"
        );
        assert_eq!(
            peer.isr_max, 3,
            "the handler is moveq, a byte move and an rte"
        );
        assert_eq!(peer.isr_instructions, 3);
    }
}
