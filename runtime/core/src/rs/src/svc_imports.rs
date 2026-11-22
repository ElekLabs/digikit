//! The second import table, `SVC_IMPORTS`: what a service file (`.dks`, role 1) imports besides `IMPORTS`. It is interface C as C-shaped functions
//! (`dkrt_abi::core_imports`: one `dkc_*` per `CoreApi` method) and the two imports only a service file makes (`dk_service_declare_row`, `dkc_panic`). The
//! list is one macro in `dkrt-abi`; `dkrt-svc-rt` declares the same names and implements `CoreApi` over them (`ImportCore`).
//!
//! The table keeps the hash of each name, not the name (8 bytes an entry).
//!
//! `shims` has one function of the declared type for each name; a const check per entry fails the build when a type differs from the list. A shim calls the
//! `RealCore` method (or the runtime function) and converts the C shape back. The loader looks the table up for role 1 only, after `IMPORTS`.
//! In host tests the shims are exported under their names (`no_mangle`), so a test can link `ImportCore` to them.
use crate::core_api::{CoreApi, REAL_CORE};
use dkrt_abi::dev_addr::DevAddr;

/// The shims, one per entry of `dkrt_abi::core_imports!`.
pub mod shims {
    use super::*;
    use dkrt_abi::dsp::DspState;
    use dkrt_abi::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
    use dkrt_abi::service::ServiceRow;
    use dkrt_sdk::api::{JobDoneFn, JobStepFn};

    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_boot_index() -> u32 {
        REAL_CORE.boot_index()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_keys_of(at: u32, kind: u32, out: *mut u32) -> u32 {
        REAL_CORE.keys_of(at, kind, &mut *(out as *mut [u32; 8])) as u32
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_legacy_state(out: *mut u8) {
        let b = REAL_CORE.legacy_state();
        let mut i = 0;
        while i < b.len() {
            *out.add(i) = b[i];
            i += 1;
        }
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_legacy_state_clear() -> i32 {
        REAL_CORE.legacy_state_clear()
    }
    #[cfg_attr(test, no_mangle)]
    #[cfg_attr(test, no_mangle)]
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_live_op() -> u32 {
        REAL_CORE.live_op()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_device_id() -> u32 {
        REAL_CORE.device_id()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_module_name(owner: u8) -> *const u8 {
        REAL_CORE.module_name(owner)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_module_of(ctx: *const u8) -> u8 {
        REAL_CORE.module_of(ctx)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_storage_stat(root: u32, path: *const u8, len: u32, out: *mut dkrt_abi::storage::StatWire) -> i32 {
        match REAL_CORE.storage_stat(root, core::slice::from_raw_parts(path, len as usize)) {
            Ok(s) => {
                *out = dkrt_abi::storage::StatWire::of(&s);
                dkrt_sdk::OK
            }
            Err(e) => e,
        }
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_storage_stat_head(root: u32, path: *const u8, len: u32, out: *mut dkrt_abi::storage::StatWire, head: *mut u8) -> i32 {
        match REAL_CORE.storage_stat_head(root, core::slice::from_raw_parts(path, len as usize), &mut *(head as *mut [u8; 64])) {
            Ok((s, n)) => {
                *out = dkrt_abi::storage::StatWire::of(&s);
                n as i32
            }
            Err(e) => e,
        }
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_irq_mask() -> u32 {
        REAL_CORE.irq_mask()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_irq_restore(sr: u32) {
        REAL_CORE.irq_restore(sr)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_settings_get(key: *const u8, key_len: u32, out: *mut u8, out_len: u32) -> i32 {
        REAL_CORE.settings_get(core::slice::from_raw_parts(key, key_len as usize), core::slice::from_raw_parts_mut(out, out_len as usize))
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_settings_put(key: *const u8, key_len: u32, value: *const u8, value_len: u32) -> i32 {
        REAL_CORE.settings_put(core::slice::from_raw_parts(key, key_len as usize), core::slice::from_raw_parts(value, value_len as usize))
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_log(kind: u16, flags: u16, args: *const u32) {
        let a = core::slice::from_raw_parts(args, 5);
        REAL_CORE.log(kind, flags, [a[0], a[1], a[2], a[3], a[4]])
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_settings_overlay_set(overlay: *const dkrt_abi::service::SettingsOverlay) {
        REAL_CORE.settings_overlay_set(overlay)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_settings_changed(id: *const u8, id_len: u32) {
        REAL_CORE.settings_changed(core::slice::from_raw_parts(id, id_len as usize))
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_live(op: u32, path: *const u8) -> i32 {
        REAL_CORE.live(op, path)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_live_state(out: *mut dkrt_sdk::DkrtLive) {
        *out = REAL_CORE.live_state()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_selection_add(path: *const u8, len: u32) -> i32 {
        REAL_CORE.selection_add(core::slice::from_raw_parts(path, len as usize))
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_module_path(owner: u8) -> *const u8 {
        REAL_CORE.module_path(owner)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_store_find(dir: *const u8, dir_len: u32, name: *const u8, name_len: u32, out: *mut u8) -> i32 {
        REAL_CORE.store_find(
            core::slice::from_raw_parts(dir, dir_len as usize),
            core::slice::from_raw_parts(name, name_len as usize),
            &mut *(out as *mut [u8; 64]),
        )
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_current_source() -> u8 {
        REAL_CORE.current_source()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_job_post(owner: u8, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32 {
        REAL_CORE.job_post(owner, step, done, ctx)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_job_cancel(owner: u8, id: u32) -> i32 {
        REAL_CORE.job_cancel(owner, id)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_dsp() -> *mut DspState {
        REAL_CORE.dsp()
    }
    #[cfg_attr(test, no_mangle)]
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_info(name_hash: u32) -> *const u32 {
        REAL_CORE.info(name_hash)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_set(slot: *const Slot, value: u32) {
        REAL_CORE.link_set(*slot, value)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_confirmed(slot: *const Slot, out: *mut u32) -> bool {
        match REAL_CORE.link_confirmed(*slot) {
            Some(v) => {
                *out = v;
                true
            }
            None => false,
        }
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_errors() -> u32 {
        REAL_CORE.link_errors()
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_forget(win: u32, first: u32, count: u32) {
        REAL_CORE.link_forget(win, first, count)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_submit(owner: Owner, stream: *const Stream, out: *mut Ticket) -> bool {
        match REAL_CORE.link_submit(owner, &*stream) {
            Some(t) => {
                *out = t;
                true
            }
            None => false,
        }
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_result(ticket: *const Ticket) -> LinkResult {
        REAL_CORE.link_result(*ticket)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_drain(owner: Owner) -> bool {
        REAL_CORE.link_drain(owner)
    }
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_link_run(owner: Owner, seq: *const Sequence) -> bool {
        REAL_CORE.link_run(owner, &*seq)
    }

    /// The row a service file declares in `init` (the declarations of the collection).
    pub use crate::collect::dk_service_declare_row;

    /// A service panicked: the runtime's panic (crash capture, safe mode on the next boot).
    #[cfg_attr(test, no_mangle)]
    pub unsafe extern "C" fn dkc_panic() -> ! {
        crate::fault::panic()
    }
}

/// One import: the 32-bit hash of its name (`name_hash`) and its address. The table keeps no names, so it is 8 bytes an entry; the const check below says no two
/// names of the list share a hash.
pub struct SvcImport {
    pub hash: u32,
    pub addr: *const (),
}

/// The table: `SVC_COUNT` entries, in the order of the list.
pub struct SvcImports(pub [SvcImport; SVC_COUNT]);
unsafe impl Sync for SvcImports {}

/// FNV-1a (32 bits) of a name; 0 is a possible value and means nothing.
pub const fn name_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

const fn distinct(h: &[u32]) -> bool {
    let mut i = 0;
    while i < h.len() {
        let mut k = i + 1;
        while k < h.len() {
            if h[i] == h[k] {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

macro_rules! svc_table {
    ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;)*) => {
        /// The number of imports a service file may add to `IMPORTS`.
        pub const SVC_COUNT: usize = [$(stringify!($name)),*].len();
        /// The names, in table order (the table itself keeps their hashes).
        #[cfg(test)]
        pub const SVC_NAMES: [&str; SVC_COUNT] = [$(stringify!($name)),*];
        // each shim has the type the list declares
        $(const _: unsafe extern "C" fn($($ty),*) $(-> $ret)? = shims::$name;)*
        const _: () = assert!(distinct(&[$(name_hash(stringify!($name).as_bytes())),*]), "two SVC_IMPORTS names have one hash");
        /// The imports of a service file (role 1) after `IMPORTS`: names and addresses from one list, so they cannot drift apart.
        pub static SVC_IMPORTS: SvcImports = SvcImports([$(SvcImport { hash: name_hash(stringify!($name).as_bytes()), addr: shims::$name as *const () }),*]);
    };
}
dkrt_abi::core_imports!(svc_table);

/// Address of the service import `name` (NUL-terminated), 0 when the table has none.
pub unsafe fn svc_import_addr(name: *const u8) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut n = 0;
    while *name.add(n) != 0 {
        h ^= *name.add(n) as u32;
        h = h.wrapping_mul(0x0100_0193);
        n += 1;
    }
    let mut i = 0;
    while i < SVC_IMPORTS.0.len() {
        if SVC_IMPORTS.0[i].hash == h {
            return DevAddr::of_fn(SVC_IMPORTS.0[i].addr as usize).get();
        }
        i += 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::address_map::AddressMap;

    #[test]
    fn the_table_is_in_the_order_of_the_list_with_the_hash_of_each_name() {
        // a shim has the device address that the host's map gives it
        let map = AddressMap::new();
        let _space = map.install();
        assert_eq!(SVC_IMPORTS.0.len(), SVC_COUNT);
        for (i, n) in SVC_NAMES.iter().enumerate() {
            assert_eq!(SVC_IMPORTS.0[i].hash, name_hash(n.as_bytes()), "{n}");
            assert!(!SVC_IMPORTS.0[i].addr.is_null());
            // each name finds its own entry through the C string the loader has
            let c = std::format!("{n}\0");
            assert_eq!(unsafe { svc_import_addr(c.as_ptr()) }, DevAddr::of_fn(SVC_IMPORTS.0[i].addr as usize).get(), "{n}");
        }
        // none of the module imports is here, so the order of the lookups does not matter
        for n in dkrt_sdk::api::BUILTIN_NAMES.iter() {
            assert!(!SVC_NAMES.contains(n), "{n} is in both tables");
        }
        assert!(SVC_NAMES.contains(&"dk_service_declare_row") && SVC_NAMES.contains(&"dkc_panic"));
        // the list is the one source of the table (a method added to it is checked by the loop above); the count is no pin
    }

    #[test]
    fn a_name_resolves_to_its_shim_and_an_unknown_name_to_zero() {
        let map = AddressMap::new();
        let _space = map.install();
        unsafe {
            assert_eq!(svc_import_addr(b"dkc_device_id\0".as_ptr()), DevAddr::of_fn(shims::dkc_device_id as usize).get());
            assert_eq!(svc_import_addr(b"dkc_nothing\0".as_ptr()), 0);
            assert_eq!(svc_import_addr(b"dk_alloc\0".as_ptr()), 0, "the module imports are the other table's");
        }
    }
}
