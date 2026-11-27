//! `<prefix>_<Msg>_encode` and `_size`.

use super::size::key_len;
use super::{CCodec, Kind, Scalar};
use crate::build::wire::schema::Field;
use std::fmt::Write as _;

impl Scalar {
    fn put(self, v: &str) -> String {
        match self {
            Scalar::Uint32 => format!("dk_wire_put_varint(w, {v})"),
            Scalar::Int32 | Scalar::Enum => format!("dk_wire_put_int32(w, {v})"),
            Scalar::Bool => format!("dk_wire_put_varint(w, ({v}) != 0)"),
        }
    }

    fn size(self, v: &str) -> String {
        match self {
            Scalar::Uint32 => format!("dk_wire_size_varint({v})"),
            Scalar::Int32 | Scalar::Enum => format!("dk_wire_size_int32({v})"),
            Scalar::Bool => "1u".into(),
        }
    }
}

impl CCodec<'_> {
    pub(super) fn encode(&mut self, i: usize) {
        let ty = self.ty(i);
        let fields = self.schema.messages[i].fields.clone();
        let mut body = String::new();
        for f in &fields {
            self.write_field(&mut body, f);
        }
        writeln!(
            self.out,
            "DK_WIRE_FN void {ty}_encode(dk_wire_writer *w, const {ty} *m)\n{{\n{body}}}\n"
        )
        .unwrap();
    }

    pub(super) fn size(&mut self, i: usize) {
        let ty = self.ty(i);
        let fields = self.schema.messages[i].fields.clone();
        let mut body = String::new();
        for f in &fields {
            self.size_field(&mut body, f);
        }
        writeln!(self.out, "DK_WIRE_FN uint32_t {ty}_size(const {ty} *m)\n{{\n    uint32_t n = 0;\n{body}    return n;\n}}\n").unwrap();
    }

    fn write_field(&self, o: &mut String, f: &Field) {
        let (n, num) = (&f.name, f.number);
        let key = |wire: &str| format!("dk_wire_put_key(w, {num}, {wire});");
        match (&f.kind, f.repeated) {
            (Kind::Scalar(s), None) => {
                writeln!(
                    o,
                    "    if (m->{n}) {{\n        {}\n        {};\n    }}",
                    key("DK_WIRE_VARINT"),
                    s.put(&format!("m->{n}"))
                )
                .unwrap();
            }
            (Kind::Scalar(s), Some(max)) => {
                writeln!(o, "    if (m->{n}_count > {max}u) {{\n        w->err = 1;\n        return;\n    }}\n    if (m->{n}_count) {{\n        uint32_t i, size = 0;\n        for (i = 0; i < m->{n}_count; i++)\n            size += {};\n        {}\n        dk_wire_put_varint(w, size);\n        for (i = 0; i < m->{n}_count; i++)\n            {};\n    }}", s.size(&format!("m->{n}[i]")), key("DK_WIRE_LEN"), s.put(&format!("m->{n}[i]"))).unwrap();
            }
            (Kind::String(max), None) | (Kind::Bytes(max), None) => {
                writeln!(o, "    if (m->{n}_len > {max}u) {{\n        w->err = 1;\n        return;\n    }}\n    if (m->{n}_len) {{\n        {}\n        dk_wire_put_varint(w, m->{n}_len);\n        dk_wire_put_bytes(w, m->{n}, m->{n}_len);\n    }}", key("DK_WIRE_LEN")).unwrap();
            }
            (Kind::String(max), Some(count)) => {
                writeln!(o, "    if (m->{n}_count > {count}u) {{\n        w->err = 1;\n        return;\n    }}\n    for (uint32_t i = 0; i < m->{n}_count; i++) {{\n        if (m->{n}_len[i] > {max}u) {{\n            w->err = 1;\n            return;\n        }}\n        {}\n        dk_wire_put_varint(w, m->{n}_len[i]);\n        dk_wire_put_bytes(w, m->{n}[i], m->{n}_len[i]);\n    }}", key("DK_WIRE_LEN")).unwrap();
            }
            (Kind::Message(m), None) => {
                let inner = self.ty(*m);
                writeln!(o, "    if (m->has_{n}) {{\n        {}\n        dk_wire_put_varint(w, {inner}_size(&m->{n}));\n        {inner}_encode(w, &m->{n});\n    }}", key("DK_WIRE_LEN")).unwrap();
            }
            (Kind::Message(m), Some(max)) => {
                let inner = self.ty(*m);
                writeln!(o, "    if (m->{n}_count > {max}u) {{\n        w->err = 1;\n        return;\n    }}\n    for (uint32_t i = 0; i < m->{n}_count; i++) {{\n        {}\n        dk_wire_put_varint(w, {inner}_size(&m->{n}[i]));\n        {inner}_encode(w, &m->{n}[i]);\n    }}", key("DK_WIRE_LEN")).unwrap();
            }
            (Kind::Bytes(_), Some(_)) => unreachable!("refused by the schema"),
        }
    }

    fn size_field(&self, o: &mut String, f: &Field) {
        let (n, k) = (&f.name, key_len(f));
        match (&f.kind, f.repeated) {
            (Kind::Scalar(s), None) => writeln!(o, "    if (m->{n})\n        n += {k}u + {};", s.size(&format!("m->{n}"))),
            (Kind::Scalar(s), Some(_)) => writeln!(o, "    if (m->{n}_count) {{\n        uint32_t i, size = 0;\n        for (i = 0; i < m->{n}_count; i++)\n            size += {};\n        n += {k}u + dk_wire_size_varint(size) + size;\n    }}", s.size(&format!("m->{n}[i]"))),
            (Kind::String(_) | Kind::Bytes(_), None) => writeln!(o, "    if (m->{n}_len)\n        n += {k}u + dk_wire_size_varint(m->{n}_len) + m->{n}_len;"),
            (Kind::String(_), Some(_)) => writeln!(o, "    for (uint32_t i = 0; i < m->{n}_count; i++)\n        n += {k}u + dk_wire_size_varint(m->{n}_len[i]) + m->{n}_len[i];"),
            (Kind::Message(m), None) => {
                let inner = self.ty(*m);
                writeln!(o, "    if (m->has_{n}) {{\n        uint32_t size = {inner}_size(&m->{n});\n        n += {k}u + dk_wire_size_varint(size) + size;\n    }}")
            }
            (Kind::Message(m), Some(_)) => {
                let inner = self.ty(*m);
                writeln!(o, "    for (uint32_t i = 0; i < m->{n}_count; i++) {{\n        uint32_t size = {inner}_size(&m->{n}[i]);\n        n += {k}u + dk_wire_size_varint(size) + size;\n    }}")
            }
            (Kind::Bytes(_), Some(_)) => unreachable!("refused by the schema"),
        }
        .unwrap();
    }
}
