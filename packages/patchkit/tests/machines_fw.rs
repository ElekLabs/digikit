//! The machine routines on the ColdFire core: the dispatch of `core/rows` (`machines-<dev>.s`) and the hooks of the machines service (`hooks-<dev>.s`), assembled as they are and run with
//! hand-built tables (they replace the Python firmware tests of the machines point), and the rows the registry tables get from the manifest. A hook is entered with the registers and
//! the stack of its site and leaves through a stub that stands for the firmware address it continues at: a case ends when the program counter reaches one.
//!
//! Skips when the m68k toolchain is absent. The table the hooks read (`DKRT_MF_TAB`) has the layout of `services/machines/src/info.rs` (its test compares the assembler's `.equ T_*` with it).

use coldfire::{Bus, BusError, Cpu};
use patchkit::build::link_sources;
use patchkit::build::toolchain::Env as Toolchain;
use patchkit::expr::{self, Env};
use patchkit::module::Library;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The RAM window of the tests: addresses `W .. W + RAM`.
const W: u32 = 0x4700_0000;
const RAM: usize = 0x20_0000;
const CODE: u32 = W + 0x1000;
const STACK_TOP: u32 = W + 0x1f_0000;
/// `DKRT_MF_TAB`, `DKRT_MF_RANK` and `DKRT_MF_RANKN` of the service.
const TAB: u32 = W + 0x10_0000;
const RANK: u32 = W + 0x10_1000;
const RANKN: u32 = W + 0x10_1800;
const ROWS: u32 = W + 0x11_0000;
const ROWSIZE: u32 = 64;
/// Where a case that returns (`rts`) ends.
const RTS: u32 = W + 0xf0;

struct Ram(Vec<u8>);

impl Ram {
    fn at(&self, addr: u32) -> Option<usize> {
        let o = addr.checked_sub(W)? as usize;
        (o < self.0.len()).then_some(o)
    }
    fn poke(&mut self, addr: u32, bytes: &[u8]) {
        let o = self.at(addr).expect("poke inside the window");
        self.0[o..o + bytes.len()].copy_from_slice(bytes);
    }
    fn word(&self, addr: u32) -> u32 {
        let o = self.at(addr).expect("read inside the window");
        u32::from_be_bytes(self.0[o..o + 4].try_into().unwrap())
    }
}

