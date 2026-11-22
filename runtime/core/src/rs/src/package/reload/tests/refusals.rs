//! The refusals of a live job, in their documented order.
use super::*;

#[test]
fn a_stopped_sequencer_and_no_other_fact_allows_the_reload() {
    let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
    assert_eq!(code(&p, Facts::default()), None);
}

#[test]
fn playing_paused_mixed_or_unreadable_transport_refuses_the_reload() {
    let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
    for s in [Some(1), Some(2), Some(3), None] {
        p.play.set(s);
        assert_eq!(code(&p, Facts::default()), Some(E_RELOAD_PLAYING), "{s:?}");
    }
}

#[test]
fn the_refusals_come_in_the_documented_order() {
    let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
    p.play.set(Some(1));
    let mut f = all();
    assert_eq!(code(&p, f), Some(E_SAFE_MODE));
    f.safe_mode = false;
    assert_eq!(code(&p, f), Some(E_RELOAD_PLAYING));
    p.play.set(Some(0));
    assert_eq!(code(&p, f), Some(dkrt_sdk::proto::E_STATE));
    f.upload_open = false;
    assert_eq!(code(&p, f), Some(E_CARD_BUSY));
    f.card_busy = false;
    assert_eq!(code(&p, f), Some(E_RELOAD_MENUS));
    f.menus = false;
    assert_eq!(code(&p, f), None);
}
