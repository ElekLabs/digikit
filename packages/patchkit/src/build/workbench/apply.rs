//! When a changed setting takes effect.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Apply {
    #[default]
    Immediate,
    NextRestart,
}

impl Apply {
    /// The byte in the binary descriptor.
    pub(super) fn tag(self) -> u8 {
        match self {
            Apply::Immediate => 0,
            Apply::NextRestart => 1,
        }
    }

    /// The word in `workbench.toml` and in the generated TypeScript.
    pub(super) fn name(self) -> &'static str {
        match self {
            Apply::Immediate => "immediate",
            Apply::NextRestart => "next-restart",
        }
    }
}
