//! VoiceHost v1: the table of a platform host that plays a digikit machine voice on a track. C side:
//! `sdk/include/dk/host/voice_host.h`, which has the rules. A host publishes the `[[info]]` table
//! named [`NAME`]; the machines service reads it with [`VoiceHost::from_info`].

mod host;
mod install;
mod install_step;
mod machine_desc;
mod name;
mod stats;
mod step;
mod table;

pub use host::VoiceHost;
pub use install::Install;
pub use install_step::InstallStep;
pub use machine_desc::MachineDesc;
pub use name::{NAME, NAME_HASH};
pub use stats::Stats;
pub use step::Step;
pub use table::{FLAG_STEREO, MAGIC, MAJOR, MINOR, RANGES};

#[cfg(test)]
mod tests;
