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

/// The glue of a service file: `service!(Shortcuts, "shortcuts", &KINDS_SHORTCUTS, parts = [Records, Tick], hooks = [...], fills = [...], exports = ..., tick = f)` for an owner object
/// `Shortcuts: Provider<ImportCore>` that also implements the traits of the parts it names. `parts` lists the optional parts the object adopts (`Records`, `Dsp`, `Tick`, `Query`,
/// `Describe`; the lifecycle part is always there; `Reentry` has no table: it gives the owner an event queue, see `Reentry`). `kinds` (`&'static [(u32, u32)]`, as `dkrt_sdk::records::KINDS_*`)
/// need the `Records` part. The macro emits the part tables, the row, the cell `DK_SVC_OWNER` (`Owner<Shortcuts>`: `init` makes the instance with `Provider::start` before the declarations,
/// `fini` stops it), the module record `dk_module`, the data word `dk_svc_abi` and (on the ColdFire) the panic handler; `Query` also declares the resident query entry and points it at the part.
///
/// After `parts`: `hooks = [...]`, `fills = [...]`, `patches = [...]`, `exports = gen_exports::exports()` (the list `patchkit gen` writes for a service with `provided` imports in `sdk/sdk.toml`:
/// `[(name hash, entry); N]`) or, only in tests (`patchkit service-check` refuses it in a service crate), `exports = [("dk_param_get", fw::dk_param_get), ...]` (name, function: declared with the
/// hash of the name) and `tick = f` (`unsafe extern "C" fn(*mut u8)`, the file's `tick`: once per UI pass, with the module context; only after `exports`), each optional list in this order.
/// `patches = [("name", 4, f), ...]` lists the patches of the service (`[[patch]]` of its module.toml): the name of the patch, its length (1, 2 or 4, the length of the site in the port file) and
/// `f`, a `fn(&Service) -> u32` that gives the bytes (the low `len` of the value) from the instance `Provider::start` made, so a table address or a count computed in `start` goes in.
/// The runtime writes them at the site when the service loads and the stock bytes back before `stop`; an optional device name as for a hook limits the entry. Only after
/// `fills`, before `exports`.
/// `parts = [..], hooks = [..], fills = [..]` with no `exports` is the form of a service with no `provided` imports (it declares no export). The macro also emits `dk_svc_declare(ctx, device)`: the
/// declarations of `init` (the host test of the assembly runs it and compares the result with the header table).
///
/// The symbols are `no_mangle` on the ColdFire only: a host test links the service crates of all files into one binary.
#[macro_export]
macro_rules! service {
    // ---- the provider form: `parts = [...]`
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [], fills = [], patches = [], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [], exports = $crate::service!(@exports $($export)*), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = [$($export:tt)*], tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [], exports = $crate::service!(@exports $($export)*), tick = Some($tick as $crate::TickFn));
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [], exports = $exports, tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], exports = $exports:expr, tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [], exports = $exports, tick = Some($tick as $crate::TickFn));
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], patches = [$($patch:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [$($patch)*], exports = $crate::service!(@exports), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], patches = [$($patch:tt)*], exports = [$($export:tt)*] $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [$($patch)*], exports = $crate::service!(@exports $($export)*), tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], patches = [$($patch:tt)*], exports = [$($export:tt)*], tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [$($patch)*], exports = $crate::service!(@exports $($export)*), tick = Some($tick as $crate::TickFn));
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], patches = [$($patch:tt)*], exports = $exports:expr $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [$($patch)*], exports = $exports, tick = None);
    };
    ($svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),* $(,)?], hooks = [$($hook:tt)*], fills = [$($fill:tt)*], patches = [$($patch:tt)*], exports = $exports:expr, tick = $tick:path $(,)?) => {
        $crate::service!(@provider $svc, $name, $kinds, parts = [$($part),*], hooks = [$($hook)*], fills = [$($fill)*], patches = [$($patch)*], exports = $exports, tick = Some($tick as $crate::TickFn));
    };
    (@provider $svc:ty, $name:literal, $kinds:expr, parts = [$($part:ident),*],
     hooks = [$(($site:literal, $entry:path $(, $dev:ident)?)),* $(,)?],
     fills = [$(($slot:literal, $fill:path $(, $fdev:ident)?)),* $(,)?],
     patches = [$(($pname:literal, $plen:literal, $pfn:path $(, $pdev:ident)?)),* $(,)?],
     exports = $exports:expr, tick = $tick:expr) => {
        const DK_SVC_KINDS: &[(u32, u32)] = $kinds;
        const DK_SVC_PARTS_ADOPTED: &[&str] = &[$(stringify!($part)),*];
        const _: () = assert!(
            $crate::parts_adopt(DK_SVC_PARTS_ADOPTED, "Records") || DK_SVC_KINDS.is_empty(),
            "a service with record kinds adopts the Records part"
        );

        static DK_SVC_LIFECYCLE: $crate::dkrt_abi::provider::LifecycleV1 =
            $crate::dkrt_abi::provider::LifecycleV1::of_in::<$svc, $crate::ImportCore, DkSvcEnter>();
        $($crate::service!(@part_table $part, $svc, DK_SVC_KINDS);)*

        static DK_SVC_PARTS: &[$crate::dkrt_abi::provider::PartRef] = &$crate::service!(@part_refs [
            $crate::dkrt_abi::provider::PartRef::new(
                $crate::dkrt_abi::provider::PART_LIFECYCLE,
                $crate::dkrt_abi::provider::LIFECYCLE_NEWEST,
                &DK_SVC_LIFECYCLE as *const _ as *const (),
            ),
        ] [$($part),*]);
        static DK_SVC_ROW: $crate::dkrt_abi::provider::ProviderRow =
            $crate::dkrt_abi::provider::ProviderRow::new($crate::name_hash($name), DK_SVC_PARTS);

        $crate::service!(@owner_events $svc; [$($part),*]);

        /// The owner object: made by `start` in `init`, taken by `stop` in `fini`. The file's hooks reach it with `DK_SVC_OWNER.with(|s| ...)`; every call into it,
        /// the part functions of the tables too, runs under its busy state (`Owner`), and with the `Reentry` part a call that arrives meanwhile is queued with
        /// `DK_SVC_OWNER.post(event)`.
        pub static DK_SVC_OWNER: $crate::service!(@owner_ty $svc; [$($part),*]) = $crate::service!(@owner_new $svc; [$($part),*]);

        /// The way the tables of this file reach the object: through the owner's busy state, so a part function never runs inside another call into the object.
        struct DkSvcEnter;
        impl $crate::dkrt_abi::provider::Enter<$svc> for DkSvcEnter {
            unsafe fn begin(_inst: *mut core::ffi::c_void) -> *mut $svc {
                DK_SVC_OWNER.enter()
            }
            unsafe fn end(_inst: *mut core::ffi::c_void) {
                DK_SVC_OWNER.leave()
            }
        }

        $crate::service!(@resident_query $svc, $($part),*);

        /// The declarations of `init`, over the instance now in `DK_SVC_OWNER` (null before `start`); `device` gives the `[digikit] id` of the device the entries with a
        /// filter are compared with.
        ///
        /// # Safety
        /// `ctx` is the module context of a collection; the entries are addresses of the file.
        pub unsafe fn dk_svc_declare(ctx: *mut u8, device: fn() -> u32) -> i32 {
            #[allow(unused_mut)]
            let mut result = $crate::declare_provider(
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
            // the patches, one call each: the values come from the instance `start` made
            $(
                if result == 0 {
                    result = $crate::service!(@patch $pname, $plen, $pfn, $crate::hook_device!($($pdev)?), ctx, device);
                }
            )*
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
    // the part list of the row: the lifecycle entry and one entry for each adopted part with a table; `Reentry` has none (it only gives the owner its event queue)
    (@part_refs [$($acc:expr,)*] []) => {
        [$($acc),*]
    };
    (@part_refs [$($acc:expr,)*] [Reentry $(, $rest:ident)*]) => {
        $crate::service!(@part_refs [$($acc,)*] [$($rest),*])
    };
    (@part_refs [$($acc:expr,)*] [$p:ident $(, $rest:ident)*]) => {
        $crate::service!(@part_refs [$($acc,)* $crate::service!(@part_ref $p),] [$($rest),*])
    };
    // the type of the owner cell and its constructor: `Reentry` among the parts gives it the event queue of the service
    (@owner_ty $svc:ty; []) => { $crate::Owner<$svc> };
    (@owner_ty $svc:ty; [Reentry $(, $rest:ident)*]) => {
        $crate::Owner<$svc, DkSvcEvents, { <$svc as $crate::Reentry<$crate::ImportCore>>::QUEUE }>
    };
    (@owner_ty $svc:ty; [$other:ident $(, $rest:ident)*]) => { $crate::service!(@owner_ty $svc; [$($rest),*]) };
    (@owner_new $svc:ty; []) => { $crate::Owner::new() };
    (@owner_new $svc:ty; [Reentry $(, $rest:ident)*]) => { $crate::Owner::new() };
    // the events of the owner: only with `Reentry`, a zero-size type that gives the owner the part's `apply`
    (@owner_events $svc:ty; []) => {};
    (@owner_events $svc:ty; [Reentry $(, $rest:ident)*]) => {
        /// The events of this file's owner: the `Reentry` part of the service.
        pub struct DkSvcEvents;
        impl $crate::Events<$svc> for DkSvcEvents {
            type Event = <$svc as $crate::Reentry<$crate::ImportCore>>::Event;
            fn apply(svc: &mut $svc, event: Self::Event) {
                <$svc as $crate::Reentry<$crate::ImportCore>>::apply(svc, &$crate::ImportCore, event)
            }
        }
    };
    (@owner_events $svc:ty; [$other:ident $(, $rest:ident)*]) => { $crate::service!(@owner_events $svc; [$($rest),*]); };
    (@owner_new $svc:ty; [$other:ident $(, $rest:ident)*]) => { $crate::service!(@owner_new $svc; [$($rest),*]) };
    // the table of one adopted part (an item) and its entry in the part list (an expression)
    (@part_table Reentry, $svc:ty, $kinds:expr) => {};
    (@part_table Records, $svc:ty, $kinds:expr) => {
        static DK_SVC_RECORDS: $crate::dkrt_abi::provider::RecordsV1 =
            $crate::dkrt_abi::provider::RecordsV1::of_in::<$svc, $crate::ImportCore, DkSvcEnter>($kinds);
    };
    (@part_table Dsp, $svc:ty, $kinds:expr) => {
        static DK_SVC_DSP: $crate::dkrt_abi::provider::DspV1 =
            $crate::dkrt_abi::provider::DspV1::of_in::<$svc, $crate::ImportCore, DkSvcEnter>();
    };
    (@part_table Tick, $svc:ty, $kinds:expr) => {
        static DK_SVC_TICK: $crate::dkrt_abi::provider::TickV1 =
            $crate::dkrt_abi::provider::TickV1::of_in::<$svc, $crate::ImportCore, DkSvcEnter>();
    };
    (@part_table Query, $svc:ty, $kinds:expr) => {
        static DK_SVC_QUERY: $crate::dkrt_abi::provider::QueryV1 =
            $crate::dkrt_abi::provider::QueryV1::of_in::<$svc, DkSvcEnter>();
    };
    (@part_table Describe, $svc:ty, $kinds:expr) => {
        static DK_SVC_DESCRIBE: $crate::dkrt_abi::provider::DescribeV1 =
            $crate::dkrt_abi::provider::DescribeV1::of_in::<$svc, $crate::ImportCore, DkSvcEnter>();
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
    // one patch: the name hash of the patch site, the bytes `$pfn` computes from the started instance (`fn(&$svc) -> u32`, the low `$plen` bytes: 1, 2 or 4) and the device filter. Before
    // `start` (the assembly test replays the declarations without an instance) the value is 0 with the declared length.
    (@patch $pname:literal, $plen:literal, $pfn:path, $only:expr, $ctx:expr, $device:expr) => {{
        const _: () = assert!($plen == 1 || $plen == 2 || $plen == 4, "a patch is 1, 2 or 4 bytes");
        let at = DK_SVC_OWNER.instance();
        // SAFETY: the instance made by `start`, alive until `fini`; `init` runs on the one task that makes and drops it
        let value = if at.is_null() { 0 } else { $pfn(unsafe { &*at }) };
        unsafe { $crate::declare_patch($ctx, $device, $crate::name_hash($pname), $crate::Patch::new(value, $plen), $only) }
    }};
    (@exports $(($ename:literal, $efn:path)),* $(,)?) => {
        [$(($crate::name_hash($ename), $efn as usize as *const u8)),*]
    };
}
