//! The device facts of the lowering (`Profile`).
use dkrt_abi::mem::iw;
use dkrt_svc_rows::dynreg::w;

/// The device facts of the lowering (a copy of the words of `dynreg_info` from 65; see core/dynreg).
#[derive(Clone, Copy)]
pub struct Profile {
    pub device: u32,
    /// The device keeps a machine's knobs in page rows (Digitone II).
    pub pages: bool,
    pub slot_base: u32,
    pub clone_of: u32,
    pub level: u32,
    pub own_dsp: bool,
    pub tune_code: u32,
    pub tune_slot: u32,
    pub level_code: u32,
    pub level_knob: u32,
    pub nslots: u32,
    pub slots: [u32; 8],
    /// Stock rows by glyph: encoder, fader, pan, onoff, tune, play.
    pub tpl: [u32; 6],
}

impl Profile {
    /// The facts of `dynreg_info` version 3 (`inf`).
    pub unsafe fn read(inf: *const u32) -> Profile {
        let mut p = Profile {
            device: iw(inf, w::DEVICE),
            pages: iw(inf, w::PG_ROWS) != 0,
            slot_base: iw(inf, w::SLOT_BASE),
            clone_of: iw(inf, w::P_CLONE),
            level: iw(inf, w::P_LEVEL),
            own_dsp: iw(inf, w::P_OWN) != 0,
            tune_code: iw(inf, w::P_TUNE),
            tune_slot: iw(inf, w::P_TUNE_SLOT),
            level_code: iw(inf, w::P_LEVEL_CODE),
            level_knob: iw(inf, w::P_LEVEL_KNOB),
            nslots: iw(inf, w::P_NSLOTS).min(8),
            slots: [0; 8],
            tpl: [0; 6],
        };
        let mut i = 0;
        while i < 8 {
            p.slots[i] = iw(inf, w::P_SLOTS + i);
            i += 1;
        }
        i = 0;
        while i < 6 {
            p.tpl[i] = iw(inf, w::P_TPL + i);
            i += 1;
        }
        p
    }
}
