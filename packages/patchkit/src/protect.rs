//! The pack-time guard of content modules: a content module never touches hardware (docs/design/module-sdk.md "Modules never touch hardware").
//! Each device file lists address windows (`[protect] windows`, `devdata::DeviceInfo::protect`) that only the firmware and `core/*` drive: the
//! flash controller, the buses to the FPGA and the peer CPU. [`check`] refuses a ColdFire object that holds an address inside one of them, for
//! every device the module is built for.
//!
//! What it sees: a module reaches hardware only through a 32-bit address held in its code or data (the loader gives it no firmware symbols; a
//! content module imports from the runtime table only, so there is no routine name to check). The check scans
//! * every relocation: the addend of a `cf.32` relocation, and
//! * every 32-bit big-endian word of a data section at a 4-byte step, and in code every word that follows an opcode word with a 32-bit
//!   operand ([`takes_long`]: an absolute-long or immediate operand, `lea`, `jsr`, `move` to an absolute-long destination, the `addi` family).
//!   Any other 2-byte pair of code is two instruction words that only look like an address.
//!
//! Windows that start below [`MIN_CHECKED`] are not checked: a word below 64 KiB is an ordinary offset or constant, so a window there (the
//! Syntakt's FlexBus CS1 at address 0) would refuse every module. They stay in the device data for the record.

use crate::dko::Dko;
use crate::error::*;

/// The lowest window start the guard checks.
pub const MIN_CHECKED: u64 = 0x1_0000;

/// A protected window of one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window<'a> {
    pub device: &'a str,
    pub lo: u64,
    pub hi: u64,
}

/// The windows of the devices `short`s that the guard checks.
pub fn windows_of<'a>(
    devices: impl IntoIterator<Item = &'a crate::devdata::DeviceInfo>,
) -> Vec<Window<'a>> {
    devices
        .into_iter()
        .flat_map(|d| {
            d.protect
                .iter()
                .filter(|w| w[0] >= MIN_CHECKED)
                .map(|w| Window {
                    device: d.short.as_str(),
                    lo: w[0],
                    hi: w[1],
                })
        })
        .collect()
}

/// Whether the opcode word `w` is followed by a 32-bit extension word: a source or destination of mode 7 with absolute long (register 1) or
/// immediate (register 4) addressing, or an immediate-to-register instruction. A ColdFire immediate `.l` operand has this form wherever the
/// compiler or an assembler puts it.
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

fn hit<'a>(ws: &'a [Window<'a>], v: u64) -> Option<&'a Window<'a>> {
    ws.iter().find(|w| w.lo <= v && v < w.hi)
}

/// Refuse `obj` (module `id`) if it holds an address of a protected window.
pub fn check(id: &str, obj: &Dko, ws: &[Window<'_>]) -> Result<()> {
    if obj.arch != "cf" || ws.is_empty() {
        return Ok(());
    }
    let refuse = |what: String, v: u64, w: &Window<'_>| {
        Err(err!(
            "{id}: {what} holds {v:#010x}, inside the protected window [{:#x}, {:#x}) of {} (the flash controller, the FPGA or the peer \
             CPU bus; docs/findings/141-syntakt-analog-and-nor-paths.md). A content module never touches hardware: use the runtime's services",
            w.lo,
            w.hi,
            w.device
        ))
    };
    for r in &obj.relocs {
        if r.rtype == "cf.32"
            && let Some(w) = hit(ws, r.addend as u32 as u64)
        {
            return refuse(
                format!("the relocation against {} in {}", r.symbol, r.section),
                r.addend as u32 as u64,
                w,
            );
        }
    }
    for s in obj.sections.iter().filter(|s| s.kind != "bss") {
        let b = unhex(&s.hex)?;
        let text = s.kind == "text";
        let mut off = 0;
        while off + 4 <= b.len() {
            if text && (off < 2 || !takes_long(u16::from_be_bytes([b[off - 2], b[off - 1]]))) {
                off += 2;
                continue;
            }
            let v = u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as u64;
            if let Some(w) = hit(ws, v) {
                return refuse(format!("section {} at offset {off:#x}", s.name), v, w);
            }
            off += if text { 2 } else { 4 };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dko::{DkoReloc, DkoSection, FORMAT};

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
        .into_iter()
        .filter(|w| w.lo >= MIN_CHECKED)
        .collect()
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
    fn a_window_at_zero_is_not_checked() {
        let o = obj(&[0, 0, 0x10, 0, 0, 0, 0, 0]);
        check("x/m", &o, &ws()).unwrap();
    }
}
