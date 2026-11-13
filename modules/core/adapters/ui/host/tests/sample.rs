use dkui_host::driver::*;
use dkui_host::*;
use std::ffi::c_int;

extern "C" {
    fn s_init() -> *mut DkUiMenu;
    fn s_main() -> *mut DkUiMenu;
    fn s_files() -> *mut DkUiMenu;
    fn s_empty() -> *mut DkUiMenu;
    fn s_applied() -> c_int;
    fn s_last_arg() -> c_int;
    fn s_remote() -> c_int;
    fn s_level() -> c_int;
    fn s_level_changes() -> c_int;
    fn s_remote_changes() -> c_int;
    fn s_check_row() -> c_int;
    fn s_check_on() -> c_int;
    fn s_panel_row() -> c_int;
    fn s_panel_action() -> c_int;
    fn s_saved() -> c_int;
    fn s_discarded() -> c_int;
    fn s_confirm_answer() -> c_int;
    fn s_picked() -> c_int;
    fn s_rc_confirm() -> c_int;
    fn s_rc_choice() -> c_int;
    fn s_rc_toast() -> c_int;
    fn s_rc_open_list() -> c_int;
    fn s_back_calls() -> c_int;
    fn s_is_open_files() -> c_int;
    fn s_files_checked(row: c_int) -> c_int;
    fn s_set_remote(v: c_int);
    fn s_refresh_main();
}

fn rows(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A fresh session with the main menu open.
fn start() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    assert!(!unsafe { s_init() }.is_null());
    assert_eq!(open(unsafe { s_main() }), 0);
    g
}

/// Opens TOOLS when the main menu is on top. Returns the index of the row named `name`.
fn tools_row(name: &str) -> usize {
    if title() == "SAMPLE" {
        press_yes(1);
    }
    ["INFO", "FILES", "EMPTY", "ERASE", "PICK", "NOTE"]
        .iter()
        .position(|n| *n == name)
        .unwrap()
}

#[test]
fn main_menu_renders_and_acts() {
    let _g = start();
    assert_eq!(title(), "SAMPLE");
    assert_eq!(
        render(),
        rows(&["APPLY", "TOOLS", "REMOTE OFF", "LEVEL 000", "APPLIED 0"])
    );
    press_yes(0);
    assert_eq!((unsafe { s_applied() }, unsafe { s_last_arg() }), (1, 42));
    // The action refreshed the menu; the copied text shows the new count.
    assert_eq!(render()[4], "APPLIED 1");
}

#[test]
fn toggle_flips_and_reports_the_new_value() {
    let _g = start();
    press_yes(2);
    assert_eq!(render()[2], "REMOTE ON");
    assert_eq!(
        (unsafe { s_remote() }, unsafe { s_remote_changes() }),
        (1, 1)
    );
    press_yes(2);
    assert_eq!(render()[2], "REMOTE OFF");
    assert_eq!(unsafe { s_remote_changes() }, 2);
    // A change made by the module shows after a refresh.
    unsafe { s_set_remote(1) };
    unsafe { s_refresh_main() };
    assert_eq!(render()[2], "REMOTE ON");
}

#[test]
fn value_row_clamps_to_its_range() {
    let _g = start();
    turn(3, 5);
    assert_eq!(render()[3], "LEVEL 005");
    turn(3, 500);
    assert_eq!(
        (render()[3].clone(), unsafe { s_level() }),
        ("LEVEL 127".into(), 127)
    );
    let n = unsafe { s_level_changes() };
    turn(3, 1); // already at max: nothing changed
    assert_eq!(unsafe { s_level_changes() }, n);
    turn(3, -1000);
    assert_eq!(render()[3], "LEVEL 000");
}

#[test]
fn submenu_text_page_and_back() {
    let _g = start();
    press_yes(1);
    assert_eq!((depth(), title().as_str()), (2, "TOOLS"));
    press_yes(0);
    assert_eq!(title(), "INFO");
    // Text is cut to DK_UI_ROW - 1 bytes.
    assert_eq!(
        render(),
        rows(&["DIGIKIT MENU", "THIS LINE IS MUCH TOO LO"[..23].as_ref()])
    );
    press_no();
    press_no();
    assert_eq!((depth(), title().as_str()), (1, "SAMPLE"));
}

#[test]
fn list_checks_panel_and_empty_text() {
    let _g = start();
    let r = tools_row("FILES");
    press_yes(r);
    assert_eq!(title(), "FILES");
    assert_eq!(render(), rows(&["  ALPHA", "  BETA", "  GAMMA"]));
    press_yes(1);
    assert_eq!((unsafe { s_check_row() }, unsafe { s_check_on() }), (1, 1));
    assert_eq!(render(), rows(&["  ALPHA", "* BETA", "  GAMMA"]));
    assert_eq!(unsafe { s_files_checked(1) }, 1);
    press_yes(1);
    assert_eq!(
        (unsafe { s_check_on() }, unsafe { s_files_checked(1) }),
        (0, 0)
    );

    press_right(2);
    assert_eq!(render(), rows(&["RENAME", "DELETE"]));
    choose_panel_row(1);
    assert_eq!(
        (unsafe { s_panel_row() }, unsafe { s_panel_action() }),
        (2, 1)
    );
    assert_eq!(title(), "FILES"); // the panel closed
                                  // A new panel is built on each RIGHT.
    press_right(0);
    choose_panel_row(0);
    assert_eq!(
        (unsafe { s_panel_row() }, unsafe { s_panel_action() }),
        (0, 0)
    );

    // The empty list shows its text.
    press_no();
    assert_eq!(open(unsafe { s_empty() }), 0);
    assert_eq!(render(), rows(&["NOTHING HERE"]));
}

