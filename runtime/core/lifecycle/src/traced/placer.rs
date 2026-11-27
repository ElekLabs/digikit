use crate::{PackageId, Placer, Step, Trace, Traced};
use dkrt_abi::provider::Status;

impl<S: Placer, T: Trace> Placer for Traced<S, T> {
    fn place(&mut self, package: PackageId) -> Result<u32, Status> {
        self.run(
            Step::Place,
            package,
            |s| s.place(package),
            |r| r.as_ref().err().copied().unwrap_or(0),
        )
    }

    fn free(&mut self, package: PackageId) {
        self.run(Step::Free, package, |s| s.free(package), |_| 0)
    }
}
