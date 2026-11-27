use super::*;

/// A link queue that takes every request and answers with the results the test queued (`Done` when none is left).
#[derive(Default)]
struct FakeLink {
    next_id: u16,
    refuse: bool,
    results: std::collections::VecDeque<LinkResult>,
    sent: Vec<&'static str>,
    sequences: Vec<usize>,
}

impl LinkQueue for FakeLink {
    fn submit(&mut self, owner: Owner, stream: &Stream) -> Option<Ticket> {
        if self.refuse {
            return None;
        }
        assert_eq!(owner, Owner::Loader);
        match stream {
            Stream::Bulk { .. } => self.sent.push("image"),
            Stream::Sequence(s) => {
                self.sent.push("sequence");
                self.sequences.push(s.n as usize);
            }
            Stream::Owned { .. } => self.sent.push("owned"),
        }
        self.next_id += 1;
        Some(Ticket {
            owner,
            id: self.next_id,
        })
    }
    fn result(&mut self, _: Ticket) -> LinkResult {
        self.results.pop_front().unwrap_or(LinkResult::Done)
    }
}

#[derive(Default)]
struct FakeStaging {
    freed: Vec<usize>,
}

impl Staging for FakeStaging {
    fn free(&mut self, buf: *const u8) {
        self.freed.push(buf as usize);
    }
}

/// Rows of every record: `n` writes, the last one flagged; slots: a sequence of one write when `slots` is set.
struct FakeRows {
    n: u32,
    slots: bool,
}

impl Rows for FakeRows {
    fn row_step(&mut self, _: usize, want: &mut u32) -> Option<RowWrite> {
        if *want >= self.n {
            return None;
        }
        Some(RowWrite {
            win: 1,
            word: *want,
            value: 7,
            published: None,
            last: *want + 1 == self.n,
            has_undo: false,
            undo: 0,
        })
    }
    fn slots(&mut self, _: usize) -> Option<Sequence> {
        let mut s = Sequence::new();
        self.slots.then(|| {
            s.push(0, 1, 2);
            s
        })
    }
}

fn job(rec: usize, buf: usize) -> Job {
    Job {
        rec,
        base: 0x2000_1000,
        buf: buf as *const u8,
        len: 64,
    }
}

struct Rig {
    t: Transfer,
    link: FakeLink,
    staging: FakeStaging,
    rows: FakeRows,
}

impl Rig {
    fn new(rows: u32) -> Rig {
        Rig {
            t: Transfer::new(),
            link: FakeLink::default(),
            staging: FakeStaging::default(),
            rows: FakeRows {
                n: rows,
                slots: false,
            },
        }
    }
    fn step(&mut self) -> Advance {
        self.t
            .step(&mut self.link, &mut self.staging, &mut self.rows)
    }
    /// Passes until something other than `Pending` comes, at most 20.
    fn until_settled(&mut self) -> Advance {
        for _ in 0..20 {
            let a = self.step();
            if a != Advance::Pending {
                return a;
            }
        }
        panic!("the load never settled");
    }
}

#[test]
fn a_load_sends_the_image_then_its_rows_and_is_loaded() {
    let mut r = Rig::new(3);
    r.t.launch(job(2, 0x100));
    assert!(r.t.busy());
    assert_eq!(r.until_settled(), Advance::Loaded);
    assert_eq!(r.link.sent, ["image", "sequence"]);
    assert_eq!(r.link.sequences, [3]);
    assert_eq!(
        r.staging.freed,
        [0x100],
        "the staging goes back once the image is acknowledged"
    );
    r.t.loaded();
    assert!(!r.t.busy());
    assert_eq!(r.t.counts(), (1, 3, 0));
}

#[test]
fn a_service_files_slots_go_last_as_one_more_sequence() {
    let mut r = Rig::new(1);
    r.rows.slots = true;
    r.t.launch(job(0, 0x100));
    assert_eq!(r.until_settled(), Advance::Loaded);
    assert_eq!(r.link.sent, ["image", "sequence", "sequence"]);
    assert_eq!(r.link.sequences, [1, 1]);
}

