//! Actual loader/DSP cleanup under host-only transport and pointer side effects.
//! No device ACK/block-tail/callback quiescence claim. All shared runtime state is
//! serialized; the module and its image buffer are blocks of the test arena (`fake_platform::test_arena`).
use crate::boot;
use crate::package::{dsp, entry};
use dkrt_sdk::*;
use crate::platform::st;
use dkrt_abi::dev_addr::DevAddr;
use crate::package::core_api::{CoreApi, REAL_CORE};
use dkrt_abi::link::{Owner, Sequence};
use crate::fake_platform::test_arena::{ScriptedArena, TestArena};
use dkrt_abi::module::Ctx;
use crate::package::handlers::fx_fixture::{frows, reset_rows};
use crate::package::handlers::machines_fixture::vrows;
use std::vec::Vec;

/// The module block and the image buffer that `seed` allocated from the arena (device addresses).
static mut BLOCKS: (u32, u32) = (0, 0);
fn cf() -> u32 {
    unsafe { BLOCKS.0 }
}
fn image() -> u32 {
    unsafe { BLOCKS.1 }
}
/// The test lock with the test arena (`seed` initialises it); dropping it forgets the blocks, takes the arena down and releases the lock.
struct Hold {
    _arena: TestArena,
}
impl Hold {
    fn new() -> Hold {
        Hold { _arena: TestArena::lock() }
    }
}
impl Drop for Hold {
    fn drop(&mut self) {
        unsafe { BLOCKS = (0, 0) }
        crate::package::fake_checked_images::watch(DevAddr::NULL);
    }
}

/// Whether the arena still holds the block at device address `a`.
fn live(a: u32) -> bool {
    unsafe { crate::arena::is_live(DevAddr::new(a)) }
}
const PUB_PC: u32 = 0xc000;
static VOICE_INFO: [u32; 12] = [0x4444_5350, 0, 1, 0x20000000, 4096, 0, 4, 8, 0, 0, 0, 0];
static FX_INFO: [u32; 8] = [0x4446_5849, 0, 1, 0, 0, 4, 0, 24];

#[derive(Clone, Copy)]
enum Completion {
    Ack,
    Refuse,
    ErrorAt(u32),
    Missing,
}
#[derive(Debug, PartialEq)]
enum Event {
    Word(u32, u32, u32),
    Fini,
    Registry,
}
struct Link {
    completion: Completion,
    original_failure: bool,
    tries: u32,
    accepted: u32,
    polls: u32,
    no_fx_info: bool,
    /// The tick callback drop (`dl_tick_cb_drop`) must name the seeded module's range after its `fini`; off for tests of the loader over fakes.
    strict_tick_drop: bool,
    events: Vec<Event>,
}
static mut LINK: Link = Link {
    completion: Completion::Ack,
    original_failure: true,
    tries: 0,
    accepted: 0,
    polls: 0,
    no_fx_info: false,
    strict_tick_drop: true,
    events: Vec::new(),
};

/// A `dyndsp_info` table that replaces `VOICE_INFO` while a test holds the lock (null: none).
pub static mut VOICE_INFO_OVERRIDE: *const u32 = core::ptr::null();

/// Sets `VOICE_INFO_OVERRIDE` for its lifetime and clears it on drop, also when the test panics. Hold it under `LOCK`.
pub(crate) struct VoiceInfoGuard;
impl VoiceInfoGuard {
    pub(crate) unsafe fn set(info: *const u32) -> Self {
        VOICE_INFO_OVERRIDE = info;
        VoiceInfoGuard
    }
}
impl Drop for VoiceInfoGuard {
    fn drop(&mut self) {
        unsafe { VOICE_INFO_OVERRIDE = core::ptr::null() }
    }
}

/// Clears `LINK.no_fx_info` on drop: the flag outlives the test that sets it and made the effect check of a later test refuse with `E_NO_DSP`.
struct NoFxInfoGuard;
impl Drop for NoFxInfoGuard {
    fn drop(&mut self) {
        unsafe { LINK.no_fx_info = false }
    }
}

/// The `dynreg_info` table of an image (the assembly tests read it from the image's info file), while a `DynregInfoGuard` lives; the other host tests have none.
static mut DYNREG_INFO_OVERRIDE: *const u32 = core::ptr::null();

