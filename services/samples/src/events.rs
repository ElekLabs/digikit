//! The events the sample-pool adapter delivers to the service, and how the owner takes one.
//!
//! The adapter's `dkpool_tick` (called first in `Tracks::tick`, which holds `&mut Samples`) fires `pool_done` and `pool_lost`. They reach the owner through the file's cell
//! as events (`Owner::post`): inside the tick they are queued and run after it returns, with the owner free again; outside a call they run at once.
use crate::samples::Samples;
use crate::tracks::{CLIENTS, ENTRIES};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Reentry;

/// One pool event, as the adapter gave it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolEvent {
    /// Load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
    Done { req: u32, result: i32 },
    /// The backing was lost: all handles are invalid.
    Lost,
}

impl<C: CoreApi> Reentry<C> for Samples {
    type Event = PoolEvent;
    /// One completion for each request that can be in flight (every entry of every client), and the loss.
    const QUEUE: usize = CLIENTS * ENTRIES + 1;

    fn apply(&mut self, core: &C, event: PoolEvent) {
        match event {
            PoolEvent::Done { req, result } => self.pool_done(req, result),
            PoolEvent::Lost => self.pool_lost(core),
        }
    }
}
