use super::*;

#[test]
fn equal_versions_fit() {
    assert_eq!(check((1, 3), (1, 3)), Ok(()));
}

#[test]
fn an_older_package_minor_fits() {
    assert_eq!(check((1, 2), (1, 3)), Ok(()));
}

#[test]
fn a_newer_package_minor_is_refused() {
    assert_eq!(check((1, 4), (1, 3)), Err(VersionMismatch::Minor));
}

#[test]
fn another_major_is_refused_whatever_the_minor() {
    assert_eq!(check((2, 0), (1, 5)), Err(VersionMismatch::Major));
    assert_eq!(check((1, 0), (2, 0)), Err(VersionMismatch::Major));
}