/// Sets the `dynreg_info` table for its lifetime and clears it on drop, also when the test panics. Hold it under `LOCK`; `info` outlives the guard's use.
pub(crate) struct DynregInfoGuard;
impl DynregInfoGuard {
    pub(crate) unsafe fn set(info: *const u32) -> Self {
        DYNREG_INFO_OVERRIDE = info;
        DynregInfoGuard
    }
}
impl Drop for DynregInfoGuard {
    fn drop(&mut self) {
        unsafe { DYNREG_INFO_OVERRIDE = core::ptr::null() }
    }
}

pub fn voice_info() -> *const u32 {
    unsafe {
        if VOICE_INFO_OVERRIDE.is_null() {
            VOICE_INFO.as_ptr()
        } else {
            VOICE_INFO_OVERRIDE
        }
    }
}
pub unsafe fn fx_info() -> *const u32 {
    if LINK.no_fx_info {
        core::ptr::null()
    } else {
        FX_INFO.as_ptr()
    }
}
/// The image's info table `name_hash` of the host tests: `dyndsp_info` and `dynfx_info` are the tables of this file (`voice_info`, `fx_info`); none for any other name.
pub unsafe fn test_info(name_hash: u32) -> Option<*const u32> {
    if name_hash == dkneeds::keys::name_hash("dynreg_info") && !DYNREG_INFO_OVERRIDE.is_null() {
        Some(DYNREG_INFO_OVERRIDE)
    } else if name_hash == dkneeds::keys::name_hash("dyndsp_info") {
        Some(voice_info())
    } else if name_hash == dkneeds::keys::name_hash("dynfx_info") {
        // the table is for the device the test runs on (the service compares the device word with `CoreApi::device_id`)
        if fx_info().is_null() {
            return Some(core::ptr::null());
        }
        static mut TABLE: [u32; 8] = FX_INFO;
        TABLE[2] = REAL_CORE.device_id();
        Some(core::ptr::addr_of!(TABLE) as *const u32)
    } else {
        None
    }
}
pub unsafe fn send_word(win: u32, at: u32, buf: *const u32, len: u32) -> i32 {
    assert_eq!(len, 4);
    LINK.tries += 1;
    if matches!(LINK.completion, Completion::Refuse) {
        return -1;
    }
    LINK.accepted += 1;
    LINK.events.push(Event::Word(win, at, *buf));
    0
}
pub unsafe fn send_image(dst: u32, buf: *const u8, len: u32) -> i32 {
    if IMAGE_OK {
        // the host tests of a service file's DSP part: an area send (the image) or a slot word (window 0) is accepted and logged as (destination, first word)
        LINK.tries += 1;
        LINK.accepted += 1;
        // only a slot word (4 bytes, the link's own buffer) is read; an area image is logged by its destination
        IMAGE_SENDS.push((dst, if len == 4 { core::ptr::read_unaligned(buf as *const u32) } else { 0 }));
        return 0;
    }
    panic!("retained loader must not send another image")
}

static mut IMAGE_OK: bool = false;

static mut IMAGE_SENDS: Vec<(u32, u32)> = Vec::new();

/// The scripted link of a host test of a service file's DSP part (`new`): area and slot sends are accepted and logged, and the transfer is taken (`ack`) or fails (a link error
/// after the first accepted send, the image). It brings up the arena too. Hold the test lock and declare it after the lock guard; dropping it ends the script and takes the arena down.
pub(crate) struct LinkScript {
    _arena: ScriptedArena,
}

impl LinkScript {
    pub(crate) unsafe fn new(ack: bool) -> LinkScript {
        let script = LinkScript { _arena: ScriptedArena::up() };
        script.set(ack);
        script
    }

    /// Start the script over: the link is idle again and answers as `ack` says, the log of sends is empty.
    pub(crate) unsafe fn set(&self, ack: bool) {
        *crate::package::link::lk() = crate::package::link::Link::new(crate::package::link::DspTransport);
        crate::package::link::lk().spin = 20_000;
        LINK = Link {
            completion: if ack { Completion::Ack } else { Completion::ErrorAt(1) },
            original_failure: false,
            tries: 0,
            accepted: 0,
            polls: 0,
            no_fx_info: false,
            strict_tick_drop: false,
            events: Vec::new(),
        };
        IMAGE_SENDS = Vec::new();
        IMAGE_OK = true;
    }
}

impl Drop for LinkScript {
    fn drop(&mut self) {
        unsafe { IMAGE_OK = false }
    }
}

