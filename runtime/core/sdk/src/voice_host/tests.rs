extern crate std;
use super::*;
use crate::{E_CONTRACT, E_FORMAT, E_MISSING, E_NOMEM, LOADING};
use core::ffi::c_void;
use core::mem::{align_of, offset_of, size_of};
use std::boxed::Box;

const P: usize = size_of::<usize>();

fn align_up(n: usize, a: usize) -> usize {
    n.div_ceil(a) * a
}

/// The layout written in `sdk/include/dk/host/voice_host.h`, which `sdk/tests/host/voice_host.c` checks with the same formulas.
#[test]
fn the_layouts_follow_the_c_header() {
    let desc = align_up(P + 4, P);
    assert_eq!(offset_of!(Install, payload_bytes), P);
    assert_eq!(offset_of!(Install, desc), desc);
    assert_eq!(offset_of!(Install, range_min), desc + P);
    assert_eq!(offset_of!(Install, range_max), desc + P + 32);
    assert_eq!(offset_of!(Install, cost), desc + P + 64);
    assert_eq!(size_of::<Install>(), align_up(desc + P + 68, P));
    assert_eq!(size_of::<Stats>(), 24);
    assert_eq!(size_of::<MachineDesc>(), 32);

    use super::table::Table;
    assert_eq!(offset_of!(Table, major), 4);
    assert_eq!(offset_of!(Table, minor), 6);
    assert_eq!(offset_of!(Table, size), 8);
    assert_eq!(offset_of!(Table, tracks), 12);
    assert_eq!(offset_of!(Table, types_max), 14);
    assert_eq!(offset_of!(Table, payload_bytes_max), 16);
    assert_eq!(offset_of!(Table, state_bytes_max), 20);
    assert_eq!(offset_of!(Table, budget), 24);
    assert_eq!(offset_of!(Table, flags), 28);
    assert_eq!(offset_of!(Table, ctx), 32);
    assert_eq!(offset_of!(Table, install), 32 + P);
    assert_eq!(offset_of!(Table, bind), 32 + 2 * P);
    assert_eq!(offset_of!(Table, retire), 32 + 3 * P);
    assert_eq!(offset_of!(Table, stats), 32 + 4 * P);
    assert_eq!(size_of::<Table>(), 32 + 5 * P);
    assert_eq!(align_of::<Table>(), P);
}

/// A host that records what it was asked and answers with fixed statuses.
#[repr(C)]
struct Fake {
    table: super::table::Table,
    status: i32,
}

unsafe extern "C" fn fake_install(ctx: *mut c_void, req: *const Install<'_>, ty: *mut u32) -> i32 {
    let fake = &*(ctx as *const Fake);
    assert_eq!((*req).cost, 7);
    if *ty == 0 {
        *ty = 3; // the host chooses
    }
    fake.status
}
unsafe extern "C" fn fake_bind(ctx: *mut c_void, track: u32, ty: u32) -> i32 {
    assert_eq!((track, ty), (2, 3));
    (*(ctx as *const Fake)).status
}
unsafe extern "C" fn fake_retire(ctx: *mut c_void, _ty: u32) -> i32 {
    (*(ctx as *const Fake)).status
}
unsafe extern "C" fn fake_stats(ctx: *mut c_void, _ty: u32, out: *mut Stats) -> i32 {
    (*out).calls = 9;
    (*(ctx as *const Fake)).status
}

fn fake(status: i32) -> Box<Fake> {
    let mut f = Box::new(Fake {
        table: super::table::Table {
            magic: MAGIC,
            major: MAJOR,
            minor: MINOR,
            size: size_of::<super::table::Table>() as u32,
            tracks: 4,
            types_max: 2,
            payload_bytes_max: 4096,
            state_bytes_max: 256,
            budget: 100,
            flags: FLAG_STEREO,
            ctx: core::ptr::null_mut(),
            install: Some(fake_install),
            bind: Some(fake_bind),
            retire: Some(fake_retire),
            stats: Some(fake_stats),
        },
        status,
    });
    f.table.ctx = (&*f as *const Fake).cast_mut().cast();
    f
}

fn read(f: &Fake) -> Result<VoiceHost, i32> {
    unsafe { VoiceHost::from_info((&f.table as *const super::table::Table).cast()) }
}

