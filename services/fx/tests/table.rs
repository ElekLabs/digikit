//! Host tests of the fx service over `FakeCore`: the table of the effect pages (the device facts and the blocks of `rows_info`), the send limit that comes from it, and what the service does
//! when the blocks are missing or do not fit (the file refuses to load). The page rows and the hooks write raw 32-bit firmware addresses, which a host cannot
//! reach: the device tests (`core/fx`, with `fx.dks` on the card) cover those.
use dkrt_abi::provider::{Describe, DspPart, Provider};
use dkrt_sdk::proto::{CAPK_FX_SLOTS, CAPK_SEND_SLOTS};
use dkrt_sdk::{E_NO_REGISTRY, OK};
use dkrt_svc_fx::fx::Fx;
use dkrt_svc_fx::fxpage::{self, DeviceFacts};
use dkrt_test_support::FakeCore;
use std::sync::Mutex;
use std::vec::Vec;

/// The service keeps its bound table in statics: one test at a time.
static LOCK: Mutex<()> = Mutex::new(());

/// The first parameter code of the effect knob block and of the send block in the fake `rows_info`.
const KNOBS_FIRST: u32 = 40;
const SEND_FIRST: u32 = 90;

/// The facts of a small device with `cells` send cells: 3 effect rows, 32 values.
fn facts(cells: u32) -> DeviceFacts {
    DeviceFacts {
        device: 1,
        page_first: 14,
        slots: 3,
        values_n: 32,
        mark_index: 70,
        send_cells: cells,
        knob_block: 2,
        send_block: 3,
        track_page: 13,
        store_off: 0x80,
        ..Default::default()
    }
}

/// An image with `rows_info` that holds the blocks of the effect knobs (owner 2) and the send knobs (owner 3) for `device`.
fn core_with_blocks(device: u32) -> FakeCore {
    let mut core = FakeCore::new();
    core.device = device;
    core.set_rows_blocks(&[
        (1, 20, 16, 0xd1),
        (2, KNOBS_FIRST, 24, 0x75),
        (3, SEND_FIRST, 5, 0x75),
    ]);
    core
}

/// The bound table's words (what the hooks read).
unsafe fn bound_words() -> Vec<u32> {
    let tab = std::ptr::addr_of!(fxpage::DKRT_FXP_TAB) as *const u32;
    (0..72).map(|i| *tab.add(i)).collect()
}

/// The bound table of each device is its generated facts, the parameter codes of the blocks in `rows_info` and the service's constants: the word offsets are the table's layout (the
/// assembler of the hooks names the same offsets: `the_offsets_the_hooks_name_are_the_word_indexes_of_the_table`).
#[test]
fn the_bound_table_equals_the_generated_facts_of_each_device() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let all = fxpage::all_device_facts();
    assert!(
        !all.is_empty(),
        "the device files give effect pages to a device"
    );
    for f in all {
        unsafe {
            fxpage::unbind();
            let core = core_with_blocks(f.device);
            assert_eq!(fxpage::bind(&core, [7, 9]), OK, "device {}", f.device);
            let t = bound_words();
            let want: [(usize, u32); 25] = [
                (0, 0x4658_5047),
                (2, f.device),
                (4, 0x2c),
                (5, f.page_first),
                (6, f.slots),
                (8, 8),
                (10, 7),
                (11, 9),
                (12, KNOBS_FIRST),
                (13, 8),
                (14, f.array_off),
                (15, f.values_n),
                (16, f.template_percent),
                (17, f.template_bipolar),
                (18, f.template_mix),
                (19, f.kit_ptr),
                (37, f.mark_index),
                (38, 0x46a7),
                (39, f.send_cells),
                (42, SEND_FIRST),
                (43, f.send_index[0]),
                (44, f.send_index[1]),
                (45, f.send_template),
                (46, f.sound_off),
                (47, f.sound_stride),
            ];
            for (word, v) in want {
                assert_eq!(t[word], v, "device {}: word {word}", f.device);
            }
            assert_eq!(
                (t[61], t[62], t[63]),
                (f.track_page, f.tail_off, f.store_off),
                "device {}",
                f.device
            );
            let tab = std::ptr::addr_of!(fxpage::DKRT_FXP_TAB) as *const u8;
            let idx: Vec<u8> = (0..32).map(|i| *tab.add(80 + i)).collect();
            assert_eq!(idx, f.value_indices, "device {}", f.device);
            // the computed words
            assert_eq!(
                (t[64], t[65], t[66]),
                (
                    f.values_n / 8,
                    f.store_off + 2 * f.values_n,
                    2 * f.mark_index
                )
            );
            fxpage::unbind();
        }
    }
}