/// The area and slot sends of the script so far: (destination byte address, first word).
pub(crate) unsafe fn image_sends() -> Vec<(u32, u32)> {
    IMAGE_SENDS.clone()
}
pub unsafe fn poll_state() -> (u32, u32) {
    LINK.polls += 1;
    if LINK.original_failure {
        LINK.original_failure = false;
        return (3, 91);
    }
    match LINK.completion {
        Completion::Ack => (2, 0),
        Completion::ErrorAt(n) if LINK.accepted >= n => (3, 92),
        Completion::Missing => (1, 0),
        Completion::Refuse => panic!("refused write must not poll"),
        _ => (2, 0),
    }
}
pub unsafe fn fini(rec: usize, addr: u32) {
    assert_eq!((rec, addr), (0, 1));
    assert!(!entry::has_tick(0), "the tick stops before fini");
    assert_eq!(vrows().row_pub[0], 0);
    assert_eq!(frows().fx_pub[0], 0);
    assert_eq!(dsp::ds().mod_len[0], 256); // invalidated but still owned at fini
    LINK.events.push(Event::Fini);
}
pub unsafe fn registry_remove(owner: u8) {
    assert_eq!(owner, 1);
    assert!(LINK.events.contains(&Event::Fini));
    assert_eq!(dsp::ds().mod_len[0], 256);
    LINK.events.push(Event::Registry);
}

/// One UI pass: the link settles and starts, then the loader steps (the order of `dkrt_tick`).
unsafe fn pass() {
    crate::package::link::pump();
    crate::package::lifecycle::dsp_step();
}

/// No transfer is on the wire and the loader has none in flight.
unsafe fn idle_link() {
    *crate::package::link::lk() = crate::package::link::Link::new(crate::package::link::DspTransport);
    crate::package::link::lk().spin = 20_000;
    dsp::ds().ticket = 0;
}

unsafe fn seed(completion: Completion, fx: bool, published: bool, queued: bool) {
    *st() = core::mem::zeroed();
    crate::arena::init();
    BLOCKS = (crate::arena::alloc(240).get(), crate::arena::alloc(496).get());
    crate::package::fake_checked_images::watch(DevAddr::new(BLOCKS.0));
    *dsp::ds() = core::mem::zeroed();
    reset_rows();
    *boot::info() = core::mem::zeroed();
    crate::package::loader::test_clear_torn();
    dsp_loading();
    dsp::init_once();
    LINK = Link {
        completion,
        original_failure: true,
        tries: 0,
        accepted: 0,
        polls: 0,
        no_fx_info: false,
        strict_tick_drop: true,
        events: Vec::new(),
    };
    let s = st();
    s.modules[0].used = 1;
    s.modules[0].base = cf();
    s.modules[0].size = 256;
    s.modules[0].fini = 1;
    entry::publish_tick(0, 0x9000);
    let d = dsp::ds();
    d.phase = if published {
        dsp::PH_ROWS
    } else {
        dsp::PH_SEND
    };
    // a transfer of the loader is on the wire: the link answers it with the seeded failure at the next pump
    *crate::package::link::lk() = crate::package::link::Link::new(crate::package::link::DspTransport);
    crate::package::link::lk().spin = 20_000;
    d.ticket = crate::package::link::lk().test_in_flight(Owner::Loader).id as u32;
    d.pending.rec = 0;
    d.pending.boot_idx = 0;
    d.pending.send_buf = if published { DevAddr::NULL } else { DevAddr::new(image()) };
    d.row_steps = 0x11223344;
    d.mod_len[0] = 256;
    d.transfers = 1;
    if fx {
        let r = frows();
        r.fx_pc[0] = PUB_PC;
        r.fx_owner[0] = 1;
        r.fx_pub[0] = u8::from(published);
        r.fx_owner[1] = 1; // later row whose transfer fails
        r.mod_fx[0] = 3;
        d.step = 18;
    } else {
        vrows().row_pc[0] = PUB_PC;
        vrows().row_owner[0] = 1;
        vrows().row_pub[0] = u8::from(published);
        vrows().row_owner[1] = 1;
        vrows().mod_rows[0] = 3;
        d.step = 16; // the second voice row, after the latch-mask publication step
    }
    if queued {
        s.modules[1].used = 1;
        d.mod_off[1] = 256;
        d.mod_len[1] = 256;
        d.nwait = 1;
        d.waiting[0].rec = 1;
        d.waiting[0].send_buf = DevAddr::new(0xe000);
    }
    // the modules of the seed are up as the package manager would have left them: a failed DSP part takes them down through it
    crate::package::lifecycle::test_up(2, if queued { &[0, 1][..] } else { &[0][..] });
}

