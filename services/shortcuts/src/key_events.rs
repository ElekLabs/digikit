//! `KeyEvents`: the seam through which the service listens to the panel's keys. The platform (`core/input`) walks the key events of the firmware once, in the UI task, and offers each
//! to its subscribers; the service file holds the routine and the implementation over the image's table (`input_keys_info`), a unit test holds [`crate::fake_keys::FakeKeys`].
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::Status;

/// The subscription of the service's key routine. The service subscribes once, when it starts, and unsubscribes when it stops; both run in the UI task, the task of the key walk,
/// and the routine runs no module code, so the walk is never inside it when the service goes down.
///
/// # Safety
/// `IDLE` is all-zero bytes: the service's state is made by a fill with zero (`Provider::start_in`), and an implementation is unsubscribed in that state.
pub unsafe trait KeyEvents {
    /// The unsubscribed state (all-zero bytes).
    const IDLE: Self;

    /// Put the routine in the platform's table. A refusal (no table in the image, a full table, the routine is subscribed already) leaves nothing subscribed; the service then does not start.
    fn subscribe<C: CoreApi>(&mut self, core: &C) -> Result<(), Status>;

    /// Take the routine out of the table. Idempotent: nothing happens when it is not subscribed.
    fn unsubscribe(&mut self);
}
