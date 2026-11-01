extern crate std;
use super::*;
use std::vec::Vec;

#[derive(Default)]
struct Fake {
    files: Vec<(Vec<u8>, FileId, u32, u32)>, // path, id, frames, channels
    free: u32,
    queued: Vec<u32>,
    sync: bool,
    loads: u32,
    released: u32,
    lost: bool,
    closed: [[bool; 16]; 2],
    drain: bool,
    published: Vec<(usize, usize, Strack)>,
    next_tok: u32,
    load_err: Option<i32>,
    drain_forever: bool,
    /// Ticks the publish writes take to land (0: at once), the ticks left for the one in flight, and a fixed failure code.
    land_after: u32,
    landing: u32,
    publish_err: i32,
    log: Vec<&'static str>,
}
fn id(size: u32, hash: u8) -> FileId {
    let mut h = [0; 32];
    h[0] = hash;
    FileId { root: 1, size, mtime: 0, flags: ID_HASH_VALID, hash_kind: 1, hash: h }
}
impl Fake {
    fn new() -> Self {
        Fake { free: 100_000, sync: true, drain: true, next_tok: 1, ..Default::default() }
    }
    fn add(&mut self, p: &str, i: FileId, frames: u32, ch: u32) {
        self.files.push((p.as_bytes().to_vec(), i, frames, ch));
    }
    fn file(&self, p: &[u8]) -> Option<&(Vec<u8>, FileId, u32, u32)> {
        self.files.iter().find(|f| f.0 == p)
    }
}
impl Env for Fake {
    fn inspect(&mut self, p: &[u8]) -> Result<Inspect, i32> {
        self.log.push("inspect");
        self.file(p).map(|f| Inspect { id: f.1, frames: f.2, rate: 48000, channels: f.3 }).ok_or(E_NOT_FOUND)
    }
    fn stat(&mut self, p: &[u8]) -> Result<FileId, i32> {
        self.file(p).map(|f| f.1).ok_or(E_NOT_FOUND)
    }
    fn load(&mut self, req: u32, i: &FileId, p: &[u8], out: &mut Handle) -> Load {
        if let Some(e) = self.load_err {
            return Load::Err(e);
        }
        let f = self.file(p).unwrap().clone();
        assert!(f.1.same(i));
        self.loads += 1;
        *out = Handle { id: self.loads, token: self.next_tok, base_l: 0x1000 * self.loads, base_r: 0x1000 * self.loads + f.2, frames: f.2, rate: 48000, channels: f.3 };
        self.next_tok += 1;
        if self.sync {
            Load::Done
        } else {
            self.queued.push(req);
            Load::Queued
        }
    }
    fn release(&mut self, _h: &Handle) {
        self.released += 1;
    }
    fn check(&mut self, _h: &Handle) -> i32 {
        if self.lost { E_LOST } else { 0 }
    }
    fn free_frames(&mut self) -> u32 {
        self.free
    }
    fn close(&mut self, c: usize, k: usize) {
        self.log.push("close");
        self.closed[c][k] = true;
    }
    fn drained(&mut self, _c: usize, _k: usize) -> bool {
        self.drain && !self.drain_forever
    }
    fn publish(&mut self, c: usize, k: usize, r: &Strack) {
        self.closed[c][k] = false;
        self.published.push((c, k, *r));
        self.landing = self.land_after;
    }
    fn published(&mut self, _c: usize, _k: usize) -> i32 {
        if self.publish_err != 0 {
            return self.publish_err;
        }
        if self.landing == 0 {
            return 1;
        }
        self.landing -= 1;
        0
    }
}
type C = Core<2, 8, 3>;
fn run(c: &mut C, e: &mut Fake, n: usize) {
    for _ in 0..n {
        c.tick(e);
    }
}
fn core() -> C {
    let mut c = C::new();
    assert!(c.open(0));
    c
}

