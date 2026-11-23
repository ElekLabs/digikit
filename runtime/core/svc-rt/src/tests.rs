use super::*;

// the imports a service file makes are the runtime's; this test binary links none, so each answers OK
mod stubs {
    #[no_mangle]
    extern "C" fn dk_service_declare_kind(_: *mut u8, _: u32, _: u32, _: *const u8) -> i32 {
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_hook(_: *mut u8, _: u32, _: *const u8) -> i32 {
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_slot(_: *mut u8, _: u32, _: u32, _: *const u8) -> i32 {
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_export(_: *mut u8, _: u32, _: *const u8) -> i32 {
        0
    }
    #[no_mangle]
    extern "C" fn dk_service_declare_row(
        _: *mut u8,
        _: *const dkrt_abi::service::ServiceRow,
    ) -> i32 {
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
