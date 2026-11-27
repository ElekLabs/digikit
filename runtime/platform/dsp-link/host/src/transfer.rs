use crate::link_queue::LinkQueue;
use dkrt_abi::dsp::GUARD_TICKS;
use dkrt_abi::link::{LinkResult, Owner, Sequence, Stream, Ticket, SEQ_MAX};
use dkrt_abi::service::RowWrite;
use dkrt_abi::REC_MAX;
use dkrt_sdk::E_DSP_XFER;

/// One load: module record `rec`, its relocated image of `len` bytes at `buf` (the host's staging memory), to go to byte address `base` of the area.
#[derive(Clone, Copy)]
pub struct Job {
    pub rec: usize,
    pub base: u32,
    pub buf: *const u8,
    pub len: u32,
}

/// Where the rows of a load come from: the service that owns the module's rows, and a service file's slots.
pub trait Rows {
    /// The write at index `*want` of record `rec`'s row sequence; `None` when there is none.
    fn row_step(&mut self, rec: usize, want: &mut u32) -> Option<RowWrite>;
    /// The slot writes of record `rec`'s service file, as one sequence; `None` when it has none.
    fn slots(&mut self, rec: usize) -> Option<Sequence>;
}

/// The staging memory of a job: given back when its image is sent or its load ends.
pub trait Staging {
    fn free(&mut self, buf: *const u8);
}

/// What one pass of `step` found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advance {
    /// Nothing to settle: idle, waiting for the DSP, or a step done.
    Pending,
    /// The load in progress failed with this code. The caller takes the module down, then calls `failed`.
    Failed(i32),
    /// Every row of the load in progress is in. The caller lists the module, then calls `loaded`.
    Loaded,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Send,
    Rows,
    /// A request is on the wire that reads the staging memory: poll until it ends, never free before.
    Quarantine,
    /// Nothing is touched until a reboot.
    Retained,
}

/// The stream of the loads of one DSP: the image through a bulk write, then the rows (one sequence each), then a service file's slots last, one load at a time with the next ones queued.
/// Each pass of `step` moves the load in progress by what the link queue has ready; nothing blocks.
pub struct Transfer {
    phase: Phase,
    ticks: u32,
    step: u32,
    ticket: u32,
    row_steps: u32,
    slot_run: u32,
    current: Option<Job>,
    waiting: [Option<Job>; REC_MAX],
    started: u32,
    row_writes: u32,
    failures: u32,
}

/// Give a job's staging memory back; a job whose image was sent holds none.
fn release(staging: &mut dyn Staging, buf: *const u8) {
    if !buf.is_null() {
        staging.free(buf);
    }
}

impl Transfer {
    pub const fn new() -> Transfer {
        Transfer {
            phase: Phase::Idle,
            ticks: 0,
            step: 0,
            ticket: 0,
            row_steps: 0,
            slot_run: 0,
            current: None,
            waiting: [None; REC_MAX],
            started: 0,
            row_writes: 0,
            failures: 0,
        }
    }

    /// Loads started, row words written and loads failed since the start.
    pub fn counts(&self) -> (u32, u32, u32) {
        (self.started, self.row_writes, self.failures)
    }

    /// Is a load in progress or queued?
    pub fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Hand a load over: it starts now when the transfer is idle, else it waits behind the others. After `retain` nothing is taken.
    pub fn launch(&mut self, job: Job) {
        if self.phase == Phase::Retained {
            return;
        }
        if self.phase == Phase::Idle {
            self.start(job);
        } else {
            let slot = self.waiting.iter_mut().find(|w| w.is_none());
            if let Some(slot) = slot {
                *slot = Some(job);
            }
        }
    }

    /// Stop touching anything until a reboot (a transfer or a clear that could not be settled).
    pub fn retain(&mut self) {
        self.phase = Phase::Retained;
    }

    pub fn retained(&self) -> bool {
        self.phase == Phase::Retained
    }

    /// The load of record `rec` is the one in progress.
    pub fn is_current(&self, rec: usize) -> bool {
        self.phase != Phase::Idle && self.current.is_some_and(|j| j.rec == rec)
    }

    /// Take the load of `rec` out of the queue behind the one in progress (its staging goes back). False when `rec` is not waiting; the load in progress is not touched here.
    pub fn dequeue(&mut self, rec: usize, staging: &mut dyn Staging) -> bool {
        let Some(i) = self
            .waiting
            .iter()
            .position(|w| w.is_some_and(|j| j.rec == rec))
        else {
            return false;
        };
        if let Some(j) = self.waiting[i].take() {
            release(staging, j.buf);
        }
        // keep the order of the others: close the gap
        let mut k = i + 1;
        while k < REC_MAX {
            self.waiting[k - 1] = self.waiting[k].take();
            k += 1;
        }
        true
    }

    /// The caller took the failed load's module down (`reclaimed`: it did; else the transfer stays retained) and the failure is over.
    pub fn failed(&mut self, reclaimed: bool, staging: &mut dyn Staging) {
        self.failures += 1;
        if !reclaimed {
            self.phase = Phase::Retained;
            return;
        }
        if let Some(j) = self.current.take() {
            release(staging, j.buf);
        }
        self.next_job();
    }

    /// The caller listed the loaded module: start the next waiting load or go idle.
    pub fn loaded(&mut self) {
        self.next_job();
    }