#[test]
fn select_reaches_ready_and_publishes_one_record() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    assert_eq!(c.select(0, 3, b"a.wav\0junk"), 0);
    assert_eq!(c.status(0, 3), QUEUED);
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 3), READY);
    let (cl, tr, r) = e.published[0];
    assert_eq!((cl, tr), (0, 3));
    assert_eq!((r.ready, r.frames, r.channels, r.fmt), (1, 500, 2, FMT_PCM16));
    assert_ne!(r.base_l, r.base_r);
    assert_eq!(c.info(0, 3).unwrap().token, r.token);
}

#[test]
fn ready_waits_until_the_record_has_landed_in_the_dsp() {
    let (mut c, mut e) = (core(), Fake::new());
    e.land_after = 3;
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), LOADING, "published, but the writes are on their way");
    assert_eq!(c.info(0, 0), Err(E_NOT_FOUND));
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 0), READY);
    assert_eq!(e.published.len(), 1);
}

#[test]
fn a_publish_the_link_gave_up_is_a_track_error_and_a_new_select_recovers() {
    let (mut c, mut e) = (core(), Fake::new());
    e.publish_err = E_DSP_XFER;
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 6);
    assert_eq!(c.status(0, 0), E_DSP_XFER);
    e.publish_err = 0;
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 8);
    assert_eq!(c.status(0, 0), READY);
}

#[test]
fn backing_lost_while_the_record_is_on_its_way_reloads() {
    let (mut c, mut e) = (core(), Fake::new());
    e.land_after = 2;
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), LOADING);
    c.lost(&mut e);
    run(&mut c, &mut e, 20);
    assert_eq!(c.status(0, 0), READY);
    assert_eq!(e.loads, 2, "reloaded from the remembered path");
}

#[test]
fn bad_arguments() {
    let (mut c, _e) = (core(), Fake::new());
    assert_eq!(c.select(0, 16, b"a"), E_CONTRACT);
    assert_eq!(c.select(0, 0, b""), E_SEL_PATH);
    assert_eq!(c.select(0, 0, &[b'a'; 256]), E_SEL_PATH);
    assert_eq!(c.select(1, 0, b"a"), E_NOT_FOUND);
    assert_eq!(c.status(0, 16), E_CONTRACT);
    assert_eq!(c.info(0, 0), Err(E_NOT_FOUND));
}

#[test]
fn mono_publishes_both_planes_at_one_address() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("m.wav", id(10, 2), 64, 1);
    c.select(0, 0, b"m.wav");
    run(&mut c, &mut e, 4);
    let r = e.published[0].2;
    assert_eq!(r.base_l, r.base_r);
}

#[test]
fn preflight_errors_leave_the_old_binding() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    e.add("big.wav", id(900, 3), 90_000, 2);
    e.add("zero.wav", id(5, 4), 0, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    c.select(0, 0, b"nope.wav");
    run(&mut c, &mut e, 2);
    assert_eq!(c.status(0, 0), E_NOT_FOUND);
    assert_eq!(c.refs(0, 0), Some(1));
    assert!(!e.closed[0][0]);
    c.select(0, 1, b"zero.wav");
    run(&mut c, &mut e, 2);
    assert_eq!(c.status(0, 1), E_FORMAT);
    c.select(0, 1, b"big.wav");
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 1), E_TOO_BIG);
    assert_eq!(e.loads, 1);
    // ERROR is sticky until the next select
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 1), E_TOO_BIG);
}

#[test]
fn dedup_shares_one_load_and_the_last_unbind_releases() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"a.wav");
    run(&mut c, &mut e, 6);
    assert_eq!((c.status(0, 0), c.status(0, 1)), (READY, READY));
    assert_eq!(e.loads, 1);
    assert_eq!(c.refs(0, 0), Some(2));
    let toks: Vec<u32> = e.published.iter().map(|p| p.2.token).collect();
    assert_ne!(toks[0], toks[1]);
    c.unbind(0, 0);
    assert_eq!(c.status(0, 0), UNUSED);
    run(&mut c, &mut e, 4);
    assert_eq!(e.released, 0);
    c.unbind(0, 1);
    run(&mut c, &mut e, 4);
    assert_eq!(e.released, 1);
    assert_eq!(c.entries_used(), 0);
}

