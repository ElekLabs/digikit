use super::*;
use crate::pool::Pool;
use crate::tracks::Tracks;
use dkrt_sdk::sample_tracks::QUEUED;
use dkrt_test_support::FakeCore;
use std::boxed::Box;
use std::format;

/// A table of the test's own, with or without a backing adapter.
fn cell(backed: bool) -> &'static PoolCell {
    let mut t = PoolTable::new();
    t.backed = backed;
    Box::leak(Box::new(PoolCell::new(t)))
}

/// The owner side of one client: its state machine and a table, as `Samples` holds them. The client is published, so handle 1 finds its table.
struct Rig {
    core: FakeCore,
    tracks: Box<Tracks>,
    table: &'static PoolCell,
}

impl Rig {
    fn new() -> Rig {
        let core = FakeCore::new();
        let mut tracks = Box::new(Tracks::new());
        let table = cell(true);
        assert!(tracks.reserve(4, 1, 0x1000, 0x2000));
        tracks.mirror(&core, table);
        Rig {
            core,
            tracks,
            table,
        }
    }
    fn add(&self, path: &[u8]) -> i32 {
        let z = [path, b"\0"].concat();
        unsafe { pool_add(self.table, &self.core, 1, z.as_ptr()) }
    }
    fn set(&self, index: u32, path: &[u8]) -> i32 {
        let z = [path, b"\0"].concat();
        unsafe { pool_set(self.table, &self.core, 1, index, z.as_ptr()) }
    }
    fn clear(&self, index: u32) -> i32 {
        pool_clear(self.table, &self.core, 1, index)
    }
    fn status(&self, index: u32) -> i32 {
        pool_status(self.table, &self.core, 1, index)
    }
    fn path(&self, index: u32) -> Result<std::vec::Vec<u8>, i32> {
        let mut buf = [0u8; 64];
        let n = unsafe { pool_path(self.table, &self.core, 1, index, buf.as_mut_ptr(), 64) };
        if n < 0 {
            Err(n)
        } else {
            Ok(buf[..n as usize].to_vec())
        }
    }
}

#[test]
fn a_client_is_visible_to_the_calls_once_the_owner_published_it() {
    let core = FakeCore::new();
    let table = cell(true);
    assert_eq!(pool_status(table, &core, 1, 0), E_NOT_FOUND);
    let mut tracks = Box::new(Tracks::new());
    assert!(tracks.reserve(4, 1, 0x1000, 0x2000));
    assert_eq!(
        pool_status(table, &core, 1, 0),
        E_NOT_FOUND,
        "not published yet"
    );
    tracks.mirror(&core, table);
    assert_eq!(pool_status(table, &core, 1, 0), UNUSED);
    assert_eq!(
        pool_status(table, &core, 2, 0),
        E_NOT_FOUND,
        "the other client slot is free"
    );
}

#[test]
fn add_answers_at_once_with_the_first_free_index_and_the_known_index_for_a_known_path() {
    let r = Rig::new();
    assert_eq!(r.add(b"SND/A"), 0);
    assert_eq!(r.add(b"SND/B"), 1);
    assert_eq!(r.add(b"SND/A"), 0, "a file added twice is one entry");
    assert_eq!(
        r.status(0),
        QUEUED,
        "the call's own result is visible at once"
    );
    assert_eq!(
        r.path(1).as_deref(),
        Ok(&b"SND/B"[..]),
        "root-relative, as given"
    );
}

#[test]
fn a_clear_is_a_hole_that_status_and_the_next_add_see_at_once() {
    let r = Rig::new();
    assert_eq!(r.add(b"SND/A"), 0);
    assert_eq!(r.add(b"SND/B"), 1);
    assert_eq!(r.clear(0), 0);
    assert_eq!(r.status(0), UNUSED, "read your own write");
    assert_eq!(r.path(0), Err(E_NOT_FOUND));
    assert_eq!(r.status(1), QUEUED, "the other index stays");
    assert_eq!(r.add(b"SND/C"), 0, "the hole is the next one an add takes");
}

