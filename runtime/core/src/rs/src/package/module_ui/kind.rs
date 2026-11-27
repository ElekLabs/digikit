//! The kind of a setting. Every rule about a kind lives here once: how its body in the descriptor parses and which stored values
//! it accepts. The body follows `[len+id][len+label][max][kind][apply]` (docs/plans/workbench-module-ui.md "Typed public settings").

use super::cursor::Cursor;

pub(super) const TEXT_MAX: usize = 64;
pub(super) const EXTENSION_MAX: usize = 16;
const FILE_NAME_MAX: usize = 47;
const DECIMALS_MAX: u8 = 4;
const CHOICES_MAX: u8 = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Kind {
    Text = 0,
    FileName = 1,
    Integer = 2,
    Boolean = 3,
    Choice = 4,
}

impl Kind {
    pub(super) fn from_tag(tag: u8) -> Option<Self> {
        // The tags in order: a table in data, not a match the compiler turns into a jump table in the code.
        const ALL: [Kind; 5] = [
            Kind::Text,
            Kind::FileName,
            Kind::Integer,
            Kind::Boolean,
            Kind::Choice,
        ];
        ALL.get(tag as usize).copied()
    }

    /// The least descriptor version that holds this kind.
    pub(super) fn version(self) -> u8 {
        match self {
            Self::Text | Self::FileName => 1,
            _ => 2,
        }
    }

    /// Is the stored value UTF-8 text?
    pub(super) fn is_text(self) -> bool {
        self.version() == 1
    }

    /// Check the body at the cursor for a field with stored length `max` and move past it.
    pub(super) fn parse_body(self, cursor: &mut Cursor, max: u8) -> Option<()> {
        // Two small matches, not one of five arms: the compiler turns a dense match into a jump table of 16-bit offsets in the
        // code, which the object's ISA check reads as instructions and refuses.
        if self.is_text() {
            self.parse_text_body(cursor, max)
        } else {
            self.parse_typed_body(cursor, max)
        }
    }

    fn parse_text_body(self, cursor: &mut Cursor, max: u8) -> Option<()> {
        match self {
            Self::FileName => {
                let ext = cursor.string()?;
                let valid = (1..=EXTENSION_MAX).contains(&ext.len())
                    && (ext.len() + 2..=FILE_NAME_MAX).contains(&(max as usize))
                    && ext
                        .iter()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
                (valid && !cursor.text()?.is_empty()).then_some(())
            }
            _ => (1..=TEXT_MAX).contains(&(max as usize)).then_some(()),
        }
    }

    fn parse_typed_body(self, cursor: &mut Cursor, max: u8) -> Option<()> {
        match self {
            Self::Integer => {
                let min = i32::from_be_bytes(cursor.array()?);
                let top = i32::from_be_bytes(cursor.array()?);
                let step = u32::from_be_bytes(cursor.array()?);
                let decimals = cursor.byte()?;
                let default = i32::from_be_bytes(cursor.array()?);
                cursor.text()?;
                (max == 4
                    && step != 0
                    && min < top
                    && steps_from(min, top, step)
                    && decimals <= DECIMALS_MAX
                    && on_range(min, top, step, default))
                .then_some(())
            }
            Self::Boolean => (max == 1 && cursor.byte()? <= 1).then_some(()),
            _ => {
                let [count, default] = cursor.array()?;
                if max != 1 || !(1..=CHOICES_MAX).contains(&count) || default >= count {
                    return None;
                }
                for _ in 0..count {
                    if cursor.text()?.is_empty() {
                        return None;
                    }
                }
                Some(())
            }
        }
    }

    /// Is `value` a stored value of a field of this kind? `body` is the descriptor from the start of the field's body: the
    /// bounds are read from it, so the registry keeps no copy of them.
    pub(super) fn accepts(self, body: &[u8], max: u8, value: &[u8]) -> bool {
        if self.is_text() {
            self.accepts_text(body, max, value)
        } else {
            self.accepts_typed(body, value)
        }
    }

    fn accepts_text(self, body: &[u8], max: u8, value: &[u8]) -> bool {
        value.len() <= max as usize
            && core::str::from_utf8(value).is_ok()
            && (self != Self::FileName || file_name_ok(value, extension(body)))
    }

    fn accepts_typed(self, body: &[u8], value: &[u8]) -> bool {
        match self {
            Self::Integer => (|| {
                let mut bounds = Cursor::new(body);
                let min = i32::from_be_bytes(bounds.array()?);
                let max = i32::from_be_bytes(bounds.array()?);
                let step = u32::from_be_bytes(bounds.array()?);
                let value = i32::from_be_bytes(value.try_into().ok()?);
                Some(on_range(min, max, step, value))
            })()
            .unwrap_or(false),
            Self::Boolean => matches!(value, [0 | 1]),
            _ => matches!((body.first(), value), (Some(count), [index]) if index < count),
        }
    }
}

/// The declared extension of a file-name body; empty for a body that has none.
pub(super) fn extension(body: &[u8]) -> &[u8] {
    Cursor::new(body).string().unwrap_or_default()
}

/// Is `value` in `min..=max` and a whole number of steps above `min`?
fn on_range(min: i32, max: i32, step: u32, value: i32) -> bool {
    (min..=max).contains(&value) && steps_from(min, value, step)
}

/// Is `to` (not below `from`) a whole number of steps above `from`? The distance fits a u32.
fn steps_from(from: i32, to: i32, step: u32) -> bool {
    (to as u32).wrapping_sub(from as u32) % step == 0
}

/// A store leaf usable by `dk_data_path`, with a nonempty stem and the declared extension.
fn file_name_ok(leaf: &[u8], extension: &[u8]) -> bool {
    let Some(dot) = leaf.iter().rposition(|&c| c == b'.') else {
        return false;
    };
    dot != 0
        && leaf[dot + 1..].eq_ignore_ascii_case(extension)
        && !leaf.windows(2).any(|pair| pair == b"..")
        && leaf
            .iter()
            .all(|c| (0x20..=0x7e).contains(c) && !matches!(c, b'/' | b'\\'))
}
