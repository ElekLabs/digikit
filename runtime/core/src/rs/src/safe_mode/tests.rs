//! The boot counter, the level of a boot and the stable period, over the real boot (`boot::run` over `FakeBootEnv`) and the real UI-loop pass (`ui_pass_with`) on a fake card.
use super::*;
use crate::boot::{self, info};
use crate::fake_boot_env::FakeBootEnv;
use crate::fake_platform::FakePlatform;
use crate::hooks;
use crate::level::{PackageStatus, CORE_ONLY_AFTER};
use crate::storage::layout::TEST_LAYOUT;
use crate::test_lock::LOCK;
use dkrt_abi::profiles;
use dkrt_sdk::sel::{self, Record};
use dkrt_sdk::store::Layout;

const L: Layout = Layout { first: 1000, sectors: 4096 };
const FOREIGN: [u8; 512] = [0x55; 512];

/// The card gate starts again (a new boot, or a look at the card between boots).
fn new_card_gate() {
    *card::info() = card::CardInfo {
        state: CARD_UNKNOWN,
        first_nonblank: u32::MAX,
        hash: 0,
        refused: 0,
        refused_range: 0,
        read_errors: 0,
        checked: 0,
        claims: 0,
        writes: 0,
        sectors: 0,
    };
    TEST_LAYOUT.with(|c| c.set(Some(L)));
}

/// A new boot: the runtime's RAM (the card gate, the boot record, the stable period's counters) starts again, the card stays.
fn new_boot() {
    new_card_gate();
    unsafe {
        *info() = core::mem::zeroed();
        UI_PASSES = 0;
        DKRT_UI_START.0 = 0;
        LATE_FLOOR = 0;
        KEEP_COUNTER = false;
    }
}

/// How a boot ends.
#[derive(Clone, Copy)]
enum End {
    /// The session stops before the first UI-loop pass.
    Early,
    /// The session reaches the first UI-loop pass and stops there, short of the stable period.
    FirstPass,
    /// The stable period passes.
    Stable,
}

/// What a boot did.
#[derive(Debug, PartialEq, Eq)]
struct Booted {
    safe: bool,
    /// `DkrtBootInfo.reason`: a `SAFE_*` value; `SAFE_NONE` for a normal boot, `SAFE_CORE_ONLY` for a core-only one.
    reason: u32,
    /// `DkrtBootInfo.core_cause`: a `CORE_*` value; `CORE_NONE` unless the boot is core-only.
    cause: u32,
    core_only: bool,
    loaded: bool,
}

/// The stable period passes in a boot that is under way: the UI-loop pass notes the start, the clock moves past the period, the next pass clears the counter.
unsafe fn stable_period(f: &FakePlatform) {
    ui_pass_with(f);
    f.tick(STABLE_BLOCKS);
    ui_pass_with(f);
}

/// One boot with the runtime's own functions (`boot::run`, `ui_pass_with`) over the card `f` and the knobs of `env`.
unsafe fn boot_with(f: &FakePlatform, env: &FakeBootEnv, end: End) -> Booted {
    new_boot();
    boot::run(env);
    match end {
        End::Early => {}
        End::FirstPass => ui_pass_with(f),
        End::Stable => stable_period(f),
    }
    let b = info();
    Booted { safe: b.safe != 0, reason: b.reason, cause: b.core_cause, core_only: b.safe != 0 && b.reason == SAFE_CORE_ONLY, loaded: env.loaded() }
}

/// A boot with nothing special about it.
unsafe fn boot(f: &FakePlatform, end: End) -> Booted {
    boot_with(f, &FakeBootEnv::new(f), end)
}

/// The record the card holds now.
unsafe fn held(f: &FakePlatform) -> Option<Record> {
    new_card_gate();
    match current_with(f) {
        Choice::Use { record, .. } => Some(record),
        _ => None,
    }
}

