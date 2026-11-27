use crate::PackageId;
use dkrt_abi::provider::Status;

/// Gives a package its memory: the code and data of its file. `place` is the first bring-up step, `free` the last take-down step.
pub trait Placer {
    /// Place `package`; the answer is its base address, which the linker binds against. On an error nothing stays placed.
    fn place(&mut self, package: PackageId) -> Result<u32, Status>;

    /// Release what `place` gave. Called once for each package that was placed.
    fn free(&mut self, package: PackageId);
}