#[test]
fn voice_latch_mask_is_published_before_the_type() {
    let _lock = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, false);
        LINK.original_failure = false;
        let d = dsp::ds();
        idle_link();
        d.step = 13;
        vrows().row_pub[0] = 0;
        vrows().row_latched[0] = 5;
        vrows().row_type[0] = 17;
        while LINK.events.len() < 4 {
            pass();
        }
        let at = 24 * VOICE_INFO[6];
        assert_eq!(LINK.events, [Event::Word(1, at + 22, 5), Event::Word(1, at + 23, 0), Event::Word(1, at + 10, 0), Event::Word(1, at, 17)]);
        assert_eq!(vrows().row_pub[0], 1);
    }
}

unsafe fn assert_retained(fx: bool, pending_buf: u32) {
    let d = dsp::ds();
    assert!(dsp::retained());
    assert!(!entry::has_tick(0), "teardown stops the tick before fini");
    assert_eq!(d.pending.rec, 0);
    assert_eq!(d.pending.send_buf.get(), pending_buf);
    assert_eq!(d.mod_len[0], 256);
    assert_eq!(d.mod_len[1], 256);
    // Publication metadata is inline now, rather than an arena-allocated step list.
    assert_eq!(if fx { frows().fx_pc[0] } else { vrows().row_pc[0] }, PUB_PC);
    assert_eq!(d.nwait, 1);
    assert_eq!(d.waiting[0].send_buf.get(), 0xe000);
    assert_eq!(d.transfers, 1);
    assert_eq!(d.failures, 1);
    assert_eq!(st().last_result, E_DSP_XFER);
    assert_eq!(boot::info().results[0], E_DSP_XFER);
    assert_eq!(st().modules[0].used, 1);
    assert_eq!(st().modules[0].base, cf());
    assert_eq!(dsp::cleanup_test_first_fit(256, 64), Some(512));
    if fx {
        assert_eq!((frows().fx_owner[0], frows().fx_pub[0]), (1, 1));
    } else {
        assert_eq!((vrows().row_owner[0], vrows().row_pub[0]), (1, 1));
    }
    assert!(!LINK
        .events
        .iter()
        .any(|e| matches!(e, Event::Fini | Event::Registry)));
    let frozen = (
        d.row_steps,
        d.ticks,
        d.step,
        LINK.tries,
        LINK.polls,
        LINK.events.len(),
    );
    for _ in 0..3 {
        pass();
    }
    let mut one = Sequence::new();
    one.push(dsp::WIN_FX, 0, 0xabcdef00);
    assert!(!REAL_CORE.link_run(Owner::Core, &one), "a retained link sends nothing");
    assert_eq!(dsp::reserve(2, &dsp::Job::empty()), E_BUSY);
    dsp::cancel(0);
    dsp::release_range(0);
    dsp::launch(d.waiting[0]);
    assert_eq!(
        (
            d.row_steps,
            d.ticks,
            d.step,
            LINK.tries,
            LINK.polls,
            LINK.events.len()
        ),
        frozen
    );
    assert_eq!(d.failures, 1);
    assert_eq!(d.nwait, 1);
    assert_eq!(d.pending.rec, 0);
}

