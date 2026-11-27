use super::*;
use crate::build::toolchain::TempDir;
use crate::build::workbench::fixture::DECLARATION;

#[test]
fn the_file_is_written_under_workbench_gen_and_checked_against_the_text() {
    let dir = TempDir::new("pk-ts-file-").unwrap();
    std::fs::write(dir.path().join("workbench.toml"), DECLARATION).unwrap();
    let file = TsFile::read(dir.path(), "test/m", Some("M"))
        .unwrap()
        .unwrap();
    assert_eq!(file.path(), dir.path().join("workbench/gen/settings.ts"));
    assert!(!file.is_current());
    file.write().unwrap();
    assert!(file.is_current());
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), file.text());
    std::fs::write(file.path(), "edited").unwrap();
    assert!(!file.is_current());
}

#[test]
fn a_module_without_workbench_toml_has_no_file() {
    let dir = TempDir::new("pk-ts-file-").unwrap();
    assert!(
        TsFile::read(dir.path(), "test/m", Some("M"))
            .unwrap()
            .is_none()
    );
}
