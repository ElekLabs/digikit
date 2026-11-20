//! The device bootstrap (OS section 2) on the ColdFire core, with just enough board to run it.
//!
//! The bootstrap is the code that the MCF5441x serial boot facility loads from SPI flash into
//! SRAM at 0x8000_0400. It tests the front-panel keys, offers the STARTUP menu (MIDI DIN OS
//! upgrade), checks the upgrade container (content checksum, HMAC), writes it to flash, and
//! otherwise loads MAIN OS from the flash container and jumps to it.
//!
//! This model runs the real bootstrap instructions. What is modelled instead of executed:
//!
//! - the QSPI flash transport: the SRAM flash-read routine, the flash-id routine, the sector
//!   erase and the page program are replaced at their entry points (see [`Board::hook`]) by a
//!   16 MiB NOR flash array with NOR semantics (erase to 0xFF, program ANDs);
//! - the panel UART (UART2): key bank dumps, version replies and the OLED record stream;
//! - the MIDI DIN UART (UART3): bytes of a SysEx file, one receive interrupt per byte;
//! - PLL lock, DDR controller "initialised" and PIT "event" status bits.
//!
//! Everything else reads zero and ignores writes. Unmapped accesses are counted, never fatal.
//! Timing is not modelled: delays return at once.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Instant;

use coldfire::{Bus, BusError, Cpu, InterruptPolicy, Stop};

pub const SRAM_BASE: u32 = 0x8000_0000;
pub const SRAM_SIZE: u32 = 0x2_0000;
pub const BOOT_LOAD: u32 = 0x8000_0400;
pub const DDR_BASE: u32 = 0x4000_0000;
pub const DDR_SIZE: u32 = 0x400_0000;
pub const FLAG_BASE: u32 = 0x4800_0000;
pub const FLASH_SIZE: usize = 0x100_0000;
pub const CONTAINER_FLASH_OFFSET: usize = 0x8_0000;
/// MAIN OS is depacked to 0x4000_0400 (`[patch] main_dest` of the Digitakt II file, the device this runner models) and runs below the 4 MiB boundary `MAIN_END`: a pc in
/// `MAIN_LOAD..MAIN_END` is MAIN. `MAIN_END` is where MAIN's image may end, not the bootstrap's stage (that is `[patch] main_stage`, which differs per device, finding 152); the runner
/// stages nothing itself, the bootstrap's own code does. The runner is for the Digitakt II recovery only.
pub const MAIN_LOAD: u32 = 0x4000_0400;
pub const MAIN_END: u32 = 0x4040_0000;

const UART2: u32 = 0xec07_0000; // panel
const UART3: u32 = 0xec07_4000; // MIDI DIN
const PANEL_VECTOR: u8 = 180;
const MIDI_VECTOR: u8 = 181;
const IRQ_LEVEL: u8 = 6;

/// Entry points replaced by the model (bootstrap 1.16 / Digitakt II; addresses are checked
/// against the loaded image by [`Machine::new`]).
pub const HOOK_FLASH_READ: u32 = 0x8000_f010; // (addr, len, dst); copy of the routine at 0x80007910
pub const HOOK_FLASH_ID: u32 = 0x8000_24ec;
pub const HOOK_QSPI_INIT: u32 = 0x8000_240a;
pub const HOOK_SECTOR_ERASE: u32 = 0x8000_2714; // (addr)
pub const HOOK_PAGE_PROGRAM: u32 = 0x8000_227e; // (addr, len, src)
pub const HOOK_STATUS_CMD: u32 = 0x8000_2742; // (addr, cmd) status-register style command
const FLASH_VARS: [(u32, u32); 3] = [
    (0x8000_64c0, 0x200),
    (0x8000_64bc, 0x4_0000),
    (0x8000_7d04, 1),
];

/// Bytes of a (command, argument count) panel framing; unknown commands take no argument.
fn panel_arg_len(cmd: u8) -> usize {
    match cmd {
        0x10..=0x17 => 9,
        0xb9 => 2,
        0xb5 => 4,
        0xbb => 3,
        0xb7 | 0xbc | 0xba | 0x40..=0x4f | 0x60 | 0x70 | 0x71 | 0x51 | 0xf0 => 1,
        _ => 0,
    }
}

