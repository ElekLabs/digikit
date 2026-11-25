//! Native input device selection and CPAL stream setup for Workbench hosts.

mod device;
mod input;

pub use device::{AudioInput, exact_device, inputs, stereo_config};
pub use input::{InputSink, open_input};
