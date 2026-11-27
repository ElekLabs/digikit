use crate::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{c_char, c_int, CStr};
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub(crate) struct Model {
    pub ticks: Vec<Tick>,
    pub now_ms: u64,
    pub menus: Vec<Menu>,
    pub stack: Vec<usize>,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<String>,
    pub log: Vec<String>,
    pub caps_off: HashSet<c_int>,
}

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::default());
}

/// Runs `f` on the model. Never call a C callback inside `f`: it would re-enter the model.
pub(crate) fn with<R>(f: impl FnOnce(&mut Model) -> R) -> R {
    MODEL.with(|m| f(&mut m.borrow_mut()))
}

pub(crate) static LOCK: Mutex<()> = Mutex::new(());

/// Takes the test lock and clears the model and the caps. Hold the result for the whole test.
pub fn session() -> MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with(|m| *m = Model::default());
    g
}

/// The log of `dk_ui_*` calls, oldest first.
pub fn call_log() -> Vec<String> {
    with(|m| m.log.clone())
}

impl Model {
    pub(crate) fn has(&self, cap: c_int) -> bool {
        !self.caps_off.contains(&cap)
    }
    pub(crate) fn log(&mut self, s: String) {
        self.log.push(s);
    }
}

pub(crate) fn text_cut(p: *const c_char, max_bytes: usize) -> String {
    if p.is_null() {
        return String::new();
    }
    let b = unsafe { CStr::from_ptr(p) }.to_bytes();
    String::from_utf8_lossy(&b[..b.len().min(max_bytes)]).into_owned()
}

pub(crate) fn row_text(p: *const c_char) -> String {
    text_cut(p, DK_UI_ROW - 1)
}
