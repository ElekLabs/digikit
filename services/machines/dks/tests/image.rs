//! The registry words of the image's `rows_info` and the assembler that reads them. They are bound over a `FakeCore` (the core's info entry) into the table of the hooks, refused when
//! they are not this device's or lack the registry, and the byte offsets the assembler files name (`.equ T_*`) are the word indexes of `info::w` (the crate's `info` owns the table:
//! `Provider::start_in` binds it).
use dkrt_svc_machines::info::*;
use dkrt_svc_rows::rows_info::w as r;
use dkrt_svc_rt::name_hash;
use dkrt_test_support::FakeCore;

/// `rows_info` of an image: a registry of three rows from type 7 with the stock list, a reader copy, and the reserved types 8 and 9.
fn rows(device: u32) -> Vec<u32> {
    let mut t = vec![0u32; r::WORDS];
    t[r::DEVICE] = device;
    t[r::M_ROWS] = 0x4200_0000;
    t[r::LIST_STOCK] = 0x4700_0000;
    t[r::M_ROW_SIZE] = 64;
    t[r::O_POS] = 48;
    t[r::M_FIRST] = 7;
    t[r::M_COUNT] = 3;
    t[r::M_ENTRIES] = 2;
    t[r::READER_REAL] = 0x4000_2000;
    t[r::SLOTMAP] = 0x4000_3000;
    t[r::PG_ROWS] = 0x4300_0000;
    t[r::PG_COUNT] = 8;
    t[r::M_DYN0] = 8;
    t[r::M_DYN_N] = 2;
    t
}

/// The bound table the words of `rows(device)` give.
fn bound(device: u32) -> [u32; WORDS] {
    let mut t = [0u32; WORDS];
    t[w::MAGIC] = MAGIC;
    t[w::VERSION] = VERSION;
    t[w::DEVICE] = device;
    t[w::FIRST] = 7;
    t[w::COUNT] = 3;
    t[w::ENTRIES] = 2;
    t[w::ROWS] = 0x4200_0000;
    t[w::SLOTMAP] = 0x4000_3000;
    t[w::PGROWS] = 0x4300_0000;
    t[w::PGN] = 8;
    t[w::READER_REAL] = 0x4000_2000;
    t[w::LIST_STOCK] = 0x4700_0000;
    t[w::ROW_SIZE] = 64;
    t[w::O_POS] = 48;
    t[w::DYN0] = 8;
    t[w::DYN_N] = 2;
    t
}

fn core_with(words: Option<Vec<u32>>, device: u32) -> FakeCore {
    let mut c = FakeCore::new();
    c.device = device;
    if let Some(t) = words {
        c.set_rows_info(t);
    }
    c
}

#[test]
fn the_registry_words_are_copied_with_the_magic_last_and_every_word_in() {
    let core = core_with(Some(rows(2)), 2);
    let mut dst = [0xffff_ffffu32; WORDS];
    assert_eq!(unsafe { bind_table(&core, dst.as_mut_ptr()) }, 0);
    assert_eq!(dst, bound(2));
}

#[test]
fn an_image_without_the_table_another_device_or_no_registry_is_refused() {
    let mut dst = [0u32; WORDS];
    let none = core_with(None, 2);
    assert_eq!(
        unsafe { bind_table(&none, dst.as_mut_ptr()) },
        dkrt_sdk::E_NO_REGISTRY
    );
    let other = core_with(Some(rows(1)), 2);
    assert_eq!(
        unsafe { bind_table(&other, dst.as_mut_ptr()) },
        dkrt_sdk::E_NO_REGISTRY
    );
    let mut bare = rows(2);
    bare[r::M_ROWS] = 0;
    assert_eq!(
        unsafe { bind_table(&core_with(Some(bare), 2), dst.as_mut_ptr()) },
        dkrt_sdk::E_NO_REGISTRY
    );
    // a table of another layout (version 1 had the parameter rows only) is not `rows_info` for this crate
    let mut old = core_with(Some(rows(2)), 2);
    let hash = name_hash("rows_info");
    old.info_tables.iter_mut().find(|(h, _)| *h == hash).unwrap().1[1] = 1;
    assert_eq!(
        unsafe { bind_table(&old, dst.as_mut_ptr()) },
        dkrt_sdk::E_NO_REGISTRY
    );
    assert_eq!(dst, [0u32; WORDS], "nothing is written for a refused table");
}

