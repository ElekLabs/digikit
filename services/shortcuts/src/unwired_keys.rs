//! `UnwiredKeys`: a subscription that reaches no platform: the owner's `KeyEvents` in a unit test and in a host build of the service file (no image, no `core/input`). It subscribes
//! nothing and counts the calls, so a test sees that the service subscribed when it started and unsubscribed when it stopped, and gives the refusal of a full or missing table as a
//! type: `UnwiredKeys<REFUSAL>` answers `REFUSAL` (a non-zero status) to every subscribe, so the owner started over it refuses to start.
use crate::key_events::KeyEvents;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;

/// Counts subscribes and unsubscribes; all-zero bytes are the unsubscribed state. `REFUSAL` is the status every subscribe answers; 0 (the default) accepts.
pub struct UnwiredKeys<const REFUSAL: Status = 0> {
    subscribed: bool,
    subscribes: u32,
    unsubscribes: u32,
}

impl<const REFUSAL: Status> UnwiredKeys<REFUSAL> {
    /// The routine is subscribed now.
    pub fn subscribed(&self) -> bool {
        self.subscribed
    }

    /// Accepted and refused subscribe calls so far.
    pub fn subscribes(&self) -> u32 {
        self.subscribes
    }

    /// Unsubscribe calls so far, those that found nothing subscribed too.
    pub fn unsubscribes(&self) -> u32 {
        self.unsubscribes
    }
}

// SAFETY: a bool and counters: all-zero bytes are the idle state.
unsafe impl<const REFUSAL: Status> KeyEvents for UnwiredKeys<REFUSAL> {
    const IDLE: UnwiredKeys<REFUSAL> = UnwiredKeys {
        subscribed: false,
        subscribes: 0,
        unsubscribes: 0,
    };

    fn subscribe<C: CoreApi>(&mut self, _core: &C) -> Result<(), Status> {
        self.subscribes += 1;
        if REFUSAL != 0 {
            return Err(REFUSAL);
        }
        self.subscribed = true;
        Ok(())
    }

    fn unsubscribe(&mut self) {
        self.unsubscribes += 1;
        self.subscribed = false;
    }
}
