use super::*;
use crate::fake::*;
use std::vec;
use std::vec::Vec;

const S: Slot = Slot { win: 2, word: 53 };
const S2: Slot = Slot { win: 2, word: 54 };

unsafe fn settle_all(l: &mut Link<FakeLink>) {
    for _ in 0..200 {
        l.pump();
    }
}

fn seq(steps: &[(u32, u32, u32)]) -> Sequence {
    let mut s = Sequence::new();
    for &(w, a, v) in steps {
        s.push(w, a, v);
    }
    s
}

#[test]
fn several_sets_before_a_send_give_one_write_of_the_newest_value() {
    let mut l = link();
    l.set(S, 1);
    l.set(S, 2);
    l.set(S, 3);
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.words(), vec![(2, 53, 3)]);
    assert_eq!(l.confirmed(S), Some(3));
    assert!(!l.dirty(S));
}

#[test]
fn a_set_during_a_transfer_is_sent_after_it_and_skips_the_values_between() {
    let mut l = link();
    l.set(S, 1);
    unsafe {
        l.pump(); // 1 is on the wire
        l.set(S, 2);
        l.set(S, 3);
        settle_all(&mut l);
    }
    assert_eq!(l.t.words(), vec![(2, 53, 1), (2, 53, 3)]);
    assert_eq!(l.confirmed(S), Some(3));
}

#[test]
fn the_same_value_again_sends_nothing() {
    let mut l = link();
    l.set(S, 7);
    unsafe { settle_all(&mut l) };
    l.set(S, 7);
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.wire.len(), 1);
}

#[test]
fn a_refused_live_write_stays_dirty_and_goes_out_when_the_link_takes_it() {
    let mut l = link();
    l.t.refuse = true;
    l.set(S, 9);
    unsafe { settle_all(&mut l) };
    assert!(l.t.wire.is_empty());
    assert!(l.dirty(S));
    assert_eq!(l.confirmed(S), None);
    l.t.refuse = false;
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.words(), vec![(2, 53, 9)]);
    assert_eq!(l.confirmed(S), Some(9));
}

#[test]
fn an_errored_live_write_is_sent_again_and_logged_as_a_resend() {
    let mut l = link();
    l.t.script.push_back(Some(92));
    l.set(S, 5);
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.words(), vec![(2, 53, 5), (2, 53, 5)]);
    assert_eq!(l.confirmed(S), Some(5));
    assert_eq!(l.errors(), 1);
    let kinds: Vec<u32> = l.t.events.iter().map(|e| e.0).collect();
    assert_eq!(kinds, vec![EV_ERROR, EV_RESEND, EV_LIVE]);
    assert_eq!(l.t.events[0].1[2], 92);
}

#[test]
fn a_dsp_restart_marks_every_word_dirty_and_none_confirmed() {
    let mut l = link();
    l.set(S, 1);
    l.set(S2, 2);
    unsafe { settle_all(&mut l) };
    assert_eq!((l.confirmed(S), l.confirmed(S2)), (Some(1), Some(2)));
    l.dsp_restarted();
    assert_eq!((l.confirmed(S), l.confirmed(S2)), (None, None));
    assert!(l.dirty(S) && l.dirty(S2));
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.wire.len(), 4);
    assert_eq!((l.confirmed(S), l.confirmed(S2)), (Some(1), Some(2)));
}

#[test]
fn a_forgotten_word_is_not_sent() {
    let mut l = link();
    l.set(S, 1);
    l.forget(2, 48, 24);
    unsafe { settle_all(&mut l) };
    assert!(l.t.wire.is_empty());
    assert_eq!(l.confirmed(S), None);
}

/// Finding 129: a writer that has finished but has not taken its result must not see the next writer's result.
#[test]
fn a_result_belongs_to_its_owner_and_never_to_another_writer() {
    let mut l = link();
    l.t.script.push_back(None); // A's word is taken
    l.t.script.push_back(Some(91)); // B's word fails
    let a = l
        .submit(Owner::Loader, &Stream::Sequence(seq(&[(1, 10, 1)])))
        .unwrap();
    let b = l
        .submit(Owner::Tracks, &Stream::owned(0, 0x100, &[7]).unwrap())
        .unwrap();
    unsafe { settle_all(&mut l) }; // both ended before A looks
    assert_eq!(
        l.result(Ticket {
            owner: Owner::Tracks,
            id: a.id
        }),
        LinkResult::Gone,
        "another owner cannot take A's result"
    );
    assert_eq!(l.result(a), LinkResult::Done);
    assert_eq!(l.result(a), LinkResult::Gone, "a result is given once");
    assert_eq!(l.result(b), LinkResult::Failed);
}

#[test]
fn the_owners_requests_go_out_in_the_order_they_were_submitted() {
    let mut l = link();
    let a1 = l
        .submit(
            Owner::Loader,
            &Stream::Sequence(seq(&[(1, 1, 1), (1, 2, 2)])),
        )
        .unwrap();
    let b1 = l
        .submit(Owner::Tracks, &Stream::owned(2, 5, &[5]).unwrap())
        .unwrap();
    let a2 = l
        .submit(Owner::Loader, &Stream::Sequence(seq(&[(2, 3, 3)])))
        .unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(
        l.t.words(),
        vec![(1, 1, 1), (1, 2, 2), (2, 5, 5), (2, 3, 3)]
    );
    assert_eq!(
        (l.result(a1), l.result(b1), l.result(a2)),
        (LinkResult::Done, LinkResult::Done, LinkResult::Done)
    );
}

