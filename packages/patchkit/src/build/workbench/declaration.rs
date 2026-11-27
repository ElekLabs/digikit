//! The text of `workbench.toml` as serde reads it. Nothing is checked here: `Schema::parse` turns it into checked settings.

use super::apply::Apply;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Declaration {
    pub(super) workbench: Version,
    #[serde(default)]
    pub(super) setting: Vec<RawField>,
    #[serde(default)]
    pub(super) operation: Vec<PreviewOperation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Version {
    pub(super) version: u8,
    #[serde(rename = "component")]
    _component: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawField {
    pub(super) id: String,
    pub(super) label: String,
    #[serde(rename = "type")]
    pub(super) kind: RawType,
    #[serde(default)]
    pub(super) apply: Apply,
    pub(super) max_bytes: Option<u8>,
    pub(super) extension: Option<String>,
    pub(super) reset_label: Option<String>,
    pub(super) min: Option<i32>,
    pub(super) max: Option<i32>,
    pub(super) step: Option<u32>,
    pub(super) decimals: Option<u8>,
    pub(super) unit: Option<String>,
    pub(super) choices: Option<Vec<String>>,
    pub(super) default: Option<toml::Value>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RawType {
    Text,
    FileName,
    Integer,
    Boolean,
    Choice,
}

impl RawType {
    pub(super) fn name(self) -> &'static str {
        match self {
            RawType::Text => "text",
            RawType::FileName => "file-name",
            RawType::Integer => "integer",
            RawType::Boolean => "boolean",
            RawType::Choice => "choice",
        }
    }
}

impl RawField {
    /// The names of the optional fields this setting sets.
    pub(super) fn present(&self) -> Vec<&'static str> {
        [
            ("max_bytes", self.max_bytes.is_some()),
            ("extension", self.extension.is_some()),
            ("reset_label", self.reset_label.is_some()),
            ("min", self.min.is_some()),
            ("max", self.max.is_some()),
            ("step", self.step.is_some()),
            ("decimals", self.decimals.is_some()),
            ("unit", self.unit.is_some()),
            ("choices", self.choices.is_some()),
            ("default", self.default.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, set)| set.then_some(name))
        .collect()
    }
}

// The preview companion uses these declarations. The device descriptor carries public settings only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewOperation {
    pub(super) id: String,
    #[serde(rename = "output")]
    _output: PreviewOutput,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PreviewOutput {
    OptionalText,
}
