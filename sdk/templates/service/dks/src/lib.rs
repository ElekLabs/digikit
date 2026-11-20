//! The `__NAME__` service as a service file: the `service!` call and nothing else. The hook assembler is `hooks.rs` and the firmware calls are `fw.rs`, when the service has any.
#![no_std]

use dkrt_sdk::records::KINDS___UPPER__;
use dkrt_svc___NAME__::records::__CAMEL__;

dkrt_svc_rt::service!(__CAMEL__, "__NAME__", &KINDS___UPPER__);

// The long form takes the lists in this order. `setup` is required for every long form (the image half: it binds the adapter's tables); `hooks`, `fills` and
// `exports` are each optional from the right (give `fills = []` when there are exports but no fills). An entry with `dt2` or `dn2` is for that device. The
// `[[hook]]` and `[[fill]]` entries of module.toml must equal these lists (service-check; the loader refuses a hook or fill mismatch with E_DECL_KIND). `exports` is
// `exports = gen_exports::exports()` when the service has `provided` imports in sdk/sdk.toml (`patchkit gen` writes `dks/src/gen_exports.rs`; declare it as
// `#[rustfmt::skip] mod gen_exports;`), and a literal list is refused.
//
// mod hooks;   // every hook as assembler
// mod fw;      // firmware calls, and the exported functions
// mod image;   // `pub unsafe fn setup<C: CoreApi>(core: &C) -> i32`
//
// dkrt_svc_rt::service!(
//     __CAMEL__,
//     "__NAME__",
//     &KINDS___UPPER__,
//     setup = image::setup::<dkrt_svc_rt::ImportCore>,
//     hooks = [("cf.x.hook.y", hooks::x_hook_y, dt2)],
//     fills = [("x_slot", hooks::x_fill, dn2)],
//     exports = gen_exports::exports()
// );
