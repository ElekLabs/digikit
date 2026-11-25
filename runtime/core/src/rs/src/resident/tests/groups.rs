//! The groups `parity.rs` leaves out. Each facade call reaches the entry of its group with the arguments in the right order and width (a table of fakes records
//! them), and each thunk that converts gives the owner the effect the owner's own function gives (the boot record, the log, the observers).
use super::{real, TABLE};
use crate::package::resident as facade;
use crate::test_lock::LOCK;
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::module::LOG_NAME;
use dkrt_abi::resident::{ApiFns, BootFns, Group, IntroFns, ResidentTable, StoreWriterFns, TransportFns};
use dkrt_sdk::{DkrtBootInfo, NEEDS_LEN, SEL_MAX};

// ---- the facade reaches the entry of its group -------------------------------------------------------------------------------------------------------------

/// What the fakes saw, in order: the entry and its arguments.
static mut SEEN: std::vec::Vec<(&'static str, [u64; 5])> = std::vec::Vec::new();

fn saw(entry: &'static str, args: [u64; 5]) {
    // SAFETY: the tests that use the fakes hold `LOCK`.
    unsafe { (*addr_of_mut!(SEEN)).push((entry, args)) }
}

unsafe extern "C" fn f_card_busy() -> bool {
    saw("card_busy", [0; 5]);
    true
}
unsafe extern "C" fn f_notify_unload(owner: u32, base: u32, size: u32) {
    saw("notify_unload", [owner as u64, base as u64, size as u64, 0, 0])
}
unsafe extern "C" fn f_poll_drop_range(base: u32, size: u32) {
    saw("poll_drop_range", [base as u64, size as u64, 0, 0, 0])
}
unsafe extern "C" fn f_reply(cmd: u32, seq: u32, status: i32, extra: usize) -> i32 {
    saw("reply", [cmd as u64, seq as u64, status as u32 as u64, extra as u64, 0]);
    7
}
unsafe extern "C" fn f_reply_body() -> *mut u8 {
    saw("reply_body", [0; 5]);
    0x40 as *mut u8
}
unsafe extern "C" fn f_commit(rec: u32) {
    saw("commit", [rec as u64, 0, 0, 0, 0])
}
unsafe extern "C" fn f_discard(rec: u32) {
    saw("discard", [rec as u64, 0, 0, 0, 0])
}
extern "C" fn f_is_open() -> bool {
    saw("is_open", [0; 5]);
    true
}
extern "C" fn f_is_locked() -> bool {
    saw("is_locked", [0; 5]);
    true
}
extern "C" fn f_retire(owner: u32) {
    saw("retire", [owner as u64, 0, 0, 0, 0])
}
unsafe extern "C" fn f_note_boot_path(k: u32, path: *const u8) {
    saw("note_boot_path", [k as u64, path as u64, 0, 0, 0])
}
unsafe extern "C" fn f_report_module(k: u32, path: *const u8, result: i32, missing: *const u8, name: *const u8) {
    saw("report_module", [k as u64, path as u64, result as u32 as u64, missing as u64, name as u64])
}
unsafe extern "C" fn f_report_provider(path: *const u8, len: u32, result: i32, hash: u32, base: u32) {
    saw("report_provider", [path as u64, len as u64, result as u32 as u64, hash as u64, base as u64])
}
unsafe extern "C" fn f_report_dsp(idx: u32, result: i32) {
    saw("report_dsp", [idx as u64, result as u32 as u64, 0, 0, 0])
}
unsafe extern "C" fn f_report_taken_down(idx: u32, code: i32) {
    saw("report_taken_down", [idx as u64, code as u32 as u64, 0, 0, 0])
}
unsafe extern "C" fn f_begin_live(k: u32) {
    saw("begin_live", [k as u64, 0, 0, 0, 0])
}
unsafe extern "C" fn f_restore_live(k: u32) {
    saw("restore_live", [k as u64, 0, 0, 0, 0])
}
unsafe extern "C" fn f_unloaded(k: u32) {
    saw("unloaded", [k as u64, 0, 0, 0, 0])
}
unsafe extern "C" fn f_live_index(path: *const u8, live: *const bool, out: *mut u32) -> i32 {
    saw("live_index", [path as u64, live as u64, 0, 0, 0]);
    *out = 5;
    0
}
unsafe extern "C" fn f_account_load(k: u32, path: *const u8, result: i32, missing: *const u8, name: *const u8) {
    saw("account_load", [k as u64, path as u64, result as u32 as u64, missing as u64, name as u64])
}

static FAKE_API: Group<ApiFns> = Group::new(ApiFns { card_busy: f_card_busy, notify_unload: f_notify_unload, poll_drop_range: f_poll_drop_range });
static FAKE_TRANSPORT: Group<TransportFns> = Group::new(TransportFns { reply: f_reply, reply_body: f_reply_body });
static FAKE_INTRO: Group<IntroFns> = Group::new(IntroFns { commit: f_commit, discard: f_discard });
static FAKE_STORE_WRITER: Group<StoreWriterFns> = Group::new(StoreWriterFns { is_open: f_is_open, is_locked: f_is_locked, retire: f_retire });
static FAKE_BOOT: Group<BootFns> = Group::new(BootFns {
    note_boot_path: f_note_boot_path,
    report_module: f_report_module,
    report_provider: f_report_provider,
    report_dsp: f_report_dsp,
    report_taken_down: f_report_taken_down,
    begin_live: f_begin_live,
    restore_live: f_restore_live,
    unloaded: f_unloaded,
    live_index: f_live_index,
    account_load: f_account_load,
});

/// Reinstalls the real table when dropped, also when an assert unwinds.
struct RestoreTable;

impl Drop for RestoreTable {
    fn drop(&mut self) {
        unsafe { facade::install(&TABLE).expect("the real table fits") };
    }
}

/// The facade of the api, transport, intro, store writer and boot groups passes each argument to its entry, in order, widened or narrowed as the table says.
#[test]
fn the_facade_calls_the_entry_of_the_api_transport_intro_writer_and_boot_groups() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let mut t: ResidentTable = unsafe { core::ptr::read(&TABLE) };
    t.api = &FAKE_API;
    t.transport = &FAKE_TRANSPORT;
    t.intro = &FAKE_INTRO;
    t.store_writer = &FAKE_STORE_WRITER;
    t.boot = &FAKE_BOOT;
    let t: &'static ResidentTable = std::boxed::Box::leak(std::boxed::Box::new(t));
    let path = b"modules/x/y.dkm\0";
    let missing = [3u8; NEEDS_LEN];
    let name = [4u8; LOG_NAME];
    let live = [false; SEL_MAX];
    unsafe {
        (*addr_of_mut!(SEEN)).clear();
        facade::install(t).expect("the fake table fits");
        let _restore = RestoreTable;
        assert!(facade::api::card_busy());
        facade::api::notify_unload(3, 0x1000, 0x200);
        facade::api::poll_drop_range(0x2000, 0x30);
        assert_eq!(facade::transport::reply(0x12, 0x345, -4, 6), 7);
        assert_eq!(facade::transport::rbody() as usize, 0x40);
        facade::intro::commit(2);
        facade::intro::discard(5);
        assert!(facade::store_writer::is_open());
        assert!(facade::store_writer::is_locked());
        facade::store_writer::retire(9);
        facade::boot::note_boot_path(1, path.as_ptr());
        facade::boot::report_module(2, path.as_ptr(), -9, &missing, &name);
        facade::boot::report_provider(&path[..5], 4, 0xabcd, 0x5000);
        facade::boot::report_dsp(6, -2);
        facade::boot::report_taken_down(7, -3);
        facade::boot::begin_live(8);
        facade::boot::restore_live(9);
        facade::boot::unloaded(10);
        assert_eq!(facade::boot::live_index(path.as_ptr(), &live), Ok(5));
        facade::boot::account_load(11, path.as_ptr(), -5, missing, &name);
        let p = path.as_ptr() as u64;
        let seen = (*addr_of!(SEEN)).clone();
        let at = |i: usize| seen[i];
        assert_eq!(at(0), ("card_busy", [0; 5]));
        assert_eq!(at(1), ("notify_unload", [3, 0x1000, 0x200, 0, 0]));
        assert_eq!(at(2), ("poll_drop_range", [0x2000, 0x30, 0, 0, 0]));
        assert_eq!(at(3), ("reply", [0x12, 0x345, (-4i32) as u32 as u64, 6, 0]));
        assert_eq!(at(4), ("reply_body", [0; 5]));
        assert_eq!(at(5), ("commit", [2, 0, 0, 0, 0]));
        assert_eq!(at(6), ("discard", [5, 0, 0, 0, 0]));
        assert_eq!(at(7), ("is_open", [0; 5]));
        assert_eq!(at(8), ("is_locked", [0; 5]));
        assert_eq!(at(9), ("retire", [9, 0, 0, 0, 0]));
        assert_eq!(at(10), ("note_boot_path", [1, p, 0, 0, 0]));
        assert_eq!(at(11).1[..3], [2, p, (-9i32) as u32 as u64]);
        assert_eq!(at(11).1[3], missing.as_ptr() as u64);
        assert_eq!(at(11).1[4], name.as_ptr() as u64);
        assert_eq!(at(12), ("report_provider", [p, 5, 4, 0xabcd, 0x5000]));
        assert_eq!(at(13), ("report_dsp", [6, (-2i32) as u32 as u64, 0, 0, 0]));
        assert_eq!(at(14), ("report_taken_down", [7, (-3i32) as u32 as u64, 0, 0, 0]));
        assert_eq!(at(15), ("begin_live", [8, 0, 0, 0, 0]));
        assert_eq!(at(16), ("restore_live", [9, 0, 0, 0, 0]));
        assert_eq!(at(17), ("unloaded", [10, 0, 0, 0, 0]));
        assert_eq!(at(18).0, "live_index");
        assert_eq!(at(18).1[..2], [p, live.as_ptr() as u64]);
        assert_eq!(at(19).1[..3], [11, p, (-5i32) as u32 as u64]);
        assert_eq!(at(19).1[4], name.as_ptr() as u64);
        assert_eq!(seen.len(), 20);
    }
}

