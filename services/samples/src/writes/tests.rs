use super::*;
use dkrt_test_support::FakeCore;

/// Client 0, track 1 has an op `kind`; its records and control sets are at fixed L2 addresses.
fn seeded(kind: u8, rec: [u32; 8]) -> Writes {
    let mut w = Writes::new();
    w.reserve(0, 0x1000, 0x2000);
    w.ready(0);
    w.ops[0][1] = Op {
        kind,
        step: 0,
        tries: 0,
        rec,
    };
    w
}

/// The writer's pass and the link's pass, `n` times (the model's time passes with each call).
fn passes(w: &mut Writes, fake: &FakeCore, n: usize) {
    for _ in 0..n {
        // SAFETY: the fake's link calls
        unsafe {
            w.pump(fake);
            fake.link.borrow_mut().pump();
        }
    }
}

#[test]
fn a_publish_op_submits_its_three_writes_in_order() {
    let mut w = seeded(OP_PUBLISH, [1, 2, 3, 4, 5, 6, 7, 8]);
    let fake = FakeCore::new();
    passes(&mut w, &fake, 60);
    let words = |w: &[u32]| {
        w.iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<std::vec::Vec<u8>>()
    };
    let wire: std::vec::Vec<(u32, u32, std::vec::Vec<u8>)> = fake
        .link
        .borrow()
        .t
        .wire
        .iter()
        .map(|w| (w.win, w.dst, w.bytes.clone()))
        .collect();
    assert_eq!(
        wire,
        [
            (0, 0x1000 + 32 + 4, words(&[2, 3, 4, 5, 6, 7, 8])),
            (0, 0x2000 + 16, words(&[0])),
            (0, 0x1000 + 32, words(&[1])),
        ]
    );
    assert_eq!(w.ops[0][1].kind, OP_NONE);
}

#[test]
fn a_load_in_progress_holds_the_table_writes() {
    let mut w = seeded(OP_CLOSE, [0; 8]);
    let fake = FakeCore::new();
    fake.dsp().phase = dkrt_abi::dsp::PH_SEND;
    passes(&mut w, &fake, 20);
    assert!(fake.link.borrow().t.wire.is_empty());
    fake.dsp().phase = PH_IDLE;
    passes(&mut w, &fake, 40);
    assert_eq!(fake.link.borrow().t.wire.len(), 2);
}

#[test]
fn a_write_that_fails_is_tried_again_and_a_stuck_op_ends_failed() {
    let mut w = seeded(OP_CLOSE, [0; 8]);
    let fake = FakeCore::new();
    fake.link.borrow_mut().t.script.push_back(Some(92));
    passes(&mut w, &fake, 80);
    // the first write failed once and was sent again; both writes of the close then went out
    assert_eq!(fake.link.borrow().t.wire.len(), 3);
    assert_eq!(w.ops[0][1].kind, OP_NONE);
    let mut w = seeded(OP_CLOSE, [0; 8]);
    fake.link
        .borrow_mut()
        .t
        .script
        .extend(std::iter::repeat(Some(92)).take(TRIES as usize));
    passes(&mut w, &fake, 400);
    assert_eq!(w.ops[0][1].kind, OP_FAILED);
    assert_eq!(w.published(0, 1), dkrt_sdk::E_DSP_XFER);
}

#[test]
fn a_close_before_the_image_is_in_writes_nothing_and_a_teardown_drains_at_once() {
    let mut w = Writes::new();
    w.reserve(0, 0x1000, 0x2000);
    w.close(0, 1);
    assert!(w.drained(0, 1), "no image, nothing to close");
    w.ready(0);
    w.close(0, 1);
    assert!(!w.drained(0, 1));
    w.begin_teardown(0);
    assert!(
        w.drained(0, 1),
        "a module that is taken down drains at once"
    );
    w.close(0, 2);
    assert_eq!(w.ops[0][2].kind, OP_NONE, "its close is not queued");
    w.publish(0, 2, &Strack::default());
    assert_eq!(w.ops[0][2].kind, OP_NONE, "its publish is dropped");
    w.end_teardown();
    w.publish(0, 2, &Strack::default());
    assert_eq!(w.ops[0][2].kind, OP_PUBLISH);
    w.release(0);
    assert!(!w.is_live(0) && w.ops[0][2].kind == OP_NONE);
}
