//! The view pool of core/ui: open, close, close all of an owner, a full pool, and what the firmware may still call before it removes a view.
use dkui_host::session;
use std::ffi::c_int;

extern "C" {
    fn vw_reset();
    fn vw_open(owner: usize, flags: u32, tag: c_int, with_key: c_int, with_closed: c_int) -> usize;
    fn vw_open_no_draw() -> usize;
    fn vw_open_null() -> usize;
    fn vw_set_closed_reopens(tag: c_int, reopen_with_tag: c_int);
    fn vw_reopened() -> usize;
    fn vw_close(h: usize);
    fn vw_close_owner(owner: usize) -> u32;
    fn vw_gone(slot: u32);
    fn vw_draw(slot: u32);
    fn vw_key(slot: u32, code: u32, flags: u32) -> c_int;
    fn vw_poll();
    fn vw_set_key_result(r: c_int);
    fn vw_fail_push(on: c_int);
    fn vw_set_key_mode(mode: c_int);
    fn vw_set_self(h: usize);
    fn vw_opened() -> usize;
    fn vw_set_key_down(on: c_int);
    fn vw_defer_poll();
    fn vw_defer_pending() -> u32;
    fn vw_fail_poll(on: c_int);
    fn vw_poll_starts() -> c_int;
    fn vw_used() -> u32;
    fn vw_count(what: c_int, tag: c_int) -> c_int;
    fn vw_last_flags(tag: c_int) -> u32;
    fn vw_pushed_overlay() -> u32;
    fn vw_closed_view(i: c_int) -> u32;
    fn vw_canvas_width() -> u32;
    fn vw_events() -> c_int;
    fn vw_event(i: c_int, field: c_int) -> u32;
}

/// The log events the pool asked for, as (kind, owner, a, b).
fn events() -> Vec<(u32, u32, u32, u32)> {
    (0..unsafe { vw_events() })
        .map(|i| unsafe {
            (
                vw_event(i, 0),
                vw_event(i, 1),
                vw_event(i, 2),
                vw_event(i, 3),
            )
        })
        .collect()
}

// `DK_LOG_F_KEEP` of sdk/sdk.toml `[log] f_keep`: every event of the pool carries it
const KEEP: u32 = 16;
fn all_kept() -> bool {
    (0..unsafe { vw_events() }).all(|i| unsafe { vw_event(i, 4) } == KEEP)
}

// the kinds of sdk/sdk.toml `[[log.kind]]`
const VIEW_OPENED: u32 = 54;
const VIEW_CLOSED: u32 = 55;

const OVERLAY: u32 = 1;
const VIEWS: usize = 4;
const DRAWS: c_int = 0;
const KEYS: c_int = 1;
const CLOSED: c_int = 2;
const PUSHES: c_int = 3;
const CLOSES: c_int = 4;
const INVALIDATES: c_int = 5;
const LAST_KEY: c_int = 6;
const NAMED: u32 = 100; // a panel code with a logical key (see tests/c/views.c)
const UNNAMED: u32 = 7;

fn slot(h: usize) -> u32 {
    ((h & 0xff) - 1) as u32
}
fn open(owner: usize, flags: u32, tag: c_int) -> usize {
    unsafe { vw_open(owner, flags, tag, 1, 1) }
}
fn count(what: c_int, tag: c_int) -> c_int {
    unsafe { vw_count(what, tag) }
}
fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = session();
    unsafe { vw_reset() };
    g
}

