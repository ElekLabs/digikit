//! The `DspPart` of the owner `Fx`: the steps of the DSP loader that name the effect rows, in the functions of `dsp/mod.rs`, and the effect guard's diagnostics.
use crate::fx::Fx;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::provider::{DspPart, Status};
use dkrt_abi::service::{Claim, RowWrite};
use dkrt_sdk::fault::DIAG_WORDS;

impl<C: CoreApi> DspPart<C> for Fx {
    fn check_sections(&mut self, core: &C, sec: &DspSec, job: &mut Job) -> Result<(), Status> {
        // SAFETY: `sec` was validated by the loader (the header and the section bounds); the entries are read inside it
        unsafe { super::check_sections(core, sec, job) }
    }

    fn check_rows(&mut self, core: &C, sec: &DspSec) -> Result<(), Status> {
        // SAFETY: as above
        unsafe { super::check_rows(core, sec) }
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
        unsafe { super::clear_fx_rows(core, module) }
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

    // every site but the voice's names an effect row (the master route, the send route, or a site this runtime does not know), by its byte offset in the table
    fn attribute(&mut self, core: &C, site: u32, id: u32) -> Option<usize> {
        // SAFETY: as above
        unsafe { super::attribute(core, site, id) }
    }

    fn diag(&mut self, core: &C, words: &[u32; DIAG_WORDS]) {
        // SAFETY: as above
        unsafe { super::diag(core, self.guard(), words) }
    }
}
