//! The pack-time guard of content modules: a content module never touches hardware (docs/design/module-sdk.md "Modules never touch hardware").
//! Each device file lists address windows (`[protect] windows`, `devdata::DeviceInfo::protect`) that only the firmware and `core/*` drive: the
//! flash controller, the buses to the FPGA and the peer CPU. [`check`] refuses a ColdFire object that holds an address inside one of them, for
//! every device the module is built for.
//!
//! What it sees: a module reaches hardware only through a 32-bit address held in its code or data (the loader gives it no firmware symbols; a
//! content module imports from the runtime table only, so there is no routine name to check). The check scans
//! * every relocation: the addend of a `cf.32` relocation, and the value of an absolute symbol (an object symbol with no section and a value
//!   other than 0; an import has value 0) plus the addend,
//! * every 32-bit big-endian word of a data section at a 4-byte step, and
//! * in code, at every 2-byte offset, the 32-bit operands of the instruction that starts there ([`decode`]): an immediate `.l` or an
//!   absolute-long operand of `move`, `lea`, `pea`, `jsr`, `jmp`, the immediate family (`ori` to `cmpi`) and the bit instructions, in any
//!   position of the operand list. Where the decoder knows no instruction, the old opcode rule applies ([`takes_long`]). Scanning every offset
//!   (not a walk from the start) keeps a data word or a jump table in code from putting the scan out of step.
//!
//! A window that starts below [`MIN_CHECKED`] (the Syntakt's FlexBus CS1 at address 0) is checked against absolute symbols only: a literal or
//! an addend below 64 KiB is an ordinary offset or constant, so a check of those would refuse every module.

use crate::dko::Dko;
use crate::error::*;

/// The lowest window start that literals, data words, addends and code operands are checked against.
pub const MIN_CHECKED: u64 = 0x1_0000;

/// A protected window of one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window<'a> {
    pub device: &'a str,
    pub lo: u64,
    pub hi: u64,
}

/// The windows of the devices, low windows included (see [`MIN_CHECKED`]).
pub fn windows_of<'a>(
    devices: impl IntoIterator<Item = &'a crate::devdata::DeviceInfo>,
) -> Vec<Window<'a>> {
    devices
        .into_iter()
        .flat_map(|d| {
            d.protect.iter().map(|w| Window {
                device: d.short.as_str(),
                lo: w[0],
                hi: w[1],
            })
        })
        .collect()
}

/// Whether the opcode word `w` is followed by a 32-bit extension word: the old rule, used where [`decode`] knows no instruction. A source or
/// destination of mode 7 with absolute long (register 1) or immediate (register 4) addressing, or an immediate-to-register instruction.
fn takes_long(w: u16) -> bool {
    let ea = w & 0x3f;
    let dest_abs_long = matches!(w >> 12, 1..=3) && (w >> 6) & 0x3f == 0x0f;
    ea == 0x39
        || ea == 0x3c
        || dest_abs_long
        || matches!(
            w & 0xfff8,
            0x0080 | 0x0280 | 0x0480 | 0x0680 | 0x0a80 | 0x0c80
        )
}

/// The extension of the effective address (`mode`, `reg`) of an operand of `size` bytes at `pos`: advances `pos` and notes the offset of a
/// 32-bit part (absolute long, immediate `.l`) in `longs`. None for a mode that does not exist.
fn ea_ext(mode: u16, reg: u16, size: usize, pos: &mut usize, longs: &mut Vec<usize>) -> Option<()> {
    match (mode, reg) {
        (0..=4, _) => {}
        (5 | 6, _) | (7, 0 | 2 | 3) => *pos += 2,
        (7, 1) => {
            longs.push(*pos);
            *pos += 4;
        }
        (7, 4) if size == 4 => {
            longs.push(*pos);
            *pos += 4;
        }
        (7, 4) => *pos += 2,
        _ => return None,
    }
    Some(())
}