#[test]
fn a_view_opens_draws_and_closes_when_the_firmware_removes_it() {
    let _g = fresh();
    let h = open(1, 0, 0);
    assert_ne!(h, 0);
    assert_eq!(count(PUSHES, 0), 1);
    unsafe { vw_draw(slot(h)) };
    assert_eq!(count(DRAWS, 0), 1);
    assert_eq!(unsafe { vw_canvas_width() }, 128);
    unsafe { vw_close(h) };
    assert_eq!(count(CLOSES, 0), 1);
    // the firmware only flagged the view: it is still used, and `closed` has not run
    assert_eq!((unsafe { vw_used() }, count(CLOSED, 0)), (1, 0));
    // and it is not drawn any more
    unsafe { vw_draw(slot(h)) };
    assert_eq!(count(DRAWS, 0), 1);
    unsafe { vw_gone(slot(h)) };
    assert_eq!((unsafe { vw_used() }, count(CLOSED, 0)), (0, 1));
}

#[test]
fn a_bad_desc_opens_nothing() {
    let _g = fresh();
    assert_eq!(unsafe { vw_open_null() }, 0);
    assert_eq!(unsafe { vw_open_no_draw() }, 0);
    assert_eq!(unsafe { vw_open(1, 2, 0, 0, 0) }, 0, "an unknown flag");
    assert_eq!(count(PUSHES, 0), 0);
}

#[test]
fn a_view_the_firmware_could_not_make_leaves_no_slot_taken() {
    let _g = fresh();
    unsafe { vw_fail_push(1) };
    assert_eq!(open(1, 0, 0), 0);
    unsafe { vw_fail_push(0) };
    for i in 0..VIEWS {
        assert_ne!(open(1, 0, i as c_int), 0, "slot {i}");
    }
}

#[test]
fn a_full_pool_refuses_and_a_removed_view_gives_its_slot_back() {
    let _g = fresh();
    let hs: Vec<usize> = (0..VIEWS).map(|i| open(1, 0, i as c_int)).collect();
    assert!(hs.iter().all(|&h| h != 0));
    assert_eq!(open(1, 0, 5), 0);
    // closing alone does not free the slot: the firmware may still call the view
    unsafe { vw_close(hs[1]) };
    assert_eq!(open(1, 0, 5), 0);
    unsafe { vw_gone(slot(hs[1])) };
    let h = open(1, 0, 5);
    assert_ne!(h, 0);
    assert_eq!(slot(h), slot(hs[1]));
}

#[test]
fn a_handle_is_dead_after_its_view_closed_even_when_the_slot_is_reused() {
    let _g = fresh();
    let old = open(1, 0, 0);
    unsafe { vw_close(old) };
    unsafe { vw_gone(slot(old)) };
    let new = open(1, 0, 1);
    assert_eq!(slot(new), slot(old));
    assert_ne!(new, old);
    unsafe { vw_close(old) }; // the old handle must not close the new view
    unsafe { vw_close(old) };
    assert_eq!(
        count(CLOSES, 0),
        1,
        "the old handle reached the firmware no more"
    );
    unsafe { vw_draw(slot(new)) };
    assert_eq!(count(DRAWS, 1), 1);
}

#[test]
fn closing_twice_asks_the_firmware_once() {
    let _g = fresh();
    let h = open(1, 0, 0);
    unsafe { vw_close(h) };
    unsafe { vw_close(h) };
    assert_eq!(count(CLOSES, 0), 1);
    unsafe { vw_close(0) };
    unsafe { vw_close(0xffff) };
    assert_eq!(count(CLOSES, 0), 1);
}

#[test]
fn closed_may_open_a_view_in_the_slot_that_just_became_free() {
    let _g = fresh();
    unsafe { vw_set_closed_reopens(0, 1) };
    let hs: Vec<usize> = (0..VIEWS).map(|i| open(1, 0, i as c_int)).collect();
    unsafe { vw_close(hs[0]) };
    unsafe { vw_gone(slot(hs[0])) };
    assert_ne!(
        unsafe { vw_reopened() },
        0,
        "the pool was full until the slot was freed"
    );
    assert_eq!(unsafe { vw_used() }, VIEWS as u32);
}

