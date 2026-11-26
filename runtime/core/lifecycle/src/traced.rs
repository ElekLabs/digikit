//! The tracing wrapper of the seams: [`Traced`] holds a seam and a [`Trace`], and each call through it is announced with `Begin` before and `End` (with the status it
//! answered) after. The manager is built over wrapped seams and is unchanged; with [`NoTrace`](crate::NoTrace) every wrapper asks `on` once and calls straight through.
//!
//! - `placer`, `linker`, `host`, `installer`: the impl of each seam trait for `Traced`.

use crate::{PackageId, Phase, Step, Trace};

mod host;
mod installer;
mod linker;
mod placer;

/// A seam `S` whose calls are traced to `T`.
pub struct Traced<S, T> {
    inner: S,
    trace: T,
}

impl<S, T: Trace> Traced<S, T> {
    pub fn new(inner: S, trace: T) -> Self {
        Traced { inner, trace }
    }

    /// Runs `call`, with its begin before and its end after when the trace is on. `status` reads the answer's status.
    fn run<R>(
        &mut self,
        step: Step,
        package: PackageId,
        call: impl FnOnce(&mut S) -> R,
        status: impl FnOnce(&R) -> i32,
    ) -> R {
        if !self.trace.on() {
            return call(&mut self.inner);
        }
        self.trace.event(Phase::Begin, step, package, 0);
        let r = call(&mut self.inner);
        self.trace.event(Phase::End, step, package, status(&r));
        r
    }
}

#[cfg(test)]
mod tests;