/// A destination operand: no immediate and no PC-relative mode.
fn ea_dest(
    mode: u16,
    reg: u16,
    size: usize,
    pos: &mut usize,
    longs: &mut Vec<usize>,
) -> Option<()> {
    if mode == 7 && matches!(reg, 2..=4) {
        return None;
    }
    ea_ext(mode, reg, size, pos, longs)
}

/// The instruction at `at` for the families that carry a 32-bit address or immediate: its length and the offsets of its 32-bit operands.
/// None when the word is no such instruction or the operands do not fit the buffer.
fn decode(b: &[u8], at: usize) -> Option<(usize, Vec<usize>)> {
    let w = u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]);
    let (m, r) = ((w >> 3) & 7, w & 7);
    let mut pos = at + 2;
    let mut longs = vec![];
    match w >> 12 {
        // move.b/.w/.l (1, 3, 2): the source operand, then the destination operand
        1..=3 => {
            let size = match w >> 12 {
                1 => 1,
                3 => 2,
                _ => 4,
            };
            let (dm, dr) = ((w >> 6) & 7, (w >> 9) & 7);
            ea_ext(m, r, size, &mut pos, &mut longs)?;
            if dm != 1 {
                ea_dest(dm, dr, size, &mut pos, &mut longs)?;
            } else if size == 1 {
                return None;
            }
        }
        0 => {
            if w & 0xf100 == 0x0100 {
                // btst, bchg, bclr, bset Dn,<ea> (mode 1 is movep: not ColdFire)
                if m == 1 {
                    return None;
                }
                ea_ext(m, r, 1, &mut pos, &mut longs)?;
            } else if w & 0xff00 == 0x0800 {
                // btst, bchg, bclr, bset #bit,<ea>
                pos += 2;
                ea_ext(m, r, 1, &mut pos, &mut longs)?;
            } else if matches!(
                w & 0xff00,
                0x0000 | 0x0200 | 0x0400 | 0x0600 | 0x0a00 | 0x0c00
            ) && (w >> 6) & 3 != 3
            {
                // ori, andi, subi, addi, eori, cmpi: the immediate, then the destination
                let size = 1usize << ((w >> 6) & 3);
                if size == 4 {
                    longs.push(pos);
                    pos += 4;
                } else {
                    pos += 2;
                }
                ea_dest(m, r, size, &mut pos, &mut longs)?;
            } else {
                return None;
            }
        }
        4 => {
            let lea = w & 0xf1c0 == 0x41c0;
            let pea = w & 0xffc0 == 0x4840 && m != 0;
            let jump = matches!(w & 0xffc0, 0x4e80 | 0x4ec0);
            if !(lea || pea || jump) {
                return None;
            }
            ea_ext(m, r, 4, &mut pos, &mut longs)?;
        }
        _ => return None,
    }
    (pos <= b.len()).then_some((pos - at, longs))
}

fn hit<'a>(ws: impl IntoIterator<Item = &'a Window<'a>>, v: u64) -> Option<&'a Window<'a>> {
    ws.into_iter().find(|w| w.lo <= v && v < w.hi)
}

/// The section of the generated `dk_workbench_descriptor` array (`workbench_descriptor.h`, one section per data object).
const WORKBENCH_DESCRIPTOR_SECTION: &str = ".rodata.dk_workbench_descriptor";

