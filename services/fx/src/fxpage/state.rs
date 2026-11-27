//! The effect pages state `FxpState` and its static `DKRT_FXP`.

use super::*;
use core::ptr::addr_of_mut;
use dkrt_abi::dsp::MAX_FX;

/// The effect pages' state: who owns each page (module record + 1), the values in force, and the strings the rows point to.
#[repr(C, align(4))]
pub struct FxpState {
    pub magic: u32,
    /// The kit object whose array the values were last put in or read from (0: none seen yet).
    pub kit: u32,
    pub polls: u32,
    pub changes: u32,
    pub sent: u32,
    pub errors: u32,
    pub seeds: u32,
    pub owner: [u8; MAX_FX],
    /// Knobs of the page (the words p0 .. p(n-1)); MIX is word 7.
    pub nknob: [u8; MAX_FX],
    /// The raw value (0 .. 0x7f00, as the kit's array holds it) in force, the one handed to the link as the word's live value (`want`) and the one the DSP has
    /// taken (`dsp`; -1: none).
    pub cur: [[i32; WORDS]; MAX_FX],
    pub want: [[i32; WORDS]; MAX_FX],
    pub dsp: [[i32; WORDS]; MAX_FX],
    /// The record's default raw values (knob j, MIX): what a kit gets that holds no values of this effect.
    pub def: [[i32; WORDS]; MAX_FX],
    /// The effect ID of each row (the record's; 0: no effect), as the kit hooks have it.
    pub id: [u32; MAX_FX],
    /// The live kit pointer changed to a kit object that had the mark (a pattern change back to a kit we know).
    pub adopts: u32,
    /// The float range of knob j: its minimum and its span max - min (f32 bits).
    pub min: [[u32; KNOBS]; MAX_FX],
    pub span: [[u32; KNOBS]; MAX_FX],
    pub(super) tab: [[u8; TEXT]; MAX_FX],
    pub(super) title: [[u8; TEXT]; MAX_FX],
    pub(super) name: [[u8; 32]; MAX_FX * WORDS],
    pub(super) short: [[u8; 16]; MAX_FX * WORDS],
    pub(super) rep_tab: [[u8; REP_SIZE]; MAX_FX],
    pub(super) rep_title: [[u8; REP_SIZE]; MAX_FX],
    /// What `remove` found at a row, one stamp for each row: the effect ID that left (0: none), a hash of the module's name, the live kit object, and the count of kit loads and
    /// initialisations at that time. A load of the same effect of the same module into the row finds the user's values still in the kit's array and keeps them (`apply`),
    /// unless a kit was loaded or the live kit changed in between. A row another module's effect takes, or one that is removed again, has its own stamp.
    pub(super) parked_id: [u32; MAX_FX],
    pub(super) parked_mod: [u32; MAX_FX],
    pub(super) parked_kit: [u32; MAX_FX],
    pub(super) parked_hooks: [u32; MAX_FX],
}

#[no_mangle]
pub static mut DKRT_FXP: FxpState = unsafe { core::mem::zeroed() };

pub(super) fn fs() -> &'static mut FxpState {
    unsafe { &mut *addr_of_mut!(DKRT_FXP) }
}
