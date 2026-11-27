//! `core/rows` (runtime/platform/rows): the parameter table reservation, host checks without firmware.
//!
//! - the reserved rows are one count from the device file (`[limits]`), and no other module reserves parameter rows;
//! - the row expressions and the write ops of the manifest against an independent row builder (finding 02);
//! - the `pf_t2` routine of the committed object on the ColdFire core (an added code answers with its template's code);
//! - `rows_info` (version, blocks, recipe, cap words) of the committed object against the device file.

use coldfire::{Bus, BusError, Cpu};
use patchkit::dko::Dko;
use patchkit::expr::{self, Env};
use patchkit::link;
use patchkit::module::{Library, Module};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rows_module(lib: &Library) -> &Module {
    lib.get("core/rows").expect("core/rows is in the library")
}

/// The devices core/rows has an object for.
fn devices(m: &Module) -> Vec<String> {
    let mut v: Vec<String> = m
        .manifest
        .object
        .iter()
        .flat_map(|o| o.devices.clone())
        .collect();
    v.sort();
    v.dedup();
    v
}

// ---- the row expressions -------------------------------------------------------------------------------------------

const TABLE: i64 = 0x4020_0000;

/// Stock parameter table of `count` 60-byte rows with distinctive words.
fn stock_table(count: usize) -> Vec<u8> {
    let mut out = vec![];
    for r in 0..count as u32 {
        for w in 0..15u32 {
            let v = r
                .wrapping_mul(0x0101_0101)
                .wrapping_add(w.wrapping_mul(0x0007_0001));
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

/// Names and bytes an expression of the manifest reads; `this` is one anchor address.
struct Ctx<'a> {
    anchors: BTreeMap<&'a str, i64>,
    symbols: BTreeMap<&'a str, i64>,
    mem: Vec<(i64, Vec<u8>)>,
    fields: BTreeMap<&'a str, i64>,
}

impl Env for Ctx<'_> {
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
                let mut v = 0i64;
                for b in &bytes[s..s + width as usize] {
                    v = v << 8 | *b as i64;
                }
                return Ok(v);
            }
        }
        panic!("read of {width} bytes at {at:#x}: outside the stock bytes");
    }
}

fn word(table: &[u8], row: usize, off: usize) -> i64 {
    let at = row * 60 + off;
    u32::from_be_bytes(table[at..at + 4].try_into().unwrap()) as i64
}

/// The appended row of a parameter, word by word, written out independently of the manifest (finding 02: one device's
/// rows are mirrored over the eight descriptor slots, the other's carry the sound-array slot itself).
fn expected_row(
    sound_array_slot: bool,
    table: &[u8],
    t: usize,
    slot: i64,
    min: i64,
    max: i64,
    default: i64,
    name: i64,
    short: i64,
) -> Vec<i64> {
    let w = |off| word(table, t, off);
    if sound_array_slot {
        return vec![
            w(0),
            slot,
            min,
            max,
            default,
            w(0x14),
            w(0x18),
            w(0x1c),
            w(0x20),
            w(0x24),
            name,
            w(0x2c),
            short,
            w(0x34),
            w(0x38),
        ];
    }
    let mirror = 25 + slot + (slot >> 2) * 2;
    let cc = if slot == 3 { 0xffff } else { 0x10 + slot };
    vec![
        w(0),
        mirror,
        min,
        max,
        default,
        w(0x14),
        cc << 16 | (w(0x18) & 0xffff),
        0x80 + slot,
        w(0x20),
        w(0x24),
        name,
        w(0x2c),
        short,
        w(0x34),
        w(0x38),
    ]
}

