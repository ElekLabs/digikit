//! The checks of a `register` section that read no firmware address: the counts, the actions, the device, the records up to the first machine row, and the owners.
//! A machine record that passes its own checks reads its row in the firmware's table, which a host cannot reach (the device tests cover that), so the machine cases
//! here are the ones that stop before.
use super::*;
use crate::records::validate;
use dkrt_sdk::records::*;
use dkrt_svc_rows::dynreg::MACH_TEXT;
use dkrt_test_support::FakeCore;
use std::vec::Vec;

type Rec = (u32, Vec<u8>);

/// A section of `recs`: the 16-byte header with the count at 12, then each record as kind, total length and body.
fn section(recs: &[Rec]) -> Vec<u8> {
    let mut s = std::vec![0u8; 16];
    s[12..16].copy_from_slice(&(recs.len() as u32).to_be_bytes());
    for (kind, body) in recs {
        s.extend_from_slice(&kind.to_be_bytes());
        s.extend_from_slice(&(8 + body.len() as u32).to_be_bytes());
        s.extend_from_slice(body);
    }
    s
}

/// `n` records of `kind` that meet the rule of `validate::Acc` (a native page has an empty body: the rule does not read it). The `i`-th has index `i`, its own key, parameter `i % 32`.
fn kinds(kind: u32, n: usize) -> Vec<Rec> {
    (0..n)
        .map(|i| {
            if kind == KIND_PARAM {
                good_param(i as u32)
            } else if kind == KIND_MACHINE {
                machine(0x100 * (i as u32 + 1), 0, 255, 0, 0)
            } else if kind == KIND_PAGE {
                page(0x10, [0; 8], 0, b"N", b"T")
            } else if kind == KIND_ACTION {
                action(i as u32 % 32, 0)
            } else {
                (kind, std::vec![])
            }
        })
        .collect()
}

/// Whether `Machines::validate` reports a problem for `recs` of `kind` (as `patchkit records --check` does) and whether the load check `ck_counts` refuses them: the same rule.
fn refused_by_both(recs: &[Rec]) -> (bool, bool) {
    let mut problems = 0;
    for kind in [KIND_PARAM, KIND_MACHINE, KIND_PAGE, KIND_ACTION] {
        let bodies: Vec<&[u8]> = recs
            .iter()
            .filter(|r| r.0 == kind)
            .map(|r| &r.1[..])
            .collect();
        validate(kind, &bodies, &mut |_| problems += 1);
    }
    (problems > 0, check(ck_counts, recs, true) != OK)
}