unsafe fn failure_case(completion: Completion) {
    seed(completion, false, true, true);
    pass();
    assert_retained(false, 0);
    assert_eq!(
        dsp::ds().last_dl_err,
        if matches!(completion, Completion::ErrorAt(_)) {
            92
        } else {
            91
        }
    );
}
#[test]
fn dsp_step_reports_a_failure_without_acting_on_it() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, true);
        let d = dsp::ds();
        crate::package::link::pump();
        assert_eq!(dsp::step(), dsp::Advance::Failed(E_DSP_XFER));
        // the module is still up and the DSP loader still counts nothing: the loader decides next
        assert!(entry::has_tick(0));
        assert_eq!((d.failures, st().modules[0].used, boot::info().results[0]), (0, 1, 0));
        dsp::fail_done(false, E_DSP_XFER);
        assert!(dsp::retained());
        assert_eq!((d.failures, st().last_result), (1, E_DSP_XFER));
    }
}
#[test]
fn invalidation_refusal_retains_and_stops() {
    let _g = Hold::new();
    unsafe {
        failure_case(Completion::Refuse);
        assert_eq!(LINK.accepted, 0);
    }
}
#[test]
fn invalidation_error_retains_and_stops() {
    let _g = Hold::new();
    unsafe {
        failure_case(Completion::ErrorAt(1));
        assert_eq!(LINK.accepted, 1);
    }
}
#[test]
fn invalidation_missing_ack_retains_and_stops() {
    let _g = Hold::new();
    unsafe {
        failure_case(Completion::Missing);
        assert_eq!(LINK.accepted, 1);
    }
}
#[test]
fn fx_partial_cleanup_does_not_discard_publication() {
    let _g = Hold::new();
    let _fx = crate::package::handlers::fx_fixture::LoadedFx::new();
    unsafe {
        seed(Completion::ErrorAt(3), true, true, true);
        pass();
        assert_retained(true, 0);
        assert_eq!(LINK.accepted, 3); // no subsequent cleanup writes after failure
    }
}
#[test]
fn missing_fx_info_retains_published_row() {
    let _g = Hold::new();
    let _fx = crate::package::handlers::fx_fixture::LoadedFx::new();
    unsafe {
        seed(Completion::Ack, true, true, true);
        let _reset = NoFxInfoGuard;
        LINK.no_fx_info = true;
        pass();
        assert_retained(true, 0);
        assert_eq!(LINK.accepted, 0);
    }
}
unsafe fn successful_case(fx: bool, published: bool) {
    seed(Completion::Ack, fx, published, true);
    pass();
    assert!(!dsp::retained());
    assert!(!entry::has_tick(0));
    assert_eq!(st().modules[0].used, 0);
    assert_eq!(dsp::ds().mod_len[0], 0);
    assert_eq!((vrows().row_owner[0], frows().fx_owner[0]), (0, 0));
    assert_eq!(dsp::cleanup_test_first_fit(256, 64), Some(0));
    assert!(LINK.events.contains(&Event::Fini));
    assert!(LINK.events.contains(&Event::Registry));
    assert!(!live(cf()));
    assert_eq!(if fx { frows().fx_pc[0] } else { vrows().row_pc[0] }, 0);
    assert_eq!(live(image()), published);
    assert_eq!(dsp::ds().phase, dsp::PH_SEND);
    assert_eq!(dsp::ds().pending.rec, 1);
    assert_eq!(dsp::ds().nwait, 0);
    assert_eq!(dsp::ds().transfers, 2);
    assert_eq!(dsp::ds().failures, 1);
}
#[test]
fn acknowledged_voice_cleanup_reclaims_and_advances_queue() {
    let _g = Hold::new();
    unsafe {
        successful_case(false, true);
        assert_eq!(LINK.accepted, 1);
    }
}
/// A reload gives up a DSP part that does not end (`Rt::cancel_dsp`): the transfer in flight is taken down as a failed one is, the record is free, no tick runs, the
/// queue goes on, and the boot record says the module failed with the code.
#[test]
fn a_cancelled_dsp_part_ends_like_a_failed_one() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, true);
        assert!(crate::package::lifecycle::get().unwrap().cancel_dsp(0, E_DSP_XFER));
        assert_eq!(st().modules[0].used, 0, "the record is free");
        assert!(!entry::has_tick(0));
        assert_eq!(dsp::ds().mod_len[0], 0);
        assert_eq!(dsp::ds().pending.rec, 1, "the queued load of the next module started");
        assert!(!dsp::retained());
        assert_eq!(boot::info().results[0], E_DSP_XFER);
        // nothing is left to cancel: the DSP part of record 0 has ended
        assert!(!crate::package::lifecycle::get().unwrap().cancel_dsp(0, E_DSP_XFER));
    }
}
/// The load that waits in the queue is given up: it is removed and its record taken down, the load in flight goes on, and the DSP is not retained.
#[test]
fn a_cancelled_queued_dsp_part_is_removed_and_the_load_in_flight_goes_on() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, true);
        // the seed's module 0 is the one the test stubs know: let it be the queued one, and module 1 the load in flight
        let d = dsp::ds();
        d.waiting[0].rec = 0;
        d.pending.rec = 1;
        assert!(crate::package::lifecycle::get().unwrap().cancel_dsp(0, E_DSP_XFER));
        assert_eq!((dsp::ds().nwait, dsp::ds().pending.rec), (0, 1), "nothing waits, the load in flight goes on");
        assert_eq!(st().modules[0].used, 0, "the queued module is taken down");
        assert!(st().modules[1].used == 1 && !dsp::retained());
        assert!(!crate::package::lifecycle::get().unwrap().cancel_dsp(0, E_DSP_XFER), "nothing is left to cancel");
    }
}
#[test]
fn acknowledged_fx_cleanup_reclaims_after_all_words() {
    let _g = Hold::new();
    let _fx = crate::package::handlers::fx_fixture::LoadedFx::new();
    unsafe {
        successful_case(true, true);
        assert_eq!(LINK.accepted, 11);
    }
}
#[test]
fn before_publication_failure_is_reclaimable() {
    let _g = Hold::new();
    unsafe {
        successful_case(false, false);
        assert_eq!(LINK.accepted, 0);
    }
}
#[test]
fn never_started_cancel_is_reclaimable() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, false, false);
        dsp::ds().send_buf = DevAddr::new(image());
        dsp::cancel(0);
        assert_eq!(dsp::ds().mod_len[0], 0);
        assert!(!live(image()));
        assert!(live(cf()));
    }
}

