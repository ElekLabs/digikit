//! `Writes`: the table writes of the track tables over the DSP link.
//!
//! The DSP link is write-only, so close, drain and publish are queued writes, one in flight at a time (`pump`): each write is a request of the owner `Tracks` on the
//! core's link queue (its bytes are copied there), and a start needs an idle loader.
//!   close:   control CLOSED = 1, then record `ready` = 0. The DSP holds a track only between ACQUIRE and RELEASE inside one `render`
//!            call, so a render that was inside when CLOSED landed ends within that block. The second write lands in a later block than
//!            the first (one transfer at a time, each acknowledged), so its acknowledgement drains the track: no COUNT read-back
//!            exists.
//!   publish: record words 1..7 (token, frames, rate, channels, fmt, base_l, base_r), control CLOSED = 0, then `ready` = 1 last.
//! A write the link refuses is retried; after `TRIES` failures the op stays failed (a close then never drains and the generic drain
//! guard of `sample_tracks` ends the request; a publish stays unpublished, `ready` = 0, the track plays silence).
//! A sticky DSP FAULT (poisoned COUNT, CLOSED = 1) is never cleared by the runtime: that track stays silent until the module reloads.
//!
//! All-zero bytes are the initial state (no op, nothing in flight, no client live), so the owner makes it where it lives with one fill.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::PH_IDLE;
use dkrt_abi::link::{LinkResult, Owner, Stream, Ticket};
use dkrt_sdk::sample_tracks::{Strack, TRACKS};

use crate::tracks::CLIENTS;

const OP_NONE: u8 = 0;
const OP_CLOSE: u8 = 1;
const OP_PUBLISH: u8 = 2;
const OP_FAILED: u8 = 3;
/// Refusals of one write before the op is given up.
const TRIES: u8 = 8;

#[derive(Clone, Copy)]
struct Op {
    kind: u8,
    step: u8,
    tries: u8,
    /// The record to publish (`Strack` words in order).
    rec: [u32; 8],
}
const OP0: Op = Op {
    kind: OP_NONE,
    step: 0,
    tries: 0,
    rec: [0; 8],
};

/// Has the op written everything?
fn complete(op: &Op) -> bool {
    (op.kind == OP_CLOSE && op.step >= 2) || (op.kind == OP_PUBLISH && op.step >= 3)
}

pub(crate) struct Writes {
    ops: [[Op; TRACKS]; CLIENTS],
    /// The write in flight: client * TRACKS + track + 1 (0: none), and the id of its request.
    flight: usize,
    flight_id: u16,
    /// Whether the DSP image of each client's module is in (table writes may start).
    live: [bool; CLIENTS],
    /// The client slot + 1 of a module that is taken down (0: none): nothing reads its tracks any more, so close drains at once and writes are dropped. Other clients are
    /// stepped normally meanwhile.
    force: usize,
    /// The L2 byte addresses of the 16 records and the 16 control sets of each client's table.
    records: [u32; CLIENTS],
    control: [u32; CLIENTS],
}

impl Writes {
    pub(crate) const fn new() -> Writes {
        Writes {
            ops: [[OP0; TRACKS]; CLIENTS],
            flight: 0,
            flight_id: 0,
            live: [false; CLIENTS],
            force: 0,
            records: [0; CLIENTS],
            control: [0; CLIENTS],
        }
    }

    /// A client slot got its table at `records` and `control`: nothing is written before `ready`.
    pub(crate) fn reserve(&mut self, client: usize, records: u32, control: u32) {
        self.records[client] = records;
        self.control[client] = control;
        self.live[client] = false;
        self.ops[client] = [OP0; TRACKS];
    }

    /// The module's DSP image is in: queued writes may go out.
    pub(crate) fn ready(&mut self, client: usize) {
        self.live[client] = true;
    }

    pub(crate) fn is_live(&self, client: usize) -> bool {
        self.live[client]
    }

    /// The client's module is taken down: close drains at once and writes are dropped until `end_teardown`.
    pub(crate) fn begin_teardown(&mut self, client: usize) {
        self.force = client + 1;
    }

    pub(crate) fn end_teardown(&mut self) {
        self.force = 0;
    }

    /// The client slot is free: drop its ops and stop writing for it. A write already in flight ends by itself (its bytes are copied into the request, and the link is
    /// serialized, so a later load of the range lands after it).
    pub(crate) fn release(&mut self, client: usize) {
        self.ops[client] = [OP0; TRACKS];
        self.live[client] = false;
    }

    /// The table words of a client (records, control), as the last `reserve` set them.
    #[cfg(test)]
    pub(crate) fn table(&self, client: usize) -> (u32, u32) {
        (self.records[client], self.control[client])
    }

