//! The tick of the file.
use super::env::DkEnv;
use super::service_state::{PASSES, PROJECT};
use crate::hooks::DKRT_PROJECT_PENDING;
use core::ptr::{addr_of, addr_of_mut, read_volatile};

/// The tick of the file (the UI task, once per pass): what the hooks recorded, the work in progress, the next piece.
pub unsafe extern "C" fn tick(_ctx: *mut u8) {
    *addr_of_mut!(PASSES) = read_volatile(addr_of!(PASSES)).wrapping_add(1);
    (*addr_of_mut!(PROJECT)).tick(&mut DkEnv, &*addr_of!(DKRT_PROJECT_PENDING));
}
