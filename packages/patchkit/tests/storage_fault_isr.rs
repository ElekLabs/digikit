//! Opt-in component replay of the DT2 1.17 ISR and its real no-waiter signal helper.
//! IRQSTAT is a controlled W1C test double: this does not prove controller
//! delivery, DMA, scheduler/waiter behavior, or native-boot storage completion.
use coldfire::{Bus, BusError, Cpu, InterruptPolicy};
use patchkit::build::{cf, toolchain::Env};
use patchkit::error::sha256_hex;
use patchkit::firmware::Firmware;
use patchkit::link::Linked;
use std::path::Path;

const STOCK_SHA: &str = "26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5";
const SITE_SHA: &str = "ecda53f8ded4ab291974386fad6a07fc3d8672509f611233aff4a9c2560b15f9";
const SITES: [u32; 3] = [0x4012_d5c2, 0x4012_d616, 0x4012_d642];
const ISR: u32 = 0x4012_d5ba;
const IRQSTAT: u32 = 0xfc0c_c030;
const MASK: u32 = 0x0031_0000;
const STATUS: [u32; 2] = [0x44e3_fea8, 0x44e3_feac];
const SEM: [u32; 2] = [0x44e3_fec0, 0x44e3_fec8];
const LOAD: u32 = 0x1000;
const SP: u32 = 0xf000;
const RETURN: u32 = 0x9000;
const VBR: u32 = 0x8000;
const VECTOR: u8 = 0x80;
const CEILING: usize = 2048;