#[test]
fn on_loaded_runs_once_per_load_and_after_the_last_row() {
    use crate::package::handlers::tests::{log, two, Call};
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, false);
        idle_link();
        log().clear();
        let d = dsp::ds();
        vrows().row_owner = [0; dsp::MAX_ROWS];
        d.step = 0; // no row of module 0 in the fake's list: every row is in
        crate::package::lifecycle::get().unwrap().dsp_step(two());
        // each service hears it once, in list order, and the load is over
        assert_eq!(*log(), vec![Call::OnLoaded { svc: 1, rec: 0 }, Call::OnLoaded { svc: 2, rec: 0 }]);
        assert_eq!(dsp::ds().phase, dsp::PH_IDLE);
        assert_eq!(boot::info().loaded, 1);
        // an idle loader has nothing to list
        log().clear();
        crate::package::lifecycle::get().unwrap().dsp_step(two());
        assert!(log().is_empty());
    }
}

#[test]
fn attribution_names_the_module_of_every_site_the_watch_reports() {
    use crate::package::handlers::services;
    use dkrt_sdk::fault::{SITE_FX_MASTER, SITE_FX_SEND, SITE_NONE, SITE_VOICE};
    let _g = Hold::new();
    let _fx = crate::package::handlers::fx_fixture::LoadedFx::new();
    unsafe {
        seed(Completion::Ack, false, true, false);
        vrows().row_owner[0] = 2;
        vrows().row_type[0] = 7;
        frows().fx_owner[1] = 3;
        // a voice site: the module that owns the machine type
        assert_eq!(services().attribute(SITE_VOICE, 7), Some(1));
        assert_eq!(services().attribute(SITE_VOICE, 8), None);
        // every other site is an effect row of the chain's table, by the byte offset the DSP reports (`4 * FX_ROW_WORDS` bytes a row; first row 0 in the test table)
        let row_bytes = 4 * dkrt_abi::dsp::FX_ROW_WORDS;
        assert_eq!(services().attribute(SITE_FX_MASTER, row_bytes), Some(2));
        assert_eq!(services().attribute(SITE_FX_SEND, row_bytes), Some(2));
        assert_eq!(services().attribute(SITE_NONE, row_bytes), Some(2));
        assert_eq!(services().attribute(9, row_bytes), Some(2));
        assert_eq!(services().attribute(SITE_FX_MASTER, 0), None);
    }
}

// Optional diagnostic firmware link boundary, absent in this host fixture.
#[no_mangle]
pub extern "C" fn dl_diag(_: *mut u32, _: u32) -> i32 {
    0
}

// core/dsp-link's feed of the stats pages, a no-op in this host fixture.
#[no_mangle]
pub extern "C" fn dl_stats_hook(feed: usize) {
    STATS_HOOK.store(feed, core::sync::atomic::Ordering::SeqCst);
}
/// The feed `dl_tick` would call now (0: none).
pub(crate) static STATS_HOOK: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

// core/sdhc-large, absent in this host fixture.
#[no_mangle]
pub extern "C" fn sdhc_large_range(_: u32, _: u32) {}
#[no_mangle]
pub extern "C" fn sdhc_large_ctl(_: u32, _: u32) -> u32 {
    0
}

// core/card-guard, absent in this host fixture (no words written: version 0).
#[no_mangle]
pub extern "C" fn dkcg_stats(_: *mut u32, _: u32) -> u32 {
    0
}

// The runtime's raw block write (tramp.s): on the device it calls the firmware's write and owns the return label that core/card-guard tells the runtime's write by.
#[no_mangle]
pub extern "C" fn dkrt_raw_block_write(sector: u32, bytes: u32, buf: *const u8) -> i32 {
    block_write(sector, bytes, buf)
}

