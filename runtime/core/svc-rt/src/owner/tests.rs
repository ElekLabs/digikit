use super::*;

#[test]
fn with_gives_nothing_before_start_and_after_stop() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert!(o.start(|| Ok(7)).is_ok());
    assert_eq!(o.with(|v| *v), Some(7));
    assert_eq!(
        o.with(|v| {
            *v += 1;
            *v
        }),
        Some(8)
    );
    assert_eq!(o.stop(), Some(8));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.instance().is_null());
    assert_eq!(o.stop(), None);
    // and it can start again after a stop
    assert!(o.start(|| Ok(1)).is_ok());
}

#[test]
fn a_second_start_is_refused_without_making_an_instance() {
    let o: Owner<u32> = Owner::new();
    let at = o.start(|| Ok(1)).unwrap();
    assert_eq!(o.instance(), at);
    let mut made = false;
    let r = o.start(|| {
        made = true;
        Ok(2)
    });
    assert_eq!(r, Err(dkrt_sdk::E_LOADED));
    assert!(!made);
    assert_eq!(o.with(|v| *v), Some(1));
}

#[test]
fn a_start_that_fails_leaves_the_cell_empty() {
    let o: Owner<u32> = Owner::new();
    assert_eq!(o.start(|| Err(dkrt_sdk::E_NOMEM)), Err(dkrt_sdk::E_NOMEM));
    assert_eq!(o.with(|v| *v), None);
    assert!(o.start(|| Ok(3)).is_ok());
}

#[test]
fn a_nested_with_finds_nothing_instead_of_a_second_reference() {
    let o: Owner<u32> = Owner::new();
    o.start(|| Ok(5)).unwrap();
    let inner = o.with(|_| o.with(|v| *v));
    assert_eq!(inner, Some(None));
    assert_eq!(o.with(|v| *v), Some(5), "the cell is usable again after");
}
