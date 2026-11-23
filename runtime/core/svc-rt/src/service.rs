//! The `service!` macro: the glue of a service file (the module record, the ABI word and the panic handler).

/// The device filter of one hook entry of `service!`: none (every device), `dt2` or `dn2`.
#[doc(hidden)]
#[macro_export]
macro_rules! hook_device {
    () => {
        0
    };
    (dt2) => {
        $crate::DEVICE_DT2
    };
    (dn2) => {
        $crate::DEVICE_DN2
    };
}

/// The service file's glue: `service!(Lfo, "lfo", KINDS_LFO)` for `impl<C: CoreApi> Service<C> for Lfo`, the name of its `[[service]]` and the record kinds
/// it owns (`&'static [(u32, u32)]`, as `dkrt_sdk::records::KINDS_*`). It emits `dk_module`, `dk_svc_abi` and (on the ColdFire) the panic handler.
///
/// After the kinds: `setup = f`, `hooks = [...]`, `fills = [...]`, `exports = gen_exports::exports()` (the list `patchkit gen` writes for a service with `provided` imports in `sdk/sdk.toml`:
/// `[(name hash, entry); N]`) or, only in tests (`patchkit service-check` refuses it in a service crate), `exports = [("dk_param_get", fw::dk_param_get), ...]` (name, function: declared with the hash of the name) and `tick = f` (`unsafe extern "C" fn(*mut u8)`, the file's `tick`: once per UI pass, with the module context; only after `exports`), each optional list in this order. Every form also emits `dk_svc_declare(ctx, device)`: the declarations
/// of `init` without `setup` (the host test of the assembly runs it and compares the result with the header table). Both exports forms
/// accept `query = dk_service_query_v1` after `exports` for the typed resident query entry declared in the manifest.
///
/// The symbols are `no_mangle` on the ColdFire only: a host test links the service crates of all three files into one binary.
#[macro_export]
macro_rules! service {
    ($svc:ty, $name:literal, $kinds:expr) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = None, hooks = [], fills = [], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*]) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*], fills = [$($fill:tt)*]) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*] $(, query = $query:path)?) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports $($export)*), tick = None $(, query = $query)?);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*] $(, query = $query:path)?, tick = $tick:path) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports $($export)*), tick = Some($tick as $crate::TickFn) $(, query = $query)?);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr $(, query = $query:path)?) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [$($fill)*], exports = $exports, tick = None $(, query = $query)?);
    };
    ($svc:ty, $name:literal, $kinds:expr, setup = $setup:expr, hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr $(, query = $query:path)?, tick = $tick:path) => {
        $crate::service!(@emit $svc, $name, $kinds, setup = Some($setup), hooks = [$($hook)*], fills = [$($fill)*], exports = $exports, tick = Some($tick as $crate::TickFn) $(, query = $query)?);
    };
    (@emit $svc:ty, $name:literal, $kinds:expr, setup = $setup:expr,
     hooks = [$(($site:literal, $entry:path $(, $dev:ident)?)),* $(,)?],
     fills = [$(($slot:literal, $fill:path $(, $fdev:ident)?)),* $(,)?],
     exports = $exports:expr, tick = $tick:expr $(, query = $query:path)?) => {
        static DK_SVC_ROW: $crate::dkrt_abi::service::ServiceRow =
            $crate::dkrt_abi::service::ServiceRow::of::<$svc, $crate::ImportCore>($name, $kinds);

        /// The declarations of `init` (without `setup`); `device` gives the `[digikit] id` of the device the entries with a filter are compared with.
        ///
        /// # Safety
        /// `ctx` is the module context of a collection; the entries are addresses of the file.
        pub unsafe fn dk_svc_declare(ctx: *mut u8, device: fn() -> u32) -> i32 {
            let result = $crate::declare(ctx, &DK_SVC_ROW, dk_svc_init as usize as *const u8, device, &$crate::service!(@hooks $(($site, $entry $(, $dev)?)),*), &$crate::service!(@hooks $(($slot, $fill $(, $fdev)?)),*), &$exports);
            $(
                if result == 0 {
                    return $crate::declare_resident_query(ctx, $query);
                }
            )?
            result
        }

        unsafe extern "C" fn dk_svc_init(ctx: *mut u8) -> i32 {
            $crate::set_init_ctx(ctx);
            let setup: Option<unsafe fn(&$crate::ImportCore) -> i32> = $setup;
            let r = match setup {
                Some(f) => f(&$crate::ImportCore),
                None => 0,
            };
            if r != 0 {
                return r;
            }
            dk_svc_declare(ctx, $crate::running_device)
        }

        #[cfg_attr(target_arch = "m68k", no_mangle)]
        pub static dk_module: $crate::ModuleDef = $crate::ModuleDef {
            init: Some(dk_svc_init),
            fini: None,
            tick: $tick,
            menu: None,
        };

        /// The ABI fingerprint the packer copies to header word 0x3c (`dkrt_abi::service::SVC_ABI`).
        #[cfg_attr(target_arch = "m68k", no_mangle)]
        pub static dk_svc_abi: u32 = $crate::dkrt_abi::service::SVC_ABI;

        #[cfg(target_arch = "m68k")]
        #[panic_handler]
        fn dk_svc_panic(_: &core::panic::PanicInfo) -> ! {
            unsafe { $crate::sys::dkc_panic() }
        }
    };
    (@hooks $(($site:literal, $entry:path $(, $dev:ident)?)),*) => {
        [$((
            $crate::name_hash($site),
            $entry as usize as *const u8,
            $crate::hook_device!($($dev)?),
        )),*]
    };
    (@exports $(($ename:literal, $efn:path)),* $(,)?) => {
        [$(($crate::name_hash($ename), $efn as usize as *const u8)),*]
    };
}