#[derive(Default)]
struct Uart {
    rx_queue: VecDeque<u8>,
    rx_cur: Option<u8>,
    rx_irq_enabled: bool,
    tx: Vec<u8>,
}

impl Uart {
    fn status(&self) -> u8 {
        0x0c | u8::from(self.rx_cur.is_some())
    }
}

#[derive(Default, Clone, Debug)]
pub struct FlashLog {
    pub erased_sectors: Vec<u32>,
    pub programmed_bytes: u64,
    pub program_calls: u64,
    pub min_addr: Option<u32>,
    pub max_end: u32,
    /// Programs that tried to set a bit from 0 to 1 (not erased first).
    pub not_erased: u64,
}

pub struct Board {
    ddr: Vec<u8>,
    flag: Vec<u8>,
    sram: Vec<u8>,
    mmio: HashMap<u32, u8>,
    pub flash: Vec<u8>,
    pub flash_log: FlashLog,
    panel: Uart,
    midi: Uart,
    midi_source: VecDeque<u8>,
    /// INTC1 source masks (SIMR/CIMR writes): sources 52 (panel UART) and 53 (MIDI UART).
    panel_unmasked: bool,
    midi_unmasked: bool,
    pub midi_fed: u64,
    panel_cmd: Vec<u8>,
    /// Key bank states (7 banks of 8 keys), as the panel reports them.
    pub keys: [u8; 7],
    key_events: bool,
    pub reset_requested: bool,
    pub unmapped: BTreeMap<u32, u64>,
    pub fetch_fault: Option<u32>,
    key_index: Option<[u32; 64]>,
    /// All bytes the guest sent to the panel UART.
    pub panel_tx: Vec<u8>,
    pub panel_commands: Vec<(u8, u8)>,
}

impl Board {
    fn new(flash: Vec<u8>) -> Board {
        Board {
            ddr: vec![0; DDR_SIZE as usize],
            flag: vec![0; 0x1000],
            sram: vec![0; SRAM_SIZE as usize],
            mmio: HashMap::new(),
            flash,
            flash_log: FlashLog::default(),
            panel: Uart::default(),
            midi: Uart::default(),
            midi_source: VecDeque::new(),
            panel_unmasked: false,
            midi_unmasked: false,
            midi_fed: 0,
            panel_cmd: Vec::new(),
            keys: [0; 7],
            key_events: false,
            reset_requested: false,
            unmapped: BTreeMap::new(),
            fetch_fault: None,
            key_index: None,
            panel_tx: Vec::new(),
            panel_commands: Vec::new(),
        }
    }

    fn region(&mut self, addr: u32) -> Option<&mut u8> {
        if (DDR_BASE..DDR_BASE + DDR_SIZE).contains(&addr) {
            Some(&mut self.ddr[(addr - DDR_BASE) as usize])
        } else if (SRAM_BASE..SRAM_BASE + SRAM_SIZE).contains(&addr) {
            Some(&mut self.sram[(addr - SRAM_BASE) as usize])
        } else if (FLAG_BASE..FLAG_BASE + 0x1000).contains(&addr) {
            Some(&mut self.flag[(addr - FLAG_BASE) as usize])
        } else {
            None
        }
    }

    fn is_mmio(addr: u32) -> bool {
        matches!(addr >> 24, 0xec | 0xfc)
    }

    fn note_unmapped(&mut self, addr: u32) {
        *self.unmapped.entry(addr & !0xfff).or_insert(0) += 1;
    }

    fn mmio_read(&mut self, addr: u32) -> u8 {
        match addr {
            // PLL lock (bit 4 of the 32-bit register at 0xfc0c0008).
            0xfc0c_000b => return 0x10,
            // DDR controller status: initialisation complete (bit 3 at 0xfc0b806c).
            0xfc0b_806f => return 0x08,
            // PIT0 PCSR: event flag (bit 2) always set so delay loops end.
            0xfc08_4001 => return 0x04,
            a if a == UART2 + 4 => return self.panel.status(),
            a if a == UART3 + 4 => return self.midi.status(),
            a if a == UART2 + 0x0c => return self.panel.rx_cur.take().unwrap_or(0),
            a if a == UART3 + 0x0c => return self.midi.rx_cur.take().unwrap_or(0),
            _ => {}
        }
        self.mmio.get(&addr).copied().unwrap_or(0)
    }

