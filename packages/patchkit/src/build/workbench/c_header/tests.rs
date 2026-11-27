use super::*;
use crate::build::workbench::fixture::DECLARATION;

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
decimals = 1
unit = "dB"
default = 0

[[setting]]
id = "mix-on"
label = "Mix on"
type = "boolean"
default = true

[[setting]]
id = "mode"
label = "Mode"
type = "choice"
choices = ["Off", "Soft clip", "Hard"]
default = "Soft clip"

[[setting]]
id = "name"
label = "Name"
type = "text"
max_bytes = 8
"#;

fn render(source: &str) -> String {
    CHeader::render(&Schema::parse(source, "test/typed", "Typed").unwrap())
}

#[test]
fn the_header_holds_the_descriptor_and_a_key_per_setting() {
    let header = render(DECLARATION);
    assert!(header.contains("#pragma once"));
    assert!(header.contains("static const uint8_t dk_workbench_descriptor[] = {\n    0x01, 0x05, 0x54, 0x79, 0x70, 0x65, 0x64, 0x02,"));
    assert!(
        header.contains(
            "#define WB_SETTING_BOOT_KEY \"boot\"\n#define WB_SETTING_BOOT_MAX_BYTES 16\n"
        )
    );
    assert!(header.contains("#define WB_SETTING_ACTIVE_MAX_BYTES 47\n"));
    // text settings get no accessors
    assert!(!header.contains("wb_setting_boot_"));
}

#[test]
fn an_integer_has_bounds_defaults_and_accessors() {
    let header = render(TYPED);
    for line in [
        "#define WB_SETTING_GAIN_KEY \"gain\"",
        "#define WB_SETTING_GAIN_MIN (-10)",
        "#define WB_SETTING_GAIN_MAX 10",
        "#define WB_SETTING_GAIN_STEP 2u",
        "#define WB_SETTING_GAIN_DEFAULT 0",
        "static inline int32_t wb_setting_gain_get(dk_settings *s)",
        "static inline int32_t wb_setting_gain_put(dk_settings *s, int32_t v)",
        "return DK_E_SET_VALUE;",
    ] {
        assert!(header.contains(line), "{line}\n{header}");
    }
}

#[test]
fn an_i32_minimum_is_written_without_an_out_of_range_literal() {
    assert_eq!(i32_literal(i32::MIN), "(-2147483647 - 1)");
    assert_eq!(i32_literal(i32::MAX), "2147483647");
    assert_eq!(i32_literal(-1), "(-1)");
    assert_eq!(i32_literal(0), "0");
}

#[test]
fn a_boolean_and_a_choice_have_accessors_and_names() {
    let header = render(TYPED);
    for line in [
        "#define WB_SETTING_MIX_ON_DEFAULT 1",
        "static inline int wb_setting_mix_on_get(dk_settings *s)",
        "static inline int32_t wb_setting_mix_on_put(dk_settings *s, int v)",
        "#define WB_SETTING_MODE_COUNT 3",
        "#define WB_SETTING_MODE_DEFAULT 1",
        "    WB_SETTING_MODE_OFF = 0,",
        "    WB_SETTING_MODE_SOFT_CLIP = 1,",
        "    WB_SETTING_MODE_HARD = 2,",
        "static inline uint8_t wb_setting_mode_get(dk_settings *s)",
        "static inline int32_t wb_setting_mode_put(dk_settings *s, uint8_t v)",
    ] {
        assert!(header.contains(line), "{line}\n{header}");
    }
}

#[test]
fn no_free_text_reaches_a_c_comment() {
    // accept: a label with comment marks is a valid label and leaves no trace of itself in the header
    let source = DECLARATION.replace("Animation", "A /* b */ c");
    let header = render(&source);
    assert!(!header.contains("A /*") && !header.contains("b */"));
    // only the generated-by line is a comment
    assert_eq!(header.matches("/*").count(), 1, "{header}");
    assert_eq!(header.matches("*/").count(), 1, "{header}");
    // refuse: the same text in a choice label sanitises to a name, never to a comment
    let source = TYPED.replace("Hard", "Hard /* x");
    let header = render(&source);
    assert_eq!(header.matches("/*").count(), 1, "{header}");
    assert!(header.contains("WB_SETTING_MODE_HARD_X = 2,"), "{header}");
}
