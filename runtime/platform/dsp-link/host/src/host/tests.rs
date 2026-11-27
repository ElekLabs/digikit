use super::*;

#[test]
fn an_image_without_the_table_leaves_the_host_idle() {
    let mut h = Host::new();
    assert_eq!(unsafe { h.start(&|_| core::ptr::null_mut()) }, 0);
    assert!(!h.started());
    assert_eq!(h.step(), 0);
    assert_eq!(h.stop(), 0);
}

#[test]
fn start_asks_for_the_voice_host_entry_and_prepares_it_without_the_magic() {
    let mut words = [0xffff_ffff_ffff_ffffu64; 16];
    let asked = std::cell::Cell::new(0);
    let p = words.as_mut_ptr().cast::<u32>();
    let mut h = Host::new();
    assert_eq!(
        unsafe {
            h.start(&|hash| {
                asked.set(hash);
                p
            })
        },
        0
    );
    assert_eq!(asked.get(), NAME_HASH);
    assert!(h.started());
    let w = words.as_ptr().cast::<u32>();
    assert_eq!(
        unsafe { *w },
        0,
        "the magic is clear: no consumer sees a host yet"
    );
    assert_eq!(
        unsafe { *w.add(2) } as usize,
        core::mem::size_of::<crate::voice_host_table::Words>()
    );
    assert_eq!(h.stop(), 0);
    assert!(!h.started());
}
