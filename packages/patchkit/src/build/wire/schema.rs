//! The subset of proto3 that module queries may use, checked from a `.proto` file (parsed by `protox`, no protoc).
//!
//! Accepted: proto3; `uint32`, `int32`, `bool`, enums; `string` and `bytes` with `(dk.max_len)`; nested and top-level messages;
//! `repeated` with `(dk.max_count)` (scalars packed, as proto3 writes them). Everything else is refused with the field named.

use crate::error::*;
use prost_reflect::{
    DescriptorPool, ExtensionDescriptor, FieldDescriptor, Kind as PbKind, MessageDescriptor,
    Syntax, Value,
};
use std::path::Path;

/// The type of a scalar field and its C member type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Scalar {
    Uint32,
    Int32,
    Bool,
    Enum,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Scalar(Scalar),
    /// A UTF-8 string of at most this many bytes (not validated on the device).
    String(u32),
    Bytes(u32),
    /// The index of the message in `Schema::messages`.
    Message(usize),
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    pub(super) name: String,
    pub(super) number: u32,
    pub(super) kind: Kind,
    /// `Some(max_count)` for a repeated field.
    pub(super) repeated: Option<u32>,
}

#[derive(Clone, Debug)]
pub(super) struct Message {
    /// The C name part: the proto name without the package, nested names joined by `_`.
    pub(super) name: String,
    pub(super) fields: Vec<Field>,
    /// Another message holds this one as a field, so it needs a `_size` function.
    pub(super) nested: bool,
}

#[derive(Clone, Debug)]
pub(super) struct EnumDef {
    pub(super) values: Vec<(String, i32)>,
}

/// One `rpc` of the service: its position in the service (from 1) is the method number of the query table.
#[derive(Clone, Debug)]
pub(super) struct Method {
    pub(super) name: String,
    /// The C name part of the handler the author writes: `ListAll` gives `list_all`.
    pub(super) handler: String,
    /// Indexes into `Schema::messages`.
    pub(super) request: usize,
    pub(super) reply: usize,
}

#[derive(Clone, Debug)]
pub(super) struct Service {
    pub(super) name: String,
    pub(super) methods: Vec<Method>,
}

/// The checked messages of one `.proto` file, every message after the messages it holds.
#[derive(Debug)]
pub struct Schema {
    /// The proto package (`pm`); the C names start with it.
    pub(super) package: String,
    pub(super) messages: Vec<Message>,
    pub(super) enums: Vec<EnumDef>,
    pub(super) service: Option<Service>,
}

fn length_option(pool: &DescriptorPool, name: &str) -> Result<ExtensionDescriptor> {
    pool.get_extension_by_name(name)
        .ok_or_else(|| err!("the schema must import dk/options.proto (sdk/proto) for {name}"))
}

impl Schema {
    /// The schema of `file` (a path under one of `includes`; `dk/options.proto` must be among them).
    pub fn read(includes: &[&Path], file: &str) -> Result<Schema> {
        let mut compiler = protox::Compiler::new(includes).map_err(|e| err!("{file}: {e}"))?;
        compiler.include_imports(true);
        compiler.open_file(file).map_err(|e| err!("{file}: {e}"))?;
        Self::from_pool(&compiler.descriptor_pool(), file)
    }

    /// The descriptor set of `file` and its imports, with the `dk` options kept (input of the TypeScript generator).
    pub fn descriptor_set(includes: &[&Path], file: &str) -> Result<Vec<u8>> {
        let mut compiler = protox::Compiler::new(includes).map_err(|e| err!("{file}: {e}"))?;
        compiler.include_imports(true);
        compiler.open_file(file).map_err(|e| err!("{file}: {e}"))?;
        Ok(compiler.descriptor_pool().encode_to_vec())
    }