fn counter(f: &FakePlatform) -> u32 {
    unsafe { held(f).expect("the card holds a record").boot_counter }
}

fn fresh_card() -> FakePlatform {
    FakePlatform::new(profiles::ALL[0])
}

fn written_sectors(f: &FakePlatform) -> Vec<u32> {
    let mut v: Vec<u32> = f.writes.borrow().iter().map(|w| w.0).collect();
    v.sort();
    v.dedup();
    v
}

unsafe fn selection_write(f: &FakePlatform, r: Record) -> i32 {
    match current_with(f) {
        Choice::Use { copy, .. } => update_with(f, r, copy, true),
        _ => update_with(f, r, 0, false),
    }
}

#[test]
fn a_fresh_device_that_does_not_reach_the_stable_period_boots_safe_on_the_third_boot() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(held(&f).is_none(), "a fresh card has no record");
        assert!(!boot(&f, End::Early).safe);
        let r = held(&f).expect("the first counted boot created the record");
        assert_eq!((r.seq, r.boot_counter, r.count), (1, 1, 0));
        assert_eq!(written_sectors(&f), [L.sel_a()], "only a selection copy was written");
        assert!(!boot(&f, End::Early).safe);
        assert_eq!(counter(&f), 2);
        let third = boot(&f, End::FirstPass);
        assert_eq!((third.safe, third.reason, third.loaded), (true, SAFE_COUNTER, false), "two bad boots: the third is safe");
        assert_eq!(counter(&f), 3, "a safe boot the counter caused counts itself, and its first UI pass does not clear the counter");
        stable_period(&f);
        assert_eq!(counter(&f), 0, "the stable period of the safe boot clears it");
        assert!(!boot(&f, End::Stable).safe, "the boot after it tries the selection again");
    }
}

#[test]
fn a_stable_session_clears_the_counter_so_later_failures_count_from_zero() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        let seen: Vec<bool> = [End::Early, End::Stable, End::Early, End::Early, End::Early].into_iter().map(|end| boot(&f, end).safe).collect();
        assert_eq!(seen, [false, false, false, false, true]);
    }
}

#[test]
fn a_crash_after_the_stable_period_counts_once_and_two_in_a_row_lead_to_safe_mode() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Stable).safe, "a stable first session");
        assert_eq!(counter(&f), 0);
        // the session crashed after its stable period: the next boot counts it (1) and itself (2), and its own stable period leaves 1
        let env = FakeBootEnv::new(&f);
        env.late_crash.set(true);
        assert!(!boot_with(&f, &env, End::Stable).safe);
        assert_eq!(counter(&f), 1);
        assert!(boot_with(&f, &env, End::Stable).safe, "a second such crash: the boot is safe");
    }
}

#[test]
fn a_card_the_runtime_may_not_write_counts_nothing_and_never_loops_into_safe_mode() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let foreign = fresh_card();
        foreign.put(L.first + 100, FOREIGN);
        let small = fresh_card();
        small.card_sectors.set(L.first + 10); // the card ends inside the store area: no store
        for (name, f) in [("foreign bytes in the area", &foreign), ("a card too small for the store", &small)] {
            for n in 0..6 {
                assert!(!boot(f, End::Early).safe, "{name}: boot {n}");
            }
            assert!(f.writes.borrow().is_empty(), "{name}: nothing was written");
            assert!(held(f).is_none(), "{name}: no record");
        }
    }
}

#[test]
fn an_existing_selection_keeps_its_paths_and_counts_into_the_copy_that_is_not_current() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        let mut r = Record::empty();
        r.seq = 5;
        r.count = 1;
        r.paths[0][..5].copy_from_slice(b"a.dkm");
        let mut b = [0u8; 512];
        sel::encode(&r, &mut b);
        f.put(L.sel_a(), b);
        assert!(!boot(&f, End::Early).safe);
        let now = held(&f).unwrap();
        assert_eq!((now.seq, now.boot_counter, now.count), (6, 1, 1));
        assert_eq!(&now.paths[0][..5], b"a.dkm");
        assert_eq!(written_sectors(&f), [L.sel_b()], "copy A stays valid while B is written");
    }
}

