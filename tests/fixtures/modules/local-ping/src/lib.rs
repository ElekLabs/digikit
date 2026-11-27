//! The fixture of a service that provides an API of its own (`local_ping_value`) and the whole provider contract of RC3: an owner object with the lifecycle part (it counts
//! the consumers that attach and detach), the tick part (it counts the passes), the query part (it answers the counters) and the state part (the `count` of consumers that ever
//! attached survives a reload of the service). Its record kind, resource, hook and settings
//! fields wait for header table v4 and the Settings part.
#![no_std]

use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{
    Consumer, ConsumerId, Provider, Query, State, Status, Ticker,
};
use dkrt_svc_rt::dkrt_sdk::{E_NOT_FOUND, E_RANGE, E_TOO_BIG};

/// The operation of the query: the three counters as big-endian words.
pub const OP_COUNTERS: u32 = 1;
/// The reply of `OP_COUNTERS`: attaches, detaches, ticks.
pub const COUNTERS_LEN: usize = 12;

/// The operation of the query that answers `count` (the state the part carries) as one big-endian word.
pub const OP_COUNT: u32 = 2;
/// The layout of the bytes the state part exports: the count as one big-endian word. The number is no small one so that a test tells a layout from a count.
pub const STATE_VERSION: u16 = 0x0107;

/// The owner object: what it saw since `start`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalPing {
    attached: u32,
    detached: u32,
    ticks: u32,
    /// The consumer attached last (its owner id), 0 before any.
    last: u8,
    /// The consumers that attached since the first copy of the service started: the state a reload carries.
    count: u32,
}

impl LocalPing {
    pub fn attached(&self) -> u32 {
        self.attached
    }

    pub fn detached(&self) -> u32 {
        self.detached
    }

    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    pub fn last_attached(&self) -> u8 {
        self.last
    }

    pub fn count(&self) -> u32 {
        self.count
    }
}

impl<C: CoreApi> Provider<C> for LocalPing {
    fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<LocalPing>) -> Result<(), Status> {
        slot.write(LocalPing::default());
        Ok(())
    }

    fn attach(&mut self, _: &C, consumer: &Consumer<'_>) -> Result<(), Status> {
        self.attached += 1;
        self.count += 1;
        self.last = consumer.id().get();
        Ok(())
    }

    fn detach(&mut self, _: &C, _: ConsumerId) {
        self.detached += 1;
    }
}

impl<C: CoreApi> Ticker<C> for LocalPing {
    fn tick(&mut self, _: &C, _: u32) {
        self.ticks += 1;
    }
}

impl<C: CoreApi> State<C> for LocalPing {
    const VERSION: u16 = STATE_VERSION;
    const ACCEPTS_FROM: u16 = STATE_VERSION;

    fn export_max(&self, _: &C) -> usize {
        4
    }

    fn export(&mut self, _: &C, out: &mut [u8]) -> usize {
        let Some(room) = out.get_mut(..4) else {
            return 0;
        };
        room.copy_from_slice(&self.count.to_be_bytes());
        4
    }

    fn import(&mut self, _: &C, from: Option<&[u8]>) -> Result<(), Status> {
        self.count = match from {
            None => 0,
            Some(b) => u32::from_be_bytes(b.try_into().map_err(|_| E_RANGE)?),
        };
        Ok(())
    }
}

impl Query for LocalPing {
    fn query(&self, op: u32, _: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
        if op == OP_COUNT {
            reply
                .get_mut(..4)
                .ok_or(E_TOO_BIG)?
                .copy_from_slice(&self.count.to_be_bytes());
            return Ok(4);
        }
        if op != OP_COUNTERS {
            return Err(E_NOT_FOUND);
        }
        let out = reply.get_mut(..COUNTERS_LEN).ok_or(E_TOO_BIG)?;
        for (word, value) in out
            .chunks_exact_mut(4)
            .zip([self.attached, self.detached, self.ticks])
        {
            word.copy_from_slice(&value.to_be_bytes());
        }
        Ok(COUNTERS_LEN)
    }
}

#[no_mangle]
pub extern "C" fn local_ping_value() -> u32 {
    42
}

dkrt_svc_rt::service!(
    LocalPing,
    "local-ping",
    &[],
    parts = [Tick, Query, State],
    hooks = [],
    fills = [],
    exports = [("local_ping_value", local_ping_value)]
);