// ---- the thunks give the owner's effect --------------------------------------------------------------------------------------------------------------------

/// The bytes of the boot record and the module names: what the boot thunks write.
fn boot_bytes() -> std::vec::Vec<u8> {
    unsafe {
        let mut v = core::slice::from_raw_parts(addr_of!(crate::boot::DKRT_BOOT) as *const u8, core::mem::size_of::<DkrtBootInfo>()).to_vec();
        v.extend_from_slice(core::slice::from_raw_parts(addr_of!(crate::boot::BOOT_NAME) as *const u8, core::mem::size_of_val(&*addr_of!(crate::boot::BOOT_NAME))));
        v
    }
}

fn set_boot_bytes(v: &[u8]) {
    unsafe {
        let n = core::mem::size_of::<DkrtBootInfo>();
        core::ptr::copy_nonoverlapping(v.as_ptr(), addr_of_mut!(crate::boot::DKRT_BOOT) as *mut u8, n);
        core::ptr::copy_nonoverlapping(v[n..].as_ptr(), addr_of_mut!(crate::boot::BOOT_NAME) as *mut u8, v.len() - n);
    }
}

/// The last log event and the count.
fn log_tail() -> (u32, Option<(u16, [u32; 5])>) {
    unsafe {
        let n = core::ptr::read_volatile(addr_of!(crate::dlog::DKRT_LOG_COUNT));
        let ring = &*addr_of!(crate::dlog::DKRT_LOG);
        let e = (n > 0).then(|| ring[((n - 1) as usize) % ring.len()]).map(|e| (e.kind, e.a));
        (n, e)
    }
}

