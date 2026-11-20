//! Records that the emulated SPI NOR holds outside the OS container.
//!
//! The analog calibration record is factory data of a real unit. The emulator never reads, copies or writes
//! one: `synthetic_calibration` builds a valid record from the format alone, with every table zero. The format
//! is the one MAIN checks at boot (finding 70, "Cold boot": `FUN_4006ecb8` reads it, `FUN_4006eb5c` sums it,
//! `FUN_4006edca` tests the marker bytes). A real unit's record is not needed to pass the check.

/// Where the record sits in the NOR.
pub(crate) const CALIBRATION_OFFSET: usize = 0x30_0000;
/// Record size: 0x10 header bytes and 0x22d30 payload bytes.
pub(crate) const CALIBRATION_BYTES: usize = 0x22d40;
const MAGIC: u32 = 0x5242_4f5a; // "RBOZ"
const PAYLOAD_BYTES: u32 = 0x22d30;
/// Marker bytes that MAIN needs nonzero (record offsets). They are the first byte of six tables and two table
/// groups; the value 1 is the smallest nonzero byte, a choice of the emulator and not a factory value.
const MARKERS: [usize; 8] = [0x17, 0x18, 0x19, 0xda, 0x14f, 0x15c, 0x1885, 0x2fae];

/// The firmware's checksum over the payload: the sum over i = 1..=len of `i ^ byte[0x10 + i - 1]`, 32 bit, `i` not masked.
fn checksum(record: &[u8]) -> u32 {
    (1..=PAYLOAD_BYTES).fold(0u32, |sum, i| {
        sum.wrapping_add(i ^ u32::from(record[0x10 + i as usize - 1]))
    })
}

/// **Synthetic emulator data**: a record that passes MAIN's check, tables zero except the marker bytes.
pub(crate) fn synthetic_calibration() -> Vec<u8> {
    let mut record = vec![0u8; CALIBRATION_BYTES];
    record[0..4].copy_from_slice(&MAGIC.to_be_bytes());
    record[4..8].copy_from_slice(&PAYLOAD_BYTES.to_be_bytes());
    record[8..12].copy_from_slice(&1u32.to_be_bytes());
    record[0x10..0x12].copy_from_slice(&1u16.to_be_bytes());
    for at in MARKERS {
        record[at] = 1;
    }
    let sum = checksum(&record);
    record[0xc..0x10].copy_from_slice(&sum.to_be_bytes());
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_record_has_the_checked_shape() {
        let r = synthetic_calibration();
        assert_eq!(r.len(), CALIBRATION_BYTES);
        assert_eq!(&r[0..4], b"RBOZ");
        assert_eq!(u16::from_be_bytes([r[0x10], r[0x11]]), 1);
        assert!(MARKERS.iter().all(|&at| r[at] != 0));
        assert_eq!(
            u32::from_be_bytes(r[0xc..0x10].try_into().unwrap()),
            checksum(&r)
        );
        // Tables are zero: only the header and the markers are set.
        let set = r.iter().filter(|&&b| b != 0).count();
        assert!(set < 40, "{set} nonzero bytes");
    }
}
