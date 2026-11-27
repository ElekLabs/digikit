/// The result of an `install` call that did not fail. Both variants carry the type the request occupies: the requested one, or the one
/// the host chose when it was asked for 0. A service continues a `Loading` install by calling `install` again with that type and
/// [`Install::continuation`](super::Install::continuation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStep {
    /// Installed; never type 0, which is the stock machine.
    Done(u32),
    /// The work goes on; the type is held for this request.
    Loading(u32),
}

impl InstallStep {
    /// The type of the request.
    pub fn type_id(&self) -> u32 {
        match *self {
            Self::Done(ty) | Self::Loading(ty) => ty,
        }
    }
}