#[test]
fn a_record_that_could_not_be_read_keeps_its_counter_at_the_stable_period() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Early).safe);
        new_boot();
        clear_counter_with(&f, 0, true);
        assert_eq!(counter(&f), 1);
        // a boot that cannot read the record is safe, writes nothing and stays safe at the stable period
        let writes = f.writes.borrow().len();
        f.bad_read.borrow_mut().extend([L.sel_a(), L.sel_b()]);
        let b = boot(&f, End::Stable);
        assert_eq!((b.safe, b.reason, b.loaded), (true, SAFE_READ, false));
        assert_eq!(f.writes.borrow().len(), writes);
    }
}

#[test]
fn a_key_or_flag_safe_boot_does_not_count_itself_and_the_stable_period_clears_the_counter() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Early).safe);
        let seq = held(&f).unwrap().seq;
        // the key
        f.key_held.set(true);
        let b = boot(&f, End::FirstPass);
        assert_eq!((b.safe, b.reason, b.loaded), (true, SAFE_KEY, false));
        let r = held(&f).unwrap();
        assert_eq!((r.boot_counter, r.seq), (1, seq), "the key boot wrote nothing: the counter is the one the failed boot left");
        stable_period(&f);
        assert_eq!(counter(&f), 0, "only the stable period of the safe boot clears it");
        f.key_held.set(false);
        // the one-shot flag: honoured once, cleared in the record, not counted
        let mut r = held(&f).unwrap();
        r.flags |= sel::FLAG_SAFE_ONCE;
        r.boot_counter = 1;
        assert_eq!(selection_write(&f, r), OK);
        let b = boot(&f, End::FirstPass);
        assert_eq!((b.safe, b.reason), (true, SAFE_FLAG));
        let r = held(&f).unwrap();
        assert_eq!((r.flags & sel::FLAG_SAFE_ONCE, r.boot_counter), (0, 1));
        assert!(!boot(&f, End::Early).safe, "the flag was honoured once");
    }
}

#[test]
fn a_dsp_hang_or_a_panic_gives_one_safe_boot_that_does_not_count() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Stable).safe);
        let seq = held(&f).unwrap().seq;
        for (hang, want) in [(true, SAFE_DSP), (false, SAFE_PANIC)] {
            let env = FakeBootEnv::new(&f);
            env.hang.set(hang);
            env.panic.set(!hang);
            let b = boot_with(&f, &env, End::FirstPass);
            assert_eq!((b.safe, b.reason), (true, want));
            let r = held(&f).unwrap();
            assert_eq!((r.boot_counter, r.seq), (0, seq), "nothing was counted or written");
        }
    }
}

#[test]
fn crashes_in_safe_mode_lead_to_core_only_boots_that_keep_the_counter_until_it_is_cleared() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Early).safe);
        assert!(!boot(&f, End::Early).safe);
        // safe boots that crash count themselves: the counter reaches CORE_ONLY_AFTER
        for want in [3, 4] {
            let b = boot(&f, End::Early);
            assert_eq!((b.safe, b.reason, b.core_only), (true, SAFE_COUNTER, false));
            assert_eq!(counter(&f), want);
        }
        assert_eq!(counter(&f), CORE_ONLY_AFTER);
        // from here on every boot is core-only, whatever the key says, even after a stable period; none counts and none writes a selection sector
        f.key_held.set(true);
        let writes = f.writes.borrow().len();
        for n in 0..3 {
            let b = hooks::without_package(|| boot(&f, End::Stable));
            assert_eq!((b.safe, b.core_only, b.loaded), (true, true, false), "boot {n}");
            assert_eq!(counter(&f), CORE_ONLY_AFTER, "boot {n}: the counter stays put, the stable period leaves it");
        }
        assert_eq!(f.writes.borrow().len(), writes, "core-only boots write nothing");
        f.key_held.set(false);
        // the package file written, or RETRY, clears the counter: the next boot runs the package
        new_boot();
        package_written_with(&f);
        let b = boot(&f, End::Stable);
        assert_eq!((b.safe, b.loaded), (false, true));
        assert_eq!(counter(&f), 0);
    }
}

