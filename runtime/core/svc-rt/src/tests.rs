use super::*;

// the imports a service file makes are the runtime's; this test binary links none, so each answers OK
mod stubs {
    /// How many hooks, slot fills and exports were declared, in that order.
    pub static COUNTS: [core::sync::atomic::AtomicUsize; 3] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; 3];
    #[no_mangle]
    extern "C" fn dk_service_declare_kind(_: *mut u8, _: u32, _: u32, _: *const u8) -> i32 {
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_hook(_: *mut u8, _: u32, _: *const u8) -> i32 {
        COUNTS[0].fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_slot(_: *mut u8, _: u32, _: u32, _: *const u8) -> i32 {
        COUNTS[1].fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_export(_: *mut u8, _: u32, _: *const u8) -> i32 {
        COUNTS[2].fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_row(
        _: *mut u8,
        _: *const dkrt_abi::service::ServiceRow,
    ) -> i32 {
        0
    }
    /// The row and the instance of the last `dk_provider_declare_row` (addresses), and how many times it was called.
    pub static DECLARED: [core::sync::atomic::AtomicUsize; 3] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; 3];
    #[no_mangle]
    extern "C" fn dk_provider_declare_row(
        _: *mut u8,
        row: *const dkrt_abi::provider::ProviderRow,
        instance: *mut u8,
    ) -> i32 {
        use core::sync::atomic::Ordering::SeqCst;
        DECLARED[0].store(row as usize, SeqCst);
        DECLARED[1].store(instance as usize, SeqCst);
        DECLARED[2].fetch_add(1, SeqCst);
        0
    }
    #[no_mangle]
    extern "C" fn dkc_device_id() -> u32 {
        0
    }
}

// a file with `tick =` has the entry in the record and the context `init` was called with; one without has a zero word
mod ticked {
    pub struct S;
    impl<C: dkrt_abi::core_api::CoreApi> dkrt_abi::service::Service<C> for S {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }
    pub static TICKS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    pub unsafe extern "C" fn tick(ctx: *mut u8) {
        TICKS.fetch_add(ctx as usize, core::sync::atomic::Ordering::Relaxed);
    }
    crate::service!(
        S,
        "ticked",
        &[],
        setup = setup,
        hooks = [],
        fills = [],
        exports = [],
        tick = tick
    );
    unsafe fn setup(_: &crate::ImportCore) -> i32 {
        assert_eq!(crate::init_ctx() as usize, 0x1234);
        0
    }
    pub unsafe fn init(ctx: *mut u8) -> i32 {
        dk_svc_init(ctx)
    }
}
mod unticked {
    pub struct S;
    impl<C: dkrt_abi::core_api::CoreApi> dkrt_abi::service::Service<C> for S {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }
    crate::service!(S, "unticked", &[]);
}

#[test]
fn tick_puts_the_entry_in_the_module_record_and_the_context_reaches_setup() {
    let t = ticked::dk_module.tick.expect("tick = f sets the entry");
    unsafe { t(5 as *mut u8) };
    assert_eq!(ticked::TICKS.load(core::sync::atomic::Ordering::Relaxed), 5);
    assert!(unticked::dk_module.tick.is_none());
    // `init` stores the context before `setup` runs (the assert in `setup`)
    assert_eq!(unsafe { ticked::init(0x1234 as *mut u8) }, 0);
    assert_eq!(init_ctx() as usize, 0x1234);
}

#[test]
fn the_module_record_is_four_words_and_a_missing_entry_is_zero() {
    // the packer reads `dk_module` as four words (init, fini, tick, menu) and takes a zero word for no entry
    assert_eq!(
        core::mem::size_of::<ModuleDef>(),
        4 * core::mem::size_of::<usize>()
    );
    let none = ModuleDef {
        init: None,
        fini: None,
        tick: None,
        menu: None,
    };
    let words: [usize; 4] = unsafe { core::mem::transmute(none) };
    assert_eq!(words, [0; 4]);
}

/// A provider file: an owner object that counts its ticks and adopts the Tick and Query parts; its hook reaches the instance through `DK_SVC_OWNER`.
mod provided {
    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use dkrt_abi::provider::{Provider, Query, Status, Ticker};

    pub static STARTS: AtomicU32 = AtomicU32::new(0);
    pub static STOPS: AtomicU32 = AtomicU32::new(0);

    pub struct S {
        pub id: u32,
        pub ticks: u32,
    }
    impl Provider<crate::ImportCore> for S {
        fn start(_: &crate::ImportCore) -> Result<S, Status> {
            Ok(S {
                id: 100 + STARTS.fetch_add(1, SeqCst),
                ticks: 0,
            })
        }
        fn stop(&mut self, _: &crate::ImportCore) {
            STOPS.fetch_add(1, SeqCst);
        }
    }
    impl Ticker<crate::ImportCore> for S {
        fn tick(&mut self, _: &crate::ImportCore, _: u32) {
            self.ticks += 1;
        }
    }
    impl Query for S {
        fn query(&self, op: u32, _: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
            reply[0] = (self.id + op) as u8;
            Ok(1)
        }
    }
    /// What a hook of the file does: reads the instance `start` made.
    pub fn hook() -> Option<(u32, u32)> {
        DK_SVC_OWNER.with(|s| (s.id, s.ticks))
    }
    crate::service!(
        S,
        "provided",
        &[],
        parts = [Tick, Query],
        hooks = [],
        fills = [],
        exports = []
    );
    pub unsafe fn init(ctx: *mut u8) -> i32 {
        dk_svc_init(ctx)
    }
    pub unsafe fn fini(ctx: *mut u8) {
        (dk_module.fini.unwrap())(ctx)
    }
    pub fn row() -> &'static dkrt_abi::provider::ProviderRow {
        &DK_SVC_ROW
    }
}

fn whole() -> core::ops::Range<usize> {
    0..usize::MAX
}

#[test]
fn the_macro_emits_exactly_the_adopted_parts_and_the_lifecycle() {
    use dkrt_abi::provider::*;
    // SAFETY: the row and its tables are statics of this binary.
    let row = unsafe { check(provided::row(), whole()) }.unwrap();
    assert_eq!(row.name(), name_hash("provided"));
    for (tag, newest, adopted) in [
        (PART_LIFECYCLE, LIFECYCLE_NEWEST, true),
        (PART_TICK, TICK_NEWEST, true),
        (PART_QUERY, QUERY_NEWEST, true),
        (PART_RECORDS, RECORDS_NEWEST, false),
        (PART_DSP, DSP_NEWEST, false),
        (PART_DESCRIBE, DESCRIBE_NEWEST, false),
    ] {
        let found = matches!(row.part(tag, newest), PartLookup::Found { .. });
        assert_eq!(found, adopted, "part {tag}");
    }
    // a file that adopts no optional part has the lifecycle part alone
    assert!(provided::dk_module.fini.is_some());
    assert!(ticked::dk_module.fini.is_none(), "the old form has no fini");
}

#[test]
fn init_starts_the_instance_before_the_row_and_a_hook_and_the_parts_reach_that_instance() {
    use core::sync::atomic::Ordering::SeqCst;
    use dkrt_abi::provider::ProviderHandle;
    assert_eq!(provided::hook(), None, "nothing before start");
    let before = stubs::DECLARED[2].load(SeqCst);
    assert_eq!(unsafe { provided::init(0x1234 as *mut u8) }, 0);
    assert_eq!(init_ctx() as usize, 0x1234);
    assert_eq!(stubs::DECLARED[2].load(SeqCst), before + 1);
    let (row, instance) = (
        stubs::DECLARED[0].load(SeqCst) as *const dkrt_abi::provider::ProviderRow,
        stubs::DECLARED[1].load(SeqCst) as *mut u8,
    );
    assert!(core::ptr::eq(row, provided::row()));
    let (id, ticks) = provided::hook().expect("the hook finds the instance start made");
    assert!((100..).contains(&id) && ticks == 0);

    // the runtime resolves the declared row with the declared instance and calls the parts through it
    let handle =
        unsafe { ProviderHandle::resolve(&*row, whole(), instance as *mut core::ffi::c_void) }
            .unwrap();
    let core = &ImportCore as *const ImportCore as *const core::ffi::c_void;
    unsafe {
        handle.tick(core, 0);
        handle.tick(core, 1);
    }
    assert_eq!(
        provided::hook(),
        Some((id, 2)),
        "the part functions work on the instance the hook sees"
    );
    let mut reply = [0u8; 4];
    assert_eq!(unsafe { handle.query(5, &[], &mut reply) }, 1);
    assert_eq!(reply[0], (id + 5) as u8);

    // a second init is refused and makes no second instance
    let starts = provided::STARTS.load(SeqCst);
    assert_eq!(
        unsafe { provided::init(0x1234 as *mut u8) },
        dkrt_sdk::E_LOADED
    );
    assert_eq!(provided::STARTS.load(SeqCst), starts);

    // fini stops the instance once
    let stops = provided::STOPS.load(SeqCst);
    unsafe { provided::fini(0x1234 as *mut u8) };
    assert_eq!(provided::STOPS.load(SeqCst), stops + 1);
    assert_eq!(provided::hook(), None, "nothing after stop");
    unsafe { provided::fini(0x1234 as *mut u8) };
    assert_eq!(
        provided::STOPS.load(SeqCst),
        stops + 1,
        "a second fini stops nothing"
    );
}

/// A provider file with hooks and fills and no `exports =`: the form of a service with no provided imports.
mod exportless {
    use dkrt_abi::provider::{Provider, Status};
    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start(_: &crate::ImportCore) -> Result<S, Status> {
            Ok(S)
        }
    }
    pub fn hook() {}
    pub fn fill() {}
    crate::service!(
        S,
        "exportless",
        &[],
        parts = [],
        hooks = [("cf.x.hook.a", hook), ("cf.x.hook.b", hook)],
        fills = [("x_slot", fill)]
    );
    pub fn init(ctx: *mut u8) -> i32 {
        unsafe { dk_svc_init(ctx) }
    }
}

