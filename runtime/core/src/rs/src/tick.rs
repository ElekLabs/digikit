//! The UI-loop tick `dkrt_tick`: services, the mailbox, the modules' ticks and the poll functions.

use crate::api::{dkrt_load, dkrt_restart};
use crate::health::{dkrt_health, DKRT_HEALTH};
use crate::hooks;
use crate::mailbox::mailbox_select;
use crate::platform::st;
use crate::state::{init_once, DKRT_MAILBOX, DKRT_POLLS, POLLS};
use crate::{
    admin_cmds, card, dlog, fault, intro, io, safe_mode, settings, store_writer, watch, worker,
};
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::hooks::tick_phase;
use dkrt_sdk::*;

/// One step of the pass after the pre-phase work of `dkrt_tick`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// A hook phase of the services (or the modules' tick), measured as a span when the flag is set (the module files' jobs and the modules' tick are not).
    Hook(u32, bool),
    Settings,
    Watch,
    Mailbox,
}

/// The steps of one UI pass after the pre-phase work, in the order they run (the order the pass always had: the modules' tick runs after the mailbox). `Click::run` of the
/// machines service (a module's click callback, run outside the owner) relies on `LAST` having run before the modules' tick, and a module tick relies on every service phase
/// and the mailbox being over.
pub(crate) const TICK_STEPS: [Step; 8] = [
    Step::Hook(tick_phase::EARLY, true),
    Step::Hook(tick_phase::FILES, false),
    Step::Hook(tick_phase::LATE, true),
    Step::Hook(tick_phase::LAST, true),
    Step::Settings,
    Step::Watch,
    Step::Mailbox,
    Step::Hook(tick_phase::MODULES, false),
];

/// One tick phase of the services, measured as a span (owner `SPAN_PHASE | p`) while a span sink and clock are set.
pub(crate) unsafe fn phase(p: u32) {
    dlog::spanned(dlog::SPAN_PHASE | p, || hooks::tick(p));
}

/// Every registered poll function, each measured as a span (owner `SPAN_POLL | i`, `i` its slot) while a span sink and clock are set.
pub(crate) unsafe fn poll_all() {
    let mut i = 0;
    while i < POLLS {
        let f = read_volatile(addr_of!(DKRT_POLLS[i]));
        if f != 0 {
            let call: unsafe extern "C" fn() = core::mem::transmute(f);
            dlog::spanned(dlog::SPAN_POLL | i as u32, || call());
        }
        i += 1;
    }
}

/// Called from the UI task's event loop (`dkrt_loop_hook`, src/cf/tramp.s) once per pass, before it waits for the next
/// event. Registers d0, d1, a0, a1 are saved by the hook. Polls the mailbox, then calls the tick of every module.
#[no_mangle]
pub unsafe extern "C" fn dkrt_tick() {
    crate::resident_query::note_ui_task();
    init_once();
    let s = st();
    s.ticks = s.ticks.wrapping_add(1);
    intro::ui_pass();
    safe_mode::ui_pass();
    admin_cmds::restart_poll();
    hooks::tick(tick_phase::POLL_JOB);
    crate::storage::backends::plusdrive::tick();
    store_writer::tick();
    crate::storage::card_guard::tick();
    fault::install();
    hooks::tick(tick_phase::POLL_DSP);
    worker::tick();
    io::tick();
    let mut i = 0;
    while i < TICK_STEPS.len() {
        // an if chain, not a `match`: see the mailbox
        if let Step::Hook(p, spanned) = TICK_STEPS[i] {
            if spanned {
                phase(p);
            } else {
                hooks::tick(p);
            }
        } else if TICK_STEPS[i] == Step::Settings {
            settings::tick();
        } else if TICK_STEPS[i] == Step::Watch {
            watch::tick();
        } else {
            mailbox();
        }
        i += 1;
    }
    poll_all();
}

/// The mailbox step: runs the command the host wrote, if any.
unsafe fn mailbox() {
    let mb = &mut *addr_of_mut!(DKRT_MAILBOX);
    let cmd = read_volatile(addr_of!(mb.cmd));
    if cmd != 0 {
        // an if chain, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
        let r = if cmd == CMD_LOAD {
            dkrt_load(mb.arg.as_ptr())
        } else if cmd == CMD_SELECT {
            mailbox_select(mb.arg.as_ptr(), 2)
        } else if cmd == CMD_SELECT_ONE {
            mailbox_select(mb.arg.as_ptr().add(1), *mb.arg.as_ptr() as u32)
        } else if cmd == CMD_RESTART {
            dkrt_restart()
        } else if cmd == CMD_HEALTH {
            dkrt_health(addr_of_mut!(DKRT_HEALTH));
            OK
        } else if cmd == CMD_CARD_PROBE {
            card::probe(mb.arg.as_ptr())
        } else if cmd >= CMD_SETTINGS_PUT && cmd <= CMD_SETTINGS_FLUSH {
            settings::mailbox(cmd, mb.arg.as_mut_ptr())
        } else {
            E_FORMAT
        };
        write_volatile(addr_of_mut!(mb.result), r);
        write_volatile(addr_of_mut!(mb.seq), mb.seq.wrapping_add(1));
        write_volatile(addr_of_mut!(mb.cmd), 0);
    }
    poll_all();
}

#[cfg(test)]
mod tests;
