use crate::*;

pub const DK_UI_TEXT_PAGE: u32 = 1;

pub const DK_UI_LIST: u32 = 2;

pub const DK_UI_WIDE: u32 = 4;

/// Opaque `dk_ui_menu *`: the menu index plus one. Never dereferenced.
#[repr(C)]
pub struct DkUiMenu {
    pub(crate) _p: [u8; 0],
}

pub(crate) type Ctx = *mut c_void;

pub(crate) type FillFn = extern "C" fn(*mut DkUiMenu, Ctx);

pub(crate) type ActFn = extern "C" fn(Ctx, c_int);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MenuDesc {
    pub name: *const c_char,
    pub title: *const c_char,
    pub flags: u32,
    pub fill: Option<FillFn>,
    pub arg: Ctx,
    pub empty: *const c_char,
    pub panel: Option<extern "C" fn(*mut DkUiMenu, c_int, Ctx)>,
    pub check: Option<extern "C" fn(Ctx, c_int, c_int)>,
    pub back: Option<extern "C" fn(Ctx) -> c_int>,
}
