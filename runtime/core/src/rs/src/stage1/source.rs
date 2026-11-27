use crate::storage::store;
use dkrt_sdk::*;

/// Where stage 1 reads the package file from, in sector-aligned pieces.
pub trait PackageSource {
    /// Look the file up: its size, or `E_NOT_FOUND` when the card has none.
    unsafe fn open(&mut self) -> Result<u32, i32>;
    /// Fill `dst` (a multiple of 512 bytes) from `offset` (a multiple of 512) of the file, never past its end. Returns the file bytes now at the start
    /// of `dst`: `min(dst.len(), size - offset)`; the rest of `dst` is not defined.
    unsafe fn read(&mut self, offset: u32, dst: &mut [u8]) -> Result<usize, i32>;
}

/// The digikit store of the card, through the existing lookup and reads.
pub struct StoreSource {
    path: &'static [u8],
    file: Option<store::File>,
}

impl StoreSource {
    /// `path` is a NUL-terminated store key.
    pub fn new(path: &'static [u8]) -> Self {
        StoreSource { path, file: None }
    }
}

impl PackageSource for StoreSource {
    unsafe fn open(&mut self) -> Result<u32, i32> {
        let f = store::find(self.path.as_ptr())?;
        let size = f.size;
        self.file = Some(f);
        Ok(size)
    }

    unsafe fn read(&mut self, offset: u32, dst: &mut [u8]) -> Result<usize, i32> {
        let Some(f) = self.file.as_ref() else {
            return Err(E_NOT_FOUND);
        };
        store::read_file_sectors(f, offset / 512, (dst.len() / 512) as u32, dst.as_mut_ptr(), dst.len()).map(|n| n as usize)
    }
}