fn compiled() -> Linked {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let env = Env::new(&root);
    env.m68k("as")
        .expect("ISR replay requires the ColdFire assembler");
    let object = cf::cf_object(
        &env,
        &[root.join("modules/core/adapters/storage-fault-proof/src/cf/storage-fault.s")],
        &[],
    )
    .unwrap();
    let externs = ['a', 'b', 'c']
        .into_iter()
        .zip(SITES)
        .map(|(name, site)| (format!("storage_fault_site_{name}"), u64::from(site)))
        .collect();
    let linked = patchkit::link::link(&object, u64::from(LOAD), &externs).unwrap();
    assert_eq!(linked.bss_size, 4);
    linked
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mmio {
    Read(u32),
    Write(u32),
}

#[derive(Clone)]
struct Memory {
    regions: Vec<(u32, Vec<u8>)>,
    irq: u32,
    mmio: Vec<Mmio>,
    writes: Vec<(u32, Vec<u8>)>,
    observer: bool,
    observer_sp: u32,
    latch: u32,
}
impl Memory {
    fn new(fw: &Firmware, linked: &Linked, patched: bool) -> Self {
        let main = fw.section(3).expect("stock MAIN section");
        let mut memory = Self {
            regions: vec![
                (main.dest, main.data.clone()),
                (0, vec![0xa5; 0x10000]),
                (0x44e3_fe00, vec![0; 0x100]),
            ],
            irq: 0,
            mmio: vec![],
            writes: vec![],
            observer: false,
            observer_sp: 0,
            latch: linked.symbols["storage_fault_bits"] as u32,
        };
        memory.put(LOAD, &linked.image).unwrap();
        memory.put(memory.latch, &[0; 4]).unwrap();
        memory
            .put(VBR + u32::from(VECTOR) * 4, &ISR.to_be_bytes())
            .unwrap();
        for (site, name) in SITES.into_iter().zip(['a', 'b', 'c']) {
            assert_eq!(
                sha256_hex(memory.get(site, 6).unwrap()),
                SITE_SHA,
                "original IRQSTAT load at {site:#x}"
            );
            if patched {
                let target = linked.symbols[&format!("storage_fault_hook_{name}")] as u32;
                let mut jump = vec![0x4e, 0xf9];
                jump.extend_from_slice(&target.to_be_bytes());
                memory.put(site, &jump).unwrap();
            }
        }
        memory.writes.clear();
        memory
    }
    fn get(&self, at: u32, n: usize) -> Result<&[u8], BusError> {
        self.regions
            .iter()
            .find_map(|(base, data)| {
                let offset = at.checked_sub(*base)? as usize;
                data.get(offset..offset.checked_add(n)?)
            })
            .ok_or(BusError {
                addr: at,
                write: false,
            })
    }
    fn put(&mut self, at: u32, bytes: &[u8]) -> Result<(), BusError> {
        for (base, data) in &mut self.regions {
            if let Some(offset) = at.checked_sub(*base).map(|v| v as usize)
                && let Some(dest) = data.get_mut(offset..offset + bytes.len())
            {
                dest.copy_from_slice(bytes);
                return Ok(());
            }
        }
        Err(BusError {
            addr: at,
            write: true,
        })
    }
    fn write(&mut self, at: u32, bytes: &[u8]) -> Result<(), BusError> {
        if self.observer {
            assert!(
                at == self.latch && bytes.len() == 4
                    || (self.observer_sp - 8..self.observer_sp - 2).contains(&at)
                        && at + bytes.len() as u32 <= self.observer_sp - 2,
                "observer wrote outside its latch/temporary stack: {at:#x}"
            );
        } else {
            self.writes.push((at, bytes.to_vec()));
        }
        self.put(at, bytes)
    }
    fn word(&self, at: u32) -> u32 {
        u32::from_be_bytes(self.get(at, 4).unwrap().try_into().unwrap())
    }
}
impl Bus for Memory {
    fn read8(&mut self, at: u32) -> Result<u8, BusError> {
        Ok(self.get(at, 1)?[0])
    }
    fn read16(&mut self, at: u32) -> Result<u16, BusError> {
        Ok(u16::from_be_bytes(self.get(at, 2)?.try_into().unwrap()))
    }
    fn read32(&mut self, at: u32) -> Result<u32, BusError> {
        if at == IRQSTAT {
            self.mmio.push(Mmio::Read(self.irq));
            return Ok(self.irq);
        }
        Ok(u32::from_be_bytes(self.get(at, 4)?.try_into().unwrap()))
    }
    fn write8(&mut self, at: u32, value: u8) -> Result<(), BusError> {
        self.write(at, &[value])
    }
    fn write16(&mut self, at: u32, value: u16) -> Result<(), BusError> {
        self.write(at, &value.to_be_bytes())
    }
    fn write32(&mut self, at: u32, value: u32) -> Result<(), BusError> {
        if at == IRQSTAT {
            self.mmio.push(Mmio::Write(value));
            self.irq &= !value;
            return Ok(());
        }
        self.write(at, &value.to_be_bytes())
    }
}

fn context(sr: u16) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.pc = RETURN;
    cpu.sr = sr;
    cpu.ctrl.vbr = VBR;
    for i in 0..8 {
        cpu.d[i] = 0x8765_0010 + i as u32 * 0x101;
        cpu.a[i] = 0x1234_1000 + i as u32 * 0x101;
    }
    cpu.a[7] = SP;
    cpu
}

