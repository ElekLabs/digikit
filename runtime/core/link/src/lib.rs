//! The DSP link of the core: one queue and one table of live values over the transport of `core/dsp-link` (docs/plans/runtime-and-services.md, R6).
//!
//! The wire protocol is unchanged: one transfer at a time (`dl_send`, `dl_send_win`, `dl_poll`), refused while the link is busy or sends its END frames.
//! What this module adds is who may use it and in what order:
//!
//! - **Live values**: `set(slot, value)` keeps the newest value of a word with a version and a dirty bit. `pump` sends a dirty word when the link is free.
//!   Several sets between two sends give one write of the newest value. The link reports DONE when the DSP has taken and copied the bytes, so the confirmed
//!   version is the version that was sent, kept here on the ColdFire side. A write that failed stays dirty and is sent again after a short hold. A DSP
//!   restart marks every word dirty again (`dsp_restarted`).
//! - **Requests with an owner**: `submit(owner, stream)` gives a `Ticket`; `result(ticket)` answers its owner only. A request is a bulk (bytes, sent in chunks
//!   of `CHUNK` so a live value can go between two chunks) or a sequence (word writes in order; the last word publishes). When a transfer of a sequence fails,
//!   the steps that have an undo word are undone in reverse order and the request fails. Requests are sent oldest first, so every owner's requests stay in order.
//! - **Priority**: an undo first, then a dirty live value, then the oldest request. A live edit therefore goes between bulk chunks and between sequence steps.
//! - `drain(owner)` cancels the owner's queued requests and waits for the transfer in flight.
//!
//! The pump runs in the UI task (`dkrt_tick`, and the blocking waits below). Nothing here runs in an interrupt. The transport is a trait so that host tests drive
//! the same code over a model of the link's states (`FakeLink`).
//!
//! If chains, not a `match`: dense arms become a PC-relative jump table, which is not ColdFire code.
#![cfg_attr(not(any(test, feature = "fake")), no_std)]
use dkrt_abi::link::*;

mod event;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod flight;
#[cfg(any(test, feature = "fake"))]
mod inspect;
mod limits;
mod link_stats;
mod live;
mod poll;
mod req;
mod sent;
pub mod stats;
#[cfg(test)]
mod tests;
mod transport;

pub use event::{EV_DONE, EV_ERROR, EV_LIVE, EV_RESEND, EV_START, EV_SUBMIT};
pub use limits::{CHUNK, REQ_MAX, SLOT_MAX};
pub use link_stats::Stats;
pub use poll::Poll;
pub use sent::Sent;
pub use transport::Transport;

use event::*;
use flight::*;
use limits::*;
use live::*;
use req::*;

pub struct Link<T: Transport> {
    pub t: T,
    live: [Live; SLOT_MAX],
    reqs: [Req; REQ_MAX],
    next_id: u16,
    next_order: u32,
    cursor: u8,
    flight: Flight,
    /// The bytes of the transfer in flight when they are not in the request (a word, an owned write).
    buf: [u8; OWNED_MAX],
    pub stats: Stats,
    /// Polls a blocking wait spins before it gives up (0: `SPIN`; a host test sets a small bound).
    pub spin: u32,
}

impl<T: Transport> Link<T> {
    pub const fn new(t: T) -> Self {
        Link {
            t,
            live: [LIVE0; SLOT_MAX],
            reqs: [REQ0; REQ_MAX],
            next_id: 0,
            next_order: 0,
            cursor: 0,
            flight: FLIGHT0,
            buf: [0; OWNED_MAX],
            stats: Stats {
                live_sent: 0,
                live_errors: 0,
                req_done: 0,
                req_failed: 0,
            },
            spin: 0,
        }
    }

    fn bound(&self) -> u32 {
        if self.spin == 0 {
            SPIN
        } else {
            self.spin
        }
    }

    // ---- live values -------------------------------------------------------------------------------------------------------

