//! The role of a `.dkm` or `.dks` file (docs/plans/runtime-and-services.md section 13.2): the role word of the header and what the header table may hold for it.
//! `RoleSteps` are the steps that differ by role, chosen from the role word: a module has only a check (its table has no PROVIDES, HOOKS, KINDS, SLOTS, BIND, FW, FWNAMES or SITES item), a service (`.dks`) checks, keeps its table, publishes its exports and
//! installs its hooks (the row of the service is R12-4).
//! The service search (`discover.rs`) uses `role_of` and `role_ok`; the loader runs the steps.
use super::exports::EXPORTS_MAX;
use super::image::Head;
use super::install::{Backend, CfItems, DspItems, HookItem, PatchItem, SERVICE_PATCHES_MAX, SITES_MAX};
use super::{Images, Loader, ProvRec, Registry};
use dkneeds::keys::{tag, Keys, FLAG_ABSENT};
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_sdk::{E_DECL_KIND, E_FORMAT, E_HOOK_STOCK, E_IMPORT, E_NOMEM, E_PLAN_FIT, E_RANGE, E_SVC_ABI, OK};

/// What a role step may need of the file besides the record: the staged file (valid until `init` runs) and where its table and image are.
#[derive(Clone, Copy)]
pub struct PlaceArgs {
    pub file: *const u8,
    pub keys_off: u32,
    pub keys_len: u32,
    pub image_size: u32,
    /// Bytes of the file's DSP image (0: no DSP section).
    pub dsp_image: u32,
    /// The loaded image block (host pointer) and the address a module sees it at; the image is `image_size` bytes, `bss` bytes of zeros follow it.
    pub mem: *mut u8,
    pub base: u32,
    pub bss: u32,
}

/// The steps of one role, in the order the loader runs them. `check` runs after the header parse and before anything is allocated (`file` is the staged
/// file); `place` after the image is copied and before `init` (it copies what `init` could overwrite in the staging buffer); `activate` after the commit,
/// the last thing a load does; `undo` when a later step fails or the record goes away. A step has what it needs in the Loader and its seams.
pub struct RoleSteps<P: PackagePlatform, M: Images, R: Registry> {
    pub check: unsafe fn(&mut Loader<P, M, R>, file: *const u8, head: &Head) -> i32,
    pub place: unsafe fn(&mut Loader<P, M, R>, rec: usize, args: &PlaceArgs) -> i32,
    pub activate: unsafe fn(&mut Loader<P, M, R>, rec: usize) -> i32,
    pub undo: unsafe fn(&mut Loader<P, M, R>, rec: usize),
}

impl<P: PackagePlatform, M: Images, R: Registry> Clone for RoleSteps<P, M, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P: PackagePlatform, M: Images, R: Registry> Copy for RoleSteps<P, M, R> {}

impl<P: PackagePlatform, M: Images, R: Registry> RoleSteps<P, M, R> {
    /// The steps of the role word `role`: 1 is a service, 2 the port file; every other word is a module, as before the table (`role_ok` refuses another word in the
    /// search).
    pub const fn of(role: u32) -> Self {
        if role == 1 {
            Self::SERVICE
        } else if role == 2 {
            Self::PORT
        } else {
            Self::MODULE
        }
    }

    /// A module has only a check: its header table has no PROVIDES, HOOKS, KINDS, SLOTS, BIND, FW, FWNAMES or SITES item.
    pub const MODULE: Self = RoleSteps { check: module_check, place: no_place, activate: no_activate, undo: no_undo };

    /// A service (a provider record): the check refuses an unknown ABI, a bad table and a hook site that is not stock, all before anything is
    /// allocated; place keeps the table; activate publishes the exports and installs the hooks, last; undo withdraws and frees.
    pub const SERVICE: Self = RoleSteps { check: service_check, place: service_place, activate: service_activate, undo: service_undo };
}

impl<P: PackagePlatform, M: Images, R: Registry> RoleSteps<P, M, R> {
    /// The port file (a provider with no code): the check refuses another firmware and a bad table; place keeps the table; it publishes and installs nothing.
    pub const PORT: Self = RoleSteps { check: port_check, place: port_place, activate: no_activate, undo: service_undo };
}

