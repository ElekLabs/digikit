//! Voice-row part of the DSP loader: the voice row of a machine type, its declared ranges and the clearing of a module's voice rows. The rows are the service's own
//! (`rows::VoiceRows`); the table of the DSP's voice registry is read through `CoreApi::info` (`dyndsp`).

mod admit;
mod check;
mod claim;
mod lookup;
mod placeholder;

pub(crate) use admit::*;
pub(crate) use check::*;
pub(crate) use claim::*;
pub use lookup::*;
pub(crate) use placeholder::*;