#[test]
fn a_core_only_boot_attaches_no_package_and_runs_none_of_its_code() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        hooks::without_package(|| {
            let env = FakeBootEnv::new(&f);
            env.package.set(PackageStatus::Missing);
            let b = boot_with(&f, &env, End::FirstPass);
            assert_eq!((b.safe, b.core_only, b.reason, b.cause), (true, true, SAFE_CORE_ONLY, CORE_MISSING));
            assert!(!hooks::installed(), "no package hook is installed after a core-only boot");
            assert!(env.steps.borrow().contains(&"stage1") && !env.steps.borrow().contains(&"package"), "stage 1 looked, the package was not started");
            assert!(!env.steps.borrow().contains(&"safe"), "the package was not told to go safe");
            // the same core boots normally the next time: the boot attaches the package
            let b = boot(&f, End::Early);
            assert!(b.loaded);
            assert!(hooks::installed());
        });
    }
}

#[test]
fn retry_after_a_core_only_boot_gives_a_normal_boot_and_the_safe_flag_does_not_survive_it() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Early).safe);
        let mut r = held(&f).unwrap();
        r.boot_counter = CORE_ONLY_AFTER;
        r.flags |= sel::FLAG_SAFE_ONCE;
        assert_eq!(selection_write(&f, r), OK);
        let b = hooks::without_package(|| boot(&f, End::Early));
        assert_eq!((b.safe, b.core_only), (true, true));
        let r = held(&f).unwrap();
        assert_eq!((r.flags & sel::FLAG_SAFE_ONCE, r.boot_counter), (0, CORE_ONLY_AFTER), "core-only used the flag up and did not count");
        new_boot();
        // RETRY's card step (`cmd_retry` calls it on the real platform): the next boot is a plain normal one
        package_written_with(&f);
        let b = boot(&f, End::Early);
        assert_eq!((b.safe, b.loaded), (false, true), "no surprise safe boot");
    }
}

#[test]
fn a_package_that_cannot_run_makes_a_core_only_boot_that_is_not_a_crash() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for status in [PackageStatus::Missing, PackageStatus::Incompatible, PackageStatus::Damaged, PackageStatus::Unreadable] {
            let f = fresh_card();
            assert!(!boot(&f, End::Early).safe);
            let seq = held(&f).unwrap().seq;
            let env = FakeBootEnv::new(&f);
            env.package.set(status);
            let b = hooks::without_package(|| {
                let b = boot_with(&f, &env, End::FirstPass);
                assert!(!hooks::installed(), "{status:?}: no package hook is installed");
                b
            });
            assert_eq!((b.safe, b.core_only, b.loaded), (true, true, false), "{status:?}");
            let r = held(&f).unwrap();
            assert_eq!((r.boot_counter, r.seq), (1, seq), "{status:?}: the boot was not counted");
            stable_period(&f);
            assert_eq!(counter(&f), 0, "{status:?}: a card state is no crash: the stable period clears the counter as in any boot");
            assert!(boot(&f, End::Early).loaded, "{status:?}: the next boot with a good package runs it");
        }
    }
}

#[test]
fn writing_the_package_after_a_crash_of_a_stable_session_leaves_no_bad_boot_counted() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        assert!(!boot(&f, End::Stable).safe);
        let env = FakeBootEnv::new(&f);
        env.late_crash.set(true);
        boot_with(&f, &env, End::FirstPass);
        assert_eq!(counter(&f), 2, "the crash and this boot are counted");
        // the package file is written while this boot runs: a new package starts clean, and the stable period does not bring the old crash back
        package_written_with(&f);
        assert_eq!(counter(&f), 0);
        stable_period(&f);
        assert_eq!(counter(&f), 0);
    }
}

