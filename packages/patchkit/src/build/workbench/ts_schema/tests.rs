use super::*;
use crate::build::workbench::fixture::{vector_descriptors, write_expected_schemas};
use serde_json::{Value, json};

/// The `schema` constant of the generated file as data: the same fields and values `TsSchema::render` writes.
fn schema_data(schema: &Schema) -> Value {
    let settings: Vec<Value> = schema
        .settings()
        .iter()
        .map(|s| {
            let mut v = json!({
                "id": s.id(),
                "label": s.label(),
                "kind": s.kind().name(),
                "apply": s.apply().name(),
            });
            let fields = match s.kind() {
                Kind::Text { max_bytes } => json!({ "maxBytes": max_bytes }),
                Kind::FileName {
                    max_bytes,
                    extension,
                    reset_label,
                } => json!({ "maxBytes": max_bytes, "extension": extension, "resetLabel": reset_label }),
                Kind::Integer {
                    min,
                    max,
                    step,
                    decimals,
                    unit,
                    default,
                } => json!({
                    "min": min, "max": max, "step": step, "decimals": decimals, "unit": unit, "default": default
                }),
                Kind::Boolean { default } => json!({ "default": default }),
                Kind::Choice { labels, default } => {
                    let texts: Vec<&str> = labels.iter().map(|l| l.text()).collect();
                    json!({ "choices": texts, "default": texts[usize::from(*default)] })
                }
            };
            v.as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            v
        })
        .collect();
    json!({ "module": schema.module(), "name": schema.name(), "version": schema.descriptor_version(), "settings": settings })
}

fn vector_schema(d: &crate::build::workbench::fixture::VectorDescriptor) -> Value {
    schema_data(&Schema::parse(&d.toml, &d.module, &d.module_name).unwrap())
}

/// `UPDATE_VECTORS=1 cargo test ... the_shared_vectors_give_the_generated_schema` rewrites `expected_schema` in the vectors file.
#[test]
fn the_shared_vectors_give_the_generated_schema() {
    if std::env::var_os("UPDATE_VECTORS").is_some() {
        write_expected_schemas(vector_schema);
    }
    for d in vector_descriptors() {
        assert_eq!(d.expected_schema, vector_schema(&d), "{}", d.name);
    }
}
