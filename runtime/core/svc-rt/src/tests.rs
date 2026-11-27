// device-literals: allow (the `service!` device filter is spelled with the device name, and the test checks it)
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
    /// The patches declared, in order: (site, value, length).
    pub static PATCHES: std::sync::Mutex<std::vec::Vec<(u32, u32, u32)>> =
        std::sync::Mutex::new(std::vec::Vec::new());
    #[no_mangle]
    extern "C" fn dk_service_declare_patch(_: *mut u8, site: u32, value: u32, len: u32) -> i32 {
        PATCHES.lock().unwrap().push((site, value, len));
        0
    }
    #[no_mangle]
    extern "C" fn dkc_device_id() -> u32 {
        0
    }
}

// a file with `tick =` has the entry in the record and the context `init` was called with; one without has a zero word
mod ticked {
    use dkrt_abi::provider::{Provider, Status};

    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            assert_eq!(crate::init_ctx() as usize, 0x1234);
            slot.write(S);
            Ok(())
        }
    }
    pub static TICKS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    pub unsafe extern "C" fn tick(ctx: *mut u8) {
        TICKS.fetch_add(ctx as usize, core::sync::atomic::Ordering::Relaxed);
    }
    crate::service!(
        S,
        "ticked",
        &[],
        parts = [],
        hooks = [],
        fills = [],
        exports = [],
        tick = tick
    );
    pub unsafe fn init(ctx: *mut u8) -> i32 {
        dk_svc_init(ctx)
    }
}
mod unticked {
    use dkrt_abi::provider::{Provider, Status};

    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S);
            Ok(())
        }
    }
    crate::service!(S, "unticked", &[], parts = []);
}

#[test]
fn tick_puts_the_entry_in_the_module_record_and_the_context_reaches_start() {
    let t = ticked::dk_module.tick.expect("tick = f sets the entry");
    unsafe { t(5 as *mut u8) };
    assert_eq!(ticked::TICKS.load(core::sync::atomic::Ordering::Relaxed), 5);
    assert!(unticked::dk_module.tick.is_none());
    // `init` stores the context before `start` runs (the assert in `start`)
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
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S {
                id: 100 + STARTS.fetch_add(1, SeqCst),
                ticks: 0,
            });
            Ok(())
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

/// A service with patches: the values come from the instance `start` made, the length is the entry's, an entry with a device name is declared only on that device.
mod patched {
    use dkrt_abi::provider::{Provider, Status};

    pub struct S {
        pub table: u32,
    }
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S { table: 0x4100_2000 });
            Ok(())
        }
    }
    impl S {
        fn table(&self) -> u32 {
            self.table
        }
        fn count(&self) -> u32 {
            self.table >> 12
        }
    }
    crate::service!(
        S,
        "patched",
        &[],
        parts = [],
        hooks = [],
        fills = [],
        patches = [
            ("cf.test.patch.table", 4, S::table),
            ("cf.test.patch.count", 2, S::count),
            ("cf.test.patch.only_dt2", 1, S::count, dt2)
        ]
    );
    static DEVICE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    fn device() -> u32 {
        DEVICE.load(core::sync::atomic::Ordering::SeqCst)
    }
    pub fn declare(id: u32) -> i32 {
        DEVICE.store(id, core::sync::atomic::Ordering::SeqCst);
        let _ = dk_svc_start();
        let r = unsafe { dk_svc_declare(core::ptr::null_mut(), device) };
        dk_svc_stop();
        r
    }
}

#[test]
fn the_patches_of_service_are_declared_with_the_values_start_computed_and_the_device_filter() {
    use dkrt_abi::profiles::{DN2, DT2};
    let h = |n: &str| name_hash(n);
    let take = || std::mem::take(&mut *stubs::PATCHES.lock().unwrap());
    take();
    assert_eq!(patched::declare(DT2.id), 0);
    assert_eq!(
        take(),
        [
            (h("cf.test.patch.table"), 0x4100_2000, 4),
            (h("cf.test.patch.count"), 0x4100_2000 >> 12, 2),
            (h("cf.test.patch.only_dt2"), 0x4100_2000 >> 12, 1)
        ]
    );
    assert_eq!(patched::declare(DN2.id), 0);
    assert_eq!(
        take().iter().map(|p| p.0).collect::<std::vec::Vec<_>>(),
        [h("cf.test.patch.table"), h("cf.test.patch.count")],
        "the dt2 entry is not declared on dn2"
    );
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
    // every file stops its owner in `fini`, whatever parts it adopts
    assert!(provided::dk_module.fini.is_some());
    assert!(ticked::dk_module.fini.is_some());
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
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S);
            Ok(())
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
    /// What the pre-tick step's `post` answered last (`Post` as a number: 0 ran, 3 no owner).
    pub static POSTED: AtomicU32 = AtomicU32::new(99);

    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S);
            Ok(())
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
            let post = DK_SVC_OWNER.post(1);
            POSTED.store(post as u32, SeqCst);
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
    pub fn stop() {
        dk_svc_stop();
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
    assert_eq!(pretick::POSTED.load(SeqCst), crate::Post::Ran as u32);
    assert_eq!(pretick::DK_SVC_OWNER.refused(), 0);
    // after `stop` the table's tick still runs the pre-tick step (it is outside the guard): its event meets no owner and is dropped, and the tick itself is refused
    pretick::stop();
    pretick::ORDER.store(0, SeqCst);
    unsafe {
        handle.tick(
            &ImportCore as *const ImportCore as *const core::ffi::c_void,
            0,
        )
    };
    assert_eq!(pretick::POSTED.load(SeqCst), crate::Post::NoOwner as u32);
    assert_eq!(pretick::ORDER.load(SeqCst), 0, "no event, no tick");
    assert_eq!(
        pretick::DK_SVC_OWNER.refused(),
        0,
        "a dead owner counts nothing"
    );
}