    fn from_pool(pool: &DescriptorPool, file: &str) -> Result<Schema> {
        let fd = pool
            .get_file_by_name(file)
            .ok_or_else(|| err!("{file}: not in the compiled pool"))?;
        if fd.syntax() != Syntax::Proto3 {
            return Err(err!("{file}: only proto3 is accepted"));
        }
        let max_len = length_option(pool, "dk.max_len")?;
        let max_count = length_option(pool, "dk.max_count")?;
        let mut schema = Schema {
            package: fd.package_name().to_string(),
            messages: vec![],
            enums: vec![],
            service: None,
        };
        let mut all: Vec<MessageDescriptor> = vec![];
        for m in fd.messages() {
            collect(&m, &mut all);
        }
        for e in pool.all_enums().filter(|e| e.parent_file() == fd) {
            let values = e
                .values()
                .map(|v| (v.name().to_string(), v.number()))
                .collect();
            schema.enums.push(EnumDef { values });
        }
        let mut done: Vec<String> = vec![];
        let mut stack: Vec<String> = vec![];
        for m in &all {
            schema.add(m, &all, &(&max_len, &max_count), &mut done, &mut stack)?;
        }
        schema.service = schema.read_service(&fd)?;
        Ok(schema)
    }

    /// The `service` of the file: none, or one (a module has one).
    fn read_service(&self, fd: &prost_reflect::FileDescriptor) -> Result<Option<Service>> {
        let mut services = fd.services();
        let Some(s) = services.next() else {
            return Ok(None);
        };
        if let Some(second) = services.next() {
            return Err(err!(
                "{}: a module has one service, this file has {} and {}",
                fd.name(),
                s.name(),
                second.name()
            ));
        }
        let package = fd.package_name();
        let message = |m: &MessageDescriptor, who: &str| -> Result<usize> {
            let name = local_name(m.full_name(), package);
            self.messages
                .iter()
                .position(|x| x.name == name)
                .ok_or_else(|| err!("{who}: message {} is from another file", m.full_name()))
        };
        let mut methods = vec![];
        for m in s.methods() {
            let who = m.full_name().to_string();
            if m.is_client_streaming() || m.is_server_streaming() {
                return Err(err!("{who}: a streaming method is not accepted"));
            }
            let handler = snake_case(m.name());
            if methods.iter().any(|x: &Method| x.handler == handler) {
                return Err(err!("{who}: the handler name {handler} is used twice"));
            }
            methods.push(Method {
                name: m.name().to_string(),
                handler,
                request: message(&m.input(), &who)?,
                reply: message(&m.output(), &who)?,
            });
        }
        if methods.len() > 255 {
            return Err(err!(
                "{}: a method number is one byte (at most 255 methods)",
                s.full_name()
            ));
        }
        Ok(Some(Service {
            name: s.name().to_string(),
            methods,
        }))
    }

    /// Adds `m` after the messages it holds; a message that holds itself, directly or not, is refused.
    fn add(
        &mut self,
        m: &MessageDescriptor,
        all: &[MessageDescriptor],
        opts: &(&ExtensionDescriptor, &ExtensionDescriptor),
        done: &mut Vec<String>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        let full = m.full_name().to_string();
        if done.contains(&full) {
            return Ok(());
        }
        if stack.contains(&full) {
            return Err(err!(
                "{full}: a message that contains itself has no fixed size"
            ));
        }
        stack.push(full.clone());
        let package = m.package_name().to_string();
        let mut fields = vec![];
        for f in m.fields() {
            if f.is_map() {
                return Err(err!("{}: a map is not accepted", field_name(&f)));
            }
            if let PbKind::Message(inner) = f.kind() {
                if !all.iter().any(|a| a.full_name() == inner.full_name()) {
                    return Err(err!(
                        "{}: message {} is from another file",
                        field_name(&f),
                        inner.full_name()
                    ));
                }
                self.add(&inner, all, opts, done, stack)?;
            }
            fields.push(self.field(&f, opts, &package)?);
        }
        for f in &fields {
            if let Kind::Message(i) = f.kind {
                self.messages[i].nested = true;
            }
        }
        let mut seen: Vec<String> = vec![];
        for f in &fields {
            for member in members(f) {
                if seen.contains(&member) {
                    return Err(err!("{full}: the C member name {member} is used twice"));
                }
                seen.push(member);
            }
        }
        stack.pop();
        done.push(full.clone());
        self.messages.push(Message {
            name: local_name(&full, &package),
            fields,
            nested: false,
        });
        Ok(())
    }

