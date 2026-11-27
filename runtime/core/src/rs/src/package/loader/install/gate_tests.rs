//! The gate of the installer: a `task` site is written or restored at run time only while the tasks that can be inside it are parked.
use super::*;
use crate::fake_platform::{Call, FakePlatform};
use dkneeds::keys::{CTX_ABOVE_UI, CTX_UI};
use dkrt_abi::profiles;

const BASE: u32 = 0x4100_0000;
const SIZE: u32 = 0x1000;
const SITE: u32 = 0x1000_0000;
const STOCK: [u8; 6] = [0xaa; 6];
/// The console-like task: its TCB and its wait object, the waiter word at `QUEUE + 0xc`.
const TCB_A: u32 = 0x4039_be58;
const QUEUE: u32 = 0x403a_0eac;
/// The BgWorker-like task: the cell holds the object, the waiter word is at `*CELL + 0x18`.
const TCB_B: u32 = 0x4317_2de4;
const CELL: u32 = 0x44f3_7090;
const OBJECT: u32 = 0x4500_0000;

fn parks() -> [Park; 2] {
    [Park { tcb: TCB_A, wait: QUEUE, indirect: false, off: 0xc, made: 0x14 }, Park { tcb: TCB_B, wait: CELL, indirect: true, off: 0x18, made: 0 }]
}

fn put(f: &FakePlatform, addr: u32, v: u32) {
    f.put_code(addr, &v.to_be_bytes());
}

/// A platform with the site in stock bytes and both tasks parked (each waiter word holds its TCB).
fn fake() -> FakePlatform {
    let f = FakePlatform::new(profiles::ALL[0]);
    f.put_code(SITE, &STOCK);
    put(&f, QUEUE + 0xc, TCB_A);
    put(&f, QUEUE + 0x14, 0x4000_0000);
    put(&f, CELL, OBJECT);
    put(&f, OBJECT + 0x18, TCB_B);
    f
}

fn hook(ctx: u8) -> HookItem {
    HookItem { addr: SITE, len: 6, check: crate::package::loader::port::bytes_check(&STOCK), entry: 0x40, ctx }
}

fn live<'a>(parks: &'a [Park]) -> Gate<'a> {
    Gate { live: true, parks: Some(parks), op: 0 }
}

fn bytes(f: &FakePlatform) -> Vec<u8> {
    let mut out = vec![0u8; 6];
    unsafe { f.read_code(SITE, &mut out) };
    out
}

fn install(ins: &mut Installer, f: &FakePlatform, ctx: u8, gate: &Gate<'_>) -> Result<(), i32> {
    unsafe { ins.install_gated(f, 1, &[hook(ctx)], &[], BASE, SIZE, &[], gate) }
}

const JMP: [u8; 6] = [0x4e, 0xf9, 0x41, 0x00, 0x00, 0x40];

#[test]
fn a_task_site_is_written_when_every_task_is_parked() {
    let f = fake();
    let mut ins = Installer::new();
    let p = parks();
    assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&p)), Ok(()));
    assert_eq!((bytes(&f), ins.count(), f.depth.get()), (JMP.to_vec(), 1, 0));
}

#[test]
fn a_task_site_is_refused_with_busy_while_a_task_runs_and_nothing_changes() {
    let p = parks();
    // each task in turn is not parked: its waiter word does not hold its TCB (it runs, or it was just woken and the post cleared the word)
    for unparked in [QUEUE + 0xc, OBJECT + 0x18] {
        let f = fake();
        put(&f, unparked, 0);
        let mut ins = Installer::new();
        assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&p)), Err(E_BUSY), "word {unparked:#x}");
        assert_eq!((bytes(&f), ins.count()), (STOCK.to_vec(), 0), "no byte changed");
        assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore], "the check ran under the mask, which was given back, and nothing was written");
        assert_eq!(f.depth.get(), 0);
        // the task parks: the next attempt installs
        put(&f, unparked, if unparked == QUEUE + 0xc { TCB_A } else { TCB_B });
        assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&p)), Ok(()));
        assert_eq!(bytes(&f), JMP.to_vec());
    }
}

#[test]
fn a_wait_word_that_holds_another_task_or_a_missing_object_is_not_parked() {
    let p = parks();
    let f = fake();
    put(&f, QUEUE + 0xc, 0x1234_5678);
    assert_eq!(install(&mut Installer::new(), &f, CTX_TASK, &live(&p)), Err(E_BUSY), "another task waits there");
    let f = fake();
    put(&f, CELL, 0);
    assert_eq!(install(&mut Installer::new(), &f, CTX_TASK, &live(&p)), Err(E_BUSY), "the BgWorker object does not exist yet");
}