    /// Advance the load in progress by what is ready now. Called once per pass of the UI loop.
    pub fn step(
        &mut self,
        link: &mut dyn LinkQueue,
        staging: &mut dyn Staging,
        rows: &mut dyn Rows,
    ) -> Advance {
        if self.phase == Phase::Idle || self.phase == Phase::Retained {
            return Advance::Pending;
        }
        self.ticks = self.ticks.saturating_add(1);
        if self.phase == Phase::Quarantine {
            if self.result(link) != LinkResult::Pending {
                self.ticket = 0;
                return Advance::Failed(E_DSP_XFER);
            }
            return Advance::Pending;
        }
        if self.ticks > GUARD_TICKS {
            if self.ticket != 0 && self.result(link) == LinkResult::Pending {
                self.phase = Phase::Quarantine;
                return Advance::Pending;
            }
            self.ticket = 0;
            return Advance::Failed(E_DSP_XFER);
        }
        loop {
            if self.phase == Phase::Send {
                if let Some(a) = self.send(link, staging) {
                    return a;
                }
            } else if self.phase == Phase::Rows {
                if let Some(a) = self.rows(link, rows) {
                    return a;
                }
            } else {
                return Advance::Pending;
            }
        }
    }

    fn start(&mut self, job: Job) {
        self.current = Some(job);
        self.phase = Phase::Send;
        self.ticks = 0;
        self.step = 0;
        self.ticket = 0;
        self.slot_run = 0;
        self.started += 1;
    }

    fn next_job(&mut self) {
        if self.phase == Phase::Retained {
            return;
        }
        match self.waiting[0].take() {
            Some(next) => {
                let mut k = 1;
                while k < REC_MAX {
                    self.waiting[k - 1] = self.waiting[k].take();
                    k += 1;
                }
                self.start(next);
            }
            None => {
                self.current = None;
                self.phase = Phase::Idle;
            }
        }
    }

    /// The result of the request in flight (`Gone` without one).
    fn result(&mut self, link: &mut dyn LinkQueue) -> LinkResult {
        if self.ticket == 0 {
            return LinkResult::Gone;
        }
        link.result(Ticket {
            owner: Owner::Loader,
            id: self.ticket as u16,
        })
    }

    /// One pass of the image send. `None`: go round the loop again in the next phase.
    fn send(&mut self, link: &mut dyn LinkQueue, staging: &mut dyn Staging) -> Option<Advance> {
        let job = self.current?;
        if self.ticket == 0 {
            let stream = Stream::Bulk {
                dst: job.base,
                src: job.buf,
                len: job.len,
            };
            if let Some(t) = link.submit(Owner::Loader, &stream) {
                self.ticket = t.id as u32;
            }
            return Some(Advance::Pending);
        }
        let r = self.result(link);
        if r == LinkResult::Pending {
            return Some(Advance::Pending);
        }
        self.ticket = 0;
        if r != LinkResult::Done {
            return Some(Advance::Failed(E_DSP_XFER));
        }
        staging.free(job.buf);
        if let Some(j) = self.current.as_mut() {
            j.buf = core::ptr::null();
        }
        self.phase = Phase::Rows;
        self.step = 0;
        self.ticks = 0;
        None
    }

    /// One pass of the row writes. `None`: go round the loop again.
    fn rows(&mut self, link: &mut dyn LinkQueue, rows: &mut dyn Rows) -> Option<Advance> {
        let rec = self.current?.rec;
        if self.ticket == 0 {
            // the next row of the module, in claim order, as one sequence (the last write publishes it)
            let mut seq = Sequence::new();
            let mut n = 0u32;
            loop {
                let mut want = self.step + n;
                let Some(w) = rows.row_step(rec, &mut want) else {
                    break;
                };
                if w.has_undo {
                    seq.push_undo(w.win, w.word, w.value, w.undo);
                } else {
                    seq.push(w.win, w.word, w.value);
                }
                if let Some(flag) = w.published {
                    seq.set_mark(flag as *mut u8, n as usize);
                }
                n += 1;
                if w.last || n as usize >= SEQ_MAX {
                    break;
                }
            }
            if n == 0 {
                // a service file's DSP code is acknowledged and it has no more rows: its slots are written last, as one sequence
                if self.slot_run == 0 {
                    self.slot_run = 1;
                    if let Some(seq) = rows.slots(rec) {
                        if let Some(t) = link.submit(Owner::Loader, &Stream::Sequence(seq)) {
                            self.ticket = t.id as u32;
                            self.row_steps = 0;
                            return Some(Advance::Pending);
                        }
                        self.slot_run = 0;
                        return Some(Advance::Pending);
                    }
                }
                return Some(Advance::Loaded);
            }
            if let Some(t) = link.submit(Owner::Loader, &Stream::Sequence(seq)) {
                self.ticket = t.id as u32;
                self.row_steps = n;
            }
            return Some(Advance::Pending);
        }
        let r = self.result(link);
        if r == LinkResult::Pending {
            return Some(Advance::Pending);
        }
        self.ticket = 0;
        if r != LinkResult::Done {
            return Some(Advance::Failed(E_DSP_XFER));
        }
        self.step += self.row_steps;
        self.row_writes += self.row_steps;
        self.ticks = 0;
        None
    }
}

#[cfg(test)]
mod tests;
