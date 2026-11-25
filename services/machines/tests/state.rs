//! The owner `Machines` itself: what `start` and `start_in` do (the image table is bound into `DKRT_MF_TAB`, or the file is refused; the state is made where it lives, over
//! whatever the slot held), and what a new owner holds (nothing collected, the gate closed, no action armed, nothing parked).
use core::mem::MaybeUninit;
use dkneeds::keys::name_hash;
use dkrt_abi::provider::Provider;
use dkrt_sdk::{E_NO_REGISTRY, E_FORMAT};
use dkrt_svc_machines::info::{self, w, DKRT_MF_TAB};
use dkrt_svc_machines::machines::Machines;
use dkrt_svc_rows::rows_info::w as r;
use dkrt_test_support::FakeCore;
use std::sync::Mutex;

/// `DKRT_MF_TAB` is one table of the crate: the tests that bind it take turns.
static TURN: Mutex<()> = Mutex::new(());

/// `rows_info` of an image with a registry of rows from type 7.
fn table(device: u32) -> Vec<u32> {
    let mut t = vec![0u32; r::WORDS];
    t[r::DEVICE] = device;
    t[r::M_ROWS] = 0x4200_0000;
    t[r::LIST_STOCK] = 0x4700_0000;
    t[r::M_ROW_SIZE] = 64;
    t[r::O_POS] = 48;
    t[r::O_NAME] = 56;
    t[r::M_FIRST] = 7;
    t[r::READER_REAL] = 0x4000_2000;
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

/// What a fresh owner shows from outside.
fn assert_initial(m: &mut Machines) {
    let ph = m.builder().placeholder();
    assert!(!ph.is_null());
    assert_eq!(m.builder().build().nm, 0);
    assert!(m.builder().args().is_none());
    assert_eq!(m.clicks().pending().callback, 0);
    let p = m.parking();
    assert_eq!(
        (p.missing_rows(), p.tries(), p.logged(), p.busy_passes()),
        (0, 0, 0, 0)
    );
    assert!(m.applied().codes().is_empty());
    assert_eq!(m.applied().native_seen(), 0);
}

#[test]
fn a_new_owner_has_nothing_collected_queued_or_parked() {
    let mut m = Box::new(Machines::new());
    assert_initial(&mut m);
}

/// Start in place, in a box that keeps its address (the rows point into the state): the way `init` and the test driver start it.
fn started(core: &FakeCore) -> Result<Box<Machines>, dkrt_abi::provider::Status> {
    let mut slot = Box::new(MaybeUninit::<Machines>::uninit());
    <Machines as Provider<FakeCore>>::start_in(core, &mut slot)?;
    // SAFETY: `start_in` wrote the whole value; `MaybeUninit<T>` and `T` have one layout
    Ok(unsafe { Box::from_raw(Box::into_raw(slot) as *mut Machines) })
}

/// The by-move `start` is no start path: the state points into itself, so it refuses.
#[test]
fn start_by_move_is_refused() {
    let core = core_with(Some(table(2)), 2);
    assert!(matches!(
        <Machines as Provider<FakeCore>>::start(&core),
        Err(E_FORMAT)
    ));
}

#[test]
fn start_refuses_an_image_without_a_usable_table_and_binds_a_good_one() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    // no table, another device's table, no machine rows
    let mut no_rows = table(2);
    no_rows[r::M_ROWS] = 0;
    for core in [
        core_with(None, 2),
        core_with(Some(table(1)), 2),
        core_with(Some(no_rows), 2),
    ] {
        assert!(matches!(started(&core), Err(E_NO_REGISTRY)));
        let mut slot = MaybeUninit::<Machines>::uninit();
        assert_eq!(
            <Machines as Provider<FakeCore>>::start_in(&core, &mut slot),
            Err(E_NO_REGISTRY)
        );
    }
    let core = core_with(Some(table(2)), 2);
    let mut owner = started(&core).expect("starts");
    assert_initial(&mut owner);
    // SAFETY: the test holds the turn; nothing else reads the table
    let bound = unsafe { *core::ptr::addr_of!(DKRT_MF_TAB) };
    assert_eq!(
        (
            bound[w::MAGIC],
            bound[w::DEVICE],
            bound[w::FIRST],
            bound[w::ROWS],
            bound[w::READER_REAL]
        ),
        (info::MAGIC, 2, 7, 0x4200_0000, 0x4000_2000),
        "the hooks' table has the registry words of the image"
    );
}

#[test]
fn start_in_makes_the_state_where_it_lives_over_whatever_the_slot_held() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let core = core_with(Some(table(2)), 2);
    let mut slot = Box::new(MaybeUninit::<Machines>::uninit());
    // a slot that held a former owner's bytes
    // SAFETY: the slot is plain memory of the size of `Machines`
    unsafe { slot.as_mut_ptr().write_bytes(0xa5, 1) };
    assert_eq!(
        <Machines as Provider<FakeCore>>::start_in(&core, &mut slot),
        Ok(())
    );
    // SAFETY: `start_in` made the state in place
    let m = unsafe { slot.assume_init_mut() };
    assert_initial(m);
}

