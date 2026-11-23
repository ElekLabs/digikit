//! The FX ROUTING row of the SETTINGS list as UI SDK calls (`<dk/platform/ui.h>`): `Fx::start` asks for the row (`dk_fx_routing_attach`, in `init`), and three callbacks run the steps of
//! `dkrt_svc_fx::routing` in the UI task. `dk_ui_open`, `dk_ui_settings_row`, `dk_ui_choice` and `dk_ui_toast` are optional imports of core/ui (`requires` in sdk/sdk.toml): without core/ui the loader binds them
//! to the absent stub, so the file loads and has no row. A host build (the assembly test of the runtime runs the declarations of `service!`) has no SDK to link: the calls are stubs with the answer of a missing service.
use dkrt_svc_fx::routing::{Routing, Step, Ui, LIST_MAX};
use dkrt_svc_rt::ImportCore;

/// Bytes of a text the popup shows, the terminator included (`DK_UI_ROW` and `DK_UI_TOAST` of <dk/platform/ui.h> are not longer).
const TEXT_MAX: usize = 32;

#[cfg(target_arch = "m68k")]
mod sys {
    use dkrt_sdk::api::{UiChoiceFn, UiSettingsOpenFn};
    extern "C" {
        pub fn dk_ui_open(ctx: *mut u8, out: *mut *mut u8) -> i32;
        pub fn dk_ui_settings_row(
            ui: *mut u8,
            text: *const u8,
            open: Option<UiSettingsOpenFn>,
            arg: *mut u8,
        ) -> i32;
        pub fn dk_ui_choice(
            ui: *mut u8,
            title: *const u8,
            items: *const u8,
            n: i32,
            done: Option<UiChoiceFn>,
            arg: *mut u8,
        ) -> i32;
        pub fn dk_ui_toast(ui: *mut u8, text: *const u8) -> i32;
    }
    // each import has the type the SDK declares
    const _: dkrt_sdk::api::DkUiOpenFn = dk_ui_open;
    const _: dkrt_sdk::api::DkUiSettingsRowFn = dk_ui_settings_row;
    const _: dkrt_sdk::api::DkUiToastFn = dk_ui_toast;
}

#[cfg(not(target_arch = "m68k"))]
mod sys {
    use dkrt_sdk::api::{UiChoiceFn, UiSettingsOpenFn};
    const MISSING: i32 = -73;
    pub unsafe fn dk_ui_open(_: *mut u8, _: *mut *mut u8) -> i32 {
        MISSING
    }
    pub unsafe fn dk_ui_settings_row(
        _: *mut u8,
        _: *const u8,
        _: Option<UiSettingsOpenFn>,
        _: *mut u8,
    ) -> i32 {
        MISSING
    }
    pub unsafe fn dk_ui_choice(
        _: *mut u8,
        _: *const u8,
        _: *const u8,
        _: i32,
        _: Option<UiChoiceFn>,
        _: *mut u8,
    ) -> i32 {
        MISSING
    }
    pub unsafe fn dk_ui_toast(_: *mut u8, _: *const u8) -> i32 {
        MISSING
    }
}

/// `text` as a NUL terminated copy, cut to what fits.
fn c_text(text: &str) -> [u8; TEXT_MAX] {
    let mut out = [0u8; TEXT_MAX];
    let n = text.len().min(TEXT_MAX - 1);
    out[..n].copy_from_slice(&text.as_bytes()[..n]);
    out
}

/// The UI of the steps: core/ui's handle.
struct DkUi(*mut u8);

impl Ui for DkUi {
    fn choice(&mut self, title: &str, items: &[&str], step: Step) -> bool {
        let title = c_text(title);
        // the popup copies the texts, so the buffers live for the call only
        let texts: [[u8; TEXT_MAX]; LIST_MAX] =
            core::array::from_fn(|i| c_text(items.get(i).copied().unwrap_or("")));
        let ptrs: [*const u8; LIST_MAX] = core::array::from_fn(|i| texts[i].as_ptr());
        let done: unsafe extern "C" fn(*mut u8, i32) = match step {
            Step::Effect => effect_done,
            Step::Route => route_done,
        };
        // SAFETY: the UI task; `ptrs` holds `items.len()` pointers to NUL terminated texts
        let r = unsafe {
            sys::dk_ui_choice(
                self.0,
                title.as_ptr(),
                ptrs.as_ptr().cast(),
                items.len().min(LIST_MAX) as i32,
                Some(done),
                core::ptr::null_mut(),
            )
        };
        r == 0
    }

    fn toast(&mut self, text: &str) {
        let text = c_text(text);
        // SAFETY: `text` is NUL terminated; the result changes nothing for the service
        unsafe {
            sys::dk_ui_toast(self.0, text.as_ptr());
        }
    }
}

/// A step of the owner's `Routing`; nothing without an owner (after `fini`).
fn step(run: impl FnOnce(&mut Routing, &mut DkUi)) {
    crate::DK_SVC_OWNER.with(|fx| {
        let (routing, ui) = fx.routing();
        run(routing, &mut DkUi(ui as *mut u8));
    });
}

unsafe extern "C" fn routing_open(_: *mut u8) {
    step(|routing, ui| routing.list(&ImportCore, ui));
}

unsafe extern "C" fn effect_done(_: *mut u8, index: i32) {
    step(|routing, ui| routing.pick_effect(&ImportCore, ui, index as usize));
}

unsafe extern "C" fn route_done(_: *mut u8, index: i32) {
    step(|routing, ui| routing.pick_route(&ImportCore, ui, index as usize));
}

/// The SETTINGS row, from `Fx::start` in `init`: core/ui's handle, or 0 when the image has no core/ui or the list is full (all rows of <dk/platform/ui.h> taken); fx then has no row.
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_routing_attach() -> usize {
    let mut ui: *mut u8 = core::ptr::null_mut();
    if sys::dk_ui_open(dkrt_svc_rt::init_ctx(), &mut ui) != 0 {
        return 0;
    }
    if sys::dk_ui_settings_row(
        ui,
        c_text("FX ROUTING").as_ptr(),
        Some(routing_open),
        core::ptr::null_mut(),
    ) != 0
    {
        return 0;
    }
    ui as usize
}
