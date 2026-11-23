use super::*;
use core::cell::Cell;
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::sample_tracks::{Env, FileId, Inspect, Load, Strack};
use dkrt_test_support::FakeCore;
use std::boxed::Box;

std::thread_local! {
    /// The test adapter: `HOOK_MORE` steps ask for more, then it fills two values and ends with `HOOK_END`.
    static HOOK_MORE: Cell<u32> = const { Cell::new(0) };
    static HOOK_END: Cell<i32> = const { Cell::new(0) };
}

unsafe extern "C" fn test_hook(
    _h: *const Handle,
    _path: *const u8,
    first: u32,
    frames: u32,
    _columns: u32,
    out: *mut i16,
    cursor: *mut u32,
) -> i32 {
    if HOOK_MORE.with(|m| m.get()) > 0 {
        HOOK_MORE.with(|m| m.set(m.get() - 1));
        return 1;
    }
    *out = -5;
    *out.add(1) = 7;
    *cursor = first + frames;
    HOOK_END.with(|e| e.get())
}

/// An environment where every file is 1000 frames long, every load is done at once and every write lands.
struct Ready;
impl Env for Ready {
    fn inspect(&mut self, _p: &[u8]) -> Result<Inspect, i32> {
        Ok(Inspect {
            id: FileId::empty(),
            frames: 1000,
            rate: 48000,
            channels: 2,
        })
    }
    fn stat(&mut self, _p: &[u8]) -> Result<FileId, i32> {
        Ok(FileId::empty())
    }
    fn load(&mut self, _req: u32, _i: &FileId, _p: &[u8], out: &mut Handle) -> Load {
        *out = Handle {
            id: 1,
            token: 7,
            base_l: 0x1000,
            base_r: 0x2000,
            frames: 1000,
            rate: 48000,
            channels: 2,
        };
        Load::Done
    }
    fn release(&mut self, _h: &Handle) {}
    fn check(&mut self, _h: &Handle) -> i32 {
        0
    }
    fn free_frames(&mut self) -> u32 {
        100_000
    }
    fn close(&mut self, _c: usize, _k: usize) {}
    fn drained(&mut self, _c: usize, _k: usize) -> bool {
        true
    }
    fn publish(&mut self, _c: usize, _k: usize, _r: &Strack) {}
    fn published(&mut self, _c: usize, _k: usize) -> i32 {
        1
    }
}

/// The public token of the first publication of a fresh core (the backing's own token is another number).
const TOKEN: u32 = 1;

/// A track core where client 0 has a READY entry 0 (token `TOKEN`), and a window of client 0 over it with the test adapter.
struct Fixture {
    core: FakeCore,
    tracks: Box<TrackCore>,
    w: Window,
}

fn fixture() -> Fixture {
    HOOK_MORE.with(|m| m.set(0));
    HOOK_END.with(|e| e.set(0));
    let mut tracks = Box::new(TrackCore::zeroed());
    tracks.start();
    assert!(tracks.open(0));
    assert_eq!(tracks.pool_add(0, b"SND/A"), 0);
    for _ in 0..10 {
        tracks.tick(&mut Ready);
    }
    assert_eq!(tracks.peak_handle(0, 0, TOKEN).map(|h| h.frames), Ok(1000));
    let mut w = WINDOW0;
    w.tracks = &*tracks as *const TrackCore;
    w.hook = Some(test_hook);
    Fixture {
        core: FakeCore::new(),
        tracks,
        w,
    }
}

fn key(first: u32) -> Key {
    Key {
        index: 0,
        token: TOKEN,
        first,
        frames: 1000,
        columns: 4,
    }
}

fn req(first: u32) -> Req {
    let mut r = REQ0;
    r.key = key(first);
    r
}

