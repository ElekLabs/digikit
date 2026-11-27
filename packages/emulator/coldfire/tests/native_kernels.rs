//! Native kernels against the interpreter from the same states (`Cpu::check_native`: registers, CCR, EMAC, instruction count and every
//! bus read and write in order). The states are random: registers, CCR, MACSR with every mode bit (the firmware only runs two modes, the
//! translation is the same for all), accumulators with extensions, MASK, memory that answers with a hash of the address, holes that
//! refuse access (a kernel must stop before the instruction and leave the fault to the interpreter), budgets that end a run anywhere.
//!
//! Needs a build with kernels (`CF_NATIVE_DIR`, see `devices/cf-native/`) and the firmware section they were made from under out/;
//! without either the test returns at once. The stock run covers the same code on real entry states: `DIGI_CF_NATIVE=check`.

use coldfire::cpu::native::KERNELS;
use coldfire::{Bus, BusError, Cpu};
use std::collections::HashMap;

struct MapBus {
    code_base: u32,
    code: Vec<u8>,
    written: HashMap<u32, u8>,
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

impl MapBus {
    fn byte(&self, a: u32) -> u8 {
        if let Some(b) = self.written.get(&a) {
            return *b;
        }
        let o = a.wrapping_sub(self.code_base) as usize;
        if o < self.code.len() {
            return self.code[o];
        }
        mix(u64::from(a)) as u8
    }
    fn hole(a: u32) -> bool {
        (a >> 12) & 0xf == 0xd
    }
    fn get(&self, a: u32, n: u32, write: bool) -> Result<u32, BusError> {
        if (0..n).any(|k| Self::hole(a.wrapping_add(k))) {
            return Err(BusError { addr: a, write });
        }
        Ok((0..n).fold(0, |v, k| v << 8 | u32::from(self.byte(a.wrapping_add(k)))))
    }
    fn put(&mut self, a: u32, n: u32, v: u32) -> Result<(), BusError> {
        if (0..n).any(|k| Self::hole(a.wrapping_add(k))) {
            return Err(BusError {
                addr: a,
                write: true,
            });
        }
        for k in 0..n {
            self.written
                .insert(a.wrapping_add(k), (v >> (8 * (n - 1 - k))) as u8);
        }
        Ok(())
    }
}

impl Bus for MapBus {
    fn read8(&mut self, a: u32) -> Result<u8, BusError> {
        self.get(a, 1, false).map(|v| v as u8)
    }
    fn read16(&mut self, a: u32) -> Result<u16, BusError> {
        self.get(a, 2, false).map(|v| v as u16)
    }
    fn read32(&mut self, a: u32) -> Result<u32, BusError> {
        self.get(a, 4, false)
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
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = mix(self.0.wrapping_add(0x9e37_79b9_7f4a_7c15));
        (self.0 >> 16) as u32
    }
}

#[test]
fn every_kernel_agrees_with_the_interpreter_on_random_states() {
    if KERNELS.is_empty() {
        eprintln!("skipped: this build has no native kernels (CF_NATIVE_DIR)");
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let image = root.join("out/sections/st-1.42/section_7_BLOB.bin");
    let Ok(code) = std::fs::read(&image) else {
        eprintln!("skipped: {} is not there", image.display());
        return;
    };
    let base = 0x4000_0400u32;
    let mut rng = Rng(0x2026_1113);
    let mut compared = 0u64;
    let mut runs = 0u32;
    let mut short = 0u32;
    for k in KERNELS {
        for trial in 0..600u32 {
            let mut bus = MapBus {
                code_base: base,
                code: code.clone(),
                written: HashMap::new(),
            };
            let mut cpu = Cpu::new();
            assert!(cpu.native_install());
            for r in 0..8 {
                cpu.d[r] = rng.next();
                // addresses: mostly word-aligned places away from holes and the code, some anywhere
                cpu.a[r] = match rng.next() % 8 {
                    0 => rng.next(),
                    1 => k.entry.wrapping_add(rng.next() % (k.len + 8)) & !1,
                    _ => 0x8000_0000 + (rng.next() % 0x8000) * 4,
                };
            }
            cpu.a[7] = 0x8000_8000 + (rng.next() % 0x1000) * 4;
            cpu.sr = 0x2700 | (rng.next() & 0x1f) as u16;
            cpu.emac.macsr = rng.next() & 0xff;
            cpu.emac.acc = [rng.next(), rng.next(), rng.next(), rng.next()];
            cpu.emac.accext01 = rng.next();
            cpu.emac.accext23 = rng.next();
            cpu.emac.mask = if rng.next().is_multiple_of(2) {
                0xffff_ffff
            } else {
                rng.next() | 0xffff
            };
            cpu.pc = k.entry;
            let budget = match trial % 4 {
                0 => 1 + u64::from(rng.next() % 200),
                1 => 1 + u64::from(rng.next() % 3000),
                _ => 100_000,
            };
            let before = cpu.icount;
            match cpu.check_native(&mut bus, budget) {
                Ok(Some(n)) => {
                    runs += 1;
                    compared += n;
                    assert_eq!(cpu.icount - before, n);
                    if cpu.pc.wrapping_sub(k.entry) < k.len {
                        short += 1;
                    }
                }
                Ok(None) => {}
                Err(why) => panic!("trial {trial}: {why}"),
            }
        }
    }
    eprintln!("{runs} kernel runs agreed ({compared} instructions), {short} of them stopped early");
    assert!(runs > 300, "too few kernel runs: {runs}");
    assert!(
        short > 30,
        "the budget and the holes stopped too few runs: {short}"
    );
}

/// Flat memory for the timing: code, then 64 KiB of data at 0x8000_0000.
struct Flat {
    code_base: u32,
    code: Vec<u8>,
    data: Vec<u8>,
}

impl Flat {
    fn slot(&mut self, a: u32, n: usize) -> &mut [u8] {
        if a >= 0x8000_0000 {
            let o = (a - 0x8000_0000) as usize;
            &mut self.data[o..o + n]
        } else {
            let o = (a - self.code_base) as usize;
            &mut self.code[o..o + n]
        }
    }
}

impl Bus for Flat {
    fn read8(&mut self, a: u32) -> Result<u8, BusError> {
        Ok(self.slot(a, 1)[0])
    }
    fn read16(&mut self, a: u32) -> Result<u16, BusError> {
        Ok(u16::from_be_bytes(self.slot(a, 2).try_into().unwrap()))
    }
    fn read32(&mut self, a: u32) -> Result<u32, BusError> {
        Ok(u32::from_be_bytes(self.slot(a, 4).try_into().unwrap()))
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), BusError> {
        self.slot(a, 1)[0] = v;
        Ok(())
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), BusError> {
        self.slot(a, 2).copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), BusError> {
        self.slot(a, 4).copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
}

/// The kernel's own speed-up: the same call, many times, on flat memory, interpreter and kernel interleaved, best of several rounds.
/// `cargo test -p coldfire --release --test native_kernels -- --ignored --nocapture`
#[test]
#[ignore = "timing"]
fn kernel_speed_against_the_interpreter() {
    let Some(k) = KERNELS.first() else {
        eprintln!("skipped: no native kernels");
        return;
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let Ok(code) = std::fs::read(root.join("out/sections/st-1.42/section_7_BLOB.bin")) else {
        eprintln!("skipped: no section");
        return;
    };
    let mut bus = Flat {
        code_base: 0x4000_0400,
        code,
        // every longword is an address inside the data, so a pointer loaded from anywhere stays in range
        data: (0..0x4000u32)
            .flat_map(|i| (0x8000_0000 + (mix(u64::from(i)) as u32 % 0x2000) * 4).to_be_bytes())
            .collect(),
    };
    let mut rng = Rng(7);
    // return address: an rts target outside the function
    let ret = 0x4000_0500u32;
    let make = |rng: &mut Rng, bus: &mut Flat| {
        let mut cpu = Cpu::new();
        cpu.native_install();
        for r in 0..8 {
            cpu.d[r] = rng.next();
            cpu.a[r] = 0x8000_0000 + 0x4000 + (rng.next() % 0x1000) * 4;
        }
        cpu.a[7] = 0x8000_f000;
        // the two stack arguments: voice index and base
        bus.write32(cpu.a[7], ret).unwrap();
        bus.write32(cpu.a[7] + 4, 0).unwrap();
        bus.write32(cpu.a[7] + 8, 0x8000_1000).unwrap();
        cpu.sr = 0x2700;
        cpu.emac.macsr = 0x20;
        cpu.pc = k.entry;
        cpu
    };
    let template = make(&mut rng, &mut bus);
    let calls = 3000u32;
    let mut best = (f64::MAX, f64::MAX);
    for _round in 0..7 {
        for native in [false, true] {
            let mut cpu = template.clone();
            let t = std::time::Instant::now();
            let mut insns = 0u64;
            for _ in 0..calls {
                cpu.pc = k.entry;
                cpu.a[7] = 0x8000_f000;
                let before = cpu.icount;
                if native {
                    assert!(cpu.run_native(&mut bus, u64::MAX).is_some());
                } else {
                    while cpu.pc != ret {
                        cpu.step(&mut bus).unwrap();
                    }
                }
                insns += cpu.icount - before;
            }
            let ns = t.elapsed().as_nanos() as f64 / insns as f64;
            let slot = if native { &mut best.1 } else { &mut best.0 };
            *slot = slot.min(ns);
        }
    }
    eprintln!(
        "kernel {:#x}: interpreter {:.2} ns/insn, native {:.2} ns/insn, speed-up {:.2}x",
        k.entry,
        best.0,
        best.1,
        best.0 / best.1
    );
}
