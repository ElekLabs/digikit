//! Each group of the table gives what the owner gives: the entries that convert (an out parameter, an error code, a narrower integer) are called through the
//! package facade and directly, and the entries that forward are the owner's own function.
use super::real;
use crate::package::resident as facade;
use crate::test_lock::LOCK;
use core::ptr::addr_of;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::storage::ROOT_STORE;
use dkrt_sdk::logk;

/// The functions of the table that are an owner's own `extern "C"` function: the same address.
#[test]
fn forwarding_entries_are_the_owners_functions() {
    let r = real();
    macro_rules! same {
        ($entry:expr, $owner:expr) => {
            assert_eq!($entry as usize, $owner as usize, stringify!($owner));
        };
    }
    same!(r.platform.irq_mask, crate::platform::irq_mask as extern "C" fn() -> u32);
    same!(r.platform.irq_restore, crate::platform::irq_restore as extern "C" fn(u32));
    same!(r.platform.flush, crate::platform::flush as extern "C" fn(u32, u32));
    same!(r.platform.has_dsp, crate::platform::has_dsp as extern "C" fn() -> bool);
    same!(r.card.writable, crate::card::writable as extern "C" fn() -> bool);
    same!(r.state.init_once, crate::state::init_once as unsafe extern "C" fn());
    same!(r.fault.panic, crate::fault::panic as extern "C" fn() -> !);
    same!(r.store_writer.is_open, crate::store_writer::is_open as extern "C" fn() -> bool);
    same!(r.store_writer.is_locked, crate::storage::lock::is_locked as extern "C" fn() -> bool);
    same!(r.api.card_busy, crate::api::card_busy as unsafe extern "C" fn() -> bool);
    same!(r.dlog.sink_on, crate::dlog::sink_on as extern "C" fn(u32) -> bool);
    same!(r.dlog.sink_present, crate::dlog::sink_present as extern "C" fn() -> bool);
    same!(r.dlog.span_end, crate::dlog::span_end as unsafe extern "C" fn(u32, u32));
    same!(r.sdk_files.pending, crate::storage::sdk_files::sdk_pending as unsafe extern "C" fn() -> i32);
    same!(r.sdk_files.abort, crate::storage::sdk_files::sdk_abort as unsafe extern "C" fn(i32) -> i32);
    same!(r.settings.module_client, crate::settings::module_client as unsafe extern "C" fn() -> i32);
    same!(r.store.read_sector, crate::store::read_sector as unsafe extern "C" fn(u32, *mut u8) -> i32);
    same!(r.boot.report_dsp, crate::boot::report_dsp as unsafe extern "C" fn(u32, i32));
}

/// The platform: out parameters (`blocks`, `play_state`, `info_dsp_slot`) and error codes (`stat`, `stat_head`, `read_all`).
#[test]
fn the_platform_group_gives_what_the_platform_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let direct = &crate::platform::PLATFORM;
    let table = &facade::platform::PLATFORM;
    let path = b"no/such/file";
    unsafe {
        assert_eq!(table.blocks(), direct.blocks());
        assert_eq!(table.play_state(), direct.play_state());
        assert_eq!(table.info_dsp_slot(0x1234_5678), direct.info_dsp_slot(0x1234_5678));
        assert_eq!(table.device_id(), direct.device_id());
        assert_eq!(table.sample_path_max(), direct.sample_path_max());
        assert_eq!(table.has_dsp(), direct.has_dsp());
        assert_eq!(table.dsp_headroom(), direct.dsp_headroom());
        assert_eq!(table.dsp_voices(), direct.dsp_voices());
        assert_eq!(table.dsp_block_cycles(), direct.dsp_block_cycles());
        assert_eq!(table.irq_mask(), direct.irq_mask());
        assert_eq!(table.list_state_bytes(ROOT_STORE), direct.list_state_bytes(ROOT_STORE));
        assert_eq!(table.stat(ROOT_STORE, path).err(), direct.stat(ROOT_STORE, path).err());
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        assert_eq!(table.stat_head(ROOT_STORE, path, &mut a).err(), direct.stat_head(ROOT_STORE, path, &mut b).err());
        let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
        assert_eq!(table.read_all(ROOT_STORE, path, a.as_mut_ptr(), 16).err(), direct.read_all(ROOT_STORE, path, b.as_mut_ptr(), 16).err());
    }
}

/// The arena: the figures (out parameter), the size and base, and a block taken through the table and given back directly.
#[test]
fn the_arena_group_gives_what_the_arena_gives() {
    let _arena = crate::fake_platform::test_arena::ScriptedArena::up();
    unsafe {
        assert_eq!(facade::arena::stats(), crate::arena::stats());
        let s = crate::platform::st();
        assert_eq!(facade::arena::size(), s.arena.hi - s.arena.lo);
        assert_eq!(facade::arena::base(), addr_of!(crate::state::DKRT_ARENA) as usize as u32);
        let block = facade::arena::alloc(64);
        assert!(!block.is_null());
        assert!(crate::arena::free(block));
        // a second free of the same block is the same refusal on both sides
        assert_eq!(facade::arena::free(block), crate::arena::free(block));
    }
}

/// The log: the source and span figures, a span that is not measured (`-1`) and an event with its five words.
#[test]
fn the_dlog_group_gives_what_the_log_gives() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        assert_eq!(facade::dlog::current_source(), crate::dlog::current_source());
        assert_eq!(facade::dlog::span_owner(), crate::dlog::span_owner());
        assert_eq!(facade::dlog::span_t0(), crate::dlog::span_t0());
        assert_eq!(facade::dlog::sink_present(), crate::dlog::sink_present());
        assert_eq!(facade::dlog::sink_on(0), crate::dlog::sink_on(0));
        let before = core::ptr::read_volatile(addr_of!(crate::dlog::DKRT_LOG_COUNT));
        facade::dlog::ev(logk::MOD_LOG, logk::F_ERROR, [1, 2, 3, 4, 5]);
        let ring = &*addr_of!(crate::dlog::DKRT_LOG);
        let e = ring[(before as usize) % ring.len()];
        assert_eq!(core::ptr::read_volatile(addr_of!(crate::dlog::DKRT_LOG_COUNT)), before + 1);
        assert_eq!((e.kind, e.a), (logk::MOD_LOG, [1, 2, 3, 4, 5]));
        // the source set by `enter_module` is the one `leave_module` takes back, on both sides
        let prev = facade::dlog::enter_module(1, 1);
        assert_eq!(facade::dlog::current_source(), crate::dlog::current_source());
        facade::dlog::leave_module(prev);
        assert_eq!(facade::dlog::current_source(), prev as u8);
    }
}

/// The store, settings, selection and the queues: error codes of an unknown file or key, the capacity figures (out parameter) and the state words.
#[test]
fn the_store_settings_and_selection_groups_give_what_the_owners_give() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let path = b"no/such/file\0";
        assert_eq!(facade::store::find(path.as_ptr()).err(), crate::store::find(path.as_ptr()).err());
        let mut out = [0u8; 8];
        assert_eq!(facade::settings::get(b"no.such.key", &mut out), crate::settings::get(b"no.such.key", &mut out));
        assert_eq!(facade::settings::capacity(), crate::settings::capacity());
        assert_eq!(facade::selection::legacy_state(), (*addr_of!(crate::selection::REC)).map);
        assert_eq!(facade::worker::cancel(0, 9999), crate::worker::cancel(0, 9999));
        assert_eq!(facade::worker::retire(0), crate::worker::retire(0));
        assert_eq!(facade::io::retire(0), crate::io::retire(0));
    }
}
