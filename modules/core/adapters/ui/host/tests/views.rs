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
    fn vw_used() -> u32;
    fn vw_count(what: c_int, tag: c_int) -> c_int;
    fn vw_last_flags(tag: c_int) -> u32;
    fn vw_pushed_overlay() -> u32;
    fn vw_closed_view(i: c_int) -> u32;
    fn vw_canvas_width() -> u32;
}

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