    fn mmio_write(&mut self, addr: u32, v: u8) {
        if addr == UART2 + 0x0c {
            self.panel_byte(v);
            return;
        }
        if addr == UART3 + 0x0c {
            self.midi.tx.push(v);
            return;
        }
        if addr == UART2 + 0x14 {
            self.panel.rx_irq_enabled = v & 2 != 0;
        }
        if addr == UART3 + 0x14 {
            self.midi.rx_irq_enabled = v & 2 != 0;
        }
        // INTC SIMR (0xfc04c01c, mask) and CIMR (0xfc04c01d, unmask): source number or bit 6 = all.
        if addr == 0xfc04_c01c || addr == 0xfc04_c01d {
            let on = addr & 1 == 1;
            if v & 0x40 != 0 {
                (self.panel_unmasked, self.midi_unmasked) = (on, on);
            } else if v == 0x34 {
                self.panel_unmasked = on;
            } else if v == 0x35 {
                self.midi_unmasked = on;
            }
        }
        // RCR (0xec090000): software reset request, bit 7.
        if addr == 0xec09_0000 && v & 0x80 != 0 {
            self.reset_requested = true;
        }
        self.mmio.insert(addr, v);
    }

    /// A byte from the guest to the panel: framed into commands; the panel answers the key-bank
    /// dump request (0x60 0x01) and the version request (0x70/0x71 0x00).
    fn panel_byte(&mut self, v: u8) {
        self.panel_tx.push(v);
        self.panel_cmd.push(v);
        let need = 1 + panel_arg_len(self.panel_cmd[0]);
        if self.panel_cmd.len() < need {
            return;
        }
        let cmd = std::mem::take(&mut self.panel_cmd);
        let arg = cmd.get(1).copied().unwrap_or(0);
        if !(0x10..=0x17).contains(&cmd[0]) {
            self.panel_commands.push((cmd[0], arg));
        }
        match (cmd[0], arg) {
            (0x60, 1) => {
                self.key_events = true;
                for bank in 0..7u8 {
                    self.panel.rx_queue.push_back(0x20 | bank);
                    self.panel.rx_queue.push_back(self.keys[bank as usize]);
                }
            }
            (0x60, 2) => self.key_events = false,
            (0x70 | 0x71, 0) => self.panel.rx_queue.extend([0x70, 0x00, 0x01, 0x10, 0x00]),
            _ => {}
        }
    }

    /// Press or release a key by its panel key code. Needs the bootstrap's key table.
    pub fn key(&mut self, code: u8, down: bool) -> Result<(), String> {
        let table = self.key_index.ok_or("key table not loaded")?;
        let idx = *table.get(code as usize).ok_or("key code out of range")?;
        if idx >= 56 {
            return Err(format!("key code {code:#x} has no panel position"));
        }
        let (bank, bit) = ((idx >> 3) as usize, idx & 7);
        let before = self.keys[bank];
        if down {
            self.keys[bank] |= 1 << bit;
        } else {
            self.keys[bank] &= !(1 << bit);
        }
        if self.keys[bank] != before && self.key_events {
            self.panel.rx_queue.push_back(0x20 | bank as u8);
            self.panel.rx_queue.push_back(self.keys[bank]);
        }
        Ok(())
    }

    fn load_key_table(&mut self) {
        let mut table = [0u32; 64];
        for (code, slot) in table.iter_mut().enumerate() {
            let a = 0x8000_6d74 + 4 * code as u32;
            *slot = (0..4).fold(0u32, |v, i| {
                v << 8 | self.sram[(a + i - SRAM_BASE) as usize] as u32
            });
        }
        self.key_index = Some(table);
    }

    fn rd(&mut self, addr: u32) -> u8 {
        if Self::is_mmio(addr) {
            return self.mmio_read(addr);
        }
        if let Some(b) = self.region(addr) {
            return *b;
        }
        self.note_unmapped(addr);
        0
    }

