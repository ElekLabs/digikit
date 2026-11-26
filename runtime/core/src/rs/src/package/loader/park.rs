//! The tasks the installer waits for (`install::Gate`): the facts the port file lists for the BgWorker task and the console task, read into `install::Park` records.
//!
//! A `task` hook (project load, save, clear and paste; the fx array hooks of the project load) runs in a task of a priority below the UI task's, which the UI task can preempt
//! between two instructions. Writing or restoring the site under the interrupt mask does not stop that: it needs the task to wait at its top-level wait object, where it is in no
//! hook. The port file lists, per device, the TCB of each such task and the place of the waiter word of its wait object (`ports/fw/rtos.toml`, `port_file = true`). The layout
//! behind the place is firmware, not port data, and the same on both devices: the queue's semaphore waiter is at `queue + 0xc` (the kernel's queue), the BgWorker's job semaphore
//! is at `object + 0x14` and its waiter at `object + 0x18`, the object being what the singleton cell holds.
use super::install::{Gate, Park, PARKS_MAX};
use dkneeds::keys::{name_hash_bytes, Keys, FLAG_ABSENT, NAME_FW};

/// One task: the names of its TCB and of its wait place, and how to get from the place to the waiter word.
struct Task {
    tcb: &'static [u8],
    wait: &'static [u8],
    indirect: bool,
    off: u32,
}

/// The tasks that reach a `task` hook (finding u0-entry-contexts, section 3).
const TASKS: [Task; 2] = [
    // the BgWorker: the cell holds the object, the job semaphore is at object + 0x14, its waiter word 4 after
    Task { tcb: b"cf.rtos.value.bgworker.tcb", wait: b"cf.rtos.value.bgworker.cell", indirect: true, off: 0x18 },
    // the console task: the queue it pops, whose semaphore (queue + 8) has its waiter at queue + 0xc
    Task { tcb: b"cf.rtos.value.console.tcb", wait: b"cf.rtos.value.console.queue", indirect: false, off: 0xc },
];

/// The names of the port file that the facts read (for the fixtures of the tests).
#[cfg(test)]
pub(super) const TASK_NAMES: [&[u8]; 4] = [TASKS[0].tcb, TASKS[0].wait, TASKS[1].tcb, TASKS[1].wait];

/// The facts of the loaded port file, ready for a [`Gate`].
pub(super) struct TaskParks {
    items: [Park; PARKS_MAX],
    n: usize,
    /// False when the port file is not loaded or does not list a task's names: a `task` site then cannot be judged.
    known: bool,
}

impl TaskParks {
    /// Read the facts from the port file `keys` (`None`: no port file). A name the firmware lacks (`FLAG_ABSENT`: a device without that task) leaves the task out; a name the
    /// port file does not list makes the facts unknown.
    pub(super) fn read(keys: Option<&Keys>) -> TaskParks {
        let mut out = TaskParks { items: [Park { tcb: 0, wait: 0, indirect: false, off: 0 }; PARKS_MAX], n: 0, known: false };
        let Some(keys) = keys else {
            return out;
        };
        let mut i = 0;
        while i < TASKS.len() {
            let t = &TASKS[i];
            i += 1;
            let (Some(tcb), Some(wait)) = (keys.find_name(NAME_FW, name_hash_bytes(t.tcb), 0), keys.find_name(NAME_FW, name_hash_bytes(t.wait), 0)) else {
                return out;
            };
            if tcb.flags & FLAG_ABSENT != 0 || wait.flags & FLAG_ABSENT != 0 {
                continue;
            }
            out.items[out.n] = Park { tcb: tcb.addr, wait: wait.addr, indirect: t.indirect, off: t.off };
            out.n += 1;
        }
        out.known = true;
        out
    }

    /// The gate for an install or restore: `live` is false at the boot, when no task has run yet.
    pub(super) fn gate(&self, live: bool) -> Gate<'_> {
        Gate { live, parks: if self.known { Some(&self.items[..self.n]) } else { None } }
    }
}
