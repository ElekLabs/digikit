//! DSP patch files: edits addressed in SHARC+ memory terms.
//!
//! ```text
//! # comment (also ';' and '//')
//! BASE <sha256 of the unpatched .syx>          optional; the caller checks it
//! PM <pc> <hex> [was <hex>]    instruction words at short-word PC <pc>; 4, 8 or
//!                              12 hex digits = a 16, 32 or 48-bit instruction
//!                              (first short word is the most significant)
//! SW <pc> <hex16> [<hex16>..]  raw short words at <pc>, in address order
//! DM <addr> <hex32> [was <hex32>]   one 32-bit word at DM byte address <addr>
//! BYTES <addr> <hex>           raw bytes at DM byte address <addr>
//! APPEND <addr> <hex>          like BYTES; in a .syx it is a new boot-stream block
//! ```
//!
//! Numbers are hex (an optional `0x`). `was` is a guard: the patch is refused
//! when the memory does not hold that value first. A short word lives at byte
//! `0x28000000 + 2*pc` (little endian), or in the `0x20000000` alias for
//! `pc` in `0xb80000..0xc00000` where the low alias is empty.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchOp {
    /// Short words from short-word PC `pc`.
    Pm {
        pc: u32,
        words: Vec<u16>,
        was: Option<Vec<u16>>,
    },
    /// Bytes at a DM byte address (little endian memory order).
    Bytes {
        addr: u32,
        bytes: Vec<u8>,
        was: Option<Vec<u8>>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DspPatch {
    pub base_sha256: Option<String>,
    pub ops: Vec<PatchOp>,
}

fn hex_u32(s: &str) -> Result<u32, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(s, 16).map_err(|_| format!("bad hex number {s:?}"))
}

fn split_hex(s: &str, chunk: usize) -> Result<Vec<u16>, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.is_empty() || s.len() % chunk != 0 || !s.is_ascii() {
        return Err(format!("bad word list {s:?}"));
    }
    (0..s.len())
        .step_by(chunk)
        .map(|i| u16::from_str_radix(&s[i..i + chunk], 16).map_err(|_| format!("bad hex {s:?}")))
        .collect()
}

fn hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.is_empty() || s.len() % 2 != 0 || !s.is_ascii() {
        return Err(format!("bad byte string {s:?}"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| format!("bad hex {s:?}")))
        .collect()
}

fn strip_comment(raw: &str) -> &str {
    let line = raw.split(['#', ';']).next().unwrap_or("");
    line.split("//").next().unwrap_or("").trim()
}

impl DspPatch {
    pub fn parse(text: &str) -> Result<DspPatch, String> {
        let mut patch = DspPatch::default();
        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw);
            if line.is_empty() {
                continue;
            }
            let err = |m: String| format!("line {}: {m}", n + 1);
            let t: Vec<&str> = line.split_whitespace().collect();
            let was = |rest: &[&str]| -> Result<Option<String>, String> {
                match rest {
                    [] => Ok(None),
                    ["was", v] => Ok(Some((*v).to_string())),
                    _ => Err(err("expected 'was <hex>'".into())),
                }
            };
            match t[0].to_ascii_uppercase().as_str() {
                "BASE" if t.len() == 2 => {
                    let h = t[1].to_ascii_lowercase();
                    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(err("BASE needs a 64-digit sha256".into()));
                    }
                    patch.base_sha256 = Some(h);
                }
                "PM" if t.len() >= 3 => {
                    let pc = hex_u32(t[1]).map_err(&err)?;
                    let words = split_hex(t[2], 4).map_err(&err)?;
                    if !matches!(words.len(), 1..=3) {
                        return Err(err("PM takes 4, 8 or 12 hex digits".into()));
                    }
                    let old = match was(&t[3..])? {
                        Some(v) => Some(split_hex(&v, 4).map_err(&err)?),
                        None => None,
                    };
                    if old.as_ref().is_some_and(|o| o.len() != words.len()) {
                        return Err(err("'was' width differs from the new value".into()));
                    }
                    patch.ops.push(PatchOp::Pm {
                        pc,
                        words,
                        was: old,
                    });
                }
                "SW" if t.len() >= 3 => {
                    let pc = hex_u32(t[1]).map_err(&err)?;
                    let mut words = Vec::new();
                    for w in &t[2..] {
                        words.extend(split_hex(w, 4).map_err(&err)?);
                    }
                    patch.ops.push(PatchOp::Pm {
                        pc,
                        words,
                        was: None,
                    });
                }
                "DM" if t.len() >= 3 => {
                    let addr = hex_u32(t[1]).map_err(&err)?;
                    let word = |s: &str| -> Result<Vec<u8>, String> {
                        Ok(hex_u32(s)?.to_le_bytes().to_vec())
                    };
                    let bytes = word(t[2]).map_err(&err)?;
                    let old = match was(&t[3..])? {
                        Some(v) => Some(word(&v).map_err(&err)?),
                        None => None,
                    };
                    patch.ops.push(PatchOp::Bytes {
                        addr,
                        bytes,
                        was: old,
                    });
                }
                // APPEND: a new boot-stream block. Applied to a running engine it is
                // the same write as BYTES; `tools/patchkit/dsp_patch.py syx` makes the block.
                "BYTES" | "APPEND" if t.len() == 3 => {
                    patch.ops.push(PatchOp::Bytes {
                        addr: hex_u32(t[1]).map_err(&err)?,
                        bytes: hex_bytes(t[2]).map_err(&err)?,
                        was: None,
                    });
                }
                _ => return Err(err(format!("cannot parse {line:?}"))),
            }
        }
        Ok(patch)
    }

    pub fn load(path: &Path) -> Result<DspPatch, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("DSP patch {}: {e}", path.display()))?;
        DspPatch::parse(&text).map_err(|e| format!("DSP patch {}: {e}", path.display()))
    }

    /// Fail unless the patch names `syx_sha256_hex` as its base (or names none).
    pub fn check_base(&self, syx_sha256_hex: &str) -> Result<(), String> {
        match &self.base_sha256 {
            Some(b) if b != syx_sha256_hex => Err(format!(
                "DSP patch is for base {b}, but the firmware is {syx_sha256_hex}"
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forms_and_comments() {
        let p = DspPatch::parse(
            "# x\nBASE 00000000000000000000000000000000000000000000000000000000000000ab\n\
             PM 1c9d51 0f82001e was 0f82001f ; scale\nSW 10 0001 0002\nDM 0x25f180 0xdeadbeef\n\
             BYTES 100 0102\n",
        )
        .unwrap();
        assert_eq!(p.ops.len(), 4);
        assert_eq!(
            p.ops[0],
            PatchOp::Pm {
                pc: 0x1c9d51,
                words: vec![0x0f82, 0x001e],
                was: Some(vec![0x0f82, 0x001f])
            }
        );
        assert_eq!(
            p.ops[2],
            PatchOp::Bytes {
                addr: 0x25f180,
                bytes: vec![0xef, 0xbe, 0xad, 0xde],
                was: None
            }
        );
        let a = DspPatch::parse("APPEND 20030000 0a0b0c0d").unwrap();
        assert_eq!(
            a.ops,
            vec![PatchOp::Bytes {
                addr: 0x2003_0000,
                bytes: vec![10, 11, 12, 13],
                was: None
            }]
        );
        assert!(DspPatch::parse("PM 1 123").is_err());
        assert!(DspPatch::parse("PM 1 0f82 was 0f82001f").is_err());
    }
}
