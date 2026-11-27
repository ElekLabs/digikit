use super::*;

#[test]
fn a_new_cell_is_phase_zero_and_all_zero_bytes() {
    let h = Handoff::new();
    assert_eq!(h.phase(), 0);
    // SAFETY: the cell is one word; its bytes are read back as they are
    let bytes: [u8; 4] = unsafe { core::mem::transmute_copy(&h) };
    assert_eq!(bytes, [0; 4], "the cell is .bss");
}

#[test]
fn a_published_phase_is_the_next_read() {
    let h = Handoff::new();
    for p in [1, 2, 3, 0] {
        h.publish(p);
        assert_eq!(h.phase(), p);
    }
}

// The two fences are not observable on a host: they bind only the compiler's reordering of the data around the word, which no test can see. The check is the code of
// the ColdFire object (`patchkit objdiff`: the data stores precede the phase store).
