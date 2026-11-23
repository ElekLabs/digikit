//! `DKPKG_PLACED`: how many bytes the package takes in the package block, for the capacity report (`CAPK_PACKAGE`). Stage 1 knows the body header; the package does not, and
//! has no resident function to ask. `patchkit syspkg build` writes the value into this word of the image (the same figure stage 1 checks against `PACKAGE_BYTES`:
//! `BodyHeader::placed_bytes`). The word is `UNSET` until then, so it lives in the image (a zero word would be bss).

/// The word before `patchkit syspkg build` has written it (and in every build that is not the package artifact).
pub const UNSET: u32 = 0xffff_ffff;

#[cfg(feature = "package-image")]
#[no_mangle]
pub static DKPKG_PLACED: u32 = UNSET;

/// The bytes the package takes in its block; 0 when the artifact was not built by `patchkit syspkg build`.
pub(crate) fn placed_bytes() -> u32 {
    #[cfg(feature = "package-image")]
    let v = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(DKPKG_PLACED)) };
    #[cfg(not(feature = "package-image"))]
    let v = UNSET;
    if v == UNSET {
        0
    } else {
        v
    }
}
