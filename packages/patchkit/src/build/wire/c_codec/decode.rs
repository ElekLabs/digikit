//! `<prefix>_<Msg>_decode`.

use super::{CCodec, Kind, Scalar};
use crate::build::wire::schema::Field;
use std::fmt::Write as _;

impl Scalar {
    /// The C expression that reads one value from the reader `r`.
    fn read(self, r: &str) -> String {
        match self {
            Scalar::Uint32 => format!("dk_wire_get_varint({r})"),
            Scalar::Int32 | Scalar::Enum => format!("(int32_t)dk_wire_get_varint({r})"),
            Scalar::Bool => format!("dk_wire_get_varint({r}) != 0"),
        }
    }
}

impl CCodec<'_> {
    pub(super) fn decode(&mut self, i: usize) {
        let ty = self.ty(i);
        let fields = self.schema.messages[i].fields.clone();
        let holds_len = fields
            .iter()
            .any(|f| !matches!(f.kind, Kind::Scalar(_)) || f.repeated.is_some());
        let mut body = String::new();
        for f in &fields {
            self.init_field(&mut body, f);
        }
        let mut cases = String::new();
        for f in &fields {
            self.read_field(&mut cases, f);
        }
        let o = &mut self.out;
        writeln!(
            o,
            "DK_WIRE_FN int32_t {ty}_decode(const uint8_t *buf, uint32_t n, {ty} *out)\n{{"
        )
        .unwrap();
        writeln!(o, "    dk_wire_reader r;\n    uint32_t f, t;").unwrap();
        if holds_len {
            writeln!(o, "    const uint8_t *s;\n    uint32_t len;").unwrap();
        }
        writeln!(o, "    dk_wire_reader_init(&r, buf, n);").unwrap();
        o.push_str(&body);
        writeln!(o, "    while (r.p != r.end) {{\n        f = dk_wire_get_key(&r, &t);\n        if (r.err || f == 0)\n            return -1;\n        switch (f) {{").unwrap();
        o.push_str(&cases);
        writeln!(o, "        default:\n            dk_wire_skip(&r, t);\n        }}\n        if (r.err)\n            return -1;\n    }}\n    return 0;\n}}\n").unwrap();
    }

    fn init_field(&self, o: &mut String, f: &Field) {
        let n = &f.name;
        match (&f.kind, f.repeated) {
            (_, Some(_)) => writeln!(o, "    out->{n}_count = 0;"),
            (Kind::Scalar(_), None) => writeln!(o, "    out->{n} = 0;"),
            (Kind::String(_), None) => writeln!(o, "    out->{n}_len = 0;\n    out->{n}[0] = 0;"),
            (Kind::Bytes(_), None) => writeln!(o, "    out->{n}_len = 0;"),
            (Kind::Message(_), None) => writeln!(o, "    out->has_{n} = 0;"),
        }
        .unwrap();
    }

    fn read_field(&self, o: &mut String, f: &Field) {
        let (n, num) = (&f.name, f.number);
        writeln!(o, "        case {num}:").unwrap();
        match (&f.kind, f.repeated) {
            (Kind::Scalar(s), None) => {
                writeln!(o, "            if (t != DK_WIRE_VARINT)\n                return -1;\n            out->{n} = {};", s.read("&r")).unwrap();
            }
            (Kind::Scalar(s), Some(max)) => {
                writeln!(o, "            if (t == DK_WIRE_LEN) {{\n                dk_wire_reader p;\n                s = dk_wire_get_len(&r, &len);\n                if (r.err)\n                    return -1;\n                dk_wire_reader_init(&p, s, len);\n                while (p.p != p.end) {{\n                    if (out->{n}_count == {max}u)\n                        return -1;\n                    out->{n}[out->{n}_count++] = {};\n                    if (p.err)\n                        return -1;\n                }}\n            }} else if (t == DK_WIRE_VARINT) {{\n                if (out->{n}_count == {max}u)\n                    return -1;\n                out->{n}[out->{n}_count++] = {};\n            }} else {{\n                return -1;\n            }}", s.read("&p"), s.read("&r")).unwrap();
            }
            (Kind::String(max), None) | (Kind::Bytes(max), None) => {
                let nul = if matches!(f.kind, Kind::String(_)) {
                    format!("\n            out->{n}[len] = 0;")
                } else {
                    String::new()
                };
                writeln!(o, "            if (t != DK_WIRE_LEN)\n                return -1;\n            s = dk_wire_get_len(&r, &len);\n            if (r.err || len > {max}u)\n                return -1;\n            dk_wire_copy(out->{n}, s, len);\n            out->{n}_len = len;{nul}").unwrap();
            }
            (Kind::String(max), Some(count)) => {
                writeln!(o, "            if (t != DK_WIRE_LEN)\n                return -1;\n            s = dk_wire_get_len(&r, &len);\n            if (r.err || len > {max}u || out->{n}_count == {count}u)\n                return -1;\n            dk_wire_copy(out->{n}[out->{n}_count], s, len);\n            out->{n}[out->{n}_count][len] = 0;\n            out->{n}_len[out->{n}_count++] = len;").unwrap();
            }
            (Kind::Message(m), None) => {
                let inner = self.ty(*m);
                writeln!(o, "            if (t != DK_WIRE_LEN)\n                return -1;\n            s = dk_wire_get_len(&r, &len);\n            if (r.err || {inner}_decode(s, len, &out->{n}) < 0)\n                return -1;\n            out->has_{n} = 1;").unwrap();
            }
            (Kind::Message(m), Some(max)) => {
                let inner = self.ty(*m);
                writeln!(o, "            if (t != DK_WIRE_LEN)\n                return -1;\n            s = dk_wire_get_len(&r, &len);\n            if (r.err || out->{n}_count == {max}u || {inner}_decode(s, len, &out->{n}[out->{n}_count]) < 0)\n                return -1;\n            out->{n}_count++;").unwrap();
            }
            (Kind::Bytes(_), Some(_)) => unreachable!("refused by the schema"),
        }
        writeln!(o, "            break;").unwrap();
    }
}
