//! The key-release queue of core/ui: actions of a key press callback wait for the release of that key.
use dkui_host::session;
use std::ffi::c_int;

extern "C" {
    fn df_reset();
    fn df_add_in_key(owner: usize, arg: u32, code: u32) -> i32;
    fn df_active_in_key() -> c_int;
    fn df_released(code: u32) -> c_int;
    fn df_set_down(on: c_int);
    fn df_set_start_fail(on: c_int);
    fn df_poll();
    fn df_drop(owner: usize) -> u32;
    fn df_cancel(arg: u32) -> c_int;
    fn df_pending() -> u32;
    fn df_ran(arg: u32) -> c_int;
    fn df_order(i: c_int) -> u32;
    fn df_starts() -> c_int;
}

const E_NOMEM: i32 = -8;

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe { df_reset() };
    g
}

#[test]
fn only_a_press_callback_is_active_and_an_action_runs_after_the_release_of_its_key() {
    let _g = fresh();
    unsafe {
        assert_eq!(df_active_in_key(), 1);
        assert_eq!(df_add_in_key(1, 0, 10), 0);
        assert_eq!(df_starts(), 1, "the poll is asked to run");
        df_set_down(1);
        df_poll();
        assert_eq!(df_ran(0), 0, "the key is down");
        assert_eq!(df_released(11), 0, "the release of another key is not ours");
        assert_eq!(df_released(10), 1);
        df_poll();
        assert_eq!((df_ran(0), df_pending()), (1, 0));
        df_poll();
        assert_eq!(df_ran(0), 1, "once");
    }
}

#[test]
fn actions_run_in_the_order_queued_and_each_waits_for_its_own_key() {
    let _g = fresh();
    unsafe {
        df_add_in_key(1, 0, 10);
        df_add_in_key(1, 1, 12);
        df_add_in_key(1, 2, 10);
        df_set_down(1);
        df_released(10);
        df_poll();
        assert_eq!((df_ran(0), df_ran(1), df_ran(2)), (1, 0, 1));
        assert_eq!((df_order(0), df_order(1)), (0, 2));
        df_released(12);
        df_poll();
        assert_eq!(df_ran(1), 1);
    }
}

#[test]
fn a_key_that_stays_up_without_a_release_event_ends_the_wait_after_three_polls() {
    let _g = fresh();
    unsafe {
        df_add_in_key(1, 0, 10);
        df_set_down(0);
        df_poll();
        df_poll();
        assert_eq!(df_ran(0), 0);
        df_poll();
        assert_eq!(df_ran(0), 1);
    }
}

#[test]
fn a_key_that_goes_down_again_restarts_the_count() {
    let _g = fresh();
    unsafe {
        df_add_in_key(1, 0, 10);
        df_set_down(0);
        df_poll();
        df_poll();
        df_set_down(1);
        df_poll();
        df_set_down(0);
        df_poll();
        df_poll();
        assert_eq!(df_ran(0), 0);
        df_poll();
        assert_eq!(df_ran(0), 1);
    }
}

#[test]
fn the_queue_has_room_for_four_and_a_poll_that_cannot_start_refuses_the_action() {
    let _g = fresh();
    unsafe {
        for i in 0..4 {
            assert_eq!(df_add_in_key(1, i, 10), 0);
        }
        assert_eq!(df_add_in_key(1, 4, 10), E_NOMEM);
        assert_eq!(df_pending(), 4);
        df_released(10);
        df_poll();
        assert_eq!(df_pending(), 0);
        df_set_start_fail(1);
        assert_eq!(df_add_in_key(1, 5, 10), E_NOMEM);
        assert_eq!(df_pending(), 0, "nothing was queued");
    }
}

#[test]
fn an_owner_or_a_cancel_removes_queued_actions() {
    let _g = fresh();
    unsafe {
        df_add_in_key(1, 0, 10);
        df_add_in_key(2, 1, 10);
        df_add_in_key(1, 2, 10);
        assert_eq!(df_drop(1), 2);
        assert_eq!(df_drop(1), 0);
        assert_eq!(df_cancel(1), 1);
        assert_eq!(df_cancel(1), 0);
        df_released(10);
        df_poll();
        assert_eq!((df_ran(0), df_ran(1), df_ran(2)), (0, 0, 0));
    }
}