unsafe fn module_check<P: PackagePlatform, M: Images, R: Registry>(_: &mut Loader<P, M, R>, file: *const u8, _: &Head) -> i32 {
    // only the role is checked here; a table outside the file or damaged keeps the error of the later steps
    let (off, len) = (dkrt_abi::mem::rd32(file.add(super::H_REG_OFF)), dkrt_abi::mem::rd32(file.add(super::H_REG_SIZE)));
    let size = dkrt_abi::mem::rd32(file.add(dkneeds::H_SIZE as usize));
    if !super::in_file(off, len, size) {
        return OK;
    }
    match table_of(file, off, len) {
        Ok(k) => role_ok(0, &k).err().unwrap_or(OK),
        Err(_) => OK,
    }
}
unsafe fn no_place<P: PackagePlatform, M: Images, R: Registry>(_: &mut Loader<P, M, R>, _: usize, _: &PlaceArgs) -> i32 {
    OK
}
unsafe fn no_activate<P: PackagePlatform, M: Images, R: Registry>(_: &mut Loader<P, M, R>, _: usize) -> i32 {
    OK
}
unsafe fn no_undo<P: PackagePlatform, M: Images, R: Registry>(_: &mut Loader<P, M, R>, _: usize) {}

/// The table of the staged file, or the refusal.
#[inline(never)]
unsafe fn table_of<'a>(file: *const u8, off: u32, len: u32) -> Result<Keys<'a>, i32> {
    Keys::parse(core::slice::from_raw_parts(file.add(off as usize), len as usize)).map_err(|_| E_FORMAT)
}

/// The hook sites a service takes, into `out`; their count. Each `HOOKS` item (hook name hash, entry) takes the `SITES` item of the loaded port file for that hook: its address,
/// displaced length and check value. A hook the firmware lacks (the item has the `absent` flag) is skipped. A hook the port file does not list (or no port file loaded) is
/// `E_IMPORT`, the hash kept for the log; a site the runtime cannot use is `E_RANGE`; more sites than the installer holds is `E_PLAN_FIT`.
#[inline(never)]
unsafe fn hook_items<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, keys: &Keys, out: &mut [HookItem; SITES_MAX]) -> Result<usize, i32> {
    let mut n = 0;
    let mut i = 0;
    while let Some((hash, entry, _)) = keys.words(tag::HOOKS, i) {
        i += 1;
        // the entry of another device (one name may have an entry for each of disjoint devices); a patch is no hook (`patch_items`)
        if !dkneeds::keys::hook_for_device(entry, l.p.device_id()) || dkneeds::keys::hook_is_patch(entry) {
            continue;
        }
        // a name has one entry for this device (the packer never writes two): which would win is not defined, so the file is malformed
        let mut j = 0;
        while j + 1 < i {
            if let Some((h, e, _)) = keys.words(tag::HOOKS, j) {
                if h == hash && dkneeds::keys::hook_for_device(e, l.p.device_id()) {
                    return Err(E_FORMAT);
                }
            }
            j += 1;
        }
        let entry = entry & dkneeds::keys::HOOK_ENTRY_MASK;
        let site = match l.port_keys() {
            Some((port, ..)) => port.find_site(hash),
            None => None,
        };
        let Some(s) = site else {
            l.import_hash = hash;
            return Err(E_IMPORT);
        };
        if s.flags & FLAG_ABSENT != 0 {
            continue;
        }
        // a patch site takes bytes, not a `jmp.l`
        if !super::port::site_ok(&s) || s.kind == dkneeds::keys::SITE_PATCH {
            return Err(E_RANGE);
        }
        if n == SITES_MAX {
            return Err(E_PLAN_FIT);
        }
        out[n] = HookItem { addr: s.addr, len: s.len as u32, check: s.check, entry, ctx: dkneeds::keys::site_context(s.flags) };
        n += 1;
    }
    Ok(n)
}

