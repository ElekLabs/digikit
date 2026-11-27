//! The runtime's selection record: which modules to load at the next boot (docs/design/digikit-store.md, "The selection record").
//!
//! Two copies of one sector at the end of the module store range (`end_sector - 2` and `end_sector - 1`). All integers are
//! big-endian:
//!
//! ```text
//!   0   4   magic "DKSL" (0x444b534c)        4   4   version = 1          8   4   seq
//!  12   4   boot_counter                    16   4   flags (bit 0: safe mode once)
//!  20   4   count (0 to 8)                  24   4   stable_blocks; 28 4 sticky type map entry 0 (zero from the builder)
//!  32 448   8 paths of 56 bytes: store paths, NUL padded, at most 55 characters, load order
//! 480  28   sticky type map, entries 1 to 7 508   4   crc32 of bytes 0..508
//! ```
//!
//! In the `plus_drive/<device>/digikit/` folder the selection is `selection.txt`: one store path per line, load order; blank
//! lines and lines starting with `#` are ignored. The builder writes both copies from it (`seq` 1, counter 0, no flags); the
//! exporter reads both copies and writes the one the runtime would use back to the same file.

use crate::{SECTOR, crc32};

pub const SELECTION_MAGIC: u32 = 0x444b_534c;
pub const SELECTION_VERSION: u32 = 1;
pub const SELECTION_MAX: usize = dkneeds::MAX_SELECTION as usize;
pub const SELECTION_PATH_BYTES: usize = 56;
/// The file name in `digikit/`.
pub const SELECTION_FILE: &str = "selection.txt";

/// One decoded copy of the record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub seq: u32,
    pub boot_counter: u32,
    pub flags: u32,
    pub paths: Vec<String>,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// A selected path: non-empty, at most 55 bytes, no NUL, no line break.
pub fn valid_selection_path(path: &str) -> bool {
    !path.is_empty() && path.len() < SELECTION_PATH_BYTES && !path.contains(['\0', '\n', '\r'])
}

/// The 512 bytes of one copy.
pub fn encode_selection(sel: &Selection) -> Result<[u8; SECTOR], String> {
    if sel.paths.len() > SELECTION_MAX {
        return Err(format!(
            "{} modules selected; at most {SELECTION_MAX}",
            sel.paths.len()
        ));
    }
    let mut out = [0u8; SECTOR];
    out[0..4].copy_from_slice(&SELECTION_MAGIC.to_be_bytes());
    out[4..8].copy_from_slice(&SELECTION_VERSION.to_be_bytes());
    out[8..12].copy_from_slice(&sel.seq.to_be_bytes());
    out[12..16].copy_from_slice(&sel.boot_counter.to_be_bytes());
    out[16..20].copy_from_slice(&sel.flags.to_be_bytes());
    out[20..24].copy_from_slice(&(sel.paths.len() as u32).to_be_bytes());
    for (i, path) in sel.paths.iter().enumerate() {
        if !valid_selection_path(path) {
            return Err(format!(
                "selected path {path:?}: at most {} bytes, not empty, no NUL or line break",
                SELECTION_PATH_BYTES - 1
            ));
        }
        out[32 + i * SELECTION_PATH_BYTES..][..path.len()].copy_from_slice(path.as_bytes());
    }
    let crc = crc32(&out[..508]);
    out[508..512].copy_from_slice(&crc.to_be_bytes());
    Ok(out)
}

/// One copy, when it is valid: magic, version, count, path NULs and CRC (the runtime's rules).
pub fn decode_selection(sector: &[u8]) -> Option<Selection> {
    if sector.len() < SECTOR
        || be32(sector, 0) != SELECTION_MAGIC
        || be32(sector, 4) != SELECTION_VERSION
        || be32(sector, 508) != crc32(&sector[..508])
    {
        return None;
    }
    let count = be32(sector, 20) as usize;
    if count > SELECTION_MAX {
        return None;
    }
    let mut paths = Vec::with_capacity(count);
    for i in 0..count {
        let field = &sector[32 + i * SELECTION_PATH_BYTES..][..SELECTION_PATH_BYTES];
        let len = field.iter().position(|&b| b == 0)?;
        if len == 0 || field[len..].iter().any(|&b| b != 0) {
            return None;
        }
        paths.push(String::from_utf8(field[..len].to_vec()).ok()?);
    }
    Some(Selection {
        seq: be32(sector, 8),
        boot_counter: be32(sector, 12),
        flags: be32(sector, 16),
        paths,
    })
}

