use super::*;
use patchkit::build::toolchain::TempDir;

const MODULE: &str = "[module]\nname = \"Typed\"\nid = \"test/typed\"\ncategory = \"test\"\n";
const WORKBENCH: &str = "[workbench]\nversion = 1\n[[setting]]\nid = \"on\"\nlabel = \"On\"\ntype = \"boolean\"\ndefault = true\n";

fn args(dir: &Path, flag: &str) -> Vec<String> {
    let mut v = vec![dir.display().to_string()];
    v.extend((!flag.is_empty()).then(|| flag.to_string()));
    v
}

fn module() -> TempDir {
    let dir = TempDir::new("pk-ts-cli-").unwrap();
    std::fs::write(dir.path().join("module.toml"), MODULE).unwrap();
    std::fs::write(dir.path().join("workbench.toml"), WORKBENCH).unwrap();
    dir
}

#[test]
fn check_fails_with_the_path_until_write_has_made_the_file() {
    let dir = module();
    let target = dir.path().join("workbench/gen/settings.ts");
    let error = run(&args(dir.path(), "--check")).unwrap_err();
    assert!(error.0.contains(&target.display().to_string()), "{error}");
    run(&args(dir.path(), "--write")).unwrap();
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("export const schema") && text.contains("  on: boolean;"));
    run(&args(dir.path(), "--check")).unwrap();
    std::fs::write(&target, "stale").unwrap();
    assert!(run(&args(dir.path(), "--check")).is_err());
    // the check changes nothing
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "stale");
}

#[test]
fn a_module_without_workbench_toml_is_left_alone() {
    let dir = TempDir::new("pk-ts-cli-").unwrap();
    std::fs::write(dir.path().join("module.toml"), MODULE).unwrap();
    for flag in ["--check", "--write", ""] {
        run(&args(dir.path(), flag)).unwrap();
    }
    assert!(!dir.path().join("workbench").exists());
}

#[test]
fn the_arguments_are_checked() {
    let dir = module();
    assert!(run(&[]).is_err());
    assert!(run(&["--write".into()]).is_err());
    assert!(run(&[dir.path().display().to_string(), "--bogus".into()]).is_err());
    let mut both = args(dir.path(), "--write");
    both.push("--check".into());
    assert!(run(&both).is_err());
}
