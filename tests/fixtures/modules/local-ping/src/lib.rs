//! The fixture of a service that provides an API of its own (`local_ping_value`) and the whole provider contract of RC3: an owner object with the lifecycle part (it counts
//! the consumers that attach and detach), the tick part (it counts the passes) and the query part (it answers the counters). Its record kind, resource, hook and settings
//! fields wait for header table v4 and the Settings part. It takes one patch (`cf.filter.hook.test_list_size_patch`, the allocation size of the chooser's order vector, which the
//! port file of the test image lists on both devices): the installer writes `LIST_SIZE_PATCH` there at load and the stock `0x0018` back before `stop`.
#![no_std]

use dkrt_svc_rt::dkrt_abi::core_api::CoreApi;
use dkrt_svc_rt::dkrt_abi::provider::{Consumer, ConsumerId, Provider, Query, Status, Ticker};
use dkrt_svc_rt::dkrt_sdk::{E_NOT_FOUND, E_TOO_BIG};

/// The operation of the query: the three counters as big-endian words.
pub const OP_COUNTERS: u32 = 1;
/// The reply of `OP_COUNTERS`: attaches, detaches, ticks.
pub const COUNTERS_LEN: usize = 12;

/// The patch value: a two-byte allocation size above the stock `0x18`, so the firmware allocates more than it copies and nothing else changes.
pub const LIST_SIZE_PATCH: u32 = 0x20;

/// The owner object: what it saw since `start`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalPing {
    attached: u32,
    detached: u32,
    ticks: u32,
    /// The consumer attached last (its owner id), 0 before any.
    last: u8,
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

    /// The bytes of the `list_size` patch, computed from the started instance.
    pub fn list_size(&self) -> u32 {
        LIST_SIZE_PATCH
    }
}

impl<C: CoreApi> Provider<C> for LocalPing {
    fn start(_: &C) -> Result<LocalPing, Status> {
        Ok(LocalPing::default())
    }

    fn attach(&mut self, _: &C, consumer: &Consumer<'_>) -> Result<(), Status> {
        self.attached += 1;
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

impl Query for LocalPing {
    fn query(&self, op: u32, _: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
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
    parts = [Tick, Query],
    hooks = [],
    fills = [],
    patches = [("cf.filter.hook.test_list_size_patch", 2, LocalPing::list_size)],
    exports = [("local_ping_value", local_ping_value)]
);