#[test]
fn a_sequence_sends_its_steps_in_order_and_the_publishing_word_last() {
    let mut l = link();
    let mut flag = 0u8;
    let mut s = Sequence::new();
    for w in 0..5 {
        s.push(1, 24 * 3 + 2 + w, w);
    }
    s.push_undo(1, 24 * 3, 17, 0xffff_ffff); // the type word, last
    s.set_mark(&mut flag, 5);
    let t = l.submit(Owner::Loader, &Stream::Sequence(s)).unwrap();
    unsafe { settle_all(&mut l) };
    let words = l.t.words();
    assert_eq!(words.len(), 6);
    assert_eq!(words[5], (1, 72, 17));
    assert_eq!(l.result(t), LinkResult::Done);
    assert_eq!(
        flag, 1,
        "the mark is set when the publishing word is accepted"
    );
}

#[test]
fn a_failed_publishing_word_is_undone_and_the_request_fails() {
    let mut l = link();
    let mut s = Sequence::new();
    s.push(2, 100, 1);
    s.push(2, 101, 2);
    s.push_undo(2, 102, 3, 0xffff_ffff);
    for _ in 0..2 {
        l.t.script.push_back(None);
    }
    l.t.script.push_back(Some(92)); // the last word errors
    let t = l.submit(Owner::Loader, &Stream::Sequence(s)).unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(
        l.t.words(),
        vec![(2, 100, 1), (2, 101, 2), (2, 102, 3), (2, 102, 0xffff_ffff)]
    );
    assert_eq!(l.result(t), LinkResult::Failed);
}

#[test]
fn the_undo_words_run_in_reverse_order_of_the_steps() {
    let mut l = link();
    let mut s = Sequence::new();
    s.push_undo(2, 10, 1, 100);
    s.push_undo(2, 11, 2, 101);
    s.push_undo(2, 12, 3, 102);
    s.push(2, 13, 4);
    l.t.script.extend([None, None, None, Some(1)]);
    let t = l.submit(Owner::Loader, &Stream::Sequence(s)).unwrap();
    unsafe { settle_all(&mut l) };
    let w = l.t.words();
    assert_eq!(&w[4..], &[(2, 12, 102), (2, 11, 101), (2, 10, 100)]);
    assert_eq!(l.result(t), LinkResult::Failed);
}

#[test]
fn a_failed_step_without_undo_words_fails_at_once() {
    let mut l = link();
    l.t.script.push_back(Some(3));
    let t = l
        .submit(
            Owner::Loader,
            &Stream::Sequence(seq(&[(2, 1, 1), (2, 2, 2)])),
        )
        .unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.wire.len(), 1, "the second step is not sent");
    assert_eq!(l.result(t), LinkResult::Failed);
}

#[test]
fn a_failed_undo_is_reported_as_a_rollback_that_did_not_complete() {
    let mut l = link();
    let mut s = Sequence::new();
    s.push_undo(2, 10, 1, 0);
    l.t.script.extend([Some(1), Some(2)]);
    let t = l.submit(Owner::Loader, &Stream::Sequence(s)).unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(l.result(t), LinkResult::RollbackFailed);
}

#[test]
fn a_live_edit_goes_between_two_chunks_of_a_bulk() {
    let mut l = link();
    let data = vec![0xabu8; (2 * CHUNK + 8) as usize];
    let t = l
        .submit(
            Owner::Loader,
            &Stream::Bulk {
                dst: 0x2000_0000,
                src: data.as_ptr(),
                len: data.len() as u32,
            },
        )
        .unwrap();
    unsafe {
        l.pump(); // chunk 1 is on the wire
        l.set(S, 42);
        settle_all(&mut l);
    }
    let w = &l.t.wire;
    assert_eq!(w.len(), 4);
    assert_eq!((w[0].win, w[0].bytes.len()), (0, CHUNK as usize));
    assert_eq!(
        (w[1].win, w[1].dst),
        (2, 53),
        "the live word goes before chunk 2"
    );
    assert_eq!(
        (w[2].dst, w[2].bytes.len()),
        (0x2000_0000 + CHUNK, CHUNK as usize)
    );
    assert_eq!((w[3].dst, w[3].bytes.len()), (0x2000_0000 + 2 * CHUNK, 8));
    assert_eq!(l.result(t), LinkResult::Done);
}

#[test]
fn a_failed_chunk_fails_the_bulk_and_sends_no_more_of_it() {
    let mut l = link();
    l.t.script.push_back(Some(100));
    let data = vec![0u8; (CHUNK + 4) as usize];
    let t = l
        .submit(
            Owner::Loader,
            &Stream::Bulk {
                dst: 0x2000_0000,
                src: data.as_ptr(),
                len: data.len() as u32,
            },
        )
        .unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(l.t.wire.len(), 1);
    assert_eq!(l.result(t), LinkResult::Failed);
}

