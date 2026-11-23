//! Interface C of a service file, both sides linked on the host: `ImportCore` of `dkrt-svc-rt` calls the `dkc_*` functions, which are the runtime's shims of
//! `SVC_IMPORTS` (exported under their names in a test build), which call `RealCore`. A few representative methods give the same answer through the C shape as
//! through `RealCore`, and the `service!` macro's `init` declares the kinds and the row to a collection.
use crate::package::core_api::{CoreApi, REAL_CORE};
use crate::test_lock::LOCK;
use dkrt_abi::core_api::LOG_INFO;
use dkrt_abi::link::{Owner, Slot, Stream};
use dkrt_abi::provider::{Provider, Records, Status};
use dkrt_abi::service::{Declare, SVC_ABI};
use dkrt_svc_rt::ImportCore;

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn plain_values_and_pointers_come_back_as_through_the_real_core() {
    let _g = lock();
    let ic = ImportCore;
    unsafe {
        assert_eq!(ic.device_id(), REAL_CORE.device_id());
        assert_eq!(ic.boot_index(), REAL_CORE.boot_index());
        assert_eq!(ic.legacy_state(), REAL_CORE.legacy_state());
        assert_eq!(ic.current_source(), REAL_CORE.current_source());
        assert_eq!(ic.info(0x1111), REAL_CORE.info(0x1111));
        assert!(ic.info(0).is_null(), "the host image has no info table");
        assert_eq!(ic.module_name(1), REAL_CORE.module_name(1));
    }
    // a reference to the core's state is the same object, a slice is the same bytes (length and address)
    assert!(core::ptr::eq(ic.dsp() as *const _, REAL_CORE.dsp() as *const _));
}

#[test]
fn the_keys_of_an_accepted_module_come_through_the_c_shape() {
    let _g = lock();
    let ic = ImportCore;
    // no plan accepted index 3: none, as through the real core
    let (mut a, mut b) = ([9u32; 8], [9u32; 8]);
    assert_eq!(ic.keys_of(3, 8, &mut a), 0);
    assert_eq!(REAL_CORE.keys_of(3, 8, &mut b), 0);
    assert_eq!((a, b), ([9; 8], [9; 8]), "no key is written for no module");
}

#[test]
fn slices_and_status_results_are_converted_both_ways() {
    let _g = lock();
    let ic = ImportCore;
    unsafe {
        // the settings: the same refusal for the same key, and an output slice that is the caller's
        let (mut x, mut y) = ([7u8; 16], [7u8; 16]);
        let want = REAL_CORE.settings_get(b"nokey", &mut y);
        assert_eq!(ic.settings_get(b"nokey", &mut x), want);
        assert_eq!(x, y);
        let value = [1u8, 2, 3];
        assert_eq!(ic.settings_put(b"nokey", &value), REAL_CORE.settings_put(b"nokey", &value));
        // the log takes five words
        ic.log(dkrt_sdk::logk::MODULE_COMMIT, LOG_INFO, [1, 2, 3, 4, 5]);
    }
}

#[test]
fn the_link_and_the_job_queue_answer_over_the_c_shape() {
    let _g = lock();
    let ic = ImportCore;
    unsafe {
        // an `Option` result is a status and an out pointer: no confirmed value, no ticket for a stream that is not valid
        let slot = Slot::new(2, 5);
        assert_eq!(ic.link_confirmed(slot), REAL_CORE.link_confirmed(slot));
        ic.link_set(slot, 9);
        assert_eq!(ic.link_confirmed(slot), REAL_CORE.link_confirmed(slot));
        assert_eq!(ic.link_errors(), REAL_CORE.link_errors());
        let odd = Stream::Bulk { dst: 1, src: core::ptr::null(), len: 0 };
        assert!(ic.link_submit(Owner::Tracks, &odd).is_none());
        assert!(REAL_CORE.link_submit(Owner::Tracks, &odd).is_none());
        // the queue: a step is posted through the table, and the id cancels it again
        crate::package::core_api::testing::test_reset();
        unsafe extern "C" fn step(_: *mut u8) -> i32 {
            0
        }
        let id = ic.job_post(1, Some(step), None, core::ptr::null_mut());
        assert!(id > 0, "{id}");
        assert_eq!(ic.job_cancel(1, id as u32), 0);
        // no step is the core's refusal
        assert_eq!(ic.job_post(1, None, None, core::ptr::null_mut()), REAL_CORE.job_post(1, None, None, core::ptr::null_mut()));
        crate::package::core_api::testing::test_reset();
    }
}

/// A service that does nothing: only the row and the glue are looked at.
struct Nothing;
impl<C: CoreApi> Provider<C> for Nothing {
    fn start(_core: &C) -> Result<Nothing, Status> {
        Ok(Nothing)
    }
}
impl<C: CoreApi> Records<C> for Nothing {}

mod glue {
    // `dk_module`, `dk_svc_abi` and the row, as a service file's crate gets them
    dkrt_svc_rt::service!(super::Nothing, "nothing", &[(90, 24), (91, 32)], parts = [Records]);

    pub fn row() -> &'static dkrt_abi::provider::ProviderRow {
        dk_svc_row()
    }
    pub fn module() -> &'static dkrt_svc_rt::ModuleDef {
        &dk_module
    }
    pub fn abi() -> u32 {
        dk_svc_abi
    }
}

#[test]
fn the_service_macro_gives_the_fingerprint_and_an_init_that_declares_kinds_and_row() {
    let _g = lock();
    // the handler is a host function of this file: the map gives it its device address
    let map = crate::fake_platform::address_map::AddressMap::new();
    let _space = map.install();
    assert_eq!(glue::abi(), SVC_ABI);
    assert_ne!(SVC_ABI, 0);
    let m = glue::module();
    assert!(m.init.is_some() && m.fini.is_some() && m.tick.is_none() && m.menu.is_none());
    assert_eq!(glue::row().name(), dkrt_abi::provider::name_hash("nothing"));
    unsafe {
        let mut col = std::boxed::Box::new(crate::package::collect::Collect::new());
        crate::package::collect::begin(&mut *col);
        let ctx = crate::package::entry::bind(0) as *mut u8;
        let r = (m.init.unwrap())(ctx);
        crate::package::collect::end();
        crate::package::entry::unbind(0);
        assert_eq!(r, 0);
        // two kinds with the handler the macro passes (the address of init, a function of the file), then the row and the instance `start` made
        let d = &col.decl;
        assert_eq!(d.n, 4);
        assert_eq!((d.items[0].tag, d.items[0].a, d.items[0].b), (Declare::KIND, 90, 24));
        assert_eq!((d.items[1].tag, d.items[1].a, d.items[1].b), (Declare::KIND, 91, 32));
        assert_ne!(d.items[0].c, 0);
        assert_eq!(d.items[0].c, d.items[1].c);
        let (row, instance) = d.provider_row_ptr().unwrap();
        assert!(core::ptr::eq(row, glue::row()));
        assert!(!instance.is_null());
        // `fini` stops the owner; outside init the declaration is refused
        (m.fini.unwrap())(core::ptr::null_mut());
        assert_eq!((m.init.unwrap())(core::ptr::null_mut()), dkrt_sdk::E_CONTEXT);
    }
}