#[test]
fn a_failed_image_or_row_write_fails_the_load_and_the_caller_reclaims_it() {
    let mut r = Rig::new(2);
    r.link.results.push_back(LinkResult::Failed);
    r.t.launch(job(1, 0x200));
    assert_eq!(r.until_settled(), Advance::Failed(E_DSP_XFER));
    r.t.failed(true, &mut r.staging);
    assert_eq!(r.staging.freed, [0x200]);
    assert!(!r.t.busy());
    assert_eq!(r.t.counts().2, 1);

    // a failure after the image: its staging is already back, so it is not freed twice
    let mut r = Rig::new(2);
    r.link
        .results
        .extend([LinkResult::Done, LinkResult::Failed]);
    r.t.launch(job(1, 0x300));
    assert_eq!(r.until_settled(), Advance::Failed(E_DSP_XFER));
    r.t.failed(true, &mut r.staging);
    assert_eq!(r.staging.freed, [0x300]);
}

#[test]
fn a_failure_the_caller_could_not_reclaim_retains_the_transfer_and_touches_nothing_more() {
    let mut r = Rig::new(1);
    r.link.results.push_back(LinkResult::Cancelled);
    r.t.launch(job(1, 0x400));
    assert_eq!(r.until_settled(), Advance::Failed(E_DSP_XFER));
    r.t.failed(false, &mut r.staging);
    assert!(r.t.retained() && r.staging.freed.is_empty());
    r.t.launch(job(2, 0x500));
    assert_eq!(r.step(), Advance::Pending);
    assert!(!r.t.is_current(2), "a retained transfer takes no load");
}

#[test]
fn loads_wait_in_order_behind_the_one_in_flight() {
    let mut r = Rig::new(1);
    r.t.launch(job(1, 0x100));
    r.t.launch(job(2, 0x200));
    r.t.launch(job(3, 0x300));
    assert!(r.t.is_current(1));
    assert_eq!(r.until_settled(), Advance::Loaded);
    r.t.loaded();
    assert!(r.t.is_current(2));
    assert_eq!(r.until_settled(), Advance::Loaded);
    r.t.loaded();
    assert!(r.t.is_current(3));
    assert_eq!(r.t.counts().0, 3);
}

#[test]
fn a_queued_load_is_removed_with_its_staging_and_the_load_in_flight_goes_on() {
    let mut r = Rig::new(1);
    r.t.launch(job(1, 0x100));
    r.t.launch(job(2, 0x200));
    r.t.launch(job(3, 0x300));
    assert!(r.t.dequeue(2, &mut r.staging));
    assert_eq!(r.staging.freed, [0x200]);
    assert!(!r.t.dequeue(2, &mut r.staging), "it is gone");
    assert!(
        !r.t.dequeue(1, &mut r.staging),
        "the load in flight is not in the queue"
    );
    assert_eq!(r.until_settled(), Advance::Loaded);
    r.t.loaded();
    assert!(r.t.is_current(3), "the order of the others is kept");
}

#[test]
fn a_request_that_the_queue_does_not_take_is_asked_again_on_the_next_pass() {
    let mut r = Rig::new(1);
    r.link.refuse = true;
    r.t.launch(job(1, 0x100));
    assert_eq!(r.step(), Advance::Pending);
    assert_eq!(r.step(), Advance::Pending);
    assert!(r.link.sent.is_empty());
    r.link.refuse = false;
    assert_eq!(r.until_settled(), Advance::Loaded);
}

#[test]
fn a_load_that_does_not_settle_in_the_guard_ticks_fails_or_waits_for_its_request() {
    // no request in flight (the queue never took it): the guard fails the load
    let mut r = Rig::new(1);
    r.link.refuse = true;
    r.t.launch(job(1, 0x100));
    let mut last = Advance::Pending;
    for _ in 0..=GUARD_TICKS {
        last = r.step();
    }
    assert_eq!(last, Advance::Failed(E_DSP_XFER));

    // a request still on the wire: the transfer quarantines and polls until the request ends, so the staging it reads is not freed under it
    let mut r = Rig::new(1);
    r.link
        .results
        .extend(std::iter::repeat(LinkResult::Pending).take(GUARD_TICKS as usize + 5));
    r.t.launch(job(1, 0x100));
    for _ in 0..=GUARD_TICKS + 1 {
        assert_eq!(r.step(), Advance::Pending);
    }
    r.link.results.clear();
    assert_eq!(
        r.step(),
        Advance::Failed(E_DSP_XFER),
        "the request ended: now the load fails"
    );
}
