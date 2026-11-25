use crate::*;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Action {
        f: Option<ActFn>,
        ctx: Ctx,
        arg: c_int,
    },
    Submenu(usize),
    Text,
    Toggle {
        value: *mut c_int,
        f: Option<ActFn>,
        ctx: Ctx,
    },
    Value {
        value: *mut c_int,
        min: c_int,
        max: c_int,
        f: Option<ActFn>,
        ctx: Ctx,
    },
}

pub(crate) struct Row {
    pub text: String,
    pub kind: Kind,
}

pub(crate) struct Menu {
    pub desc: Option<MenuDesc>, // None for a list's side panel
    pub rows: Vec<Row>,
    pub checks: Vec<bool>,
    pub selected: usize,
}

pub(crate) fn handle(id: usize) -> *mut DkUiMenu {
    (id + 1) as *mut DkUiMenu
}

pub(crate) fn id_of(m: *const DkUiMenu) -> Option<usize> {
    let v = m as usize;
    (v != 0 && with(|s| v - 1 < s.menus.len())).then(|| v - 1)
}

pub(crate) fn desc_of(id: usize) -> Option<MenuDesc> {
    with(|s| s.menus[id].desc)
}

/// Clears the rows and calls the fill function.
pub(crate) fn refill(id: usize) {
    let Some(d) = desc_of(id) else { return };
    with(|s| s.menus[id].rows.clear());
    if let Some(f) = d.fill {
        f(handle(id), d.arg);
    }
}

pub(crate) fn open_id(id: usize) -> c_int {
    let Some(d) = desc_of(id) else { return -1 };
    let ok = with(|s| {
        s.has(CAP_MENU)
            && (d.flags & DK_UI_TEXT_PAGE == 0 || s.has(CAP_TEXT_PAGE))
            && (d.flags & (DK_UI_LIST | DK_UI_WIDE) == 0 || s.has(CAP_LIST))
    });
    if !ok {
        return -1;
    }
    refill(id);
    with(|s| {
        if !s.stack.contains(&id) {
            s.stack.push(id);
        }
    });
    0
}

pub(crate) fn add(m: *mut DkUiMenu, text: *const c_char, kind: Kind, what: &str) {
    let Some(id) = id_of(m) else { return };
    let list_text = matches!(kind, Kind::Text)
        && desc_of(id).is_some_and(|d| d.flags & (DK_UI_LIST | DK_UI_WIDE) != 0);
    let text = text_cut(
        text,
        if list_text {
            DK_UI_LIST_TEXT
        } else {
            DK_UI_ROW
        } - 1,
    );
    with(|s| {
        s.log(format!("{what} {text}"));
        s.menus[id].rows.push(Row { text, kind });
    });
}

/// # Safety
/// `d` is null or points at a valid descriptor.
#[no_mangle]
pub unsafe extern "C" fn dk_ui_menu_new(_ui: *mut c_void, d: *const MenuDesc) -> *mut DkUiMenu {
    if d.is_null() {
        return std::ptr::null_mut();
    }
    let d = unsafe { *d };
    with(|s| {
        let ok = s.has(CAP_MENU)
            && (d.flags & DK_UI_TEXT_PAGE == 0 || s.has(CAP_TEXT_PAGE))
            && (d.flags & (DK_UI_LIST | DK_UI_WIDE) == 0 || s.has(CAP_LIST));
        if !ok {
            return std::ptr::null_mut();
        }
        s.log(format!("menu_new {}", text_cut(d.name, 64)));
        s.menus.push(Menu {
            desc: Some(d),
            rows: vec![],
            checks: vec![],
            selected: 0,
        });
        handle(s.menus.len() - 1)
    })
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_open(m: *mut DkUiMenu) -> c_int {
    let Some(id) = id_of(m) else { return -1 };
    with(|s| s.log(format!("open {id}")));
    open_id(id)
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_close(m: *mut DkUiMenu) {
    if let Some(id) = id_of(m) {
        with(|s| {
            s.log(format!("close {id}"));
            s.stack.retain(|&x| x != id);
        });
    }
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_select(m: *mut DkUiMenu, row: c_int) {
    let Some(id) = id_of(m) else { return };
    if row >= 0 {
        with(|s| {
            s.log(format!("select {id} {row}"));
            s.menus[id].selected = row as usize;
        });
    }
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_is_open(m: *const DkUiMenu) -> c_int {
    id_of(m).is_some_and(|id| with(|s| s.stack.contains(&id))) as c_int
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_refresh(m: *mut DkUiMenu) {
    let Some(id) = id_of(m) else { return };
    with(|s| s.log(format!("refresh {id}")));
    refill(id);
    with(|s| {
        let n = s.menus[id].rows.len();
        s.menus[id].selected = s.menus[id].selected.min(n.saturating_sub(1));
    });
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_add_action(
    m: *mut DkUiMenu,
    text: *const c_char,
    f: Option<ActFn>,
    ctx: Ctx,
    arg: c_int,
) {
    add(m, text, Kind::Action { f, ctx, arg }, "action");
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_add_submenu(
    m: *mut DkUiMenu,
    text: *const c_char,
    sub: *mut DkUiMenu,
) {
    if let Some(sid) = id_of(sub) {
        add(m, text, Kind::Submenu(sid), "submenu");
    }
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_add_text(m: *mut DkUiMenu, text: *const c_char) {
    add(m, text, Kind::Text, "text");
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_add_toggle(
    m: *mut DkUiMenu,
    text: *const c_char,
    value: *mut c_int,
    f: Option<ActFn>,
    ctx: Ctx,
) {
    if cap_on(CAP_TOGGLE) {
        add(m, text, Kind::Toggle { value, f, ctx }, "toggle");
    }
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_add_value(
    m: *mut DkUiMenu,
    text: *const c_char,
    value: *mut c_int,
    min: c_int,
    max: c_int,
    f: Option<ActFn>,
    ctx: Ctx,
) {
    if cap_on(CAP_VALUE) {
        add(
            m,
            text,
            Kind::Value {
                value,
                min,
                max,
                f,
                ctx,
            },
            "value",
        );
    }
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_check(list: *mut DkUiMenu, row: c_int, on: c_int) {
    let Some(id) = id_of(list) else { return };
    if row < 0 {
        return;
    }
    with(|s| {
        s.log(format!("check {row} {on}"));
        let c = &mut s.menus[id].checks;
        if c.len() <= row as usize {
            c.resize(row as usize + 1, false);
        }
        c[row as usize] = on != 0;
    });
}

#[no_mangle]
pub extern "C" fn dk_ui_menu_checked(list: *const DkUiMenu, row: c_int) -> c_int {
    let Some(id) = id_of(list) else { return 0 };
    with(|s| {
        row >= 0
            && s.menus[id]
                .checks
                .get(row as usize)
                .copied()
                .unwrap_or(false)
    }) as c_int
}
