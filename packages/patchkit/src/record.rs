//! The patch record `DKPR`: stored in the patched image so that `inspect` and `verify` can tell what was
//! applied, and so that a patched image is never patched again.
//!
//! ```text
//! "DKPR"  u16be format (1)  u16be count  [32] base .syx sha256  count x ( u8 len, "id@version" )  pad to 4
//! ```

use crate::error::*;

pub const MAGIC: &[u8; 4] = b"DKPR";
pub const FORMAT: u16 = 1;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Record {
    pub base_sha256: String,
    pub modules: Vec<String>,
}

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
        Ok(Record {
            base_sha256,
            modules,
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
        };
        let e = r.encode();
        assert_eq!(e.len() % 4, 0);
        assert_eq!(Record::decode(&e).unwrap(), r);
    }
}