/// The boot thunks write the boot record and the log as the owner's functions do: the same steps through the table and directly, from the same record, end in the same
/// bytes and the same log events.
#[test]
fn the_boot_group_gives_what_the_boot_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let r = real().boot;
    let path = b"modules/dk/ping.dkm\0";
    let missing = [7u8; NEEDS_LEN];
    let mut name = [0u8; LOG_NAME];
    name[..4].copy_from_slice(b"ping");
    let start = boot_bytes();
    let log0 = log_tail().0;
    // one script, run through the table and then through the owner
    macro_rules! script {
        ($via:ident) => {{
            set_boot_bytes(&start);
            let mut out = std::vec::Vec::new();
            let mut live = [true; SEL_MAX];
            live[3] = false;
            unsafe {
                script!(@call $via, note_boot_path(0, path.as_ptr()));
                script!(@call $via, note_boot_path(3, path.as_ptr()));
                out.push(script!(@live $via, &live));
                script!(@call $via, begin_live(3));
                script!(@call $via, report_module(3, path.as_ptr(), -9, &missing, &name));
                script!(@call $via, report_provider(path, 0, 0x1234, 0x6000));
                script!(@call $via, report_dsp(3, 0));
                script!(@call $via, report_taken_down(3, -7));
                script!(@call $via, account_load(3, path.as_ptr(), -9, missing, &name));
                script!(@call $via, restore_live(3));
                script!(@call $via, unloaded(2));
            }
            out.push(boot_bytes());
            (out, log_tail())
        }};
        (@call table, note_boot_path($k:expr, $p:expr)) => { (r.note_boot_path)($k, $p) };
        (@call table, begin_live($k:expr)) => { (r.begin_live)($k) };
        (@call table, restore_live($k:expr)) => { (r.restore_live)($k) };
        (@call table, unloaded($k:expr)) => { (r.unloaded)($k) };
        (@call table, report_module($k:expr, $p:expr, $r:expr, $m:expr, $n:expr)) => { (r.report_module)($k, $p, $r, $m.as_ptr(), $n.as_ptr()) };
        (@call table, report_provider($p:expr, $r:expr, $h:expr, $b:expr)) => { (r.report_provider)($p.as_ptr(), $p.len() as u32, $r, $h, $b) };
        (@call table, report_dsp($i:expr, $r:expr)) => { (r.report_dsp)($i, $r) };
        (@call table, report_taken_down($i:expr, $c:expr)) => { (r.report_taken_down)($i, $c) };
        (@call table, account_load($k:expr, $p:expr, $r:expr, $m:expr, $n:expr)) => { (r.account_load)($k, $p, $r, $m.as_ptr(), $n.as_ptr()) };
        (@call owner, note_boot_path($k:expr, $p:expr)) => { crate::boot::note_boot_path($k as usize, $p) };
        (@call owner, begin_live($k:expr)) => { crate::boot::begin_live($k as usize) };
        (@call owner, restore_live($k:expr)) => { crate::boot::restore_live($k as usize) };
        (@call owner, unloaded($k:expr)) => { crate::boot::unloaded($k as usize) };
        (@call owner, report_module($k:expr, $p:expr, $r:expr, $m:expr, $n:expr)) => { crate::boot::report_module($k as usize, $p, $r, &$m, &$n) };
        (@call owner, report_provider($p:expr, $r:expr, $h:expr, $b:expr)) => { crate::boot::report_provider($p, $r, $h, $b) };
        (@call owner, report_dsp($i:expr, $r:expr)) => { crate::boot::report_dsp($i, $r) };
        (@call owner, report_taken_down($i:expr, $c:expr)) => { crate::boot::report_taken_down($i, $c) };
        (@call owner, account_load($k:expr, $p:expr, $r:expr, $m:expr, $n:expr)) => { crate::boot::account_load($k as usize, $p, $r, $m, &$n) };
        (@live table, $l:expr) => {{
            let mut at = 0u32;
            let code = (r.live_index)(path.as_ptr(), $l.as_ptr(), &mut at);
            let mut b = std::vec::Vec::new();
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(&if code == 0 { at } else { 0 }.to_le_bytes());
            b
        }};
        (@live owner, $l:expr) => {{
            let (code, at) = match crate::boot::live_index(path.as_ptr(), $l) {
                Ok(i) => (0i32, i as u32),
                Err(e) => (e, 0),
            };
            let mut b = std::vec::Vec::new();
            b.extend_from_slice(&code.to_le_bytes());
            b.extend_from_slice(&at.to_le_bytes());
            b
        }};
    }
    let via_table = script!(table);
    let log_table = log_tail().0 - log0;
    let log1 = log_tail().0;
    let via_owner = script!(owner);
    let log_owner = log_tail().0 - log1;
    assert_eq!(via_table.0[0], via_owner.0[0], "live_index");
    let diff: std::vec::Vec<usize> = (0..via_table.0[1].len()).filter(|i| via_table.0[1][*i] != via_owner.0[1][*i]).collect();
    assert!(diff.is_empty(), "boot record bytes differ at {diff:?}");
    assert_eq!(via_table.1 .1, via_owner.1 .1);
    assert_eq!(log_table, log_owner);
    assert!(log_table > 0, "the script logs");
    set_boot_bytes(&start);
}

