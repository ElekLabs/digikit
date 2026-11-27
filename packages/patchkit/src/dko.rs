//! `.dko.json`: a relocatable object in canonical JSON (sorted keys, integers only), made by `patchkit build` (`crate::build`) from a toolchain's
//! output and consumed by the linker. The model, the reader and the ISA check record live in `dksharc::dko`, the one reader of the format
//! (the module DSP analysis and the workbench read the same objects); patchkit re-exports them.
//!
//! Units: ColdFire sections and symbols are bytes. SHARC+ text sections hold the instruction parcels in
//! boot-stream order (each 16-bit parcel little endian); text symbol values and relocation offsets count
//! parcels; data sections and their symbols count bytes. A SHARC relocation writes the target's natural
//! address: the short-word pc for a text symbol, the byte address for a data symbol or an extern value.

pub use dksharc::dko::{Dko, DkoReloc, DkoSection, DkoSymbol, FORMAT, ISA_CHECK_KEY};
