use super::*;
use crate::build::workbench::fixture::DECLARATION;
use crate::build::{
    BuildOptions, Status, build_module, load_object, manifest, module_status, object_files,
    toolchain::Env,
};

/// Writes a module whose module.c is `source` and returns the directory.
fn module_dir(prefix: &str, workbench: &str, source: &str) -> TempDir {
    let dir = TempDir::new(prefix).unwrap();
    let text = format!(
        r#"
[module]
name = "A label"
id = "test/{}"
category = "test"

[[object]]
path = "build/test.dko.json"
sources = ["module.c"]
"#,
        std::process::id()
    );
    std::fs::write(dir.path().join("module.toml"), text).unwrap();
    std::fs::write(dir.path().join("workbench.toml"), workbench).unwrap();
    std::fs::write(dir.path().join("module.c"), source).unwrap();
    dir
}

#[test]
fn the_coldfire_cache_tracks_an_external_modules_workbench_source_and_name() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let env = Env::new(&root);
    if !env.m68k_or_skip() {
        return;
    }
    let dir = module_dir(
        "pk-workbench-test-",
        DECLARATION,
        "#include \"workbench_descriptor.h\"\nconst uint8_t *descriptor(void) { return dk_workbench_descriptor; }\n",
    );
    let module = dir.path().join("module.toml");
    let workbench = dir.path().join("workbench.toml");
    let text = std::fs::read_to_string(&module).unwrap();
    let object_path = || object_files(&root, dir.path()).unwrap().remove(0).1;
    let before = object_path();
    assert_eq!(object_path(), before);
    let opts = BuildOptions {
        root: root.clone(),
        ..Default::default()
    };
    let built = build_module(dir.path(), &opts).unwrap().objects.remove(0);
    assert_eq!(built.path, before);
    assert_eq!(
        module_status(&root, dir.path()).unwrap(),
        [("build/test.dko.json".into(), true)]
    );
    assert_eq!(
        build_module(dir.path(), &opts).unwrap().objects[0].status,
        Status::Unchanged
    );

    std::fs::write(&workbench, DECLARATION.replace("next-restart", "immediate")).unwrap();
    let modified = object_path();
    assert_ne!(modified, before);
    assert!(!modified.exists());
    let stored = load_object(
        &env,
        dir.path(),
        &manifest(dir.path()).unwrap(),
        "build/test.dko.json",
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&modified).unwrap(), stored);
    std::fs::write(&module, text.replace("A label", "Another label")).unwrap();
    assert_ne!(object_path(), modified);
    std::fs::remove_file(&workbench).unwrap();
    assert!(object_files(&root, dir.path()).is_err());
}

const TYPED: &str = r#"
[workbench]
version = 1

[[setting]]
id = "gain"
label = "Gain"
type = "integer"
min = -10
max = 10
step = 2
default = 4

[[setting]]
id = "mix-on"
label = "Mix on"
type = "boolean"
default = true

[[setting]]
id = "mode"
label = "Mode"
type = "choice"
choices = ["Off", "Soft", "Hard"]
default = "Soft"

[[setting]]
id = "name"
label = "Name"
type = "text"
max_bytes = 8
"#;

/// A host program that includes the generated header over a one-key store and calls every accessor.
const PROGRAM: &str = r#"
#include <stdio.h>
#include <string.h>
#include "workbench_descriptor.h"

struct dk_settings { char key[48]; uint8_t value[8]; int32_t len; };
static struct dk_settings store;
int32_t dk_settings_get(dk_settings *s, const char *key, void *out, uint32_t cap) {
    if (strcmp(s->key, key) != 0) return DK_E_SET_NOKEY;
    if ((uint32_t)s->len > cap) return DK_E_SET_VALUE;
    memcpy(out, s->value, (size_t)s->len);
    return s->len;
}
int32_t dk_settings_put(dk_settings *s, const char *key, const void *value, uint32_t len) {
    strcpy(s->key, key);
    memcpy(s->value, value, len);
    s->len = (int32_t)len;
    return 0;
}
static void set(const char *key, const char *bytes, int len) {
    strcpy(store.key, key);
    memcpy(store.value, bytes, (size_t)len);
    store.len = len;
}
static int failed;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); failed = 1; } } while (0)

