//! The fixture of a service that takes payloads (`Payloads` part, docs/design/digikit-dkm.md) and nothing else: the lifecycle part, the payloads part and the query part that
//! answers what it saw. A module that carries a body for it names the service `local-payload` and a local kind:
//!
//! - kind 1: the first byte of the body is the number of calls the install answers `Pending` before it is `Done` (0xff: the install refuses with `E_INIT`); the second byte, when
//!   there is one, is the number of calls the retire answers `Pending` for the consumer.
//! - kind 2: `Done` at once.
//! - `check` refuses another kind, an empty body, and a kind 1 body that starts with 0xfe (`E_FORMAT`).
#![no_std]

use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{ConsumerId, Payloads, Provider, Query, Status, Step};
use dkrt_svc_rt::dkrt_sdk::{E_FORMAT, E_INIT, E_NOT_FOUND, E_TOO_BIG};

/// The operation of the query: what the service saw, as big-endian words.
pub const OP_SEEN: u32 = 1;
/// The reply of `OP_SEEN`: kind 1 installed, kind 2 installed, retired, install calls (every call, pending ones too), the length of the body of the last first call,
/// the consumer (owner id) of the last first call.
pub const SEEN_LEN: usize = 24;

pub const KIND_SLOW: u16 = 1;
pub const KIND_FAST: u16 = 2;
/// The first body byte of kind 1 that makes the install refuse, and the one that makes `check` refuse.
pub const REFUSE_INSTALL: u8 = 0xff;
pub const REFUSE_CHECK: u8 = 0xfe;

/// The owner object: what it saw since `start`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalPayload {
    slow: u32,
    fast: u32,
    retired: u32,
    calls: u32,
    last_len: u32,
    last_consumer: u8,
    /// Install calls of the kind 1 payload still to answer `Pending`.
    install_left: u8,
    /// Retire calls still to answer `Pending`.
    retire_left: u8,
}

impl LocalPayload {
    pub fn slow_installed(&self) -> u32 {
        self.slow
    }
    pub fn fast_installed(&self) -> u32 {
        self.fast
    }
    pub fn retired(&self) -> u32 {
        self.retired
    }
    pub fn install_calls(&self) -> u32 {
        self.calls
    }
}

impl<C: CoreApi> Provider<C> for LocalPayload {
    fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<LocalPayload>) -> Result<(), Status> {
        slot.write(LocalPayload::default());
        Ok(())
    }
}

impl<C: CoreApi> Payloads<C> for LocalPayload {
    fn check(
        &mut self,
        _: &C,
        kind: u16,
        body: &[u8],
        _: ConsumerId,
    ) -> Result<(), Status> {
        let slow_refused = kind == KIND_SLOW && body.first() == Some(&REFUSE_CHECK);
        if (kind != KIND_SLOW && kind != KIND_FAST) || body.is_empty() || slow_refused {
            return Err(E_FORMAT);
        }
        Ok(())
    }

    fn install(
        &mut self,
        _: &C,
        kind: u16,
        body: Option<&[u8]>,
        consumer: ConsumerId,
    ) -> Result<Step, Status> {
        self.calls += 1;
        if let Some(b) = body {
            self.last_len = b.len() as u32;
            self.last_consumer = consumer.get();
            if kind == KIND_SLOW {
                if b.first() == Some(&REFUSE_INSTALL) {
                    return Err(E_INIT);
                }
                self.install_left = b.first().copied().unwrap_or(0);
                self.retire_left = b.get(1).copied().unwrap_or(0);
            }
        }
        if kind == KIND_FAST {
            self.fast += 1;
            return Ok(Step::Done);
        }
        if self.install_left > 0 {
            self.install_left -= 1;
            return Ok(Step::Pending);
        }
        self.slow += 1;
        Ok(Step::Done)
    }

    fn retire(&mut self, _: &C, _: ConsumerId) -> Step {
        if self.retire_left > 0 {
            self.retire_left -= 1;
            return Step::Pending;
        }
        self.retired += 1;
        Step::Done
    }
}

impl Query for LocalPayload {
    fn query(&self, op: u32, _: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
        if op != OP_SEEN {
            return Err(E_NOT_FOUND);
        }
        let out = reply.get_mut(..SEEN_LEN).ok_or(E_TOO_BIG)?;
        for (word, value) in out.chunks_exact_mut(4).zip([
            self.slow,
            self.fast,
            self.retired,
            self.calls,
            self.last_len,
            self.last_consumer as u32,
        ]) {
            word.copy_from_slice(&value.to_be_bytes());
        }
        Ok(SEEN_LEN)
    }
}

dkrt_svc_rt::service!(
    LocalPayload,
    "local-payload",
    &[],
    parts = [Query, Payloads],
    hooks = [],
    fills = [],
    exports = []
);
