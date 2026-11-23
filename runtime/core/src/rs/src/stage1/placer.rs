use dkrt_abi::dev_addr::DevAddr;

/// Where stage 1 puts the package body. The real placer (`PackagePlacer`) copies it to the package block (`PACKAGE_BYTES`, outside the module arena) and makes it runnable; a host test records the bytes.
pub trait Placer {
    /// A body of `size` bytes is coming; `head` is its first bytes (as many as stage 1 has read, at least the file's first chunk less its header). An error (an `E_*`
    /// code) means it does not fit or is no body: stage 1 loads nothing.
    fn begin(&mut self, size: u32, head: &[u8]) -> Result<(), i32>;
    /// The next bytes of the body, at `offset` from its start.
    fn write(&mut self, offset: u32, bytes: &[u8]);
    /// The body did not pass the checks: forget it.
    fn abandon(&mut self);
    /// The whole body is written and its hash matched: make it run (relocate, clear what it needs cleared, flush the caches) and give the address of its entry. An error means
    /// the body does not hold together (a relocation outside the image): stage 1 abandons it.
    fn finish(&mut self) -> Result<DevAddr, i32>;
}

/// A placer that lives longer than one `Stage1` (the boot keeps the package's block to give it back).
impl<P: Placer> Placer for &mut P {
    fn begin(&mut self, size: u32, head: &[u8]) -> Result<(), i32> {
        (**self).begin(size, head)
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        (**self).write(offset, bytes)
    }
    fn abandon(&mut self) {
        (**self).abandon()
    }
    fn finish(&mut self) -> Result<DevAddr, i32> {
        (**self).finish()
    }
}
