use super::*;
use core::cell::{Cell, RefCell};
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::sample_tracks::{Env, FileId, Inspect, Load, Strack};
use dkrt_test_support::FakeCore;
use std::boxed::Box;
use std::vec::Vec;

std::thread_local! {
    /// The `PeakDone` events the job-end entry made, for the test to apply as the owner would.
    static ENDED: RefCell<Vec<PoolEvent>> = const { RefCell::new(Vec::new()) };
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

/// The job-end entry of the tests: the cell takes the end, and the event waits for the test to apply it (the file's entry posts it to the owner).
unsafe extern "C" fn test_done(ctx: *mut u8, status: i32) {
    let event = PeakJob::end(ctx, status);
    ENDED.with(|e| e.borrow_mut().push(event));
}

/// The public token of the first publication of a fresh core (the backing's own token is another number).
const TOKEN: u32 = 1;

/// A track core where client 0 has a READY entry 0 (token `TOKEN`), the windows of the owner over it with the test adapter, and the jobs' cells outside them.
struct Fixture {
    core: FakeCore,
    tracks: Box<TrackCore>,
    pw: Box<PeakWindows>,
    jobs: Box<PeakJobs>,
}

fn fixture() -> Fixture {
    HOOK_MORE.with(|m| m.set(0));
    HOOK_END.with(|e| e.set(0));
    ENDED.with(|e| e.borrow_mut().clear());
    let mut tracks = Box::new(TrackCore::zeroed());
    tracks.start();
    assert!(tracks.open(0));
    assert_eq!(tracks.pool_add(0, b"SND/A"), 0);
    for _ in 0..10 {
        tracks.tick(&mut Ready);
    }
    assert_eq!(tracks.peak_handle(0, 0, TOKEN).map(|h| h.frames), Ok(1000));
    Fixture {
        core: FakeCore::new(),
        tracks,
        pw: Box::new(PeakWindows::new()),
        jobs: Box::new(PeakJobs::new()),
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

impl Fixture {
    fn link() -> Link {
        Link {
            hook: test_hook,
            done: test_done,
        }
    }

    /// One call of `sample_pool_peaks_window` with a valid request.
    fn lookup(&mut self, first: u32, out: &mut [u8; 16]) -> i32 {
        let w = &mut self.pw.win[0];
        w.client = 0;
        unsafe {
            lookup(
                &self.core,
                &self.jobs.job[0],
                w,
                &req(first),
                Self::link(),
                out.as_mut_ptr(),
            )
        }
    }

    /// One pass of the job task. The ends of jobs are events: they are applied by `apply_ends`, as the owner does after the call they arrived in.
    fn pump(&self) {
        // SAFETY: the test's jobs are the cells of this fixture
        unsafe { self.core.job_pump() };
    }

    fn apply_ends(&mut self) {
        let events: Vec<PoolEvent> = ENDED.with(|e| e.borrow_mut().drain(..).collect());
        for event in events {
            let PoolEvent::PeakDone { job } = event else {
                panic!("a peak-window end is a PeakDone");
            };
            self.pw
                .finish(&self.core, &self.tracks, unsafe { &*job }, Some(test_hook));
        }
    }

    fn state(&self) -> State {
        self.pw.win[0].state
    }

    fn cell(&self) -> &PeakJob {
        &self.jobs.job[0]
    }

    fn cursor(&self) -> u32 {
        unsafe { (*self.cell().data.get()).cursor }
    }
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
    let mut out = [0u8; 16];
    assert_eq!(f.lookup(0, &mut out), 0);
    assert_eq!(f.state(), State::Pending);
    let job = f.pw.win[0].job;
    assert_eq!(f.lookup(0, &mut out), 0);
    assert_eq!(f.pw.win[0].job, job);
    assert!(!f.pw.win[0].has_next);
}

#[test]
fn steps_then_the_end_event_make_it_ready_and_copy() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(2));
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    f.pump();
    f.pump();
    f.pump();
    assert_eq!(f.state(), State::Pending);
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Ready);
    assert_eq!(f.cursor(), 1000);
    assert_eq!(f.lookup(0, &mut out), 4);
    assert_eq!(&out[..2], &(-5i16).to_ne_bytes());
    assert_eq!(&out[2..4], &7i16.to_ne_bytes());
}