#[test]
fn identity_without_hash_needs_the_same_path() {
    let (mut c, mut e) = (core(), Fake::new());
    let mut i = id(100, 0);
    i.flags = 0;
    i.mtime = 7;
    e.add("a.wav", i, 500, 2);
    e.add("b.wav", i, 500, 2);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"b.wav");
    c.select(0, 2, b"a.wav");
    run(&mut c, &mut e, 8);
    assert_eq!(e.loads, 2);
}

#[test]
fn reselect_closes_and_drains_before_the_backing_is_touched() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    e.add("b.wav", id(200, 2), 300, 1);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    e.drain = false;
    c.select(0, 0, b"b.wav");
    run(&mut c, &mut e, 5);
    assert!(e.closed[0][0]);
    assert_eq!(c.status(0, 0), QUEUED);
    assert_eq!((e.loads, e.released), (1, 0));
    e.drain = true;
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 0), READY);
    assert_eq!((e.loads, e.released), (2, 1));
    assert_eq!(c.info(0, 0).unwrap().channels, 1);
    assert!(!e.closed[0][0]);
}

#[test]
fn reselect_of_the_same_file_keeps_its_entry() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    let t1 = c.info(0, 0).unwrap().token;
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 6);
    assert_eq!((e.loads, e.released), (1, 0));
    assert_ne!(c.info(0, 0).unwrap().token, t1);
}

#[test]
fn drain_that_never_acks_fails_closed_and_keeps_the_pcm() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    e.add("b.wav", id(200, 2), 300, 1);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    e.drain_forever = true;
    c.select(0, 0, b"b.wav");
    run(&mut c, &mut e, DRAIN_GUARD as usize + 10);
    assert_eq!(c.status(0, 0), E_DSP_XFER);
    assert_eq!(e.released, 0);
}

#[test]
fn supersede_while_loading_drops_the_old_result() {
    let (mut c, mut e) = (core(), Fake::new());
    e.sync = false;
    e.add("a.wav", id(100, 1), 500, 2);
    e.add("b.wav", id(200, 2), 300, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), LOADING);
    c.select(0, 0, b"b.wav");
    assert_eq!(c.status(0, 0), QUEUED);
    run(&mut c, &mut e, 3);
    assert_eq!(e.queued.len(), 2);
    // the first completion arrives late: the track must not publish it
    c.done(e.queued[0], 0);
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), LOADING);
    assert!(e.published.is_empty());
    assert_eq!(e.released, 1, "orphan entry released once terminal");
    c.done(e.queued[1], 0);
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), READY);
    assert_eq!(c.info(0, 0).unwrap().frames, 300);
    c.done(9999, 0);
    c.done(0, 0);
}

#[test]
fn load_errors_become_sticky_track_errors() {
    let (mut c, mut e) = (core(), Fake::new());
    e.sync = false;
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    c.done(e.queued[0], E_POOL);
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), E_POOL);
    assert_eq!(c.entries_used(), 0);
    e.load_err = Some(E_POOL);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), E_POOL);
    assert_eq!(c.entries_used(), 0);
}

#[test]
fn file_changed_between_select_and_load_is_refused() {
    let (mut c, mut e) = (core(), Fake::new());
    e.sync = false;
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 3);
    e.files[0].1 = id(100, 9);
    c.done(e.queued[0], 0);
    run(&mut c, &mut e, 3);
    assert_eq!(c.status(0, 0), E_CHANGED);
    assert!(e.published.is_empty());
}

#[test]
fn backing_lost_reloads_bound_tracks() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"a.wav");
    run(&mut c, &mut e, 6);
    let t0 = c.info(0, 0).unwrap().token;
    c.lost(&mut e);
    assert_eq!(c.status(0, 0), LOADING);
    run(&mut c, &mut e, 8);
    assert_eq!((c.status(0, 0), c.status(0, 1)), (READY, READY));
    assert_eq!(e.loads, 2, "one reload shared by both tracks");
    assert_eq!(e.released, 0, "dead handles are never released");
    assert_ne!(c.info(0, 0).unwrap().token, t0);
}

