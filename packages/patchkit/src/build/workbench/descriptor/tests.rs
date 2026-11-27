use super::*;
use crate::build::workbench::fixture::vector_descriptors;

#[test]
fn the_shared_vectors_give_the_descriptor_bytes() {
    for d in vector_descriptors() {
        let bytes = Schema::parse(&d.toml, &d.module, &d.module_name)
            .unwrap()
            .descriptor()
            .to_vec();
        assert_eq!(bytes, d.bytes, "{}", d.name);
    }
}
