use super::*;
use crate::sha256::Sha;
use dkrt_sdk::syspkg::{Header, HeaderError, HEADER_BYTES};
use dkrt_sdk::{E_BUSY, E_NOT_FOUND};

/// How often a store that answers `E_BUSY` is asked again before the card counts as unreadable. Each ask already waits in the store lock
/// (`storage::lock::acquire`, `WAIT_STEPS` of 1 ms), so this bounds the wait to a few such periods; nothing else holds the store this early in the boot.
pub const BUSY_RETRIES: u32 = 3;

/// Run `step`; an `E_BUSY` answer is asked again up to `BUSY_RETRIES` times.
fn retrying<T>(mut step: impl FnMut() -> Result<T, i32>) -> Result<T, i32> {
    let mut left = BUSY_RETRIES;
    loop {
        match step() {
            Err(E_BUSY) if left > 0 => left -= 1,
            r => return r,
        }
    }
}

impl<S: PackageSource, P: Placer> Stage1<'_, S, P> {
    /// Find the package, check it and place its body. Checks in this order, each before the work after it: the file exists, the header reads (a header
    /// of another magic or format is incompatible, a file too short for one is damaged), the resident table is compatible, the file has the size the
    /// header says, then the body is streamed through the chunk buffer into the placer and its hash is compared. A body that does not match is abandoned.
    /// Only a file the card does not have is `Missing`. A card read that fails (after `BUSY_RETRIES` for a busy store) is `Unreadable`: the boot goes on
    /// without the package this time, and the next boot tries again.
    pub unsafe fn load(&mut self) -> PackageStatus {
        let file_size = match retrying(|| self.source.open()) {
            Ok(n) => n,
            Err(E_NOT_FOUND) => return PackageStatus::Missing,
            Err(_) => return PackageStatus::Unreadable,
        };
        let first = (file_size as usize).min(self.chunk.len());
        match self.read_chunk(0, first) {
            Ok(true) => {}
            Ok(false) => return PackageStatus::Damaged,
            Err(_) => return PackageStatus::Unreadable,
        }
        let header = match Header::decode(&self.chunk[..first]) {
            Ok(h) => h,
            Err(HeaderError::Short) => return PackageStatus::Damaged,
            Err(HeaderError::BadMagic | HeaderError::BadFormat) => return PackageStatus::Incompatible,
        };
        if !header.compatible(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR) {
            return PackageStatus::Incompatible;
        }
        if file_size as u64 != HEADER_BYTES as u64 + header.size() as u64 {
            return PackageStatus::Damaged;
        }
        if self.placer.begin(header.size()).is_err() {
            return PackageStatus::Damaged;
        }
        match self.stream(&header, file_size) {
            Ok(true) => PackageStatus::Ok,
            Ok(false) => {
                self.placer.abandon();
                PackageStatus::Damaged
            }
            Err(_) => {
                self.placer.abandon();
                PackageStatus::Unreadable
            }
        }
    }

    /// Read the chunk at `offset` of the file; `Ok(false)` when the source gave fewer than the `want` file bytes the chunk should hold.
    unsafe fn read_chunk(&mut self, offset: u32, want: usize) -> Result<bool, i32> {
        let n = retrying(|| self.source.read(offset, self.chunk))?;
        Ok(n >= want)
    }

    /// Hand the body to the placer chunk by chunk (the first chunk is read already) and compare its hash with the header's. `Ok(false)`: the body does
    /// not match; `Err`: a read failed.
    unsafe fn stream(&mut self, header: &Header, file_size: u32) -> Result<bool, i32> {
        let mut sha = Sha::new();
        let (mut at, mut chunk_at) = (HEADER_BYTES as u32, 0u32);
        loop {
            let end = (chunk_at + self.chunk.len() as u32).min(file_size);
            let bytes = &self.chunk[(at - chunk_at) as usize..(end - chunk_at) as usize];
            sha.update(bytes.as_ptr(), bytes.len() as u32);
            self.placer.write(at - HEADER_BYTES as u32, bytes);
            at = end;
            if at >= file_size {
                break;
            }
            chunk_at += self.chunk.len() as u32;
            let want = (file_size - chunk_at).min(self.chunk.len() as u32) as usize;
            if !self.read_chunk(chunk_at, want)? {
                return Ok(false);
            }
        }
        let mut digest = [0u8; 32];
        sha.finish(digest.as_mut_ptr());
        Ok(header.body_matches(at - HEADER_BYTES as u32, &digest))
    }
}