#[test]
fn the_device_facts_name_distinct_devices() {
    let all = fxpage::all_device_facts();
    for (i, f) in all.iter().enumerate() {
        assert!(all[i + 1..].iter().all(|g| g.device != f.device));
        assert_eq!(fxpage::device_facts(f.device), Some(f));
    }
    assert_eq!(fxpage::device_facts(0xdead), None);
}

#[test]
fn without_rows_info_or_with_blocks_that_do_not_fit_the_file_refuses_to_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let device = fxpage::all_device_facts()[0].device;
    unsafe {
        fxpage::unbind();
        // no `rows_info` in the image
        let mut bare = FakeCore::new();
        bare.device = device;
        assert_eq!(fxpage::bind(&bare, [0, 0]), E_NO_REGISTRY);
        assert!(!fxpage::available(&bare));
        // `rows_info` without the block of the effect knobs, or without the send block
        for kept in [[(3, SEND_FIRST, 2, 0x75)], [(2, KNOBS_FIRST, 24, 0x75)]] {
            let mut core = FakeCore::new();
            core.device = device;
            core.set_rows_blocks(&kept);
            assert_eq!(fxpage::bind(&core, [0, 0]), E_NO_REGISTRY, "{kept:?}");
        }
        // a knob block that holds fewer rows than 8 for each effect row, a send block that holds fewer rows than the device has cells
        for blocks in [
            [(2, KNOBS_FIRST, 8, 0x75), (3, SEND_FIRST, 2, 0x75)],
            [(2, KNOBS_FIRST, 24, 0x75), (3, SEND_FIRST, 0, 0x75)],
        ] {
            let mut core = FakeCore::new();
            core.device = device;
            core.set_rows_blocks(&blocks);
            assert_eq!(fxpage::bind(&core, [0, 0]), E_NO_REGISTRY, "{blocks:?}");
        }
        // a device the generated facts do not name
        assert_eq!(
            fxpage::bind(&core_with_blocks(0xdead), [0, 0]),
            E_NO_REGISTRY
        );
        // facts that ask for more rows than the service holds state for
        let mut big = facts(2);
        big.slots = 99;
        assert_eq!(
            fxpage::bind_facts(&core_with_blocks(1), &big, [0, 0]),
            E_NO_REGISTRY
        );
    }
}

/// The owner's `start` is the file's `init` step: it binds the table and refuses the file when the image has no `rows_info` with the blocks.
#[test]
fn the_owner_starts_over_the_blocks_of_the_image_and_refuses_the_file_without_them() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let device = fxpage::all_device_facts()[0].device;
    unsafe { fxpage::unbind() };
    let mut bare = FakeCore::new();
    bare.device = device;
    assert_eq!(
        <Fx as Provider<FakeCore>>::start(&bare).map(|_| ()),
        Err(E_NO_REGISTRY),
        "no rows_info in the image"
    );
    unsafe { fxpage::unbind() };
    let core = core_with_blocks(device);
    assert!(<Fx as Provider<FakeCore>>::start(&core).is_ok());
    assert!(
        unsafe { fxpage::available(&FakeCore::new()) },
        "the table stays bound for the hooks"
    );
    unsafe { fxpage::unbind() };
}

#[test]
fn the_effect_slots_are_the_rows_of_the_chain_in_use_and_the_image_has() {
    let _g = dkrt_test_support::contract::locked();
    dkrt_svc_fx::rows::reset();
    let mut core = core_with_blocks(1);
    core.set_dynfx(6, 4);
    let fx = Fx::new();
    // an image without core/dynfx has no effect rows; with it, the free rows of its table
    assert_eq!(
        Describe::cap(&fx, &FakeCore::new(), CAPK_FX_SLOTS),
        Some((0, 0))
    );
    assert_eq!(Describe::cap(&fx, &core, CAPK_FX_SLOTS), Some((4, 0)));
    // a row a module claimed is in use; once its module gives it back it is not (a freed row is not counted)
    dkrt_svc_fx::rows::frows().fx_owner[1] = 3;
    assert_eq!(Describe::cap(&fx, &core, CAPK_FX_SLOTS), Some((4, 1)));
    let mut fx = fx;
    DspPart::release(&mut fx, &core, 2);
    assert_eq!(Describe::cap(&fx, &core, CAPK_FX_SLOTS), Some((4, 0)));
}