#[test]
fn closing_an_owner_closes_its_views_and_calls_nothing_of_them() {
    let _g = fresh();
    let a = open(1, 0, 0);
    let b = open(2, 0, 1);
    let c = open(1, OVERLAY, 2);
    assert_eq!(unsafe { vw_close_owner(1) }, 2);
    assert_eq!(count(CLOSES, 0), 2);
    let closed_views: Vec<u32> = (0..2).map(|i| unsafe { vw_closed_view(i) }).collect();
    assert_eq!(closed_views, [0x1000 + slot(a), 0x1000 + slot(c)]);
    // the firmware may still draw and send keys before it removes them
    unsafe { vw_draw(slot(a)) };
    unsafe { vw_key(slot(a), NAMED, 1) };
    unsafe { vw_gone(slot(a)) };
    unsafe { vw_gone(slot(c)) };
    assert_eq!(
        (
            count(DRAWS, 0),
            count(KEYS, 0),
            count(CLOSED, 0),
            count(CLOSED, 2)
        ),
        (0, 0, 0, 0)
    );
    // the other owner's view is untouched
    unsafe { vw_draw(slot(b)) };
    assert_eq!((count(DRAWS, 1), unsafe { vw_used() }), (1, 1));
}

#[test]
fn closing_an_owner_with_a_view_that_is_closing_already() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_close(a) };
    assert_eq!(unsafe { vw_close_owner(1) }, 0, "it was closed before");
    assert_eq!(count(CLOSES, 0), 1);
    unsafe { vw_gone(slot(a)) };
    assert_eq!(count(CLOSED, 0), 0, "the owner is gone: no closed call");
}

#[test]
fn a_full_screen_view_gets_the_named_key_and_decides_if_it_is_used() {
    let _g = fresh();
    let h = open(1, 0, 0);
    unsafe { vw_set_key_result(1) };
    assert_eq!(unsafe { vw_key(slot(h), NAMED, 0x11) }, 1);
    assert_eq!(
        (count(KEYS, 0), count(LAST_KEY, 0), unsafe {
            vw_last_flags(0)
        }),
        (1, 1, 0x11)
    );
    unsafe { vw_set_key_result(0) };
    assert_eq!(
        unsafe { vw_key(slot(h), NAMED, 0) },
        0,
        "not used: passes on"
    );
}

#[test]
fn a_full_screen_view_takes_a_key_without_a_name_and_does_not_call_key() {
    let _g = fresh();
    let h = open(1, 0, 0);
    unsafe { vw_set_key_result(0) };
    assert_eq!(unsafe { vw_key(slot(h), UNNAMED, 1) }, 1);
    assert_eq!(count(KEYS, 0), 0);
}

#[test]
fn an_overlay_passes_every_key_on_and_never_calls_key() {
    let _g = fresh();
    let h = open(1, OVERLAY, 0);
    unsafe { vw_set_key_result(1) };
    assert_eq!(unsafe { vw_key(slot(h), NAMED, 1) }, 0);
    assert_eq!(count(KEYS, 0), 0);
    assert_eq!(unsafe { vw_pushed_overlay() }, 1 << slot(h));
    unsafe { vw_close(h) };
    assert_eq!(
        unsafe { vw_key(slot(h), NAMED, 1) },
        0,
        "still an overlay while it closes"
    );
}

#[test]
fn a_full_screen_view_without_key_or_a_closing_one_takes_every_key() {
    let _g = fresh();
    let h = unsafe { vw_open(1, 0, 0, 0, 0) };
    assert_eq!(unsafe { vw_key(slot(h), NAMED, 1) }, 1);
    let g = open(1, 0, 1);
    unsafe { vw_close(g) };
    assert_eq!(
        unsafe { vw_key(slot(g), NAMED, 1) },
        1,
        "a key for a closing view must not reach the stock screen"
    );
    assert_eq!(count(KEYS, 1), 0);
}

