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

/// The glue of a service file that provides a `ProviderRow` (RC3): `service!(Shortcuts, "shortcuts", &KINDS_SHORTCUTS, parts = [Records, Tick], hooks = [...], fills = [...],
/// exports = ..., tick = f)` for an owner object `Shortcuts: Provider<ImportCore>` that also implements the traits of the parts it names. `parts` lists the optional parts the
/// object adopts (`Records`, `Dsp`, `Tick`, `Query`, `Describe`; the lifecycle part is always there); the later lists are those of the old form, all optional but in the same
/// order. `parts = [..], hooks = [..], fills = [..]` with no `exports` is the form of a service with no `provided` imports (it declares no export). The macro emits the part tables, the row, the cell `DK_SVC_OWNER` (`Owner<Shortcuts>`: `init` makes the instance with `Provider::start` before the declarations, `fini`
/// stops it) and the module record; `Query` also declares the resident query entry and points it at the part. `kinds` need the `Records` part.
///
/// The old form: `service!(Lfo, "lfo", KINDS_LFO)` for `impl<C: CoreApi> Service<C> for Lfo`, the name of its `[[service]]` and the record kinds
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
    // ---- the provider form: `parts = [...]`
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [], fills = [], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports $($export)*), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*], tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], exports = $crate::service!(@exports $($export)*), tick = Some($tick as $crate::TickFn));
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], exports = $exports, tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr, tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], exports = $exports, tick = Some($tick as $crate::TickFn));
    };
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
    (@provider $svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),*],
     hooks = [$(($site:literal, $entry:path $(, $dev:ident)?)),* $(,)?],
     fills = [$(($slot:literal, $fill:path $(, $fdev:ident)?)),* $(,)?],
     exports = $exports:expr, tick = $tick:expr) => {
        const DK_SVC_KINDS: &[(u32, u32)] = $kinds;
        const DK_SVC_PARTS_ADOPTED: &[&str] = &[$(stringify!($part)),*];
        const _: () = assert!(
            $crate::parts_adopt(DK_SVC_PARTS_ADOPTED, "Records") || DK_SVC_KINDS.is_empty(),
            "a service with record kinds adopts the Records part"
        );

        static DK_SVC_LIFECYCLE: $crate::dkrt_abi::provider::LifecycleV1 =
            $crate::dkrt_abi::provider::LifecycleV1::of::<$svc, $crate::ImportCore>();
        $($crate::service!(@part_table $part, $svc, DK_SVC_KINDS);)*

        static DK_SVC_PARTS: &[$crate::dkrt_abi::provider::PartRef] = &[
            $crate::dkrt_abi::provider::PartRef::new(
                $crate::dkrt_abi::provider::PART_LIFECYCLE,
                $crate::dkrt_abi::provider::LIFECYCLE_NEWEST,
                &DK_SVC_LIFECYCLE as *const _ as *const (),
            ),
            $($crate::service!(@part_ref $part)),*
        ];
        static DK_SVC_ROW: $crate::dkrt_abi::provider::ProviderRow =
            $crate::dkrt_abi::provider::ProviderRow::new($crate::name_hash($name), DK_SVC_PARTS);

        /// The owner object: made by `start` in `init`, taken by `stop` in `fini`. The file's hooks reach it with `DK_SVC_OWNER.with(|s| ...)`.
        pub static DK_SVC_OWNER: $crate::Owner<$svc> = $crate::Owner::new();

        $crate::service!(@resident_query $svc, $($part),*);

        /// The declarations of `init`, over the instance now in `DK_SVC_OWNER` (null before `start`); `device` gives the `[digikit] id` of the device the entries with a
        /// filter are compared with.
        ///
        /// # Safety
        /// `ctx` is the module context of a collection; the entries are addresses of the file.
        pub unsafe fn dk_svc_declare(ctx: *mut u8, device: fn() -> u32) -> i32 {
            let result = $crate::declare_provider(
                ctx,
                DK_SVC_KINDS,
                &DK_SVC_ROW,
                DK_SVC_OWNER.instance() as *mut u8,
                dk_svc_init as usize as *const u8,
                device,
                &$crate::service!(@hooks $(($site, $entry $(, $dev)?)),*),
                &$crate::service!(@hooks $(($slot, $fill $(, $fdev)?)),*),
                &$exports,
            );
            if result == 0 {
                return $crate::service!(@declare_query ctx, $($part),*);
            }
            result
        }

        /// The file's row, for a host test that declares it itself (the runtime's assembly tests replay the declarations of the packed header and cannot run the file's `init`).
        pub fn dk_svc_row() -> &'static $crate::dkrt_abi::provider::ProviderRow {
            &DK_SVC_ROW
        }

        /// What `init` does before the declarations: make the instance (`Provider::start`) and keep it in `DK_SVC_OWNER`. `E_LOADED` when there is one.
        pub fn dk_svc_start() -> Result<*mut u8, i32> {
            DK_SVC_OWNER
                .start(|slot| <$svc as $crate::dkrt_abi::provider::Provider<$crate::ImportCore>>::start_in(&$crate::ImportCore, slot))
                .map(|instance| instance as *mut u8)
        }

        /// What `fini` does: `Provider::stop` on the instance, once.
        pub fn dk_svc_stop() {
            DK_SVC_OWNER.stop(|instance| {
                <$svc as $crate::dkrt_abi::provider::Provider<$crate::ImportCore>>::stop(instance, &$crate::ImportCore)
            });
        }

        unsafe extern "C" fn dk_svc_init(ctx: *mut u8) -> i32 {
            $crate::set_init_ctx(ctx);
            if let Err(status) = dk_svc_start() {
                return status;
            }
            let result = dk_svc_declare(ctx, $crate::running_device);
            if result != 0 {
                // the declarations are refused: the runtime keeps no row, so nothing calls the instance again
                dk_svc_stop();
            }
            result
        }

        unsafe extern "C" fn dk_svc_fini(_: *mut u8) {
            dk_svc_stop();
        }

        #[cfg_attr(target_arch = "m68k", no_mangle)]
        pub static dk_module: $crate::ModuleDef = $crate::ModuleDef {
            init: Some(dk_svc_init),
            fini: Some(dk_svc_fini),
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
    // the table of one adopted part (an item) and its entry in the part list (an expression)
    (@part_table Records, $svc:ty, $kinds:expr) => {
        static DK_SVC_RECORDS: $crate::dkrt_abi::provider::RecordsV1 =
            $crate::dkrt_abi::provider::RecordsV1::of::<$svc, $crate::ImportCore>($kinds);
    };
    (@part_table Dsp, $svc:ty, $kinds:expr) => {
        static DK_SVC_DSP: $crate::dkrt_abi::provider::DspV1 =
            $crate::dkrt_abi::provider::DspV1::of::<$svc, $crate::ImportCore>();
    };
    (@part_table Tick, $svc:ty, $kinds:expr) => {
        static DK_SVC_TICK: $crate::dkrt_abi::provider::TickV1 =
            $crate::dkrt_abi::provider::TickV1::of::<$svc, $crate::ImportCore>();
    };
    (@part_table Query, $svc:ty, $kinds:expr) => {
        static DK_SVC_QUERY: $crate::dkrt_abi::provider::QueryV1 =
            $crate::dkrt_abi::provider::QueryV1::of::<$svc>();
    };
    (@part_table Describe, $svc:ty, $kinds:expr) => {
        static DK_SVC_DESCRIBE: $crate::dkrt_abi::provider::DescribeV1 =
            $crate::dkrt_abi::provider::DescribeV1::of::<$svc, $crate::ImportCore>();
    };
    (@part_ref Records) => {
        $crate::dkrt_abi::provider::PartRef::new(
            $crate::dkrt_abi::provider::PART_RECORDS,
            $crate::dkrt_abi::provider::RECORDS_NEWEST,
            &DK_SVC_RECORDS as *const _ as *const (),
        )
    };
    (@part_ref Dsp) => {
        $crate::dkrt_abi::provider::PartRef::new(
            $crate::dkrt_abi::provider::PART_DSP,
            $crate::dkrt_abi::provider::DSP_NEWEST,
            &DK_SVC_DSP as *const _ as *const (),
        )
    };
    (@part_ref Tick) => {
        $crate::dkrt_abi::provider::PartRef::new(
            $crate::dkrt_abi::provider::PART_TICK,
            $crate::dkrt_abi::provider::TICK_NEWEST,
            &DK_SVC_TICK as *const _ as *const (),
        )
    };
    (@part_ref Query) => {
        $crate::dkrt_abi::provider::PartRef::new(
            $crate::dkrt_abi::provider::PART_QUERY,
            $crate::dkrt_abi::provider::QUERY_NEWEST,
            &DK_SVC_QUERY as *const _ as *const (),
        )
    };
    (@part_ref Describe) => {
        $crate::dkrt_abi::provider::PartRef::new(
            $crate::dkrt_abi::provider::PART_DESCRIBE,
            $crate::dkrt_abi::provider::DESCRIBE_NEWEST,
            &DK_SVC_DESCRIBE as *const _ as *const (),
        )
    };
    // the resident query entry: only with the Query part. It calls the part's function with the instance of `DK_SVC_OWNER`.
    (@resident_query $svc:ty, $($part:ident),*) => {
        $crate::service!(@resident_query_of [$($part),*] $svc);
    };
    (@resident_query_of [] $svc:ty) => {};
    (@resident_query_of [Query $(, $rest:ident)*] $svc:ty) => {
        #[cfg_attr(target_arch = "m68k", no_mangle)]
        pub unsafe extern "C" fn dk_service_query_v1(op: u32, req: *const u8, req_len: u32, reply: *mut u8, reply_len: u32) -> i32 {
            let instance = DK_SVC_OWNER.instance();
            if instance.is_null() {
                return $crate::dkrt_sdk::E_MISSING;
            }
            (DK_SVC_QUERY.query)(instance as *mut core::ffi::c_void, op, req, req_len, reply, reply_len)
        }
    };
    (@resident_query_of [$other:ident $(, $rest:ident)*] $svc:ty) => {
        $crate::service!(@resident_query_of [$($rest),*] $svc);
    };
    (@declare_query $ctx:expr, ) => { 0 };
    (@declare_query $ctx:expr, Query $(, $rest:ident)*) => {
        $crate::declare_resident_query($ctx, dk_service_query_v1)
    };
    (@declare_query $ctx:expr, $other:ident $(, $rest:ident)*) => {
        $crate::service!(@declare_query $ctx, $($rest),*)
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
