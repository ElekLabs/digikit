//! The fixture of the owner's two-phase call (`Owner::step`) on the device. The test hook (as `test/svc-test` takes it, on the SETTINGS list rebuild) enters the owner once per rebuild: `decide` counts the
//! step, and `act`, with the owner idle, calls the service's own export `local_step_value`, which enters the owner again (a firmware function that calls back into the service does
//! the same). So `steps` equals `reentries` and the owner refuses nothing. One nested call, armed through a word of the file, is made inside `decide`: the owner refuses it
//! and counts one, so a count of 0 means something. The counts are in `LOCAL_STEP_STATS` for a test that reads the file's memory.
#![cfg_attr(target_arch = "m68k", no_std)]
#![cfg_attr(target_arch = "m68k", feature(asm_experimental_arch))]

use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{Provider, Status};
use dkrt_svc_rt::Outside;

pub mod hooks;

/// The words a test reads (and the third it writes): steps, re-entries, and the arming word of the nested call (non-zero: the next `decide` makes one nested call and clears it).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub static mut LOCAL_STEP_STATS: [u32; 3] = [0; 3];
/// Index of the arming word in `LOCAL_STEP_STATS`.
pub const ARM: usize = 2;

/// The owner object: what it counted since `start`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalStep {
    steps: u32,
    reentries: u32,
}

impl LocalStep {
    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn reentries(&self) -> u32 {
        self.reentries
    }

    fn publish(&self) {
        // SAFETY: one UI task writes the words; the test reads them with the device paused
        unsafe {
            write_volatile(addr_of_mut!(LOCAL_STEP_STATS[0]), self.steps);
            write_volatile(addr_of_mut!(LOCAL_STEP_STATS[1]), self.reentries);
        }
    }
}

impl<C: CoreApi> Provider<C> for LocalStep {
    fn start_in(_: &C, slot: &mut MaybeUninit<LocalStep>) -> Result<(), Status> {
        slot.write(LocalStep::default());
        Ok(())
    }
}

/// The first phase of a step, in the owner: counts the step and makes the one nested call when armed.
fn decide(owner: &mut LocalStep, _: ()) {
    owner.steps += 1;
    owner.publish();
    // SAFETY: as `publish`
    unsafe {
        if read_volatile(addr_of!(LOCAL_STEP_STATS[ARM])) != 0 {
            write_volatile(addr_of_mut!(LOCAL_STEP_STATS[ARM]), 0);
            // a call into the owner inside its own call: refused and counted
            let _ = DK_SVC_OWNER.with(|_| ());
        }
    }
}

/// One run of the hook: a step whose `act` calls the service's export. `None` without an owner.
pub fn step_once() -> Option<u32> {
    DK_SVC_OWNER.step((), decide, |(), _: &Outside| local_step_value())
}

/// The entry `hooks.s` calls on a run of the hook (C calling convention, the UI task). The stock code always goes on through the tail.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub extern "C" fn dk_local_step_run() {
    let _ = step_once();
}

/// The service's own export, called from the act phase: it enters the owner and counts the re-entry.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub extern "C" fn local_step_value() -> u32 {
    DK_SVC_OWNER
        .with(|owner| {
            owner.reentries += 1;
            owner.publish();
            42
        })
        .unwrap_or(0)
}

dkrt_svc_rt::service!(
    LocalStep,
    "local-step",
    &[],
    parts = [],
    hooks = [("cf.menu.hook.settings_list.test", hooks::local_step_hook)],
    fills = [],
    exports = [("local_step_value", local_step_value)]
);