#[test]
fn drain_cancels_queued_requests_waits_for_the_transfer_in_flight_and_keeps_other_owners() {
    let mut l = link();
    let data = vec![1u8; 2 * CHUNK as usize];
    let a1 = l
        .submit(
            Owner::Loader,
            &Stream::Bulk {
                dst: 0x2000_0000,
                src: data.as_ptr(),
                len: data.len() as u32,
            },
        )
        .unwrap();
    let a2 = l
        .submit(Owner::Loader, &Stream::Sequence(seq(&[(1, 1, 1)])))
        .unwrap();
    let b = l
        .submit(Owner::Tracks, &Stream::owned(2, 5, &[5]).unwrap())
        .unwrap();
    unsafe {
        l.pump(); // chunk 1 of A's bulk is on the wire
        assert!(l.drain(Owner::Loader));
        assert_eq!(l.t.wire.len(), 1, "nothing more of A's bulk was sent");
        let before = l.t.wire.len();
        settle_all(&mut l);
        // only B's word follows; A's chunk 2 and sequence are gone
        assert_eq!(l.t.words()[before..], [(2, 5, 5)]);
    }
    assert_eq!(
        (l.result(a1), l.result(a2)),
        (LinkResult::Gone, LinkResult::Gone)
    );
    assert_eq!(l.result(b), LinkResult::Done);
}

#[test]
fn a_blocking_run_returns_the_outcome_of_the_sequence() {
    let mut l = link();
    unsafe {
        assert!(l.run(Owner::Core, &seq(&[(1, 0, 0xffff_ffff), (1, 1, 2)])));
        l.t.script.push_back(Some(5));
        assert!(!l.run(Owner::Core, &seq(&[(1, 3, 3)])));
    }
}

#[test]
fn a_full_queue_refuses_a_submit_until_a_result_is_taken() {
    let mut l = link();
    let one = Stream::Sequence(seq(&[(1, 1, 1)]));
    let mut ts = Vec::new();
    for _ in 0..REQ_MAX {
        ts.push(l.submit(Owner::Loader, &one).unwrap());
    }
    assert!(l.submit(Owner::Loader, &one).is_none());
    unsafe { settle_all(&mut l) };
    assert_eq!(l.result(ts[0]), LinkResult::Done);
    assert!(l.submit(Owner::Loader, &one).is_some());
}

#[test]
fn a_request_for_the_reserved_window_or_an_odd_size_is_refused() {
    let mut l = link();
    assert!(l
        .submit(Owner::Core, &Stream::Sequence(seq(&[(3, 0, 0)])))
        .is_none());
    assert!(l
        .submit(Owner::Core, &Stream::Sequence(Sequence::new()))
        .is_none());
    assert!(l
        .submit(
            Owner::Core,
            &Stream::Bulk {
                dst: 0x2000_0000,
                src: [0u8; 6].as_ptr(),
                len: 6
            }
        )
        .is_none());
    l.set(Slot { win: 3, word: 0 }, 1);
    unsafe { settle_all(&mut l) };
    assert!(l.t.wire.is_empty());
}

#[test]
fn the_stats_control_words_reach_window_3_only_through_set_stats_ctl() {
    let mut l = link();
    assert!(l.set_stats_ctl(STATS_ON, 1));
    assert!(l.set_stats_ctl(STATS_CLEAR, 7));
    assert!(l.set_stats_ctl(STATS_PERIOD, 666_582));
    assert!(!l.set_stats_ctl(3, 1), "no fourth control word");
    unsafe { settle_all(&mut l) };
    let mut w = l.t.words();
    w.sort();
    assert_eq!(w, vec![(3, 0, 1), (3, 1, 7), (3, 2, 666_582)]);
    assert_eq!(l.confirmed(Slot::new(3, 1)), Some(7));
}

#[test]
fn a_hard_refusal_fails_the_request_instead_of_waiting_for_ever() {
    let mut l = link();
    l.t.hard = true;
    let t = l
        .submit(Owner::Loader, &Stream::Sequence(seq(&[(1, 1, 1)])))
        .unwrap();
    unsafe { settle_all(&mut l) };
    assert_eq!(l.result(t), LinkResult::Failed);
}

#[test]
fn requests_log_submit_start_done_and_error_events() {
    let mut l = link();
    l.t.script.push_back(None);
    l.t.script.push_back(Some(7));
    let a = l
        .submit(Owner::Loader, &Stream::Sequence(seq(&[(1, 1, 1)])))
        .unwrap();
    let b = l
        .submit(Owner::Tracks, &Stream::owned(2, 5, &[5]).unwrap())
        .unwrap();
    unsafe { settle_all(&mut l) };
    l.result(a);
    l.result(b);
    let kinds: Vec<u32> = l.t.events.iter().map(|e| e.0).collect();
    assert_eq!(
        kinds,
        vec![EV_SUBMIT, EV_SUBMIT, EV_START, EV_DONE, EV_START, EV_ERROR]
    );
}