#[test]
fn poll_asks_for_a_redraw_of_each_open_view_only() {
    let _g = fresh();
    let a = open(1, 0, 0);
    open(1, OVERLAY, 1);
    unsafe { vw_poll() };
    assert_eq!(count(INVALIDATES, 0), 2);
    unsafe { vw_close(a) };
    unsafe { vw_poll() };
    assert_eq!(count(INVALIDATES, 0), 3);
}

#[test]
fn calls_with_a_slot_outside_the_pool_do_nothing() {
    let _g = fresh();
    unsafe {
        vw_draw(99);
        vw_gone(99);
        assert_eq!(vw_key(99, NAMED, 1), 0);
        vw_gone(0); // a free slot
    }
    assert_eq!(unsafe { vw_used() }, 0);
}

#[test]
fn a_view_whose_poll_cannot_start_is_not_opened_and_its_closed_never_runs() {
    let _g = fresh();
    unsafe { vw_fail_poll(1) };
    assert_eq!(open(1, 0, 0), 0);
    assert_eq!(count(PUSHES, 0), 0, "the firmware view was not made");
    assert_eq!((unsafe { vw_used() }, count(CLOSED, 0)), (0, 0));
    unsafe { vw_fail_poll(0) };
    // nothing is stuck: every slot is still free
    for i in 0..VIEWS {
        assert_ne!(open(1, 0, i as c_int), 0, "slot {i}");
    }
    assert_eq!(unsafe { vw_poll_starts() } as usize, 1 + VIEWS);
}

const PRESS: u32 = 1; // DK_UI_KEY_DOWN
const RELEASE: u32 = 0x10; // a release event: bit 0 clear

#[test]
fn a_view_opened_in_a_key_press_is_pushed_after_the_release_which_the_old_view_takes() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_set_key_down(1) };
    assert_eq!(
        unsafe { vw_key(slot(a), NAMED, PRESS) },
        0,
        "the module did not use the key"
    );
    let b = unsafe { vw_opened() };
    assert_ne!(b, 0, "the handle is given at once");
    assert_eq!(count(PUSHES, 0), 1, "only the first view is on the stack");
    assert_eq!(events().len(), 1, "and only that one was logged");
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 1, "the key is still down");
    // the release event goes to the view that is on top: it takes it, though the module says 0
    unsafe { vw_set_key_mode(0) };
    assert_eq!(unsafe { vw_key(slot(a), NAMED, RELEASE) }, 1);
    assert_eq!(count(PUSHES, 0), 1);
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 2, "pushed after the release");
    assert_eq!(
        events().len(),
        2,
        "logged when it was pushed, not when it was asked for"
    );
    assert_eq!(unsafe { vw_defer_pending() }, 0);
    unsafe { vw_draw(slot(b)) };
    assert_eq!(count(DRAWS, 5), 1, "and drawn");
}

#[test]
fn a_view_that_waits_for_its_push_is_not_drawn_or_polled_and_takes_no_key() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    let b = unsafe { vw_opened() };
    unsafe { vw_draw(slot(b)) };
    assert_eq!(count(DRAWS, 5), 0);
    assert_eq!(unsafe { vw_key(slot(b), NAMED, PRESS) }, 0);
    unsafe { vw_poll() };
    assert_eq!(count(INVALIDATES, 0), 1, "only the open view is redrawn");
    assert_eq!(unsafe { vw_used() }, 2, "but its slot is taken");
}

#[test]
fn closing_a_view_in_a_key_press_waits_for_the_release_too() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_self(a) };
    unsafe { vw_set_key_mode(2) };
    unsafe { vw_set_key_down(1) };
    assert_eq!(unsafe { vw_key(slot(a), NAMED, PRESS) }, 0);
    assert_eq!(count(CLOSES, 0), 0, "the view stays on the stack");
    unsafe { vw_set_key_mode(0) };
    unsafe { vw_defer_poll() };
    assert_eq!(count(CLOSES, 0), 0, "key down");
    assert_eq!(
        unsafe { vw_key(slot(a), NAMED, RELEASE) },
        1,
        "the release does not reach the view below"
    );
    unsafe { vw_defer_poll() };
    assert_eq!(count(CLOSES, 0), 1);
    assert_eq!(unsafe { vw_close_owner(1) }, 0);
}

