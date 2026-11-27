//! The kind of a setting with the data each kind carries. Every rule about a kind lives here once: the encoder and both renderers match
//! on this enum.

use super::declaration::{RawField, RawType};
use super::label::Label;
use crate::error::*;
use std::collections::HashSet;

pub(super) const MAX_TEXT_BYTES: u8 = 64;
pub(super) const MAX_FILE_NAME_BYTES: u8 = 47;
const MAX_EXTENSION_BYTES: usize = 16;
pub(super) const MAX_DECIMALS: u8 = 4;
pub(super) const MAX_UNIT_BYTES: usize = 8;
pub(super) const MAX_CHOICES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text {
        max_bytes: u8,
    },
    FileName {
        max_bytes: u8,
        extension: String,
        reset_label: String,
    },
    Integer {
        min: i32,
        max: i32,
        step: u32,
        decimals: u8,
        unit: String,
        default: i32,
    },
    Boolean {
        default: bool,
    },
    Choice {
        labels: Vec<Label>,
        default: u8,
    },
}

impl Kind {
    /// The checked kind of a declared setting.
    pub(super) fn parse(raw: &RawField) -> Result<Kind> {
        let id = &raw.id;
        let allowed: &[&str] = match raw.kind {
            RawType::Text => &["max_bytes"],
            RawType::FileName => &["max_bytes", "extension", "reset_label"],
            RawType::Integer => &["min", "max", "step", "decimals", "unit", "default"],
            RawType::Boolean => &["default"],
            RawType::Choice => &["choices", "default"],
        };
        for field in raw.present() {
            if allowed.contains(&field) {
                continue;
            }
            return Err(if matches!(field, "extension" | "reset_label") {
                err!("setting {id:?}: extension and reset_label require type file-name")
            } else {
                err!(
                    "setting {id:?}: field {field} does not apply to type {}",
                    raw.kind.name()
                )
            });
        }
        match raw.kind {
            RawType::Text => Ok(Kind::Text {
                max_bytes: Self::max_bytes(raw, MAX_TEXT_BYTES)?,
            }),
            RawType::FileName => Self::file_name(raw),
            RawType::Integer => Self::integer(raw),
            RawType::Boolean => match raw.default {
                Some(toml::Value::Boolean(default)) => Ok(Kind::Boolean { default }),
                _ => Err(err!(
                    "setting {id:?}: boolean needs default = true or false"
                )),
            },
            RawType::Choice => Self::choice(raw),
        }
    }

    fn max_bytes(raw: &RawField, limit: u8) -> Result<u8> {
        let id = &raw.id;
        let max_bytes = raw
            .max_bytes
            .ok_or_else(|| err!("setting {id:?}: max_bytes is required"))?;
        if !(1..=limit).contains(&max_bytes) {
            return Err(err!("setting {id:?}: max_bytes must be 1 to {limit}"));
        }
        Ok(max_bytes)
    }