#[test]
fn the_provider_form_without_exports_declares_the_hooks_and_fills_and_no_export() {
    use core::sync::atomic::Ordering::SeqCst;
    let before = [0, 1, 2].map(|i| stubs::COUNTS[i].load(SeqCst));
    assert_eq!(exportless::init(0x1234 as *mut u8), 0);
    let after = [0, 1, 2].map(|i| stubs::COUNTS[i].load(SeqCst));
    assert_eq!(after[0] - before[0], 2, "hooks as declared");
    assert_eq!(after[1] - before[1], 1, "fills as declared");
    assert_eq!(after[2] - before[2], 0, "no export table");
}

/// A provider file whose pre-tick step posts an event to the owner, as a foreign call that fires a callback does.
mod pretick {
    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use dkrt_abi::provider::{Provider, Reentry, Status, Ticker};

    /// The order the owner saw: 1 for a pre-tick event, 2 for the tick.
    pub static ORDER: AtomicU32 = AtomicU32::new(0);

    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start(_: &crate::ImportCore) -> Result<S, Status> {
            Ok(S)
        }
    }
    impl Reentry<crate::ImportCore> for S {
        type Event = u32;
        const QUEUE: usize = 1;
        fn apply(&mut self, _: &crate::ImportCore, event: u32) {
            ORDER.store(ORDER.load(SeqCst) * 10 + event, SeqCst);
        }
    }
    impl Ticker<crate::ImportCore> for S {
        fn tick(&mut self, _: &crate::ImportCore, _: u32) {
            ORDER.store(ORDER.load(SeqCst) * 10 + 2, SeqCst);
        }
        fn pre_tick(_: &crate::ImportCore, _: u32) {
            // the callback of the foreign call: with the owner idle the event runs at once
            assert_eq!(DK_SVC_OWNER.post(1), crate::Post::Ran);
        }
    }
    crate::service!(
        S,
        "pretick",
        &[],
        parts = [Tick, Reentry],
        hooks = [],
        fills = [],
        exports = []
    );
    pub unsafe fn start() {
        assert_eq!(dk_svc_init(0x1234 as *mut u8), 0);
    }
    pub fn row() -> &'static dkrt_abi::provider::ProviderRow {
        &DK_SVC_ROW
    }
}

#[test]
fn the_pre_tick_step_runs_before_the_owner_is_entered_so_its_events_come_before_the_tick() {
    use core::sync::atomic::Ordering::SeqCst;
    use dkrt_abi::provider::ProviderHandle;
    unsafe { pretick::start() };
    let handle = unsafe {
        ProviderHandle::resolve(pretick::row(), whole(), 0x1234 as *mut core::ffi::c_void)
    }
    .unwrap();
    unsafe {
        handle.tick(
            &ImportCore as *const ImportCore as *const core::ffi::c_void,
            0,
        )
    };
    assert_eq!(pretick::ORDER.load(SeqCst), 12, "the event, then the tick");
    assert_eq!(pretick::DK_SVC_OWNER.refused(), 0);
}