impl Bus for Ram {
    fn read8(&mut self, addr: u32) -> Result<u8, BusError> {
        let o = self.at(addr).ok_or(BusError { addr, write: false })?;
        Ok(self.0[o])
    }
    fn read16(&mut self, addr: u32) -> Result<u16, BusError> {
        Ok(u16::from_be_bytes([
            self.read8(addr)?,
            self.read8(addr + 1)?,
        ]))
    }
    fn read32(&mut self, addr: u32) -> Result<u32, BusError> {
        Ok(u32::from_be_bytes([
            self.read8(addr)?,
            self.read8(addr + 1)?,
            self.read8(addr + 2)?,
            self.read8(addr + 3)?,
        ]))
    }
    fn write8(&mut self, addr: u32, value: u8) -> Result<(), BusError> {
        let o = self.at(addr).ok_or(BusError { addr, write: true })?;
        self.0[o] = value;
        Ok(())
    }
    fn write16(&mut self, addr: u32, value: u16) -> Result<(), BusError> {
        for (i, b) in value.to_be_bytes().iter().enumerate() {
            self.write8(addr + i as u32, *b)?;
        }
        Ok(())
    }
    fn write32(&mut self, addr: u32, value: u32) -> Result<(), BusError> {
        for (i, b) in value.to_be_bytes().iter().enumerate() {
            self.write8(addr + i as u32, *b)?;
        }
        Ok(())
    }
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// Assembled sources with their stubs.
struct Unit {
    image: Vec<u8>,
    symbols: HashMap<String, u32>,
    stubs: Vec<(String, u32)>,
}

/// The sources (paths under the repository root) assembled at `CODE`; `values` are the undefined symbols that are numbers, `stubs` the ones that end a case.
fn unit(sources: &[&str], values: &[(&str, u32)], stubs: &[&str]) -> Option<Unit> {
    let env = Toolchain::new(&root());
    if !env.m68k_or_skip() {
        return None;
    }
    let mut symmap: HashMap<String, u32> =
        values.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let mut stub_at = vec![("rts".to_string(), RTS)];
    for (i, s) in stubs.iter().enumerate() {
        let a = W + 0x100 + 0x10 * i as u32;
        symmap.insert(s.to_string(), a);
        stub_at.push((s.to_string(), a));
    }
    let paths: Vec<PathBuf> = sources.iter().map(|s| root().join(s)).collect();
    let (l, _) = link_sources(
        &root(),
        &paths,
        CODE,
        &symmap,
        &[root().join("ports/bindings/inc")],
    )
    .unwrap_or_else(|e| panic!("{sources:?}: {e}"));
    Some(Unit {
        image: l.image,
        symbols: l.symbols,
        stubs: stub_at,
    })
}

/// One run of a routine: the registers, the stack (word 0 on top) and the memory it starts with.
#[derive(Clone, Default)]
struct Case {
    d: [u32; 8],
    a: [u32; 8],
    stack: Vec<u32>,
    mem: Vec<(u32, Vec<u8>)>,
}

struct Out {
    stub: String,
    d: [u32; 8],
    a: [u32; 8],
    stack: Vec<u32>,
    ram: Ram,
}

impl Unit {
    fn run(&self, routine: &str, case: &Case) -> Out {
        let mut ram = Ram(vec![0; RAM]);
        ram.poke(CODE, &self.image);
        for (at, bytes) in &case.mem {
            ram.poke(*at, bytes);
        }
        let sp = STACK_TOP - 4 * case.stack.len() as u32;
        for (i, w) in case.stack.iter().enumerate() {
            ram.poke(sp + 4 * i as u32, &be(*w));
        }
        let mut cpu = Cpu::new();
        cpu.pc = self.symbols[routine];
        cpu.sr = 0x2700;
        cpu.d = case.d;
        cpu.a = case.a;
        cpu.a[7] = sp;
        let stub = loop {
            if let Some((name, _)) = self.stubs.iter().find(|(_, a)| *a == cpu.pc) {
                break name.clone();
            }
            cpu.step(&mut ram).unwrap();
            assert_eq!(cpu.last_exception, None, "{routine}");
            assert!(cpu.pc != 0, "{routine} ran off");
        };
        let stack = (0..16).map(|i| ram.word(cpu.a[7] + 4 * i)).collect();
        Out {
            stub,
            d: cpu.d,
            a: cpu.a,
            stack,
            ram,
        }
    }
}

/// A row of the machines registry: the descriptor strings (made at run time), nine ID fields, clone_of, position, own_dsp, the name and the short name.
struct Entry {
    name: &'static str,
    short: &'static str,
    f: [u32; 9],
    clone_of: u32,
    position: u32,
    own_dsp: u32,
}

const STR_AT: u32 = W + 0x12_0000;

/// The rows and their strings as memory to poke.
fn rows(entries: &[Entry], long_at: &dyn Fn(usize) -> (u32, u32)) -> Vec<(u32, Vec<u8>)> {
    let (mut rows, mut strings) = (vec![], vec![]);
    for (k, e) in entries.iter().enumerate() {
        let n = STR_AT + strings.len() as u32;
        strings.extend_from_slice(e.name.as_bytes());
        strings.push(0);
        let s = STR_AT + strings.len() as u32;
        strings.extend_from_slice(e.short.as_bytes());
        strings.push(0);
        let (long, short) = long_at(k);
        rows.extend_from_slice(&be(long));
        rows.extend_from_slice(&be(short));
        for f in e.f {
            rows.extend_from_slice(&be(f));
        }
        for w in [e.clone_of, e.position, e.own_dsp, n, s] {
            rows.extend_from_slice(&be(w));
        }
    }
    assert_eq!(rows.len() as u32, ROWSIZE * entries.len() as u32);
    vec![(ROWS, rows), (STR_AT, strings)]
}

/// `DKRT_MF_TAB` with the words the hooks read (the layout of `info.rs`): first, count, rows, the slot map and the page rows.
fn table(first: u32, count: u32, slotmap: u32, pg_rows: u32, pg_n: u32) -> (u32, Vec<u8>) {
    let mut w = [0u32; 16];
    w[3] = first;
    w[4] = count;
    w[6] = ROWS;
    w[7] = slotmap;
    w[8] = pg_rows;
    w[9] = pg_n;
    w[12] = ROWSIZE;
    w[13] = 48;
    (TAB, w.iter().flat_map(|x| be(*x)).collect())
}

// ---- the dispatch of core/rows ------------------------------------------------------------------------------------------------

/// What `mf_dispatch` of `src` does for the types of a registry of `count` rows from `first`: the row for a type of the registry, the stock path for any other, and a row with no descriptor
/// strings gets the empty one; the register `a0` and the row's other words are kept.
fn check_dispatch(src: &str, first: u32, entries: &[Entry]) {
    let count = entries.len() as u32;
    let Some(u) = unit(
        &[src],
        &[
            ("_mf_first", first),
            ("_mf_count", count),
            ("_mf_rows", ROWS),
        ],
        &["_mf_t_dispatch"],
    ) else {
        return;
    };
    let rep = u.symbols["mf_empty_rep"] + 12;
    // the first row has strings of its own, the others have none (a gap row, a row the service took back)
    let long_at = |k: usize| {
        if k == 0 {
            (STR_AT + 0x800, STR_AT + 0x900)
        } else {
            (0, 0)
        }
    };
    let mem = rows(entries, &long_at);
    let case = |t: u32| Case {
        stack: vec![RTS, t],
        a: [0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0],
        mem: mem.clone(),
        ..Case::default()
    };
    for k in 0..count {
        let o = u.run("mf_dispatch", &case(first + k));
        let row = ROWS + k * ROWSIZE;
        assert_eq!(
            (o.stub.as_str(), o.d[0]),
            ("rts", row),
            "type {}",
            first + k
        );
        // the return address is popped; the argument stays for the caller
        assert_eq!(
            o.a,
            [0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, STACK_TOP - 4],
            "the registers are kept"
        );
        let (long, short) = (o.ram.word(row), o.ram.word(row + 4));
        if k == 0 {
            assert_eq!(
                (long, short),
                (STR_AT + 0x800, STR_AT + 0x900),
                "strings of a row are kept"
            );
        } else {
            assert_eq!(
                (long, short),
                (rep, rep),
                "a row without strings has the empty one"
            );
        }
        // the other words of the row are not touched
        assert_eq!(o.ram.word(row + 8), entries[k as usize].f[0]);
        assert_eq!(
            o.ram.word(row + 56),
            STR_AT + o_name_offset(entries, k as usize)
        );
    }
    for t in [first.wrapping_sub(1), 0, first + count, 200] {
        assert_eq!(
            u.run("mf_dispatch", &case(t)).stub,
            "_mf_t_dispatch",
            "type {t}"
        );
    }
    // the empty representation is a std::string of no characters with the reference count -1
    let o = u.run("mf_dispatch", &case(first));
    assert_eq!(
        (
            o.ram.word(rep - 12),
            o.ram.word(rep - 8),
            o.ram.word(rep - 4),
            o.ram.word(rep)
        ),
        (0, 0, 0xffff_ffff, 0)
    );
    assert_eq!(
        o.ram.at(u.symbols["mf_empty"]).map(|a| o.ram.0[a]),
        Some(0),
        "the empty C string"
    );
}

fn o_name_offset(entries: &[Entry], k: usize) -> u32 {
    entries[..k]
        .iter()
        .map(|e| (e.name.len() + e.short.len() + 2) as u32)
        .sum()
}

const ENTRIES_MR: [Entry; 3] = [
    Entry {
        name: "XSlice",
        short: "XSL",
        f: [0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0, 0xFE, 0x0A],
        clone_of: 6,
        position: 7,
        own_dsp: 0,
    },
    Entry {
        name: "Pluck",
        short: "PLK",
        f: [0xCA, 0x113, 0x114, 0, 0x115, 0x116, 0, 0xD1, 0x0A],
        clone_of: 0,
        position: 2,
        own_dsp: 0,
    },
    Entry {
        name: "XClone",
        short: "XCL",
        f: [0xF8, 0xF9, 0, 0xFB, 0xFC, 0xFD, 0, 0xFE, 0x0A],
        clone_of: 6,
        position: 255,
        own_dsp: 0,
    },
];
const FIRST_MR: u32 = 7;

const ENTRIES_PG: [Entry; 3] = [
    Entry {
        name: "FM Clone",
        short: "FMC",
        f: [0; 9],
        clone_of: 0,
        position: 4,
        own_dsp: 0,
    },
    Entry {
        name: "Sine",
        short: "SIN",
        f: [0; 9],
        clone_of: 0,
        position: 4,
        own_dsp: 1,
    },
    Entry {
        name: "Mid Clone",
        short: "MCL",
        f: [0; 9],
        clone_of: 4,
        position: 255,
        own_dsp: 0,
    },
];
const FIRST_PG: u32 = 5;

#[test]
fn the_dispatch_answers_a_type_of_the_registry_with_its_row_and_gives_a_row_without_strings_the_empty_ones()
 {
    check_dispatch(
        "runtime/platform/rows/src/cf/type-dispatch-dt2.s",
        FIRST_MR,
        &ENTRIES_MR,
    );
    check_dispatch(
        "runtime/platform/rows/src/cf/type-dispatch-dn2.s",
        FIRST_PG,
        &ENTRIES_PG,
    );
}

// ---- the slot-map hooks ---------------------------------------------------------------------------------------------------

const HOOKS_MR: &str = "services/machines/dks/src/hooks-dt2.s";
const ABSENT: &str = "tests/fixtures/native-page-absent.s";
const SLOTMAP: u32 = W + 0x10_2000;
const TYPE_FN: u32 = W + 0x12_1000;
const TAIL_TYPEARG: &str = "dk_tail_09514a38";

fn unit_mr() -> Option<Unit> {
    unit(
        &[HOOKS_MR, ABSENT],
        &[
            ("DKRT_MF_TAB", TAB),
            ("DKRT_MF_RANK", RANK),
            ("DKRT_MF_RANKN", RANKN),
            ("mf_spt", TYPE_FN),
        ],
        &[
            "mf_x_rank",
            TAIL_TYPEARG,
            "mf_x_typearg",
            "mf_x_c5_ne",
            "mf_x_c5_eq",
            "mf_x_sp_stock",
            "mf_x_sp_alt",
        ],
    )
}

/// The memory of a slot-map case: the rows, the table, and the slot map of the image.
fn mem_mr() -> Vec<(u32, Vec<u8>)> {
    let mut m = rows(&ENTRIES_MR, &|_| (0, 0));
    m.push(table(FIRST_MR, ENTRIES_MR.len() as u32, SLOTMAP, 0, 0));
    // descriptor field offset of mirror index 25 .. 34 (-1: none)
    let map: [i32; 10] = [8, 12, 16, 20, -1, -1, 24, 28, 32, 36];
    m.push((SLOTMAP, map.iter().flat_map(|x| be(*x as u32)).collect()));
    m
}

#[test]
fn the_rank_hook_gives_the_insert_the_pairs_of_the_service() {
    let Some(u) = unit_mr() else { return };
    let mut mem = mem_mr();
    mem.push((RANKN, be(9).to_vec()));
    let o = u.run(
        "mf_rank",
        &Case {
            stack: vec![0x1111, 0x2222, 0x3333],
            mem,
            ..Case::default()
        },
    );
    assert_eq!(o.stub, "mf_x_rank");
    // the map argument is untouched; begin and end cover the 9 pairs of 8 bytes
    assert_eq!(o.stack[..3], [0x1111, RANK, RANK + 8 * 9]);
}

#[test]
fn the_masked_clone_test_maps_added_types_to_their_clone() {
    let Some(u) = unit_mr() else { return };
    let count = ENTRIES_MR.len() as u32;
    for t in [
        0,
        4,
        6,
        FIRST_MR,
        FIRST_MR + 1,
        FIRST_MR + 2,
        FIRST_MR + count,
        77,
    ] {
        let o = u.run(
            "mf_clone5",
            &Case {
                d: [t, 9, 0, 33, 0, 0, 0, 0],
                a: [5, 0, 0, 0, 0, 0, 0, 0],
                mem: mem_mr(),
                ..Case::default()
            },
        );
        let want = if (FIRST_MR..FIRST_MR + count).contains(&t) {
            ENTRIES_MR[(t - FIRST_MR) as usize].clone_of
        } else {
            t
        };
        assert_eq!(
            o.stub,
            if want & !2 == 4 {
                "mf_x_c5_eq"
            } else {
                "mf_x_c5_ne"
            },
            "type {t}"
        );
        assert_eq!(o.d[0], want & !2, "type {t}");
        // d1 holds 4: the displaced `moveq #4,D1` of the stock test; the others are kept
        assert_eq!((o.d[1], o.d[3], o.a[0]), (4, 33, 5), "type {t}");
    }
}

#[test]
fn the_type_argument_answers_fields_for_added_types_and_hands_the_rest_on() {
    let Some(u) = unit_mr() else { return };
    let count = ENTRIES_MR.len() as u32;
    let slotmap: [Option<u32>; 10] = [
        Some(8),
        Some(12),
        Some(16),
        Some(20),
        None,
        None,
        Some(24),
        Some(28),
        Some(32),
        Some(36),
    ];
    for k in 0..count {
        for idx in 20..40u32 {
            let stack = vec![0, 0, 0, 0, FIRST_MR + k, 0xBBBB];
            let o = u.run(
                "mf_typearg",
                &Case {
                    d: [0, 0x77, 0x88, 0, 0, 0, 0, 0],
                    a: [idx, 0, 0x99, 0, 0, 0, 0, 0],
                    stack,
                    mem: mem_mr(),
                    ..Case::default()
                },
            );
            let e = &ENTRIES_MR[k as usize];
            assert_eq!(
                (o.d[1], o.d[2], o.a[0], o.a[2]),
                (0x77, 0x88, idx, 0x99),
                "row {k} index {idx}"
            );
            if (25..=34).contains(&idx) && slotmap[(idx - 25) as usize].is_some() {
                assert_eq!(o.stub, "mf_x_typearg", "row {k} index {idx}");
                assert_eq!(
                    o.d[0],
                    e.f[((slotmap[(idx - 25) as usize].unwrap() - 8) / 4) as usize],
                    "row {k} index {idx}"
                );
            } else if (25..=34).contains(&idx) {
                // slots 29 and 30: the clone's type replaces the argument
                assert_eq!(
                    (o.stub.as_str(), o.stack[4]),
                    (TAIL_TYPEARG, e.clone_of),
                    "row {k} index {idx}"
                );
            } else {
                assert_eq!(
                    (o.stub.as_str(), o.stack[4]),
                    (TAIL_TYPEARG, FIRST_MR + k),
                    "row {k} index {idx}"
                );
            }
        }
    }
    for t in [0, 6, FIRST_MR + count] {
        let o = u.run(
            "mf_typearg",
            &Case {
                d: [0, 0x77, 0x88, 0, 0, 0, 0, 0],
                a: [27, 0, 0x99, 0, 0, 0, 0, 0],
                stack: vec![0, 0, 0, 0, t, 0xBBBB],
                mem: mem_mr(),
                ..Case::default()
            },
        );
        assert_eq!(o.stub, TAIL_TYPEARG, "type {t}");
    }
}

const VIEW: u32 = W + 0x13_0000;
const PAGES: u32 = VIEW + 0x200;

/// A stand-in SourcePageView: +0x7c begin, +0x80 end of the page vector, +0x90 the page index.
fn run_src_page(u: &Unit, typ: i32, end_entries: u32, idx: u32) -> (String, u32, u32) {
    let mut view = vec![0u8; 0x100];
    view[0x7c..0x80].copy_from_slice(&be(PAGES));
    view[0x80..0x84].copy_from_slice(&be(PAGES + 4 * end_entries));
    view[0x84..0x88].copy_from_slice(&be(PAGES + 8));
    view[0x90..0x94].copy_from_slice(&be(idx));
    // the stand-in of the type function: `move.l #type,d0; rts`
    let mut f = vec![0x20, 0x3c];
    f.extend_from_slice(&be(typ as u32));
    f.extend_from_slice(&[0x4e, 0x75]);
    let mut mem = mem_mr();
    mem.push((VIEW, view));
    mem.push((TYPE_FN, f));
    let o = u.run(
        "mf_srcpage",
        &Case {
            a: [0, 0, VIEW, 0, 0, 0, 0, 0],
            stack: vec![0xAAAA, 0xBBBB],
            mem,
            ..Case::default()
        },
    );
    let (begin, end, cap) = (
        o.ram.word(VIEW + 0x7c),
        o.ram.word(VIEW + 0x80),
        o.ram.word(VIEW + 0x84),
    );
    assert_eq!(
        (begin, cap),
        (PAGES, PAGES + 8),
        "begin and capacity are never touched"
    );
    assert_eq!(o.a[2], VIEW, "the view register is kept");
    assert_eq!(o.stack[0], 0xAAAA, "nothing is left on the stack");
    (o.stub, (end - begin) / 4, o.ram.word(VIEW + 0x90))
}

/// SourcePageView::vfunc_4, first test `tst.l 0x90(A2); beq.w T`: an added type (>= machines.first) gets page 0 and a one-entry page vector (no page dots, the page key has nothing to flip to) and
/// takes the knob page; every other type, including -1, gets the two-entry vector back and the stock path (page 1 = the sample view stays).
#[test]
fn the_src_page_hook_gives_added_types_one_page_and_stock_types_two() {
    let Some(u) = unit_mr() else { return };
    for typ in [FIRST_MR as i32, 9, 18, 23] {
        for (end_entries, idx) in [(2, 0), (2, 1), (1, 0)] {
            assert_eq!(
                run_src_page(&u, typ, end_entries, idx),
                ("mf_x_sp_alt".to_string(), 1, 0),
                "{typ} {end_entries} {idx}"
            );
        }
    }
    for typ in [0, 3, 6, -1] {
        // page 0: the knob page; page 1: the sample view (stock continues at site + 8); a vector cut by an added type is restored
        assert_eq!(
            run_src_page(&u, typ, 2, 0),
            ("mf_x_sp_alt".to_string(), 2, 0),
            "{typ}"
        );
        assert_eq!(
            run_src_page(&u, typ, 2, 1),
            ("mf_x_sp_stock".to_string(), 2, 1),
            "{typ}"
        );
        assert_eq!(
            run_src_page(&u, typ, 1, 0),
            ("mf_x_sp_alt".to_string(), 2, 0),
            "{typ}"
        );
    }
}

// ---- the page-row hooks ---------------------------------------------------------------------------------------------------

const HOOKS_PG: &str = "services/machines/dks/src/hooks-dn2.s";
const OBJ: u32 = W + 0x12_4000;
const PGROWS: u32 = W + 0x12_5000;
const PGSTR: u32 = W + 0x12_6000;
const PGSIZE: u32 = 84;
/// The call sites of the stock type reader (`mf_rc<n>`, the return addresses the hook treats as identity callers) and the lengths of the calls.
const RC: [u32; 9] = [
    0x1000, 0x1100, 0x1200, 0x1300, 0x1400, 0x1500, 0x1600, 0x1700, 0x1800,
];
const RC_LEN: [u32; 9] = [4, 4, 6, 6, 6, 6, 6, 6, 6];
const TAILS: [(&str, &str); 3] = [
    ("mf_typemap0", "dk_tail_02e8a3f1"),
    ("mf_typemap1", "dk_tail_763365db"),
    ("mf_typemap2", "dk_tail_42d651c5"),
];
/// Where the list copy of the test ends: the stand-in of `dks_list_copy`.
const COPY_FN: u32 = W + 0x12_7000;

fn unit_pg() -> Option<Unit> {
    let mut values: Vec<(String, u32)> = vec![
        ("DKRT_MF_TAB".into(), TAB),
        ("dks_list_copy".into(), COPY_FN),
    ];
    for (i, a) in RC.iter().enumerate() {
        values.push((format!("mf_rc{i}"), *a));
    }
    let values: Vec<(&str, u32)> = values.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    unit(
        &[HOOKS_PG, ABSENT],
        &values,
        &[
            "mf_x_g_next",
            "mf_x_g_taken",
            "dk_tail_02e8a3f1",
            "dk_tail_763365db",
            "dk_tail_42d651c5",
            "mf_ret_tm0",
            "mf_ret_tm1",
            "mf_ret_tm2",
            "mf_x_ft",
            "mf_x_rd",
            "mf_x_ow",
            "mf_x_copy",
        ],
    )
}

fn mem_pg() -> Vec<(u32, Vec<u8>)> {
    let mut m = rows(&ENTRIES_PG, &|_| (0, 0));
    m.push(table(FIRST_PG, ENTRIES_PG.len() as u32, 0, PGROWS, 0));
    m
}

fn clone_of(t: u32) -> u32 {
    let n = ENTRIES_PG.len() as u32;
    if (FIRST_PG..FIRST_PG + n).contains(&t) {
        ENTRIES_PG[(t - FIRST_PG) as usize].clone_of
    } else {
        t
    }
}

#[test]
fn group_sends_added_types_to_the_group_one_path() {
    let Some(u) = unit_pg() else { return };
    for t in 0..12u32 {
        let o = u.run(
            "mf_group",
            &Case {
                d: [t, 5, 0, 0, 0, 0, 0, 0],
                mem: mem_pg(),
                ..Case::default()
            },
        );
        let d0 = if (FIRST_PG..FIRST_PG + 3).contains(&t) {
            0
        } else {
            t
        };
        // the stock test `moveq #3,D1; cmp.l D0,D1; bge.b T`: taken for the types 0 to 3 and for every added type
        assert_eq!(
            o.stub,
            if d0 <= 3 {
                "mf_x_g_taken"
            } else {
                "mf_x_g_next"
            },
            "type {t}"
        );
        assert_eq!(o.d[0], d0, "type {t}");
    }
}

#[test]
fn the_type_maps_replace_the_argument_by_clone_of() {
    let Some(u) = unit_pg() else { return };
    for (name, off) in [
        ("mf_typemap0", 4usize),
        ("mf_typemap1", 8),
        ("mf_typemap2", 0xc),
    ] {
        let tail = TAILS.iter().find(|(n, _)| *n == name).unwrap().1;
        for t in [0, 4, FIRST_PG, FIRST_PG + 1, FIRST_PG + 2, FIRST_PG + 3, 99] {
            let mut stack = vec![0xAAAAu32; 8];
            stack[off / 4] = t;
            if name == "mf_typemap2" {
                stack[2] = 9; // the slot argument, which the stock code has loaded into a0
            }
            let d1 = if name == "mf_typemap2" { 0x64 } else { 8 };
            let o = u.run(
                name,
                &Case {
                    d: [7, d1, 0, 0, 0, 0, 0, 0],
                    a: [9, 0, 0, 0, 0, 0, 0, 0],
                    stack: stack.clone(),
                    mem: mem_pg(),
                    ..Case::default()
                },
            );
            assert_eq!(o.stub, tail, "{name} type {t}");
            assert_eq!(o.stack[off / 4], clone_of(t), "{name} type {t}");
            if name == "mf_typemap2" {
                // d1 = 0x64 and a0 = the slot are set before this site and used after it
                assert_eq!((o.d[1], o.a[0]), (0x64, 9), "type {t}");
            }
            // nothing else of the stack changes
            for i in 0..8 {
                if i != off / 4 {
                    assert_eq!(o.stack[i], stack[i], "{name} type {t} word {i}");
                }
            }
        }
    }
}

#[test]
fn the_frame_type_sends_the_clone_unless_the_machine_has_its_own_dsp() {
    let Some(u) = unit_pg() else { return };
    for t in [
        0u32,
        3,
        4,
        FIRST_PG,
        FIRST_PG + 1,
        FIRST_PG + 2,
        FIRST_PG + 3,
        200,
    ] {
        let mut mem = mem_pg();
        mem.push((OBJ + 0xD8C, vec![t as u8]));
        let o = u.run(
            "mf_frametype",
            &Case {
                d: [0x1234_5678, 5, 6, 0, 0, 0, 0, 0],
                a: [OBJ, 7, 0, 0, 0, 0, 0, 0],
                mem,
                ..Case::default()
            },
        );
        let n = ENTRIES_PG.len() as u32;
        let want = if (FIRST_PG..FIRST_PG + n).contains(&t) {
            let e = &ENTRIES_PG[(t - FIRST_PG) as usize];
            if e.own_dsp != 0 { t } else { e.clone_of }
        } else if t < 128 {
            t
        } else {
            t.wrapping_sub(256)
        };
        assert_eq!(o.stub, "mf_x_ft", "type {t}");
        assert_eq!(o.d[0] & 0xffff, want & 0xffff, "type {t}");
        assert_eq!(o.d[1..3], [5, 6], "type {t}");
    }
}

#[test]
fn the_type_reader_maps_except_for_the_identity_callers() {
    let Some(u) = unit_pg() else { return };
    let n = ENTRIES_PG.len() as u32;
    for t in [
        0i32,
        4,
        FIRST_PG as i32,
        FIRST_PG as i32 + 1,
        FIRST_PG as i32 + 2,
        FIRST_PG as i32 + 3,
        -3,
    ] {
        let identity: Vec<u32> = (0..9).map(|i| RC[i] + RC_LEN[i]).collect();
        let mut callers = vec![0x777];
        callers.extend(identity.iter().copied());
        callers.push(RC[0]);
        for caller in callers {
            let mut mem = mem_pg();
            mem.push((OBJ + 0xDE, vec![t as u8]));
            let o = u.run(
                "mf_reader",
                &Case {
                    d: [1, 2, 0, 0, 0, 0, 0, 0],
                    a: [OBJ, 0x55, 0, 0, 0, 0, 0, 0],
                    stack: vec![0x99, caller],
                    mem,
                    ..Case::default()
                },
            );
            let real = identity.contains(&caller);
            let tt = t as u32;
            let want = if (FIRST_PG..FIRST_PG + n).contains(&tt) && !real {
                ENTRIES_PG[(tt - FIRST_PG) as usize].clone_of
            } else {
                tt
            };
            assert_eq!(o.stub, "mf_x_rd");
            assert_eq!(o.d[0], want, "type {t} caller {caller:#x}");
            assert_eq!((o.a[1], o.stack[0], o.stack[1]), (0x55, 0x99, caller));
        }
    }
}

#[test]
fn the_owner_test_maps_d3() {
    let Some(u) = unit_pg() else { return };
    for t in [0u32, 2, FIRST_PG, FIRST_PG + 1, FIRST_PG + 2, FIRST_PG + 3] {
        let mut mem = mem_pg();
        mem.push((OBJ + 0xDE, vec![t as u8]));
        let o = u.run(
            "mf_owner",
            &Case {
                d: [11, 0, 0x4242, 9, 0, 0, 0, 0],
                a: [OBJ, 3, 0, 0, 0, 0, 0, 0],
                stack: vec![1, 2],
                mem,
                ..Case::default()
            },
        );
        assert_eq!(
            (o.stub.as_str(), o.d[3]),
            ("mf_x_ow", clone_of(t)),
            "type {t}"
        );
        assert_eq!((o.d[0], o.a[1]), (11, 3));
        // d2 is pushed
        assert_eq!((o.stack[0], o.stack[1], o.stack[2]), (0x4242, 1, 2));
    }
}

/// The list getter's copy: the hook replaces the library call and the store after it; the arguments are on the stack as at the original call, and the result of the copy is stored in the vector.
#[test]
fn the_list_hook_calls_the_copy_and_stores_its_end_in_the_vector() {
    let Some(u) = unit_pg() else { return };
    // the stand-in of `dks_list_copy`: `move.l #END,d0; rts`; a2 is the vector being returned, the three arguments are on the stack
    let end = W + 0x14_0000;
    let mut f = vec![0x20, 0x3c];
    f.extend_from_slice(&be(end));
    f.extend_from_slice(&[0x4e, 0x75]);
    let vector = W + 0x14_1000;
    let o = u.run(
        "mf_copy",
        &Case {
            a: [0, 0, vector, 0, 0, 0, 0, 0],
            stack: vec![0x111, 0x222, 0x333],
            mem: vec![(COPY_FN, f)],
            ..Case::default()
        },
    );
    assert_eq!(o.stub, "mf_x_copy");
    assert_eq!(
        o.ram.word(vector + 4),
        end,
        "the end of the copy goes in the vector"
    );
    assert_eq!(
        o.stack[..3],
        [0x111, 0x222, 0x333],
        "the arguments are as they were"
    );
    assert_eq!(o.a[2], vector);
}

// ---- own parameter pages (registry `pages`) ----------------------------------------------------------------------------------

/// (machine type, name, title, nine codes, nine slots): FM Clone has two pages, Sine one, Mid Clone none.
fn pages() -> [(u32, &'static str, &'static str, [u32; 9], [u32; 9]); 3] {
    [
        (
            FIRST_PG,
            "PGA",
            "Pages Test",
            [0x141, 0x142, 0x143, 0x144, 0, 0, 0, 0, 10],
            [25, 26, 27, 30, 0, 0, 0, 0, 0],
        ),
        (
            FIRST_PG,
            "PGB",
            "Pages Test",
            [0x145, 0x146, 0, 0, 0, 0, 0, 0, 10],
            [40, 41, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            FIRST_PG + 1,
            "SIN",
            "Sine",
            [0xD4, 0, 0, 0, 0, 0, 0, 0, 10],
            [35, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
    ]
}

fn mem_pages() -> Vec<(u32, Vec<u8>)> {
    let mut m = rows(&ENTRIES_PG, &|_| (0, 0));
    m.push(table(
        FIRST_PG,
        ENTRIES_PG.len() as u32,
        0,
        PGROWS,
        pages().len() as u32,
    ));
    let (mut rows, mut strings) = (vec![], vec![]);
    for (mach, name, title, codes, slots) in pages() {
        let n = PGSTR + strings.len() as u32;
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        let t = PGSTR + strings.len() as u32;
        strings.extend_from_slice(title.as_bytes());
        strings.push(0);
        rows.extend_from_slice(&be(n));
        rows.extend_from_slice(&be(t));
        for c in codes {
            rows.extend_from_slice(&be(c));
        }
        rows.extend_from_slice(&be(mach));
        for s in slots {
            rows.extend_from_slice(&be(s));
        }
    }
    assert_eq!(rows.len() as u32, PGSIZE * pages().len() as u32);
    m.push((PGROWS, rows));
    m.push((PGSTR, strings));
    m
}

#[test]
fn the_page_count_is_the_pages_of_the_type_and_the_clone_mapping_for_the_rest() {
    let Some(u) = unit_pg() else { return };
    for t in [0, 4, FIRST_PG, FIRST_PG + 1, FIRST_PG + 2, FIRST_PG + 3] {
        let o = u.run(
            "mf_typemap0",
            &Case {
                d: [0, 8, 0, 0, 0, 0, 0, 0],
                a: [9, 0, 0, 0, 0, 0, 0, 0],
                stack: vec![RTS, t],
                mem: mem_pages(),
                ..Case::default()
            },
        );
        let own = if t == FIRST_PG {
            2
        } else if t == FIRST_PG + 1 {
            1
        } else {
            0
        };
        if own != 0 {
            assert_eq!((o.stub.as_str(), o.d[0]), ("mf_ret_tm0", own), "type {t}");
        } else {
            // no own pages: the clone mapping and the stock code
            assert_eq!(
                (o.stub.as_str(), o.stack[1]),
                ("dk_tail_02e8a3f1", clone_of(t)),
                "type {t}"
            );
        }
    }
}

#[test]
fn the_page_descriptor_is_the_row_in_range_and_the_stock_answer_out_of_it() {
    let Some(u) = unit_pg() else { return };
    let case = |t: u32, page: i32| Case {
        d: [0, 0, 0xD2D2, 0, 0, 0, 0, 0],
        stack: vec![0xD2D2, RTS, t, page as u32],
        mem: mem_pages(),
        ..Case::default()
    };
    // in range: the descriptor is the row, returned by an rts with d2 restored from the stack
    for (k, (t, page)) in [(FIRST_PG, 0), (FIRST_PG, 1), (FIRST_PG + 1, 0)]
        .into_iter()
        .enumerate()
    {
        let o = u.run("mf_typemap1", &case(t, page));
        assert_eq!(
            (o.stub.as_str(), o.d[0], o.d[2]),
            ("mf_ret_tm1", PGROWS + k as u32 * PGSIZE, 0xD2D2),
            "type {t} page {page}"
        );
    }
    // out of range (and not own): the stock code answers (MIDI's row for a type above 4, the clone's page otherwise)
    let out: Vec<Out> = [(FIRST_PG, 2), (FIRST_PG, -1), (FIRST_PG + 2, 0), (0, 0)]
        .into_iter()
        .map(|(t, p)| u.run("mf_typemap1", &case(t, p)))
        .collect();
    assert!(out.iter().all(|o| o.stub == "dk_tail_763365db"));
    assert_eq!(out[0].stack[2], FIRST_PG, "the own type stays above 4");
    assert_eq!(out[1].stack[2], FIRST_PG);
    assert_eq!(
        out[2].stack[2], ENTRIES_PG[2].clone_of,
        "Mid Clone: its clone_of"
    );
}

// ---- the registry tables the image copies ------------------------------------------------------------------------------------

struct Ctx {
    symbols: BTreeMap<&'static str, i64>,
    mem: Vec<(i64, Vec<u8>)>,
    anchors: BTreeMap<&'static str, i64>,
    fields: BTreeMap<&'static str, i64>,
}

impl Env for Ctx {
    fn symbol(&self, name: &str) -> Option<i64> {
        self.symbols.get(name).copied()
    }
    fn field(&self, name: &str) -> Option<i64> {
        self.fields.get(name).copied()
    }
    fn image(&self, anchor: &str, off: i64, width: u8) -> patchkit::Result<i64> {
        let at = self.anchors[anchor] + off;
        for (base, bytes) in &self.mem {
            if at >= *base && at + width as i64 <= *base + bytes.len() as i64 {
                let s = (at - base) as usize;
                return Ok(bytes[s..s + width as usize]
                    .iter()
                    .fold(0i64, |v, b| v << 8 | *b as i64));
            }
        }
        panic!("read of {width} bytes at {at:#x}: outside the stock bytes");
    }
}

/// The rows the copies of the stock tables get for a registry row: the clone's name hint, permission mask, per-type byte and DSP selector word, and the lengths of the copies.
#[test]
fn the_stock_table_copies_append_the_clones_row() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let section = |module: &str, name: &str| {
        lib.get(module)
            .unwrap()
            .manifest
            .section
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("{module} has no section {name}"))
    };
    let (names, permit, pertype) = (
        section("core/rows", "mf_names"),
        section("core/rows", "mf_permit"),
        section("core/rows", "mf_pertype"),
    );
    let selector = section("core/dyndsp", "mf_selector");
    let first = FIRST_MR as i64;
    let anchors: BTreeMap<&str, i64> = [
        ("cf.machine.data.name.table", 0x1000),
        ("cf.machine.data.permit.table", 0x2000),
        ("cf.machine.data.pertype.table", 0x3000),
        ("dsp.machine.data.selector.table", 0x4000),
        ("cf.machine.fn.dispatch", 0x9000),
    ]
    .into_iter()
    .collect();
    let mut mem: Vec<(i64, Vec<u8>)> = vec![];
    for t in 0..first {
        mem.push((0x1000 + 12 * t + 8, be(0xA0 + t as u32).to_vec()));
        mem.push((0x2000 + 4 * t, be(0xB0 + t as u32).to_vec()));
        mem.push((0x3000 + t, vec![0xC0 + t as u8]));
        mem.push((0x4000 + 4 * t, be(0xD0 + t as u32).to_vec()));
    }
    mem.push((0x9001, vec![(first - 1) as u8])); // the dispatch's `moveq #last,D1`
    let eval = |text: &str, fields: &[(&'static str, i64)], c: &mut Ctx| {
        c.fields = fields.iter().copied().collect();
        expr::eval_str(text, c).unwrap()
    };
    let mut ctx = Ctx {
        symbols: [
            ("machines.first", first),
            ("machines.count", ENTRIES_MR.len() as i64),
        ]
        .into_iter()
        .collect(),
        mem,
        anchors,
        fields: BTreeMap::new(),
    };
    let rows_of = |s: &patchkit::module::SectionDef| {
        s.append
            .as_ref()
            .unwrap()
            .row
            .iter()
            .map(|e| e.text())
            .collect::<Vec<_>>()
    };
    for e in &ENTRIES_MR {
        let f = [
            ("clone_of", e.clone_of as i64),
            ("name", 0x500),
            ("short", 0x600),
        ];
        let c = e.clone_of as i64;
        let got: Vec<i64> = rows_of(&names)
            .iter()
            .map(|t| eval(t, &f, &mut ctx))
            .collect();
        assert_eq!(got, [0x500, 0x600, 0xA0 + c]);
        assert_eq!(eval(&rows_of(&permit)[0], &f, &mut ctx), 0xB0 + c);
        // the per-type byte of a slot-map device is a `u8:` row
        let t = rows_of(&pertype)[0].trim_start_matches("u8:").to_string();
        assert_eq!(eval(&t, &f, &mut ctx), 0xC0 + c);
        assert_eq!(eval(&rows_of(&selector)[0], &f, &mut ctx), 0xD0 + c);
    }
    // the copies hold the stock types: one row for each (the dispatch's `moveq #last` plus one)
    let len = names.from.len.as_ref().unwrap().text();
    assert_eq!(eval(&len, &[], &mut ctx), first * 12);
}
