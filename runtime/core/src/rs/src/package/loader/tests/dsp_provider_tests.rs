//! A service file with a DSP part over fakes (docs/plans/r12-8-sharc-stubs.md 2.5 and 2.6). The ColdFire half is live from the boot hook; the DSP code and then the slots of
//! the file follow over the UI-loop passes; a failure of the DSP part takes the service and its dependents down again through the package manager. The
//! DSP link is scripted (`cleanup_tests::LinkScript`): the area image and the slot words arrive as sends that the script logs.
use super::*;
use crate::package::cleanup_tests as link_script;
use crate::fake_platform::Call;
use dkneeds::keys::{name_hash, Slot, Table};
use dkrt_lifecycle::PackageId;

const SLOT_NAME: &str = "voice_render";

/// A DSP section: an image of `bytes`, no relocation, import, voice or effect (`dkmdsp` layout, version 2, DT2).
fn dsp_part(bytes: u32) -> Vec<u8> {
    let mut s = vec![0u8; 64 + bytes as usize];
    let words = [
        (0, 0x444b_4453),
        (4, 2),
        (8, 1),
        (12, bytes),
        (16, 0),
        (20, 8),
        (24, 64),
        (28, 64),
        (32, 0),
        (36, 64),
        (40, 0),
        (44, 64),
        (48, 0),
        (52, 64),
        (56, 0),
    ];
    for (at, v) in words {
        s[at..at + 4].copy_from_slice(&u32::to_be_bytes(v));
    }
    s
}

/// The service file of `table` with the DSP section `dsp` behind its header table.
fn service_file_dsp(table: &Table, dsp: &[u8]) -> Vec<u8> {
    let mut v = service_file_of(module_file(), table, 0);
    while v.len() % 16 != 0 {
        v.push(0);
    }
    let off = v.len() as u32;
    v.extend_from_slice(dsp);
    put(&mut v, dkneeds::H_DSP_OFF, off);
    put(&mut v, dkneeds::H_DSP_SIZE, dsp.len() as u32);
    seal(&mut v);
    v
}

/// The provider `svc-test` of the other tests, with a DSP image of 16 bytes and a SLOTS item that fills the DSP slot `SLOT_NAME` with the entry 0 of that image.
fn dsp_provider_file() -> Vec<u8> {
    static SLOTS: std::sync::OnceLock<[Slot; 1]> = std::sync::OnceLock::new();
    let slots = SLOTS.get_or_init(|| [Slot { id: name_hash(SLOT_NAME), flags: dkneeds::keys::SLOT_DSP, entry: 0 }]);
    let table = Table { slots, ..svc_table() };
    service_file_dsp(&table, &dsp_part(16))
}

/// A loader whose store holds that provider and the consumer module `consumer` (a hard need of `SVC`), the image has the DSP slot, the link is scripted and the search found the
/// provider.
unsafe fn boot_with_dsp_provider(ack: bool) -> (Fake, link_script::LinkScript) {
    let mut l = staged_boot(dsp_provider_file(), consumer_file(&user_table()));
    l.p.info_dsp.borrow_mut().push((name_hash(SLOT_NAME), 1));
    // the DSP of the test: the link is idle and answers as scripted
    *dsp::ds() = core::mem::zeroed();
    dsp::init_once();
    let script = link_script::LinkScript::new(ack);
    l.find_providers(1);
    assert_eq!(l.found.n, 1, "the search found the provider");
    (l, script)
}

/// UI-loop passes until the DSP loader is idle again (at most 200), through the lifecycle as the device does.
unsafe fn run_passes(rt: &mut crate::package::lifecycle::Rt<FakePlatform, FakeImages, FakeRegistry>) {
    let mut n = 0;
    loop {
        crate::package::link::pump();
        rt.dsp_step(crate::package::handlers::services());
        n += 1;
        if (dsp::state() == STATE_IDLE && dsp::ds().nwait == 0) || n > 200 {
            break;
        }
    }
    assert!(n <= 200, "the DSP loader did not finish");
}

#[test]
fn a_services_dsp_code_is_acknowledged_first_then_its_slots_are_written_in_one_sequence() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, script) = boot_with_dsp_provider(true);
        let mut rt = boot_selection(&mut l, &[PATH]);
        let rec = MAX_MODULES;
        // the ColdFire half is live from the boot hook, the DSP half is under way: the file is `LOADING`, its hooks are in, its slot is claimed and not written
        assert_eq!(records::module(rec).used, 1);
        assert_ne!(site_bytes(&l, 2), STOCK[..2], "the ColdFire hook is live");
        assert!(l.dsp_install.claimed(rec as u8));
        assert!(link_script::image_sends().is_empty(), "nothing is sent in the boot hook");
        run_passes(&mut rt);
        let sends = link_script::image_sends();
        let area = dsp::area_base();
        assert_eq!(sends.len(), 2, "the code, then the slots: {sends:x?}");
        assert_eq!(sends[0].0, area, "the image goes to the first range of the area");
        // the slot write: one send to the word of the slot (index 1 of the block, below the area), with the pc of the entry in the placed image
        assert_eq!(sends[1], (area - 64 + 4, dkrt_abi::dsp::L2_SW_BASE), "slot 1 holds the pc of the DSP entry");
        assert_eq!(dsp::ds().failures, 0);
        drop(script);
    }
}

