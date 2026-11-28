//! `core/input` key subscribers (runtime/platform/input): the key walker's hook and the subscriber table of the committed object on the ColdFire core, for every device whose
//! object list has it (the devices that have the walker).
//!
//! - `input_keys_info` has the words `key_subscribers.h` names, and its entries are the two calls;
//! - the hook with no subscriber jumps to the tail of the site with the stack as the entry had it and D0, D1, A0 and A1 intact (the stock code reads them);
//! - a subscriber is called with the walker's `vc` and `ev`; one that answers nonzero ends the walk: the hook returns 1 to the caller and the tail is never reached;
//! - one that answers 0 lets the event reach the tail, intact again; an unsubscribed one is not called.

mod common;

use coldfire::{Bus, Cpu};
use common::Ram;
use patchkit::devdata;
use patchkit::dko::Dko;
use patchkit::link;
use patchkit::module::Library;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const MODULE: &str = "core/input";

const BASE: u64 = 0x1000;
/// The tail of the site: the test ends a run when the pc gets here (the stock walker would continue).
const TAIL: u32 = 0x7000;
const STACK: u32 = 0xf000;
/// The return address of the caller of the walker.
const RETURN: u32 = 0xf100;
/// A subscriber at `CONSUMER` answers 1 for the arguments `VC` and `EV` and 0 otherwise.
const CONSUMER: u32 = 0x8000;
const VC: u32 = 0xaaaa_5555;
const EV: u32 = 0x5555_aaaa;
/// A subscriber that answers 0 always.
const PASSER: u32 = 0x8040;

/// `move.l 4(sp),d0; cmpi.l #VC,d0; bne fail; move.l 8(sp),d0; cmpi.l #EV,d0; bne fail; moveq #1,d0; rts; fail: moveq #0,d0; rts`.
const CONSUMER_CODE: [u16; 16] = [
    0x202f, 0x0004, 0x0c80, 0xaaaa, 0x5555, 0x6610, 0x202f, 0x0008, 0x0c80, 0x5555, 0xaaaa, 0x6604,
    0x7001, 0x4e75, 0x7000, 0x4e75,
];
/// `moveq #0,d0; rts`.
const PASSER_CODE: [u16; 2] = [0x7000, 0x4e75];

/// Where a run ended.
#[derive(Debug, PartialEq, Eq)]
enum End {
    /// The hook returned to the caller of the walker.
    Returned,
    /// The hook jumped to the tail of the site.
    Tail,
}

struct Rig {
    ram: Ram,
    l: link::Linked,
}

impl Rig {
    /// The rig of the device's key subscriber object, or none when the device has none (no key walker).
    fn new(lib: &Library, dev: &str) -> Option<Rig> {
        let m = lib.get(MODULE).expect("core/input is in the library");
        let (_, dko): (_, &Dko) = m.manifest.object.iter().zip(&m.objects).find(|(o, _)| {
            o.path.contains("key-subscribers") && o.devices.iter().any(|d| d == dev)
        })?;
        let externs: BTreeMap<String, u64> = [("input_key_tail".to_string(), TAIL as u64)].into();
        let l = link::link(dko, BASE, &externs).unwrap();
        let mut ram = Ram(vec![0; 0x1_0000]);
        ram.0[BASE as usize..BASE as usize + l.image.len()].copy_from_slice(&l.image);
        for (at, code) in [(CONSUMER, &CONSUMER_CODE[..]), (PASSER, &PASSER_CODE[..])] {
            for (i, w) in code.iter().enumerate() {
                ram.write16(at + 2 * i as u32, *w).unwrap();
            }
        }
        Some(Rig { ram, l })
    }

    fn sym(&self, name: &str) -> u32 {
        self.l.symbols[name] as u32
    }

    /// Runs `entry` with `args` on the stack (the first at 4(SP)), the registers set to canaries, until the pc is `RETURN` or `TAIL`. The stack is as the entry had it at either end
    /// (`RETURN` popped by an `rts`); D2 .. D7 hold their canaries, and with `live` (the hook) D1, A0 and A1 do too.
    fn run(&mut self, entry: u32, args: &[u32], live: bool) -> (Cpu, End) {
        let mut cpu = Cpu::new();
        cpu.pc = entry;
        cpu.sr = 0x2000;
        let sp = STACK - 4 * args.len() as u32 - 4;
        cpu.a[7] = sp;
        self.ram.write32(sp, RETURN).unwrap();
        for (i, a) in args.iter().enumerate() {
            self.ram.write32(sp + 4 + 4 * i as u32, *a).unwrap();
        }
        for r in 0..8 {
            cpu.d[r] = 0xd0d0_0000 + r as u32;
        }
        for r in 0..7 {
            cpu.a[r] = 0xa0a0_0000 + r as u32;
        }
        let mut steps = 0;
        let end = loop {
            if cpu.pc == RETURN {
                assert_eq!(
                    cpu.a[7],
                    sp + 4,
                    "a return leaves the stack as the caller had it"
                );
                break End::Returned;
            }
            if cpu.pc == TAIL {
                assert_eq!(cpu.a[7], sp, "the tail gets the stack as the entry had it");
                break End::Tail;
            }
            cpu.step(&mut self.ram).unwrap();
            assert_eq!(cpu.last_exception, None, "exception at {:#x}", cpu.pc);
            steps += 1;
            assert!(steps < 2000, "the call does not end");
        };
        // the registers of the compiled-code convention that the caller keeps: D2 .. D7 (the walker's own `movem` saves them)
        for r in 2..8 {
            assert_eq!(cpu.d[r], 0xd0d0_0000 + r as u32, "d{r} is kept");
        }
        if live {
            // the stock walker reads D1, A0 and A1 (and D0) at its entry: the hook gives them back as it found them
            assert_eq!(cpu.d[1], 0xd0d0_0001, "d1 is kept");
            assert_eq!(
                (cpu.a[0], cpu.a[1]),
                (0xa0a0_0000, 0xa0a0_0001),
                "a0 and a1 are kept"
            );
        }
        (cpu, end)
    }