/// The words of a `dynreg_info` table: the device has page rows when `pages`; the first parameter code, knob count and slot base of these tests.
fn info(pages: bool) -> Vec<u32> {
    let mut words = std::vec![0u32; 128];
    words[w::PG_ROWS] = pages as u32;
    words[w::P_FIRST] = 100;
    words[w::KNOBS] = KNOBS as u32;
    words[w::SLOT_BASE] = 10;
    words[w::M_FIRST] = 7;
    words[w::M_DYN0] = 16;
    words[w::M_DYN_N] = 8;
    words
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn text(b: &mut [u8], at: usize, field: usize, s: &[u8]) {
    b[at..at + field].fill(0);
    b[at..at + s.len()].copy_from_slice(s);
}

fn check(f: unsafe fn(&Ctx) -> i32, recs: &[Rec], pages: bool) -> i32 {
    let (s, inf) = (section(recs), info(pages));
    unsafe {
        f(&Ctx {
            reg: s.as_ptr(),
            image_size: 100,
            inf: inf.as_ptr(),
        })
    }
}

fn check_core(f: unsafe fn(&FakeCore, &Ctx) -> i32, recs: &[Rec], pages: bool) -> i32 {
    check_core_with(f, recs, info(pages))
}

fn check_core_with(f: unsafe fn(&FakeCore, &Ctx) -> i32, recs: &[Rec], inf: Vec<u32>) -> i32 {
    let mut core = FakeCore::new();
    // the run-time machine types are the device profile's: any profile that has them
    core.device = dkrt_abi::profiles::ALL
        .iter()
        .find(|p| p.dyn_types > 0)
        .expect("a profile with run-time machine types")
        .id;
    let s = section(recs);
    unsafe {
        f(
            &core,
            &Ctx {
                reg: s.as_ptr(),
                image_size: 100,
                inf: inf.as_ptr(),
            },
        )
    }
}

#[test]
fn a_section_may_hold_the_most_rows_of_each_kind_the_registry_has_and_native_pages_for_its_machines_only(
) {
    for (kind, most) in [
        (KIND_PARAM, MAX_PARAM),
        (KIND_MACHINE, MAX_MACH),
        (KIND_PAGE, MAX_PAGE),
    ] {
        assert_eq!(
            check(ck_counts, &kinds(kind, most), true),
            OK,
            "{most} of kind {kind}"
        );
        assert_eq!(
            check(ck_counts, &kinds(kind, most + 1), true),
            E_FORMAT,
            "{} of kind {kind}",
            most + 1
        );
    }
    // a machine has at most two native pages (the two kinds), so three for one machine are refused
    let with_native = |n| [kinds(KIND_MACHINE, 1), kinds(KIND_NATIVE_PAGE, n)].concat();
    assert_eq!(check(ck_counts, &with_native(2), true), OK);
    assert_eq!(check(ck_counts, &with_native(3), true), E_FORMAT);
    assert_eq!(check(ck_counts, &[], true), OK);
}

fn action(parameter: u32, offset: u32) -> Rec {
    let mut b = std::vec![0u8; LEN_ACTION as usize - 8];
    put(&mut b, 0, parameter);
    put(&mut b, 4, offset);
    (KIND_ACTION, b)
}

#[test]
fn an_action_names_a_parameter_of_the_file_once_and_code_inside_the_image() {
    let params = |n| kinds(KIND_PARAM, n);
    let with = |n, acts: &[Rec]| [params(n), acts.to_vec()].concat();
    assert_eq!(check(ck_actions, &with(1, &[action(0, 99)]), true), OK);
    // an offset at the image size is outside it (the image of these checks is 100 bytes)
    assert_eq!(
        check(ck_actions, &with(1, &[action(0, 100)]), true),
        E_FORMAT
    );
    // a parameter the file does not have (one parameter with two actions is the rule of `validate`, see `the_validator_and_the_load_check_apply_one_rule`)
    assert_eq!(check(ck_actions, &with(1, &[action(1, 0)]), true), E_FORMAT);
    assert_eq!(
        check(ck_actions, &with(2, &[action(0, 0), action(1, 4)]), true),
        OK
    );
}

#[test]
fn pages_need_a_device_that_has_page_rows() {
    let page = kinds(KIND_PAGE, 1);
    assert_eq!(check(ck_device, &page, false), E_FORMAT);
    assert_eq!(check(ck_device, &page, true), OK);
    assert_eq!(check(ck_device, &[], false), OK);
}

fn param(
    index: u32,
    template: u32,
    slot: u32,
    min: i32,
    max: i32,
    name: &[u8],
    short: &[u8],
) -> Rec {
    let mut b = std::vec![0u8; LEN_PARAM as usize - 8];
    put(&mut b, 0, index);
    put(&mut b, 4, template);
    put(&mut b, 8, slot);
    put(&mut b, 12, min as u32);
    put(&mut b, 16, max as u32);
    text(&mut b, 24, 32, name);
    text(&mut b, 56, 16, short);
    (KIND_PARAM, b)
}

fn good_param(index: u32) -> Rec {
    param(index, 5, 10, 0, 127, b"Level", b"LVL")
}

#[test]
fn a_parameter_has_a_template_before_the_run_time_rows_and_a_slot_of_the_page() {
    let one = |r: Rec| check_core(ck_records, &[r], false);
    assert_eq!(
        check_core(ck_records, &[good_param(0), good_param(1)], false),
        OK
    );
    // the template is a row before the first of the run-time parameters; the slot is one of the page's knobs
    assert_eq!(one(param(0, 100, 10, 0, 127, b"A", b"A")), E_FORMAT);
    assert_eq!(one(param(0, 99, 10, 0, 127, b"A", b"A")), OK);
    assert_eq!(one(param(0, 5, 9, 0, 127, b"A", b"A")), E_FORMAT);
    assert_eq!(
        one(param(0, 5, 10 + KNOBS as u32, 0, 127, b"A", b"A")),
        E_FORMAT
    );
    assert_eq!(one(param(0, 5, 9 + KNOBS as u32, 0, 127, b"A", b"A")), OK);
}

fn page(machine_key: u32, knobs: [u32; 8], level: u32, name: &[u8], title: &[u8]) -> Rec {
    let mut b = std::vec![0u8; LEN_PAGE as usize - 8];
    put(&mut b, 0, machine_key);
    for (j, k) in knobs.iter().enumerate() {
        put(&mut b, 4 + 4 * j, *k);
    }
    put(&mut b, 36, level);
    text(&mut b, 40, MACH_TEXT, name);
    text(&mut b, 40 + MACH_TEXT, MACH_TEXT, title);
    (KIND_PAGE, b)
}

#[test]
fn a_page_knob_names_a_parameter_of_the_file_and_the_table_has_the_knob_count() {
    let no_knobs = [0u32; 8];
    let with_knob = |k: u32| {
        let mut knobs = no_knobs;
        knobs[2] = k;
        check_core(
            ck_records,
            &[good_param(0), page(0x10, knobs, 0, b"Name", b"Title")],
            true,
        )
    };
    assert_eq!(with_knob(0), OK, "no knob at all in the slot is fine");
    assert_eq!(with_knob(KNOB_PARAM), OK, "parameter 0 of the file");
    assert_eq!(
        with_knob(KNOB_PARAM | 1),
        E_FORMAT,
        "the file has one parameter"
    );
    assert_eq!(
        with_knob(5),
        E_FORMAT,
        "a knob that is not a parameter of the file"
    );
    // a table that has another knob count than the build refuses every page
    let mut inf = info(true);
    inf[w::KNOBS] = KNOBS as u32 - 1;
    assert_eq!(
        check_core_with(ck_records, &[page(0x10, no_knobs, 0, b"N", b"T")], inf),
        E_FORMAT
    );
}

/// A machine record of the type key `key` that clones `clone` with the constant word and the own-page flag as given: the raw words that `ck_records` checks first.
fn machine(key: u32, clone: u32, constant: u32, own: u32, last: u32) -> Rec {
    let mut b = std::vec![0u8; LEN_MACHINE as usize - 8];
    put(&mut b, 0, key);
    put(&mut b, 4, clone);
    put(&mut b, 8, constant);
    put(&mut b, 12, own);
    put(&mut b, 16, last);
    text(&mut b, 52, MACH_TEXT, b"Machine");
    text(&mut b, 52 + MACH_TEXT, MACH_TEXT, b"MCH");
    (KIND_MACHINE, b)
}

#[test]
fn a_machine_is_refused_for_an_own_page_without_page_rows_and_for_a_type_outside_the_run_time_range(
) {
    let one = |r: Rec, pages: bool| check_core(ck_records, &[r], pages);
    // the run-time types are 16 to 23 and the stock machines are the types below 7 (the table of these tests)
    assert_eq!(
        one(machine(0x10, 0, 255, 1, 0), false),
        E_FORMAT,
        "an own page needs a device with page rows"
    );
    // the key resolves to a type below the run-time range, past it, and the clone source is not a stock machine
    assert_eq!(one(machine(0x07, 0, 255, 0, 0), true), E_TYPE_RANGE);
    assert_eq!(
        one(machine(0x00, 0, 255, 0, 0), true),
        E_TYPE_RANGE,
        "an unresolved key"
    );
    assert_eq!(one(machine(0x18, 0, 255, 0, 0), true), E_TYPE_RANGE);
    assert_eq!(one(machine(0x10, 7, 255, 0, 0), true), E_TYPE_RANGE);
}

#[test]
fn the_validator_and_the_load_check_apply_one_rule() {
    let none: [Rec; 0] = [];
    assert_eq!(refused_by_both(&none), (false, false));
    let ok = [
        kinds(KIND_PARAM, 2),
        kinds(KIND_MACHINE, 2),
        kinds(KIND_PAGE, 1),
        kinds(KIND_ACTION, 2),
    ]
    .concat();
    assert_eq!(refused_by_both(&ok), (false, false));
    let bad: [(&str, Vec<Rec>); 14] = [
        (
            "parameter index is not its position",
            std::vec![good_param(1)],
        ),
        (
            "two parameters with index 0",
            std::vec![good_param(0), good_param(0)],
        ),
        (
            "minimum over maximum",
            std::vec![param(0, 5, 10, 1, 0, b"A", b"A")],
        ),
        (
            "empty long name",
            std::vec![param(0, 5, 10, 0, 1, b"", b"A")],
        ),
        (
            "long name of 32",
            std::vec![param(0, 5, 10, 0, 1, &[b'n'; 32], b"A")],
        ),
        (
            "unprintable short name",
            std::vec![param(0, 5, 10, 0, 1, b"A", b"\x07")],
        ),
        (
            "page level over 0xffff",
            std::vec![page(0x10, [0; 8], 0x1_0000, b"N", b"T")],
        ),
        (
            "page without a title",
            std::vec![page(0x10, [0; 8], 0, b"N", b"")],
        ),
        (
            "machine constant word",
            std::vec![machine(0x10, 0, 254, 0, 0)],
        ),
        ("machine own flag", std::vec![machine(0x10, 0, 255, 2, 0)]),
        (
            "machine level",
            std::vec![machine(0x10, 0, 255, 0, 0x1_0000)],
        ),
        (
            "same machine key twice",
            std::vec![machine(0x10, 0, 255, 0, 0), machine(0x10, 0, 255, 0, 0)],
        ),
        ("action parameter 32", std::vec![action(32, 0)]),
        (
            "one parameter with two actions",
            std::vec![action(0, 0), action(0, 4)],
        ),
    ];
    for (what, recs) in bad {
        assert_eq!(refused_by_both(&recs), (true, true), "{what}");
    }
    // more rows than the registry has: the first record past the limit is the problem
    assert_eq!(
        refused_by_both(&kinds(KIND_PAGE, MAX_PAGE + 1)),
        (true, true)
    );
    assert_eq!(refused_by_both(&kinds(KIND_PAGE, MAX_PAGE)), (false, false));
    // a body of the wrong size is the validator's problem (the load path never sees one: the walker checks the length)
    let mut short = good_param(0);
    short.1.pop();
    let bodies = [&short.1[..]];
    let mut n = 0;
    validate(KIND_PARAM, &bodies, &mut |_| n += 1);
    assert_eq!(n, 1);
}

#[test]
fn a_page_belongs_to_a_machine_of_the_same_file() {
    let machines = [machine(0x10, 0, 255, 0, 0)];
    let pg = |key: u32| page(key, [0; 8], 0, b"N", b"T");
    assert_eq!(
        check_core(ck_owners, &[machines[0].clone(), pg(0x10)], true),
        OK
    );
    assert_eq!(
        check_core(ck_owners, &[machines[0].clone(), pg(0x11)], true),
        E_FORMAT
    );
    assert_eq!(check_core(ck_owners, &[pg(0x10)], true), E_FORMAT);
    // the machine may come after its page; a file with no page has nothing to own
    assert_eq!(
        check_core(ck_owners, &[pg(0x10), machines[0].clone()], true),
        OK
    );
    assert_eq!(check_core(ck_owners, &machines, true), OK);
}
