//! Turn dispatch.
use super::*;
use crate::native_page::fake_firmware::*;

#[test]
fn turn_dispatch() {
    let t = table(0, 1);
    let v = FakeView::new();
    let mut m = Module::default();
    let ev = |control, delta, coarse| Event {
        control,
        delta,
        coarse,
    };
    assert_eq!(
        turn(&t, &v, &ev(8, 1, false), &mut m),
        0,
        "PARAM edit runs stock"
    );
    assert_eq!(turn(&t, &v, &ev(1, -2, true), &mut m), 1);
    assert_eq!(m.events, [(0, 9, -2, true)]);
    assert_eq!(*v.invalidated.borrow(), 1);
    assert_eq!(
        turn(&t, &v, &ev(5, 1, false), &mut m),
        1,
        "unused is consumed"
    );
    assert_eq!(m.events.len(), 1);
    assert_eq!(turn(&t, &v, &ev(0, 1, false), &mut m), 0);
    assert_eq!(turn(&t, &v, &ev(9, 1, false), &mut m), 0);
    let mut other = FakeView::new();
    other.index = 0;
    assert_eq!(
        turn(&t, &other, &ev(1, 1, false), &mut m),
        0,
        "not a native page"
    );
}
