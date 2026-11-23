use super::Placer;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_sdk::syspkg::body::{relocate, BodyHeader, HEADER_BYTES};
use dkrt_sdk::{E_FORMAT, E_TOO_BIG};

/// The placer of the device: the package body goes to the package block (`state::package_block`, `PACKAGE_BYTES` of its own, outside the module arena) and stays there for
/// the whole boot. The body is placed as it is read (body header, image, relocation table); `finish` relocates the image where it lies, clears the bss after it (over the
/// relocation table, which has done its work) and flushes both caches over the block. The image starts at `HEADER_BYTES` (32) from the block, which is 16-byte aligned, so
/// the image is aligned as its header asks. A body whose `placed_bytes` are over `PACKAGE_BYTES` is refused (`E_TOO_BIG`) and nothing is written.
pub struct PackagePlacer {
    block: DevAddr,
    header: Option<BodyHeader>,
}

impl PackagePlacer {
    pub const fn new() -> PackagePlacer {
        PackagePlacer { block: DevAddr::NULL, header: None }
    }

    /// The block the package was placed in, `[lo, hi)`: from the block start to the end of what `begin` asked for, rounded to 16 bytes. `None` before `begin`
    /// succeeded and after `abandon`.
    pub fn extent(&self) -> Option<(u32, u32)> {
        let h = self.header?;
        let bytes = ((h.placed_bytes() as u32) + 15) & !15;
        Some((self.block.get(), self.block.get() + bytes))
    }

    /// The device address of the image start (the base the package is relocated to); null before `begin` succeeded.
    fn base(&self) -> DevAddr {
        if self.block.is_null() {
            DevAddr::NULL
        } else {
            self.block.add(HEADER_BYTES as u32)
        }
    }
}

impl Placer for PackagePlacer {
    fn begin(&mut self, size: u32, head: &[u8]) -> Result<(), i32> {
        let h = BodyHeader::peek(head, size).map_err(|_| E_FORMAT)?;
        let need = u32::try_from(h.placed_bytes()).map_err(|_| E_TOO_BIG)?;
        let (block, bytes) = crate::state::package_block();
        if block.is_null() || need > bytes {
            return Err(E_TOO_BIG);
        }
        self.block = block;
        self.header = Some(h);
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) {
        // SAFETY: `begin` reserved `placed_bytes`, which is at least the body's size, and stage 1 writes inside the body.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.block.add(offset).ptr::<u8>(), bytes.len()) }
    }

    fn abandon(&mut self) {
        self.block = DevAddr::NULL;
        self.header = None;
    }

    fn finish(&mut self) -> Result<DevAddr, i32> {
        let h = self.header.ok_or(E_FORMAT)?;
        let base = self.base();
        let image_len = h.image as usize;
        // SAFETY: the block holds `placed_bytes` bytes, written by `write`; the image and the relocation table do not overlap, and the bss lies after the image in the block.
        unsafe {
            let image = core::slice::from_raw_parts_mut(base.ptr::<u8>(), image_len);
            let table = core::slice::from_raw_parts(base.add(h.image).ptr::<u8>() as *const u8, 4 * h.relocs as usize);
            relocate(image, table, base.get()).map_err(|_| E_FORMAT)?;
            core::ptr::write_bytes(base.add(h.image).ptr::<u8>(), 0, h.bss as usize);
            crate::platform::flush(self.block.get(), h.placed_bytes() as u32);
        }
        Ok(base.add(h.entry))
    }
}