    fn file_name(raw: &RawField) -> Result<Kind> {
        let id = &raw.id;
        let max_bytes = Self::max_bytes(raw, MAX_FILE_NAME_BYTES)?;
        let extension = raw.extension.as_deref().unwrap_or_default();
        if !(1..=MAX_EXTENSION_BYTES).contains(&extension.len())
            || !extension
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err(err!(
                "setting {id:?}: extension must be 1 to {MAX_EXTENSION_BYTES} lowercase ASCII letters or digits"
            ));
        }
        if usize::from(max_bytes) < extension.len() + 2 {
            return Err(err!(
                "setting {id:?}: max_bytes must fit a stem, dot, and extension"
            ));
        }
        let reset_label = raw.reset_label.clone().unwrap_or_else(|| "None".into());
        if !(1..=255).contains(&reset_label.len()) {
            return Err(err!(
                "setting {id:?}: reset label must be 1 to 255 UTF-8 bytes"
            ));
        }
        Ok(Kind::FileName {
            max_bytes,
            extension: extension.into(),
            reset_label,
        })
    }

    fn integer(raw: &RawField) -> Result<Kind> {
        let id = &raw.id;
        let (Some(min), Some(max)) = (raw.min, raw.max) else {
            return Err(err!("setting {id:?}: integer needs min and max"));
        };
        if max <= min {
            return Err(err!("setting {id:?}: max must be greater than min"));
        }
        let step = raw.step.unwrap_or(1);
        if step == 0 {
            return Err(err!("setting {id:?}: step must be at least 1"));
        }
        let span = i64::from(max) - i64::from(min);
        if span % i64::from(step) != 0 {
            return Err(err!("setting {id:?}: max - min must be a multiple of step"));
        }
        let decimals = raw.decimals.unwrap_or(0);
        if decimals > MAX_DECIMALS {
            return Err(err!("setting {id:?}: decimals must be 0 to {MAX_DECIMALS}"));
        }
        let unit = raw.unit.clone().unwrap_or_default();
        if unit.len() > MAX_UNIT_BYTES || !unit.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return Err(err!(
                "setting {id:?}: unit must be 0 to {MAX_UNIT_BYTES} printable ASCII bytes"
            ));
        }
        let Some(toml::Value::Integer(default)) = raw.default else {
            return Err(err!("setting {id:?}: integer needs an integer default"));
        };
        let default = i32::try_from(default)
            .ok()
            .filter(|d| (min..=max).contains(d))
            .ok_or_else(|| err!("setting {id:?}: default is outside min and max"))?;
        if (i64::from(default) - i64::from(min)) % i64::from(step) != 0 {
            return Err(err!("setting {id:?}: default is not on a step"));
        }
        Ok(Kind::Integer {
            min,
            max,
            step,
            decimals,
            unit,
            default,
        })
    }

    fn choice(raw: &RawField) -> Result<Kind> {
        let id = &raw.id;
        let texts = raw
            .choices
            .as_deref()
            .ok_or_else(|| err!("setting {id:?}: choice needs choices"))?;
        if !(1..=MAX_CHOICES).contains(&texts.len()) {
            return Err(err!(
                "setting {id:?}: choices must hold 1 to {MAX_CHOICES} labels"
            ));
        }
        let mut seen = HashSet::new();
        let mut labels = vec![];
        for text in texts {
            if !seen.insert(text.as_str()) {
                return Err(err!("setting {id:?}: duplicate choice {text:?}"));
            }
            labels.push(Label::new(text).map_err(|e| err!("setting {id:?}: {e}"))?);
        }
        let Some(toml::Value::String(default)) = &raw.default else {
            return Err(err!("setting {id:?}: choice needs a default label"));
        };
        let default = texts
            .iter()
            .position(|t| t == default)
            .ok_or_else(|| err!("setting {id:?}: default {default:?} is not one of the choices"))?;
        Ok(Kind::Choice {
            labels,
            default: default as u8,
        })
    }

    /// The kind byte of the binary descriptor.
    pub(super) fn tag(&self) -> u8 {
        match self {
            Kind::Text { .. } => 0,
            Kind::FileName { .. } => 1,
            Kind::Integer { .. } => 2,
            Kind::Boolean { .. } => 3,
            Kind::Choice { .. } => 4,
        }
    }

    /// The word in `workbench.toml` and in the generated TypeScript.
    pub(super) fn name(&self) -> &'static str {
        match self {
            Kind::Text { .. } => "text",
            Kind::FileName { .. } => "file-name",
            Kind::Integer { .. } => "integer",
            Kind::Boolean { .. } => "boolean",
            Kind::Choice { .. } => "choice",
        }
    }

    /// True for the kinds a version 1 descriptor holds.
    pub(super) fn is_version_1(&self) -> bool {
        matches!(self, Kind::Text { .. } | Kind::FileName { .. })
    }

    /// The most bytes the stored value holds.
    pub(super) fn stored_len(&self) -> u8 {
        match self {
            Kind::Text { max_bytes } | Kind::FileName { max_bytes, .. } => *max_bytes,
            Kind::Integer { .. } => 4,
            Kind::Boolean { .. } | Kind::Choice { .. } => 1,
        }
    }

    #[cfg(test)]
    /// Whether `value` is a stored value of this kind: length and UTF-8 for the text kinds (file names are checked against the module's
    /// files by the runtime), range, step and length for an integer, 0 or 1 for a boolean, an index below the count for a choice.
    pub(super) fn accepts(&self, value: &[u8]) -> bool {
        match self {
            Kind::Text { max_bytes } | Kind::FileName { max_bytes, .. } => {
                value.len() <= usize::from(*max_bytes) && std::str::from_utf8(value).is_ok()
            }
            Kind::Integer { min, max, step, .. } => {
                let Ok(bytes) = <[u8; 4]>::try_from(value) else {
                    return false;
                };
                let v = i32::from_be_bytes(bytes);
                (*min..=*max).contains(&v)
                    && (i64::from(v) - i64::from(*min)) % i64::from(*step) == 0
            }
            Kind::Boolean { .. } => matches!(value, [0 | 1]),
            Kind::Choice { labels, .. } => matches!(value, [i] if usize::from(*i) < labels.len()),
        }
    }
}

#[cfg(test)]
mod tests;
