//! Effect pages: the FX view page and the knobs of a run-time effect (docs/design/digikit-registry.md "Effect pages", docs/findings/61-fx-pages.md).
//!
//! A `.dkm` that carries an EFFECT record (`register` kind 4) for one of its effects gets a page after CHO in the FX view of the Digitakt II. The page is built
//! from rows that `core/rows` reserved at patch time (the blocks of `rows_info`) and that nothing here names a device for: the device facts (`DeviceFacts`, generated from the device files) are the table of the kit's value
//! array and the device's free value indices. For effect row k (the k-th free row of `core/dynfx`) the record makes
//!
//! - the page row k of the page rows of this service (`DKRT_FXP_ROWS`): tab label, title and the nine parameter codes (the knobs at their positions, MIX on the level slot),
//! - eight parameter rows (`params.fp<8k + j>`): knob j (the effect's parameter word p_j) and MIX (j = 7), each pointing at the kit's value array index
//!   `value_indices[8k + j]` as its mirror index, so the firmware's own page code shows and edits the value there,
//! - the list of pages of the FX view (the stock three, then the pages of the loaded effects in row order).
//!
//! `tick` (once per UI-loop pass) reads the values of the loaded effects from the kit's array and sends a changed value to the effect's row in the DSP, one word
//! per transfer through window 2 of `core/dsp-link`: knob j as a float in [min, max] into p_j (min + (max - min) * raw / 0x7f00), MIX as a float 0 .. 1 into the row's
//! mix word (raw / 0x7f00), which the adapter `core/fx` applies as the dry/wet blend. A value is the raw s16 of the array, 0 to 0x7f00 (the display shows raw / 256).
//! The values are saved with the kit (persistence, finding 61): the hooks `fxp_load` and `fxp_save` run in the firmware's kit loader and saver, put the
//! values of the loaded effects into the kit's spare storage halfwords and take them out again, together with the effect's ID (the EFFECT record's `id`). A load
//! gives an effect row the stored values only when the stored ID is the one of the effect now in the row, and the effect's defaults otherwise, before the UI loop
//! looks at the array again, so no zero ever reaches the DSP. A third hook, `fxp_init` (the firmware's initialiser of the array: a new kit, a fresh kit that Load Kit fills),
//! gives each loaded effect its defaults. Both put a mark in a spare index of the array; a kit object without the mark (another pattern's kit, made before the effect was loaded) gets the
//! defaults the first time `tick` finds it live, before any value is read from it.
//! The array is the live kit's (`[fx] kit_ptr` of the device file): the kit accessor of the firmware (`fw_kit_of`) answers another object, the project's.
//!
//! Everything runs in the UI task. No float arithmetic runs in an interrupt; the arithmetic is the compiler's soft float.
pub use dkrt_sdk::records::{KIND_EFFECT, LEN_EFFECT};
mod aligned16;
mod apply;
mod facts;
mod kit;
mod layout;
mod list;
mod record;
mod state;
mod symbols;
mod table;
#[rustfmt::skip]
mod tables;
#[cfg(test)]
mod tests;
mod tick;
pub use aligned16::*;
pub use apply::*;
pub use facts::*;
pub(crate) use kit::*;
use layout::*;
pub use list::*;
pub use record::*;
pub use state::*;
pub use symbols::*;
pub use table::*;
pub use tick::*;
