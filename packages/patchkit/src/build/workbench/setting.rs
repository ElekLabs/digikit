//! One public setting: its identity, when it applies and its kind.

use super::apply::Apply;
use super::declaration::RawField;
use super::descriptor::MAX_STRING_BYTES;
use super::ident::Ident;
use super::kind::Kind;
use crate::error::*;
use dkrt_sdk::settings::{KEY_MAX, module_key};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Setting {
    id: String,
    label: String,
    apply: Apply,
    kind: Kind,
    ident: Ident,
}

impl Setting {
    /// The checked setting of a declaration for the module `module`.
    pub(super) fn parse(raw: &RawField, module: &str) -> Result<Setting> {
        if module_key(module.as_bytes(), raw.id.as_bytes(), &mut [0; KEY_MAX]).is_none() {
            return Err(err!("invalid module settings key {:?}", raw.id));
        }
        let ident = Ident::of_id(&raw.id).ok_or_else(|| {
            err!(
                "invalid module settings key {:?}: not a C identifier",
                raw.id
            )
        })?;
        if !(1..=MAX_STRING_BYTES).contains(&raw.label.len()) {
            return Err(err!(
                "settings label must be 1 to {MAX_STRING_BYTES} UTF-8 bytes"
            ));
        }
        Ok(Setting {
            id: raw.id.clone(),
            label: raw.label.clone(),
            apply: raw.apply,
            kind: Kind::parse(raw)?,
            ident,
        })
    }

    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn apply(&self) -> Apply {
        self.apply
    }

    pub(super) fn kind(&self) -> &Kind {
        &self.kind
    }

    /// `WB_SETTING_<ID>_<suffix>`: a macro or enum constant of the generated header.
    pub(super) fn constant(&self, suffix: &str) -> String {
        format!("WB_SETTING_{}_{suffix}", self.ident.upper())
    }

    /// `wb_setting_<id>_<verb>`: an inline function of the generated header.
    pub(super) fn function(&self, verb: &str) -> String {
        format!("wb_setting_{}_{verb}", self.ident.lower())
    }

    /// Every C name the header defines for this setting.
    pub(super) fn symbols(&self) -> Vec<String> {
        let mut out = vec![self.constant("KEY")];
        let mut constants: Vec<&str> = vec![];
        match &self.kind {
            Kind::Text { .. } | Kind::FileName { .. } => constants.push("MAX_BYTES"),
            Kind::Integer { .. } => constants.extend(["MIN", "MAX", "STEP", "DEFAULT"]),
            Kind::Boolean { .. } => constants.push("DEFAULT"),
            Kind::Choice { labels, .. } => {
                constants.extend(["COUNT", "DEFAULT"]);
                out.extend(labels.iter().map(|l| self.constant(l.ident().upper())));
            }
        }
        out.extend(constants.into_iter().map(|c| self.constant(c)));
        if !matches!(self.kind, Kind::Text { .. } | Kind::FileName { .. }) {
            out.extend(["valid", "get", "put"].map(|v| self.function(v)));
        }
        out
    }
}
