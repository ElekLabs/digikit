//! `core/lfo-table` (runtime/platform/lfo-table): the wave stubs and the set and clear calls of the committed object on the ColdFire core, and the patched images.
//!
//! - each stub gives the value of `interp_cells` of services/lfo (the formula below, written out again here) for any phase, and changes only D0, D1, A0 and the flags;
//! - `lfo_table_set` and `lfo_table_clear` change the function table and the cells as the contract says, under the interrupt mask, and refuse a wave outside the range;
//! - the patched images (skipped when a stock image is absent): the two clamp immediates, the eight `lea` operands, the stock cells and the added cells of the four tables, `lfo_table_info`.

use coldfire::{Bus, Cpu};
use patchkit::apply::{self, Applied, Options};
use patchkit::devdata;
use patchkit::dko::Dko;
use patchkit::firmware::Firmware;
use patchkit::link;
use patchkit::module::{Library, Module};
use patchkit::port::Registry;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const MODULE: &str = "core/lfo-table";

/// The devices whose file adds LFO waves (`[limits] lfo_waves`): the short names.
fn devices() -> Vec<String> {
    devdata::all()
        .iter()
        .filter(|d| d.limit_of("lfo_waves").is_some_and(|n| n > 0))
        .map(|d| d.short.clone())
        .collect()
}

fn module(lib: &Library) -> &Module {
    lib.get(MODULE).expect("core/lfo-table is in the library")
}

fn waves(dev: &str) -> u32 {
    devdata::by_short(dev)
        .and_then(|d| d.limit_of("lfo_waves"))
        .unwrap_or_else(|| panic!("{dev} has no [limits] lfo_waves")) as u32
}

// ---- the objects on the ColdFire core ------------------------------------------------------------------------------

mod common;
use common::Ram;

const BASE: u64 = 0x1000;
const FN_TABLE: u32 = 0x8000; // the section lfo_fn: 7 stock entries, then N
const STACK: u32 = 0xf000;
const RETURN: u32 = 0xf100; // where a call ends: the return address the test pushes
const DATA_END: usize = 0xe000; // below: the object, its data and the tables; above: the stack of the calls

/// A device's object linked at `BASE`, with the function table at `FN_TABLE` (7 stock words, then the sine's address N times, as the section is built).
struct Rig {
    ram: Ram,
    l: link::Linked,
    n: u32,
}

impl Rig {
    fn new(m: &Module, dev: &str) -> Rig {
        let (_, dko): (_, &Dko) = m
            .manifest
            .object
            .iter()
            .zip(&m.objects)
            .find(|(o, _)| o.devices.iter().any(|d| d == dev))
            .unwrap_or_else(|| panic!("no object for {dev}"));
        let externs: BTreeMap<String, u64> = [("lfo_fn_table".to_string(), FN_TABLE as u64)].into();
        let l = link::link(dko, BASE, &externs).unwrap();
        let mut ram = Ram(vec![0; 0x1_0000]);
        ram.0[BASE as usize..BASE as usize + l.image.len()].copy_from_slice(&l.image);
        let n = waves(dev);
        for k in 0..7 + n {
            // the stock entries are distinct addresses; the added ones are the sine's (slot 1)
            let v = if k < 7 {
                0x4013_9100 + 0x10 * k
            } else {
                0x4013_9110
            };
            ram.write32(FN_TABLE + 4 * k, v).unwrap();
        }
        Rig { ram, l, n }
    }
    fn sym(&self, name: &str) -> u32 {
        self.l.symbols[name] as u32
    }
    fn cells(&self) -> u32 {
        self.sym("lfo_wave_cells")
    }
    fn fn_entry(&mut self, wave: u32) -> u32 {
        self.ram.read32(FN_TABLE + 4 * wave).unwrap()
    }
    /// Calls `entry` with `args` on the stack (the m68k C convention: the first at 4(SP)), the registers set to the canaries, until it returns.
    /// Returns the cpu after the return, and the list of the instructions' pcs it ran.
    fn call(&mut self, entry: u32, args: &[u32]) -> (Cpu, usize) {
        let mut cpu = Cpu::new();
        cpu.pc = entry;
        cpu.sr = 0x2000;
        let sp = STACK - 4 * args.len() as u32 - 4;
        cpu.a[7] = sp;
        self.ram.write32(sp, RETURN).unwrap();
        for (i, a) in args.iter().enumerate() {
            self.ram.write32(sp + 4 + 4 * i as u32, *a).unwrap();
        }
        for r in 0..8 {
            cpu.d[r] = 0xd0d0_0000 + r as u32;
        }
        for r in 0..7 {
            cpu.a[r] = 0xa0a0_0000 + r as u32;
        }
        let mut steps = 0;
        while cpu.pc != RETURN {
            cpu.step(&mut self.ram).unwrap();
            assert_eq!(cpu.last_exception, None, "exception at {:#x}", cpu.pc);
            steps += 1;
            assert!(steps < 2000, "the call does not return");
        }
        assert_eq!(cpu.a[7], sp + 4, "the call leaves the stack as it found it");
        (cpu, steps)
    }
}