fn replay(memory: &mut Memory, linked: &Linked, irq: u32, sr: u16) -> Cpu {
    memory.irq = irq;
    memory.mmio.clear();
    memory.writes.clear();
    let mut cpu = context(sr);
    let before_d = cpu.d;
    let before_a = cpu.a;
    // Synthetic vector/level are controlled fixture setup, not the device's
    // interrupt installation or arbitration. Exception entry and RTE are real.
    assert!(
        cpu.take_interrupt(memory, VECTOR, Some(3), InterruptPolicy::Device)
            .unwrap()
    );
    let mut signal_entries = [0usize; 2];
    let mut rte_entries = 0;
    for _ in 0..CEILING {
        if cpu.pc == RETURN {
            cpu.resolve_nzv();
            assert_eq!(cpu.d, before_d);
            assert_eq!(cpu.a, before_a);
            assert_eq!(cpu.sr, sr);
            assert_eq!(rte_entries, 1, "return through actual ISR RTE");
            let signals = (irq & 0x0031_0003).count_ones() as usize;
            assert_eq!(signal_entries, [signals; 2], "real signal helper path");
            return cpu;
        }
        if cpu.pc == 0x4012_d69e {
            rte_entries += 1;
        }
        if cpu.pc == 0x4000_155c {
            signal_entries[0] += 1;
        }
        if cpu.pc == 0x4000_11ee {
            signal_entries[1] += 1;
        }
        memory.observer = (LOAD..LOAD + linked.image.len() as u32).contains(&cpu.pc);
        if ['a', 'b', 'c']
            .into_iter()
            .any(|name| linked.symbols[&format!("storage_fault_hook_{name}")] as u32 == cpu.pc)
        {
            memory.observer_sp = cpu.a[7];
        }
        let pc = cpu.pc;
        cpu.step(memory)
            .unwrap_or_else(|error| panic!("ISR step at {pc:#x}: {error:?}"));
        assert_eq!(
            cpu.last_exception, None,
            "unexpected exception from {pc:#x}"
        );
    }
    panic!("ISR exceeded {CEILING} instructions at {:#x}", cpu.pc);
}

fn expected_mmio(mut irq: u32) -> Vec<Mmio> {
    let mut events = vec![];
    for bit in [0x10000, 2, 0x200000, 0x100000, 1] {
        events.push(Mmio::Read(irq));
        if irq & bit != 0 {
            events.push(Mmio::Write(bit));
            irq &= !bit;
        }
    }
    events
}

#[test]
fn actual_isr_preserves_stock_results_and_latches_errors() {
    let Some(path) = std::env::var_os("DIGI_STORAGE_FAULT_ISR_SYX") else {
        eprintln!(
            "SKIP controlled storage ISR replay: set DIGI_STORAGE_FAULT_ISR_SYX to stock DT2 1.17 .syx"
        );
        return;
    };
    let raw =
        std::fs::read(&path).expect("DIGI_STORAGE_FAULT_ISR_SYX must name a readable stock image");
    let fw = Firmware::parse(&raw).expect("parse configured stock image");
    assert_eq!(fw.syx_sha256, STOCK_SHA, "requires exact stock DT2 1.17");
    let linked = compiled();
    let cases: &[&[u32]] = &[
        &[0],
        &[0x10000],
        &[0x100000],
        &[0x200000],
        &[1],
        &[2],
        &[3],
        &[0x10001],
        &[0x100001],
        &[0x200001],
        &[0x310003],
        &[0x10000, 0, 0x100001, 0x200002, 1],
    ];
    for case in cases {
        for sr in [0x2000, 0x201f, 0x2215] {
            let mut stock = Memory::new(&fw, &linked, false);
            let mut patched = Memory::new(&fw, &linked, true);
            let mut sticky = 0;
            for &irq in *case {
                let original = replay(&mut stock, &linked, irq, sr);
                let observed = replay(&mut patched, &linked, irq, sr);
                assert_eq!(
                    (
                        observed.d,
                        observed.a,
                        observed.sr,
                        observed.pc,
                        observed.other_a7
                    ),
                    (
                        original.d,
                        original.a,
                        original.sr,
                        original.pc,
                        original.other_a7
                    )
                );
                assert_eq!(stock.mmio, expected_mmio(irq));
                assert_eq!(patched.mmio, stock.mmio);
                assert_eq!(patched.irq, stock.irq);
                assert_eq!(
                    patched.writes, stock.writes,
                    "all writes excluding observer latch/temporary stack"
                );
                for at in STATUS.into_iter().chain(SEM).chain(SEM.map(|at| at + 4)) {
                    assert_eq!(patched.word(at), stock.word(at), "status/signal {at:#x}");
                }
                sticky |= irq & MASK;
                assert_eq!(patched.word(patched.latch), sticky);
                assert_eq!(stock.word(stock.latch), 0);
            }
        }
    }
}