    fn wr(&mut self, addr: u32, v: u8) {
        if Self::is_mmio(addr) {
            self.mmio_write(addr, v);
        } else if let Some(b) = self.region(addr) {
            *b = v;
        } else {
            self.note_unmapped(addr);
        }
    }

    pub fn read_mem(&mut self, addr: u32, len: usize) -> Vec<u8> {
        (0..len as u32)
            .map(|i| self.rd(addr.wrapping_add(i)))
            .collect()
    }

    pub fn write_mem(&mut self, addr: u32, data: &[u8]) {
        for (i, b) in data.iter().enumerate() {
            self.wr(addr.wrapping_add(i as u32), *b);
        }
    }

    fn read_u32(&mut self, addr: u32) -> u32 {
        (0..4).fold(0u32, |v, i| v << 8 | self.rd(addr + i) as u32)
    }

    fn write_u32(&mut self, addr: u32, v: u32) {
        self.write_mem(addr, &v.to_be_bytes());
    }

    fn flash_read(&self, addr: u32, len: u32) -> Vec<u8> {
        (0..len)
            .map(|i| self.flash.get((addr + i) as usize).copied().unwrap_or(0xff))
            .collect()
    }

    fn flash_erase(&mut self, addr: u32) {
        let base = addr & !0x3_ffff;
        self.flash_log.erased_sectors.push(base);
        self.note_flash_write(base, 0x4_0000);
        let end = ((base + 0x4_0000) as usize).min(self.flash.len());
        self.flash[base as usize..end].fill(0xff);
    }

    fn flash_program(&mut self, addr: u32, data: &[u8]) {
        self.flash_log.program_calls += 1;
        self.flash_log.programmed_bytes += data.len() as u64;
        self.note_flash_write(addr, data.len() as u32);
        for (i, b) in data.iter().enumerate() {
            if let Some(f) = self.flash.get_mut(addr as usize + i) {
                if *b & !*f != 0 {
                    self.flash_log.not_erased += 1;
                }
                *f &= *b;
            }
        }
    }

    fn note_flash_write(&mut self, addr: u32, len: u32) {
        let log = &mut self.flash_log;
        log.min_addr = Some(log.min_addr.map_or(addr, |m| m.min(addr)));
        log.max_end = log.max_end.max(addr + len);
    }
}

impl Bus for Board {
    fn read8(&mut self, addr: u32) -> Result<u8, BusError> {
        Ok(self.rd(addr))
    }
    fn read16(&mut self, addr: u32) -> Result<u16, BusError> {
        Ok((self.rd(addr) as u16) << 8 | self.rd(addr.wrapping_add(1)) as u16)
    }
    fn read32(&mut self, addr: u32) -> Result<u32, BusError> {
        Ok(self.read_u32(addr))
    }
    fn write8(&mut self, addr: u32, v: u8) -> Result<(), BusError> {
        self.wr(addr, v);
        Ok(())
    }
    fn write16(&mut self, addr: u32, v: u16) -> Result<(), BusError> {
        self.wr(addr, (v >> 8) as u8);
        self.wr(addr.wrapping_add(1), v as u8);
        Ok(())
    }
    fn write32(&mut self, addr: u32, v: u32) -> Result<(), BusError> {
        self.write_u32(addr, v);
        Ok(())
    }
    fn fetch16(&mut self, addr: u32) -> Result<u16, BusError> {
        let mapped = (DDR_BASE..DDR_BASE + DDR_SIZE).contains(&addr)
            || (SRAM_BASE..SRAM_BASE + SRAM_SIZE).contains(&addr);
        if !mapped {
            self.fetch_fault.get_or_insert(addr);
            return Err(BusError { addr, write: false });
        }
        self.read16(addr)
    }
}

/// Why a run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// Instruction limit.
    Limit,
    /// Wall-clock limit.
    Seconds,
    /// A stop address was reached.
    StopPc(u32),
    /// Control entered MAIN OS (0x4000_0400..0x4040_0000), as `jsr (*0x40000400)` does.
    MainOs(u32),
    /// The CPU took a fault exception (vector below 64) or fetched from unmapped memory.
    Fault {
        vector: u8,
        pc: u32,
    },
    Halted,
    Unimplemented(String),
    /// The guest asked for a soft reset (RCR bit 7) and no reboot was allowed.
    Reset,
}