#[test]
fn a_device_object_the_runtime_cannot_read_takes_no_boot_and_attaches_no_package() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        new_boot();
        hooks::without_package(|| {
            let env = FakeBootEnv::new(&f);
            boot::enter(false, &env);
            assert_ne!(info().magic, boot::INFO_MAGIC, "the boot did not run");
            assert!(!hooks::installed(), "no package attached");
            assert!(env.steps.borrow().is_empty(), "no step ran");
            boot::enter(true, &env);
            assert!(hooks::installed(), "a valid device boots and attaches");
        });
    }
}

#[test]
fn stage_1_runs_before_the_decision_and_the_package_starts_only_in_a_boot_that_runs_it() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        let env = FakeBootEnv::new(&f);
        let b = boot_with(&f, &env, End::Early);
        assert_eq!((b.safe, b.reason, b.cause), (false, SAFE_NONE, CORE_NONE));
        let steps = env.steps.borrow().clone();
        let at = |name: &str| steps.iter().position(|s| *s == name).unwrap_or_else(|| panic!("no step {name}: {steps:?}"));
        assert!(at("stage1") < at("package") && at("package") < at("tables") && at("tables") < at("load"), "{steps:?}");
        // a safe boot (a DSP hang before) starts the package for its menu; it loads no module
        let env = FakeBootEnv::new(&f);
        env.hang.set(true);
        let b = boot_with(&f, &env, End::Early);
        assert_eq!((b.safe, b.reason, b.core_only), (true, SAFE_DSP, false));
        assert!(env.steps.borrow().contains(&"package") && !env.steps.borrow().contains(&"load"));
    }
}

#[test]
fn every_way_a_package_is_refused_is_a_core_only_boot_with_its_own_cause() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for (status, cause) in [
            (PackageStatus::Missing, CORE_MISSING),
            (PackageStatus::Incompatible, CORE_INCOMPATIBLE),
            (PackageStatus::Damaged, CORE_DAMAGED),
            (PackageStatus::Unreadable, CORE_UNREADABLE),
        ] {
            let f = fresh_card();
            hooks::without_package(|| {
                let env = FakeBootEnv::new(&f);
                env.package.set(status);
                let b = boot_with(&f, &env, End::Early);
                assert_eq!((b.safe, b.core_only, b.reason, b.cause, b.loaded), (true, true, SAFE_CORE_ONLY, cause, false), "{status:?}");
            });
        }
    }
}

/// A fault or a hang in the package's entry never returns, so the boot is counted before the package starts: the record holds the count when the entry runs, boot after boot
/// until the counter reaches `CORE_ONLY_AFTER` and the boot runs no package code at all.
#[test]
fn a_fault_in_the_package_entry_counts_toward_safe_mode_and_core_only() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        let mut seen_at_start = Vec::new();
        for _ in 0..CORE_ONLY_AFTER {
            let env = FakeBootEnv::new(&f);
            boot_with(&f, &env, End::Early);
            assert!(env.steps.borrow().contains(&"package"), "the package starts while the counter is below the limit");
            seen_at_start.push(env.counter_at_start.get());
        }
        // what the card held at the moment the package's entry ran (a reset there leaves exactly this): this boot already counted
        assert_eq!(seen_at_start, [Some(1), Some(2), Some(3), Some(4)]);
        let env = FakeBootEnv::new(&f);
        let b = hooks::without_package(|| boot_with(&f, &env, End::Early));
        assert_eq!((b.safe, b.core_only, b.cause), (true, true, CORE_CRASHES));
        assert!(!env.steps.borrow().contains(&"package"), "no package code runs once the boots are counted out");
    }
}