/// `ApiFns`: the unload thunk tells the observers of the owner (the owner id as the record index, a range dropping the observers inside it), and the poll thunk drops
/// the polls in its range.
#[test]
fn the_api_group_gives_what_the_api_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    static mut TOLD: std::vec::Vec<u32> = std::vec::Vec::new();
    unsafe extern "C" fn observer(owner: u32) {
        (*addr_of_mut!(TOLD)).push(owner)
    }
    let r = real().api;
    unsafe {
        (*addr_of_mut!(TOLD)).clear();
        assert_eq!(crate::api::dkrt_unload_observer_add(observer as usize), dkrt_sdk::OK);
        (r.notify_unload)(4, 0x10, 0x10);
        crate::api::notify_unload(5, 0x10, 0x10);
        let told = (*addr_of!(TOLD)).clone();
        crate::api::dkrt_unload_observer_remove(observer as usize);
        assert_eq!(told, [4, 5]);

        // the poll thunk drops the poll inside its range and keeps one outside it
        let (inside, outside) = (0x9000usize, 0x9100usize);
        assert_eq!(crate::api::dkrt_poll_add(inside), dkrt_sdk::OK);
        assert_eq!(crate::api::dkrt_poll_add(outside), dkrt_sdk::OK);
        (r.poll_drop_range)(0x9000, 0x10);
        let t = &*addr_of!(crate::state::DKRT_POLLS);
        let (has_inside, has_outside) = (t.contains(&inside), t.contains(&outside));
        crate::api::dkrt_poll_remove(inside);
        crate::api::dkrt_poll_remove(outside);
        assert!(!has_inside && has_outside);
    }
}

