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

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../../../packages/module-ui/tests/fixtures/save-names.json"
    ))
    .unwrap()
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn pack_names(names: &[String]) -> Vec<u8> {
    let list: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    pack(&list)
}

#[test]
fn the_names_of_the_shared_fixture_are_accepted_and_refused() {
    let fx = fixture();
    for name in strings(&fx["accept"]) {
        assert!(check_name(&name).is_ok(), "accept {name:?}");
    }
    for name in strings(&fx["refuse"]) {
        assert!(check_name(&name).is_err(), "refuse {name:?}");
    }
}

#[test]
fn a_refused_name_refuses_the_whole_list_before_any_write() {
    let dir = folder("bad").join("inner");
    std::fs::create_dir_all(&dir).unwrap();
    for bad in strings(&fixture()["refuse"]) {
        let err = write_all(&dir, &pack(&[("first.bin", b"1"), (bad.as_str(), b"2")])).unwrap_err();
        assert!(!err.is_empty());
        assert!(!dir.join("first.bin").exists(), "{bad:?}");
    }
    assert!(!dir.parent().unwrap().join("x").exists());
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn only_one_normal_component_is_joined() {
    assert!(single_component("a.bin"));
    for bad in ["", ".", "..", "a/b", "/a", "../a"] {
        assert!(!single_component(bad), "{bad:?}");
    }
}

#[test]
fn names_that_differ_only_in_case_are_refused() {
    let dir = folder("case");
    let fx = fixture();
    for list in fx["acceptLists"].as_array().unwrap() {
        assert!(
            write_all(&dir, &pack_names(&strings(list))).is_ok(),
            "{list}"
        );
    }
    for list in fx["refuseLists"].as_array().unwrap() {
        let err = write_all(&dir, &pack_names(&strings(list))).unwrap_err();
        assert!(err.contains("twice"), "{list}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_count_and_the_total_size_are_limited_before_any_write() {
    let dir = folder("limits");
    let fx = fixture();
    let max_files = fx["limits"]["maxFiles"].as_u64().unwrap() as usize;
    let max_total = fx["limits"]["maxTotalBytes"].as_u64().unwrap() as usize;
    let names = |n: usize| (0..n).map(|i| format!("f{i}")).collect::<Vec<_>>();
    assert_eq!(
        write_all(&dir, &pack_names(&names(max_files))).unwrap() as usize,
        max_files
    );
    let dir2 = folder("limits2");
    assert!(write_all(&dir2, &pack_names(&names(max_files + 1)))
        .unwrap_err()
        .contains("at most"));
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
    let big = vec![0u8; max_total / 2 + 1];
    let err = write_all(&dir2, &pack(&[("a", &big), ("b", &big)])).unwrap_err();
    assert!(err.contains("at most"), "{err}");
    assert_eq!(std::fs::read_dir(&dir2).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[cfg(unix)]
#[test]
fn a_symlink_target_is_refused_and_nothing_is_written() {
    let dir = folder("link");
    let outside = folder("link-outside").join("victim");
    std::fs::write(&outside, b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("b.bin")).unwrap();
    let err = write_all(&dir, &pack(&[("a.bin", b"1"), ("b.bin", b"2")])).unwrap_err();
    assert!(err.contains("symbolic link"), "{err}");
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    assert!(!dir.join("a.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(outside.parent().unwrap());
}

#[test]
fn a_directory_target_is_refused_and_no_temp_file_stays() {
    let dir = folder("dirtarget");
    std::fs::create_dir(dir.join("d")).unwrap();
    assert!(write_all(&dir, &pack(&[("d", b"1")])).is_err());
    write_all(&dir, &pack(&[("ok", b"1")])).unwrap();
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["d", "ok"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cut_list_is_refused() {
    let dir = folder("cut");
    let mut packed = pack(&[("a.bin", b"data")]);
    packed.truncate(packed.len() - 1);
    assert!(write_all(&dir, &packed).unwrap_err().contains("cut short"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_at_a_temporary_path_is_not_followed() {
    let dir = folder("temp-link");
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"keep").unwrap();
    // Every temporary name the old scheme could pick, planted as a link to the victim.
    for i in 0..2 {
        let name = format!(".digikit-save-{}-{i}.tmp", std::process::id());
        std::os::unix::fs::symlink(&victim, dir.join(name)).unwrap();
    }
    write_all(&dir, &pack(&[("a.bin", b"new"), ("b.bin", b"new")])).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), b"new");
    let _ = std::fs::remove_dir_all(&dir);
}
