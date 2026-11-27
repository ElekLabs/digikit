use super::canvas::HEIGHT;
use super::page::{NativePage, ENCODERS, FLAG_WIDE_CANVAS};
use super::view::View;

/// `dk_page_frame` (sdk/include/dk/page/page.h) without the canvas function pointers: `version`, `size`, `track`, `params[8]`, the canvas width and height, `word_count`,
/// `word_values[8]`, `level`. `size` is the size of the C struct, 100 bytes with the two 4-byte pointers.
///
/// The byte offsets below are the layout. `tests/fixtures/modules/native-page/src/frame-layout.h` repeats them and the fixture module (C) asserts them against the struct
/// at compile time (`_Static_assert` on `sizeof` and `offsetof`); `tests` checks the header against these constants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    track: u32,
    params: [i32; ENCODERS],
    width: u32,
    height: u32,
    word_count: u32,
    word_values: [i32; 8],
    level: i32,
}

pub const FRAME_VERSION: u32 = 1;
pub const OFF_VERSION: usize = 0;
pub const OFF_SIZE: usize = 4;
pub const OFF_TRACK: usize = 8;
pub const OFF_PARAMS: usize = 12;
pub const OFF_CANVAS_WIDTH: usize = 44;
pub const OFF_CANVAS_HEIGHT: usize = 48;
pub const OFF_CANVAS_TEXT: usize = 52;
pub const OFF_CANVAS_FILL: usize = 56;
pub const OFF_WORD_COUNT: usize = 60;
pub const OFF_WORD_VALUES: usize = 64;
pub const OFF_LEVEL: usize = 96;
pub const FRAME_BYTES: usize = 100;

impl Frame {
    /// The frame a page shows: PARAM values by encoder position, the level and the word snapshot, read in the C order (parameters, level, words).
    pub(super) fn read(view: &impl View, page: &NativePage, track: u32, width: u32) -> Self {
        let mut f = Frame {
            track,
            params: [0; ENCODERS],
            width,
            height: HEIGHT as u32,
            word_count: page.word_codes().len() as u32,
            word_values: [0; 8],
            level: 0,
        };
        for b in page.bindings().iter().filter(|b| b[0] == 1) {
            f.params[b[2] as usize] = view.value(b[1]);
        }
        f.level = view.value(page.level_code());
        for (i, &code) in page.word_codes().iter().enumerate() {
            f.word_values[i] = view.value(code);
        }
        f
    }
    /// The canvas width of a page.
    pub(super) fn width_of(page: &NativePage) -> u32 {
        if page.flags() & FLAG_WIDE_CANVAS != 0 {
            super::canvas::WIDE_WIDTH
        } else {
            super::canvas::WIDTH
        }
    }
    pub fn track(&self) -> u32 {
        self.track
    }
    pub fn params(&self) -> &[i32; ENCODERS] {
        &self.params
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn word_count(&self) -> u32 {
        self.word_count
    }
    pub fn word_values(&self) -> &[i32; 8] {
        &self.word_values
    }
    pub fn level(&self) -> i32 {
        self.level
    }

    /// The frame as the ColdFire lays it out in memory (big endian); the two canvas callbacks are `fn_text` and `fn_fill` addresses.
    pub fn to_bytes(&self, fn_text: u32, fn_fill: u32) -> [u8; FRAME_BYTES] {
        let mut out = [0u8; FRAME_BYTES];
        let mut put = |at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_be_bytes());
        put(OFF_VERSION, FRAME_VERSION);
        put(OFF_SIZE, FRAME_BYTES as u32);
        put(OFF_TRACK, self.track);
        for (i, p) in self.params.iter().enumerate() {
            put(OFF_PARAMS + 4 * i, *p as u32);
        }
        put(OFF_CANVAS_WIDTH, self.width);
        put(OFF_CANVAS_HEIGHT, self.height);
        put(OFF_CANVAS_TEXT, fn_text);
        put(OFF_CANVAS_FILL, fn_fill);
        put(OFF_WORD_COUNT, self.word_count);
        for (i, w) in self.word_values.iter().enumerate() {
            put(OFF_WORD_VALUES + 4 * i, *w as u32);
        }
        put(OFF_LEVEL, self.level as u32);
        out
    }
}

#[cfg(test)]
mod tests;
