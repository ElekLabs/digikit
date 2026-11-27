//! The sizes the transport and the stack budget are checked against: the largest encoding of each message and the size of its C struct.

use super::super::schema::{Field, Kind, Scalar, Schema};

/// Bytes of the varint of `n`.
fn varint_len(n: u64) -> u32 {
    (64 - (n | 1).leading_zeros()).div_ceil(7)
}

pub(super) fn key_len(f: &Field) -> u32 {
    varint_len(u64::from(f.number) << 3)
}

impl Scalar {
    /// Largest encoded size of one value.
    pub(super) fn max_size(self) -> u32 {
        match self {
            Scalar::Uint32 => 5,
            Scalar::Int32 | Scalar::Enum => 10,
            Scalar::Bool => 1,
        }
    }
}

/// The most bytes of one message, by message index (a message comes after the messages it holds).
pub(in crate::build::wire) fn max_sizes(schema: &Schema) -> Vec<u32> {
    let mut max: Vec<u32> = vec![];
    for m in &schema.messages {
        let mut total = 0;
        for f in &m.fields {
            let key = key_len(f);
            let one = |kind: &Kind| match kind {
                Kind::Scalar(s) => s.max_size(),
                Kind::String(n) | Kind::Bytes(n) => varint_len(u64::from(*n)) + n,
                Kind::Message(i) => varint_len(u64::from(max[*i])) + max[*i],
            };
            total += match (&f.kind, f.repeated) {
                (Kind::Scalar(s), Some(n)) => {
                    let body = n * s.max_size();
                    key + varint_len(u64::from(body)) + body
                }
                (kind, Some(n)) => n * (key + one(kind)),
                (kind, None) => key + one(kind),
            };
        }
        max.push(total);
    }
    max
}

/// An upper bound of `sizeof` of each message's C struct (every member padded to 4 bytes; the ColdFire pads to 2), by message index.
pub(in crate::build::wire) fn struct_sizes(schema: &Schema) -> Vec<u32> {
    let mut sizes: Vec<u32> = vec![];
    let pad = |n: u32| n.div_ceil(4) * 4;
    for m in &schema.messages {
        let mut total = 0;
        for f in &m.fields {
            total += match (&f.kind, f.repeated) {
                (Kind::Scalar(_), None) => 4,
                (Kind::Scalar(Scalar::Bool), Some(n)) => pad(n) + 4,
                (Kind::Scalar(_), Some(n)) => 4 * n + 4,
                (Kind::String(n), None) => pad(n + 1) + 4,
                (Kind::Bytes(n), None) => pad(*n) + 4,
                (Kind::String(n), Some(c)) => c * pad(n + 1) + 4 * c + 4,
                (Kind::Message(i), None) => sizes[*i] + 4,
                (Kind::Message(i), Some(c)) => c * sizes[*i] + 4,
                (Kind::Bytes(_), Some(_)) => unreachable!("refused by the schema"),
            };
        }
        sizes.push(pad(total));
    }
    sizes
}
