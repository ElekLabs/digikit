use super::machine_desc::MachineDesc;
use super::table::RANGES;
use core::marker::PhantomData;

/// `dk_voice_install`: one voice type to install. It borrows the payload and the description. The host copies the payload during the
/// first `install` call for the request. A continuation (the call after `Loading`) passes a null payload: [`Install::continuation`].
/// A null payload with 0 bytes is the silent voice: [`Install::new`] makes it from an empty slice.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Install<'a> {
    pub(super) payload: *const u8,
    pub(super) payload_bytes: u32,
    pub(super) desc: *const MachineDesc,
    pub(super) range_min: [i32; RANGES],
    pub(super) range_max: [i32; RANGES],
    pub(super) cost: u32,
    borrowed: PhantomData<&'a ()>,
}

impl<'a> Install<'a> {
    /// `ranges` are the machine's `[[param]]` (min, max) pairs in order; the rest of the eight entries stay zero.
    /// Returns `None` for more than eight ranges or a payload over `u32::MAX` bytes.
    pub fn new(
        payload: &'a [u8],
        desc: &'a MachineDesc,
        ranges: &[(i32, i32)],
        cost: u32,
    ) -> Option<Self> {
        if ranges.len() > RANGES {
            return None;
        }
        let mut range_min = [0; RANGES];
        let mut range_max = [0; RANGES];
        for (i, (min, max)) in ranges.iter().enumerate() {
            range_min[i] = *min;
            range_max[i] = *max;
        }
        Some(Self {
            // An empty slice has a dangling, non-null pointer: the host reads only NULL as "no payload".
            payload: if payload.is_empty() {
                core::ptr::null()
            } else {
                payload.as_ptr()
            },
            payload_bytes: u32::try_from(payload.len()).ok()?,
            desc,
            range_min,
            range_max,
            cost,
            borrowed: PhantomData,
        })
    }

    /// The same request without its payload: the call that continues an install after the host answered `Loading`.
    pub fn continuation(&self) -> Self {
        Self {
            payload: core::ptr::null(),
            payload_bytes: 0,
            ..*self
        }
    }
}