/// The copy the runtime uses: the valid one with the higher `seq` (equal: A). `None`: no record, an empty selection.
pub fn current_selection(a: Option<Selection>, b: Option<Selection>) -> Option<Selection> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.seq > a.seq { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// `selection.txt`: one path per line.
pub fn parse_selection_txt(text: &str) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !valid_selection_path(line) {
            return Err(format!(
                "line {}: {line:?} is not a store path of at most {} bytes",
                n + 1,
                SELECTION_PATH_BYTES - 1
            ));
        }
        paths.push(line.to_string());
    }
    if paths.len() > SELECTION_MAX {
        return Err(format!("{} modules; at most {SELECTION_MAX}", paths.len()));
    }
    Ok(paths)
}

/// The text the exporter writes.
pub fn format_selection_txt(paths: &[String]) -> String {
    let mut out = String::from(
        "# Modules the digikit runtime loads at boot, one store path per line, in load order (docs/design/digikit-store.md).\n",
    );
    for p in paths {
        out.push_str(p);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(seq: u32, paths: &[&str]) -> Selection {
        Selection {
            seq,
            boot_counter: 1,
            flags: 0,
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_record_round_trips_and_its_checks_hold() {
        let s = sel(7, &["modules/a.dkm", "modules/dir/b.dkm"]);
        let bytes = encode_selection(&s).unwrap();
        assert_eq!(&bytes[0..4], b"DKSL");
        assert_eq!(decode_selection(&bytes), Some(s));
        // a torn copy fails its CRC; a bad count and a path with bytes after the NUL are refused
        let mut torn = bytes;
        torn[40] ^= 1;
        assert_eq!(decode_selection(&torn), None);
        let mut count = bytes;
        count[23] = 9;
        let crc = crc32(&count[..508]);
        count[508..].copy_from_slice(&crc.to_be_bytes());
        assert_eq!(decode_selection(&count), None);
        assert_eq!(decode_selection(&[0u8; SECTOR]), None);
    }

    #[test]
    fn the_higher_seq_wins_and_a_wins_a_tie() {
        let (a, b) = (sel(2, &["a"]), sel(1, &["b"]));
        assert_eq!(
            current_selection(Some(a.clone()), Some(b.clone())),
            Some(a.clone())
        );
        assert_eq!(
            current_selection(Some(b.clone()), Some(a.clone())),
            Some(a.clone())
        );
        let tie = sel(2, &["c"]);
        assert_eq!(
            current_selection(Some(a.clone()), Some(tie)),
            Some(a.clone())
        );
        assert_eq!(current_selection(None, Some(b.clone())), Some(b));
        assert_eq!(current_selection(None, None), None);
    }

    #[test]
    fn the_text_file_has_comments_limits_and_errors() {
        let text = "# c\n\n modules/a.dkm \nmodules/b.dkm\n";
        let paths = parse_selection_txt(text).unwrap();
        assert_eq!(paths, ["modules/a.dkm", "modules/b.dkm"]);
        assert_eq!(
            parse_selection_txt(&format_selection_txt(&paths)).unwrap(),
            paths
        );
        assert!(parse_selection_txt(&"x\n".repeat(9)).is_err());
        assert!(parse_selection_txt(&format!("{}\n", "x".repeat(56))).is_err());
        assert!(parse_selection_txt(&format!("{}\n", "x".repeat(55))).is_ok());
        let nine = Selection {
            seq: 1,
            boot_counter: 0,
            flags: 0,
            paths: vec!["x".into(); 9],
        };
        assert!(encode_selection(&nine).is_err());
    }
}
