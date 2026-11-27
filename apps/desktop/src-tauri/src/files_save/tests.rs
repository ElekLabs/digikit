use super::*;

fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, bytes) in files {
        out.extend((name.len() as u32).to_be_bytes());
        out.extend(name.as_bytes());
        out.extend((bytes.len() as u32).to_be_bytes());
        out.extend(*bytes);
    }
    out
}

fn folder(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("digi-files-save-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn files_are_written_in_the_folder_and_replace_an_old_one() {
    let dir = folder("ok");
    std::fs::write(dir.join("a.bin"), b"old").unwrap();
    let n = write_all(&dir, &pack(&[("a.bin", b"new"), ("b.bin", b"")])).unwrap();
    assert_eq!(n, 2);
    assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"new");
    assert_eq!(std::fs::read(dir.join("b.bin")).unwrap(), b"");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_that_leaves_the_folder_refuses_the_whole_list_before_any_write() {
    let dir = folder("bad").join("inner");
    std::fs::create_dir_all(&dir).unwrap();
    for bad in ["../x", "a/b", "a\\b", "..", "", "."] {
        let err = write_all(&dir, &pack(&[("first.bin", b"1"), (bad, b"2")])).unwrap_err();
        assert!(err.contains("plain file name"), "{bad}: {err}");
    }
    assert!(!dir.join("first.bin").exists());
    assert!(!dir.parent().unwrap().join("x").exists());
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn a_cut_list_is_refused() {
    let dir = folder("cut");
    let mut packed = pack(&[("a.bin", b"data")]);
    packed.truncate(packed.len() - 1);
    assert!(write_all(&dir, &packed).unwrap_err().contains("cut short"));
    let _ = std::fs::remove_dir_all(&dir);
}
