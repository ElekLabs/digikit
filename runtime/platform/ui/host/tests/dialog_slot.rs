//! The open dialog's slot of core/ui: one dialog at a time, an answer frees the slot before it runs, and an unload of the owner takes the answer function away.
use dkui_host::session;
use std::ffi::c_int;

extern "C" {
    fn ds_reset();
    fn ds_begin(serial: u32, owner: usize, tag: c_int) -> c_int;
    fn ds_begin_no_done(serial: u32, owner: usize) -> c_int;
    fn ds_is_open() -> c_int;
    fn ds_detach(serial: u32);
    fn ds_answer(answer: i32) -> c_int;
    fn ds_drop_owner(owner: usize) -> c_int;
    fn ds_calls(tag: c_int) -> c_int;
    fn ds_last_answer(tag: c_int) -> i32;
}

#[test]
fn a_dialog_answers_once_and_frees_the_slot() {
    let _g = session();
    unsafe {
        ds_reset();
        assert_eq!(ds_begin(1, 3, 0), 0);
        assert_eq!(ds_is_open(), 1);
        assert_eq!(ds_begin(2, 3, 1), -1, "one dialog at a time");
        assert_eq!(ds_answer(7), 1);
        assert_eq!((ds_calls(0), ds_last_answer(0)), (1, 7));
        assert_eq!(ds_is_open(), 0, "the answer freed the slot");
        assert_eq!(ds_answer(8), 0, "a second answer finds no dialog");
        assert_eq!(ds_calls(0), 1);
        assert_eq!(ds_begin(3, 3, 1), 0, "free again");
    }
}

#[test]
fn closing_without_an_answer_frees_the_slot_for_the_right_serial_only() {
    let _g = session();
    unsafe {
        ds_reset();
        assert_eq!(ds_begin(5, 3, 0), 0);
        ds_detach(4);
        assert_eq!(ds_is_open(), 1, "another window's detach changes nothing");
        ds_detach(5);
        assert_eq!(ds_is_open(), 0);
        assert_eq!(ds_calls(0), 0, "NO calls nothing");
        assert_eq!(ds_begin_no_done(0, 3), -1, "serial 0 means none");
    }
}

#[test]
fn an_unload_of_the_owner_while_its_dialog_is_open_drops_its_answer_function() {
    let _g = session();
    unsafe {
        ds_reset();
        assert_eq!(ds_begin(1, 3, 0), 0);
        assert_eq!(
            ds_drop_owner(4),
            0,
            "another owner's unload leaves the dialog alone"
        );
        assert_eq!(ds_drop_owner(3), 1);
        assert_eq!(ds_is_open(), 1, "the window stays until it closes");
        assert_eq!(ds_begin(2, 5, 1), -1, "and still blocks a new dialog");
        assert_eq!(ds_answer(1), 0, "its answer calls nothing");
        assert_eq!(ds_calls(0), 0, "the unloaded module's code is not called");
        assert_eq!(ds_is_open(), 0);
        assert_eq!(ds_drop_owner(3), 0, "nothing open: nothing to drop");
    }
}
