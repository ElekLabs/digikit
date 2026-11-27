extern crate std;
use super::*;
use crate::owner::tests::{interrupt_at, mask_depth, masks_taken};
use crate::ImportCore;
use std::cell::Cell;
use std::rc::Rc;

#[test]
fn a_section_takes_one_mask_and_gives_it_back() {
    let s = Shared::new(5u32);
    let before = masks_taken();
    let r = s.with(&ImportCore, |v| {
        assert_eq!(mask_depth(), 1, "the closure runs masked");
        *v += 1;
        *v
    });
    assert_eq!(r, 6);
    assert_eq!(masks_taken(), before + 1);
    assert_eq!(mask_depth(), 0);
    assert_eq!(s.with(&ImportCore, |v| *v), 6, "the change stays");
}

#[test]
fn an_interrupt_between_two_sections_sees_the_whole_first_one() {
    // the table is a pair that must stay equal: the "interrupt" runs when the first section gives its mask back, never inside it
    let s = Rc::new(Shared::new((0u32, 0u32)));
    let seen = Rc::new(Cell::new((9, 9)));
    let (s2, seen2) = (s.clone(), seen.clone());
    interrupt_at(1, move || seen2.set(s2.with(&ImportCore, |v| *v)));
    s.with(&ImportCore, |v| {
        v.0 = 1;
        v.1 = 1;
    });
    assert_eq!(seen.get(), (1, 1));
}

#[test]
#[should_panic(expected = "nested section")]
fn a_nested_section_on_one_cell_panics_instead_of_forming_a_second_reference() {
    let s = Shared::new(0u32);
    s.with(&ImportCore, |_| {
        s.with(&ImportCore, |_| ());
    });
}
