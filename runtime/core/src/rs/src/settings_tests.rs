//! The settings store of the runtime over the host card model: what `tests/settings` first saw on a device (put and get, the delay, copy A then B, a
//! restart that reads the record back, the answers of the mailbox commands, a torn copy, a store that reaches into the settings area).
//! The format and the choice of a copy are `dkrt_sdk::settings`' own tests; the clock here is the UI-loop pass counter (the host has no block counter),
//! its unit is tested by `write_due` in `settings.rs`.
use crate::host_io::card;
use crate::settings::{self as rt, DKRT_SET};
use crate::store::TEST_LAYOUT;
use crate::store_tests::{upload, Fixture, L};
use dkrt_sdk::store::Layout;
use core::ptr::addr_of_mut;
use dkrt_sdk::proto::*;
use dkrt_sdk::settings as set;
use dkrt_sdk::*;
use std::vec::Vec;

/// UI-loop passes that make the delay on the host (`DELAY_PASSES` of `settings.rs`).
const DELAY: u32 = 240;

const COPY_A: u32 = L.set_a();
const COPY_B: u32 = L.set_b();

/// A boot of the settings store: the state of a fresh start (`State::INITIAL`, what the static starts as), then `load` as the boot hook calls it.
unsafe fn boot() {
    *addr_of_mut!(DKRT_SET) = rt::State::INITIAL;
    rt::load();
    // what the load wrote is the store's own; the tests ask what the settings code writes after it
    quiet();
}

/// Forget the writes so far: `written()` then lists what the next steps write.
fn quiet() {
    card(|c| c.log.clear());
}

/// The newest `SETTINGS_LOAD` event: (flags, args).
unsafe fn load_event() -> (u16, [u32; 5]) {
    let (_, newest) = crate::dlog::range();
    let mut b = [0u8; 32];
    for seq in (1..=newest).rev() {
        assert!(crate::dlog::copy_event(seq, b.as_mut_ptr()));
        if u16::from_be_bytes([b[8], b[9]]) == logk::SETTINGS_LOAD {
            let w = |i: usize| u32::from_be_bytes([b[12 + 4 * i], b[13 + 4 * i], b[14 + 4 * i], b[15 + 4 * i]]);
            return (u16::from_be_bytes([b[10], b[11]]), [w(0), w(1), w(2), w(3), w(4)]);
        }
    }
    panic!("no SETTINGS_LOAD event");
}

/// Passes of the UI loop, as many as `n`.
unsafe fn ticks(n: u32) {
    for _ in 0..n {
        rt::tick();
    }
}

/// Every sector written since the last boot, whatever it is (the copies, header B, the store, the selection), sorted.
fn written() -> Vec<u32> {
    card(|c| {
        let mut v = c.log.clone();
        v.sort_unstable();
        v.dedup();
        v
    })
}

fn seq_in(copy_sector: u32) -> Option<u32> {
    card(|c| c.sectors.get(&copy_sector).and_then(|s| set::peek(s)).map(|(seq, _)| seq))
}

/// One mailbox command: `arg` as the mailbox holds it (96 bytes), the answer and the value left in `arg`.
unsafe fn mail(cmd: u32, key: &[u8], value: &[u8]) -> (i32, Vec<u8>) {
    let mut arg = [0u8; 96];
    arg[0] = key.len() as u8;
    // a key or value that does not fit is sent as its lengths alone, as a host that ignores the size would
    if 2 + key.len() + value.len() <= 96 {
        arg[1..1 + key.len()].copy_from_slice(key);
        arg[1 + key.len()] = value.len() as u8;
        arg[2 + key.len()..2 + key.len() + value.len()].copy_from_slice(value);
    } else if 1 + key.len() < 96 {
        arg[1 + key.len()] = value.len() as u8;
    }
    let r = rt::mailbox(cmd, arg.as_mut_ptr());
    (r, if r > 0 { arg[..r as usize].to_vec() } else { Vec::new() })
}

const KEY: &[u8] = b"fx/route/test/dyn-fx-gain";

