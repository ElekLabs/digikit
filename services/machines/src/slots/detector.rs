//! The click detector: tells a click of an encoder from a press and a turn.
/// Key code of the click of encoder 0; encoder `e` is code `FIRST_CODE + e`, for e = 0 to 7.
pub const FIRST_CODE: u32 = 0x29;
pub const ENCODERS: u32 = 8;
/// Flag bits of a key event (`event+0x10`).
pub const F_PRESS: u32 = 0x01;
pub const F_CHORD: u32 = 0x02;
pub const F_REPEAT: u32 = 0x08;
pub const F_RELEASE: u32 = 0x10;

pub(super) const NONE: u32 = 0xff;

/// Tells a click from a press and a turn. Pure state, no firmware access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detector {
    /// The encoder whose press is waiting for its release (`NONE`: no press).
    enc: u32,
    view: usize,
    turned: bool,
}

impl Detector {
    pub const fn new() -> Self {
        Detector {
            enc: NONE,
            view: 0,
            turned: false,
        }
    }

    /// A key event of `view`. Returns the encoder of a click: its release, when its press came on the same view and no turn came in between.
    /// A chord (FUNC and so on) and a repeat never make a click; another key's press cancels the pending press.
    pub fn key(&mut self, view: usize, code: u32, flags: u32) -> Option<u32> {
        let enc = code.wrapping_sub(FIRST_CODE);
        if enc >= ENCODERS {
            if flags & F_PRESS != 0 && flags & F_REPEAT == 0 {
                self.enc = NONE;
            }
            return None;
        }
        if flags & F_RELEASE != 0 {
            let click =
                self.enc == enc && self.view == view && !self.turned && flags & F_CHORD == 0;
            self.enc = NONE;
            return if click { Some(enc) } else { None };
        }
        if flags & F_PRESS != 0 && flags & F_REPEAT == 0 {
            self.enc = if flags & F_CHORD == 0 { enc } else { NONE };
            self.view = view;
            self.turned = false;
        }
        None
    }

    /// An encoder turn on `view`: the press that is waiting becomes a coarse step.
    pub fn turn(&mut self, view: usize) {
        if self.enc != NONE && self.view == view {
            self.turned = true;
        }
    }
}