/// A provider file that adopts the Describe part: its words come from a `Shared` cell outside the owner, as a service that answers the MODULES reply from another task does.
mod described {
    use crate::Shared;
    use dkrt_abi::provider::{ConsumerId, Describe, Provider, Status, WordSink};

    pub static PUBLISHED: Shared<[u32; 2]> = Shared::new([0; 2]);

    pub struct S;
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S);
            Ok(())
        }
    }
    impl Describe<crate::ImportCore> for S {
        fn describe(core: &crate::ImportCore, who: ConsumerId, out: &mut WordSink) {
            let [a, b] = PUBLISHED.with(core, |w| *w);
            if who.get() == 1 {
                out.push(a);
                out.push(b);
            }
        }
    }
    crate::service!(
        S,
        "described",
        &[],
        parts = [Describe],
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
fn describe_answers_while_the_owner_is_busy_and_while_there_is_none() {
    use dkrt_abi::provider::ProviderHandle;
    let handle = unsafe {
        ProviderHandle::resolve(described::row(), whole(), 0x1234 as *mut core::ffi::c_void)
    }
    .unwrap();
    assert!(handle.has_describe());
    let core = &ImportCore as *const ImportCore as *const core::ffi::c_void;
    described::PUBLISHED.with(&ImportCore, |w| *w = [7, 9]);
    let mut words = [0u32; 4];
    // no owner yet: the words are still there (they are not the owner's)
    assert_eq!(unsafe { handle.describe(core, 1, &mut words) }, 2);
    assert_eq!(words[..2], [7, 9]);
    unsafe { described::start() };
    // inside a call of the owner (the UI task busy in a tick, a window or a hook) the table's other parts refuse; describe answers
    let busy = described::DK_SVC_OWNER.with(|_| {
        let mut w = [0u32; 4];
        (unsafe { handle.describe(core, 1, &mut w) }, [w[0], w[1]])
    });
    assert_eq!(busy, Some((2, [7, 9])));
    assert_eq!(
        described::DK_SVC_OWNER.refused(),
        0,
        "it never asked the owner"
    );
    // a consumer the service holds nothing for: no word
    assert_eq!(unsafe { handle.describe(core, 2, &mut words) }, 0);
    // a short buffer drops the words that do not fit
    let mut one = [0u32; 1];
    assert_eq!(unsafe { handle.describe(core, 1, &mut one) }, 1);
}

/// A provider file that adopts the State part: the `service!` arm builds the table and the handle carries bytes into the owner behind its guard.
mod stateful {
    use dkrt_abi::provider::{Provider, State, Status};

    pub struct S {
        pub n: u8,
    }
    impl Provider<crate::ImportCore> for S {
        fn start_in(
            _: &crate::ImportCore,
            slot: &mut core::mem::MaybeUninit<S>,
        ) -> Result<(), Status> {
            slot.write(S { n: 1 });
            Ok(())
        }
    }
    impl State<crate::ImportCore> for S {
        const VERSION: u16 = 2;
        const ACCEPTS_FROM: u16 = 1;
        fn export_max(&self, _: &crate::ImportCore) -> usize {
            1
        }
        fn export(&mut self, _: &crate::ImportCore, out: &mut [u8]) -> usize {
            out[0] = self.n;
            1
        }
        fn import(&mut self, _: &crate::ImportCore, from: Option<&[u8]>) -> Result<(), Status> {
            self.n = from.map_or(0, |b| b[0]);
            Ok(())
        }
    }
    crate::service!(
        S,
        "stateful",
        &[],
        parts = [State],
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
fn the_state_part_exports_and_imports_through_the_owner() {
    use dkrt_abi::provider::ProviderHandle;
    let handle = unsafe {
        ProviderHandle::resolve(stateful::row(), whole(), 0x1234 as *mut core::ffi::c_void)
    }
    .unwrap();
    assert_eq!(handle.state_versions(), Some((2, 1)));
    let core = &ImportCore as *const ImportCore as *const core::ffi::c_void;
    // no owner yet: nothing to export, and an import is refused as busy
    assert_eq!(unsafe { handle.state_export_max(core) }, 0);
    assert_eq!(unsafe { handle.state_import(core, None) }, dkrt_sdk::E_BUSY);
    unsafe { stateful::start() };
    let mut out = [0u8; 1];
    assert_eq!(unsafe { handle.state_export_max(core) }, 1);
    assert_eq!(unsafe { handle.state_export(core, &mut out) }, 1);
    assert_eq!(out, [1]);
    assert_eq!(unsafe { handle.state_import(core, Some((1, &[9]))) }, 0);
    unsafe { handle.state_export(core, &mut out) };
    assert_eq!(out, [9]);
    // a version the service does not take: it decides with None
    unsafe { handle.state_import(core, Some((3, &[5]))) };
    unsafe { handle.state_export(core, &mut out) };
    assert_eq!(out, [0]);
}