#[test]
fn set_replaces_the_entry_and_a_full_pool_refuses_add() {
    let r = Rig::new();
    assert_eq!(r.set(3, b"SND/X"), 0);
    assert_eq!(r.status(3), QUEUED);
    assert_eq!(r.path(3).as_deref(), Ok(&b"SND/X"[..]));
    assert_eq!(r.set(3, b"SND/Y"), 0);
    assert_eq!(r.path(3).as_deref(), Ok(&b"SND/Y"[..]));
    for i in 0..ENTRIES - 1 {
        assert!(r.add(format!("SND/{i}").as_bytes()) >= 0);
    }
    assert_eq!(r.add(b"SND/OVER"), E_POOL);
}

#[test]
fn the_refusals_come_in_the_order_the_calls_always_had() {
    let r = Rig::new();
    let none = cell(false);
    unsafe {
        assert_eq!(pool_add(r.table, &r.core, 1, core::ptr::null()), E_SEL_PATH);
        assert_eq!(
            pool_add(none, &r.core, 1, b"A\0".as_ptr()),
            E_CONTRACT,
            "no adapter"
        );
        assert_eq!(
            pool_add(r.table, &r.core, 2, b"A\0".as_ptr()),
            E_NOT_FOUND,
            "no such table"
        );
        assert_eq!(
            pool_add(r.table, &r.core, 1, b"\0".as_ptr()),
            E_SEL_PATH,
            "empty path"
        );
        assert_eq!(
            pool_set(r.table, &r.core, 1, 16, b"A\0".as_ptr()),
            E_CONTRACT,
            "index"
        );
    }
    assert_eq!(r.status(16), E_CONTRACT);
    assert_eq!(r.clear(16), E_CONTRACT);
    assert_eq!(r.path(16), Err(E_CONTRACT));
    r.add(b"SND/LONG-NAME");
    let mut tiny = [0u8; 4];
    assert_eq!(
        unsafe { pool_path(r.table, &r.core, 1, 0, tiny.as_mut_ptr(), 4) },
        E_SEL_PATH,
        "the buffer is too small"
    );
}