/// The transport thunks: the body buffer is the owner's, and a reply writes the status into byte 0 and records it as the owner's reply does.
#[test]
fn the_transport_group_gives_what_the_transport_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let r = real().transport;
    unsafe {
        assert_eq!((r.reply_body)(), addr_of_mut!(crate::transport::RBODY) as *mut u8);
        let _ = (r.reply)(0x20, 3, -2, 0);
        assert_eq!(*(r.reply_body)(), (-2i32) as u8);
        assert_eq!(core::ptr::read(addr_of!(crate::transport::DKRT_UPLOAD_LAST)), -2);
    }
}

/// The groups whose every entry is an owner's own function: the same address as the owner's, entry by entry.
#[test]
fn the_remaining_forwarding_entries_are_the_owners_functions() {
    let r = real();
    macro_rules! same {
        ($entry:expr, $owner:expr) => {
            assert_eq!($entry as usize, $owner as usize, stringify!($owner));
        };
    }
    use crate::storage::sdk_files as sdk;
    same!(r.sdk_files.create, sdk::sdk_create);
    same!(r.sdk_files.write, sdk::sdk_write);
    same!(r.sdk_files.close, sdk::sdk_close);
    same!(r.sdk_files.abort, sdk::sdk_abort);
    same!(r.sdk_files.delete, sdk::sdk_delete);
    same!(r.sdk_files.commit, sdk::sdk_commit);
    same!(r.sdk_files.pending, sdk::sdk_pending);
    same!(r.api.card_busy, crate::api::card_busy);
    same!(r.api.poll_drop_range, crate::api::poll_drop_range);
    same!(r.boot.report_dsp, crate::boot::report_dsp);
    same!(r.boot.report_taken_down, crate::boot::report_taken_down);
    same!(r.store_writer.is_open, crate::store_writer::is_open);
    same!(r.state.init_once, crate::state::init_once);
}

