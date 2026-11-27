//! The C identifier of a setting: the one place that maps a setting id to C names.

/// An upper-case C identifier: `WB_SETTING_<ident>_KEY` and `wb_setting_<ident lower>_get` are built from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ident(String);

impl Ident {
    /// The identifier of a setting id: upper case, `-` becomes `_`. `None` when the result is not a C identifier.
    pub(super) fn of_id(id: &str) -> Option<Ident> {
        let upper: String = id
            .chars()
            .map(|c| {
                if c == '-' {
                    '_'
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        let mut chars = upper.chars();
        let first = chars.next()?;
        let valid = (first.is_ascii_uppercase() || first == '_')
            && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        valid.then_some(Ident(upper))
    }

    /// The identifier of a choice label: each run of characters outside `A-Z`, `a-z`, `0-9` becomes one `_`, the ends are trimmed.
    /// `None` when nothing is left.
    pub(super) fn of_label(label: &str) -> Option<Ident> {
        let mut out = String::new();
        let mut gap = false;
        for c in label.chars() {
            if c.is_ascii_alphanumeric() {
                if gap && !out.is_empty() {
                    out.push('_');
                }
                gap = false;
                out.push(c.to_ascii_uppercase());
            } else {
                gap = true;
            }
        }
        (!out.is_empty()).then_some(Ident(out))
    }

    pub(super) fn upper(&self) -> &str {
        &self.0
    }

    pub(super) fn lower(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests;