#[test]
fn info_is_the_published_words_of_a_ready_entry_only() {
    let mut r = Rig::new();
    let mut out = [0u8; 16];
    let info = |r: &Rig, out: &mut [u8; 16]| unsafe {
        pool_info(r.table, &r.core, 1, 0, out.as_mut_ptr())
    };
    assert_eq!(info(&r, &mut out), E_NOT_FOUND, "an unused entry has none");
    r.add(b"SND/A");
    assert_eq!(info(&r, &mut out), E_NOT_FOUND, "queued is not ready");
    // the machine says READY with its info
    let mut open = [false; CLIENTS];
    open[0] = true;
    let mut snap = [[Snap {
        status: UNUSED,
        info: Info {
            frames: 0,
            rate: 0,
            channels: 0,
            token: 0,
        },
    }; ENTRIES]; CLIENTS];
    snap[0][0] = Snap {
        status: READY,
        info: Info {
            frames: 1000,
            rate: 48000,
            channels: 2,
            token: 7,
        },
    };
    // a waiting request wins over the publication
    r.table.with(&r.core, |t| t.publish(&open, &snap));
    assert_eq!(info(&r, &mut out), E_NOT_FOUND);
    // once the owner took the request, the publication is shown
    r.tracks.intake(&r.core, r.table);
    r.table.with(&r.core, |t| t.publish(&open, &snap));
    assert_eq!(info(&r, &mut out), 0);
    let w = |i: usize| u32::from_be_bytes(out[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!((w(0), w(1), w(2), w(3)), (1000, 48000, 2, 7));
}

#[test]
fn a_status_the_owner_computed_never_overwrites_a_waiting_request() {
    let mut r = Rig::new();
    assert_eq!(r.add(b"SND/A"), 0);
    // the owner publishes before it took the request: the machine still says UNUSED
    r.tracks.mirror(&r.core, r.table);
    assert_eq!(r.status(0), QUEUED, "the newer request wins");
    r.tracks.intake(&r.core, r.table);
    r.tracks.mirror(&r.core, r.table);
    assert_eq!(r.status(0), QUEUED, "the machine now has the select");
    assert_eq!(r.tracks.core().status(0, 0), QUEUED);
}

#[test]
fn the_owner_applies_the_requests_to_the_machine_the_last_one_wins() {
    let mut r = Rig::new();
    r.add(b"SND/A");
    r.set(0, b"SND/B");
    r.add(b"SND/C");
    r.clear(1);
    r.tracks.intake(&r.core, r.table);
    assert_eq!(
        r.tracks.core().path(0, 0),
        Ok(&b"/SND/B"[..]),
        "the last request of the entry"
    );
    assert_eq!(
        r.tracks.core().status(0, 1),
        UNUSED,
        "a cleared entry that was never loaded"
    );
    // a second intake finds nothing
    r.tracks.intake(&r.core, r.table);
    assert_eq!(r.tracks.core().path(0, 0), Ok(&b"/SND/B"[..]));
}

#[test]
fn a_released_client_has_no_entries_and_no_requests() {
    let mut r = Rig::new();
    r.add(b"SND/A");
    r.tracks.release(&r.core, &Pool::none(), 4, |_| {});
    r.tracks.mirror(&r.core, r.table);
    assert_eq!(r.status(0), E_NOT_FOUND);
    // the slot is taken by another module: it starts empty
    assert!(r.tracks.reserve(6, 1, 0x3000, 0x4000));
    r.tracks.mirror(&r.core, r.table);
    assert_eq!(r.status(0), UNUSED);
    r.tracks.intake(&r.core, r.table);
    assert_eq!(
        r.tracks.core().status(0, 0),
        UNUSED,
        "the old request is gone"
    );
}

#[test]
fn every_call_is_one_masked_section() {
    let r = Rig::new();
    let taken = || r.core.host.masks_taken.get();
    let one = |what: &str, f: &dyn Fn()| {
        let before = taken();
        f();
        assert_eq!(taken(), before + 1, "{what}");
        assert_eq!(r.core.host.masked.get(), 0, "{what} leaves a mask open");
    };
    one("add", &|| {
        r.add(b"SND/A");
    });
    one("set", &|| {
        r.set(1, b"SND/B");
    });
    one("status", &|| {
        r.status(0);
    });
    one("path", &|| {
        let _ = r.path(0);
    });
    one("clear", &|| {
        r.clear(0);
    });
}

#[test]
fn a_task_switch_between_two_adds_cannot_make_them_share_an_entry() {
    // the interrupt (or the MIDI task) runs when the first add gives its mask back, and adds another file: the table decides each add in one section
    let r = Rig::new();
    let other: &'static FakeCore = Box::leak(Box::new(FakeCore::new()));
    let table = r.table;
    let got = std::rc::Rc::new(core::cell::Cell::new(-99));
    let got2 = got.clone();
    r.core.host.interrupt_at_restore(1, move || {
        got2.set(unsafe { pool_add(table, other, 1, b"SND/B\0".as_ptr()) });
    });
    assert_eq!(r.add(b"SND/A"), 0);
    assert_eq!(
        got.get(),
        1,
        "the other add found the first one's entry taken"
    );
    assert_eq!(r.path(0).as_deref(), Ok(&b"SND/A"[..]));
    assert_eq!(r.path(1).as_deref(), Ok(&b"SND/B"[..]));
    // the same file added by both: one entry
    let r = Rig::new();
    let table = r.table;
    let got = std::rc::Rc::new(core::cell::Cell::new(-99));
    let got2 = got.clone();
    r.core.host.interrupt_at_restore(1, move || {
        got2.set(unsafe { pool_add(table, other, 1, b"SND/A\0".as_ptr()) });
    });
    assert_eq!(r.add(b"SND/A"), 0);
    assert_eq!(got.get(), 0);
    assert_eq!(r.status(1), UNUSED);
}

#[test]
fn a_reset_empties_the_table_in_place() {
    let mut t = PoolTable::new();
    t.backed = true;
    t.open[1] = true;
    t.slots[1][5].status = 3;
    t.slots[1][5].req = Req::Select;
    t.reset(false);
    assert!(!t.backed && !t.open[1]);
    assert!(t.slots[1][5].status == UNUSED && t.slots[1][5].req == Req::None);
    t.reset(true);
    assert!(t.backed);
}
