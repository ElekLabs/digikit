use super::*;
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

fn put<T>(v: T) -> impl FnOnce(&mut MaybeUninit<T>) -> Result<(), Status> {
    move |slot| {
        slot.write(v);
        Ok(())
    }
}

#[test]
fn with_gives_nothing_before_start_and_after_stop() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(7)).is_ok());
    assert_eq!(o.with(|v| *v), Some(7));
    assert_eq!(
        o.with(|v| {
            *v += 1;
            *v
        }),
        Some(8)
    );
    let mut seen = 0;
    assert!(o.stop(|v| seen = *v));
    assert_eq!(seen, 8);
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(!o.stop(|_| unreachable!()));
    // and it can start again after a stop
    assert!(o.start(put(1)).is_ok());
}

#[test]
fn a_second_start_is_refused_without_making_an_instance() {
    let o: Owner<u32> = Owner::new();
    let at = o.start(put(1)).unwrap();
    assert_eq!(o.instance(), at);
    let mut made = false;
    let r = o.start(|_| {
        made = true;
        Ok(())
    });
    assert_eq!(r, Err(dkrt_sdk::E_LOADED));
    assert!(!made);
    assert_eq!(o.with(|v| *v), Some(1));
}

#[test]
fn a_start_that_fails_leaves_the_cell_empty() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.start(|_| Err(dkrt_sdk::E_NOMEM)), Err(dkrt_sdk::E_NOMEM));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(3)).is_ok());
}

#[test]
fn a_nested_with_finds_nothing_instead_of_a_second_reference() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let inner = o.with(|_| o.with(|v| *v));
    assert_eq!(inner, Some(None));
    assert_eq!(o.with(|v| *v), Some(5), "the cell is usable again after");
}

#[test]
fn stop_inside_with_is_refused_and_the_state_stays() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let r = o.with(|v| {
        let stopped = o.stop(|_| unreachable!());
        *v += 1;
        (stopped, *v)
    });
    assert_eq!(r, Some((false, 6)));
    assert_eq!(o.with(|v| *v), Some(6), "the instance is still live");
}

#[test]
fn start_inside_with_is_refused_and_the_state_stays() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let r = o.with(|v| {
        let started = o.start(|_| unreachable!());
        (started, *v)
    });
    assert_eq!(r, Some((Err(dkrt_sdk::E_LOADED), 5)));
    assert_eq!(o.with(|v| *v), Some(5));
}

#[test]
fn start_inside_stop_is_refused_and_the_cell_ends_empty() {
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let mut inner = None;
    assert!(o.stop(|_| inner = Some(o.start(put(9)))));
    assert_eq!(inner, Some(Err(dkrt_sdk::E_LOADED)));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(put(1)).is_ok(), "a start after the stop works");
}

#[test]
fn start_inside_start_is_refused() {
    let o: Owner<u32> = Owner::new();
    let mut inner = None;
    let r = o.start(|slot| {
        inner = Some(o.start(put(9)));
        slot.write(4);
        Ok(())
    });
    assert!(r.is_ok());
    assert_eq!(inner, Some(Err(dkrt_sdk::E_LOADED)));
    assert_eq!(o.with(|v| *v), Some(4));
}

#[test]
fn with_and_stop_inside_a_stop_are_refused() {
    // (inside a start nothing is live yet, so `with` and `stop` refuse there without the transition guard: not tested here)
    let o: Owner<u32> = Owner::new();
    o.start(put(5)).unwrap();
    let mut seen = None;
    assert!(o.stop(|_| seen = Some((o.with(|v| *v), o.stop(|_| ()), o.instance().is_null()))));
    assert_eq!(seen, Some((None, false, true)));
}

static DROPS: AtomicU32 = AtomicU32::new(0);
struct Counted;
impl Drop for Counted {
    fn drop(&mut self) {
        DROPS.fetch_add(1, SeqCst);
    }
}

#[test]
fn stop_drops_the_value_once_in_place() {
    let o: Owner<Counted> = Owner::new();
    o.start(put(Counted)).unwrap();
    let at = o.instance();
    let before = DROPS.load(SeqCst);
    assert!(o.stop(|v| assert_eq!(v as *mut Counted, at)));
    assert_eq!(DROPS.load(SeqCst), before + 1);
    assert!(!o.stop(|_| ()));
    assert_eq!(
        DROPS.load(SeqCst),
        before + 1,
        "a second stop drops nothing"
    );
}
