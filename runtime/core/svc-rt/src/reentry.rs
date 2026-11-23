//! `Reentry`: the events a service takes while one of its own calls is running.
//!
//! A callback of the service (a pool event, a hook) may fire inside a call the object made (`tick`, `release`, ...). The owner is busy then and never forms a second
//! `&mut Self`: `Owner::post` queues the event, and the owner hands it to [`Reentry::apply`] right after the outer call returns, in the same task, in order. A service
//! adopts it with `parts = [.., Reentry]` in `service!`.

/// The events of a service and how the object takes one.
pub trait Reentry {
    /// One event: plain data (what the callback was given), small, copied into the queue.
    type Event: Copy;
    /// How many events wait at most while a call runs. A full queue refuses the event and counts it (`Owner::refused`).
    const QUEUE: usize;
    /// Take `event`: runs after the call it arrived in, with the object free again.
    fn apply(&mut self, event: Self::Event);
}