/// Refuse `obj` (module `id`) if it holds an address of a protected window.
pub fn check(id: &str, obj: &Dko, ws: &[Window<'_>]) -> Result<()> {
    if obj.arch != "cf" || ws.is_empty() {
        return Ok(());
    }
    // literals, data words, addends and code operands are checked against the windows from MIN_CHECKED up
    let high: Vec<&Window<'_>> = ws.iter().filter(|w| w.lo >= MIN_CHECKED).collect();
    let refuse = |what: String, v: u64, w: &Window<'_>| {
        Err(err!(
            "{id}: {what} holds {v:#010x}, inside the protected window [{:#x}, {:#x}) of {} (hardware, or the firmware code that drives the \
             NOR flash; findings 141 and 146). A content module never touches hardware: use the runtime's services. \
             If the word is a table entry that only looks like an address, change the layout of the table",
            w.lo,
            w.hi,
            w.device
        ))
    };
    for r in &obj.relocs {
        if r.rtype != "cf.32" {
            continue;
        }
        let at = format!(
            "the relocation against {} at {}+{:#x}",
            r.symbol, r.section, r.offset
        );
        // an absolute symbol (no section, a value: an import has value 0): its value is an address, so every window applies
        if let Some(sy) = obj
            .symbols
            .iter()
            .find(|s| s.name == r.symbol && s.section.is_none() && s.value != 0)
        {
            let v = (sy.value as u32).wrapping_add(r.addend as u32) as u64;
            if let Some(w) = hit(ws, v) {
                return refuse(format!("{at} (an absolute symbol)"), v, w);
            }
        }
        if let Some(w) = hit(high.iter().copied(), r.addend as u32 as u64) {
            return refuse(at, r.addend as u32 as u64, w);
        }
    }
    let word = |b: &[u8], o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    for s in obj.sections.iter().filter(|s| s.kind != "bss") {
        let b = unhex(&s.hex)?;
        let place = |off: usize, v: u64, w: &Window<'_>| {
            refuse(format!("section {} at offset {off:#x}", s.name), v, w)
        };
        if s.kind != "text" {
            // The Workbench descriptor is a byte string (lengths, bounds, labels) that no code takes as an address: its words are
            // not data words, and a bound such as 0x10000000 would read as the flash window.
            if s.name == WORKBENCH_DESCRIPTOR_SECTION {
                continue;
            }
            for off in (0..b.len().saturating_sub(3)).step_by(4) {
                let v = word(&b, off);
                if let Some(w) = hit(high.iter().copied(), v) {
                    return place(off, v, w);
                }
            }
            continue;
        }
        for at in (0..b.len().saturating_sub(1)).step_by(2) {
            let longs = match decode(&b, at) {
                Some((_, longs)) => longs,
                None if at + 6 <= b.len() && takes_long(u16::from_be_bytes([b[at], b[at + 1]])) => {
                    vec![at + 2]
                }
                None => vec![],
            };
            for off in longs {
                let v = word(&b, off);
                if let Some(w) = hit(high.iter().copied(), v) {
                    return place(off, v, w);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dko::{DkoReloc, DkoSection, DkoSymbol, FORMAT};

    fn obj(text: &[u8]) -> Dko {
        Dko {
            format: FORMAT,
            arch: "cf".into(),
            source_sha256: "0".repeat(64),
            toolchain: Default::default(),
            sections: vec![DkoSection {
                name: ".text".into(),
                kind: "text".into(),
                align: 4,
                size: text.len() as u32,
                hex: hex(text),
            }],
            symbols: vec![],
            relocs: vec![],
        }
    }

    fn ws() -> Vec<Window<'static>> {
        vec![
            Window {
                device: "dev",
                lo: 0,
                hi: 0x2_0000,
            },
            Window {
                device: "dev",
                lo: 0xFC05_C000,
                hi: 0xFC05_D000,
            },
        ]
    }

    #[test]
    fn a_literal_in_a_window_is_refused_after_an_opcode_with_a_long_operand() {
        // move.l #0xFC05C034,A0 = 207C FC05 C034, behind one other word
        let o = obj(&[0x4e, 0x71, 0x20, 0x7c, 0xfc, 0x05, 0xc0, 0x34, 0x4e, 0x75]);
        let e = check("x/m", &o, &ws()).unwrap_err().to_string();
        assert!(
            e.contains("0xfc05c034") && e.contains("0xfc05c000") && e.contains("finding"),
            "{e}"
        );
    }

    #[test]
    fn a_relocation_addend_in_a_window_is_refused() {
        let mut o = obj(&[0, 0, 0, 0]);
        o.relocs.push(DkoReloc {
            section: ".text".into(),
            offset: 0,
            rtype: "cf.32".into(),
            symbol: "buf".into(),
            addend: 0xFC05_C010,
        });
        assert!(check("x/m", &o, &ws()).is_err());
    }

    #[test]
    fn the_workbench_descriptor_bytes_are_not_data_words_but_other_data_is() {
        let bytes = [0x10, 0, 0, 0x04, 0, 0, 0, 0];
        let window = vec![Window {
            device: "dev",
            lo: 0x1000_0000,
            hi: 0x1002_0000,
        }];
        let mut o = obj(&[0x4e, 0x75]);
        o.sections.push(DkoSection {
            name: ".rodata.dk_workbench_descriptor".into(),
            kind: "data".into(),
            align: 1,
            size: 8,
            hex: hex(&bytes),
        });
        check("x/m", &o, &window).unwrap();
        o.sections[1].name = ".rodata.table".into();
        assert!(check("x/m", &o, &window).is_err());
    }

    #[test]
    fn an_ordinary_module_passes() {
        let mut o = obj(&[0x70, 0x00, 0x4e, 0x75, 0x00, 0x01, 0x00, 0x20]);
        o.relocs.push(DkoReloc {
            section: ".text".into(),
            offset: 4,
            rtype: "cf.32".into(),
            symbol: "dk_log".into(),
            addend: 8,
        });
        check("x/m", &o, &ws()).unwrap();
    }

    #[test]
    fn two_instruction_words_that_look_like_an_address_pass() {
        // or.b d0,d6 (0x8c00) then move.l d7,-(sp) (0x2f07): not an operand
        let o = obj(&[0x4e, 0x71, 0x8c, 0x00, 0x2f, 0x07, 0x4e, 0x75]);
        let w = vec![Window {
            device: "dev",
            lo: 0x8c00_0000,
            hi: 0x8c01_0000,
        }];
        check("x/m", &o, &w).unwrap();
    }

    #[test]
    fn a_window_at_zero_is_not_checked_against_literals_or_addends() {
        let o = obj(&[0, 0, 0x10, 0, 0, 0, 0, 0]);
        check("x/m", &o, &ws()).unwrap();
        let mut o = obj(&[0, 0, 0, 0]);
        o.relocs.push(DkoReloc {
            section: ".text".into(),
            offset: 0,
            rtype: "cf.32".into(),
            symbol: ".sec..text".into(),
            addend: 0x1000,
        });
        check("x/m", &o, &ws()).unwrap();
    }

    fn abs_sym(o: &mut Dko, name: &str, value: u64, addend: i64) {
        o.symbols.push(DkoSymbol {
            name: name.into(),
            section: None,
            value,
            global: true,
        });
        o.relocs.push(DkoReloc {
            section: ".text".into(),
            offset: 0,
            rtype: "cf.32".into(),
            symbol: name.into(),
            addend,
        });
    }

    #[test]
    fn a_relocation_against_an_absolute_symbol_in_any_window_is_refused() {
        // the low window [0, 0x20000) and the high window
        let mut o = obj(&[0, 0, 0, 0]);
        abs_sym(&mut o, "cs1", 0x1_0000, 0x10);
        let e = check("x/m", &o, &ws()).unwrap_err().to_string();
        assert!(
            e.contains("absolute symbol") && e.contains("0x00010010") && e.contains(".text+0x0"),
            "{e}"
        );
        let mut o = obj(&[0, 0, 0, 0]);
        abs_sym(&mut o, "nor", 0xFC05_C000, 0x34);
        assert!(check("x/m", &o, &ws()).is_err());
    }

    #[test]
    fn an_absolute_symbol_outside_the_windows_and_an_import_pass() {
        let mut o = obj(&[0, 0, 0, 0]);
        abs_sym(&mut o, "ram", 0x4000_0000, 8);
        check("x/m", &o, &ws()).unwrap();
        // an import has value 0 and a small addend: not an address
        let mut o = obj(&[0, 0, 0, 0]);
        abs_sym(&mut o, "dk_log", 0, 8);
        check("x/m", &o, &ws()).unwrap();
    }

    fn code(ops: &[&[u8]]) -> Vec<u8> {
        let mut v = vec![0x4e, 0x71];
        for o in ops {
            v.extend_from_slice(o);
        }
        v.extend_from_slice(&[0x4e, 0x75]);
        v
    }

    const NOR: [u8; 4] = [0xFC, 0x05, 0xC0, 0x34];
    const RAM: [u8; 4] = [0x40, 0x00, 0x10, 0x00];

    fn refused(text: &[u8]) -> Option<String> {
        check("x/m", &obj(text), &ws()).err().map(|e| e.to_string())
    }

    #[test]
    fn move_immediate_to_absolute_long_checks_the_address() {
        // move.l #0x1234,abs.l = 23FC 0000 1234 <addr>
        let mk = |a: [u8; 4]| code(&[&[0x23, 0xfc, 0, 0, 0x12, 0x34], &a]);
        let e = refused(&mk(NOR)).expect("refused");
        assert!(e.contains("offset 0x8") && e.contains("0xfc05c034"), "{e}");
        assert!(refused(&mk(RAM)).is_none());
    }

    #[test]
    fn move_absolute_to_absolute_checks_the_destination() {
        // move.l abs.l,abs.l = 23F9 <src> <dst>
        let mk = |s: [u8; 4], d: [u8; 4]| code(&[&[0x23, 0xf9], &s, &d]);
        assert!(refused(&mk(RAM, NOR)).is_some());
        assert!(refused(&mk(NOR, RAM)).is_some());
        assert!(refused(&mk(RAM, RAM)).is_none());
    }

    #[test]
    fn a_compare_immediate_with_a_memory_destination_checks_the_immediate() {
        // cmpi.l #imm,(a0) = 0C90 <imm>; cmpi.l #imm,(d16,a0) = 0CA8 <imm> 0010; addi.l #imm,abs.l = 06B9 <imm> <addr>
        assert!(refused(&code(&[&[0x0c, 0x90], &NOR])).is_some());
        assert!(refused(&code(&[&[0x0c, 0x90], &RAM])).is_none());
        assert!(refused(&code(&[&[0x0c, 0xa8], &NOR, &[0, 0x10]])).is_some());
        assert!(refused(&code(&[&[0x0c, 0xa8], &RAM, &[0, 0x10]])).is_none());
        assert!(refused(&code(&[&[0x06, 0xb9], &RAM, &NOR])).is_some());
        assert!(refused(&code(&[&[0x06, 0xb9], &RAM, &RAM])).is_none());
    }

    #[test]
    fn lea_pea_jsr_with_an_absolute_long_operand_are_checked() {
        // lea abs.l,a0 = 41F9; pea abs.l = 4879; jsr abs.l = 4EB9; jmp abs.l = 4EF9
        for op in [[0x41, 0xf9], [0x48, 0x79], [0x4e, 0xb9], [0x4e, 0xf9]] {
            assert!(refused(&code(&[&op, &NOR])).is_some(), "{op:x?}");
            assert!(refused(&code(&[&op, &RAM])).is_none(), "{op:x?}");
        }
    }

    #[test]
    fn the_decoder_gives_length_and_operand_offsets() {
        let b = [0x23, 0xfc, 0, 0, 0x12, 0x34, 1, 2, 3, 4];
        assert_eq!(decode(&b, 0), Some((10, vec![2, 6])));
        // move.l d0,d1 and an unknown word
        assert_eq!(decode(&[0x22, 0x00], 0), Some((2, vec![])));
        assert_eq!(decode(&[0xd0, 0x80], 0), None);
        // truncated operand
        assert_eq!(decode(&[0x23, 0xfc, 0, 0], 0), None);
    }
}