#[test]
fn a_services_dsp_failure_takes_the_service_and_its_dependents_down_through_the_manager() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, script) = boot_with_dsp_provider(false);
        let mut rt = boot_selection(&mut l, &[PATH]);
        let rec = MAX_MODULES;
        // the dependent loaded against the provider's export, and its own DSP load is queued behind the provider's
        let b = crate::boot::info();
        assert_eq!((b.states[0], b.loaded), (STATE_ACTIVE, 1));
        dsp::ds().waiting[0].rec = 0;
        dsp::ds().nwait = 1;
        run_passes(&mut rt);
        // nothing of the provider is left: hooks stock, slot claim gone, no export, record free; the DSP range is back
        assert_eq!(records::module(rec).used, 0);
        assert_eq!(site_bytes(&l, 6), STOCK, "the ColdFire hooks are back to stock");
        assert_eq!((l.installer.count(), l.dsp_install.claimed(rec as u8)), (0, false));
        assert_eq!(l.exports.resolve(name_hash("dk_svctest_ping")), None);
        assert_eq!(dsp::ds().mod_len[rec], 0);
        assert!(!rt.manager().unwrap().live().contains(PackageId(rec as u8)), "the service is down");
        // the dependent went first, with E_MISSING in its module state; its queued DSP job is gone
        assert_eq!(records::module(0).used, 0);
        assert_eq!((crate::boot::info().states[0], crate::boot::info().results[0], crate::boot::info().loaded), (STATE_FAILED, E_MISSING, 0));
        assert_eq!(dsp::ds().nwait, 0);
        // the one lifecycle: the registry is told in that order (dependent, then the service), fini likewise
        assert_eq!(l.r.log.iter().filter(|e| matches!(e, Ev::Remove(_))).copied().collect::<Vec<_>>(), vec![Ev::Remove(0), Ev::Remove(rec)]);
        let fini = |r: usize| l.m.log.iter().position(|e| matches!(e, Ev::Fini(x, _) if *x == r));
        assert!(fini(0).unwrap() < fini(rec).unwrap(), "dependents first");
        // the slot was never written, so there is nothing to clear; a restore went to the platform under one mask (only a service restores hook sites)
        assert_eq!(link_script::image_sends().len(), 1, "only the image was sent");
        assert!(l.p.calls.borrow().contains(&Call::Mask));
        assert!(!dsp::retained());
        drop(script);
    }
}

#[test]
fn slots_that_were_sent_are_cleared_before_the_code_goes_and_a_clear_that_fails_keeps_the_memory_until_a_retry() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let (mut l, script) = boot_with_dsp_provider(true);
        let mut rt = boot_selection(&mut l, &[PATH]);
        run_passes(&mut rt);
        let rec = MAX_MODULES;
        let area = dsp::area_base();
        assert_eq!(link_script::image_sends().len(), 2);
        // the service goes away (a reload from the menu, or a later failure): the slot is zeroed through the link before anything is freed
        let base = records::module(rec).base;
        rt.manager().unwrap().unload(PackageId(rec as u8)).unwrap();
        let sends = link_script::image_sends();
        assert_eq!(sends.last().copied(), Some((area - 64 + 4, 0)), "the slot word is zero again");
        let free = l.m.log.iter().position(|e| *e == Ev::Free(base)).unwrap();
        assert!(free > l.m.log.iter().position(|e| matches!(e, Ev::Fini(..))).unwrap());
        assert_eq!(site_bytes(&l, 6), STOCK);
        drop(script);
        // a link that does not take the zeros: the take-down stops at the restore, nothing is freed, and a retry with a link that answers finishes it
        let (mut l, script) = boot_with_dsp_provider(true);
        let mut rt = boot_selection(&mut l, &[PATH]);
        run_passes(&mut rt);
        let rec = MAX_MODULES;
        let service = PackageId(rec as u8);
        script.set(false);
        crate::package::files::ready(rec);
        let busy = rt.manager().unwrap().unload(service);
        assert!(matches!(busy, Err(dkrt_lifecycle::LifecycleError::Busy { package, .. }) if package == service), "{busy:?}");
        assert_eq!(rt.manager().unwrap().restore_pending(), Some(service));
        assert_eq!(crate::package::files::ready_state(rec), 2, "the file client is not retired");
        assert_eq!(records::module(rec).used, 1, "the record and its memory are kept");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..)) && *e != Ev::Fini(0, records::module(0).base + 4)), "no fini of the service before the slots are known to be zero");
        script.set(true);
        rt.manager().unwrap().retry_pending().unwrap();
        assert_eq!(records::module(rec).used, 0, "the retry took the service down");
        drop(script);
    }
}

#[test]
fn a_dsp_slot_fill_needs_a_dsp_slot_of_the_image_and_an_entry_inside_the_dsp_image() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the image has no such slot: the file is refused as a whole, nothing is installed
        let (mut l, script) = boot_with_dsp_provider(true);
        l.p.info_dsp.borrow_mut().clear();
        let _rt = boot_selection(&mut l, &[PATH]);
        assert_eq!(records::module(MAX_MODULES).used, 0);
        assert_eq!((l.installer.count(), l.dsp_install.claimed(MAX_MODULES as u8)), (0, false));
        assert_eq!(site_bytes(&l, 6), STOCK);
        drop(script);
    }
}