/// `interp_cells` of services/lfo/src/shapes.rs from the samples: the cell of the top 5 bits of the phase, its step times the next 15 bits, shifted once.
fn formula(s: &[i16; 32], phase: u32) -> u32 {
    let i = (phase >> 27) as usize;
    let f = ((phase >> 12) & 0x7fff) as i32;
    let v = (s[i] as i32) << 16;
    let d = s[(i + 1) & 31] as i32 - s[i] as i32;
    v.wrapping_add(d.wrapping_mul(f).wrapping_shl(1)) as u32
}

/// 32 samples that use the whole range, both signs and the largest step (full scale to full scale).
fn samples(seed: u32) -> [i16; 32] {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    let mut s = [0i16; 32];
    for (i, v) in s.iter_mut().enumerate() {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *v = match i {
            0 => i16::MAX,
            1 => i16::MIN,
            31 => i16::MIN,
            _ => (x >> 16) as i16,
        };
    }
    s
}

fn phases() -> Vec<u32> {
    let mut v = vec![
        0,
        1,
        0x0000_0fff,
        0x0000_1000,
        0x07ff_ffff,
        0x0800_0000,
        0x7fff_ffff,
        0x8000_0000,
        0xffff_ffff,
        0xf800_0000,
        0xf7ff_ffff,
    ];
    let mut x = 12345u32;
    for _ in 0..200 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        v.push(x);
    }
    v
}

fn write_cells(rig: &mut Rig, k: u32, s: &[i16; 32]) {
    for i in 0..32 {
        let a = s[i] as i32;
        let b = s[(i + 1) & 31] as i32;
        let at = rig.cells() + (k * 32 + i as u32) * 8;
        rig.ram.write32(at, (a << 16) as u32).unwrap();
        rig.ram.write32(at + 4, (b - a) as u32).unwrap();
    }
}

#[test]
fn each_stub_gives_the_value_of_interp_cells_and_changes_only_d0_d1_a0() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = module(&lib);
    for dev in devices() {
        let dev = dev.as_str();
        let mut rig = Rig::new(m, dev);
        let table = rig.sym("lfo_wave_fn");
        let mut counts = std::collections::BTreeSet::new();
        for k in 0..rig.n {
            let s = samples(k);
            write_cells(&mut rig, k, &s);
            let stub = rig.ram.read32(table + 4 * k).unwrap();
            for phase in phases() {
                let before = rig.ram.0.clone();
                let (cpu, steps) = rig.call(stub, &[phase]);
                assert_eq!(
                    cpu.d[0],
                    formula(&s, phase),
                    "{dev} wave {k} phase {phase:#x}"
                );
                for r in 2..8 {
                    assert_eq!(cpu.d[r], 0xd0d0_0000 + r as u32, "{dev}: D{r} is kept");
                }
                for r in 1..7 {
                    assert_eq!(cpu.a[r], 0xa0a0_0000 + r as u32, "{dev}: A{r} is kept");
                }
                assert!(
                    rig.ram.0[..DATA_END] == before[..DATA_END],
                    "{dev}: a stub writes no memory"
                );
                counts.insert(steps);
            }
        }
        // 18 instructions, rts included: the stub's lea and bra, then the 16 of the shared evaluation
        assert_eq!(counts.into_iter().collect::<Vec<_>>(), vec![18], "{dev}");
    }
}