#[test]
fn a_close_outside_a_key_press_is_at_once() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_close(a) };
    assert_eq!(count(CLOSES, 0), 1);
}

#[test]
fn without_a_release_event_the_key_state_ends_the_wait() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_set_key_down(1) };
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    unsafe { vw_defer_poll() };
    unsafe { vw_set_key_down(0) };
    unsafe { vw_defer_poll() };
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 1, "the key must stay up for three polls");
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 2);
}

#[test]
fn closing_a_handle_that_waits_for_its_push_ends_it_with_closed_and_no_push() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    let b = unsafe { vw_opened() };
    unsafe { vw_close(b) };
    assert_eq!(count(CLOSED, 5), 1);
    assert_eq!(unsafe { vw_defer_pending() }, 0);
    unsafe { vw_key(slot(a), NAMED, RELEASE) };
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 1, "nothing was pushed");
    assert_eq!(unsafe { vw_used() }, 1);
}

#[test]
fn an_owner_that_unloads_while_its_view_waits_for_the_push_drops_it() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    assert_eq!(unsafe { vw_defer_pending() }, 1);
    unsafe { vw_close_owner(1) };
    assert_eq!(unsafe { vw_defer_pending() }, 0);
    unsafe { vw_set_key_mode(0) };
    unsafe { vw_key(slot(a), NAMED, RELEASE) };
    unsafe { vw_defer_poll() };
    assert_eq!(count(PUSHES, 0), 1, "no push for the unloaded owner");
    assert_eq!(count(CLOSED, 5), 0, "and nothing of its code is called");
    unsafe { vw_gone(slot(a)) };
    assert_eq!(unsafe { vw_used() }, 0, "all slots free");
}

#[test]
fn a_deferred_push_that_fails_tells_the_module_with_closed() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    unsafe { vw_fail_push(1) };
    unsafe { vw_key(slot(a), NAMED, RELEASE) };
    unsafe { vw_defer_poll() };
    assert_eq!(count(CLOSED, 5), 1);
    assert_eq!(unsafe { vw_used() }, 1, "the slot is free again");
}

#[test]
fn a_full_queue_refuses_the_open_and_takes_no_slot() {
    let _g = fresh();
    let a = open(1, 0, 0);
    unsafe { vw_set_key_mode(1) };
    // the pool has room for four views: the first is open, the key opens three more, then the pool is full, not the queue
    for _ in 0..3 {
        unsafe { vw_key(slot(a), NAMED, PRESS) };
        assert_ne!(unsafe { vw_opened() }, 0);
    }
    unsafe { vw_key(slot(a), NAMED, PRESS) };
    assert_eq!(unsafe { vw_opened() }, 0, "no slot left");
    assert_eq!(unsafe { vw_used() }, VIEWS as u32);
}

#[test]
fn the_pool_logs_when_a_view_is_pushed_and_when_it_leaves_the_stack() {
    let _g = fresh();
    let h = open(7, OVERLAY, 0);
    let s = slot(h);
    assert_eq!(
        events(),
        [(VIEW_OPENED, 7, OVERLAY, s)],
        "pushed: owner, flags, slot"
    );
    unsafe { vw_close(h) };
    assert_eq!(
        events().len(),
        1,
        "a close request is no event: the view is still on the stack"
    );
    unsafe { vw_gone(s) };
    assert_eq!(
        events()[1],
        (VIEW_CLOSED, 7, s, 0),
        "the firmware removed it"
    );
    assert!(
        all_kept(),
        "both events carry the keep flag, so a card without the debug module keeps them"
    );
}
