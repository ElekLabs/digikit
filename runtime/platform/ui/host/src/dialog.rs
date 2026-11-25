use crate::*;
use std::ffi::{c_char, c_int, c_void};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogKind {
    Confirm,
    Choice,
}

pub(crate) struct Dialog {
    pub kind: DialogKind,
    pub title: String,
    pub text: String,
    pub items: Vec<String>,
    pub confirm_done: Option<extern "C" fn(Ctx, c_int)>,
    pub ctx: Ctx,
}

#[no_mangle]
pub extern "C" fn dk_ui_confirm(
    _ui: *mut c_void,
    title: *const c_char,
    text: *const c_char,
    done: Option<extern "C" fn(Ctx, c_int)>,
    ctx: Ctx,
) -> c_int {
    with(|s| {
        if !s.has(CAP_CONFIRM) || s.dialog.is_some() {
            return -1;
        }
        let (title, text) = (text_cut(title, 80), text_cut(text, 80));
        s.log(format!("confirm {title}"));
        s.dialog = Some(Dialog {
            kind: DialogKind::Confirm,
            title,
            text,
            items: vec![],
            confirm_done: done,
            ctx,
        });
        0
    })
}

/// An item keeps a tab as it is: the real service turns "NAME\tREST" into pixel padding (`choice_item` of ui.c, `<dk/platform/ui_text.h>`), which the fake does not model.
///
/// # Safety
/// `items` points at `n` valid C strings.
#[no_mangle]
pub unsafe extern "C" fn dk_ui_choice(
    _ui: *mut c_void,
    title: *const c_char,
    items: *const *const c_char,
    n: c_int,
    done: Option<extern "C" fn(Ctx, c_int)>,
    ctx: Ctx,
) -> c_int {
    with(|s| {
        if !s.has(CAP_CHOICE) || s.dialog.is_some() || n <= 0 || items.is_null() {
            return -1;
        }
        let items = (0..n as usize)
            .map(|i| text_cut(unsafe { *items.add(i) }, DK_UI_LIST_TEXT - 1))
            .collect();
        let title = row_text(title);
        s.log(format!("choice {title}"));
        s.dialog = Some(Dialog {
            kind: DialogKind::Choice,
            title,
            text: String::new(),
            items,
            confirm_done: done,
            ctx,
        });
        0
    })
}