#[test]
fn ui_and_above_ui_sites_never_wait() {
    let p = parks();
    for ctx in [CTX_UI, CTX_ABOVE_UI] {
        let f = fake();
        put(&f, QUEUE + 0xc, 0);
        put(&f, OBJECT + 0x18, 0);
        let mut ins = Installer::new();
        assert_eq!(install(&mut ins, &f, ctx, &live(&p)), Ok(()), "ctx {ctx}");
        assert_eq!(bytes(&f), JMP.to_vec());
        // and the restore of such a site does not wait either
        assert_eq!(unsafe { ins.restore_gated(&f, 1, &live(&p)) }, Ok(()));
        assert_eq!(bytes(&f), STOCK.to_vec());
    }
}

#[test]
fn the_boot_never_waits() {
    let f = fake();
    put(&f, QUEUE + 0xc, 0);
    put(&f, OBJECT + 0x18, 0);
    let p = parks();
    let mut ins = Installer::new();
    // no task has run at the boot: the words are 0 and the gate is not live
    assert_eq!(install(&mut ins, &f, CTX_TASK, &Gate { live: false, parks: Some(&p), op: 0 }), Ok(()));
    assert_eq!(bytes(&f), JMP.to_vec());
    assert_eq!(unsafe { ins.restore_gated(&f, 1, &Gate::BOOT) }, Ok(()));
    assert_eq!(bytes(&f), STOCK.to_vec());
    // the ungated forms are the boot gate
    assert_eq!(unsafe { ins.install(&f, 1, &[hook(CTX_TASK)], &[], BASE, SIZE, &[]) }, Ok(()));
    unsafe { ins.restore(&f, 1) };
    assert_eq!(bytes(&f), STOCK.to_vec());
}

#[test]
fn a_task_site_is_restored_only_while_the_tasks_are_parked() {
    let f = fake();
    let p = parks();
    let mut ins = Installer::new();
    install(&mut ins, &f, CTX_TASK, &live(&p)).unwrap();
    put(&f, OBJECT + 0x18, 0);
    f.calls.borrow_mut().clear();
    assert_eq!(unsafe { ins.restore_gated(&f, 1, &live(&p)) }, Err(E_BUSY));
    assert_eq!((bytes(&f), ins.count(), f.depth.get()), (JMP.to_vec(), 1, 0), "the hook is still in and still owned");
    assert_eq!(*f.calls.borrow(), [Call::Mask, Call::Restore]);
    // the BgWorker parks: the retry takes the site down
    put(&f, OBJECT + 0x18, TCB_B);
    assert_eq!(unsafe { ins.restore_gated(&f, 1, &live(&p)) }, Ok(()));
    assert_eq!((bytes(&f), ins.count()), (STOCK.to_vec(), 0));
}

#[test]
fn a_patch_of_task_context_waits_too() {
    let f = fake();
    f.put_code(0x2000_0100, &[1, 2, 3, 4]);
    let p = parks();
    let patch = |ctx| PatchItem { addr: 0x2000_0100, len: 4, check: crate::package::loader::port::bytes_check(&[1, 2, 3, 4]), value: 9, ctx };
    let mut ins = Installer::new();
    put(&f, QUEUE + 0xc, 0);
    assert_eq!(unsafe { ins.install_gated(&f, 1, &[], &[patch(CTX_TASK)], BASE, SIZE, &[], &live(&p)) }, Err(E_BUSY));
    assert_eq!(ins.patch_count(), 0);
    assert_eq!(unsafe { ins.install_gated(&f, 1, &[], &[patch(CTX_UI)], BASE, SIZE, &[], &live(&p)) }, Ok(()));
}

#[test]
fn a_port_file_without_the_task_facts_refuses_a_task_site_at_run_time() {
    let f = fake();
    let mut ins = Installer::new();
    let g = Gate { live: true, parks: None, op: 0 };
    assert_eq!(install(&mut ins, &f, CTX_TASK, &g), Err(E_IMPORT));
    assert_eq!(bytes(&f), STOCK.to_vec());
    // a device with no such task has an empty list: nothing to wait for
    assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&[])), Ok(()));
}

#[test]
fn a_task_that_was_never_created_does_not_hold_a_task_site() {
    // the console task exists only on a unit with its serial-console flag: its queue was never built (the word at `made` is 0), so it is parked whatever the waiter word says
    let p = parks();
    let f = fake();
    put(&f, QUEUE + 0xc, 0);
    put(&f, QUEUE + 0x14, 0);
    let mut ins = Installer::new();
    assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&p)), Ok(()));
    // a built queue whose task runs still waits
    let f = fake();
    put(&f, QUEUE + 0xc, 0);
    let mut ins = Installer::new();
    assert_eq!(install(&mut ins, &f, CTX_TASK, &live(&p)), Err(E_BUSY));
}