/// The patches a service takes, into `out`; their count. Each `HOOKS` item whose entry is the patch entry (`HOOK_ENTRY_PATCH`) takes the `SITES` item of the loaded port file for that
/// name: its address, length (1, 2 or 4) and the check value of the stock bytes. `values` are the patches the service declared in `init` (name hash, value, length), kept by the commit;
/// a patch the port file lists as absent on this firmware is skipped, with or without a value. A name the port file does not list is `E_IMPORT` (the hash kept for the log), a site that is
/// no patch site `E_RANGE`, a declared length that is not the site's `E_FORMAT`, a patch with no declared value `E_DECL_KIND` (the commit compared the declarations with the table, so only a
/// file that is not the packer's gets here). With no `values` (the check before `init`) the items carry value 0: the stock check needs no value.
#[inline(never)]
unsafe fn patch_items<P: PackagePlatform, M: Images, R: Registry>(
    l: &mut Loader<P, M, R>,
    keys: &Keys,
    values: Option<&[(u32, u32, u8)]>,
    out: &mut [PatchItem; SERVICE_PATCHES_MAX],
) -> Result<usize, i32> {
    let mut n = 0;
    let mut i = 0;
    while let Some((hash, entry, _)) = keys.words(tag::HOOKS, i) {
        i += 1;
        if !dkneeds::keys::hook_for_device(entry, l.p.device_id()) || !dkneeds::keys::hook_is_patch(entry) {
            continue;
        }
        // a name has one entry for this device (the packer never writes two)
        let mut j = 0;
        while j + 1 < i {
            if let Some((h, e, _)) = keys.words(tag::HOOKS, j) {
                if h == hash && dkneeds::keys::hook_for_device(e, l.p.device_id()) {
                    return Err(E_FORMAT);
                }
            }
            j += 1;
        }
        let site = match l.port_keys() {
            Some((port, ..)) => port.find_site(hash),
            None => None,
        };
        let Some(s) = site else {
            l.import_hash = hash;
            return Err(E_IMPORT);
        };
        if s.flags & FLAG_ABSENT != 0 {
            continue;
        }
        if !super::port::patch_ok(&s) {
            return Err(E_RANGE);
        }
        let mut value = 0;
        if let Some(values) = values {
            let Some(v) = values.iter().find(|v| v.0 == hash) else {
                return Err(E_DECL_KIND);
            };
            if v.2 as u16 != s.len {
                return Err(E_FORMAT);
            }
            value = v.1;
        }
        if n == out.len() {
            return Err(E_PLAN_FIT);
        }
        out[n] = PatchItem { addr: s.addr, len: s.len as u32, check: s.check, value, ctx: dkneeds::keys::site_context(s.flags) };
        n += 1;
    }
    Ok(n)
}

/// The `FW` item of the port file against the `fw_id` entry of the image's info table (the first 8 bytes of the SHA-256 of the stock firmware, two words). A port file
/// for another firmware, or an image with no `fw_id`, is `E_HOOK_STOCK`, the text of a site that is not stock. (`role_ok` made sure the file has one `FW` item.)
#[inline(never)]
unsafe fn fw_check<P: PackagePlatform>(p: &P, keys: &Keys) -> Result<(), i32> {
    let Some(id) = keys.fw(0) else {
        return Err(E_HOOK_STOCK);
    };
    if dkrt_abi::fw_id::image_fw_id(p) != Some(id) {
        return Err(E_HOOK_STOCK);
    }
    Ok(())
}

unsafe fn service_check<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, file: *const u8, head: &Head) -> i32 {
    if !l.r.abi_ok(dkrt_abi::mem::rd32(file.add(dkneeds::H_SVC_ABI as usize))) {
        return E_SVC_ABI;
    }
    let (off, len) = (dkrt_abi::mem::rd32(file.add(super::H_REG_OFF)), dkrt_abi::mem::rd32(file.add(super::H_REG_SIZE)));
    let size = dkrt_abi::mem::rd32(file.add(dkneeds::H_SIZE as usize));
    if !super::in_file(off, len, size) {
        return E_FORMAT;
    }
    let keys = match table_of(file, off, len) {
        Ok(k) => k,
        Err(e) => return e,
    };
    if let Err(e) = role_ok(1, &keys) {
        return e;
    }
    if keys.count(tag::EXPORTS) > EXPORTS_MAX {
        return E_FORMAT;
    }
    // every hook and patch site holds the stock bytes the port file's check value is of: checked here, before `init` runs, and again under the mask at the install
    let mut hooks = core::mem::zeroed::<[HookItem; SITES_MAX]>();
    let n = match hook_items(l, &keys, &mut hooks) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let mut patches = core::mem::zeroed::<[PatchItem; SERVICE_PATCHES_MAX]>();
    let np = match patch_items(l, &keys, None, &mut patches) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let checked = match l.replacing {
        Some(rec) => l.installer.check_replacing(&l.p, rec, &hooks[..n], &patches[..np], head.image_size),
        None => l.installer.check(&l.p, &hooks[..n], &patches[..np], head.image_size),
    };
    match checked {
        Ok(()) => OK,
        Err(e) => e,
    }
}