impl End {
    pub fn name(&self) -> String {
        match self {
            End::Limit => "limit".into(),
            End::Seconds => "seconds".into(),
            End::StopPc(p) => format!("stop_pc:{p:#x}"),
            End::MainOs(p) => format!("main_os:{p:#x}"),
            End::Fault { vector, pc } => format!("fault:vector{vector}@{pc:#x}"),
            End::Halted => "halted".into(),
            End::Unimplemented(f) => format!("unimplemented:{f}"),
            End::Reset => "reset".into(),
        }
    }
}

/// A key press or release when the guest first reaches `pc`.
#[derive(Clone, Debug)]
pub struct PcKey {
    pub pc: u32,
    pub code: u8,
    pub down: bool,
}

pub struct Machine {
    pub cpu: Cpu,
    pub board: Board,
    boot_body: Vec<u8>,
    /// Boot mode argument (1 = normal boot).
    pub mode: u32,
    watch: Vec<bool>,
    pub pc_hits: BTreeMap<u32, (u64, u64)>,
    pub stop_pcs: Vec<u32>,
    pub pc_keys: Vec<PcKey>,
    /// Keep running when control enters MAIN OS (to see it fault) instead of stopping.
    pub run_main: bool,
    pub icount: u64,
    pub first_exception: Option<(u8, u32, u64)>,
    pub main_os_entry: Option<(u32, u64)>,
}

impl Machine {
    /// `boot` is the extracted section 2 file (4-byte length word, then the image that loads at
    /// 0x8000_0400, its first two longwords being the initial SP and PC). `flash` is the 16 MiB
    /// NOR array.
    pub fn new(boot: &[u8], flash: Vec<u8>) -> Result<Machine, String> {
        if boot.len() < 12 || boot.len() - 4 > (SRAM_SIZE - 0x400) as usize {
            return Err("bootstrap image has a bad size".into());
        }
        if flash.len() != FLASH_SIZE {
            return Err(format!("flash image must be {FLASH_SIZE} bytes"));
        }
        // The hook sites are those of the Digitakt II 1.16/1.17 bootstrap (version 0x0201).
        const SITES: [(u32, [u8; 8]); 7] = [
            (
                0x8000_0de8,
                [0x30, 0x39, 0xec, 0x09, 0x00, 0x0e, 0x59, 0x8f],
            ),
            (
                0x8000_240a,
                [0x70, 0x17, 0x13, 0xc0, 0xfc, 0x04, 0x00, 0x2d],
            ),
            (
                0x8000_24ec,
                [0x4e, 0x56, 0xff, 0xf8, 0x70, 0xff, 0x2f, 0x02],
            ),
            (
                0x8000_2714,
                [0x2f, 0x0a, 0x4e, 0xba, 0xfa, 0xd0, 0x2f, 0x2f],
            ),
            (
                0x8000_227e,
                [0x4f, 0xef, 0xff, 0xf4, 0x48, 0xd7, 0x04, 0x0c],
            ),
            (
                0x8000_2742,
                [0x4f, 0xef, 0xff, 0xf4, 0x48, 0xd7, 0x04, 0x0c],
            ),
            (
                0x8000_7910,
                [0x20, 0x39, 0xfc, 0x05, 0xc0, 0x00, 0x2f, 0x03],
            ),
        ];
        if boot.get(4 + 8..4 + 10) != Some(&[0x02, 0x01][..]) {
            return Err("unsupported bootstrap: version word at 0x80000408 is not 0x0201".into());
        }
        for (addr, want) in SITES {
            let off = (addr - BOOT_LOAD) as usize + 4;
            if boot.get(off..off + 8) != Some(&want[..]) {
                return Err(format!(
                    "unsupported bootstrap: bytes at {addr:#x} differ (hooks are for DT2 1.16)"
                ));
            }
        }
        let mut m = Machine {
            cpu: Cpu::new(),
            board: Board::new(flash),
            boot_body: boot[4..].to_vec(),
            mode: 1,
            watch: vec![false; (SRAM_SIZE / 2) as usize],
            pc_hits: BTreeMap::new(),
            stop_pcs: Vec::new(),
            pc_keys: Vec::new(),
            run_main: false,
            icount: 0,
            first_exception: None,
            main_os_entry: None,
        };
        m.power_on();
        Ok(m)
    }