/// The four list patches get 4 bytes per listed type (a gap row is not listed) and the addresses of the service's list buffer, and `start` leaves the buffer holding the order.
#[test]
fn start_builds_the_list_and_gives_the_patches_their_bytes() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let mut t = table(1);
    t[r::M_COUNT] = 3;
    t[r::M_ENTRIES] = 2;
    t[r::LIST_STOCK] = 0x4700_0000;
    t[r::M_ROW_SIZE] = 64;
    t[r::O_POS] = 48;
    let core = core_with(Some(t), 1);
    // seven stock types in order 0 .. 6; the rows: position 2, a gap row, the end
    for i in 0..7u32 {
        core.mem.borrow_mut().insert(0x4700_0000 + 4 * i, i);
    }
    for (k, p) in [2u32, 0xffff_ffff, 255].iter().enumerate() {
        core.mem
            .borrow_mut()
            .insert(0x4200_0000 + 64 * k as u32 + 48, *p);
    }
    let m = started(&core).unwrap();
    assert_eq!(
        m.list_bytes(),
        9 * 4,
        "seven stock types and two listed rows"
    );
    assert_eq!(m.list_end(), m.list_start() + 9 * 4);
    let list = unsafe { *core::ptr::addr_of!(dkrt_svc_machines::list::DKRT_MF_LIST) };
    assert_eq!(&list[..9], [0, 1, 7, 2, 3, 4, 5, 6, 9]);
}

/// The page half of `mf_ensure` is the service's: `start` makes the reserved page rows free rows with a string name, in the place and by `start_in` alike.
#[test]
fn start_makes_the_reserved_page_rows_free_rows() {
    use dkrt_abi::mem::FwMem;
    use dkrt_svc_rows::rows_info::w as dw;
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    const ROWS: u32 = 0x60_0000;
    let mut rows_words = table(2);
    rows_words[dw::PG_ROWS] = ROWS;
    rows_words[dw::PG_ROW_SIZE] = 0x54;
    rows_words[dw::PG_DYN_N] = 2;
    rows_words[dw::PGO_NAME] = 0;
    rows_words[dw::PGO_TITLE] = 4;
    rows_words[dw::PGO_C] = 8;
    rows_words[dw::PGO_S] = 0x30;
    rows_words[dw::PGO_MACH] = 0x2c;
    rows_words[dw::PG_FREE] = 0x7fff_ffff;
    {
        let core = core_with(Some(rows_words.clone()), 2);
        core.mem.borrow_mut().insert(ROWS, 0x4000_0000);
        core.mem.borrow_mut().insert(ROWS + 0x54 + 0x2c, 5);
        // the rows point at the empty string of the owner, so the owner must stay at its address: the in-place start
        let mut slot = Box::new(MaybeUninit::<Machines>::uninit());
        <Machines as Provider<FakeCore>>::start_in(&core, &mut slot).unwrap();
        unsafe {
            let owner = slot.assume_init_ref();
            let rep = core::ptr::addr_of!(owner.reg().empty_rep) as u32 + 12;
            assert_eq!(core.rd(ROWS), rep);
            assert_eq!(core.rd(ROWS + 0x54 + 0x2c), 0x7fff_ffff);
        }
    }
}

/// The descriptor strings of a machine row were buffers of this file: `stop` takes them out of the reserved rows (the dispatch of core/rows gives a row with none the empty one), and
/// leaves the rows before them and their other words alone.
#[test]
fn stop_takes_the_descriptor_strings_out_of_the_reserved_rows() {
    use dkrt_abi::mem::FwMem;
    let (rows, size) = (0x4200_0000u32, 64u32);
    let mut t = table(2);
    t[r::M_ROWS] = rows;
    t[r::M_ROW_SIZE] = size;
    t[r::M_FIRST] = 5;
    t[r::M_DYN0] = 7;
    t[r::M_DYN_N] = 2;
    t[r::O_SHORT] = 4;
    t[r::O_NAME] = 56;
    let core = core_with(Some(t), 2);
    // rows for types 5 .. 8: the descriptor strings (words 0 and 1) point at a buffer of the file, the name pointer (word 14) at a name
    for k in 0..4u32 {
        let mut m = core.mem.borrow_mut();
        m.insert(rows + k * size, 0x5000_0000 + k);
        m.insert(rows + k * size + 4, 0x5000_0100 + k);
        m.insert(rows + k * size + 56, 0x5000_0200 + k);
    }
    Provider::<FakeCore>::stop(&mut Machines::new(), &core);
    let word = |k: u32, off: u32| unsafe { core.rd(rows + k * size + off) };
    assert_eq!(
        (word(0, 0), word(1, 0)),
        (0x5000_0000, 0x5000_0001),
        "the rows before the reserved ones keep theirs"
    );
    assert_eq!(
        (word(2, 0), word(2, 4), word(3, 0), word(3, 4)),
        (0, 0, 0, 0)
    );
    assert_eq!(
        (word(2, 56), word(3, 56)),
        (0x5000_0202, 0x5000_0203),
        "nothing else of a row changes"
    );
}

/// The row layout words of the bound table (`DKRT_MF_TAB`) come from `rows_info` of the image, and `Reserved::bound` carries them to the list copy.
#[test]
fn the_row_layout_words_come_from_rows_info() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let mut t = table(2);
    t[r::O_NAME] = 40;
    t[r::O_POS] = 44;
    t[r::M_ROW_SIZE] = 72;
    let core = core_with(Some(t), 2);
    let _owner = started(&core).expect("starts");
    // SAFETY: the test holds the turn; nothing else reads the table
    let res = unsafe { dkrt_svc_machines::list::Reserved::bound() };
    assert_eq!((res.o_name, res.row_size), (40, 72));
    let bound = unsafe { *core::ptr::addr_of!(DKRT_MF_TAB) };
    assert_eq!((bound[w::O_NAME], bound[w::O_POS]), (40, 44));
}