#[test]
fn a_good_table_is_accepted_with_its_geometry() {
    let f = fake(0);
    let host = read(&f).unwrap();
    assert_eq!(
        (host.tracks(), host.types_max(), host.budget()),
        (4, 2, 100)
    );
    assert_eq!(
        (host.payload_bytes_max(), host.state_bytes_max()),
        (4096, 256)
    );
    assert!(host.stereo());
}

#[test]
fn a_table_is_refused_when_it_does_not_fit_this_crate() {
    assert_eq!(
        unsafe { VoiceHost::from_info(core::ptr::null()) }.err(),
        Some(E_MISSING)
    );
    let mut f = fake(0);
    f.table.magic = 0;
    assert_eq!(read(&f).err(), Some(E_FORMAT));
    let mut f = fake(0);
    f.table.major = MAJOR + 1;
    assert_eq!(read(&f).err(), Some(E_CONTRACT));
    let mut f = fake(0);
    f.table.size -= 1;
    assert_eq!(read(&f).err(), Some(E_FORMAT));
    let mut f = fake(0);
    f.table.bind = None;
    assert_eq!(read(&f).err(), Some(E_FORMAT));
}

#[test]
fn a_longer_table_of_a_newer_minor_is_accepted() {
    let mut f = fake(0);
    f.table.minor = MINOR + 1;
    f.table.size += 8;
    assert!(read(&f).is_ok());
}

#[test]
fn calls_map_the_status_words() {
    let desc = MachineDesc::new(2, 4, 64, 16, 1, 128);
    let req = Install::new(&[1, 2, 3], &desc, &[(0, 127), (-5, 5)], 7).unwrap();

    let f = fake(0);
    let host = read(&f).unwrap();
    assert_eq!(host.install(&req, 0), Ok(InstallStep::Done(3)));
    assert_eq!(host.bind(2, 3), Ok(Step::Done(())));
    assert_eq!(
        host.stats(3).map(|s| match s {
            Step::Done(s) => s.calls(),
            Step::Loading => 0,
        }),
        Ok(9)
    );

    let f = fake(LOADING);
    let host = read(&f).unwrap();
    assert_eq!(host.install(&req, 0), Ok(InstallStep::Loading(3)));
    assert_eq!(host.retire(3), Ok(Step::Loading));

    let f = fake(E_NOMEM);
    let host = read(&f).unwrap();
    assert_eq!(host.install(&req, 0), Err(E_NOMEM));

    let f = fake(5);
    let host = read(&f).unwrap();
    assert_eq!(host.retire(3), Err(E_FORMAT));
}

#[test]
fn install_passes_the_requested_type_and_returns_it_on_loading() {
    let desc = MachineDesc::new(2, 4, 64, 16, 1, 128);
    let req = Install::new(&[1, 2, 3], &desc, &[], 7).unwrap();
    let f = fake(0);
    let host = read(&f).unwrap();
    assert_eq!(host.install(&req, 5), Ok(InstallStep::Done(5)));
    let f = fake(LOADING);
    let host = read(&f).unwrap();
    let step = host.install(&req, 5).unwrap();
    assert_eq!((step, step.type_id()), (InstallStep::Loading(5), 5));
}

#[test]
fn an_empty_payload_is_null_and_a_continuation_has_none() {
    let desc = MachineDesc::new(0, 1, 0, 0, 0, 0);
    let silent = Install::new(&[], &desc, &[], 1).unwrap();
    assert!(silent.payload.is_null() && silent.payload_bytes == 0);
    let req = Install::new(&[1, 2], &desc, &[], 1).unwrap();
    assert!(!req.payload.is_null());
    let next = req.continuation();
    assert!(next.payload.is_null() && next.payload_bytes == 0 && next.cost == 1);
}

#[test]
fn an_install_takes_at_most_eight_ranges() {
    let desc = MachineDesc::new(0, 1, 0, 0, 0, 0);
    assert!(Install::new(&[], &desc, &[(0, 1); 9], 1).is_none());
}

#[test]
fn the_name_hash_is_the_one_core_api_info_uses() {
    assert_eq!(NAME_HASH, dkneeds::keys::name_hash(NAME));
}
