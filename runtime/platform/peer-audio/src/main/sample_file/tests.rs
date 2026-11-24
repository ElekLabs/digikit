use super::*;
use core::sync::atomic::{AtomicU32, Ordering};

static RELEASES: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
unsafe extern "C" fn dkrt_store_lease(path: *const u8, file: *mut DkrtStoreFile) -> i32 {
    file.write(DkrtStoreFile {
        first: 0,
        size: if *path == b'v' { 68 } else { 64 },
        sha: [0; 32],
    });
    1
}

#[no_mangle]
extern "C" fn dkrt_store_unlease(lease: i32) {
    assert_eq!(lease, 1);
    RELEASES.fetch_add(1, Ordering::Relaxed);
}

#[no_mangle]
unsafe extern "C" fn dkrt_store_read(
    _file: *const DkrtStoreFile,
    sector: u32,
    count: u32,
    dst: *mut u8,
) -> i32 {
    assert_eq!((sector, count), (0, 1));
    let buffer = core::slice::from_raw_parts_mut(dst, SECTOR_BYTES);
    buffer.fill(0);
    buffer[7] = 4;
    buffer[8..12].copy_from_slice(&48_000u32.to_be_bytes());
    buffer[64..68].copy_from_slice(&[1, 2, 3, 4]);
    0
}

#[test]
fn opening_and_reading_a_sample_holds_its_extent_until_drop() {
    let mut sector = Sector::empty();
    assert!(matches!(
        unsafe { SampleFile::open(c"invalid".as_ptr().cast(), &mut sector) },
        Err(E_FORMAT)
    ));
    assert_eq!(RELEASES.load(Ordering::Relaxed), 1);
    let mut file = unsafe { SampleFile::open(c"valid".as_ptr().cast(), &mut sector) }.unwrap();
    assert_eq!(file.read(0, &mut sector), Ok(&[1, 2, 3, 4][..]));
    assert_eq!(file.read(2, &mut sector), Err(E_FORMAT));
    assert_eq!(RELEASES.load(Ordering::Relaxed), 1);
    drop(file);
    assert_eq!(RELEASES.load(Ordering::Relaxed), 2);
}

#[test]
fn chunks_cover_payload_exactly_at_partial_and_full_sector_boundaries() {
    for frames in [1, 224, 225, 255, 256, 257, 1920, BANK_FRAMES] {
        let mut cursor = 0;
        while cursor < frames {
            let (sector, bytes) = chunk(cursor, frames);
            assert_eq!(
                sector * SECTOR_BYTES + bytes.start,
                HEADER_BYTES + cursor * 2
            );
            assert!(bytes.end <= SECTOR_BYTES);
            assert!(!bytes.is_empty());
            assert!(bytes.len() <= CHUNK_FRAMES * 2);
            assert_eq!(bytes.len() % 2, 0);
            cursor += bytes.len() / 2;
        }
        assert_eq!(cursor, frames);
    }
}