#[test]
fn a_blank_card_has_an_empty_record_that_the_mailbox_commands_fill() {
    let _f = Fixture::new();
    unsafe {
        boot();
        let (flags, args) = load_event();
        assert_eq!((flags & 1, args), (0, [0, set::NONE, 0, 18, 0]), "no valid copy: state 0, copy none, seq 0, header only, no entry");
        assert_eq!(rt::capacity(), (set::AREA as u32, 18));
        assert!(rt::snapshot().is_none(), "no record while there is no copy and nothing changed");
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]).0, E_SET_NOKEY);

        assert_eq!(mail(CMD_SETTINGS_PUT, KEY, &[1]).0, 0);
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]), (1, [1].to_vec()));
        assert_eq!(mail(CMD_SETTINGS_PUT, b"has space", b"x").0, E_SET_KEY);
        assert_eq!(mail(CMD_SETTINGS_GET, b"has space", &[]).0, E_SET_KEY);
        assert_eq!(mail(CMD_SETTINGS_REMOVE, b"has space", &[]).0, E_SET_KEY);
        assert_eq!(mail(CMD_SETTINGS_GET, b"fx/route/none", &[]).0, E_SET_NOKEY);
        assert_eq!(mail(CMD_SETTINGS_PUT, KEY, &[1]).0, 0, "the same value again");
        assert_eq!(rt::capacity().1, 18 + (1 + KEY.len() as u32 + 2 + 1), "the header and the one entry");
        assert_eq!(mail(CMD_SETTINGS_PUT, KEY, &[0u8; 100]).0, E_SET_VALUE, "a value that does not fit the mailbox argument");
        assert_eq!(mail(CMD_SETTINGS_PUT, &[b'k'; 100], &[]).0, E_SET_KEY, "a key that does not fit the mailbox argument");
        assert_eq!(mail(CMD_SETTINGS_REMOVE, KEY, &[]).0, 0);
        assert_eq!(mail(CMD_SETTINGS_REMOVE, KEY, &[]).0, 0, "also when it is gone");
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]).0, E_SET_NOKEY);

        let (flags, copy, _, _) = {
            let s = rt::snapshot().expect("a change is waiting");
            (s.0, s.1, s.2, s.3.len())
        };
        assert_eq!((flags, copy), (1, set::NONE), "the SETTINGS flag: not on the card, no copy");
    }
}

#[test]
fn the_record_is_written_after_the_delay_to_the_other_copy_and_a_restart_reads_it_back() {
    let _f = Fixture::new();
    unsafe {
        boot();
        mail(CMD_SETTINGS_PUT, KEY, &[1]);
        ticks(DELAY / 2);
        // a second change restarts the delay
        mail(CMD_SETTINGS_PUT, KEY, &[0]);
        ticks(DELAY - 1);
        assert_eq!(written(), Vec::<u32>::new(), "quiet for less than the delay since the last change");
        ticks(1);
        assert_eq!(written(), [L.first, L.first + L.header_b(), COPY_A], "the first write on a blank card makes the empty store (both headers) and writes copy A");
        quiet();
        assert_eq!(seq_in(COPY_A), Some(1));
        let s = rt::snapshot().expect("a copy is on the card");
        assert_eq!((s.0, s.1, s.2), (0, 0, 1), "nothing waiting, copy A, seq 1");
        assert_eq!(set::why(s.3), "ok");
        let copy_a = card(|c| c.sectors[&COPY_A]);

        // the next change goes to the other copy and leaves A as it is
        mail(CMD_SETTINGS_PUT, b"fx/send/test/dyn-fx-gain", &[127; 16]);
        ticks(DELAY);
        assert_eq!(written(), [COPY_B], "only copy B");
        assert_eq!(seq_in(COPY_B), Some(2));
        assert_eq!(card(|c| c.sectors[&COPY_A]), copy_a, "copy A is as it was");

        // a restart: the boot reads copy B
        boot();
        let (flags, args) = load_event();
        assert_eq!((flags & 1, &args[..3], args[4]), (0, &[1, 1, 2][..], 2), "state 1, copy B, seq 2, two entries");
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]), (1, [0].to_vec()));
        let s = rt::snapshot().unwrap();
        assert_eq!((s.0, s.1, s.2), (0, 1, 2));

        // the next write goes to A again with seq 3, and a removed key stays removed
        mail(CMD_SETTINGS_PUT, b"fx/route/test/other", &[1]);
        mail(CMD_SETTINGS_REMOVE, b"fx/send/test/dyn-fx-gain", &[]);
        assert_eq!(mail(CMD_SETTINGS_GET, b"fx/send/test/dyn-fx-gain", &[]).0, E_SET_NOKEY, "gone at once");
        ticks(DELAY);
        assert_eq!(seq_in(COPY_A), Some(3));
        assert_eq!(seq_in(COPY_B), Some(2));
    }
}

