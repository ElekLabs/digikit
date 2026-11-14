//! `machines` service: run-time machine types, parameters, page clicks, native pages.
#[cfg(not(test))]
pub mod fw;
pub mod nativepage;
pub mod pageclick;
pub mod paramapi;
pub mod dsp;
pub mod records;