/// A package that was placed and accepted but gives no hooks cannot run on this core: the boot is core-only with `CORE_INCOMPATIBLE` (nothing crashed, so the counter
/// is cleared by the stable period as for any package refusal), the block goes back and the host directory is empty.
#[test]
fn a_package_that_gives_no_hooks_makes_a_core_only_boot_that_is_incompatible() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        hooks::without_package(|| {
            let env = FakeBootEnv::new(&f);
            env.start_fails.set(true);
            let b = boot_with(&f, &env, End::Stable);
            assert_eq!((b.safe, b.core_only, b.reason, b.cause, b.loaded), (true, true, SAFE_CORE_ONLY, CORE_INCOMPATIBLE, false));
            let steps = env.steps.borrow().clone();
            assert!(steps.contains(&"package") && steps.contains(&"abandon") && !steps.contains(&"load"), "{steps:?}");
            assert!(!hooks::installed(), "no hooks were installed");
            assert_eq!(counter(&f), 0, "the stable period of a core-only boot that no crash caused clears the count");
            assert!((0..dkrt_sdk::HOST_COUNT as u32).all(|id| (*core::ptr::addr_of!(crate::host_dir::DKRT_HOST_DIR)).get(id).is_none()));
        });
    }
}

/// Core-only by crashes gives the placed package's block back, and a core-only boot leaves the host directory empty.
#[test]
fn a_core_only_boot_by_crashes_abandons_the_placed_package_and_leaves_the_host_directory_empty() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        for _ in 0..CORE_ONLY_AFTER {
            boot(&f, End::Early);
        }
        // the previous boots filled the directory (the package of this binary publishes at its entry)
        assert!((*core::ptr::addr_of!(crate::host_dir::DKRT_HOST_DIR)).get(dkrt_sdk::HOST_DSP).is_some());
        let env = FakeBootEnv::new(&f);
        let b = hooks::without_package(|| boot_with(&f, &env, End::Early));
        assert_eq!((b.core_only, b.cause), (true, CORE_CRASHES));
        assert!(env.steps.borrow().contains(&"abandon"));
        assert!((0..dkrt_sdk::HOST_COUNT as u32).all(|id| (*core::ptr::addr_of!(crate::host_dir::DKRT_HOST_DIR)).get(id).is_none()), "no package, no entries");
    }
}

fn dir() -> dkrt_abi::host_dir::HostDir {
    unsafe { *core::ptr::addr_of!(crate::host_dir::DKRT_HOST_DIR) }
}

/// The core fills its own entries before the package starts and marks the directory ready only after the package has attached: a tool that reads while the package starts finds the
/// directory not ready. The package of this binary fills its entries at its attach, so after the boot every id is filled.
#[test]
fn the_host_directory_is_ready_only_after_the_package_attached_and_the_core_filled_its_ids_before() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        let env = FakeBootEnv::new(&f);
        let b = boot_with(&f, &env, End::Early);
        assert!(!b.core_only);
        assert_eq!(
            env.dir_at_start.get(),
            Some((false, dkrt_sdk::HOST_CORE_OWNED.len())),
            "while the package starts: not ready, the core's ids already filled"
        );
        let d = dir();
        assert!(d.is_ready(), "ready once the package attached");
        assert_eq!(d.get(dkrt_sdk::HOST_PACKAGE_LO), Some(0x4010_0000));
        assert_eq!(d.get(dkrt_sdk::HOST_PACKAGE_HI), Some(0x4011_0000));
        for id in 0..dkrt_sdk::HOST_COUNT as u32 {
            assert!(d.get(id).is_some(), "id {id} is filled in a normal boot");
        }
    }
}

