//! The per-track sample table record of a module (the DSP section record of kind 2).
/// Validate the version-one sample-table record, kind 2 (little-endian words: kind, local id, records and control byte offsets,
/// four zero words): 16 records of 512 bytes and 16 control sets of 256 bytes, in the initialized image, zero, not overlapping.
pub fn valid_tracks(record: &[u8], image: &[u8]) -> bool {
    if record.len() != 32 || image.len() > u32::MAX as usize {
        return false;
    }
    let word = |i: usize| u32::from_le_bytes(record[i..i + 4].try_into().unwrap());
    let (records, control) = (word(8), word(12));
    let inside = |at: u32, len: u32| {
        at & 3 == 0
            && (at as usize)
                .checked_add(len as usize)
                .is_some_and(|e| e <= image.len())
            && image[at as usize..(at + len) as usize]
                .iter()
                .all(|x| *x == 0)
    };
    word(0) == 2
        && inside(records, TRACK_RECORDS_BYTES)
        && inside(control, TRACK_CONTROL_BYTES)
        && (records >= control + TRACK_CONTROL_BYTES || control >= records + TRACK_RECORDS_BYTES)
        && word(16) == 0
        && word(20) == 0
        && word(24) == 0
        && word(28) == 0
}
pub const TRACK_RECORDS_BYTES: u32 = 512;
pub const TRACK_CONTROL_BYTES: u32 = 256;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    #[test]
    fn track_table_bounds_zero_and_overlap() {
        let mut r = [0u8; 32];
        let image = [0u8; 1024];
        for (i, v) in [(0, 2u32), (8, 0), (12, 512)] {
            r[i..i + 4].copy_from_slice(&v.to_le_bytes());
        }
        assert!(valid_tracks(&r, &image));
        let set =
            |r: &mut [u8; 32], i: usize, v: u32| r[i..i + 4].copy_from_slice(&v.to_le_bytes());
        for (word, value) in [
            (0, 1u32),
            (8, 4),
            (8, 513),
            (12, 770),
            (12, 1022),
            (12, 2),
            (16, 1),
            (28, 1),
            (8, u32::MAX - 3),
        ] {
            let mut bad = r;
            set(&mut bad, word, value);
            assert!(!valid_tracks(&bad, &image), "word {word} = {value}");
        }
        let mut dirty = image;
        dirty[600] = 1;
        assert!(!valid_tracks(&r, &dirty));
        assert!(!valid_tracks(&r, &image[..767]));
    }
}
