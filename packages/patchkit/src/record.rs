//! The patch record `DKPR`: stored in the patched image so that `inspect` and `verify` can tell what was
//! applied, and so that a patched image is never patched again.
//!
//! ```text
//! "DKPR"  u16be format (1)  u16be count  [32] base .syx sha256  count x ( u8 len, "id@version" )  pad to 4
//!   [ "RPK1"  u16be count  count x ( u8 len, role, u8 len, "id@version", u32be dest, u32be stage_end, u32be len, [32] sha256 )  pad to 4 ]
//! ```
//!
//! The optional `RPK1` block follows the padded module list: one entry per read-only section that a module stores packed (`[[repack]]` of its
//! manifest): the section's role, the claiming module and its buffer layout, and the stock section's length and SHA-256. A reader that stops after
//! the module list (every reader before the block) never sees it; a record without the block decodes with no entries.

use crate::error::*;

pub const MAGIC: &[u8; 4] = b"DKPR";
pub const FORMAT: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Record {
    pub base_sha256: String,
    pub modules: Vec<String>,
    /// The packed read-only sections: what `verify` checks a packed section against without the module manifests.
    pub repacks: Vec<Repack>,
}

/// One `[[repack]]` of a module in the record (see the module doc).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Repack {
    pub role: String,
    /// `id@version` of the claiming module (one of the record's modules).
    pub module: String,
    pub dest: u32,
    pub stage_end: u32,
    /// Length and SHA-256 of the stock (decoded) section.
    pub len: u32,
    pub sha256: String,
}

const REPACK_MAGIC: &[u8; 4] = b"RPK1";

impl Record {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&FORMAT.to_be_bytes());
        v.extend_from_slice(&(self.modules.len() as u16).to_be_bytes());
        v.extend_from_slice(&unhex(&self.base_sha256).expect("sha256 hex"));
        for m in &self.modules {
            v.push(m.len() as u8);
            v.extend_from_slice(m.as_bytes());
        }
        v.resize(v.len().div_ceil(4) * 4, 0);
        if !self.repacks.is_empty() {
            v.extend_from_slice(REPACK_MAGIC);
            v.extend_from_slice(&(self.repacks.len() as u16).to_be_bytes());
            for r in &self.repacks {
                for s in [&r.role, &r.module] {
                    v.push(s.len() as u8);
                    v.extend_from_slice(s.as_bytes());
                }
                for n in [r.dest, r.stage_end, r.len] {
                    v.extend_from_slice(&n.to_be_bytes());
                }
                v.extend_from_slice(&unhex(&r.sha256).expect("sha256 hex"));
            }
            v.resize(v.len().div_ceil(4) * 4, 0);
        }
        v
    }

    /// Decode a record at the start of `b`.
    pub fn decode(b: &[u8]) -> Result<Record> {
        if b.len() < 40 || &b[..4] != MAGIC {
            return Err(err!("not a DKPR record"));
        }
        let fmt = u16::from_be_bytes([b[4], b[5]]);
        if fmt != FORMAT {
            return Err(err!("DKPR format {fmt} is not supported"));
        }
        let n = u16::from_be_bytes([b[6], b[7]]) as usize;
        let base_sha256 = hex(&b[8..40]);
        let mut at = 40;
        let mut modules = vec![];
        for _ in 0..n {
            let l = *b.get(at).ok_or_else(|| err!("DKPR record is truncated"))? as usize;
            let s = b
                .get(at + 1..at + 1 + l)
                .ok_or_else(|| err!("DKPR record is truncated"))?;
            modules.push(
                String::from_utf8(s.to_vec()).map_err(|_| err!("DKPR module name is not UTF-8"))?,
            );
            at += 1 + l;
        }
        let mut repacks = vec![];
        at = at.div_ceil(4) * 4;
        if b.get(at..at + 4) == Some(&REPACK_MAGIC[..]) {
            let cut = || err!("DKPR repack block is truncated");
            let n = u16::from_be_bytes(b.get(at + 4..at + 6).ok_or_else(cut)?.try_into().unwrap());
            at += 6;
            for _ in 0..n {
                let mut strs = vec![];
                for _ in 0..2 {
                    let l = *b.get(at).ok_or_else(cut)? as usize;
                    let s = b.get(at + 1..at + 1 + l).ok_or_else(cut)?;
                    strs.push(
                        String::from_utf8(s.to_vec())
                            .map_err(|_| err!("DKPR repack name is not UTF-8"))?,
                    );
                    at += 1 + l;
                }
                let word = |o: usize| -> Result<u32> {
                    Ok(u32::from_be_bytes(
                        b.get(o..o + 4).ok_or_else(cut)?.try_into().unwrap(),
                    ))
                };
                let (dest, stage_end, len) = (word(at)?, word(at + 4)?, word(at + 8)?);
                let sha = b.get(at + 12..at + 44).ok_or_else(cut)?;
                at += 44;
                let module = strs.pop().unwrap();
                repacks.push(Repack {
                    role: strs.pop().unwrap(),
                    module,
                    dest,
                    stage_end,
                    len,
                    sha256: hex(sha),
                });
            }
        }
        Ok(Record {
            base_sha256,
            modules,
            repacks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let r = Record {
            base_sha256: "ab".repeat(32),
            modules: vec!["core/fx@0.1.0".into(), "x/y@1".into()],
            repacks: vec![],
        };
        let e = r.encode();
        assert_eq!(e.len() % 4, 0);
        assert_eq!(Record::decode(&e).unwrap(), r);
    }

    #[test]
    fn repack_entries_round_trip_and_an_old_reader_stops_before_them() {
        let r = Record {
            base_sha256: "ab".repeat(32),
            modules: vec!["core/peer-image@0.1.0".into()],
            repacks: vec![Repack {
                role: "cf2".into(),
                module: "core/peer-image@0.1.0".into(),
                dest: 0x4f84f2c6,
                stage_end: 0x4f8af2c1,
                len: 383760,
                sha256: "cd".repeat(32),
            }],
        };
        let e = r.encode();
        assert_eq!(e.len() % 4, 0);
        assert_eq!(Record::decode(&e).unwrap(), r);
        // the same record without the block is what every earlier encoder wrote: it decodes with no entries, and the block is
        // after the padded module list, where the earlier decoder stops
        let old = Record {
            repacks: vec![],
            ..r.clone()
        };
        let o = old.encode();
        assert!(e.starts_with(&o));
        assert_eq!(Record::decode(&o).unwrap().repacks, []);
        assert!(
            Record::decode(&e[..e.len() - 8]).is_err(),
            "a cut block is refused"
        );
    }
}