/// `.equ T_NAME, value` lines of an assembler file.
fn equates(text: &str) -> Vec<(String, usize)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(".equ")?.trim();
            let (name, value) = rest.split_once(',')?;
            let name = name.trim();
            name.starts_with("T_").then(|| {
                (
                    name.to_string(),
                    value.split('|').next().unwrap().trim().parse().unwrap(),
                )
            })
        })
        .collect()
}

#[test]
fn the_offsets_the_assembler_names_are_the_word_indexes_of_the_table() {
    // the common file reads no word of the table any more (its native page entries are Rust); each device's file names the words it reads
    assert!(equates(include_str!("../src/hooks-common.s")).is_empty());
    for file in [
        include_str!("../src/hooks-dt2.s"),
        include_str!("../src/hooks-dn2.s"),
    ] {
        let eq = equates(file);
        assert!(!eq.is_empty());
        for (name, at) in eq {
            let index = ASM_OFFSETS
                .iter()
                .find(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("{name} is not in ASM_OFFSETS"))
                .1;
            assert_eq!(at, 4 * index, "{name}");
        }
    }
}

#[test]
fn the_tail_a_hook_names_is_the_hash_of_its_hook() {
    // `dk_tail_<8 hex digits of name_hash(hook)>` (patchkit `bind::tail_import`) in the assembler: every one is the tail of a hook of the service
    let hooks = [
        "cf.machine.hook.type_page.count",
        "cf.machine.hook.type_page.desc",
        "cf.machine.hook.param_code.map",
        "cf.machine.hook.param_code.args",
        "cf.page.hook.source_draw",
        "cf.page.hook.source_code",
        "cf.page.hook.source_key",
        "cf.page.hook.source_turn",
    ];
    let all = [
        include_str!("../src/hooks-common.s"),
        include_str!("../src/hooks-dt2.s"),
        include_str!("../src/hooks-dn2.s"),
    ]
    .concat();
    for h in hooks {
        let tail = format!("dk_tail_{:08x}", name_hash(h));
        assert!(all.contains(&tail), "{h}: the assembler has no {tail}");
    }
    for (i, _) in all.match_indices("dk_tail_") {
        let t = &all[i..i + 16];
        if !t[8..].bytes().all(|c| c.is_ascii_hexdigit()) {
            continue; // prose: `dk_tail_<hash of the hook>`
        }
        assert!(
            hooks
                .iter()
                .any(|h| t == format!("dk_tail_{:08x}", name_hash(h))),
            "{t} is the tail of no hook"
        );
    }
}

/// The aliases the assembler files use for firmware (`mf_x_*` exits, `mf_ret_*`, `mf_stock_*`, `mf_spt`, `mf_rc<n>`) are exactly the `[object.externs]` of module.toml: an alias
/// without an entry would stay an unresolved import, an entry without a use is stale. Each target resolves to an import family of the port file (`hook.<hook>.exit[.n]|ret|stock|tail`
/// or `cf.<name>`), and its hook is one the service takes or the fill (`frametype`) enters.
#[test]
fn the_firmware_aliases_of_the_assembler_are_the_externs_of_the_module() {
    let toml = include_str!("../module.toml");
    let externs: Vec<(String, String)> = toml
        .split("[object.externs]")
        .nth(1)
        .unwrap()
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        })
        .filter(|(k, _)| !k.starts_with('#') && !k.is_empty())
        .collect();
    assert!(externs.len() > 20);
    let all = [
        include_str!("../src/hooks-common.s"),
        include_str!("../src/hooks-dt2.s"),
        include_str!("../src/hooks-dn2.s"),
    ]
    .concat();
    let code: String = all
        .lines()
        .map(|l| l.split('|').next().unwrap().split("/*").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let words: std::collections::BTreeSet<&str> = code
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| {
            w.starts_with("mf_x_")
                || w.starts_with("mf_ret_")
                || w.starts_with("mf_stock_")
                || w == &"mf_spt"
                || (w.starts_with("mf_rc") && w.len() == 6)
        })
        .collect();
    for (alias, target) in &externs {
        assert!(
            words.contains(alias.as_str()),
            "{alias} = {target:?} is never used"
        );
        assert!(
            target.starts_with("cf.") || target.starts_with("hook."),
            "{alias}: {target}"
        );
    }
    for w in &words {
        assert!(
            externs.iter().any(|(k, _)| k == w),
            "{w} has no entry in [object.externs]"
        );
    }
}
