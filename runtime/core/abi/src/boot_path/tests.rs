extern crate std;
use super::*;
use std::vec::Vec;

fn h(p: &str) -> u32 {
    let mut v: Vec<u8> = p.as_bytes().iter().copied().collect();
    v.push(0);
    unsafe { path_hash(v.as_ptr()) }
}

fn boot_of(list: &[&str]) -> [u32; SEL_MAX] {
    let mut out = [0u32; SEL_MAX];
    for (k, p) in list.iter().enumerate() {
        out[k] = h(p);
    }
    out
}

#[test]
fn a_live_load_takes_the_index_its_path_had_at_the_boot_else_a_free_one_no_boot_path_owns_else_a_new_one(
) {
    let boot = boot_of(&["modules/a.dkm", "modules/b.dkm", "modules/c.dkm"]);
    let mut live = [false; SEL_MAX];
    // the path was in the boot's selection and no module lives there
    assert_eq!(pick_index(h("modules/b.dkm"), &boot, 3, &live), Ok(1));
    // a module lives at that index (it cannot be another file, a boot index belongs to its path): the file takes a new index, not another boot path's
    live[1] = true;
    assert_eq!(
        pick_index(h("modules/b.dkm"), &boot, 3, &live),
        Ok(3),
        "the other boot indices belong to their paths"
    );
    assert_eq!(
        pick_index(h("modules/new.dkm"), &boot, 3, &[false; SEL_MAX]),
        Ok(3),
        "an unknown file does not take the index of an unloaded boot module"
    );
    // an index an earlier live load added is free for any file once its module is gone
    let mut count4 = boot;
    count4[3] = 0;
    assert_eq!(
        pick_index(h("modules/new.dkm"), &count4, 4, &[false; SEL_MAX]),
        Ok(3)
    );
    assert_eq!(
        pick_index(h("modules/new.dkm"), &count4, 4, &{
            let mut l = [false; SEL_MAX];
            l[3] = true;
            l
        }),
        Ok(4)
    );
    // the table is full
    let full = [true; SEL_MAX];
    assert_eq!(
        pick_index(h("modules/new.dkm"), &boot_of(&[]), SEL_MAX, &full),
        Err(E_SEL_COUNT)
    );
    assert_eq!(
        pick_index(
            h("modules/new.dkm"),
            &[1; SEL_MAX],
            SEL_MAX + 3,
            &[false; SEL_MAX]
        ),
        Err(E_SEL_COUNT),
        "a count past the table is cut to it; every index belongs to a boot path"
    );
    // an empty boot selection: the first index
    assert_eq!(
        pick_index(h("modules/x.dkm"), &boot_of(&[]), 0, &[false; SEL_MAX]),
        Ok(0)
    );
}

#[test]
fn a_module_keeps_its_boot_index_after_another_used_it_and_long_leaves_with_one_prefix_do_not_collide(
) {
    // A at 0 and B at 1 at the boot. A is unloaded, C loads (a new index, 2, since 0 belongs to A), C is unloaded: A loads again at 0
    let boot = boot_of(&["modules/a.dkm", "modules/b.dkm"]);
    let mut live = [false, true, false, false, false, false, false, false];
    let c = pick_index(h("modules/c.dkm"), &boot, 2, &live).unwrap();
    assert_eq!(c, 2);
    live[c] = true;
    live[c] = false;
    assert_eq!(
        pick_index(h("modules/a.dkm"), &boot, 3, &live),
        Ok(0),
        "A is back at the index it had"
    );
    // taxonomy leaves of 35 characters that differ in the last characters only (the old name match cut them at 27)
    let (x, y) = (
        "modules/machines/digikit-long-machine-name-1.dkm",
        "modules/machines/digikit-long-machine-name-2.dkm",
    );
    let boot = boot_of(&[x, y]);
    assert_eq!(pick_index(h(y), &boot, 2, &[false; SEL_MAX]), Ok(1));
    assert_eq!(pick_index(h(x), &boot, 2, &[false; SEL_MAX]), Ok(0));
    assert_ne!(h(x), h(y));
}
