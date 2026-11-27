//! Unit tests of the state handoff over fakes: the arena and the two copies of a service.
use super::*;
use std::cell::RefCell;
use std::vec::Vec;

/// An arena of numbered blocks: it counts the ones that are out, and refuses when `room` blocks are out.
struct Arena {
    room: usize,
    next: u32,
    out: Vec<(u32, Vec<u8>)>,
}

impl Arena {
    fn new(room: usize) -> Arena {
        Arena { room, next: 0x1000, out: Vec::new() }
    }
}

impl Blocks for Arena {
    unsafe fn alloc(&mut self, n: u32) -> Option<Block> {
        if self.out.len() >= self.room {
            return None;
        }
        self.next += 0x1000;
        self.out.push((self.next, vec![0u8; n as usize]));
        let b = self.out.last_mut().unwrap();
        Some(Block { ptr: b.1.as_mut_ptr(), addr: DevAddr::new(b.0) })
    }
    unsafe fn free(&mut self, addr: DevAddr) {
        let at = self.out.iter().position(|b| b.0 == addr.get()).expect("a block that is out");
        self.out.remove(at);
    }
}

/// A copy of a service: its name, its part (version and accepted range; none for no part), the bytes it exports and what its `import` got.
struct Copy {
    name: u32,
    part: Option<(u16, u16)>,
    max: usize,
    bytes: Vec<u8>,
    got: RefCell<Vec<Option<(u16, Vec<u8>)>>>,
    answer: i32,
}

impl Copy {
    fn new(name: u32, part: Option<(u16, u16)>, bytes: &[u8]) -> Copy {
        Copy { name, part, max: bytes.len(), bytes: bytes.to_vec(), got: RefCell::new(Vec::new()), answer: OK }
    }
    fn imports(&self) -> Vec<Option<(u16, Vec<u8>)>> {
        self.got.borrow().clone()
    }
}

impl Donor for &Copy {
    fn name(&self) -> u32 {
        self.name
    }
    fn versions(&self) -> Option<(u16, u16)> {
        self.part
    }
    unsafe fn export_max(&self) -> usize {
        self.max
    }
    unsafe fn export(&self, out: &mut [u8]) -> usize {
        out[..self.bytes.len()].copy_from_slice(&self.bytes);
        self.bytes.len()
    }
    unsafe fn import(&self, from: Option<(u16, &[u8])>) -> i32 {
        self.got.borrow_mut().push(from.map(|(v, b)| (v, b.to_vec())));
        self.answer
    }
}

const SVC: usize = MAX_MODULES;
const NAME: u32 = 0xabcd;

/// A reload of one service through the three calls, the old copy `old` and the new one `new`.
unsafe fn round(c: &mut CarriedState, arena: &mut Arena, old: &Copy, new: &Copy) -> (i32, Outcome) {
    assert_eq!(c.reserve(&[SVC], |_| Some(old), arena), OK);
    c.export(SVC, &old);
    c.import(SVC, &new)
}

#[test]
fn the_bytes_of_the_old_copy_reach_the_new_one() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let (old, new) = (Copy::new(NAME, Some((2, 1)), b"state"), Copy::new(NAME, Some((2, 1)), b""));
        let (status, outcome) = round(&mut c, &mut a, &old, &new);
        assert_eq!((status, outcome), (OK, Outcome::Carried { version: 2, len: 5 }));
        assert_eq!(new.imports(), vec![Some((2, b"state".to_vec()))]);
    }
}

#[test]
fn another_name_drops_the_bytes_and_the_new_copy_starts_fresh() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let (old, new) = (Copy::new(NAME, Some((1, 1)), b"x"), Copy::new(NAME + 1, Some((1, 1)), b""));
        let (status, outcome) = round(&mut c, &mut a, &old, &new);
        assert_eq!((status, outcome), (OK, Outcome::Dropped { why: Dropped::Name, version: 1, len: 1 }));
        assert_eq!(new.imports(), vec![None]);
    }
}

#[test]
fn a_layout_outside_the_range_is_import_none() {
    unsafe {
        for (old_version, newest, from) in [(1, 3, 2), (4, 3, 1)] {
            let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
            let (old, new) = (Copy::new(NAME, Some((old_version, 1)), b"x"), Copy::new(NAME, Some((newest, from)), b""));
            let (_, outcome) = round(&mut c, &mut a, &old, &new);
            assert_eq!(outcome, Outcome::Dropped { why: Dropped::Version, version: old_version, len: 1 });
            assert_eq!(new.imports(), vec![None], "old {old_version}, new {from}..={newest}");
        }
    }
}

