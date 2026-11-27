//! A read-only query call: the MIDI handler queues one call per module, the UI task runs the module's function with a reply buffer,
//! and the outcome waits for the poll verb. Reads are idempotent, so the runtime keeps one outcome (the latest) for all modules and a
//! client asks again after a lost reply.

use super::query_table::CallFn;
use super::{take_word, Enrollment, E_BUSY, E_MSG};
use dkrt_abi::command::REPLY_BODY_MAX;
use dkrt_sdk::proto::{E_MODULE_UI_METHOD, E_MODULE_UI_TOO_BIG};
use dkrt_sdk::{E_NOT_FOUND, OK};

/// The most request bytes of one call.
pub(super) const REQUEST_MAX: usize = 160;
/// The poll reply header: the state byte and the status word. The reply body of the transport holds a status byte before it.
const HEADER: usize = 5;
/// The most reply bytes of one call.
pub(super) const REPLY_MAX: usize = REPLY_BODY_MAX - 1 - HEADER;
const NO_REC: u8 = u8::MAX;

#[derive(Clone, Copy)]
pub(super) struct PendingCall {
    ticket: u32,
    instance: u32,
    method: u8,
    len: u8,
    bytes: [u8; REQUEST_MAX],
}

impl PendingCall {
    pub(super) const NONE: Self = Self {
        ticket: 0,
        instance: 0,
        method: 0,
        len: 0,
        bytes: [0; REQUEST_MAX],
    };

    pub(super) fn queued(&self) -> bool {
        self.ticket != 0
    }
}

/// What the latest finished call left: its owner, ticket, status and reply.
pub(super) struct CallOutcome {
    rec: u8,
    ticket: u32,
    status: i32,
    len: u16,
    bytes: [u8; REPLY_MAX],
}

impl CallOutcome {
    pub(super) const EMPTY: Self = Self {
        rec: NO_REC,
        ticket: 0,
        status: OK,
        len: 0,
        bytes: [0; REPLY_MAX],
    };

    /// Forget the outcome of module `rec` (it unloaded) or, with `NO_REC`, whichever is held.
    pub(super) fn forget(&mut self, rec: usize) {
        if self.rec as usize == rec || rec == NO_REC as usize {
            self.rec = NO_REC;
        }
    }

    fn encode(&self, out: &mut [u8]) -> usize {
        let len = self.len as usize;
        out[0] = 0;
        out[1..HEADER].copy_from_slice(&self.status.to_be_bytes());
        out[HEADER..HEADER + len].copy_from_slice(&self.bytes[..len]);
        HEADER + len
    }
}

impl Enrollment {
    /// Queue `[ticket u32][method u8][request]`. The MIDI handler only checks the envelope; the module's function runs on the UI task.
    pub(super) fn queue_call(&mut self, tail: &[u8], draining: bool) -> Result<usize, i32> {
        let mut rest = tail;
        let ticket = take_word(&mut rest)?;
        let (&method, request) = rest.split_first().ok_or(E_MSG)?;
        if ticket == 0 || request.len() > REQUEST_MAX {
            return Err(E_MSG);
        }
        if self.call.ticket == ticket {
            let same =
                self.call.method == method && &self.call.bytes[..self.call.len as usize] == request;
            return if same { Ok(0) } else { Err(E_MSG) };
        }
        if draining || self.call.queued() {
            return Err(E_BUSY);
        }
        if self.queries.find(method).is_none() {
            return Err(E_MODULE_UI_METHOD);
        }
        let mut call = PendingCall {
            ticket,
            instance: self.instance,
            method,
            len: request.len() as u8,
            bytes: [0; REQUEST_MAX],
        };
        call.bytes[..request.len()].copy_from_slice(request);
        self.call = call;
        Ok(0)
    }

    /// The pending state of `ticket`, when it is this module's queued call.
    pub(super) fn poll_call(&self, ticket: u32, out: &mut [u8]) -> Option<usize> {
        (self.call.ticket == ticket).then(|| {
            out[0] = 1;
            1
        })
    }
}

/// A queued call the UI task is about to run, with copies so the module's function runs unlocked.
pub(super) struct CallJob {
    rec: usize,
    ticket: u32,
    len: u8,
    bytes: [u8; REQUEST_MAX],
    call: CallFn,
    out: *mut u8,
}

impl CallJob {
    pub(super) fn rec(&self) -> usize {
        self.rec
    }

    /// Call the module's function with `ctx`; the reply goes into the registry's outcome buffer. Returns (status, reply length).
    pub(super) unsafe fn run(&self, ctx: *mut u8) -> (i32, usize) {
        let mut out_len = REPLY_MAX as u32;
        let status = (self.call)(
            ctx,
            self.bytes.as_ptr(),
            self.len as u32,
            self.out,
            &mut out_len,
        );
        if status != OK {
            (status, 0)
        } else if out_len as usize > REPLY_MAX {
            (E_MODULE_UI_TOO_BIG, 0)
        } else {
            (OK, out_len as usize)
        }
    }
}

impl super::Registry {
    /// Take the next queued call, one module at a time in turn. A call whose instance is no longer the loaded one, or whose method
    /// is gone, finishes with its own error at once.
    pub(super) fn start_call(&mut self, is_draining: impl Fn(usize) -> bool) -> Option<CallJob> {
        let modules = self.modules.len();
        let rec = (0..modules)
            .map(|i| (self.next_call + i) % modules)
            .find(|&rec| self.modules[rec].call.queued() && !is_draining(rec))?;
        self.next_call = (rec + 1) % modules;
        let slot = &self.modules[rec];
        let pending = slot.call;
        let call = slot.queries.find(pending.method);
        let current = slot.matches(pending.instance);
        let job = CallJob {
            rec,
            ticket: pending.ticket,
            len: pending.len,
            bytes: pending.bytes,
            call: call.unwrap_or(missing),
            out: self.outcome.bytes.as_mut_ptr(),
        };
        if !current {
            self.finish_call(&job, E_NOT_FOUND, 0);
            None
        } else if call.is_none() {
            self.finish_call(&job, E_MODULE_UI_METHOD, 0);
            None
        } else {
            // the buffer is being written: no poll reads the previous outcome
            self.outcome.forget(NO_REC as usize);
            Some(job)
        }
    }

    pub(super) fn finish_call(&mut self, job: &CallJob, status: i32, len: usize) {
        let slot = &mut self.modules[job.rec];
        if slot.call.ticket != job.ticket {
            return;
        }
        slot.call = PendingCall::NONE;
        self.outcome.rec = job.rec as u8;
        self.outcome.ticket = job.ticket;
        self.outcome.status = status;
        self.outcome.len = len as u16;
    }

    /// The poll fallback: the queued or finished call of `ticket` for module `rec`.
    pub(super) fn poll_call(&self, rec: usize, ticket: u32, out: &mut [u8]) -> Result<usize, i32> {
        if let Some(n) = self.modules[rec].poll_call(ticket, out) {
            Ok(n)
        } else if self.outcome.rec as usize == rec && self.outcome.ticket == ticket {
            Ok(self.outcome.encode(out))
        } else {
            Err(E_NOT_FOUND)
        }
    }
}

unsafe extern "C" fn missing(_: *mut u8, _: *const u8, _: u32, _: *mut u8, _: *mut u32) -> i32 {
    E_MODULE_UI_METHOD
}

#[cfg(test)]
mod tests;