#[test]
fn argument_errors() {
    let mut buf = [0i16; 8];
    let p = buf.as_mut_ptr() as *const u8;
    assert_eq!(check_args(p, 10, 4, 16), Ok(()));
    assert_eq!(check_args(core::ptr::null(), 10, 4, 16), Err(E_CONTRACT));
    assert_eq!(check_args(unsafe { p.add(1) }, 10, 4, 16), Err(E_CONTRACT));
    assert_eq!(check_args(p, 10, 0, 16), Err(E_CONTRACT));
    assert_eq!(
        check_args(p, 10, WINDOW_MAX as u32 + 1, 4096),
        Err(E_CONTRACT)
    );
    assert_eq!(check_args(p, 10, 4, 15), Err(E_CONTRACT));
    assert_eq!(check_args(p, 0, 4, 16), Err(E_CONTRACT));
    assert_eq!(
        check_args(p, 10, WINDOW_MAX as u32, WINDOW_MAX as u32 * 4),
        Ok(())
    );
}

#[test]
fn new_key_posts_a_job_and_same_key_waits() {
    let mut f = fixture();
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        assert_eq!(lookup(&f.core, w, &req(0), out.as_mut_ptr()), 0);
        assert_eq!((*w).state, State::Pending);
        let job = (*w).job;
        assert_eq!(lookup(&f.core, w, &req(0), out.as_mut_ptr()), 0);
        assert_eq!((*w).job, job);
        assert!(!(*w).has_next);
    }
}

#[test]
fn steps_then_done_make_it_ready_and_copy() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(2));
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        f.core.job_pump();
        f.core.job_pump();
        f.core.job_pump();
        assert_eq!((*w).state, State::Pending);
        f.core.job_pump();
        assert_eq!((*w).state, State::Ready);
        assert_eq!((*w).cursor, 1000);
        assert_eq!(lookup(&f.core, w, &req(0), out.as_mut_ptr()), 4);
    }
    assert_eq!(&out[..2], &(-5i16).to_ne_bytes());
    assert_eq!(&out[2..4], &7i16.to_ne_bytes());
}

#[test]
fn key_change_while_pending_cancels_and_restarts() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(100));
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        let first_job = (*w).job;
        f.core.job_pump();
        assert_eq!(lookup(&f.core, w, &req(500), out.as_mut_ptr()), 0);
        assert!((*w).has_next);
        f.core.job_pump(); // the old job ends cancelled, `done` posts the new one
        assert!(!(*w).has_next);
        assert_eq!((*w).state, State::Pending);
        assert_ne!((*w).job, first_job);
        assert_eq!((*w).cur.key, key(500));
        assert_eq!((*w).cursor, 500);
        HOOK_MORE.with(|m| m.set(0));
        f.core.job_pump();
        f.core.job_pump();
        assert_eq!((*w).state, State::Ready);
        assert_eq!(lookup(&f.core, w, &req(500), out.as_mut_ptr()), 4);
    }
}

#[test]
fn failed_status_stays_until_the_key_changes() {
    let mut f = fixture();
    HOOK_END.with(|e| e.set(-3));
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        f.core.job_pump();
        f.core.job_pump();
        assert_eq!((*w).state, State::Failed(-3));
        let job = (*w).job;
        assert_eq!(lookup(&f.core, w, &req(0), out.as_mut_ptr()), -3);
        assert_eq!((*w).job, job);
        assert_eq!(lookup(&f.core, w, &req(1), out.as_mut_ptr()), 0);
        assert_eq!((*w).state, State::Pending);
        assert_ne!((*w).job, job);
    }
}

#[test]
fn cancelled_job_goes_idle() {
    let mut f = fixture();
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        assert_eq!(f.core.job_cancel(0, (*w).job), 0);
        f.core.job_pump();
        assert_eq!((*w).state, State::Idle);
    }
}

#[test]
fn a_reloaded_slot_goes_idle() {
    let mut f = fixture();
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        // the entry is cleared and so holds no load of the key's token any more
        assert_eq!(f.tracks.unbind(0, 0), 0);
        f.core.job_pump();
        f.core.job_pump();
        assert_eq!((*w).state, State::Idle);
    }
}

#[test]
fn reset_while_pending_ends_idle_after_the_job() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(100));
    let w = &mut f.w as *mut Window;
    let mut out = [0u8; 16];
    unsafe {
        lookup(&f.core, w, &req(0), out.as_mut_ptr());
        f.core.job_pump();
        reset_window(&f.core, w);
        assert_eq!((*w).state, State::Pending);
        f.core.job_pump();
        assert_eq!((*w).state, State::Idle);
        assert!(!(*w).abandon);
    }
}
