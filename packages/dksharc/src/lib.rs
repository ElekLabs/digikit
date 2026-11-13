//! Static analysis of the SHARC+ DSP section of a module.
//!
//! [`Input`] holds the text of a module's DSP image with its relocations, imports and symbols; [`analyze`] decodes it with the
//! decoder of `sharc_rt` and builds the model of [`model`]: functions, basic blocks with their delay slots, DO loops, calls (module
//! functions and imports) and per-instruction memory operands at form level. A decode that is not confident is reported in
//! `Analysis::undecoded`, never guessed.

mod analyze;
mod forms;
mod input;
mod model;

pub use analyze::analyze;
pub use input::{Entry, Input, L2_BYTE_BASE, L2_SW_BASE, TextReloc, TextSymbol};
pub use model::*;