int main(void) {
    dk_settings *s = &store;
    /* integer: the default when unset, the value when stored, the default for each kind of bad record */
    CHECK(wb_setting_gain_get(s) == WB_SETTING_GAIN_DEFAULT && WB_SETTING_GAIN_DEFAULT == 4);
    set(WB_SETTING_GAIN_KEY, "\xff\xff\xff\xf6", 4);
    CHECK(wb_setting_gain_get(s) == -10);
    set(WB_SETTING_GAIN_KEY, "\x00\x00\x00\x0a", 4);
    CHECK(wb_setting_gain_get(s) == 10);
    set(WB_SETTING_GAIN_KEY, "\x00\x00\x00\x0c", 4);
    CHECK(wb_setting_gain_get(s) == 4);
    set(WB_SETTING_GAIN_KEY, "\x00\x00\x00\x03", 4);
    CHECK(wb_setting_gain_get(s) == 4);
    set(WB_SETTING_GAIN_KEY, "\x00\x00\x08", 3);
    CHECK(wb_setting_gain_get(s) == 4);
    CHECK(wb_setting_gain_put(s, -2) == 0 && wb_setting_gain_get(s) == -2);
    CHECK(memcmp(store.value, "\xff\xff\xff\xfe", 4) == 0 && store.len == 4);
    CHECK(wb_setting_gain_put(s, 3) == DK_E_SET_VALUE);
    CHECK(wb_setting_gain_put(s, 12) == DK_E_SET_VALUE);
    CHECK(wb_setting_gain_put(s, -12) == DK_E_SET_VALUE);
    CHECK(wb_setting_gain_get(s) == -2);
    /* boolean */
    memset(&store, 0, sizeof store);
    CHECK(wb_setting_mix_on_get(s) == 1);
    set(WB_SETTING_MIX_ON_KEY, "\x00", 1);
    CHECK(wb_setting_mix_on_get(s) == 0);
    set(WB_SETTING_MIX_ON_KEY, "\x02", 1);
    CHECK(wb_setting_mix_on_get(s) == 1);
    CHECK(wb_setting_mix_on_put(s, 0) == 0 && wb_setting_mix_on_get(s) == 0);
    CHECK(wb_setting_mix_on_put(s, 2) == DK_E_SET_VALUE);
    CHECK(wb_setting_mix_on_put(s, -1) == DK_E_SET_VALUE);
    /* choice */
    memset(&store, 0, sizeof store);
    CHECK(wb_setting_mode_get(s) == WB_SETTING_MODE_SOFT);
    set(WB_SETTING_MODE_KEY, "\x02", 1);
    CHECK(wb_setting_mode_get(s) == WB_SETTING_MODE_HARD);
    set(WB_SETTING_MODE_KEY, "\x03", 1);
    CHECK(wb_setting_mode_get(s) == WB_SETTING_MODE_SOFT);
    CHECK(wb_setting_mode_put(s, WB_SETTING_MODE_OFF) == 0 && wb_setting_mode_get(s) == 0);
    CHECK(wb_setting_mode_put(s, WB_SETTING_MODE_COUNT) == DK_E_SET_VALUE);
    CHECK(wb_setting_mode_put(s, 255) == DK_E_SET_VALUE);
    CHECK(WB_SETTING_NAME_MAX_BYTES == 8 && dk_workbench_descriptor[0] == 2);
    puts(failed ? "FAILED" : "OK");
    return failed;
}
"#;

#[test]
fn the_generated_header_compiles_and_its_accessors_follow_the_rules() {
    let Ok(cc) = std::process::Command::new("cc").arg("--version").output() else {
        eprintln!("SKIP: no host C compiler");
        return;
    };
    assert!(cc.status.success());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = module_dir("pk-workbench-cc-", TYPED, PROGRAM);
    let header = Header::read(dir.path(), "test/typed", Some("Typed"))
        .unwrap()
        .unwrap();
    let exe = dir.path().join("check");
    let build = std::process::Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(header.dir())
        .arg("-I")
        .arg(root.join("sdk/include"))
        .arg("-o")
        .arg(&exe)
        .arg(dir.path().join("module.c"))
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = std::process::Command::new(&exe).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
}

#[test]
fn the_digest_changes_with_the_declaration_and_the_name() {
    let dir = module_dir("pk-workbench-digest-", DECLARATION, "");
    let digest = |name: &str| {
        *Header::read(dir.path(), "test/m", Some(name))
            .unwrap()
            .unwrap()
            .digest()
    };
    assert_eq!(digest("A"), digest("A"));
    assert_ne!(digest("A"), digest("B"));
    let before = digest("A");
    std::fs::write(
        dir.path().join("workbench.toml"),
        DECLARATION.replace("16", "17"),
    )
    .unwrap();
    assert_ne!(digest("A"), before);
}