#[test]
fn backing_lost_closes_a_playing_track_before_any_file_access() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 6);
    assert_eq!(c.status(0, 0), READY);
    e.log.clear();
    c.lost(&mut e);
    assert_eq!(e.log, ["close"], "the close is issued by lost() itself");
    run(&mut c, &mut e, 8);
    assert_eq!(e.log[1], "inspect");
    assert_eq!(c.status(0, 0), READY);
}

#[test]
fn backing_lost_with_a_changed_or_missing_file() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    e.add("b.wav", id(100, 2), 500, 2);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"b.wav");
    run(&mut c, &mut e, 6);
    e.files[0].1 = id(100, 7);
    e.files.pop();
    c.lost(&mut e);
    run(&mut c, &mut e, 8);
    assert_eq!(c.status(0, 0), E_CHANGED);
    assert_eq!(c.status(0, 1), E_LOST);
}

#[test]
fn check_failure_before_publish_counts_as_loss() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 500, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    e.lost = true;
    run(&mut c, &mut e, 2);
    assert_eq!(c.status(0, 0), LOADING);
    e.lost = false;
    run(&mut c, &mut e, 8);
    assert_eq!(c.status(0, 0), READY);
}

#[test]
fn project_change_selects_first_then_unbinds_and_keeps_shared_entries() {
    let (mut c, mut e) = (core(), Fake::new());
    for (n, h) in [("a.wav", 1u8), ("b.wav", 2), ("c.wav", 3)] {
        e.add(n, id(100 + h as u32, h), 400, 2);
    }
    c.project_changed(5);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"b.wav");
    run(&mut c, &mut e, 8);
    c.project_changed(6);
    c.select(0, 0, b"a.wav");
    c.select(0, 2, b"c.wav");
    run(&mut c, &mut e, 8);
    assert_eq!(e.loads, 3);
    assert_eq!(c.status(0, 1), UNUSED, "project 6 has no track 1");
    c.project_changed(5);
    run(&mut c, &mut e, 10);
    assert_eq!((c.status(0, 0), c.status(0, 1), c.status(0, 2)), (READY, READY, UNUSED));
    assert_eq!(e.loads, 4, "a.wav survived through dedup; b.wav was released by project 6 and loads again");
}

#[test]
fn selections_made_before_the_project_is_known_belong_to_the_first_project() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 400, 2);
    c.select(0, 4, b"a.wav");
    c.project_changed(2);
    run(&mut c, &mut e, 5);
    assert_eq!(c.status(0, 4), READY);
    c.project_changed(3);
    run(&mut c, &mut e, 5);
    assert_eq!(c.status(0, 4), UNUSED);
    c.project_changed(2);
    run(&mut c, &mut e, 6);
    assert_eq!(c.status(0, 4), READY);
}

#[test]
fn missing_file_after_project_change_is_a_per_track_error() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 400, 2);
    c.project_changed(1);
    c.select(0, 0, b"a.wav");
    c.select(0, 1, b"gone.wav");
    c.project_changed(2);
    c.project_changed(1);
    run(&mut c, &mut e, 8);
    assert_eq!(c.status(0, 0), READY);
    assert_eq!(c.status(0, 1), E_NOT_FOUND);
}

#[test]
fn teardown_waits_for_the_drain_then_frees() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 400, 2);
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    e.drain = false;
    assert!(!c.teardown(0, &mut e));
    e.drain = true;
    assert!(c.teardown(0, &mut e) || c.teardown(0, &mut e));
    assert!(!c.is_open(0));
    run(&mut c, &mut e, 2);
    assert_eq!((e.released, c.entries_used()), (1, 0));
}

#[test]
fn tokens_and_requests_do_not_wrap() {
    let (mut c, mut e) = (core(), Fake::new());
    e.add("a.wav", id(100, 1), 400, 2);
    c.next_token = u32::MAX;
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 0), E_NOMEM);
    let mut c = core();
    c.next_req = u32::MAX;
    c.select(0, 0, b"a.wav");
    run(&mut c, &mut e, 4);
    assert_eq!(c.status(0, 0), E_NOMEM);
}
