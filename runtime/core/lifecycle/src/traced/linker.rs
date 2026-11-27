use crate::{Linker, PackageId, Step, Trace, Traced};
use dkrt_abi::provider::Status;

impl<S: Linker, T: Trace> Linker for Traced<S, T> {
    fn link(&mut self, package: PackageId, at: u32, services: &[PackageId]) -> Result<(), Status> {
        self.run(
            Step::Link,
            package,
            |s| s.link(package, at, services),
            |r| r.err().unwrap_or(0),
        )
    }

    fn unbind(&mut self, package: PackageId) {
        self.run(Step::Unbind, package, |s| s.unbind(package), |_| 0)
    }
}
