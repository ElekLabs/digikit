//! The binary descriptor a module registers in init.

use super::kind::Kind;
use super::schema::Schema;
use crate::error::*;

const MAX_BYTES: usize = 512;
/// The most bytes of a string in the descriptor: its length takes one byte.
pub(super) const MAX_STRING_BYTES: usize = 255;

fn push_string(out: &mut Vec<u8>, value: &str) {
    // Every string was checked to fit in a byte when the schema was read.
    out.push(value.len() as u8);
    out.extend_from_slice(value.as_bytes());
}

impl Schema {
    /// The descriptor: version, module name, count, then per setting `[len+id][len+label][max][kind][apply]` and the body of its kind.
    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        let mut out = vec![self.descriptor_version()];
        push_string(&mut out, self.name());
        out.push(self.settings().len() as u8);
        for setting in self.settings() {
            push_string(&mut out, setting.id());
            push_string(&mut out, setting.label());
            let kind = setting.kind();
            out.extend_from_slice(&[kind.stored_len(), kind.tag(), setting.apply().tag()]);
            match kind {
                Kind::Text { .. } => {}
                Kind::FileName {
                    extension,
                    reset_label,
                    ..
                } => {
                    push_string(&mut out, extension);
                    push_string(&mut out, reset_label);
                }
                Kind::Integer {
                    min,
                    max,
                    step,
                    decimals,
                    unit,
                    default,
                } => {
                    out.extend_from_slice(&min.to_be_bytes());
                    out.extend_from_slice(&max.to_be_bytes());
                    out.extend_from_slice(&step.to_be_bytes());
                    out.push(*decimals);
                    out.extend_from_slice(&default.to_be_bytes());
                    out.push(unit.len() as u8);
                    out.extend_from_slice(unit.as_bytes());
                }
                Kind::Boolean { default } => out.push(u8::from(*default)),
                Kind::Choice { labels, default } => {
                    out.extend_from_slice(&[labels.len() as u8, *default]);
                    for label in labels {
                        out.push(label.text().len() as u8);
                        out.extend_from_slice(label.text().as_bytes());
                    }
                }
            }
            if out.len() > MAX_BYTES {
                return Err(err!("Workbench descriptor exceeds {MAX_BYTES} bytes"));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