/// What `init` of a service file declares is compared with the file's header table. The table's `HOOKS` items carry a device mask in the top byte of the entry and list
/// the hooks of every device; this device's declarations hold the bare entry and only the hooks the firmware has. The copy of the table that is compared is made to
/// match: an item for another device is dropped with its mask cleared, and so is each hook that the port file lists as absent (a hook that it does not list stays, and the later `E_IMPORT` names it). A `SLOTS` item
/// carries a device mask too (bits 16 to 23 of its flags): an item for another device is dropped, and the mask of the others is cleared (`init` declares flags without it).
#[inline(never)]
pub(super) unsafe fn filter_hooks<P: PackagePlatform, M: Images, R: Registry>(l: &Loader<P, M, R>, keys: &mut crate::package::collect::KeysCopy) {
    let port = l.port_keys();
    // the hooks of another device are kept aside: a service declares all its hooks, and the commit drops the declarations that the table lists for another device only
    keys.set_aside_foreign_hooks(l.p.device_id());
    let d = &mut keys.decl;
    let (mut from, mut to) = (0, 0);
    while from < d.n {
        let mut it = d.items[from];
        if it.tag == tag::EXPORTS {
            it.b &= !dkneeds::keys::EXPORT_FLAGS;
        }
        from += 1;
        if it.tag == tag::SLOTS {
            if !dkneeds::keys::slot_for_device(it.b, l.p.device_id()) {
                continue;
            }
            it.b &= !dkneeds::keys::SLOT_DEVICES_MASK;
        }
        if it.tag == tag::HOOKS {
            it.b &= dkneeds::keys::HOOK_ENTRY_MASK;
            let gone = match &port {
                Some((p, ..)) => matches!(p.find_site(it.a), Some(s) if s.flags & FLAG_ABSENT != 0),
                None => false,
            };
            if gone {
                continue;
            }
        }
        d.items[to] = it;
        to += 1;
    }
    d.n = to;
}

/// The provider index of record `rec`.
fn index_of(rec: usize) -> usize {
    rec - dkrt_abi::MAX_MODULES
}

unsafe fn service_place<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize, a: &PlaceArgs) -> i32 {
    let idx = index_of(rec);
    l.provs[idx] = ProvRec::EMPTY;
    l.provs[idx].image_size = a.image_size;
    l.provs[idx].dsp_image = a.dsp_image;
    let keys = match table_of(a.file, a.keys_off, a.keys_len) {
        Ok(k) => k,
        Err(e) => return e,
    };
    l.provs[idx].provides = keys.words(tag::PROVIDES, 0).map(|(name, version, _)| (name, version));
    l.provs[idx].callback_order = keys.words(tag::CALLBACK_ORDER, 0).map(|w| w.1);
    // the exports are image offsets
    let mut i = 0;
    let query_hash = dkneeds::keys::name_hash(dkrt_sdk::RESIDENT_QUERY_ENTRY);
    while let Some((name, entry, _)) = keys.words(tag::EXPORTS, i) {
        if entry & !dkneeds::keys::EXPORT_FLAGS >= a.image_size
            || (entry & dkneeds::keys::EXPORT_QUERY != 0
                && (name != query_hash || entry & dkneeds::keys::EXPORT_BOUND == 0))
        {
            return E_FORMAT;
        }
        i += 1;
    }
    if keys.count(tag::HOOKS) == 0 && keys.count(tag::EXPORTS) == 0 && keys.count(tag::SLOTS) == 0 {
        return OK;
    }
    // the staged file is overwritten while `init` reads from the store: HOOKS, EXPORTS and SLOTS are kept in an arena block (the whole table)
    let blk = match l.m.alloc(a.keys_len) {
        Some(b) => b,
        None => return E_NOMEM,
    };
    core::ptr::copy_nonoverlapping(a.file.add(a.keys_off as usize), blk.ptr, a.keys_len as usize);
    l.provs[idx].block = Some(blk);
    l.provs[idx].len = a.keys_len;
    OK
}

/// The port file's checks, all before anything is allocated: the table fits the role; the file is for this image's firmware (`FW` against `fw_id`, `E_HOOK_STOCK`
/// otherwise); the image starts with the `rts` that every `dk_ret_` resolves to and its bss holds the copies of the sites; every site is one the runtime can use; and
/// the live bytes of each site that is copied are the stock ones (the check value, `E_HOOK_STOCK`).
unsafe fn port_check<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, file: *const u8, head: &Head) -> i32 {
    let (off, len) = (dkrt_abi::mem::rd32(file.add(super::H_REG_OFF)), dkrt_abi::mem::rd32(file.add(super::H_REG_SIZE)));
    let size = dkrt_abi::mem::rd32(file.add(dkneeds::H_SIZE as usize));
    if !super::in_file(off, len, size) {
        return E_FORMAT;
    }
    let keys = match table_of(file, off, len) {
        Ok(k) => k,
        Err(e) => return e,
    };
    if let Err(e) = role_ok(2, &keys) {
        return e;
    }
    if let Err(e) = fw_check(&l.p, &keys) {
        return e;
    }
    let first = core::ptr::read_unaligned(file.add(head.image_off as usize) as *const u16);
    if head.image_size < 2 || u16::from_be(first) != super::port::RTS || head.bss < super::port::tails_total(&keys) {
        return E_FORMAT;
    }
    let mut i = 0;
    while let Some(s) = keys.site(i) {
        i += 1;
        if s.flags & dkneeds::keys::FLAG_ABSENT != 0 {
            continue;
        }
        if !super::port::site_ok(&s) {
            return E_RANGE;
        }
        if super::port::copies(&s) && super::port::site_check(&l.p, s.addr, s.len as usize) != s.check {
            return E_HOOK_STOCK;
        }
    }
    OK
}

