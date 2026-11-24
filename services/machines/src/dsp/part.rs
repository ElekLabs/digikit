//! The `DspPart` of the owner `Machines`: the steps of the DSP loader that name the voice rows (`dsp/*`), the voice type's attribution and the machine guard's diagnostics.
use crate::machines::{status, Machines};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{DspPart, Status};
use dkrt_abi::service::{Claim, RowWrite};
use dkrt_sdk::fault::{DIAG_WORDS, SITE_VOICE};

impl<C: CoreApi> DspPart<C> for Machines {
    fn check_sections(&mut self, core: &C, sec: &DspSec, job: &mut Job) -> Result<(), Status> {
        // SAFETY: `sec` was validated by the loader (the header and the section bounds); the entries are read inside it
        status(unsafe { super::check_sections(core, sec, job) })
    }

    fn check_rows(&mut self, core: &C, sec: &DspSec) -> Result<(), Status> {
        // SAFETY: as above
        status(unsafe { super::check_rows(core, sec) })
    }

    fn check_types(&mut self, core: &C, sec: &DspSec) -> Result<(), Status> {
        // SAFETY: as above
        status(unsafe { super::check_types(core, sec) })
    }

    /// A real load always wins a voice row: the placeholders of other numbers are freed, and the parking state is told which machine rows fell back to a gap row.
    fn admit(&mut self, core: &C, job: &Job) -> Result<(), Status> {
        // SAFETY: `job` is the loader's job: its voice entries lie in the section checked before
        status(unsafe {
            // the machine rows the parking state is told about are this crate's registry
            super::make_room(core, job, &mut |t| self.parking_mut().give_way(core, t))
        })
    }

    fn claim(&mut self, core: &C, c: &mut Claim) {
        // SAFETY: `c` is the loader's claim of one module: its job entries lie in the section checked before
        unsafe { super::claim(core, c) }
    }

    fn release(&mut self, core: &C, module: usize) {
        // SAFETY: the rows are the service's own table
        unsafe { super::release(core, module) }
    }

    fn clear(&mut self, core: &C, module: usize) -> bool {
        // SAFETY: as above
        unsafe { super::clear_rows(core, module) }
    }

    fn row_step(
        &mut self,
        core: &C,
        module: usize,
        want: &mut u32,
        write: bool,
    ) -> Option<RowWrite> {
        // SAFETY: as above
        unsafe { super::row_step(core, module, want, write) }
    }

    // a voice site names a machine type, which a voice row of a module holds; every other site is not this service's
    fn attribute(&mut self, core: &C, site: u32, id: u32) -> Option<usize> {
        if site == SITE_VOICE {
            // SAFETY: as above
            unsafe { super::module_of_type(core, id) }
        } else {
            None
        }
    }

    fn diag(&mut self, core: &C, words: &[u32; DIAG_WORDS]) {
        // SAFETY: the guard's memory is this crate's table
        unsafe { crate::guard::diag(core, words) }
    }
}