// The +Drive write adapter: absent in this host fixture (version 0 is refused) until a test installs the fake of `host_io::Pdw`.
#[no_mangle]
pub extern "C" fn dkpw_version() -> u32 {
    crate::host_io::pdw_version()
}
#[no_mangle]
pub extern "C" fn dkpw_begin(path: *const u8, size: u32, hash4: *const u8, flags: u32) -> i32 {
    crate::host_io::pdw_begin(path, size, hash4, flags)
}
#[no_mangle]
pub extern "C" fn dkpw_write(buf: *const u8, len: u32) -> i32 {
    crate::host_io::pdw_write(buf, len)
}
#[no_mangle]
pub extern "C" fn dkpw_close(out: *mut u8) -> i32 {
    crate::host_io::pdw_close(out)
}
#[no_mangle]
pub extern "C" fn dkpw_abort() {
    crate::host_io::pdw_abort()
}
#[no_mangle]
pub extern "C" fn dkpw_delete(path: *const u8) -> i32 {
    crate::host_io::pdw_delete(path)
}
#[no_mangle]
pub extern "C" fn dkpw_recover(out: *mut u32) -> i32 {
    crate::host_io::pdw_recover(out)
}
#[no_mangle]
pub extern "C" fn dkpw_commit() -> i32 {
    crate::host_io::pdw_commit()
}
#[no_mangle]
pub extern "C" fn dkpw_staging() -> *const u8 {
    b"/.dkstage\0".as_ptr()
}

// No firmware file systems or DSP tick callback are installed in this host fixture.
#[no_mangle]
pub static dkrt_device_id: u32 = 0;
/// core/health is not in the host fixture: the runtime's links to it answer nothing (the magic stays 0).
#[no_mangle]
pub extern "C" fn health_read() {}
#[no_mangle]
pub extern "C" fn health_heap() {}
/// The device object of a device with a DSP, a store and an eSDHC (the host fixture's board).
#[no_mangle]
pub static dkrt_dev: dkrt_sdk::DkrtDevice = dkrt_sdk::DkrtDevice {
    magic: dkrt_sdk::DEV_MAGIC,
    version: dkrt_sdk::DEV_VERSION,
    size: core::mem::size_of::<dkrt_sdk::DkrtDevice>() as u32,
    caps: dkrt_sdk::DEV_HAS_DSP | dkrt_sdk::DEV_HAS_STORE | dkrt_sdk::DEV_HAS_SDHC | dkrt_sdk::DEV_HAS_SAFE_KEY,
    console_uart_base: 0xec07_0000,
    ddr_uncached_alias: 0x4800_0000,
    ddr_cached_base: 0x4000_0000,
    ddr_bytes: 0x0800_0000,
    esdhc_base: 0xfc0c_c000,
    safe_key: 19,
    keep_base: 0x47fe_0000,
    keep_bytes: 0x1000,
    dsp_block_cycles: 0xa_2c2b,
    dyn_type_first: 16,
    dyn_types: 8,
    tracks: 16,
    key_codes: [0; dkrt_sdk::DEV_KEYS],
    key_flag_press: 0,
    key_flag_func: 1,
    key_flag_repeat: 3,
    key_flag_release: 4,
};
#[no_mangle]
pub static dkrt_sample_fs: [u32; 4] = [0; 4];
#[no_mangle]
pub static dkrt_files_fs: [u32; 3] = [0; 3];
#[no_mangle]
pub static dkrt_store_first: u32 = 0;
#[no_mangle]
pub static dkrt_store_sectors: u32 = 0;
#[no_mangle]
pub extern "C" fn dl_tick_cb_drop(start: u32, end: u32) {
    unsafe {
        if LINK.strict_tick_drop {
            assert_eq!((start, end), (cf(), cf() + 256));
            assert!(LINK.events.contains(&Event::Fini));
        }
    }
}

/// The loader tests over fakes use other addresses and no seeded link: the tick callback drop is not checked.
pub(crate) fn lenient_tick_drop() {
    unsafe { LINK.strict_tick_drop = false }
}

/// The other firmware anchors that the platform tests keep reachable: nothing in the host tests may write the card or send MIDI through them
/// (the tests of platform behaviour use `FakePlatform`).
#[export_name = "cf.storage.fn.block.write"]
pub extern "C" fn block_write(sector: u32, bytes: u32, buf: *const u8) -> i32 {
    match crate::host_io::card_write(sector, bytes, buf) {
        Some(r) => r,
        None => panic!("host tests must not write firmware storage"),
    }
}
#[export_name = "cf.midi.fn.send"]
pub extern "C" fn midi_send(_: u32, _: *const u8, _: u32) {
    panic!("host tests must not send MIDI through the firmware")
}
/// The image's info table: a function here, not a table with the magic, so every lookup through the real platform is null.
#[no_mangle]
pub extern "C" fn dkrt_info_table() {}