/// The port file keeps its whole table in an arena block (the staged file is overwritten while `init` reads from the store, and the imports of the services that load
/// after it are looked up in it), and fills the copies of its tail and stock sites in the bss of its image: the displaced bytes from flash, then `jmp.l` behind the site.
/// The check of the file compared those bytes with the check value; the caches are written back at the end.
unsafe fn port_place<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize, a: &PlaceArgs) -> i32 {
    let idx = index_of(rec);
    l.provs[idx] = ProvRec::EMPTY;
    l.provs[idx].image_size = a.image_size;
    let blk = match l.m.alloc(a.keys_len) {
        Some(b) => b,
        None => return E_NOMEM,
    };
    core::ptr::copy_nonoverlapping(a.file.add(a.keys_off as usize), blk.ptr, a.keys_len as usize);
    l.provs[idx].block = Some(blk);
    l.provs[idx].len = a.keys_len;
    let keys = match table_of(a.file, a.keys_off, a.keys_len) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let mut off = a.image_size;
    let mut i = 0;
    while let Some(s) = keys.site(i) {
        i += 1;
        if s.flags & dkneeds::keys::FLAG_ABSENT != 0 || !super::port::copies(&s) {
            continue;
        }
        let len = s.len as usize;
        let at = a.mem.add(off as usize);
        l.p.read_code(s.addr, core::slice::from_raw_parts_mut(at, len));
        core::ptr::copy_nonoverlapping(0x4ef9u16.to_be_bytes().as_ptr(), at.add(len), 2);
        core::ptr::copy_nonoverlapping(s.addr.wrapping_add(s.len as u32).to_be_bytes().as_ptr(), at.add(len + 2), 4);
        off += super::port::tail_bytes(&s);
    }
    l.p.flush(a.base.wrapping_add(a.image_size), off - a.image_size);
    OK
}

/// The table of the loaded service file `rec` that its place step kept, and the record's copy of what it keeps; `None` when it kept none (no hooks, exports or slots).
unsafe fn kept_table<P: PackagePlatform, M: Images, R: Registry>(l: &Loader<P, M, R>, rec: usize) -> Result<Option<(Keys<'static>, ProvRec)>, i32> {
    let pr = l.provs[index_of(rec)];
    let Some(blk) = pr.block else {
        return Ok(None);
    };
    match Keys::parse(core::slice::from_raw_parts(blk.ptr as *const u8, pr.len as usize)) {
        Ok(k) => Ok(Some((k, pr))),
        Err(_) => Err(E_FORMAT),
    }
}

/// Publish the exports of the loaded service file `rec` (`Exports`): each `EXPORTS` item is an image offset. Returns the address of its resident query entry (0: none), or
/// `None` when the file kept no table. The query address is kept by the install (`service_install`), so it is public to a load only when the install has succeeded.
pub(super) unsafe fn service_publish<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize) -> Result<Option<u32>, i32> {
    let Some((keys, _)) = kept_table(l, rec)? else {
        return Ok(None);
    };
    let base = super::records::module(rec).base;
    let mut i = 0;
    let mut query_v1 = 0;
    while let Some(w) = keys.words(tag::EXPORTS, i) {
        let bound = w.1 & dkneeds::keys::EXPORT_BOUND != 0;
        let addr = base.wrapping_add(w.1 & !dkneeds::keys::EXPORT_FLAGS);
        if w.1 & dkneeds::keys::EXPORT_QUERY != 0 {
            query_v1 = addr;
        } else if let Err(e) = l.exports.publish_with_scope(rec, w.0, addr, bound) {
            return Err(e);
        }
        i += 1;
    }
    Ok(Some(query_v1))
}