#[test]
fn a_flush_writes_a_pending_change_at_once_and_a_clean_record_writes_nothing() {
    let _f = Fixture::new();
    unsafe {
        boot();
        assert_eq!(rt::flush_if_dirty(), 0);
        assert_eq!(written(), Vec::<u32>::new());
        mail(CMD_SETTINGS_PUT, KEY, &[1]);
        assert_eq!(rt::snapshot().unwrap().0 & 1, 1, "the change is not on the card yet");
        assert_eq!(rt::flush_if_dirty(), 0, "what dkrt_restart calls before the reset");
        assert_eq!(written(), [L.first, L.first + L.header_b(), COPY_A], "the empty store (both headers) and copy A");
        boot();
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]), (1, [1].to_vec()));
        assert_eq!(rt::flush_if_dirty(), 0);
        assert_eq!(written(), Vec::<u32>::new(), "nothing is written after the restart");
    }
}

#[test]
fn a_torn_newer_copy_leaves_the_older_and_the_next_write_goes_to_the_torn_one() {
    let _f = Fixture::new();
    unsafe {
        // copy A seq 4, copy B seq 5 with a flipped byte
        for (sector, seq, v) in [(COPY_A, 4u32, &b"old"[..]), (COPY_B, 5, &b"new"[..])] {
            let mut a = [0u8; set::AREA];
            set::init(&mut a[..], seq);
            set::put(&mut a[..], b"k", v).unwrap();
            set::seal(&mut a[..], seq);
            card(|c| {
                let mut s = [0u8; 512];
                s.copy_from_slice(&a[..512]);
                c.sectors.insert(sector, s);
            });
        }
        crate::host_io::card_flip(COPY_B, 20, 0x55);
        boot();
        let (flags, args) = load_event();
        assert_eq!((flags & 1, &args[..3]), (1, &[2, 0, 4][..]), "an error event: state 2, copy A, seq 4");
        let mut out = [0u8; 8];
        assert_eq!(rt::get(b"k", &mut out), 3);
        assert_eq!(&out[..3], b"old");
        let a_before = card(|c| c.sectors[&COPY_A]);
        rt::put(b"k2", b"new");
        ticks(DELAY);
        assert_eq!(seq_in(COPY_B), Some(5), "the next write goes to the other copy with seq 5");
        assert_eq!(card(|c| c.sectors[&COPY_A]), a_before, "the good copy is untouched");
    }
}

#[test]
fn a_store_that_reaches_into_the_settings_area_turns_the_settings_off() {
    let _f = Fixture::new();
    unsafe {
        // a store written when the card was bigger than this layout says: its file runs into the tail of the settings area and header B
        assert_eq!(upload("x", &[7u8; 20 * 512]), OK);
        crate::store::test_forget_checked();
        TEST_LAYOUT.with(|c| c.set(Some(Layout { first: L.first, sectors: 60 })));
        boot();
        let (flags, args) = load_event();
        assert_eq!((flags & 1, args[0], args[3]), (1, 4, E_BAD_STORE as u32), "an error event: state 4, E_BAD_STORE");
        assert_eq!(rt::capacity(), (0, 0));
        assert_eq!(mail(CMD_SETTINGS_PUT, KEY, &[1]).0, E_SET_OFF);
        assert_eq!(mail(CMD_SETTINGS_GET, KEY, &[]).0, E_SET_OFF);
        assert_eq!(mail(CMD_SETTINGS_REMOVE, KEY, &[]).0, E_SET_OFF);
        assert!(rt::snapshot().is_none());
        assert_eq!(rt::flush_if_dirty(), E_SET_OFF);
        ticks(DELAY);
        assert_eq!(written(), Vec::<u32>::new());
    }
}
