//! The upload commands about modules: MODULES, LOADS and SELECT, and the commands that modules register (`dk_host_register`).

use dkrt_abi::mem::wr32;
use crate::package::resident::{boot, dlog};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_sdk::proto::*;
use dkrt_sdk::OK;
use crate::package::resident::transport::{rbody, reply};

/// The sections of module record `owner` (a record index + 1) in a MODULES reply, into `out`: `u8 n_sections`, then per section `u32 service name hash, u8 n, n words` (big
/// endian), in the table's order. A service that has no word for the record gives no section. The core reads no meaning into the words. The length written, or `None` when the
/// sections do not fit `out` (the caller ends the page before the record).
pub(crate) unsafe fn put_sections(table: &dkrt_abi::service::ServiceTable, owner: u8, out: &mut [u8]) -> Option<usize> {
    let mut at = 1usize;
    let mut n = 0u8;
    let mut fits = !out.is_empty();
    table.describe(owner, |name, words| {
        let end = at + 5 + 4 * words.len();
        if !fits || end > out.len() || n == u8::MAX {
            fits = false;
            return;
        }
        out[at..at + 4].copy_from_slice(&name.to_be_bytes());
        out[at + 4] = words.len() as u8;
        let mut k = 0;
        while k < words.len() {
            out[at + 5 + 4 * k..at + 9 + 4 * k].copy_from_slice(&words[k].to_be_bytes());
            k += 1;
        }
        at = end;
        n += 1;
    });
    if !fits {
        return None;
    }
    out[0] = n;
    Some(at)
}

/// The fixed part of a MODULES record: `u8 index` to `u32 dsp_len`.
pub(crate) struct Fixed {
    pub(crate) index: u8,
    pub(crate) name: [u8; 32],
    pub(crate) version: u32,
    /// The runtime version the module needs.
    pub(crate) needs: u32,
    /// The module's Workbench instance token (0: it has no descriptor).
    pub(crate) token: u32,
    pub(crate) cf_base: u32,
    pub(crate) cf_size: u32,
    pub(crate) dsp_off: u32,
    pub(crate) dsp_len: u32,
}

/// One record of the MODULES reply into `out`: the fixed part (61 bytes) and the sections of module record `fixed.index` (`put_sections`). The length written, or `None` when it does
/// not fit `out`.
pub(crate) unsafe fn put_record(table: &dkrt_abi::service::ServiceTable, fixed: &Fixed, out: &mut [u8]) -> Option<usize> {
    if out.len() <= RECORD_FIXED {
        return None;
    }
    let sections = put_sections(table, fixed.index + 1, &mut out[RECORD_FIXED..])?;
    out[0] = fixed.index;
    out[1..33].copy_from_slice(&fixed.name);
    let words = [fixed.version, fixed.needs, fixed.token, fixed.cf_base, fixed.cf_size, fixed.dsp_off, fixed.dsp_len];
    let mut k = 0;
    while k < words.len() {
        out[33 + 4 * k..37 + 4 * k].copy_from_slice(&words[k].to_be_bytes());
        k += 1;
    }
    Some(RECORD_FIXED + sections)
}

/// MODULES: the loaded modules and what they hold. Body: `u8 start` (a module record index). Reply: `u32 arena_free, arena_largest, dsp_free,
/// dsp_largest`, `u8 total` (loaded modules), `u8 n` (records in this reply), then per record `u8 index`, `name[32]`, `u32 version`, 8 bytes (`u32` the runtime
/// version the module needs, then its Workbench instance token, 0 if it has no descriptor), `u32 cf_base, cf_size, dsp_off, dsp_len` (the fixed part ends here, 61 bytes), `u8 n_sections`
/// and per section `u32 service name hash, u8 n, n words` (`put_sections`: what the loaded services hold for the record, by their `Describe` part; a service with nothing gives
/// no section; the core reads no meaning into the words). A reply holds the records from `start` on that fit; the host asks again from the last index + 1 until it has `total`.
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
    let room = dkrt_abi::command::REPLY_BODY_MAX - 1;
    let table = crate::package::handlers::services();
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
        let fixed = Fixed {
            index: i as u8,
            name: m.name,
            version: m.version,
            needs: (*crate::package::resident::state::module_runtime())[i],
            token: crate::package::module_ui::instance(i),
            cf_base: m.base,
            cf_size: m.size,
            dsp_off: d.mod_off[i],
            dsp_len: d.mod_len[i],
        };
        // a record whose sections do not fit ends the page
        let Some(len) = put_record(&table, &fixed, core::slice::from_raw_parts_mut(rb.add(at), room.saturating_sub(at))) else {
            break;
        };
        at += len;
        n += 1;
        i += 1;
    }
    *rb.add(17) = n;
    reply(CMD_MODULES, seq, OK, at)
}

