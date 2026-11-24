//! The builder of the owner: the collection of the module in `init` (with its gate) and the callback arguments the last commit wrote.
use super::*;
use core::ptr::addr_of_mut;
use dkrt_svc_rows::builder::Gate;

/// The state of the builder calls (one module is in `init` at a time): the collection (its gate tells whose `init` is open) and the callback arguments of the section the last
/// commit wrote (`apply` reads them). The owner holds it; zero bytes are not its start state (the gate is closed at `u32::MAX`), so `Builder::init` writes it in place.
pub struct Builder {
    pub(super) build: Build,
    pub(super) args: Option<ApplyArgs>,
}

impl Default for Builder {
    fn default() -> Builder {
        Builder::new()
    }
}

impl Builder {
    /// A builder with nothing collected and the gate closed. The collection is 5 KiB: a start in a cell uses [`Builder::init`].
    pub fn new() -> Builder {
        Builder {
            build: Build::new(),
            args: None,
        }
    }

    /// The start state written where it lives, with no copy of the collection through the stack: the rest of the collection is valid as zero bytes (`begin` writes
    /// `Build::new()` before the first call of every module), the gate is closed.
    ///
    /// # Safety
    /// `at` points at memory for a `Builder` that holds nothing that needs a drop (all zero bytes, as a fresh cell).
    pub unsafe fn init(at: *mut Builder) {
        addr_of_mut!((*at).build.gate).write(Gate::new());
        addr_of_mut!((*at).args).write(None);
    }

    /// The placeholder handle of a refused call.
    pub fn placeholder(&self) -> *mut u8 {
        self.build.placeholder()
    }

    /// The collection (host tests read it).
    pub fn build(&mut self) -> &mut Build {
        &mut self.build
    }

    /// `Records::begin`: forget what an earlier module collected and open the collection for module record `rec`.
    pub fn begin(&mut self, rec: usize) {
        // SAFETY: the collection is plain data; the write replaces it whole
        unsafe { core::ptr::write(addr_of_mut!(self.build), Build::new()) };
        self.args = None;
        self.build.gate.begin(rec);
    }

    /// Close the collection: later builder calls return the placeholder.
    pub fn end(&mut self) {
        self.build.gate.end();
    }

    /// The callback arguments of the section the last commit wrote.
    pub fn args(&self) -> Option<&ApplyArgs> {
        self.args.as_ref()
    }
}
