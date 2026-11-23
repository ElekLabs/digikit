//! The upload commands about modules: MODULES, LOADS and SELECT, and the commands that modules register (`dk_host_register`).

use dkrt_abi::mem::wr32;
use crate::package::resident::{boot, dlog};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_sdk::proto::*;
use dkrt_sdk::OK;
use crate::package::resident::transport::{rbody, reply};

/// Does a loaded module have machine type `ty` (a run-time type some module owns)? The machines service answers per module (`module_info`, the same export the
/// MODULES reply reads). A row nobody loaded is a gap row: it plays as type 0.
pub(crate) fn machine_type_loaded(ty: u32) -> bool {
    let s = crate::package::resident::state::st();
    let mut i = 0;
    while i < dkrt_abi::MAX_MODULES {
        if s.modules[i].used != 0 {
            if let Some(mm) = unsafe { crate::package::handlers::exports::module_info(i as u8 + 1) } {
                if mm.types[..(mm.nt as usize).min(8)].iter().any(|t| *t as u32 == ty) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// MODULES: the loaded modules and what they hold. Body: `u8 start` (a module record index). Reply: `u32 arena_free, arena_largest, dsp_free,
/// dsp_largest`, `u8 total` (loaded modules), `u8 n` (records in this reply), then per record `u8 index`, `name[32]`, `u32 version`, 8 bytes (`u32` the runtime
/// version the module needs, then its Workbench instance token, 0 if it has no descriptor), `u32 cf_base, cf_size, dsp_off, dsp_len, rows` (voice rows as a mask), `u8 n_types` and that many machine
/// type bytes (at most 8: the machine rows of the module, then its voice rows' types), `u32 faults` (the guard's count for those types), `u8 flags` (bit 0:
/// the guard switched one of them off). A reply holds the records from `start` on that fit; the host asks again from the last index + 1 until it has `total`.
pub(crate) unsafe fn cmd_modules(seq: u32, body: *const u8, blen: usize) -> i32 {
    if blen < 1 {
        return reply(CMD_MODULES, seq, E_MSG, 0);
    }
    let start = *body as usize;
    let s = crate::package::resident::state::st();
    let rb = rbody().add(1);
    let (af, al) = crate::package::resident::arena::stats();
    let (df, dl) = crate::package::dsp::area_stats();
    wr32(rb, af);
    wr32(rb.add(4), al);
    wr32(rb.add(8), df);
    wr32(rb.add(12), dl);
    let mut total = 0u8;
    let mut i = 0;
    while i < dkrt_abi::MAX_MODULES {
        if s.modules[i].used != 0 {
            total += 1;
        }
        i += 1;
    }
    *rb.add(16) = total;
    let mut at = 18usize;
    let mut n = 0u8;
    let d = crate::package::dsp::ds();
    i = start;
    while i < dkrt_abi::MAX_MODULES {
        let m = &s.modules[i];
        if m.used == 0 {
            i += 1;
            continue;
        }
        // the machines of the module (the machines service: voice rows, types, guard counts; none without the service file)
        let mm = crate::package::handlers::exports::module_info(i as u8 + 1).unwrap_or_default();
        let nt = (mm.nt as usize).min(8);
        if at + 71 + nt > dkrt_abi::command::REPLY_BODY_MAX - 1 {
            break;
        }
        *rb.add(at) = i as u8;
        dkrt_abi::mem::copy(rb.add(at + 1), m.name.as_ptr(), 32);
        wr32(rb.add(at + 33), m.version);
        // the version of the runtime the module needs, then its Workbench instance token
        wr32(rb.add(at + 37), (*crate::package::resident::state::module_runtime())[i]);
        wr32(rb.add(at + 41), crate::package::module_ui::instance(i));
        wr32(rb.add(at + 45), m.base);
        wr32(rb.add(at + 49), m.size);
        wr32(rb.add(at + 53), d.mod_off[i]);
        wr32(rb.add(at + 57), d.mod_len[i]);
        wr32(rb.add(at + 61), mm.rows);
        *rb.add(at + 65) = nt as u8;
        let mut j = 0;
        while j < nt {
            *rb.add(at + 66 + j) = mm.types[j];
            j += 1;
        }
        wr32(rb.add(at + 66 + nt), mm.faults);
        *rb.add(at + 70 + nt) = (mm.guard_off != 0) as u8;
        at += 71 + nt;
        n += 1;
        i += 1;
    }
    *rb.add(17) = n;
    reply(CMD_MODULES, seq, OK, at)
}

/// LOADS: what this boot's selection did. Body: none. Reply (version 2, `dkrt_abi::state::loads_v2`): `u8 version` (2), `u8 n` (the modules of the selection, at
/// most 8), then per module in selection order `i32 result` (`OK`, a negative `E_*`, or LOADING while its DSP part is going), `u8 state` (`[load.state]`),
/// 3 zero bytes, `u32 detail[2]` (what the plan refused on), `needs[12]` (with E_IMPORT: the module its imports need that is not in the image, without `core/`,
/// NUL padded; else zeros) and `name[28]` (the file name of its path, NUL padded). The same data as the DIGIKIT menu's BOOT page, for a host that has no debug
/// module to read the log from.
pub(crate) unsafe fn cmd_loads(seq: u32) -> i32 {
    let rb = core::slice::from_raw_parts_mut(rbody().add(1), dkrt_abi::state::loads_len(dkrt_sdk::SEL_MAX));
    let at = dkrt_abi::state::loads_v2(&boot::loads_view(), rb);
    reply(CMD_LOADS, seq, OK, at)
}

pub(crate) unsafe fn h_modules(seq: u32, _buf: *mut u8, body: *const u8, blen: usize) -> i32 {
    cmd_modules(seq, body, blen)
}
pub(crate) unsafe fn h_loads(seq: u32, _buf: *mut u8, _body: *const u8, _blen: usize) -> i32 {
    cmd_loads(seq)
}
// ---- commands that modules register (dk_host_register) ------------------------------------------------------------------

/// Registered ranges at most (a module takes one per command group: digikit/remote 2, digikit/probe 1).
pub(crate) const SVC_MAX: usize = 8;

#[derive(Clone, Copy)]
pub(crate) struct Svc {
    pub(crate) first: u8,
    pub(crate) last: u8,
    /// `dlog::current_source()` of the registering module (record index + 1); 0: the slot is free.
    pub(crate) owner: u8,
    pub(crate) h: Option<dkrt_sdk::HostCmdFn>,
}

pub(crate) static mut SVC: [Svc; SVC_MAX] = [Svc { first: 0, last: 0, owner: 0, h: None }; SVC_MAX];

pub(crate) fn svc() -> &'static mut [Svc; SVC_MAX] {
    unsafe { &mut *addr_of_mut!(SVC) }
}

/// Commands of each module record (owner - 1) whose handler runs now, on the MIDI task. A reload waits until the count is 0 before it frees the module's code
/// (`reload`); the one writer is the MIDI task, which counts the command before the handler can be released.
static mut IN_FLIGHT: [u8; dkrt_abi::MAX_MODULES] = [0; dkrt_abi::MAX_MODULES];
/// Bit `owner - 1`: the module is being reloaded; its commands are refused with `E_BUSY` while the ones in flight finish.
static mut DRAINING: u16 = 0;
const _: () = assert!(dkrt_abi::MAX_MODULES <= 16);

/// Refuse (`on`) or accept again the commands of the module whose source index is `owner`.
pub(crate) unsafe fn set_draining(owner: u8, on: bool) {
    if owner == 0 || owner as usize > dkrt_abi::MAX_MODULES {
        return;
    }
    let sr = crate::package::resident::platform::irq_mask();
    let bit = 1u16 << (owner - 1);
    let d = &mut *addr_of_mut!(DRAINING);
    *d = if on { *d | bit } else { *d & !bit };
    crate::package::resident::platform::irq_restore(sr);
}

pub(crate) unsafe fn is_draining(rec: usize) -> bool {
    core::ptr::read_volatile(addr_of!(DRAINING)) & (1u16 << rec) != 0
}

/// The handler of command `cmd` and its owner, with the command counted as in flight for that owner: the lookup and the count are one step under the interrupt mask,
/// so a module that is torn down either is not found or is counted. `Ok(None)`: no module registered the command; `Err(())`: its module is being reloaded
/// (`set_draining`). Pair a found handler with `end_cmd`.
pub(crate) unsafe fn begin_cmd(cmd: u8) -> Result<Option<(dkrt_sdk::HostCmdFn, u8)>, ()> {
    let sr = crate::package::resident::platform::irq_mask();
    let found = service_of(cmd).and_then(|s| s.h.map(|h| (h, s.owner)));
    let mut r = Ok(found);
    if let Some((_, owner)) = found {
        if owner != 255 && owner > 0 {
            if *addr_of!(DRAINING) & (1u16 << (owner - 1)) != 0 {
                r = Err(());
            } else {
                let c = &mut (*addr_of_mut!(IN_FLIGHT))[owner as usize - 1];
                *c = c.saturating_add(1);
            }
        }
    }
    crate::package::resident::platform::irq_restore(sr);
    r
}

/// The handler returned: the command of `owner` is no longer in flight.
pub(crate) unsafe fn end_cmd(owner: u8) {
    if owner != 255 && owner > 0 {
        // masked like `begin_cmd`: the count is a read-modify-write, and the task that reads it can interrupt this one
        let sr = crate::package::resident::platform::irq_mask();
        let c = &mut (*addr_of_mut!(IN_FLIGHT))[owner as usize - 1];
        *c = c.saturating_sub(1);
        crate::package::resident::platform::irq_restore(sr);
    }
}

/// Host tests: `n` commands of `owner` are in flight.
#[cfg(test)]
pub(crate) unsafe fn test_set_in_flight(owner: u8, n: u8) {
    (*addr_of_mut!(IN_FLIGHT))[owner as usize - 1] = n;
}

/// Commands of the module whose source index is `owner` that are in flight.
pub(crate) unsafe fn in_flight(owner: u8) -> u8 {
    if owner == 0 || owner as usize > dkrt_abi::MAX_MODULES {
        return 0;
    }
    core::ptr::read_volatile(addr_of!(IN_FLIGHT[owner as usize - 1]))
}

/// The handler of a registered command.
pub(crate) fn service(cmd: u8) -> Option<dkrt_sdk::HostCmdFn> {
    service_of(cmd).and_then(|s| s.h)
}

pub(crate) fn service_of(cmd: u8) -> Option<&'static Svc> {
    let t = svc();
    let mut i = 0;
    while i < SVC_MAX {
        if t[i].owner != 0 && cmd >= t[i].first && cmd <= t[i].last {
            return Some(&t[i]);
        }
        i += 1;
    }
    None
}

/// `dk_host_register`: `E_CMD_RANGE` when the range is empty, outside every reserved range, over a registered one, or there is no free
/// slot; else 0.
pub unsafe fn host_register(first: u32, count: u32, handler: Option<dkrt_sdk::HostCmdFn>) -> i32 {
    if handler.is_none() || count == 0 || first == 0 || first > 0xff || count > 0xff || first + count - 1 > 0xff {
        return E_CMD_RANGE as i32;
    }
    let (lo, hi) = (first as u8, (first + count - 1) as u8);
    let mut reserved = false;
    let mut i = 0;
    while i < RANGES.len() {
        if lo >= RANGES[i].0 && hi <= RANGES[i].1 {
            reserved = true;
        }
        i += 1;
    }
    if !reserved {
        return E_CMD_RANGE as i32;
    }
    let t = svc();
    let mut free = SVC_MAX;
    i = 0;
    while i < SVC_MAX {
        if t[i].owner == 0 {
            free = i;
        } else if lo <= t[i].last && t[i].first <= hi {
            return E_CMD_RANGE as i32;
        }
        i += 1;
    }
    if free == SVC_MAX {
        return E_CMD_RANGE as i32;
    }
    // a registration outside a module's init (owner 0) is kept under owner 255: nothing releases it
    let owner = dlog::current_source();
    t[free] = Svc { first: lo, last: hi, owner: if owner == 0 { 255 } else { owner }, h: handler };
    OK
}

/// The module whose source index is `owner` was taken down: release its ranges.
pub unsafe fn release_cmds(owner: u8) {
    set_draining(owner, false);
    let t = svc();
    let mut i = 0;
    while i < SVC_MAX {
        if t[i].owner == owner {
            t[i] = Svc { first: 0, last: 0, owner: 0, h: None };
        }
        i += 1;
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::LOCK;
    use crate::transport::{dkrt_midi_rx, pack, DKRT_UPLOAD_LAST, RBODY};
    use dkrt_sdk::E_BUSY;

    unsafe extern "C" fn answers_ok(_cmd: u32, _seq: u32, _body: *const u8, _len: u32, _out: *mut u8) -> i32 {
        0
    }

    /// One request of command `cmd` with no body, through the MIDI hook; the status the runtime replied with.
    unsafe fn ask(cmd: u8) -> i32 {
        let msg = [0xf0, 0x7d, 0x64, 0x6b, VERSION, cmd, 1, 0xf7];
        assert_eq!(dkrt_midi_rx(msg.as_ptr(), msg.len() as u32, 0), 1);
        DKRT_UPLOAD_LAST
    }

    #[test]
    fn a_command_of_a_module_that_is_being_reloaded_answers_busy_and_counts_nothing() {
        let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        unsafe {
            // the runtime is initialised (the hook's `init_once` would set up the arena)
            crate::package::resident::state::st().magic = dkrt_abi::runtime_state::STATE_MAGIC;
            let prev = dlog::enter_module(0, dlog::PHASE_CALLBACK);
            assert_eq!(host_register(0x38, 1, Some(answers_ok)), OK);
            dlog::leave_module(prev);
            assert_eq!(ask(0x38), OK, "it runs when the module is not draining");
            assert_eq!(in_flight(1), 0, "and is not in flight once it returned");
            set_draining(1, true);
            assert_eq!(ask(0x38), E_BUSY, "its module is draining: the host asks again");
            assert_eq!(in_flight(1), 0);
            set_draining(1, false);
            assert_eq!(ask(0x38), OK);
            release_cmds(1);
            assert_eq!(ask(0x38), E_PROTO, "released: nobody registered it");
        }
    }

    /// What the device test `modules-capacity` showed on the firmware: MODULES pages by the start index (the records from `start` on), `total` stays the number of loaded
    /// modules, and a start past the last record gives none.
    #[test]
    fn modules_pages_by_start_index_and_the_total_stays_the_loaded_count() {
        let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        unsafe {
            crate::package::resident::state::st().magic = dkrt_abi::runtime_state::STATE_MAGIC;
            let used: Vec<u32> = (0..dkrt_abi::MAX_MODULES).map(|i| crate::package::resident::state::st().modules[i].used).collect();
            for i in 0..dkrt_abi::MAX_MODULES {
                crate::package::resident::state::st().modules[i].used = u32::from(i < 3);
            }
            let page = |start: u8| {
                let mut msg = [0xf0, 0x7d, 0x64, 0x6b, VERSION, CMD_MODULES as u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                let n = pack([start].as_ptr(), 1, msg.as_mut_ptr().add(7));
                msg[7 + n] = 0xf7;
                assert_eq!(dkrt_midi_rx(msg.as_ptr(), 8 + n as u32, 0), 1);
                let rb = (addr_of!(RBODY) as *const u8).add(1);
                (*rb.add(16), *rb.add(17), *rb.add(18))
            };
            assert_eq!(page(0), (3, 3, 0), "from the first record");
            assert_eq!(page(1), (3, 2, 1), "from record 1: the two after it");
            assert_eq!((page(9).0, page(9).1), (3, 0), "past the end: no record, the total stays");
            for (i, u) in used.into_iter().enumerate() {
                crate::package::resident::state::st().modules[i].used = u;
            }
        }
    }

    unsafe extern "C" fn answers_77(_cmd: u32, _seq: u32, _body: *const u8, _len: u32, _out: *mut u8) -> i32 {
        -77
    }

    /// What the device test `service-commands` showed on the firmware: a range is refused when it is empty, outside every reserved range, over a registered one (or has no
    /// handler); a handler's own status reaches the host; a command nobody registered is E_PROTO.
    #[test]
    fn a_range_that_is_empty_unreserved_or_taken_is_refused_and_the_handlers_status_is_the_reply() {
        let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        unsafe {
            crate::package::resident::state::st().magic = dkrt_abi::runtime_state::STATE_MAGIC;
            let prev = dlog::enter_module(0, dlog::PHASE_CALLBACK);
            let range = E_CMD_RANGE as i32;
            assert_eq!(host_register(0x38, 0, Some(answers_ok)), range, "empty");
            assert_eq!(host_register(0x38, 1, None), range, "no handler");
            assert_eq!(host_register(0x40, 1, Some(answers_ok)), range, "outside every reserved range");
            assert_eq!(host_register(0x3d, 2, Some(answers_ok)), range, "across the end of a reserved range");
            assert_eq!(host_register(0x38, 1, Some(answers_77)), OK);
            assert_eq!(host_register(0x38, 1, Some(answers_ok)), range, "over a registered one");
            assert_eq!(ask(0x38), -77, "the handler's own status");
            assert_eq!(ask(0x39), E_PROTO, "inside the reserved range but not registered");
            dlog::leave_module(prev);
            release_cmds(1);
        }
    }
}