/// What a service file installs: its hook sites and the slot words it fills, from its kept table and the port file.
pub(super) struct ServiceItems {
    pub hooks: [HookItem; SITES_MAX],
    pub nh: usize,
    /// The patches, with the values the service declared in `init`.
    pub patches: [PatchItem; SERVICE_PATCHES_MAX],
    pub np: usize,
    /// ColdFire slots: (address of the word, value).
    pub slots: [(u32, u32); dkneeds::SLOTS_MAX as usize],
    pub ns: usize,
    /// DSP slots: (index in the slot block, pc of the dispatch code).
    pub dsp_slots: [(u32, u32); dkneeds::SLOTS_MAX as usize],
    pub nd: usize,
}

impl ServiceItems {
    pub(super) const fn empty() -> ServiceItems {
        ServiceItems {
            hooks: [HookItem { addr: 0, len: 0, check: [0; 8], entry: 0, ctx: 0 }; SITES_MAX],
            nh: 0,
            patches: [PatchItem { addr: 0, len: 0, check: [0; 8], value: 0, ctx: 0 }; SERVICE_PATCHES_MAX],
            np: 0,
            slots: [(0u32, 0u32); dkneeds::SLOTS_MAX as usize],
            ns: 0,
            dsp_slots: [(0u32, 0u32); dkneeds::SLOTS_MAX as usize],
            nd: 0,
        }
    }
}

/// The items of the loaded service file `rec` into `it` (zeroed by the caller: `ServiceItems::empty`), `false` when it kept no table. A hook the port file does not list, a slot the image lacks or a bad entry refuses the file
/// before anything is installed.
pub(super) unsafe fn service_items<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize, it: &mut ServiceItems) -> Result<bool, i32> {
    let Some((keys, pr)) = kept_table(l, rec)? else {
        return Ok(false);
    };
    let base = super::records::module(rec).base;
    it.nh = hook_items(l, &keys, &mut it.hooks)?;
    // the patches: sites from the port file, bytes from the declarations `init` made (kept by the commit); a service that declared none has no patch item in its table
    let declared = pr.patches;
    it.np = patch_items(l, &keys, Some(&declared[..pr.npatch as usize]), &mut it.patches)?;
    // the pointer slots the file fills: a SLOTS item is the name hash of a `[[slot]]` of an image adapter (the image's info table gives the address of its word) and the entry
    // of this file. A slot the image does not have, or a name that is an `[[info]]` data table, refuses the file, before anything is installed.
    for sl in keys.slots() {
        // a slot of another device's image: the file serves every device, and its mask says which fills are for this one
        if !dkneeds::keys::slot_for_device(sl.flags, l.p.device_id()) {
            continue;
        }
        if sl.flags & dkneeds::keys::SLOT_DSP != 0 {
            // a DSP slot: the name is a `[[slot]] space = "dsp"` of an image adapter (its index in the slot block comes from the info table) and the entry is a byte offset in
            // this file's DSP image, which must be there and even (a pc is whole words)
            if it.nd == it.dsp_slots.len() || pr.dsp_image == 0 || sl.entry >= pr.dsp_image || sl.entry & 1 != 0 {
                return Err(E_FORMAT);
            }
            let idx = match l.p.info_dsp_slot(sl.id) {
                Some(i) => i,
                None => return Err(E_FORMAT),
            };
            it.dsp_slots[it.nd] = (idx, crate::package::dsp::placed_pc(rec, sl.entry));
            it.nd += 1;
            continue;
        }
        if it.ns == it.slots.len() || sl.entry >= pr.image_size {
            return Err(E_FORMAT);
        }
        let at = l.p.info_slot(sl.id);
        if at.is_null() {
            return Err(E_FORMAT);
        }
        it.slots[it.ns] = (at.get(), base.wrapping_add(sl.entry));
        it.ns += 1;
    }
    Ok(true)
}

