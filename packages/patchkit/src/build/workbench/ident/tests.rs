use super::*;

#[test]
fn an_id_maps_to_an_upper_case_c_identifier() {
    assert_eq!(Ident::of_id("gain").unwrap().upper(), "GAIN");
    assert_eq!(Ident::of_id("mix-level_2").unwrap().upper(), "MIX_LEVEL_2");
    assert_eq!(Ident::of_id("Mix").unwrap().lower(), "mix");
    assert_eq!(Ident::of_id("_x").unwrap().upper(), "_X");
}

#[test]
fn an_id_that_is_no_c_identifier_is_refused() {
    for id in ["", "2x", "a.b", "a/b", "a b", "été", ".."] {
        assert!(Ident::of_id(id).is_none(), "{id:?}");
    }
}

#[test]
fn a_label_is_reduced_to_upper_case_words() {
    assert_eq!(Ident::of_label("Soft").unwrap().upper(), "SOFT");
    assert_eq!(Ident::of_label("Hard clip!").unwrap().upper(), "HARD_CLIP");
    assert_eq!(Ident::of_label("  --3 d--  ").unwrap().upper(), "3_D");
    assert_eq!(Ident::of_label("Épais x").unwrap().upper(), "PAIS_X");
}

#[test]
fn a_label_without_letters_or_digits_is_refused() {
    for label in ["", " ", "---", "é"] {
        assert!(Ident::of_label(label).is_none(), "{label:?}");
    }
}