    fn find_live(&self, slot: Slot) -> Option<usize> {
        let mut i = 0;
        while i < SLOT_MAX {
            let e = &self.live[i];
            if e.used && e.win == slot.win && e.word == slot.word {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Set the live value of `slot`. The same value again changes nothing.
    pub fn set(&mut self, slot: Slot, value: u32) {
        if slot.win >= WIN_RESERVED {
            return;
        }
        self.set_live(slot, value);
    }

    /// Set a control word of the stats window (window 3, `STATS_ON`, `STATS_CLEAR` or `STATS_PERIOD`). Only the runtime calls this, for the owner of the span channel
    /// (`dk_dsp_stats_ctl`); `set` and `submit` refuse window 3. Returns false for another word.
    pub fn set_stats_ctl(&mut self, word: u32, value: u32) -> bool {
        if word >= STATS_CTL_WORDS {
            return false;
        }
        self.set_live(Slot::new(WIN_STATS as u32, word), value);
        true
    }

    fn set_live(&mut self, slot: Slot, value: u32) {
        if let Some(i) = self.find_live(slot) {
            let e = &mut self.live[i];
            if e.value != value {
                e.value = value;
                e.ver = e.ver.wrapping_add(1);
                e.dirty = true;
            }
            return;
        }
        let mut i = 0;
        while i < SLOT_MAX {
            if !self.live[i].used {
                self.live[i] = Live {
                    used: true,
                    dirty: true,
                    win: slot.win,
                    word: slot.word,
                    value,
                    ver: 1,
                    ..LIVE0
                };
                return;
            }
            i += 1;
        }
    }

    /// The value of `slot` the DSP has taken.
    pub fn confirmed(&self, slot: Slot) -> Option<u32> {
        match self.find_live(slot) {
            Some(i) if self.live[i].has_conf => Some(self.live[i].conf_value),
            _ => None,
        }
    }

    /// Whether `slot` has a value the DSP has not taken yet.
    pub fn dirty(&self, slot: Slot) -> bool {
        match self.find_live(slot) {
            Some(i) => self.live[i].dirty,
            None => false,
        }
    }

    pub fn errors(&self) -> u32 {
        self.stats.live_errors
    }

    /// Drop the live values of `count` words from `first` of window `win`.
    pub fn forget(&mut self, win: u32, first: u32, count: u32) {
        let mut i = 0;
        while i < SLOT_MAX {
            let e = &mut self.live[i];
            if e.used && e.win as u32 == win && e.word >= first && e.word - first < count {
                e.used = false;
            }
            i += 1;
        }
    }

    /// The DSP restarted: it holds none of the values. Every word is dirty again and none counts as confirmed.
    pub fn dsp_restarted(&mut self) {
        let mut i = 0;
        while i < SLOT_MAX {
            let e = &mut self.live[i];
            if e.used {
                e.dirty = true;
                e.has_conf = false;
                e.hold = 0;
            }
            i += 1;
        }
    }

    // ---- requests ----------------------------------------------------------------------------------------------------------

    /// Queue a request. `None` when the queue is full or the request is not valid.
    pub fn submit(&mut self, owner: Owner, stream: &Stream) -> Option<Ticket> {
        let mut r = REQ0;
        r.owner = owner.index() as u8;
        let (kind, steps, len) = if let &Stream::Bulk { dst, src, len } = stream {
            if src.is_null() || len == 0 || len & 3 != 0 || dst & 3 != 0 {
                return None;
            }
            r.kind = K_BULK;
            r.dst = dst;
            r.src = src;
            r.len = len;
            (K_BULK, 0, len)
        } else if let Stream::Owned {
            win,
            dst,
            len,
            bytes,
        } = stream
        {
            let (win, dst, len) = (*win, *dst, *len);
            if len == 0 || len & 3 != 0 || len as usize > OWNED_MAX || win >= WIN_RESERVED {
                return None;
            }
            r.kind = K_OWNED;
            r.win = win;
            r.dst = dst;
            r.len = len as u32;
            r.bytes = *bytes;
            (K_OWNED, 0, len as u32)
        } else if let Stream::Sequence(seq) = stream {
            if seq.n == 0 || seq.n as usize > SEQ_MAX {
                return None;
            }
            let mut i = 0;
            while i < seq.n as usize {
                if seq.steps[i].win >= WIN_RESERVED {
                    return None;
                }
                i += 1;
            }
            r.kind = K_SEQ;
            r.seq = *seq;
            (K_SEQ, seq.n as u32, seq.n as u32 * 4)
        } else {
            return None;
        };
        let mut i = 0;
        while i < REQ_MAX {
            if self.reqs[i].state == R_FREE {
                // ids start at 1 and skip 0; `next_id` is the last one given, so the whole queue is zero at start (it lives in .bss)
                let id = if self.next_id == u16::MAX {
                    1
                } else {
                    self.next_id + 1
                };
                self.next_id = id;
                r.id = id;
                r.order = self.next_order;
                self.next_order = self.next_order.wrapping_add(1);
                r.state = R_QUEUED;
                self.reqs[i] = r;
                self.t.event(
                    EV_SUBMIT,
                    [
                        owner.index() as u32,
                        id as u32,
                        kind as u32 | steps << 8,
                        len,
                    ],
                );
                return Some(Ticket { owner, id });
            }
            i += 1;
        }
        None
    }

    fn find_req(&self, t: Ticket) -> Option<usize> {
        let mut i = 0;
        while i < REQ_MAX {
            let r = &self.reqs[i];
            if r.state != R_FREE && r.id == t.id && r.owner == t.owner.index() as u8 {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// The result of a request, for its owner only. A terminal result is given once.
    pub fn result(&mut self, t: Ticket) -> LinkResult {
        let i = match self.find_req(t) {
            Some(i) => i,
            None => return LinkResult::Gone,
        };
        let s = self.reqs[i].state;
        if s == R_QUEUED || s == R_ACTIVE || s == R_ROLLBACK {
            return LinkResult::Pending;
        }
        self.reqs[i].state = R_FREE;
        if s == R_DONE {
            LinkResult::Done
        } else if s == R_FAILED {
            LinkResult::Failed
        } else {
            LinkResult::RollbackFailed
        }
    }

    /// Cancel the owner's requests and wait for its transfer in flight. `false` when that did not end within the spin bound.
    pub unsafe fn drain(&mut self, owner: Owner) -> bool {
        let o = owner.index() as u8;
        let fl = self.flight;
        let mut i = 0;
        let mut waiting = false;
        while i < REQ_MAX {
            let r = &mut self.reqs[i];
            if r.state != R_FREE && r.owner == o {
                if fl.kind >= F_CHUNK && fl.idx as usize == i {
                    // its transfer is on the wire: settle it first, then drop the request
                    r.cancel = true;
                    waiting = true;
                } else {
                    r.state = R_FREE;
                }
            }
            i += 1;
        }
        let mut n = 0;
        while waiting && self.flight.kind != F_NONE && self.reqs[fl.idx as usize].cancel {
            match self.t.poll() {
                Poll::Busy => {}
                Poll::Done => self.settle(true, 0),
                Poll::Error(c) => self.settle(false, c),
            }
            n += 1;
            if n > self.bound() {
                return false;
            }
        }
        true
    }

    /// Give up a request nobody waits for: its slot is freed now, or when its transfer on the wire ends.
    fn abandon(&mut self, t: Ticket) {
        if let Some(i) = self.find_req(t) {
            if self.flight.kind >= F_CHUNK && self.flight.idx as usize == i {
                self.reqs[i].cancel = true;
            } else {
                self.reqs[i].state = R_FREE;
            }
        }
    }

    /// Send `seq` for `owner` and wait for the result.
    pub unsafe fn run(&mut self, owner: Owner, seq: &Sequence) -> bool {
        let stream = Stream::Sequence(*seq);
        let mut n = 0;
        let ticket = loop {
            if let Some(t) = self.submit(owner, &stream) {
                break t;
            }
            self.pump();
            n += 1;
            if n > self.bound() {
                return false;
            }
        };
        loop {
            self.pump();
            let r = self.result(ticket);
            if r == LinkResult::Done {
                return true;
            }
            if r != LinkResult::Pending {
                return false;
            }
            n += 1;
            if n > self.bound() {
                self.abandon(ticket);
                return false;
            }
        }
    }

    /// A transfer of `owner` on the wire, for the loader's host tests: its request ends with whatever the transport answers to the next poll.
    #[cfg(any(test, feature = "fake"))]
    pub fn test_in_flight(&mut self, owner: Owner) -> Ticket {
        let t = self
            .submit(
                owner,
                &Stream::Sequence({
                    let mut s = Sequence::new();
                    s.push(1, 0, 0);
                    s
                }),
            )
            .unwrap();
        let i = self.find_req(t).unwrap();
        self.reqs[i].state = R_ACTIVE;
        self.flight = Flight {
            kind: F_STEP,
            idx: i as u8,
            win: 0,
            ver: 0,
            word: 0,
            value: 4,
        };
        t
    }

    // ---- the pump ----------------------------------------------------------------------------------------------------------

    /// Settle the transfer in flight, then start the next one when the link takes it. Once per UI pass.
    pub unsafe fn pump(&mut self) {
        if self.flight.kind != F_NONE {
            match self.t.poll() {
                Poll::Busy => return,
                Poll::Done => self.settle(true, 0),
                Poll::Error(c) => self.settle(false, c),
            }
        }
        self.start();
    }

    /// The transfer in flight ended.
    fn settle(&mut self, ok: bool, code: u32) {
        let f = self.flight;
        self.flight = FLIGHT0;
        let ix = f.idx as usize;
        if f.kind == F_LIVE {
            let e = &mut self.live[ix];
            if !e.used || e.win != f.win || e.word != f.word {
                return; // the word was forgotten while its write was on the wire
            }
            if ok {
                e.has_conf = true;
                e.conf_ver = f.ver;
                e.conf_value = f.value;
                if e.ver == f.ver {
                    e.dirty = false;
                }
                e.failed = false;
                e.tries = 0;
                self.stats.live_sent += 1;
                self.t
                    .event(EV_LIVE, [f.idx as u32, f.ver as u32, f.win as u32, f.word]);
            } else {
                e.hold = HOLD;
                e.failed = true;
                self.stats.live_errors += 1;
                self.t
                    .event(EV_ERROR, [0xff, 0x8000 | f.idx as u32, code, f.idx as u32]);
            }
            return;
        }
        let r = &mut self.reqs[ix];
        if r.cancel {
            r.state = R_FREE;
            return;
        }
        if f.kind == F_CHUNK {
            if ok {
                r.off += f.value;
                if r.off >= r.len {
                    r.state = R_DONE;
                }
            } else {
                r.state = R_FAILED;
                r.code = code;
            }
        } else if f.kind == F_STEP {
            if ok {
                r.pos += 1;
                if r.pos >= r.seq.n {
                    r.state = R_DONE;
                }
            } else {
                r.code = code;
                // undo what may have landed, from the failed step back
                r.rb = r.pos as i8;
                r.state = R_ROLLBACK;
                Self::next_undo(r);
            }
        } else if f.kind == F_UNDO {
            if ok {
                r.rb -= 1;
                Self::next_undo(r);
            } else {
                r.state = R_RBFAILED;
            }
        }
        let (st, owner, id, code, pos) =
            (r.state, r.owner as u32, r.id as u32, r.code, r.pos as u32);
        if st == R_DONE {
            self.stats.req_done += 1;
            self.t.event(EV_DONE, [owner, id, 0, 0]);
        } else if st == R_FAILED || st == R_RBFAILED {
            self.stats.req_failed += 1;
            self.t.event(EV_ERROR, [owner, id, code, pos]);
        }
    }

    /// Move `rb` to the next step with an undo word; with none left the rollback is complete and the request failed.
    fn next_undo(r: &mut Req) {
        while r.rb >= 0 && r.seq.steps[r.rb as usize].flags & STEP_UNDO == 0 {
            r.rb -= 1;
        }
        if r.rb < 0 {
            r.state = R_FAILED;
        }
    }

    unsafe fn start(&mut self) {
        // 1. an undo: a half-published row must not stay visible
        let mut best = REQ_MAX;
        let mut i = 0;
        while i < REQ_MAX {
            if self.reqs[i].state == R_ROLLBACK
                && (best == REQ_MAX || self.reqs[i].order < self.reqs[best].order)
            {
                best = i;
            }
            i += 1;
        }
        if best != REQ_MAX {
            let s = self.reqs[best].seq.steps[self.reqs[best].rb as usize];
            self.buf[..4].copy_from_slice(&s.undo.to_le_bytes());
            let ok = self.t.send(s.win as u32, s.at, self.buf.as_ptr(), 4);
            if ok == Sent::Accepted {
                self.flight = Flight {
                    kind: F_UNDO,
                    idx: best as u8,
                    win: 0,
                    ver: 0,
                    word: 0,
                    value: 0,
                };
            } else if ok == Sent::Hard {
                self.reqs[best].state = R_RBFAILED;
            }
            return;
        }
        // 2. a dirty live word
        let mut i = 0;
        while i < SLOT_MAX {
            if self.live[i].hold > 0 {
                self.live[i].hold -= 1;
            }
            i += 1;
        }
        let mut n = 0;
        let mut j = self.cursor as usize;
        while n < SLOT_MAX {
            let e = self.live[j];
            if e.used && e.dirty && e.hold == 0 {
                self.buf[..4].copy_from_slice(&e.value.to_le_bytes());
                let ok = self.t.send(e.win as u32, e.word, self.buf.as_ptr(), 4);
                if ok == Sent::Accepted {
                    if e.failed {
                        self.t
                            .event(EV_RESEND, [j as u32, e.ver as u32, e.tries as u32 + 1, 0]);
                        self.live[j].tries = e.tries.saturating_add(1);
                    }
                    self.flight = Flight {
                        kind: F_LIVE,
                        idx: j as u8,
                        win: e.win,
                        ver: e.ver,
                        word: e.word,
                        value: e.value,
                    };
                    self.cursor = if j + 1 == SLOT_MAX { 0 } else { j as u8 + 1 };
                } else if ok == Sent::Hard {
                    self.live[j].hold = HOLD;
                    self.stats.live_errors += 1;
                }
                return;
            }
            j = if j + 1 == SLOT_MAX { 0 } else { j + 1 };
            n += 1;
        }
        // 3. the oldest request
        let mut best = REQ_MAX;
        let mut i = 0;
        while i < REQ_MAX {
            let s = self.reqs[i].state;
            if (s == R_QUEUED || s == R_ACTIVE)
                && (best == REQ_MAX || self.reqs[i].order < self.reqs[best].order)
            {
                best = i;
            }
            i += 1;
        }
        if best == REQ_MAX {
            return;
        }
        let r = self.reqs[best];
        let (owner, id) = (r.owner as u32, r.id as u32);
        let (win, dst, buf, len, kind, step);
        if r.kind == K_BULK {
            let n = if r.len - r.off > CHUNK {
                CHUNK
            } else {
                r.len - r.off
            };
            (win, dst, buf, len, kind, step) =
                (0, r.dst + r.off, r.src.add(r.off as usize), n, F_CHUNK, 0);
        } else if r.kind == K_OWNED {
            self.buf[..OWNED_MAX].copy_from_slice(&r.bytes);
            (win, dst, buf, len, kind, step) =
                (r.win as u32, r.dst, self.buf.as_ptr(), r.len, F_CHUNK, 0);
        } else {
            let s = r.seq.steps[r.pos as usize];
            self.buf[..4].copy_from_slice(&s.value.to_le_bytes());
            (win, dst, buf, len, kind, step) =
                (s.win as u32, s.at, self.buf.as_ptr(), 4, F_STEP, s.flags);
        }
        let ok = self.t.send(win, dst, buf, len);
        if ok == Sent::Accepted {
            if r.state == R_QUEUED {
                self.reqs[best].state = R_ACTIVE;
                self.t.event(EV_START, [owner, id, 0, 0]);
            }
            if step & STEP_MARK != 0 && !r.seq.mark.is_null() {
                *r.seq.mark = 1;
            }
            self.flight = Flight {
                kind,
                idx: best as u8,
                win: 0,
                ver: 0,
                word: 0,
                value: len,
            };
        } else if ok == Sent::Hard {
            self.reqs[best].state = R_FAILED;
            self.reqs[best].code = u32::MAX;
            self.stats.req_failed += 1;
            self.t.event(EV_ERROR, [owner, id, u32::MAX, 0]);
        }
    }
}
