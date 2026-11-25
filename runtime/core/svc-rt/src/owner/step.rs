//! `Owner::step`: a call into the owner in two phases, so a firmware call never runs inside it. The first phase (`decide`) works on the object and gets no handle to the
//! firmware or the core; the second (`act`) runs after the owner is idle again and gets the [`Outside`] token, the only way to make a firmware handle (a service's `Fw` type
//! takes the token in its constructor). A firmware function that calls back into the service (a hook, a code mapper) then finds the owner idle, never refused.
use super::{Events, Owner};
use core::marker::PhantomData;

/// The proof that the owner is not inside a call: only [`Owner::step`] makes one, and only for its `act` phase, which runs after the object was released. A service's firmware
/// handle is built from it (`DkFw::new(&Outside)`), so a handle cannot be made inside a `with` closure that has no token. Do not keep an `Outside`, or a handle made from it, for a later `with`: the token proves the owner idle only during the `act` call that got it, and a kept one reaches firmware from inside a later call into the owner.
pub struct Outside<'a>(PhantomData<&'a ()>);

impl<T, A: Events<T>, const N: usize> Owner<T, A, N> {
    /// `decide(object, input)` on the object (one call, as [`Owner::run`]); its answer then goes to `act` with the [`Outside`] token, after the call ended and the queued
    /// events ran. `None`, and `act` is not called, when `decide` did not run: no live object, or a call into the same owner was in progress (counted in `refused()`).
    ///
    /// `decide` is a function pointer: it captures nothing, so a firmware handle made before the call cannot be carried into it, and it cannot make one (the token is the only
    /// way). What it needs from outside comes in as plain `input`; what `act` needs from the object goes out as `X`.
    ///
    /// Residual hole: a service can still call a firmware function directly from `decide` (no handle is needed for a bare function); the host test of its entries with a
    /// re-entering fake covers it, and review.
    pub fn step<I, X, R>(
        &self,
        input: I,
        decide: fn(&mut T, I) -> X,
        act: impl FnOnce(X, &Outside) -> R,
    ) -> Option<R> {
        let x = self.with(|t| decide(t, input))?;
        Some(act(x, &Outside(PhantomData)))
    }
}
