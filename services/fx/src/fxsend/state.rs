//! The send state `SendState` and its static `DKRT_FXS`.

use super::*;
use core::ptr::addr_of_mut;
use dkrt_abi::dsp;

#[repr(C, align(4))]
pub struct SendState {
    pub magic: u32,
    /// Send cells in use, loader count last seen, passes left until it counts as still, restores done, turns found, DSP words sent, failed, puts.
    pub used: u32,
    pub nsound: u32,
    pub settle: u32,
    pub restores: u32,
    pub turns: u32,
    pub sent: u32,
    pub errors: u32,
    pub puts: u32,
    /// The tracks (bit mask) whose Sound array gets the stored amounts before it is read.
    pub restore: u32,
    /// Per effect row: the routes the effect allows (`ROUTE_*` bits) and its default route (`ROUTE_*`), from its record.
    pub allow: [u8; dsp::MAX_FX],
    pub dflt: [u8; dsp::MAX_FX],
    /// Per effect row: 1 when it runs on a send bus, its cell + 1, the length of its module id and the id.
    pub route: [u8; dsp::MAX_FX],
    pub cell: [u8; dsp::MAX_FX],
    pub idlen: [u8; dsp::MAX_FX],
    /// Per cell: its effect row + 1.
    pub row: [u8; SEND_MAX],
    pub id: [[u8; 32]; dsp::MAX_FX],
    /// The amounts, one byte for each track.
    pub amt: [[u8; TRACKS]; dsp::MAX_FX],
    /// The amount words the DSP row has taken, and the ones handed to the link as live values.
    pub dsp: [[u32; 4]; dsp::MAX_FX],
    pub want: [[u32; 4]; dsp::MAX_FX],
    /// The live kit object last seen (the 16 Sound objects lie in it), and for each cell and track the raw value last written to or read from `Sound[idx]` (-1: none).
    pub kit: u32,
    pub shown: [[i32; TRACKS]; SEND_MAX],
    pub zero_wait: [[u8; TRACKS]; SEND_MAX],
    pub(super) name: [[u8; 16]; SEND_MAX],
    pub(super) short: [[u8; 8]; SEND_MAX],
}

#[no_mangle]
pub static mut DKRT_FXS: SendState = unsafe { core::mem::zeroed() };

pub(super) fn ss() -> &'static mut SendState {
    unsafe { &mut *addr_of_mut!(DKRT_FXS) }
}

/// Does effect row `k` run on a send bus (the FX view lists the send effects before the master effects).
#[inline]
pub fn is_send(k: usize) -> bool {
    k < dsp::MAX_FX && ss().route[k] != 0
}

/// Send effects loaded (CAPACITY `used`).
#[inline]
pub fn used() -> u32 {
    ss().used
}
