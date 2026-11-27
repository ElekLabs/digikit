use super::*;
use dkrt_sdk::voice_host::{VoiceHost, FLAG_STEREO};

unsafe extern "C" fn install(
    _: *mut c_void,
    _: *const dkrt_sdk::voice_host::Install<'_>,
    t: *mut u32,
) -> i32 {
    *t = 9;
    0
}
unsafe extern "C" fn bind(_: *mut c_void, _: u32, _: u32) -> i32 {
    0
}
unsafe extern "C" fn retire(_: *mut c_void, _: u32) -> i32 {
    0
}
unsafe extern "C" fn stats(_: *mut c_void, _: u32, _: *mut dkrt_sdk::voice_host::Stats) -> i32 {
    0
}

fn offer() -> Offer {
    Offer {
        tracks: 8,
        types_max: 16,
        payload_bytes_max: 8192,
        state_bytes_max: 512,
        budget: 1000,
        flags: FLAG_STEREO,
        ctx: core::ptr::null_mut(),
        install: install as *const c_void,
        bind: bind as *const c_void,
        retire: retire as *const c_void,
        stats: stats as *const c_void,
    }
}

/// The words of an info entry: zero, as the image holds them before the host starts.
fn zeroed() -> Box<Words> {
    Box::new(unsafe { core::mem::zeroed() })
}

fn table_of(w: &mut Words) -> VoiceHostTable {
    unsafe { VoiceHostTable::at((w as *mut Words).cast()) }
}

#[test]
fn a_consumer_finds_no_host_until_the_host_serves() {
    let mut w = zeroed();
    let mut t = table_of(&mut w);
    assert!(!t.serving());
    t.prepare();
    assert!(!t.serving());
    let p = (&*w as *const Words).cast::<u8>();
    assert_eq!(
        unsafe { VoiceHost::from_info(p) }.err(),
        Some(dkrt_sdk::E_FORMAT),
        "the magic is 0 while the host only prepared"
    );
    assert_eq!(w.size as usize, core::mem::size_of::<Words>());
}

#[test]
fn the_sdk_accepts_the_table_the_host_serves_with_its_limits() {
    let mut w = zeroed();
    let mut t = table_of(&mut w);
    t.prepare();
    t.serve(&offer());
    assert!(t.serving());
    let host = unsafe { VoiceHost::from_info((&*w as *const Words).cast::<u8>()) }
        .expect("a table the sdk reads");
    assert_eq!(
        (host.tracks(), host.types_max(), host.budget()),
        (8, 16, 1000)
    );
}

#[test]
fn withdrawing_clears_the_magic_before_anything_else() {
    let mut w = zeroed();
    let mut t = table_of(&mut w);
    t.serve(&offer());
    t.withdraw();
    assert!(!t.serving());
    assert_eq!((w.tracks, w.budget), (0, 0));
    assert!(w.install.is_null());
}
