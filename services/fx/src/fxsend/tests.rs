use super::*;
use dkrt_sdk::*;

/// The offsets that the device tests read from `DKRT_FXS` (`fx-routes-ui.ts`, `fx-track-route.device.test.ts`).
#[test]
fn the_layout_the_device_tests_read_is_unchanged() {
    assert_eq!(core::mem::offset_of!(SendState, restores), 16);
    assert_eq!(core::mem::offset_of!(SendState, allow), 40);
    assert_eq!(core::mem::offset_of!(SendState, dflt), 44);
}

/// The four DSP amount words take the 16 track amounts as bytes: track t is byte t % 4 of word t / 4 (low byte first).
#[test]
fn send_amounts_are_packed_four_tracks_to_a_word() {
    let mut a = [0u8; TRACKS];
    a[0] = 1;
    a[1] = 2;
    a[3] = 127;
    a[4] = 5;
    a[15] = 9;
    assert_eq!(words_of(&a), [0x7f00_0201, 5, 0, 9 << 24]);
    assert_eq!(words_of(&[0; TRACKS]), [0; 4]);
}

/// The routes a record allows and its default, from its flags: master unless NOMASTER, send when SEND, default send only with DEFAULT_SEND.
#[test]
fn route_flags_give_the_allowed_routes_and_the_default() {
    assert_eq!(routes_of(0), (ROUTE_MASTER, ROUTE_MASTER));
    assert_eq!(
        routes_of(FXF_SEND),
        (ROUTE_MASTER | ROUTE_SEND, ROUTE_MASTER)
    );
    assert_eq!(
        routes_of(FXF_NOMASTER | FXF_SEND),
        (ROUTE_SEND, ROUTE_MASTER)
    );
    assert_eq!(
        routes_of(FXF_SEND | FXF_DEFAULT_SEND),
        (ROUTE_MASTER | ROUTE_SEND, ROUTE_SEND)
    );
}

/// The settings keys are `fx/route/<module id>` and `fx/send/<module id>`; the id is copied up to its length.
#[test]
fn settings_keys_carry_the_module_id() {
    let mut id = [0u8; 32];
    id[..9].copy_from_slice(b"fx/gain-x");
    let mut buf = [0u8; KEY_MAX];
    let n = key_of(&mut buf, b"fx/send/", &id, 9);
    assert_eq!(&buf[..n], b"fx/send/fx/gain-x");
}

/// The saved route is used when the effect allows it; a route it does not allow, a value that is no route and no entry give the default.
#[test]
fn the_saved_route_falls_back_to_the_default() {
    use dkrt_test_support::FakeCore;
    let _g = crate::test_lock::hold();
    let core = FakeCore::new();
    unsafe {
        let s = ss();
        s.id[3][..4].copy_from_slice(b"mod0");
        s.idlen[3] = 4;
        s.allow[3] = ROUTE_MASTER | ROUTE_SEND;
        s.dflt[3] = ROUTE_MASTER;
        assert_eq!(saved_route(&core, 3), ROUTE_MASTER, "no entry: the default");
        core.settings
            .borrow_mut()
            .push((b"fx/route/mod0".to_vec(), std::vec![1]));
        assert_eq!(saved_route(&core, 3), ROUTE_SEND);
        s.allow[3] = ROUTE_MASTER;
        assert_eq!(
            saved_route(&core, 3),
            ROUTE_MASTER,
            "the effect no longer allows send"
        );
        s.allow[3] = ROUTE_MASTER | ROUTE_SEND;
        core.settings.borrow_mut()[0].1 = std::vec![7];
        assert_eq!(saved_route(&core, 3), ROUTE_MASTER, "7 is no route");
    }
}

#[test]
fn route_choices_use_pending_sends_and_the_image_slot_limit() {
    let both = ROUTE_MASTER | ROUTE_SEND;
    assert_eq!(allowed_choices(both, 2, 0, 0), ROUTE_MASTER.into());
    assert_eq!(allowed_choices(both, 2, 1, 2), both.into());
    assert_eq!(allowed_choices(both, 2, 1, 1), ROUTE_MASTER.into());
    assert_eq!(allowed_choices(both, 2, 5, 1), both.into());
    assert_eq!(allowed_choices(ROUTE_SEND, 2, 1, 1), 0);
    assert_eq!(allowed_choices(ROUTE_MASTER, 2, 0, 2), ROUTE_MASTER.into());
}

/// The describe words of an effect module: one word for each of its rows that has a page, in row order: the row (bits 0 to 7), then the route it runs on, the route the settings
/// give it, the routes it allows and its default, four bits each. A module with no such row gets no word.
#[test]
fn describe_gives_one_word_for_each_effect_row_of_the_consumer() {
    use dkrt_abi::provider::{ConsumerId, Describe, WordSink};
    use dkrt_test_support::FakeCore;
    let _g = crate::test_lock::hold();
    let core = FakeCore::new();
    let both = ROUTE_MASTER | ROUTE_SEND;
    unsafe {
        let s = ss();
        for (k, owner, allow, running_send) in [
            (0, 2u8, both, true),
            (2, 2, ROUTE_MASTER, false),
            (3, 3, both, false),
        ] {
            crate::fxpage::set_page_owner_for_test(k, owner);
            s.id[k][..2].copy_from_slice(&[b'm', b'0' + k as u8]);
            s.idlen[k] = 2;
            s.allow[k] = allow;
            s.dflt[k] = ROUTE_MASTER;
            s.route[k] = running_send as _;
        }
        // row 0 is saved as master while it runs on the send bus (a change waiting for its reload)
        core.settings
            .borrow_mut()
            .push((b"fx/route/m0".to_vec(), std::vec![0]));
    }
    let words = |who: u8| {
        let mut buf = [0u32; 8];
        let mut sink = WordSink::new(&mut buf);
        <crate::fx::Fx as Describe<FakeCore>>::describe(&core, ConsumerId::new(who), &mut sink);
        sink.words().to_vec()
    };
    let word = |row: u32, running: u8, saved: u8, allowed: u8, dflt: u8| {
        row | (running as u32) << 8
            | (saved as u32) << 12
            | (allowed as u32) << 16
            | (dflt as u32) << 20
    };
    assert_eq!(
        words(2),
        std::vec![
            word(0, ROUTE_SEND, ROUTE_MASTER, both, ROUTE_MASTER),
            word(2, ROUTE_MASTER, ROUTE_MASTER, ROUTE_MASTER, ROUTE_MASTER),
        ]
    );
    assert_eq!(
        words(3),
        std::vec![word(3, ROUTE_MASTER, ROUTE_MASTER, both, ROUTE_MASTER)]
    );
    assert_eq!(
        words(9),
        std::vec::Vec::<u32>::new(),
        "no row of that consumer: no section"
    );
    assert_eq!(
        words(0),
        std::vec::Vec::<u32>::new(),
        "a free row has owner 0, which is no consumer"
    );
    unsafe {
        for k in [0, 2, 3] {
            crate::fxpage::set_page_owner_for_test(k, 0);
        }
    }
}
