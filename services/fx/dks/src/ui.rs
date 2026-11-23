//! The FX ROUTING row of the SETTINGS list as UI SDK calls (`<dk/platform/ui.h>`): `Fx::start` asks for the row (`dk_fx_routing_attach`, in `init`), and three callbacks run the steps of
//! `dkrt_svc_fx::routing` in the UI task. `dk_ui_open`, `dk_ui_settings_row`, `dk_ui_choice` and `dk_ui_toast` are optional imports of core/ui (`requires` in sdk/sdk.toml): without core/ui the loader binds them
//! to the absent stub, so the file loads and has no row. A host build (the assembly test of the runtime runs the declarations of `service!`) has no SDK to link: the calls are stubs with the answer of a missing service.
use core::sync::atomic::{AtomicUsize, Ordering};
use dkrt_svc_fx::routing::{Event, Line, Step, Ui, LIST_MAX};
use dkrt_svc_rt::Post;

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

/// The UI handle of the row (0: none): a full queue answers through it while the owner is busy.
static UI: AtomicUsize = AtomicUsize::new(0);

/// The UI of the steps: core/ui's handle. A `Line` is NUL terminated and cut to what the popup shows, and the popup copies the text.
struct DkUi(*mut u8);

impl Ui for DkUi {
    fn choice(&mut self, title: &Line, items: &[Line], step: Step) -> bool {
        let ptrs: [*const u8; LIST_MAX] =
            core::array::from_fn(|i| items.get(i).map_or(core::ptr::null(), Line::as_ptr));
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

    fn toast(&mut self, text: &Line) {
        // SAFETY: the text is NUL terminated; the result changes nothing for the service
        unsafe {
            sys::dk_ui_toast(self.0, text.as_ptr());
        }
    }
}

/// What the runtime shows for a refused hand-over of a step: a full queue is a toast, never silence.
pub fn refusal(post: Post) -> Option<&'static str> {
    (post == Post::Full).then_some("BUSY: TRY AGAIN")
}

/// Hand `event` to the owner: it runs at once, or after the call of the service that is running (`Reentry`). A full queue says so.
pub fn submit(event: Event) -> Post {
    let post = crate::DK_SVC_OWNER.post(event);
    if let Some(text) = refusal(post) {
        DkUi(UI.load(Ordering::Relaxed) as *mut u8).toast(&Line::of(text));
    }
    post
}

/// The steps' UI for the owner's `Reentry::apply` (`routing::apply`).
#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_ui_choice(
    ui: usize,
    title: *const Line,
    items: *const Line,
    n: usize,
    step: u32,
) -> bool {
    let step = if step == 0 { Step::Effect } else { Step::Route };
    DkUi(ui as *mut u8).choice(&*title, core::slice::from_raw_parts(items, n), step)
}

#[cfg_attr(target_arch = "m68k", no_mangle)]
pub unsafe extern "C" fn dk_fx_ui_toast(ui: usize, text: *const Line) {
    DkUi(ui as *mut u8).toast(&*text)
}

unsafe extern "C" fn routing_open(_: *mut u8) {
    submit(Event::List);
}

unsafe extern "C" fn effect_done(_: *mut u8, index: i32) {
    submit(Event::Effect(index as u8));
}

unsafe extern "C" fn route_done(_: *mut u8, index: i32) {
    submit(Event::Route(index as u8));
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
        Line::of("FX ROUTING").as_ptr(),
        Some(routing_open),
        core::ptr::null_mut(),
    ) != 0
    {
        return 0;
    }
    UI.store(ui as usize, Ordering::Relaxed);
    ui as usize
}