/// A placed package whose block the core cannot describe (no extent) does not run: core-only, incompatible, its start never called, the directory empty and not ready.
#[test]
fn a_package_without_a_block_extent_is_a_start_failure_and_the_directory_stays_not_ready() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        hooks::without_package(|| {
            let env = FakeBootEnv::new(&f);
            env.extent.set(None);
            let b = boot_with(&f, &env, End::Early);
            assert_eq!((b.core_only, b.cause, b.loaded), (true, CORE_INCOMPATIBLE, false));
            let steps = env.steps.borrow().clone();
            assert!(!steps.contains(&"package") && steps.contains(&"abandon"), "{steps:?}");
            assert!(!hooks::installed());
            let d = dir();
            assert!(!d.is_ready() && (0..dkrt_sdk::HOST_COUNT as u32).all(|id| d.get(id).is_none()));
        });
    }
}

/// A start failure leaves the directory empty and not ready, even though the core filled its ids before the start.
#[test]
fn a_start_failure_clears_what_the_core_filled_before_it() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let f = fresh_card();
        hooks::without_package(|| {
            let env = FakeBootEnv::new(&f);
            env.start_fails.set(true);
            boot_with(&f, &env, End::Early);
            assert_eq!(env.dir_at_start.get(), Some((false, dkrt_sdk::HOST_CORE_OWNED.len())));
            let d = dir();
            assert!(!d.is_ready() && (0..dkrt_sdk::HOST_COUNT as u32).all(|id| d.get(id).is_none()));
        });
    }
}

/// The counter of a package that fails to start, over the real boot. The boot is counted before the package's entry runs (a fault there must count), and a failure that
/// returns is no crash: the stable period of its core-only boot clears the count, as for a package refused on the card. A boot that ends early leaves the count, so a package that
/// fails and then crashes the boot works its way up the same ladder as any crash: safe boots, then core-only by crashes, which keeps the counter until the package is written or RETRY runs.
#[test]
fn a_start_failure_counts_its_boot_until_the_stable_period_and_retry_clears_the_ladder() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let failing = |f: &FakePlatform, end: End| {
            hooks::without_package(|| {
                let env = FakeBootEnv::new(f);
                env.start_fails.set(true);
                let b = boot_with(f, &env, end);
                let started = env.steps.borrow().contains(&"package");
                (b, started)
            })
        };
        // a failure whose boot reaches the stable period: counted during the boot, cleared by the period; the next boot with a good package is normal
        let f = fresh_card();
        let (b, started) = failing(&f, End::FirstPass);
        assert_eq!((b.core_only, b.cause, started), (true, CORE_INCOMPATIBLE, true));
        assert_eq!(counter(&f), 1, "counted before the package started");
        stable_period(&f);
        assert_eq!(counter(&f), 0, "no crash: the stable period of the core-only boot clears the count");
        let b = boot(&f, End::Stable);
        assert_eq!((b.safe, b.loaded), (false, true));
        assert_eq!(counter(&f), 0);

        // failures whose boots end early (a crash behind each): the counter climbs, safe boots count themselves, and at CORE_ONLY_AFTER the package is not started any more
        let f = fresh_card();
        for want in 1..=CORE_ONLY_AFTER {
            let (b, started) = failing(&f, End::Early);
            assert_eq!((b.core_only, b.cause, started), (true, CORE_INCOMPATIBLE, true), "boot {want}");
            assert_eq!(counter(&f), want, "boot {want}: the boot counts itself, whatever level the failure then gives");
        }
        let (b, started) = failing(&f, End::Early);
        assert_eq!((b.core_only, b.cause, started), (true, CORE_CRASHES, false), "from the fourth count the package is not tried");
        assert_eq!(counter(&f), CORE_ONLY_AFTER, "core-only by crashes keeps the counter");
        // RETRY (or writing the package) clears it: the next boot runs the package, normal and counted from zero
        new_boot();
        package_written_with(&f);
        assert_eq!(counter(&f), 0);
        let b = boot(&f, End::Stable);
        assert_eq!((b.safe, b.loaded), (false, true));
        assert_eq!(counter(&f), 0);
    }
}