unsafe fn service_activate<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize) -> i32 {
    let query_v1 = match service_publish(l, rec) {
        Ok(Some(q)) => q,
        Ok(None) => return OK,
        Err(e) => return e,
    };
    let mut it = ServiceItems::empty();
    match service_items(l, rec, &mut it) {
        Ok(true) => {}
        Ok(false) => return OK,
        Err(e) => return e,
    }
    let base = super::records::module(rec).base;
    let image_size = l.provs[index_of(rec)].image_size;
    // the DSP slots are claimed (not written: the stubs stay stock until the DSP acknowledges the code), then the hooks go in last: nothing after this can fail, so a live
    // site is never taken down again. A failure of the hooks gives the claim back.
    if let Err(e) = Backend::install(&mut l.dsp_install, &l.p, rec as u8, &DspItems { slots: &it.dsp_slots[..it.nd] }) {
        return e;
    }
    let parks = l.task_parks();
    let cf = CfItems { hooks: &it.hooks[..it.nh], patches: &it.patches[..it.np], image_base: base, image_size, slots: &it.slots[..it.ns], gate: parks.gate(l.live()).in_op(l.op()) };
    match Backend::install(&mut l.installer, &l.p, rec as u8, &cf) {
        Ok(()) => {
            l.provs[index_of(rec)].query_v1 = query_v1;
            OK
        }
        Err(e) => {
            Backend::restore(&mut l.dsp_install, &l.p, rec as u8);
            e
        }
    }
}

unsafe fn service_undo<P: PackagePlatform, M: Images, R: Registry>(l: &mut Loader<P, M, R>, rec: usize) {
    let idx = index_of(rec);
    l.exports.withdraw(rec);
    crate::package::settings_overlay::withdraw(rec);
    if let Some(b) = l.provs[idx].block {
        l.m.free(b.addr);
    }
    l.provs[idx] = ProvRec::EMPTY;
}

/// The role word of a file: 0 a module, 1 a service (`.dks`), 2 the port file (`.dkport`). `head` is the first 64 bytes of the file; the word is big endian, like every header field.
pub fn role_of(head: &[u8; 64]) -> u32 {
    let at = dkneeds::H_ROLE as usize;
    u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]])
}

