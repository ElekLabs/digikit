//! A read-only query call. The whole device has ONE call in flight: one request buffer and one outcome buffer. The MIDI handler queues
//! a call only when the slot is idle; the UI task runs the module's function; the poll verb of the same client hands the outcome over
//! and frees the slot. An outcome nobody polls is freed after `OUTCOME_BLOCKS` (2 s), so a client that went away cannot block the
//! device. While a call is queued, running or unread, a call from any module answers busy, so no result is lost to another module.

use super::query_table::CallFn;
use super::{take_word, E_BUSY, E_MSG};
use dkrt_abi::command::REPLY_BODY_MAX;
use dkrt_sdk::proto::{
    E_MODULE_UI_METHOD, E_MODULE_UI_TOO_BIG, QUERY_REPLY_MAX, QUERY_REQUEST_MAX,
};
use dkrt_sdk::{E_NOT_FOUND, OK};

/// The most request bytes of one call, after the envelope (`[protocol.limits]` of `sdk/sdk.toml`).
pub(super) const REQUEST_MAX: usize = QUERY_REQUEST_MAX;
/// The most reply bytes of one call, after the poll header (`[protocol.limits]`).
pub(super) const REPLY_MAX: usize = QUERY_REPLY_MAX;
/// The poll header: the state byte, the runtime status word and the handler result word.
const HEADER: usize = 9;
/// The call envelope: verb, module, instance, request id, method.
const ENVELOPE: usize = 1 + 1 + 4 + 4 + 1;
/// The command body of the MIDI transport; the reply body's first byte is the status.
const REQUEST_BODY_MAX: usize = 160;
const _: () = assert!(ENVELOPE + REQUEST_MAX <= REQUEST_BODY_MAX);
const _: () = assert!(1 + HEADER + REPLY_MAX <= REPLY_BODY_MAX);

/// How long an unread outcome is kept: 2 s of DSP blocks (the settings writer uses the same delay) or, without the counter, UI passes
/// (about 30 a second).
pub(super) const OUTCOME_BLOCKS: u32 = 3000;
pub(super) const OUTCOME_PASSES: u32 = 60;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Queued,
    Running,
    /// Finished; the outcome waits for its poll.
    Done,
}

/// The one call of the device: who asked, the request, and the outcome.
pub(super) struct CallSlot {
    state: State,
    rec: u8,
    method: u8,
    request_len: u8,
    instance: u32,
    ticket: u32,
    /// When the call finished, in the clock of `Registry::age_out`.
    done_at: u32,
    runtime: i32,
    handler: i32,
    reply_len: u16,
    request: [u8; REQUEST_MAX],
    reply: [u8; REPLY_MAX],
}

impl CallSlot {
    pub(super) const IDLE: Self = Self {
        state: State::Idle,
        rec: 0,
        method: 0,
        request_len: 0,
        instance: 0,
        ticket: 0,
        done_at: 0,
        runtime: OK,
        handler: OK,
        reply_len: 0,
        request: [0; REQUEST_MAX],
        reply: [0; REPLY_MAX],
    };

    fn is(&self, rec: usize, instance: u32, ticket: u32) -> bool {
        self.state != State::Idle
            && self.rec as usize == rec
            && self.instance == instance
            && self.ticket == ticket
    }

    fn request(&self) -> &[u8] {
        &self.request[..self.request_len as usize]
    }

    /// `[0][runtime status][handler result][reply bytes]`
    fn encode(&self, out: &mut [u8]) -> usize {
        let len = self.reply_len as usize;
        out[0] = 0;
        out[1..5].copy_from_slice(&self.runtime.to_be_bytes());
        out[5..HEADER].copy_from_slice(&self.handler.to_be_bytes());
        out[HEADER..HEADER + len].copy_from_slice(&self.reply[..len]);
        HEADER + len
    }
}

/// A queued call the UI task is about to run. The module's function reads the registry's request buffer and writes its reply buffer
/// unlocked: the slot is `Running`, so nothing else touches either.
pub(super) struct CallJob {
    rec: usize,
    instance: u32,
    ticket: u32,
    len: u8,
    call: CallFn,
    request: *const u8,
    reply: *mut u8,
}

/// What a call left: the runtime's status, the handler's result and the reply length.
pub(super) struct CallResult {
    pub(super) runtime: i32,
    pub(super) handler: i32,
    pub(super) len: usize,
}

impl CallResult {
    const fn refused(runtime: i32) -> Self {
        Self {
            runtime,
            handler: OK,
            len: 0,
        }
    }
}

impl CallJob {
    pub(super) fn rec(&self) -> usize {
        self.rec
    }

