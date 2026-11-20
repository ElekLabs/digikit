//! Run the firmware's own aPLib depacker (the bootstrap's, section 2) on a stream.
//!
//! usage: depack_check BOOTSTRAP.bin STREAM.bin ENTRY_HEX --stage HEX --base HEX --dest HEX [OUT.bin]
//!   --stage  where the packed section is staged: the device's `[patch] main_stage` for the MAIN OS (0x40400000 on the Digitakt II and Digitone II, 0x40200000 on the
//!            Syntakt, finding 152); for another section any address that does not overlap the output
//!   --base   the address section 2 is loaded at: the device's `[patch] boot_load`
//!   --dest   where the output goes: the device's `[patch] main_dest`
//!   No defaults: a device's numbers are device data, and a wrong default would make a check pass or fail for the wrong device.
//!
//! BOOTSTRAP.bin is the decompressed section 2 (loaded at 0x80000400). STREAM.bin
//! is a section as stored in the container, with its 8-byte header. The bootstrap
//! calls the depacker as `pea dst; pea src; jsr` (0x800005f0); this does
//! the same on the native ColdFire core (ENTRY is the depacker's address; 0x80000436
//! in the 1.16 and 1.11 bootstraps, found by tools/firmware/aplib_check.py from its prologue), with the stream at the stage (where the
//! bootstrap stages a packed section) and the output at 0x40000400 (where MAIN OS
//! goes). Prints one JSON line: bytes written (the routine's return value), the
//! SHA-256 of the output and the instruction count.

use coldfire::{Bus, BusError, Cpu};
use sha2::{Digest, Sha256};
use std::{env, fs};

const SRAM: u32 = 0x8000_0000;
const SRAM_LEN: usize = 0x10_0000;
const DDR: u32 = 0x4000_0000;
const DDR_LEN: usize = 0x0200_0000;
const RETURN_TO: u32 = 0x8000_0100;

struct Mem {
    sram: Vec<u8>,
    ddr: Vec<u8>,
}

impl Mem {
    fn slot(&mut self, addr: u32, len: usize) -> Result<&mut [u8], BusError> {
        let err = BusError { addr, write: false };
        let (mem, base) = if (SRAM..SRAM + SRAM_LEN as u32).contains(&addr) {
            (&mut self.sram, SRAM)
        } else if (DDR..DDR + DDR_LEN as u32).contains(&addr) {
            (&mut self.ddr, DDR)
        } else {
            return Err(err);
        };
        let at = (addr - base) as usize;
        mem.get_mut(at..at + len).ok_or(err)
    }
}

impl Bus for Mem {
    fn read8(&mut self, a: u32) -> Result<u8, BusError> {
        Ok(self.slot(a, 1)?[0])
    }
    fn read16(&mut self, a: u32) -> Result<u16, BusError> {
        Ok(u16::from_be_bytes(self.slot(a, 2)?.try_into().unwrap()))
    }
    fn read32(&mut self, a: u32) -> Result<u32, BusError> {
        Ok(u32::from_be_bytes(self.slot(a, 4)?.try_into().unwrap()))
    }
    fn write8(&mut self, a: u32, v: u8) -> Result<(), BusError> {
        self.slot(a, 1).map_err(|e| BusError { write: true, ..e })?[0] = v;
        Ok(())
    }
    fn write16(&mut self, a: u32, v: u16) -> Result<(), BusError> {
        self.slot(a, 2)
            .map_err(|e| BusError { write: true, ..e })?
            .copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), BusError> {
        self.slot(a, 4)
            .map_err(|e| BusError { write: true, ..e })?
            .copy_from_slice(&v.to_be_bytes());
        Ok(())
    }
}

fn hex_flag(args: &mut Vec<String>, name: &str) -> u32 {
    match args.iter().position(|a| a == name) {
        Some(i) => {
            let v = args
                .get(i + 1)
                .unwrap_or_else(|| panic!("{name} needs a value"));
            let n = u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap();
            args.drain(i..i + 2);
            n
        }
        None => {
            eprintln!("{name} is required (the device's value)");
            std::process::exit(2);
        }
    }
}

fn main() {
    let mut args: Vec<String> = env::args().collect();
    let stream_at = hex_flag(&mut args, "--stage");
    let boot_base = hex_flag(&mut args, "--base");
    let out_at = hex_flag(&mut args, "--dest");
    if args.len() < 4 {
        eprintln!(
            "usage: depack_check BOOTSTRAP.bin STREAM.bin ENTRY_HEX [OUT.bin] --stage HEX --base HEX --dest HEX"
        );
        std::process::exit(2);
    }
    let boot = fs::read(&args[1]).unwrap();
    let stream = fs::read(&args[2]).unwrap();
    let mut mem = Mem {
        sram: vec![0; SRAM_LEN],
        ddr: vec![0; DDR_LEN],
    };
    mem.slot(boot_base, boot.len())
        .unwrap()
        .copy_from_slice(&boot);
    mem.slot(stream_at, stream.len())
        .unwrap()
        .copy_from_slice(&stream);
    let mut cpu = Cpu::new();
    let sp = SRAM + SRAM_LEN as u32 - 0x100;
    // Stack at the call: [return address][src][dst].
    mem.write32(sp, RETURN_TO).unwrap();
    mem.write32(sp + 4, stream_at).unwrap();
    mem.write32(sp + 8, out_at).unwrap();
    cpu.a[7] = sp;
    cpu.pc = u32::from_str_radix(args[3].trim_start_matches("0x"), 16).unwrap();
    cpu.sr = 0x2700;
    let mut steps = 0u64;
    while cpu.pc != RETURN_TO {
        if let Err(stop) = cpu.step(&mut mem) {
            eprintln!("stopped at pc={:#x} after {steps} steps: {stop:?}", cpu.pc);
            std::process::exit(3);
        }
        steps += 1;
        if steps > 4_000_000_000 {
            eprintln!("step limit");
            std::process::exit(4);
        }
    }
    let len = cpu.d[0] as usize;
    let out = mem.slot(out_at, len).unwrap().to_vec();
    if let Some(path) = args.get(4) {
        fs::write(path, &out).unwrap();
    }
    let digest: String = Sha256::digest(&out)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("{{\"len\":{len},\"sha256\":\"{digest}\",\"steps\":{steps}}}");
}