/// The bytes of the fixed part of a MODULES record: `u8 index` to `u32 dsp_len`.
const RECORD_FIXED: usize = 61;

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
    /// Section tests: the encoder of the MODULES reply over the fake table of two services (`handlers::tests`: service 1 has two words for every consumer, service 2 one word for
    /// consumer 3 and none for the others).
    mod sections {
        use super::*;
        use crate::package::handlers::tests::two;
        use dkrt_abi::provider::name_hash;

        fn put(owner: u8, room: usize) -> Option<std::vec::Vec<u8>> {
            let mut buf = [0xeeu8; 64];
            let n = unsafe { put_sections(&two(), owner, &mut buf[..room]) }?;
            Some(buf[..n].to_vec())
        }

        fn section(name: &str, words: &[u32]) -> std::vec::Vec<u8> {
            let mut v = name_hash(name).to_be_bytes().to_vec();
            v.push(words.len() as u8);
            for w in words {
                v.extend_from_slice(&w.to_be_bytes());
            }
            v
        }

        #[test]
        fn a_service_with_words_for_the_record_gives_a_section_and_a_silent_one_gives_none() {
            // consumer 1: only service 1 has words
            let mut want = std::vec![1u8];
            want.extend(section("fake1", &[0x1101, 2]));
            assert_eq!(put(1, 64), Some(want));
            // consumer 3: both, in table order
            let mut want = std::vec![2u8];
            want.extend(section("fake1", &[0x1103, 6]));
            want.extend(section("fake2", &[0x2203]));
            assert_eq!(put(3, 64), Some(want));
        }

        #[test]
        fn sections_that_do_not_fit_are_refused_whole() {
            // consumer 3 needs 1 + (4 + 1 + 8) + (4 + 1 + 4) = 23 bytes
            assert_eq!(put(3, 23).map(|v| v.len()), Some(23));
            assert_eq!(put(3, 22), None, "the second section is cut: none of the record is written");
            assert_eq!(put(1, 14).map(|v| v.len()), Some(14));
            assert_eq!(put(1, 13), None);
            assert_eq!(put(1, 0), None, "no room for the count");
        }
    }

    /// The machines service itself (the crate of the packed file, over the table that `machines_row` builds), through the encoder: a module that owns a voice row has the machines
    /// section of five words, one that owns nothing has none. The host runs no ColdFire code and loads no DSP, so no module registers a machine or a voice row here (the assembly tests
    /// cannot show it): the voice row mask is set as a DSP load would.
    #[test]
    fn the_machines_service_gives_a_section_to_a_module_with_a_voice_row_and_none_to_one_without() {
        use crate::package::fixture_provider::table;
        let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        let t = unsafe { table(std::vec![crate::package::handlers::test_seams::machines_row().resolve().unwrap()]) };
        unsafe {
            crate::package::handlers::test_seams::set_voice_rows(0, Some(0b1));
            let sections = |owner: u8| {
                let mut buf = [0u8; 128];
                let n = put_sections(&t, owner, &mut buf).unwrap();
                buf[..n].to_vec()
            };
            let with = sections(1);
            assert_eq!(with[0], 1, "one section for the module with a voice row: {with:x?}");
            assert_eq!(with[1..5], dkrt_abi::provider::name_hash("machines").to_be_bytes(), "it is the machines section");
            assert_eq!(with[5], 5, "five words");
            assert_eq!(u32::from_be_bytes(with[6..10].try_into().unwrap()), 0b1, "the first is the voice rows of the module");
            assert_eq!(sections(2), [0], "a module with nothing has no section");
            crate::package::handlers::test_seams::set_voice_rows(0, None);
        }
    }

    /// The reply the host decoders read: `tests/fixtures/protocol/modules-reply.json` holds the bytes of two records (the `machines` service has five words for the first, the
    /// `fx` service two for the second, and each is silent for the other record) and what a host reads from them; the TypeScript and Python decoders test against the same file.
    /// `DIGI_WRITE_FIXTURES=1` writes it again.
    mod fixture {
        use super::*;
        use crate::package::handlers::tests::{fake, set_describe, KINDS_1, KINDS_2, LOCK};
        use crate::package::fixture_provider::table;
        use dkrt_abi::provider::name_hash;

        const MACHINES: [u32; 5] = [0b101, 3, 2 | 1 << 8, 0x1e10, 0];
        const FX: [u32; 2] = [0x13_1200, 0x11_1101];

        fn record(index: u8, name: &str, base: u32) -> Fixed {
            let mut n = [0u8; 32];
            n[..name.len()].copy_from_slice(name.as_bytes());
            Fixed { index, name: n, version: 0x0001_0203, needs: 0x000b_0400, token: if index == 0 { 0 } else { 7 }, cf_base: base, cf_size: 0x1000, dsp_off: 0x40 * index as u32, dsp_len: 0x200 }
        }

        fn reply() -> std::vec::Vec<u8> {
            let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
            set_describe(Some(&[(1, 1, &MACHINES), (2, 2, &FX)]));
            let t = table(std::vec![fake::<1>("machines", &KINDS_1), fake::<2>("fx", &KINDS_2)]);
            let mut body = std::vec::Vec::new();
            for w in [1000u32, 500, 2000, 1500] {
                body.extend_from_slice(&w.to_be_bytes());
            }
            body.extend_from_slice(&[2, 2]);
            for r in [record(0, "sine", 0x4000_0000), record(1, "gain", 0x4000_2000)] {
                let mut buf = [0u8; 200];
                let n = unsafe { put_record(&t, &r, &mut buf) }.unwrap();
                body.extend_from_slice(&buf[..n]);
            }
            set_describe(None);
            body
        }

        fn hex(b: &[u8]) -> String {
            b.iter().map(|x| std::format!("{x:02x}")).collect()
        }

        fn json(body: &[u8]) -> String {
            let (m, f) = (std::format!("{:08x}", name_hash("machines")), std::format!("{:08x}", name_hash("fx")));
            let words = |w: &[u32]| w.iter().map(|x| x.to_string()).collect::<std::vec::Vec<_>>().join(", ");
            std::format!(
                "{{\n  \"hex\": \"{}\",\n  \"total\": 2,\n  \"modules\": [\n    {{\"index\": 0, \"name\": \"sine\", \"version\": 66051, \"needs\": 721920, \"uiInstance\": null, \"cf_base\": 1073741824, \"cf_size\": 4096, \"dsp_off\": 0, \"dsp_len\": 512,\n     \"sections\": {{\"{m}\": [{}]}},\n     \"rows\": 5, \"types\": [16, 30], \"faults\": 3, \"guard_off\": true, \"effects\": []}},\n    {{\"index\": 1, \"name\": \"gain\", \"version\": 66051, \"needs\": 721920, \"uiInstance\": 7, \"cf_base\": 1073750016, \"cf_size\": 4096, \"dsp_off\": 64, \"dsp_len\": 512,\n     \"sections\": {{\"{f}\": [{}]}},\n     \"rows\": 0, \"types\": [], \"faults\": 0, \"guard_off\": false,\n     \"effects\": [{{\"row\": 0, \"running\": 2, \"saved\": 1, \"allowed\": 3, \"dflt\": 1}}, {{\"row\": 1, \"running\": 1, \"saved\": 1, \"allowed\": 1, \"dflt\": 1}}]}}\n  ]\n}}\n",
                hex(body),
                words(&MACHINES),
                words(&FX),
            )
        }

        #[test]
        fn the_reply_bytes_are_the_fixture_the_host_decoders_read() {
            let body = reply();
            // layout: 18 bytes of header, then each record: 61 fixed bytes, `u8 n_sections`, `u32 hash, u8 n, n words` per section
            assert_eq!(body.len(), 18 + (61 + 1 + 4 + 1 + 20) + (61 + 1 + 4 + 1 + 8));
            assert_eq!(&body[16..18], [2, 2], "total, n");
            assert_eq!(body[18 + 61], 1, "one section for the first record");
            assert_eq!(&body[18 + 62..18 + 66], name_hash("machines").to_be_bytes());
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../tests/fixtures/protocol/modules-reply.json");
            let want = json(&body);
            if std::env::var_os("DIGI_WRITE_FIXTURES").is_some() {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, &want).unwrap();
            }
            assert_eq!(std::fs::read_to_string(&path).expect("tests/fixtures/protocol/modules-reply.json (DIGI_WRITE_FIXTURES=1 writes it)"), want);
        }
    }
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