    /// Reset the CPU and SRAM; DDR, the reset-flag word and the flash array survive.
    fn power_on(&mut self) {
        self.board.sram.fill(0);
        let body = self.boot_body.clone();
        self.board.write_mem(BOOT_LOAD, &body);
        self.board.mmio.clear();
        self.board.panel = Uart::default();
        self.board.midi = Uart::default();
        self.board.panel_unmasked = false;
        self.board.midi_unmasked = false;
        self.board.panel_cmd.clear();
        self.board.key_events = false;
        self.board.reset_requested = false;
        self.board.load_key_table();
        self.cpu = Cpu::new();
        // The image's vector pair is SP = 0x80010000, PC = 0x80000de8. That entry takes a boot
        // mode as a stack argument (mode 0 executes a debug-register write, 1 is the normal
        // boot, see docs/findings/01). Whatever stage hands over the mode on the device is
        // outside section 2, so the model enters as a call: return address 0, then the mode.
        let sp = self.board.read_u32(BOOT_LOAD).wrapping_sub(8);
        self.board.write_u32(sp, 0);
        self.board.write_u32(sp + 4, self.mode);
        self.cpu.a[7] = sp;
        self.cpu.pc = self.board.read_u32(BOOT_LOAD + 4);
    }

    pub fn watch_pc(&mut self, pc: u32) {
        if (SRAM_BASE..SRAM_BASE + SRAM_SIZE).contains(&pc) {
            self.watch[((pc - SRAM_BASE) >> 1) as usize] = true;
            self.pc_hits.entry(pc).or_insert((0, 0));
        }
    }

    pub fn set_midi(&mut self, bytes: &[u8]) {
        self.board.midi_source = bytes.iter().copied().collect();
    }

    /// Power-cycle the CPU and SRAM with the current settings (use after changing `mode`).
    pub fn restart(&mut self) {
        self.power_on();
    }

    /// Run the next boot after a soft reset: keys released, SRAM reloaded.
    pub fn reboot(&mut self) {
        self.board.keys = [0; 7];
        self.board.midi_source.clear();
        self.main_os_entry = None;
        self.power_on();
    }

    fn rts(&mut self) {
        let sp = self.cpu.a[7];
        self.cpu.pc = self.board.read_u32(sp);
        self.cpu.a[7] = sp.wrapping_add(4);
    }

    fn arg(&mut self, n: u32) -> u32 {
        let sp = self.cpu.a[7];
        self.board.read_u32(sp + 4 * n + 4)
    }

    /// Replace the QSPI transport routines. Returns true when the instruction was consumed.
    fn hook(&mut self, pc: u32) -> bool {
        match pc {
            HOOK_FLASH_READ => {
                let (addr, len, dst) = (self.arg(0), self.arg(1), self.arg(2));
                let data = self.board.flash_read(addr, len);
                self.board.write_mem(dst, &data);
                self.cpu.invalidate_external_write(dst, len as usize);
            }
            HOOK_FLASH_ID => {
                for (a, v) in FLASH_VARS {
                    self.board.write_u32(a, v);
                }
                self.cpu.d[0] = 0;
            }
            HOOK_QSPI_INIT | HOOK_STATUS_CMD => {}
            HOOK_SECTOR_ERASE => {
                let addr = self.arg(0);
                self.board.flash_erase(addr);
            }
            HOOK_PAGE_PROGRAM => {
                let (addr, len, src) = (self.arg(0), self.arg(1), self.arg(2));
                let data = self.board.read_mem(src, len as usize);
                self.board.flash_program(addr, &data);
            }
            _ => return false,
        }
        self.rts();
        true
    }

    fn ipl(&self) -> u8 {
        ((self.cpu.sr >> 8) & 7) as u8
    }

