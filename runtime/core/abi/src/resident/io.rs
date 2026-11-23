//! `IoFns`: the I/O task's queue (`io.rs`).

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoFns {
    /// The steps of record `rec` are gone: false while one still runs.
    pub retire: unsafe extern "C" fn(rec: u32) -> bool,
}