#[test]
fn the_row_expressions_match_an_independent_row_builder() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let rows = rows_module(&lib);
    let table = stock_table(0x113);
    let sections: Vec<_> = rows
        .manifest
        .section
        .iter()
        .filter(|s| s.name == "pf_table")
        .collect();
    assert!(!sections.is_empty());
    for sec in sections {
        let dev = sec.devices.first().expect("pf_table names its device");
        let append = sec.append.as_ref().expect("pf_table appends rows");
        assert_eq!(append.row.len(), 15, "{dev}");
        // a section either mirrors the slots or takes the sound-array slot itself (25 ..= 64)
        let sound_array_slot = append.row[1].text() == "$slot";
        for template in [0usize, 0xd1, 0x112] {
            for slot in 0..8i64 {
                let ctx = Ctx {
                    anchors: [("cf.param.data.descriptor_table", TABLE)].into(),
                    symbols: BTreeMap::new(),
                    mem: vec![(TABLE, table.clone())],
                    fields: [
                        ("template", template as i64),
                        ("slot", slot),
                        ("min", 0),
                        ("max", 0x7f00),
                        ("default", 0x4000),
                        ("name", 0x47f0_0010),
                        ("short", 0x47f0_0020),
                    ]
                    .into(),
                };
                let got: Vec<i64> = append
                    .row
                    .iter()
                    .map(|e| expr::eval_str(&e.text(), &ctx).unwrap() & 0xffff_ffff)
                    .collect();
                let want = expected_row(
                    sound_array_slot,
                    &table,
                    template,
                    slot,
                    0,
                    0x7f00,
                    0x4000,
                    0x47f0_0010,
                    0x47f0_0020,
                );
                assert_eq!(got, want, "{dev} template {template:#x} slot {slot}");
            }
        }
        // the copied length is the stock count, read from the accessor's compare immediate
        let len = sec
            .from
            .len
            .as_ref()
            .expect("pf_table copies the stock rows")
            .text();
        let ctx = Ctx {
            anchors: [("cf.param.fn.value_class", 0x2000)].into(),
            symbols: BTreeMap::new(),
            mem: vec![(0x2006, 0x113u32.to_be_bytes().to_vec())],
            fields: BTreeMap::new(),
        };
        assert_eq!(expr::eval_str(&len, &ctx).unwrap(), 0x113 * 60, "{dev}");
    }
}

#[test]
fn the_reference_and_bound_writes_move_every_reader_into_the_copy() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let rows = rows_module(&lib);
    for dev in devices(rows) {
        let write = |anchors: &str| {
            let ops: Vec<_> = rows
                .manifest
                .op
                .iter()
                .filter(|o| {
                    o.anchors.as_deref() == Some(anchors) && o.devices.iter().any(|d| *d == dev)
                })
                .collect();
            assert_eq!(ops.len(), 1, "{dev} {anchors}");
            ops[0]
        };
        // a reference into the stock table is repointed at the same offset of the copy
        let refs = write("cf.param.sites.descriptor_table.ref.*");
        assert_eq!((refs.offset, refs.width), (0, Some(4)), "{dev}");
        let old = TABLE as u32 + 120;
        let ctx = Ctx {
            anchors: [("this", 0x1000), ("cf.param.data.descriptor_table", TABLE)].into(),
            symbols: [
                ("pf_table", 0x47f0_0000),
                ("cf.param.data.descriptor_table", TABLE),
            ]
            .into(),
            mem: vec![(0x1000, old.to_be_bytes().to_vec())],
            fields: BTreeMap::new(),
        };
        let v = expr::eval_str(&refs.value.as_ref().unwrap().text(), &ctx).unwrap();
        assert_eq!(v, 0x47f0_0000 + 120, "{dev}");
        // a clamp bound `cmpi.l #N,Dn` (the immediate at +2) grows by the number of added rows
        let bounds = write("cf.param.sites.descriptor_table.bound.*");
        assert_eq!((bounds.offset, bounds.width), (2, Some(4)), "{dev}");
        let ctx = Ctx {
            anchors: [("this", 0x2000)].into(),
            symbols: [("params.count", 3)].into(),
            mem: vec![(0x2002, 0x113u32.to_be_bytes().to_vec())],
            fields: BTreeMap::new(),
        };
        let v = expr::eval_str(&bounds.value.as_ref().unwrap().text(), &ctx).unwrap();
        assert_eq!(v, 0x113 + 3, "{dev}");
    }
}

// ---- the object on the ColdFire core -------------------------------------------------------------------------------

struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read8(&mut self, addr: u32) -> Result<u8, BusError> {
        self.0
            .get(addr as usize)
            .copied()
            .ok_or(BusError { addr, write: false })
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
        *self
            .0
            .get_mut(addr as usize)
            .ok_or(BusError { addr, write: true })? = value;
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

const FIRST: u32 = 0x113;
const ROWS: u32 = 0x8000;
const EDITS: u32 = 0x9000;
const TAIL: u32 = 0xa000;
const ROW_SIZE: u32 = 28;
const BASE: u64 = 0x1000;

/// Registry row layouts of the manifest: the byte offset of a field.
fn field_offset(m: &Module, registry: &str, field: &str) -> u32 {
    let r = m
        .manifest
        .registry
        .iter()
        .find(|r| r.name == registry)
        .unwrap();
    4 * r.field.iter().position(|f| f.name == field).unwrap() as u32
}

/// The object of `dev` named `stem`, linked at `BASE` with `externs` by the name the object uses.
fn linked(m: &Module, dev: &str, stem: &str, externs: &BTreeMap<String, u64>) -> link::Linked {
    let (o, dko): (_, &Dko) = m
        .manifest
        .object
        .iter()
        .zip(&m.objects)
        .find(|(o, _)| o.path.contains(stem) && o.devices.iter().any(|d| d == dev))
        .unwrap_or_else(|| panic!("no {stem} object for {dev}"));
    // the object's own names for its externs: the manifest maps each to a symbol of the image
    let _ = o;
    link::link(dko, BASE, externs).unwrap()
}

/// What `pf_t2` makes of `code`, with `stock_edits` rows of (code, class) behind `ROWS` templates.
fn run_pf_t2(
    m: &Module,
    dev: &str,
    templates: &[u32],
    edits: &[(u32, u32)],
    code: u32,
) -> (u32, u32, u32, u32) {
    let externs: BTreeMap<String, u64> = [
        ("_pf_first", FIRST as u64),
        ("_pf_count", templates.len() as u64),
        ("_pf_rows", ROWS as u64),
        ("_pf_rowsize", ROW_SIZE as u64),
        ("_pe_count", edits.len() as u64),
        ("_pe_rows", EDITS as u64),
        ("_pe_rowsize", ROW_SIZE as u64),
        ("_pf_t_t2", TAIL as u64),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let l = linked(m, dev, "params", &externs);
    let mut ram = Ram(vec![0; 0x10000]);
    ram.0[BASE as usize..BASE as usize + l.image.len()].copy_from_slice(&l.image);
    let template_at = field_offset(m, "params", "template");
    for (i, t) in templates.iter().enumerate() {
        ram.write32(ROWS + i as u32 * ROW_SIZE + template_at, *t)
            .unwrap();
    }
    let (code_at, class_at) = (
        field_offset(m, "param_edits", "code"),
        field_offset(m, "param_edits", "class"),
    );
    for (i, (c, k)) in edits.iter().enumerate() {
        ram.write32(EDITS + i as u32 * ROW_SIZE + code_at, *c)
            .unwrap();
        ram.write32(EDITS + i as u32 * ROW_SIZE + class_at, *k)
            .unwrap();
    }
    let mut cpu = Cpu::new();
    cpu.pc = l.symbols["pf_t2"] as u32;
    cpu.sr = 0x2700;
    cpu.a[7] = 0xf000;
    cpu.d[0] = code;
    cpu.d[1] = 5;
    cpu.a[0] = 9;
    for _ in 0..200 {
        if cpu.pc == TAIL {
            break;
        }
        cpu.step(&mut ram).unwrap();
        assert_eq!(cpu.last_exception, None);
    }
    assert_eq!(cpu.pc, TAIL, "pf_t2 of {dev} must leave through its tail");
    (cpu.d[0], cpu.d[1], cpu.a[0], cpu.a[7])
}

#[test]
fn an_added_code_answers_with_its_template_and_other_codes_are_kept() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = rows_module(&lib);
    let templates = [0xd1, 0x20, 0x112];
    for dev in devices(m) {
        for code in [0, 0x112, FIRST + 3, 0x1000] {
            // below the first added code and past the last: unchanged, and nothing else of the caller changes
            assert_eq!(
                run_pf_t2(m, &dev, &templates, &[], code),
                (code, 5, 9, 0xf000),
                "{dev} {code:#x}"
            );
        }
        for (k, t) in templates.iter().enumerate() {
            assert_eq!(
                run_pf_t2(m, &dev, &templates, &[], FIRST + k as u32),
                (*t, 5, 9, 0xf000),
                "{dev} added {k}"
            );
        }
    }
}

#[test]
fn an_edited_stock_code_answers_with_its_class() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = rows_module(&lib);
    let templates = [0xd1, 0x20, 0x112];
    let edits = [(0x103, 0xd1), (0x20, 0)];
    for dev in devices(m) {
        // an edit with a class: the code's knob cell is drawn as that class; an edit without one, a code with no edit and an added code are as before
        let r = |code| run_pf_t2(m, &dev, &templates, &edits, code).0;
        assert_eq!(r(0x103), 0xd1, "{dev}");
        assert_eq!(r(0x20), 0x20, "{dev}");
        assert_eq!(r(0x102), 0x102, "{dev}");
        assert_eq!(r(FIRST), 0xd1, "{dev}");
        assert_eq!(r(FIRST + 2), 0x112, "{dev}");
    }
}

// ---- rows_info -----------------------------------------------------------------------------------------------------