    /// CLOSE the track generation. A publish still queued is dropped: the close writes CLOSED and `ready` = 0 anyway. Before the DSP image is in there is nothing to close.
    pub(crate) fn close(&mut self, client: usize, track: usize) {
        let live = self.live[client] && self.force != client + 1;
        let op = &mut self.ops[client][track];
        *op = OP0;
        if live {
            op.kind = OP_CLOSE;
        }
    }

    /// The close has landed (or the module is taken down).
    pub(crate) fn drained(&self, client: usize, track: usize) -> bool {
        self.force == client + 1 || self.ops[client][track].kind == OP_NONE
    }

    /// The publish writes have all landed: 1; still on their way: 0; given up: `E_DSP_XFER`.
    pub(crate) fn published(&self, client: usize, track: usize) -> i32 {
        let kind = self.ops[client][track].kind;
        if kind == OP_FAILED {
            return dkrt_sdk::E_DSP_XFER;
        }
        if kind == OP_NONE {
            1
        } else {
            0
        }
    }

    /// Queue the record of a track for writing (dropped for a module that is taken down).
    pub(crate) fn publish(&mut self, client: usize, track: usize, rec: &Strack) {
        if self.force == client + 1 {
            return;
        }
        let op = &mut self.ops[client][track];
        *op = OP0;
        op.kind = OP_PUBLISH;
        op.rec = [
            rec.ready,
            rec.token,
            rec.frames,
            rec.rate,
            rec.channels,
            rec.fmt,
            rec.base_l,
            rec.base_r,
        ];
    }

    /// The next write of an op: (destination L2 byte address, the words, their count). None when the op is complete.
    /// If chains, not a `match`: dense arms become a PC-relative jump table, which is not ColdFire code.
    fn next_write(&self, c: usize, k: usize, op: &Op) -> Option<(u32, [u32; 7], usize)> {
        let rec = self.records[c] + 32 * k as u32;
        let ctl = self.control[c] + 16 * k as u32;
        let mut w = [0u32; 7];
        if op.kind == OP_CLOSE {
            if op.step == 0 {
                w[0] = 1;
                return Some((ctl, w, 1));
            }
            if op.step == 1 {
                return Some((rec, w, 1));
            }
        } else if op.kind == OP_PUBLISH {
            if op.step == 0 {
                let mut i = 0;
                while i < 7 {
                    w[i] = op.rec[1 + i];
                    i += 1;
                }
                return Some((rec + 4, w, 7));
            }
            if op.step == 1 {
                return Some((ctl, w, 1));
            }
            if op.step == 2 {
                w[0] = op.rec[0];
                return Some((rec, w, 1));
            }
        }
        None
    }

    /// Finish the write in flight, then start the next one. Called twice per UI-loop pass.
    ///
    /// # Safety
    /// The core's link calls (`CoreApi`).
    pub(crate) unsafe fn pump<C: CoreApi>(&mut self, api: &C) {
        if self.flight != 0 {
            let done = api.link_result(Ticket {
                owner: Owner::Tracks,
                id: self.flight_id,
            });
            if done == LinkResult::Pending {
                return;
            }
            let (c, k) = ((self.flight - 1) / TRACKS, (self.flight - 1) % TRACKS);
            self.flight = 0;
            let op = &mut self.ops[c][k];
            if done == LinkResult::Done {
                op.step += 1;
                op.tries = 0;
            } else {
                op.tries += 1;
            }
        }
        let mut c = 0;
        while c < CLIENTS {
            let mut k = 0;
            while k < TRACKS {
                let op = &mut self.ops[c][k];
                if op.kind == OP_CLOSE || op.kind == OP_PUBLISH {
                    if op.tries >= TRIES {
                        op.kind = OP_FAILED;
                    } else if complete(op) {
                        op.kind = OP_NONE;
                    }
                }
                k += 1;
            }
            c += 1;
        }
        // a table write waits for the loader: the rows of a module that loads are not in the DSP yet
        let d = api.dsp();
        if d.phase != PH_IDLE || d.nwait != 0 {
            return;
        }
        c = 0;
        while c < CLIENTS {
            let mut k = 0;
            while k < TRACKS {
                let op = self.ops[c][k];
                if (op.kind == OP_CLOSE || op.kind == OP_PUBLISH) && self.live[c] {
                    if let Some((dst, words, n)) = self.next_write(c, k, &op) {
                        if let Some(stream) = Stream::owned(0, dst, &words[..n]) {
                            if let Some(t) = api.link_submit(Owner::Tracks, &stream) {
                                self.flight = c * TRACKS + k + 1;
                                self.flight_id = t.id;
                            }
                        }
                        return;
                    }
                }
                k += 1;
            }
            c += 1;
        }
    }
}

#[cfg(test)]
mod tests;