#[test]
fn a_stub_reads_the_cells_of_its_own_wave_only() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = module(&lib);
    let dev = devices().pop().unwrap();
    let mut rig = Rig::new(m, &dev);
    let table = rig.sym("lfo_wave_fn");
    let (a, b) = (samples(1), samples(2));
    write_cells(&mut rig, 0, &a);
    write_cells(&mut rig, 5, &b);
    let (s0, s5) = (
        rig.ram.read32(table).unwrap(),
        rig.ram.read32(table + 20).unwrap(),
    );
    assert_ne!(s0, s5);
    assert_eq!(
        rig.call(s0, &[0x1234_5678]).0.d[0],
        formula(&a, 0x1234_5678)
    );
    assert_eq!(
        rig.call(s5, &[0x1234_5678]).0.d[0],
        formula(&b, 0x1234_5678)
    );
}

#[test]
fn set_makes_the_stub_of_the_wave_play_the_samples_and_clear_gives_the_sine_back() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = module(&lib);
    for dev in devices() {
        let dev = dev.as_str();
        let mut rig = Rig::new(m, dev);
        let (set, clear) = (rig.sym("lfo_table_set"), rig.sym("lfo_table_clear"));
        let first = rig.ram.read32(rig.sym("lfo_table_info") + 8).unwrap();
        assert_eq!(first, 7);
        let stubs = rig.sym("lfo_wave_fn");
        let s = samples(77);
        for wave in first..first + rig.n {
            let at = 0x2000;
            for (i, v) in s.iter().enumerate() {
                rig.ram.write16(at + 2 * i as u32, *v as u16).unwrap();
            }
            let (cpu, _) = rig.call(set, &[wave, at]);
            assert_eq!(cpu.d[0], 0, "{dev} wave {wave}: set succeeds");
            let stub = rig.ram.read32(stubs + 4 * (wave - first)).unwrap();
            assert_eq!(
                rig.fn_entry(wave),
                stub,
                "{dev} wave {wave}: the entry is the stub"
            );
            for phase in phases().into_iter().take(40) {
                assert_eq!(
                    rig.call(stub, &[phase]).0.d[0],
                    formula(&s, phase),
                    "{dev} wave {wave}"
                );
            }
            // the interrupt mask was taken and given back: the status register is what the caller had
            assert_eq!(rig.call(set, &[wave, at]).0.sr & 0xff00, 0x2000);
            rig.call(clear, &[wave]);
            assert_eq!(
                rig.fn_entry(wave),
                rig.fn_entry(1),
                "{dev} wave {wave}: clear gives the sine"
            );
        }
    }
}

#[test]
fn set_and_clear_refuse_a_wave_outside_the_added_range_and_change_nothing() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = module(&lib);
    let dev = devices().remove(0);
    let mut rig = Rig::new(m, &dev);
    let (set, clear) = (rig.sym("lfo_table_set"), rig.sym("lfo_table_clear"));
    let last = 6 + rig.n;
    for wave in [0, 6, last + 1, 100, u32::MAX] {
        let before = rig.ram.0.clone();
        let (cpu, _) = rig.call(set, &[wave, 0x2000]);
        assert_eq!(cpu.d[0], u32::MAX, "set {wave}");
        rig.call(clear, &[wave]);
        assert!(
            rig.ram.0[..DATA_END] == before[..DATA_END],
            "wave {wave}: nothing is written"
        );
    }
}

// ---- the patched images --------------------------------------------------------------------------------------------

/// Memory of a patched MAIN OS: the image plus the segment chain behind it.
struct Main {
    data: Vec<u8>,
    base: u32,
    segs: Vec<(u32, Vec<u8>)>,
}

impl Main {
    fn stock(syx: &[u8]) -> Main {
        let s = Firmware::parse(syx).unwrap();
        let s = s.section(3).unwrap();
        Main {
            data: s.data.clone(),
            base: s.dest,
            segs: vec![],
        }
    }
    fn patched(syx: &[u8], stock_len: usize) -> Main {
        let mut m = Main::stock(syx);
        let mut at = stock_len;
        loop {
            let len = u32::from_be_bytes(m.data[at..at + 4].try_into().unwrap());
            if len == u32::MAX {
                break;
            }
            let dest = u32::from_be_bytes(m.data[at + 4..at + 8].try_into().unwrap());
            m.segs
                .push((dest, m.data[at + 12..at + 12 + len as usize].to_vec()));
            at += 12 + len as usize;
        }
        m
    }
    fn read(&self, addr: u32, n: usize) -> Vec<u8> {
        if let Some(o) = addr.checked_sub(self.base)
            && (o as usize + n) <= self.data.len()
        {
            return self.data[o as usize..o as usize + n].to_vec();
        }
        for (d, p) in &self.segs {
            if addr >= *d && (addr - d) as usize + n <= p.len() {
                return p[(addr - d) as usize..(addr - d) as usize + n].to_vec();
            }
        }
        panic!("{addr:#x}+{n} is not in the MAIN OS");
    }
    fn u32s(&self, addr: u32, n: usize) -> Vec<u32> {
        self.read(addr, n * 4)
            .chunks(4)
            .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
            .collect()
    }
}