/// The firmware's allocator: the hooks table makes the real package code reachable from every host test, and none of it may allocate firmware memory.
#[export_name = "cf.mem.fn.new"]
pub extern "C" fn mem_new(_: u32) -> *mut u8 {
    panic!("host tests must not allocate firmware memory")
}

#[export_name = "cf.mem.fn.free"]
pub extern "C" fn mem_free(_: *mut u8) {
    panic!("host tests must not free firmware memory")
}

#[export_name = "cf.storage.fn.capacity"]
pub extern "C" fn card_capacity() -> u32 {
    crate::host_io::card_capacity()
}

#[export_name = "cf.storage.fn.block.read"]
pub extern "C" fn block_read(sector: u32, bytes: u32, buf: *mut u8) -> i32 {
    match crate::host_io::card_read(sector, bytes, buf) {
        Some(r) => r,
        None => panic!("host cleanup tests must not read firmware storage"),
    }
}


unsafe extern "C" fn test_sink(_: *const crate::dlog::Ev) {}

/// A module without `fini` that set a sink: the unload releases the channel (it would point into freed memory).
#[test]
fn unload_of_a_module_without_fini_releases_its_sink() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, true);
        crate::dlog::test_reset_sinks(); // a test before may have left channel 0 to another owner (E_BUSY)
        st().modules[0].fini = 0;
        LINK.events.push(Event::Fini); // the registry hook of this harness expects a fini before it; none runs
        let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_INIT);
        assert_eq!(crate::dlog::dk_sink_set(0, test_sink as *const () as usize as *mut u8), OK);
        crate::dlog::leave_module(prev);
        assert!(crate::dlog::sink_on(0));
        pass();
        assert_eq!(st().modules[0].used, 0);
        assert!(!crate::dlog::sink_on(0));
    }
}

static mut OBSERVED: Vec<(u32, u32)> = Vec::new();
/// An unload observer of the tests: the owner and whether the module's record is still in use when it is called.
unsafe extern "C" fn test_observer(owner: u32) {
    let used = st().modules[owner as usize].used as u32;
    (*core::ptr::addr_of_mut!(OBSERVED)).push((owner, used));
}

/// An unload tells the observers which owner is gone, after the module's `fini` and with its record and memory still there; a failed or removed
/// observer is not called.
#[test]
fn unload_of_a_module_tells_the_observers_before_its_memory_is_freed() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, true);
        (*core::ptr::addr_of_mut!(OBSERVED)).clear();
        assert_eq!(crate::api::dkrt_unload_observer_add(test_observer as *const () as usize), OK);
        let removed: usize = 0x1234;
        assert_eq!(crate::api::dkrt_unload_observer_add(removed), OK);
        crate::api::dkrt_unload_observer_remove(removed);
        pass();
        assert_eq!(st().modules[0].used, 0);
        crate::api::dkrt_unload_observer_remove(test_observer as *const () as usize);
        assert_eq!(*core::ptr::addr_of!(OBSERVED), [(0, 1)], "once, for owner 0, while the record was in use");
        assert!(LINK.events.contains(&Event::Fini));
    }
}

/// Module 0 of the boot selection has been planned, started and answered LOADING: its ColdFire part is live and its DSP part is going.
unsafe fn dsp_loading() {
    boot::states().begin();
    assert!(boot::states().planned(0, dkrt_abi::plan::Verdict::Accepted));
    assert!(boot::states().init(0));
    assert!(boot::states().finished(0, LOADING));
    assert_eq!(boot::info().states[0], STATE_DSP_LOADING);
}

#[test]
fn a_failed_dsp_part_makes_its_module_failed_with_the_dsp_code() {
    let _g = Hold::new();
    unsafe {
        seed(Completion::Refuse, false, true, true);
        dsp_loading();
        pass();
        assert_eq!(boot::info().results[0], E_DSP_XFER);
        assert_eq!(boot::info().states[0], STATE_FAILED);
    }
}

#[test]
fn a_loaded_dsp_part_makes_its_module_active() {
    use crate::package::handlers::tests::two;
    let _g = Hold::new();
    unsafe {
        seed(Completion::Ack, false, true, false);
        idle_link();
        let d = dsp::ds();
        vrows().row_owner = [0; dsp::MAX_ROWS];
        d.step = 0;
        dsp_loading();
        crate::package::lifecycle::get().unwrap().dsp_step(two());
        assert_eq!(boot::info().results[0], OK);
        assert_eq!(boot::info().states[0], STATE_ACTIVE);
    }
}
