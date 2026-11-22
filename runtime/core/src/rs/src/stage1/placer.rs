/// Where stage 1 puts the package body. The real placer copies to the arena top (`core.package`, after RC2s-1 S7); a host test records the bytes.
pub trait Placer {
    /// A body of `size` bytes is coming. An error (an `E_*` code) means it does not fit: stage 1 loads nothing.
    fn begin(&mut self, size: u32) -> Result<(), i32>;
    /// The next bytes of the body, at `offset` from its start.
    fn write(&mut self, offset: u32, bytes: &[u8]);
    /// The body did not pass the checks: forget it.
    fn abandon(&mut self);
}
