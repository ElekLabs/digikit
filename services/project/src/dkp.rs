//! The project file `.dkp` (docs/design/digikit-project.md section 4) over the record codec of the settings record (`dkrt_sdk::settings::Fmt`): the header, the sections, the
//! RAM image and the three calls of the project scope (`get`, `put`, `remove`).
//!
//! ```text
//! 0   4   magic "DKPJ"
//! 4   2   u16 version = 1
//! 6   2   u16 device id (`[digikit] id`)
//! 8   1   slot (0 to 127)
//! 9   1   name length (0 to 16)
//! 10  16  name, zero padded, as the slot listing read it
//! 26  4   stock save stamp (the listing record's word at +0x18)
//! 30  4   write counter: 1 for a new file, +1 for each write (the `seq` of the codec)
//! 34  4   `used`, 42 to 16384
//! 38  4   crc32 (zlib) over 0..38 and 42..used
//! 42  ..  sections: u8 tag | u16 length | body; a tag this version does not know is skipped
//!           tag 1, the module list: u8 id length | id | u32 hash, repeated
//!           tag 2, the data of one owner: u8 owner length | owner | entries (u8 key length | key | u16 value length | value), each key `<owner>/<key>`
//! ```
//! All integers are big-endian. The RAM image is the file's own bytes: a `put` splices into the owner's section and keeps `used` current; `seal` writes the counter and the crc32.
//!
//! Layout: `format` (constants), `image`, `head`, `section`, `check`, `modules`, `entries`, `scope`.
mod check;
mod entries;
mod format;
mod head;
mod image;
#[cfg(test)]
mod json;
mod modules;
mod scope;
mod section;
#[cfg(test)]
mod tests;

pub use check::*;
pub use entries::*;
pub use format::*;
pub use head::*;
pub use image::*;
pub use modules::*;
pub use scope::*;
use section::*;