/// Whether the header table `keys` fits the role: a module (0) has no PROVIDES, HOOKS, KINDS, SLOTS, BIND, FW, FWNAMES or SITES item; a service (1) has table version 2 and
/// exactly one PROVIDES item and no BIND or FW item (a bound file, which this runtime does not load); the port file (2) has version 3, one PROVIDES and one FW item and no item of a module or a service. Another role is `E_FORMAT`, like the other refusals.
pub fn role_ok(role: u32, keys: &Keys) -> Result<(), i32> {
    // the entries the table has, one bit per tag
    let mut has = 0u32;
    let mut t = tag::NEEDS;
    while t <= tag::SERVICE_IMPORTS {
        if keys.count(t) != 0 {
            has |= 1 << t;
        }
        t += 1;
    }
    let bit = |t: u16| 1u32 << t;
    let port = bit(tag::FWNAMES) | bit(tag::SITES);
    if role == 0 {
        // No packer writes HOOKS, KINDS or SLOTS for a module (patchkit major3.rs writes only USES and KEYS, dkm.rs adds NEEDS): they are a service's (role 1).
        // A content module owns no firmware hook, record kind or plug-in point, so two modules can clash only on keys and capacity.
        let service = bit(tag::CALLBACK_ORDER) | bit(tag::PROVIDES) | bit(tag::HOOKS) | bit(tag::KINDS) | bit(tag::SLOTS) | bit(tag::BIND) | bit(tag::FW);
        if has & (service | port) != 0 {
            return Err(E_FORMAT);
        }
        Ok(())
    } else if role == 1 {
        if keys.version() != 2 || keys.count(tag::PROVIDES) != 1 || has & (port | bit(tag::BIND) | bit(tag::FW)) != 0 {
            return Err(E_FORMAT);
        }
        if let Some(order) = keys.words(tag::CALLBACK_ORDER, 0) {
            if keys.words(tag::PROVIDES, 0).map(|w| w.0) != Some(order.0) {
                return Err(E_FORMAT);
            }
        }
        Ok(())
    } else if role == 2 {
        // the port file: version 3, the one name it provides and the firmware it is for, its names and sites; nothing a service or a module declares
        if keys.version() != 3 || keys.count(tag::PROVIDES) != 1 || keys.count(tag::FW) != 1 || has & !(bit(tag::PROVIDES) | bit(tag::FW) | port) != 0 {
            return Err(E_FORMAT);
        }
        Ok(())
    } else {
        Err(E_FORMAT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dkneeds::keys::{write_table, KindDecl, Pair, Slot, Table};
    use std::vec;
    use std::vec::Vec;

    const KEEP: Table<'static> =
        Table { needs: &[], uses: &[], keys: &[], kinds: &[], slots: &[], hooks: &[], exports: &[], resources: &[], provides: &[], bind: &[], fw: &[], names: &[], sites: &[], callback_order: None, service_imports: &[] };

    fn bytes(t: &Table) -> Vec<u8> {
        let mut b = vec![0u8; dkneeds::keys::table_size(t)];
        write_table(t, &mut b).unwrap();
        b
    }

    fn check(role: u32, t: &Table) -> Result<(), i32> {
        let b = bytes(t);
        role_ok(role, &Keys::parse(&b).unwrap())
    }

    const ONE: &[Pair] = &[Pair { a: 7, b: 0x010000 }];

    #[test]
    fn the_role_word_is_read_big_endian_from_the_header() {
        let mut head = [0u8; 64];
        assert_eq!(role_of(&head), 0);
        head[dkneeds::H_ROLE as usize + 3] = 1;
        assert_eq!(role_of(&head), 1);
        head[dkneeds::H_ROLE as usize] = 1;
        assert_eq!(role_of(&head), 0x0100_0001);
    }

    #[test]
    fn a_module_with_nothing_for_a_service_is_ok() {
        assert_eq!(check(0, &KEEP), Ok(()));
    }

    #[test]
    fn a_module_with_a_provides_hooks_kinds_slots_or_fw_item_is_refused() {
        assert_eq!(check(0, &Table { provides: ONE, ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(0, &Table { hooks: &[Pair { a: 9, b: 4 }], ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(0, &Table { kinds: &[KindDecl { kind: 90, len: 16, entry: 8 }], ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(0, &Table { slots: &[Slot { id: 7, flags: 0, entry: 0 }], ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(0, &Table { fw: &[[0; 8]], ..KEEP }), Err(E_FORMAT));
    }

    #[test]
    fn callback_order_must_belong_to_the_providing_service() {
        let order = Table { provides: ONE, callback_order: Some(Pair { a: ONE[0].a, b: 100 }), ..KEEP };
        assert_eq!(check(1, &order), Ok(()));
        assert_eq!(check(0, &order), Err(E_FORMAT));
        let mismatch = Table { callback_order: Some(Pair { a: 42, b: 100 }), ..order };
        assert_eq!(check(1, &mismatch), Err(E_FORMAT));
    }

    #[test]
    fn a_service_needs_version_2_and_exactly_one_provides_item() {
        assert_eq!(check(1, &Table { provides: ONE, ..KEEP }), Ok(()));
        assert_eq!(check(1, &KEEP), Err(E_FORMAT));
        assert_eq!(check(1, &Table { provides: &[Pair { a: 7, b: 0 }, Pair { a: 8, b: 0 }], ..KEEP }), Err(E_FORMAT));
        // a table of version 1
        let mut b = bytes(&Table { provides: ONE, ..KEEP });
        b[4..6].copy_from_slice(&1u16.to_be_bytes());
        if let Ok(k) = Keys::parse(&b) {
            assert_eq!(role_ok(1, &k), Err(E_FORMAT));
        }
    }

    #[test]
    fn another_role_is_refused() {
        assert_eq!(check(3, &Table { provides: ONE, ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(u32::MAX, &KEEP), Err(E_FORMAT));
    }

    #[test]
    fn the_port_file_has_version_3_one_provides_and_one_fw_and_no_item_of_a_service() {
        use dkneeds::keys::{NameRec, SiteRec};
        const NAMES: &[NameRec] = &[NameRec { hash: 1, addr: 0x100, n: 0, kind: 0, flags: 0 }];
        const SITES: &[SiteRec] = &[SiteRec { hash: 2, addr: 0x200, len: 6, kind: 0, flags: 0, check: [0; 8] }];
        let port = Table { provides: ONE, fw: &[[7; 8]], names: NAMES, sites: SITES, ..KEEP };
        assert_eq!(check(2, &port), Ok(()));
        assert_eq!(check(2, &Table { fw: &[], ..port }), Err(E_FORMAT), "a port file names its firmware");
        assert_eq!(check(2, &Table { provides: &[], ..port }), Err(E_FORMAT));
        assert_eq!(check(2, &Table { hooks: &[Pair { a: 9, b: 4 }], ..port }), Err(E_FORMAT));
        assert_eq!(check(2, &Table { exports: &[Pair { a: 9, b: 4 }], ..port }), Err(E_FORMAT));
        // a version 2 table has no port entries, so a port file without them is a version 2 table: refused
        assert_eq!(check(2, &Table { names: &[], sites: &[], ..port }), Err(E_FORMAT));
        // a module or a service holds none of the port's entries
        assert_eq!(check(0, &Table { names: NAMES, ..KEEP }), Err(E_FORMAT));
        assert_eq!(check(1, &Table { provides: ONE, sites: SITES, ..KEEP }), Err(E_FORMAT));
    }
}
