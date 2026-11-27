use crate::build::workbench::fixture::{vector_descriptors, vector_values};
use crate::build::workbench::schema::Schema;

#[test]
fn the_shared_vectors_agree_with_the_accept_rules() {
    let mut schemas = std::collections::HashMap::new();
    for d in vector_descriptors() {
        schemas.insert(
            d.name.clone(),
            Schema::parse(&d.toml, &d.module, &d.module_name).unwrap(),
        );
    }
    let values = vector_values();
    assert!(values.iter().any(|v| v.valid) && values.iter().any(|v| !v.valid));
    for v in values {
        let kind = schemas[&v.descriptor].setting(&v.setting).unwrap().kind();
        assert_eq!(
            kind.accepts(&v.value),
            v.valid,
            "{}: {} {:02x?}",
            v.descriptor,
            v.rule,
            v.value
        );
    }
}
