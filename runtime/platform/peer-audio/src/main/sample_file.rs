use dkrt_sdk::sample::{native_header, HEADER_BYTES};
use dkrt_sdk::{DkrtStoreFile, E_FORMAT, E_READ, E_TOO_BIG};

use crate::mailbox::{BANK_FRAMES, CHUNK_FRAMES};

const SECTOR_BYTES: usize = 512;

extern "C" {
    fn dkrt_store_lease(path: *const u8, file: *mut DkrtStoreFile) -> i32;
    fn dkrt_store_unlease(lease: i32);
    fn dkrt_store_read(file: *const DkrtStoreFile, sector: u32, count: u32, dst: *mut u8) -> i32;
}

#[repr(align(16))]
pub struct Sector {
    data: [u8; SECTOR_BYTES],
}

impl Sector {
    pub const fn empty() -> Self {
        Self {
            data: [0; SECTOR_BYTES],
        }
    }
}

struct Lease {
    id: i32,
    file: DkrtStoreFile,
}

impl Drop for Lease {
    fn drop(&mut self) {
        unsafe { dkrt_store_unlease(self.id) };
    }
}

impl Lease {
    unsafe fn open(path: *const u8) -> Result<Self, i32> {
        let mut file = DkrtStoreFile {
            first: 0,
            size: 0,
            sha: [0; 32],
        };
        let id = dkrt_store_lease(path, &mut file);
        if id <= 0 {
            return Err(id);
        }
        Ok(Self { id, file })
    }

    fn read(&self, sector: usize, dst: &mut Sector) -> Result<(), i32> {
        let result =
            unsafe { dkrt_store_read(&self.file, sector as u32, 1, dst.data.as_mut_ptr()) };
        if result != 0 {
            return Err(E_READ);
        }
        Ok(())
    }
}

fn chunk(first: usize, total: usize) -> (usize, core::ops::Range<usize>) {
    let offset = HEADER_BYTES + first * 2;
    let sector = offset / SECTOR_BYTES;
    let start = offset % SECTOR_BYTES;
    let frames = (total - first)
        .min(CHUNK_FRAMES)
        .min((SECTOR_BYTES - start) / 2);
    (sector, start..start + 2 * frames)
}

pub struct SampleFile {
    lease: Lease,
    frames: usize,
    sector: usize,
}

impl SampleFile {
    pub unsafe fn open(path: *const u8, dst: &mut Sector) -> Result<Self, i32> {
        let lease = Lease::open(path)?;
        lease.read(0, dst)?;
        let (frames, channels) = native_header(&dst.data, lease.file.size)?;
        if channels != 1 {
            return Err(E_FORMAT);
        }
        if frames as usize > BANK_FRAMES {
            return Err(E_TOO_BIG);
        }
        Ok(Self {
            lease,
            frames: frames as usize,
            sector: 0,
        })
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn read<'a>(&mut self, first: usize, dst: &'a mut Sector) -> Result<&'a [u8], i32> {
        if first >= self.frames {
            return Err(E_FORMAT);
        }
        let (sector, bytes) = chunk(first, self.frames);
        if sector != self.sector {
            self.lease.read(sector, dst)?;
            self.sector = sector;
        }
        dst.data.get(bytes).ok_or(E_FORMAT)
    }
}

#[cfg(test)]
mod tests;
