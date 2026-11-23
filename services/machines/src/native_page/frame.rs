/// `dk_page_frame` without the canvas function pointers: `version`, `size`, `track`, `params[8]`, the canvas width and height, `word_count`, `word_values[8]`, `level`.
/// `size` is the size of the C struct (4 + 4 + 4 + 32 + canvas 16 + 4 + 32 + 4 = 100 bytes with the two pointers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub track: u32,
    pub params: [i32; 8],
    pub width: u32,
    pub height: u32,
    pub word_count: u32,
    pub word_values: [i32; 8],
    pub level: i32,
}

pub const FRAME_VERSION: u32 = 1;
pub const FRAME_SIZE: u32 = 100;
pub const FRAME_BYTES: usize = 3 * 4 + 32 + 2 * 4 + 4 + 32 + 4 + 4 * 2 /* callbacks */;

impl Frame {
    /// The frame as the ColdFire lays it out in memory (big endian); the two canvas callbacks are `fn_text` and `fn_fill` addresses.
    pub fn to_bytes(&self, fn_text: u32, fn_fill: u32) -> [u8; FRAME_BYTES] {
        let mut out = [0u8; FRAME_BYTES];
        let mut at = 0;
        let mut put = |v: u32| {
            out[at..at + 4].copy_from_slice(&v.to_be_bytes());
            at += 4;
        };
        put(FRAME_VERSION);
        put(FRAME_SIZE);
        put(self.track);
        for p in self.params {
            put(p as u32);
        }
        put(self.width);
        put(self.height);
        put(fn_text);
        put(fn_fill);
        put(self.word_count);
        for w in self.word_values {
            put(w as u32);
        }
        put(self.level as u32);
        out
    }
}
