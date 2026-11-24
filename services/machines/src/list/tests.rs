use super::*;
use std::vec::Vec;

const STOCK: [u32; 7] = [3, 0, 1, 2, 4, 5, 6];

fn run(positions: &[u32]) -> (Vec<u32>, Vec<[u32; 2]>) {
    let mut list = [0u32; MAX_TYPES];
    let mut rank = [[0u32; 2]; MAX_TYPES];
    let n = build(&STOCK, positions, 7, &mut list, &mut rank);
    (list[..n].to_vec(), rank[..n].to_vec())
}

#[test]
fn entries_sit_at_their_positions_in_row_order() {
    // row 0 (type 7) at 2, row 1 (type 8) at the end, row 2 (type 9) at 0
    let (list, _) = run(&[2, 255, 0]);
    assert_eq!(list, [9, 3, 0, 7, 1, 2, 4, 5, 6, 8]);
}

#[test]
fn a_position_above_the_length_is_the_end() {
    let (list, _) = run(&[255, 255]);
    assert_eq!(list, [3, 0, 1, 2, 4, 5, 6, 7, 8]);
}

#[test]
fn a_gap_row_is_not_listed_and_takes_no_place() {
    let (list, _) = run(&[GAP, 1, GAP]);
    assert_eq!(list, [3, 8, 0, 1, 2, 4, 5, 6]);
}

#[test]
fn no_rows_leave_the_stock_order() {
    let (list, rank) = run(&[]);
    assert_eq!(list, STOCK);
    assert_eq!(rank.len(), STOCK.len());
}

#[test]
fn rank_pairs_name_each_listed_type_with_its_display_index() {
    let (list, rank) = run(&[2, GAP, 255]);
    assert_eq!(list.len(), rank.len());
    for (i, pair) in rank.iter().enumerate() {
        assert_eq!(*pair, [list[i], i as u32]);
    }
    // the gap row's type (8) has no pair
    assert!(rank.iter().all(|p| p[0] != 8));
}

#[test]
fn from_image_reads_the_stock_order_and_the_positions_through_the_core() {
    use dkrt_test_support::FakeCore;
    let mut core = FakeCore::new();
    let (stock, rows, size, o_pos) = (0x4700_0000u32, 0x4701_0000u32, 64u32, 48u32);
    for (i, t) in STOCK.iter().enumerate() {
        core.mem.borrow_mut().insert(stock + 4 * i as u32, *t);
    }
    for (k, p) in [2u32, GAP, 255].iter().enumerate() {
        core.mem
            .borrow_mut()
            .insert(rows + k as u32 * size + o_pos, *p);
    }
    let n = unsafe { from_image(&core, stock, 7, 3, rows, size, o_pos) }.unwrap();
    assert_eq!(n, 9);
    let list = unsafe { *addr_of!(DKRT_MF_LIST) };
    assert_eq!(&list[..n], [3, 0, 7, 1, 2, 4, 5, 6, 9]);
    assert_eq!(
        unsafe { core::ptr::read_volatile(addr_of!(DKRT_MF_RANKN)) },
        9
    );
}

#[test]
fn from_image_refuses_a_registry_that_does_not_fit_or_a_missing_stock_list() {
    use dkrt_test_support::FakeCore;
    let core = FakeCore::new();
    assert!(unsafe { from_image(&core, 0, 7, 1, 0x4701_0000, 64, 48) }.is_none());
    assert!(unsafe { from_image(&core, 0x4700_0000, 40, 30, 0x4701_0000, 64, 48) }.is_none());
}

/// The list getter's copy leaves out the reserved rows that have no name, keeps the stock types and the filled rows, in order, and answers the end of what it wrote.
#[test]
fn the_list_copy_leaves_out_the_reserved_rows_that_have_no_name() {
    use dkrt_test_support::FakeCore;
    let core = FakeCore::new();
    // types 0 .. 4 stock, 5 and 6 gap rows of the registry that are not reserved, 7 .. 9 reserved: 7 and 9 filled, 8 free
    let t = Reserved {
        first: 5,
        rows: 0x4700_0000,
        row_size: 64,
        dyn0: 7,
        dyn_n: 3,
        o_name: 56,
    };
    let (empty, filled) = (0x4702_0000u32, 0x4702_0010u32);
    let mut mem = core.mem.borrow_mut();
    mem.insert(empty, 0);
    mem.insert(filled, 0x5000_0000);
    for (k, name) in [
        (0u32, empty),
        (1, empty),
        (2, filled),
        (3, empty),
        (4, filled),
    ] {
        mem.insert(t.rows + k * 64 + 56, name);
    }
    let list = [3u32, 0, 1, 2, 4, 5, 7, 8, 9, 6];
    for (i, ty) in list.iter().enumerate() {
        mem.insert(0x4710_0000 + 4 * i as u32, *ty);
    }
    drop(mem);
    // `rd8` of the fake reads the byte of the word
    let end = unsafe {
        copy_listed(
            &core,
            &t,
            0x4710_0000,
            0x4710_0000 + 4 * list.len() as u32,
            0x4720_0000,
        )
    };
    let got: Vec<u32> = (0..(end - 0x4720_0000) / 4)
        .map(|i| core.mem.borrow()[&(0x4720_0000 + 4 * i)])
        .collect();
    // 5 and 6 are rows of the registry but not reserved: kept even though their names are empty; 8 is reserved and free: left out
    assert_eq!(got, [3, 0, 1, 2, 4, 5, 7, 9, 6]);
    // nothing to copy
    assert_eq!(
        unsafe { copy_listed(&core, &t, 0x4710_0000, 0x4710_0000, 0x4720_0100) },
        0x4720_0100
    );
}