#[test]
fn a_new_copy_without_the_part_drops_the_bytes_and_is_not_called() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let (old, new) = (Copy::new(NAME, Some((1, 1)), b"x"), Copy::new(NAME, None, b""));
        let (status, outcome) = round(&mut c, &mut a, &old, &new);
        assert_eq!((status, outcome), (OK, Outcome::Dropped { why: Dropped::NoPart, version: 1, len: 1 }));
        assert!(new.imports().is_empty());
    }
}

#[test]
fn a_new_copy_with_the_part_and_no_bytes_gets_import_none() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        // a fresh start (the boot, or an old copy without the part)
        let new = Copy::new(NAME, Some((1, 1)), b"");
        assert_eq!(c.import(SVC, &&new), (OK, Outcome::Fresh));
        assert_eq!(new.imports(), vec![None]);
        let none = Copy::new(NAME, None, b"");
        assert_eq!(c.reserve(&[SVC], |_| Some(&none), &mut a), OK);
        assert!(!c.is_held(), "no part, no block");
    }
}

#[test]
fn the_status_of_import_is_the_services() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let old = Copy::new(NAME, Some((1, 1)), b"x");
        let mut new = Copy::new(NAME, Some((1, 1)), b"");
        new.answer = dkrt_sdk::E_BUSY;
        assert_eq!(round(&mut c, &mut a, &old, &new).0, dkrt_sdk::E_BUSY);
    }
}

#[test]
fn a_request_over_the_cap_is_a_refusal_with_nothing_taken() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let small = Copy::new(NAME, Some((1, 1)), b"ok");
        let mut big = Copy::new(NAME + 1, Some((1, 1)), b"");
        big.max = CAP + 1;
        let which = [&small, &big];
        assert_eq!(c.reserve(&[SVC, SVC + 1], |r| Some(which[r - SVC]), &mut a), dkrt_sdk::E_NOMEM);
        assert!(a.out.is_empty() && !c.is_held() && c.held_bytes() == 0, "the first service's block went back");
        big.max = CAP;
        let which = [&small, &big];
        assert_eq!(c.reserve(&[SVC, SVC + 1], |r| Some(which[r - SVC]), &mut a), OK);
        assert_eq!(a.out.len(), 2);
    }
}

#[test]
fn a_full_arena_is_a_refusal_and_frees_the_blocks_it_took() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(1));
        let (one, two) = (Copy::new(NAME, Some((1, 1)), b"a"), Copy::new(NAME + 1, Some((1, 1)), b"b"));
        let which = [&one, &two];
        assert_eq!(c.reserve(&[SVC, SVC + 1], |r| Some(which[r - SVC]), &mut a), dkrt_sdk::E_NOMEM);
        assert!(a.out.is_empty() && !c.is_held());
    }
}

#[test]
fn every_end_frees_every_block() {
    unsafe {
        // an end after the reservation (a refusal before the take-down), after the export and after the import
        for stage in 0..3 {
            let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
            let (old, new) = (Copy::new(NAME, Some((1, 1)), b"abc"), Copy::new(NAME, Some((1, 1)), b""));
            assert_eq!(c.reserve(&[SVC], |_| Some(&old), &mut a), OK);
            assert_eq!(a.out.len(), 1);
            assert!(c.held_bytes() >= 3);
            if stage > 0 {
                c.export(SVC, &&old);
            }
            if stage > 1 {
                c.import(SVC, &&new);
            }
            c.release_all(&mut a);
            assert!(a.out.is_empty() && !c.is_held() && c.held_bytes() == 0, "stage {stage}");
            c.release_all(&mut a);
        }
    }
}

#[test]
fn modules_have_no_slot() {
    unsafe {
        let (mut c, mut a) = (CarriedState::NEW, Arena::new(4));
        let old = Copy::new(NAME, Some((1, 1)), b"x");
        assert_eq!(c.reserve(&[0, 3], |_| Some(&old), &mut a), OK);
        assert!(!c.is_held());
        assert_eq!(c.import(0, &&old).1, Outcome::Fresh);
    }
}
