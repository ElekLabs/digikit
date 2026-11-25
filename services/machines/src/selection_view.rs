//! The machine selection view: its item list is built again after the rows changed.
use dkneeds::keys::name_hash;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::iw;

/// The info table of the resident view capture (`machine_view` of `core/ui`): word 0 is the address of the variable that holds the view (0 until the firmware made it), word 1 the
/// firmware function that builds the view's items, `build(view)`.
pub const VIEW_VAR: usize = 0;
pub const VIEW_BUILD: usize = 1;

/// The firmware builds the machine selection view's item list once, from the list getter, and keeps it. After the rows changed, build it again (the builder clears
/// its items first); nothing to do while the view does not exist yet, or on an image without the hook that remembers it.
///
/// # Safety
/// `core` gives the image's tables and calls the firmware's builder.
pub(crate) unsafe fn refresh_view<C: CoreApi>(core: &C) {
    let t = core.info(name_hash("machine_view"));
    if t.is_null() {
        return;
    }
    let (var, build) = (iw(t, VIEW_VAR), iw(t, VIEW_BUILD));
    if var == 0 || build == 0 {
        return;
    }
    let view = core.rd(var);
    if view != 0 {
        core.call(build, &[view as usize]);
    }
}
