//! `samples` service, handlers (interface B): the track-table part of the DSP section and the listing of a loaded module's tracks.
//! It owns no record kind of the `register` section.
use super::tracks;
use dkrt_abi::dsp::{DspSec, Job};
use dkrt_abi::service::{tick_phase, Claim, Service};

/// The samples service.
pub struct Samples;

impl Service for Samples {
    unsafe fn on_loaded(rec: usize) {
        tracks::ready(rec);
    }

    unsafe fn tick(phase: u32) {
        if phase == tick_phase::EARLY {
            tracks::tick();
        }
    }

    unsafe fn dsp_check_sections(sec: &DspSec) -> i32 {
        super::dsp::check_sections(sec)
    }

    unsafe fn dsp_admit(job: &Job) -> i32 {
        super::dsp::admit(job)
    }

    unsafe fn dsp_claim(c: &mut Claim) {
        super::dsp::claim(c)
    }

    unsafe fn dsp_release(rec: usize) {
        tracks::remove(rec);
    }
}