/// The stock function entries of the six stock waves (the redesign's check of the image bytes), by the sha256 of the stock image.
const STOCK_FUNCTIONS: [(&str, [u32; 6]); 2] = [
    (
        "26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5",
        [
            0x4013_9156,
            0x4013_916c,
            0x4013_9138,
            0x4013_914a,
            0x4013_91c6,
            0x4013_91b6,
        ],
    ),
    (
        "7e5b8a733d4ab1fdf7e5d22757f7d8eddaa3cbf71d0858874a791844c02ceb35",
        [
            0x4013_753e,
            0x4013_7554,
            0x4013_7520,
            0x4013_7532,
            0x4013_75ae,
            0x4013_759e,
        ],
    ),
];

struct Case {
    dev: String,
    raw: Vec<u8>,
    port_id: String,
    functions: [u32; 6],
}

/// The patchable devices that add waves, with their stock images (found in `firmware/` by the sha256 of the port); a device without its image is skipped.
fn cases() -> Vec<Case> {
    let reg = Registry::from_dir(&root().join("ports")).unwrap();
    let mut out = vec![];
    for d in devdata::all() {
        if !d.limit_of("lfo_waves").is_some_and(|n| n > 0) {
            continue;
        }
        let version = &d
            .identity
            .acceptance
            .as_ref()
            .expect("an acceptance firmware")
            .firmware;
        let port_id = format!("{}-{}", d.short, version);
        let port = reg
            .by_id(&port_id)
            .unwrap_or_else(|| panic!("no port {port_id}"));
        let Some(raw) = std::fs::read_dir(root().join("firmware"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "syx"))
            .filter_map(|e| std::fs::read(e.path()).ok())
            .find(|b| patchkit::error::sha256_hex(b) == port.port.syx_sha256)
        else {
            eprintln!("SKIP {port_id}: the stock image is not here");
            continue;
        };
        let functions = STOCK_FUNCTIONS
            .iter()
            .find(|(h, _)| *h == port.port.syx_sha256)
            .map(|(_, f)| *f)
            .unwrap_or_else(|| panic!("{port_id}: no stock function table recorded"));
        out.push(Case {
            dev: d.short.clone(),
            raw,
            port_id,
            functions,
        });
    }
    out
}

fn applied(c: &Case, lib: &Library) -> Applied {
    let reg = Registry::from_dir(&root().join("ports")).unwrap();
    apply::apply_with(&c.raw, &reg, lib, &[MODULE.to_string()], Options::default()).unwrap()
}

fn plan_section(a: &Applied, name: &str) -> u32 {
    a.plan
        .sections
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no section {name}"))
        .addr as u32
}