    fn field(
        &self,
        f: &FieldDescriptor,
        (max_len, max_count): &(&ExtensionDescriptor, &ExtensionDescriptor),
        package: &str,
    ) -> Result<Field> {
        let who = field_name(f);
        if f.is_group() || f.is_required() {
            return Err(err!("{who}: groups and required fields are not accepted"));
        }
        if f.containing_oneof().is_some() {
            return Err(err!("{who}: oneof (and proto3 optional) is not accepted"));
        }
        let option = |ext: &ExtensionDescriptor| -> Result<Option<u32>> {
            let options = f.options();
            if !options.has_extension(ext) {
                return Ok(None);
            }
            match options.get_extension(ext).as_ref() {
                Value::U32(0) => Err(err!("{who}: {} must be at least 1", ext.full_name())),
                Value::U32(n) => Ok(Some(*n)),
                _ => Err(err!("{who}: {} is not a number", ext.full_name())),
            }
        };
        let len = option(max_len)?;
        let count = option(max_count)?;
        let kind = match f.kind() {
            PbKind::Uint32 => Kind::Scalar(Scalar::Uint32),
            PbKind::Int32 => Kind::Scalar(Scalar::Int32),
            PbKind::Bool => Kind::Scalar(Scalar::Bool),
            PbKind::Enum(_) => Kind::Scalar(Scalar::Enum),
            PbKind::String | PbKind::Bytes => {
                let n =
                    len.ok_or_else(|| err!("{who}: a string or bytes field needs (dk.max_len)"))?;
                if matches!(f.kind(), PbKind::String) {
                    Kind::String(n)
                } else {
                    Kind::Bytes(n)
                }
            }
            PbKind::Message(m) => {
                let name = local_name(m.full_name(), package);
                Kind::Message(
                    self.messages
                        .iter()
                        .position(|x| x.name == name)
                        .expect("added first"),
                )
            }
            other => {
                return Err(err!(
                    "{who}: type {other:?} is not accepted (only uint32, int32, bool, enum, string, bytes, message)"
                ));
            }
        };
        if len.is_some() && !matches!(kind, Kind::String(_) | Kind::Bytes(_)) {
            return Err(err!("{who}: (dk.max_len) is for string and bytes only"));
        }
        let repeated = if f.is_list() {
            let n = count.ok_or_else(|| err!("{who}: a repeated field needs (dk.max_count)"))?;
            // prost-reflect's `is_packed` is wrong for a field with a custom option: read `packed` itself (proto3 packs by default)
            let unpacked = f.options().has_field_by_name("packed")
                && f.options()
                    .get_field_by_name("packed")
                    .is_some_and(|v| v.as_bool() == Some(false));
            if matches!(kind, Kind::Scalar(_)) && unpacked {
                return Err(err!("{who}: a repeated scalar must be packed"));
            }
            if matches!(kind, Kind::Bytes(_)) {
                return Err(err!("{who}: repeated bytes is not accepted"));
            }
            Some(n)
        } else {
            if count.is_some() {
                return Err(err!("{who}: (dk.max_count) is for repeated fields only"));
            }
            None
        };
        Ok(Field {
            name: f.name().to_string(),
            number: f.number(),
            kind,
            repeated,
        })
    }
}

fn field_name(f: &FieldDescriptor) -> String {
    f.full_name().to_string()
}

/// `pm.Outer.Inner` in package `pm` -> `Outer_Inner`.
fn local_name(full: &str, package: &str) -> String {
    let rest = full
        .strip_prefix(package)
        .map_or(full, |r| r.trim_start_matches('.'));
    rest.replace('.', "_")
}

/// `ListAll` -> `list_all`, `GetURL` -> `get_url`.
fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower = i > 0 && !chars[i - 1].is_ascii_uppercase() && chars[i - 1] != '_';
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            if i > 0 && chars[i - 1] != '_' && (prev_lower || next_lower) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

fn collect(m: &MessageDescriptor, out: &mut Vec<MessageDescriptor>) {
    if m.is_map_entry() {
        return;
    }
    out.push(m.clone());
    for inner in m.child_messages() {
        collect(&inner, out);
    }
}

/// The C struct members a field takes.
pub(super) fn members(f: &Field) -> Vec<String> {
    let n = &f.name;
    match (&f.kind, f.repeated) {
        (Kind::String(_) | Kind::Bytes(_), None) => vec![n.clone(), format!("{n}_len")],
        (Kind::String(_), Some(_)) => vec![n.clone(), format!("{n}_len"), format!("{n}_count")],
        (Kind::Message(_), None) => vec![n.clone(), format!("has_{n}")],
        (_, Some(_)) => vec![n.clone(), format!("{n}_count")],
        _ => vec![n.clone()],
    }
}

#[cfg(test)]
mod tests;