#[test]
fn list_back_asks_only_when_something_changed() {
    let _g = start();
    // No change: NO closes at once.
    let r = tools_row("FILES");
    press_yes(r);
    press_no();
    assert_eq!(
        (unsafe { s_back_calls() }, unsafe { s_is_open_files() }),
        (1, 0)
    );

    // A change: NO opens a confirm and the list stays.
    let r = tools_row("FILES");
    assert_eq!(depth(), 2);
    press_yes(r);
    press_yes(0);
    press_no();
    assert_eq!(unsafe { s_is_open_files() }, 1);
    let (kind, title, text, _) = dialog().unwrap();
    assert_eq!(
        (kind, title.as_str(), text.as_str()),
        (DialogKind::Confirm, "SAVE?", "Keep the changes?")
    );
    answer_confirm(true);
    assert_eq!((unsafe { s_saved() }, unsafe { s_discarded() }), (1, 0));
    assert_eq!(unsafe { s_is_open_files() }, 0);
    assert!(dialog().is_none());

    // NO on the dialog discards.
    let r = tools_row("FILES");
    press_yes(r);
    press_yes(2);
    press_no();
    press_no(); // NO answers the confirm
    assert_eq!(unsafe { s_confirm_answer() }, 0);
    assert_eq!((unsafe { s_saved() }, unsafe { s_discarded() }), (1, 1));
    assert_eq!(unsafe { s_is_open_files() }, 0);
}

#[test]
fn confirm_choice_and_toast() {
    let _g = start();
    let r = tools_row("ERASE");
    press_yes(r);
    assert_eq!(unsafe { s_rc_confirm() }, 0);
    press_no();
    assert_eq!(unsafe { s_confirm_answer() }, 0);
    press_yes(r);
    answer_confirm(true);
    assert_eq!(unsafe { s_confirm_answer() }, 1);

    press_yes(tools_row("PICK"));
    let (kind, title, _, items) = dialog().unwrap();
    assert_eq!((kind, title.as_str()), (DialogKind::Choice, "PICK ONE"));
    assert_eq!(items, rows(&["ONE", "TWO", "THREE"]));
    press_no(); // closes without done
    assert!(dialog().is_none());
    assert_eq!(unsafe { s_picked() }, -1);
    press_yes(tools_row("PICK"));
    pick_choice(2);
    assert_eq!(unsafe { s_picked() }, 2);

    press_yes(tools_row("NOTE"));
    assert_eq!(
        toasts(),
        rows(&["SAVED 1234567890123456789012345678"[..32].as_ref()])
    );
    assert_eq!(unsafe { s_rc_toast() }, 0);
}

#[test]
fn missing_capabilities_do_nothing_and_return_minus_one() {
    let _g = session();
    for cap in [CAP_CONFIRM, CAP_CHOICE, CAP_TOAST] {
        set_cap(cap, false);
        assert!(!cap_on(cap));
    }
    assert!(!unsafe { s_init() }.is_null());
    assert_eq!(open(unsafe { s_main() }), 0);
    for (name, rc) in [
        ("ERASE", s_rc_confirm as unsafe extern "C" fn() -> c_int),
        ("PICK", s_rc_choice),
        ("NOTE", s_rc_toast),
    ] {
        let r = tools_row(name);
        press_yes(r);
        assert_eq!(unsafe { rc() }, -1, "{name}");
        press_no();
    }
    assert!(dialog().is_none());
    assert!(toasts().is_empty());
}

#[test]
fn missing_toggle_and_value_are_left_out() {
    let _g = session();
    set_cap(CAP_TOGGLE, false);
    set_cap(CAP_VALUE, false);
    unsafe { s_init() };
    open(unsafe { s_main() });
    assert_eq!(render(), rows(&["APPLY", "TOOLS", "APPLIED 0"]));
}

#[test]
fn missing_list_capability_refuses_the_list() {
    let _g = session();
    set_cap(CAP_LIST, false);
    unsafe { s_init() };
    assert!(unsafe { s_files() }.is_null());
    open(unsafe { s_main() });
    let r = tools_row("FILES");
    press_yes(r);
    assert_eq!(unsafe { s_rc_open_list() }, -1);
    assert_eq!(title(), "TOOLS");
}

#[test]
fn missing_menu_capability_gives_no_menus() {
    let _g = session();
    set_cap(CAP_MENU, false);
    assert!(unsafe { s_init() }.is_null());
    assert_eq!(open(unsafe { s_main() }), -1);
    assert_eq!(depth(), 0);
}