    /// Deliver at most one pending UART receive interrupt.
    fn service_irq(&mut self) {
        if self.ipl() >= IRQ_LEVEL {
            return;
        }
        let b = &mut self.board;
        let vector = if b.panel_unmasked
            && b.panel.rx_irq_enabled
            && b.panel.rx_cur.is_none()
            && !b.panel.rx_queue.is_empty()
        {
            b.panel.rx_cur = b.panel.rx_queue.pop_front();
            PANEL_VECTOR
        } else if b.midi_unmasked && b.midi.rx_irq_enabled && b.midi.rx_cur.is_none() {
            match b.midi_source.pop_front() {
                Some(v) => {
                    b.midi.rx_cur = Some(v);
                    b.midi_fed += 1;
                    MIDI_VECTOR
                }
                None => return,
            }
        } else {
            return;
        };
        let r = self.cpu.take_interrupt(
            &mut self.board,
            vector,
            Some(IRQ_LEVEL),
            InterruptPolicy::Device,
        );
        debug_assert!(r.is_ok());
    }

    /// Run until a stop condition. Counts instructions from the start of this call.
    pub fn run(&mut self, limit: u64, seconds: f64, allow_reboot: u32) -> (End, Vec<BootSummary>) {
        let start = Instant::now();
        let mut boots = Vec::new();
        let mut reboots_left = allow_reboot;
        let mut n = 0u64;
        let end = loop {
            if n >= limit {
                break End::Limit;
            }
            if n & 0xfff == 0 && start.elapsed().as_secs_f64() >= seconds {
                break End::Seconds;
            }
            let pc = self.cpu.pc;
            if (SRAM_BASE..SRAM_BASE + SRAM_SIZE).contains(&pc) {
                if self.watch[((pc - SRAM_BASE) >> 1) as usize] {
                    let e = self.pc_hits.entry(pc).or_insert((0, 0));
                    if e.0 == 0 {
                        e.1 = self.icount;
                    }
                    e.0 += 1;
                    let keys: Vec<PcKey> = self
                        .pc_keys
                        .iter()
                        .filter(|k| k.pc == pc)
                        .cloned()
                        .collect();
                    for k in keys {
                        if e.0 == 1 {
                            let _ = self.board.key(k.code, k.down);
                        }
                    }
                    if self.stop_pcs.contains(&pc) {
                        break End::StopPc(pc);
                    }
                }
                if self.hook(pc) {
                    n += 1;
                    self.icount += 1;
                    continue;
                }
            } else if (MAIN_LOAD..MAIN_END).contains(&pc) {
                if self.main_os_entry.is_none() {
                    self.main_os_entry = Some((pc, self.icount));
                }
                if !self.run_main {
                    break End::MainOs(pc);
                }
            }
            self.service_irq();
            match self.cpu.step(&mut self.board) {
                Ok(()) => {}
                Err(Stop::Halted) => break End::Halted,
                Err(Stop::Unimplemented(f)) => break End::Unimplemented(format!("{f:?}@{pc:#x}")),
            }
            n += 1;
            self.icount += 1;
            if let Some(v) = self.cpu.last_exception
                && v < 64
            {
                self.first_exception.get_or_insert((v, pc, self.icount));
                break End::Fault { vector: v, pc };
            }
            if self.board.reset_requested {
                boots.push(self.summary(&End::Reset));
                if reboots_left == 0 {
                    break End::Reset;
                }
                reboots_left -= 1;
                self.reboot();
            }
        };
        boots.push(self.summary(&end));
        (end, boots)
    }

    fn summary(&self, end: &End) -> BootSummary {
        BootSummary {
            end: end.name(),
            icount: self.icount,
            pc_hits: self.pc_hits.iter().map(|(k, v)| (*k, v.0)).collect(),
            flash_erased: self.board.flash_log.erased_sectors.clone(),
            flash_programmed: self.board.flash_log.programmed_bytes,
            flash_min: self.board.flash_log.min_addr,
            flash_max_end: self.board.flash_log.max_end,
            midi_fed: self.board.midi_fed,
            panel_commands: self.board.panel_commands.len() as u64,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BootSummary {
    pub end: String,
    pub icount: u64,
    pub pc_hits: Vec<(u32, u64)>,
    pub flash_erased: Vec<u32>,
    pub flash_programmed: u64,
    pub flash_min: Option<u32>,
    pub flash_max_end: u32,
    pub midi_fed: u64,
    pub panel_commands: u64,
}