    /// Call the module's function with `ctx`. The runtime status is `OK` whenever the function ran (except for a reply length it
    /// refuses); the function's own return value is only the handler result.
    pub(super) unsafe fn run(&self, ctx: *mut u8) -> CallResult {
        let mut out_len = REPLY_MAX as u32;
        let handler = (self.call)(ctx, self.request, self.len as u32, self.reply, &mut out_len);
        if handler != OK {
            CallResult {
                runtime: OK,
                handler,
                len: 0,
            }
        } else if out_len as usize > REPLY_MAX {
            CallResult::refused(E_MODULE_UI_TOO_BIG)
        } else {
            CallResult {
                runtime: OK,
                handler: OK,
                len: out_len as usize,
            }
        }
    }
}

impl super::Registry {
    /// Queue `[ticket u32][method u8][request]` of module `rec`. The MIDI handler only checks the envelope; the module's function runs
    /// on the UI task. The same call again is accepted; any other call is busy while the slot is in use.
    pub(super) fn queue_call(
        &mut self,
        rec: usize,
        tail: &[u8],
        draining: bool,
    ) -> Result<usize, i32> {
        let mut rest = tail;
        let ticket = take_word(&mut rest)?;
        let (&method, request) = rest.split_first().ok_or(E_MSG)?;
        if ticket == 0 || request.len() > REQUEST_MAX {
            return Err(E_MSG);
        }
        let instance = self.modules[rec].instance;
        if self.call.is(rec, instance, ticket) {
            let same = self.call.method == method && self.call.request() == request;
            return if same { Ok(0) } else { Err(E_MSG) };
        }
        if draining || self.call.state != State::Idle {
            return Err(E_BUSY);
        }
        if self.modules[rec].queries.find(method).is_none() {
            return Err(E_MODULE_UI_METHOD);
        }
        let call = &mut self.call;
        call.state = State::Queued;
        call.rec = rec as u8;
        call.instance = instance;
        call.ticket = ticket;
        call.method = method;
        call.request_len = request.len() as u8;
        call.request[..request.len()].copy_from_slice(request);
        Ok(0)
    }

    /// Take the queued call. A call whose instance is no longer the loaded one, or whose method is gone, finishes with its own error
    /// at once. A call of a draining module stays queued (and the slot busy) until the module unloads or the drain ends.
    pub(super) fn start_call(
        &mut self,
        is_draining: impl Fn(usize) -> bool,
        now: u32,
    ) -> Option<CallJob> {
        if self.call.state != State::Queued || is_draining(self.call.rec as usize) {
            return None;
        }
        let rec = self.call.rec as usize;
        let function = self.modules[rec].queries.find(self.call.method);
        let current = self.modules[rec].matches(self.call.instance);
        let job = CallJob {
            rec,
            instance: self.call.instance,
            ticket: self.call.ticket,
            len: self.call.request_len,
            call: function.unwrap_or(missing),
            request: self.call.request.as_ptr(),
            reply: self.call.reply.as_mut_ptr(),
        };
        let refusal = if !current {
            Some(E_NOT_FOUND)
        } else if function.is_none() {
            Some(E_MODULE_UI_METHOD)
        } else {
            None
        };
        match refusal {
            Some(status) => {
                self.finish_call(&job, CallResult::refused(status), now);
                None
            }
            None => {
                self.call.state = State::Running;
                Some(job)
            }
        }
    }

    /// Keep the result for the poll. Ignored unless `job` is still the call of the slot (module, instance and ticket).
    pub(super) fn finish_call(&mut self, job: &CallJob, result: CallResult, now: u32) {
        let call = &mut self.call;
        if !call.is(job.rec, job.instance, job.ticket) {
            return;
        }
        call.state = State::Done;
        call.done_at = now;
        call.runtime = result.runtime;
        call.handler = result.handler;
        call.reply_len = result.len as u16;
    }

    /// The poll fallback: the queued or finished call of `ticket` for module `rec`. A finished call is handed over once and freed.
    pub(super) fn poll_call(
        &mut self,
        rec: usize,
        instance: u32,
        ticket: u32,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let call = &mut self.call;
        if !call.is(rec, instance, ticket) {
            return Err(E_NOT_FOUND);
        }
        if call.state == State::Done {
            let n = call.encode(out);
            call.state = State::Idle;
            Ok(n)
        } else {
            out[0] = 1;
            Ok(1)
        }
    }

    /// Free an outcome nobody polled for `limit` (in the unit of `now`).
    pub(super) fn age_out(&mut self, now: u32, limit: u32) {
        if self.call.state == State::Done && now.wrapping_sub(self.call.done_at) >= limit {
            self.call.state = State::Idle;
        }
    }

    /// Module `rec` unloads: its queued call or held outcome goes before its code is released. A call is never running then: unload
    /// runs on the UI task between UI passes, and a call runs inside one (`tick`).
    pub(super) fn cancel_calls(&mut self, rec: usize) {
        if self.call.state != State::Idle && self.call.rec as usize == rec {
            self.call.state = State::Idle;
        }
    }
}

unsafe extern "C" fn missing(_: *mut u8, _: *const u8, _: u32, _: *mut u8, _: *mut u32) -> i32 {
    E_MODULE_UI_METHOD
}

#[cfg(test)]
mod tests;