    fn subscribe(&mut self, f: u32) -> i32 {
        let entry = self.ram.read32(self.sym("input_keys_info") + 12).unwrap();
        self.run(entry, &[f], false).0.d[0] as i32
    }

    fn unsubscribe(&mut self, f: u32) -> i32 {
        let entry = self.ram.read32(self.sym("input_keys_info") + 16).unwrap();
        self.run(entry, &[f], false).0.d[0] as i32
    }

    /// The key walker's hook with the event `(vc, ev)`.
    fn key(&mut self, vc: u32, ev: u32) -> (Cpu, End) {
        let hook = self.sym("input_key_walk_hook");
        self.run(hook, &[vc, ev], true)
    }
}

fn rigs() -> Option<Vec<(String, Rig)>> {
    let lib = Library::scan_repo_or_skip(&root())?;
    let rigs: Vec<(String, Rig)> = devdata::all()
        .iter()
        .filter_map(|d| Rig::new(&lib, &d.short).map(|r| (d.short.clone(), r)))
        .collect();
    assert!(!rigs.is_empty(), "a device has the key walker");
    Some(rigs)
}

#[test]
fn the_info_table_states_the_slots_and_the_two_calls() {
    let Some(rigs) = rigs() else {
        return;
    };
    for (dev, mut rig) in rigs {
        let info = rig.sym("input_keys_info");
        let word = |rig: &mut Rig, i: u32| rig.ram.read32(info + 4 * i).unwrap();
        assert_eq!(word(&mut rig, 0), 0x4b45_5953, "{dev}: 'KEYS'");
        assert_eq!(word(&mut rig, 1), 1, "{dev}: version");
        assert_eq!(word(&mut rig, 2), 4, "{dev}: slots");
        assert_eq!(word(&mut rig, 3), rig.sym("key_subscribe"), "{dev}");
        assert_eq!(word(&mut rig, 4), rig.sym("key_unsubscribe"), "{dev}");
    }
}

#[test]
fn the_hook_passes_an_event_with_no_subscriber_to_the_tail_untouched() {
    let Some(rigs) = rigs() else {
        return;
    };
    for (dev, mut rig) in rigs {
        let (cpu, end) = rig.key(VC, EV);
        assert_eq!(end, End::Tail, "{dev}");
        assert_eq!(cpu.d[0], 0xd0d0_0000, "{dev}: d0 is live at the tail");
    }
}

#[test]
fn a_subscriber_gets_the_event_and_the_one_that_consumes_it_ends_the_walk() {
    let Some(rigs) = rigs() else {
        return;
    };
    for (dev, mut rig) in rigs {
        assert_eq!(rig.subscribe(PASSER), 0, "{dev}: slot 0");
        assert_eq!(rig.subscribe(CONSUMER), 1, "{dev}: slot 1");
        assert_eq!(rig.subscribe(CONSUMER), -2, "{dev}: subscribed already");
        // the consumer sees the walker's arguments and answers 1: the hook returns to the caller with d0 = 1
        let (cpu, end) = rig.key(VC, EV);
        assert_eq!((end, cpu.d[0]), (End::Returned, 1), "{dev}");
        // other arguments: it answers 0 and the event reaches the tail
        let (cpu, end) = rig.key(VC, EV + 1);
        assert_eq!(end, End::Tail, "{dev}");
        assert_eq!(cpu.d[0], 0xd0d0_0000, "{dev}: d0 is live at the tail");
        // unsubscribed, it is not called any more
        assert_eq!(rig.unsubscribe(CONSUMER), 0, "{dev}");
        assert_eq!(rig.unsubscribe(CONSUMER), -3, "{dev}: not subscribed");
        assert_eq!(rig.key(VC, EV).1, End::Tail, "{dev}");
    }
}