#[test]
fn a_current_table_binds_and_gives_the_pages_and_the_send_limit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        fxpage::unbind();
        let core = core_with_blocks(1);
        assert_eq!(fxpage::bind_facts(&core, &facts(2), [0, 0]), OK);
        assert!(fxpage::available(&core));
        // pages reserved by the image, none in use
        assert_eq!(fxpage::counts(&core), (3, 0));
        // the CAPACITY kind of the send cells comes from the table; another kind is not this service's
        let fx = Fx::new();
        assert_eq!(Describe::cap(&fx, &core, CAPK_SEND_SLOTS), Some((2, 0)));
        assert_eq!(Describe::cap(&fx, &core, 0xee), None);
    }
}

#[test]
fn the_send_limit_is_the_cells_of_the_table_up_to_what_the_service_holds() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for (cells, limit) in [(0, 0), (1, 1), (2, 2), (5, 2)] {
            fxpage::unbind();
            let core = core_with_blocks(1);
            assert_eq!(fxpage::bind_facts(&core, &facts(cells), [0, 0]), OK);
            assert_eq!(
                dkrt_svc_fx::fxsend::slots(&core),
                limit,
                "{cells} cells in the table"
            );
        }
        // an image without a send page: no limit and no effect on the send route
        fxpage::unbind();
        assert_eq!(dkrt_svc_fx::fxsend::slots(&FakeCore::new()), 0);
    }
}

/// The byte offsets the assembler of the hooks names (`.equ T_NAME, offset` in `dks/src/hooks.rs`) are the word indexes of the table (`fxpage::ASM_OFFSETS`).
#[test]
fn the_offsets_the_hooks_name_are_the_word_indexes_of_the_table() {
    let text = include_str!("../dks/src/hooks.rs");
    let mut seen = 0;
    for l in text.lines() {
        let Some(rest) = l.trim().strip_prefix(".equ T_") else {
            continue;
        };
        let (name, value) = rest.split_once(',').unwrap();
        let value: usize = value.split('|').next().unwrap().trim().parse().unwrap();
        let name = format!("T_{}", name.trim());
        let index = fxpage::ASM_OFFSETS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not in ASM_OFFSETS"))
            .1;
        assert_eq!(value, 4 * index, "{name}");
        seen += 1;
    }
    assert_eq!(
        seen,
        fxpage::ASM_OFFSETS.len(),
        "every offset of ASM_OFFSETS is named in the assembler"
    );
}

/// The two firmware words of the hooks' file land in the words 10 and 11 of the bound table (the stock list and the builder's `moveq N,D1`).
#[test]
fn the_firmware_words_of_the_file_are_put_into_the_table() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        fxpage::unbind();
        let core = core_with_blocks(1);
        assert_eq!(
            fxpage::bind_facts(&core, &facts(2), [0x4010_0000, 0x4011_2230]),
            OK
        );
        let tab = std::ptr::addr_of!(fxpage::DKRT_FXP_TAB) as *const u32;
        assert_eq!((*tab.add(10), *tab.add(11)), (0x4010_0000, 0x4011_2230));
        fxpage::unbind();
    }
}

#[test]
fn a_second_start_keeps_the_live_table() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        fxpage::unbind();
        assert_eq!(
            fxpage::bind_facts(&core_with_blocks(1), &facts(2), [1, 2]),
            OK
        );
        let tab = std::ptr::addr_of!(fxpage::DKRT_FXP_TAB) as *const u32;
        let before: Vec<u32> = (0..64).map(|i| *tab.add(i)).collect();
        // a reload of the file binds again: the live table stays, it is not made again over it
        assert_eq!(fxpage::bind(&FakeCore::new(), [1, 2]), OK);
        assert!(fxpage::available(&FakeCore::new()));
        // other firmware words and another image do not touch the bound table
        assert_eq!(fxpage::bind(&core_with_blocks(1), [3, 4]), OK);
        let after: Vec<u32> = (0..64).map(|i| *tab.add(i)).collect();
        assert_eq!(before, after);
        assert_eq!((after[10], after[11]), (1, 2));
        fxpage::unbind();
    }
}