#[test]
fn key_change_while_pending_cancels_and_restarts() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(100));
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    let first_job = f.pw.win[0].job;
    f.pump();
    assert_eq!(f.lookup(500, &mut out), 0);
    assert!(f.pw.win[0].has_next);
    f.pump(); // the old job ends cancelled; its end event starts the new one
    f.apply_ends();
    assert!(!f.pw.win[0].has_next);
    assert_eq!(f.state(), State::Pending);
    assert_ne!(f.pw.win[0].job, first_job);
    assert_eq!(f.pw.win[0].cur, key(500));
    assert_eq!(f.cursor(), 500);
    HOOK_MORE.with(|m| m.set(0));
    f.pump();
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Ready);
    assert_eq!(f.lookup(500, &mut out), 4);
}

#[test]
fn failed_status_stays_until_the_key_changes() {
    let mut f = fixture();
    HOOK_END.with(|e| e.set(-3));
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    f.pump();
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Failed(-3));
    let job = f.pw.win[0].job;
    assert_eq!(f.lookup(0, &mut out), -3);
    assert_eq!(f.pw.win[0].job, job);
    assert_eq!(f.lookup(1, &mut out), 0);
    assert_eq!(f.state(), State::Pending);
    assert_ne!(f.pw.win[0].job, job);
}

#[test]
fn cancelled_job_goes_idle() {
    let mut f = fixture();
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    assert_eq!(unsafe { f.core.job_cancel(0, f.pw.win[0].job) }, 0);
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Idle);
}

#[test]
fn a_reloaded_slot_goes_idle() {
    let mut f = fixture();
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    // the entry is cleared and so holds no load of the key's token any more
    assert_eq!(f.tracks.unbind(0, 0), 0);
    f.pump();
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Idle);
}

#[test]
fn reset_while_pending_ends_idle_after_the_job() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(100));
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    f.pump();
    f.pw.reset(&f.core, 0);
    assert_eq!(f.state(), State::Pending);
    f.pump();
    f.apply_ends();
    assert_eq!(f.state(), State::Idle);
    assert!(!f.pw.win[0].abandon);
}

#[test]
fn the_phase_hands_the_cell_between_the_tasks() {
    let mut f = fixture();
    HOOK_MORE.with(|m| m.set(1));
    let mut out = [0u8; 16];
    assert_eq!(f.cell().phase_of(), IDLE);
    f.lookup(0, &mut out);
    assert_eq!(f.cell().phase_of(), POSTED, "posted: the worker's");
    f.pump();
    assert_eq!(f.cell().phase_of(), RUNNING, "the first step took it");
    f.pump();
    assert_eq!(
        f.cell().phase_of(),
        RUNNING,
        "the last step ended; the runtime has not ended the job"
    );
    f.pump();
    assert_eq!(f.cell().phase_of(), DONE, "the end of the job gave it back");
    // the end changed the cell and nothing in the owner: the window waits for the event
    assert_eq!(f.state(), State::Pending);
    f.apply_ends();
    assert_eq!(f.state(), State::Ready);
    assert_eq!(f.cell().phase_of(), DONE, "the UI task may read the result");
    // another key starts the next job in the same cell
    f.lookup(500, &mut out);
    assert_eq!(f.cell().phase_of(), POSTED);
}

#[test]
fn the_end_of_a_job_touches_no_window_state_and_a_step_outside_a_job_does_nothing() {
    let mut f = fixture();
    let mut out = [0u8; 16];
    f.lookup(0, &mut out);
    f.pump();
    f.pump();
    assert_eq!(f.cell().phase_of(), DONE);
    // the end changed the cell and nothing in the owner: the window waits for its event
    assert_eq!(f.state(), State::Pending);
    // a late step in a finished job's cell (the hook is still set) is refused and writes nothing
    unsafe {
        (*f.cell().data.get()).result[0] = 0;
        assert_eq!(step(f.cell() as *const PeakJob as *mut u8), E_CONTRACT);
        assert_eq!((*f.cell().data.get()).result[0], 0);
    }
    assert_eq!(f.cell().phase_of(), DONE);
    // the same for a cell that never had a job
    let fresh = PeakJobs::new();
    unsafe { assert_eq!(step(&fresh.job[0] as *const PeakJob as *mut u8), E_CONTRACT) };
    assert_eq!(fresh.job[0].phase_of(), IDLE);
}