/// The import rows: the entries that name an owner's function hold its address, every function of the resident core is there, and the ones only the device links are
/// null on the host (their rows are checked against `sdk/sdk.toml` by `imports.rs` at build time).
#[test]
fn the_import_rows_name_the_owners_functions() {
    let i = real().imports;
    assert_eq!(i.dk_log_event as usize, crate::dlog::dk_log_event as usize);
    assert_eq!(i.dk_sink_set as usize, crate::dlog::dk_sink_set as usize);
    assert_eq!(i.dk_span_sink_set as usize, crate::dlog::dk_span_sink_set as usize);
    assert_eq!(i.dk_debug_log_sink_set as usize, crate::dlog::dk_debug_log_sink_set as usize);
    let served = [
        i.memcpy, i.memset, i.memcmp, i.muldi3, i.udivdi3, i.umoddi3, i.dk_alloc, i.dk_free, i.dk_log, i.dk_log_event, i.dk_sink_set, i.dk_span_sink_set, i.dk_time_blocks,
        i.dk_io_post, i.dk_io_cancel, i.dk_io_wake, i.dk_helper_run, i.dk_helper_wait, i.dk_job_post, i.dk_job_cancel, i.dk_hash32_init, i.dk_hash32_update,
        i.dk_hash32_finish, i.dk_intro_take, i.dk_debug_log_sink_set,
    ];
    assert!(served.iter().all(|p| !p.is_null()));
    let device_only = [
        i.dk_fw_alloc, i.dk_fw_free, i.dk_fw_block_read, i.dk_debug_peek, i.dk_debug_span_clock, i.dk_debug_poll, i.dk_debug_track_type, i.dk_debug_selected_params, i.dk_debug_top_view,
        i.dk_debug_setup, i.dk_device_track_count, i.dk_selected_track, i.dk_debug_speed_flags,
    ];
    assert!(device_only.iter().all(|p| p.is_null()));
    let mut all = std::vec::Vec::from(served);
    all.sort();
    all.dedup();
    assert_eq!(all.len(), served.len(), "no two imports share a function");
}

/// The host directory group: what the package publishes through the table is what a tool reads in `DKRT_HOST_DIR` by id.
#[test]
fn the_host_dir_group_fills_the_directory_the_tools_read() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        crate::host_dir::clear();
        let r = real();
        assert!((r.host_dir.publish)(dkrt_sdk::HOST_TABLE_BAD, 0x4000_0040));
        assert_eq!((*addr_of!(crate::host_dir::DKRT_HOST_DIR)).get(dkrt_sdk::HOST_TABLE_BAD), Some(0x4000_0040));
        assert!(!(r.host_dir.publish)(dkrt_sdk::HOST_COUNT as u32, 4));
        assert!(!(r.host_dir.publish)(dkrt_sdk::HOST_PACKAGE_LO, 4), "the core's ids are not the package's to write");
        crate::host_dir::clear();
    }
}
