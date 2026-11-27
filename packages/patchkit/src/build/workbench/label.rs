//! One label of a choice setting with its C name.

use super::ident::Ident;
use crate::error::*;

pub(super) const LABEL_MAX_BYTES: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Label {
    text: String,
    ident: Ident,
}

impl Label {
    /// A label of 1 to 24 UTF-8 bytes that leaves a C name.
    pub(super) fn new(text: &str) -> Result<Label> {
        if !(1..=LABEL_MAX_BYTES).contains(&text.len()) {
            return Err(err!(
                "choice label {text:?} must be 1 to {LABEL_MAX_BYTES} UTF-8 bytes"
            ));
        }
        let ident = Ident::of_label(text)
            .ok_or_else(|| err!("choice label {text:?} needs a letter or a digit"))?;
        Ok(Label {
            text: text.into(),
            ident,
        })
    }

    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn ident(&self) -> &Ident {
        &self.ident
    }
}
