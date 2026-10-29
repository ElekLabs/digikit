//! Internal allocation-free metadata enumeration for task/background context.
//! Opening verifies the whole table once (CRC and tail scan), with the existing
//! redundant-table fallback. Pages read only one bounded table chunk. Output is
//! staging: discard every returned entry after any error. Fingerprints detect
//! observed commits, not an immutable snapshot, lease, or ABA/collision protection.
use super::*;

/// Owned metadata only: neither file contents nor its SHA has been validated.
/// The full path includes a NUL terminator; bytes after it are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub path: [u8; PATH_BYTES],
    pub entry: Entry,
}

pub struct CatalogCursor {
    io: Io,
    lay: Layout,
    header: Hdr,
    generation: [u32; 4],
    next: u32,
    error: i32,
}

fn buffer_ok(ptr: *mut u8, cap: usize) -> bool {
    !ptr.is_null() && ptr as usize & 15 == 0 && cap >= 512 && cap & 511 == 0 && cap <= isize::MAX as usize && (ptr as usize).checked_add(cap).is_some()
}

impl CatalogCursor {
    /// Verify the store once between matching fingerprints; no buffers retained.
    /// This full-table operation belongs in task/background context, never audio.
    /// Initial lookup preserves `store::open` redundant-copy fallback semantics.
    ///
    /// # Safety
    /// `fp` holds 512 writable bytes and `scratch` holds `cap` writable bytes,
    /// private, disjoint and 16-byte aligned. I/O and layout remain valid for the
    /// cursor lifetime. Subsequent calls must not overlap their buffers/output.
    pub unsafe fn open(io: &Io, lay: &Layout, fp: *mut u8, scratch: *mut u8, cap: usize) -> Result<Self, i32> {
        if lay.sectors <= TAIL || lay.first.checked_add(lay.sectors).is_none() || !buffer_ok(fp, 512) || !buffer_ok(scratch, cap) {
            return Err(E_BAD_STORE);
        }
        let generation = fingerprint(io, lay, fp).ok_or(E_READ)?;
        let table = super::open(io, lay, fp, scratch, core::cmp::min(cap, MAX_IO_SECTORS as usize * 512), true)?;
        if fingerprint(io, lay, fp).ok_or(E_READ)? != generation {
            return Err(crate::E_BUSY);
        }
        Ok(Self { io: *io, lay: *lay, header: table.h, generation, next: 0, error: 0 })
    }

    fn fail<T>(&mut self, error: i32) -> Result<T, i32> {
        self.error = error;
        Err(error)
    }

    unsafe fn check(&mut self, fp: *mut u8) -> Result<(), i32> {
        let observed = fingerprint(&self.io, &self.lay, fp);
        if observed.is_none() {
            return self.fail(E_READ);
        }
        if observed != Some(self.generation) {
            return self.fail(crate::E_BUSY);
        }
        Ok(())
    }

    /// Advance the raw table index without filtering. At most one table read,
    /// bounded by scratch sectors, MAX_IO_SECTORS, and output capacity (an odd
    /// index/capacity can require one unused half-sector). Zero is only table EOF;
    /// even repeated EOF checks generation. Errors, including bad bounds, stick.
    /// Discard ALL staging output on error, including entries from earlier pages.
    /// No PCM/hash validation or READY publication occurs here.
    ///
    /// # Safety
    /// `fp` and `scratch` meet `open`'s writable/disjoint buffer obligations and
    /// are disjoint from the caller-owned `out` slice. Nothing retains a buffer.
    pub unsafe fn read_page(&mut self, fp: *mut u8, scratch: *mut u8, cap: usize, out: &mut [CatalogEntry]) -> Result<usize, i32> {
        if self.error != 0 {
            return Err(self.error);
        }
        if !buffer_ok(fp, 512) || !buffer_ok(scratch, cap) || out.is_empty() {
            return self.fail(E_BAD_STORE);
        }
        self.check(fp)?;
        let left = self.header.count - self.next;
        if left == 0 {
            self.check(fp)?;
            return Ok(0);
        }
        let skip = (self.next & 1) as usize;
        let sectors = core::cmp::min(cap / 512, MAX_IO_SECTORS as usize);
        let n = core::cmp::min(left as usize, core::cmp::min(out.len(), sectors * 2 - skip));
        let count = (skip + n + 1) / 2;
        let r = read_sectors(&self.io, &self.lay, self.header.table_first + self.next / 2, count as u32, scratch, cap);
        if r != OK {
            return self.fail(r);
        }
        let mut i = 0;
        while i < n {
            let raw = scratch.add((skip + i) * ENTRY);
            let mut path = [0; PATH_BYTES];
            let mut k = 0;
            while k < PATH_BYTES && *raw.add(k) != 0 {
                path[k] = *raw.add(k);
                k += 1;
            }
            let entry = entry_of(raw);
            if k == PATH_BYTES || (entry.kind != KIND_FILE && entry.kind != KIND_DIR) {
                return self.fail(E_BAD_STORE);
            }
            out[i] = CatalogEntry { path, entry };
            i += 1;
        }
        self.check(fp)?;
        self.next += n as u32;
        Ok(n)
    }
}