#[test]
fn the_patched_image_points_every_reader_at_the_copies_and_raises_the_clamps() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let reg = Registry::from_dir(&root().join("ports")).unwrap();
    for c in &cases() {
        let a = applied(c, &lib);
        let port = reg.by_id(&c.port_id).unwrap();
        let stock_main = Main::stock(&c.raw);
        let stock_len = stock_main.data.len();
        let main = Main::patched(&a.syx, stock_len);
        let n = waves(&c.dev);
        let anchor = |name: &str| port.anchor(name).unwrap_or_else(|| panic!("{name}")).addr as u32;
        // the clamps: the immediate of the first moveq is 6 + N, the store of the clamp (`moveq 6,D3`) and the rest of the site are stock
        for site in ["cf.lfo.site.tick.clamp", "cf.lfo.site.read.clamp"] {
            let at = anchor(site);
            let stock = stock_main.read(at, 8);
            let now = main.read(at, 8);
            assert_eq!(stock[1], 6, "{} {site}: the stock clamp is 6", c.dev);
            assert_eq!(now[1] as u32, 6 + n, "{} {site}", c.dev);
            assert_eq!(
                now[0], stock[0],
                "{} {site}: the register of the moveq",
                c.dev
            );
            assert_eq!(
                now[2..],
                stock[2..],
                "{} {site}: the compare, the branch and the store are stock",
                c.dev
            );
            assert_eq!(
                &now[6..8],
                &[0x76, 0x06],
                "{} {site}: the store of the clamp stays 6",
                c.dev
            );
        }
        // the four tables: stock cells, then N cells; eight operands
        let tables = [
            (
                "lfo_edge4",
                "cf.lfo.data.edge_mode4",
                "cf.lfo.site.edge_mode4.ref",
            ),
            (
                "lfo_edge3p",
                "cf.lfo.data.edge_mode3_pos",
                "cf.lfo.site.edge_mode3_pos.ref",
            ),
            (
                "lfo_edge3n",
                "cf.lfo.data.edge_mode3_neg",
                "cf.lfo.site.edge_mode3_neg.ref",
            ),
            (
                "lfo_fn",
                "cf.lfo.data.wave_fn_table",
                "cf.lfo.site.wave_fn_table.ref",
            ),
        ];
        let mut operands = 0;
        for (section, data, sites) in tables {
            let copy = plan_section(&a, section);
            let stock = stock_main.u32s(anchor(data), 7);
            let words = main.u32s(copy, 7 + n as usize);
            assert_eq!(
                words[..7],
                stock[..],
                "{} {section}: the stock cells are kept",
                c.dev
            );
            for (k, w) in words[7..].iter().enumerate() {
                assert_eq!(
                    *w, stock[1],
                    "{} {section}: added cell {k} is cell 1 (the sine)",
                    c.dev
                );
            }
            for reader in [1, 2] {
                let site = anchor(&format!("{sites}.{reader}"));
                let (was, now) = (stock_main.read(site, 6), main.read(site, 6));
                assert_eq!(
                    now[..2],
                    was[..2],
                    "{} {section}: the lea opcode is stock",
                    c.dev
                );
                assert!(
                    was[0] & 0xf1 == 0x41 && was[1] == 0xf9,
                    "{} {section}: it is a `lea (ABS).l,An`",
                    c.dev
                );
                assert_eq!(
                    u32::from_be_bytes(now[2..6].try_into().unwrap()),
                    copy,
                    "{} {section} reader {reader}",
                    c.dev
                );
                assert_eq!(
                    u32::from_be_bytes(was[2..6].try_into().unwrap()),
                    anchor(data),
                    "{} {section} reader {reader}: the stock operand is the stock table",
                    c.dev
                );
                operands += 1;
            }
        }
        assert_eq!(operands, 8, "{}", c.dev);
        // the stock contents the redesign found: the edge tables, and the function entries of the six stock waves
        let sec = |s: &str| main.u32s(plan_section(&a, s), 7);
        assert_eq!(sec("lfo_edge4")[..7], [0, 0, 0, 0, 0, 0x7fff_ffff, 0]);
        assert_eq!(sec("lfo_edge3p")[..7], [0; 7]);
        assert_eq!(sec("lfo_edge3n")[..7], [0, 0, 0, 0, 0x7fff_ffff, 0, 0]);
        assert_eq!(sec("lfo_fn")[..6], c.functions);
        assert_eq!(sec("lfo_fn")[6], 0, "RAND has no function");
    }
}

#[test]
fn the_info_table_states_the_waves_and_the_two_calls() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    for c in &cases() {
        let a = applied(c, &lib);
        let sym = |n: &str| a.plan.symbols[&format!("{MODULE}@0.1.0:{n}")] as u32;
        let info = a
            .plan
            .info
            .iter()
            .find(|i| i.name == "lfo_table_info")
            .expect("lfo_table_info is exported");
        assert_eq!(
            info.words,
            vec![
                0x4c46_4f54,
                1,
                7,
                waves(&c.dev),
                32,
                sym("lfo_table_set"),
                sym("lfo_table_clear")
            ],
            "{}",
            c.dev
        );
        assert_eq!(info.addr as u32, sym("lfo_table_info"));
    }
}
