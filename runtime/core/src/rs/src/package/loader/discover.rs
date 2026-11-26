//! Find the service providers of a boot (docs/plans/runtime-and-services.md section 13.2): the hard needs of the selection that no runtime service meets are
//! looked up as `services/<name>.dks` in the store. NEEDS carries only the hash of a service name, so the boot lists `services/` once and matches
//! `name_hash_bytes(stem)` of each file against the needs. A provider found adds its own hard needs to the search (transitively), up to `PROVIDER_MAX` providers.
//!
//! The port file is the one provider that is no service file: a need of `port` (`PORT_NAME`) is met by the file of `ports/` whose `FW` item equals the image's `fw_id`
//! (the name of the file does not matter, and the other port files are not read further). No such file: the need stays unmet and the planner refuses its dependents.
//!
//! The listing and the file read are the runtime's own (`Cards`: `store::list` with the prefix `services/`, and `Images::stage`, the read that loads a module),
//! so the search links no code of its own for the card. Nothing is allocated: the candidates, the search list and the result are fixed arrays. A provider is
//! checked in this order, and the first failure skips the file (the planner then reports the need as missing): the file's header and role word, the header
//! table range, the table walk and its parse.
use super::role::role_ok;
use super::{Images, Staged};
use super::TABLE_MAX;
use dkneeds::keys::{name_hash, name_hash_bytes, tag, Keys};
use dkrt_sdk::{DkrtStoreEntry, PROVIDER_MAX};

/// Whether service name hash `h` is one the runtime provides itself (the `runtime` floor and the services of the image): a need of it is no provider's to meet,
/// and a provider file cannot take the name. The services that have handle types in the SDK but are service files (`fx`) are not here.
pub(crate) fn is_runtime_service(h: u32) -> bool {
    if h == name_hash("runtime") {
        return true;
    }
    let mut k = 0;
    while k < dkrt_sdk::records::HANDLE_SERVICES.len() {
        if name_hash(dkrt_sdk::records::HANDLE_SERVICES[k]) == h {
            return true;
        }
        k += 1;
    }
    false
}

/// The folder of the provider files in the store root, and the file extension.
const DIR: &[u8] = b"services/";
const EXT: &[u8] = b".dks";
/// The name a port file provides (`PROVIDES`), and the folder and extension of its files.
pub const PORT_NAME: &[u8] = b"port";
const PORT_DIR: &[u8] = b"ports/";
const PORT_EXT: &[u8] = b".dkport";
/// Longest file stem the search considers (50: `services/<stem>.dks` and its NUL fit the 64 bytes of a selection path), and the longest path it builds.
const STEM_MAX: usize = 50;
pub const PATH_CAP: usize = DIR.len() + STEM_MAX + EXT.len();
/// Most `.dks` files of the folder the search remembers. A folder with more `.dks` files sets `Found::truncated`; the files past the limit are not looked at. `truncated` is also
/// set when every provider slot (`PROVIDER_MAX`) is taken and a need is left unsearched, or the port file has no slot: `find_providers` logs it (`E_PLAN_FIT`).
const CAND_MAX: usize = 32;
/// Most service hashes the search keeps. Distinct non-runtime needs beyond the provider slots can never all be met; 4 times `PROVIDER_MAX` leaves room for
/// needs of refused or unmatched services. A need past the limit sets `Found::truncated`.
const WANT_MAX: usize = 4 * PROVIDER_MAX;

/// The causes of `Found::truncated` (a set of bits): the provider table has no slot left for a service or the port file, the folder has more files than `CAND_MAX`,
/// the search list has no room for a need (`WANT_MAX`). Each has its own text in the provider log (`Loader::find_providers`).
pub const TRUNC_TABLE: u8 = 1;
pub const TRUNC_FILES: u8 = 2;
pub const TRUNC_NEEDS: u8 = 4;

/// What the search needs of the card: the runtime's store listing (`prefix` NUL-terminated, files only, in path order) and the read of one whole file into
/// the staging buffer. Every `Images` is one (the real one lists and reads the store; the fakes of the host tests list and read a map).
pub trait Cards {
    unsafe fn list(&mut self, prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32;
    unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32>;
}

impl<M: Images> Cards for M {
    unsafe fn list(&mut self, prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
        Images::list(self, prefix, out, max)
    }
    unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
        Images::stage(self, path)
    }
}

/// The providers found: for provider `k`, its store path and its header table (the bytes the planner parses).
pub struct Found {
    pub n: usize,
    /// The limits of the search that were passed (`TRUNC_*` bits: no provider slot left, more `.dks` files than `CAND_MAX`, more needs than `WANT_MAX`): a provider may be missing from the result.
    pub truncated: u8,
    /// The store was busy (`E_BUSY`) at a listing or a file read of the last search: the result may lack providers that are on the card.
    pub busy: bool,
    // the search's work arrays live here, not on the boot thread's stack: the files of the folder and whether each is taken, and the service hashes searched for
    cands: [DkrtStoreEntry; CAND_MAX + 1],
    c_used: [bool; CAND_MAX],
    search: [u32; WANT_MAX],
    pub path_len: [usize; PROVIDER_MAX],
    pub paths: [[u8; PATH_CAP]; PROVIDER_MAX],
    pub table_len: [usize; PROVIDER_MAX],
    pub tables: [[u8; TABLE_MAX]; PROVIDER_MAX],
}

impl Found {
    pub const fn new() -> Found {
        Found {
            n: 0,
            truncated: 0,
            busy: false,
            cands: unsafe { core::mem::zeroed() },
            c_used: [false; CAND_MAX],
            search: [0; WANT_MAX],
            path_len: [0; PROVIDER_MAX],
            paths: [[0; PATH_CAP]; PROVIDER_MAX],
            table_len: [0; PROVIDER_MAX],
            tables: [[0; TABLE_MAX]; PROVIDER_MAX],
        }
    }
    /// The store path of provider `k`.
    pub fn path(&self, k: usize) -> &[u8] {
        &self.paths[k][..self.path_len[k]]
    }
    /// The header table of provider `k`.
    pub fn table(&self, k: usize) -> &[u8] {
        &self.tables[k][..self.table_len[k]]
    }
}

fn has(list: &[u32], n: usize, h: u32) -> bool {
    let mut i = 0;
    while i < n {
        if list[i] == h {
            return true;
        }
        i += 1;
    }
    false
}

/// The length of the NUL-terminated path of candidate `c`.
fn path_of(c: &DkrtStoreEntry) -> &[u8] {
    let mut n = 0;
    while n < c.path.len() && c.path[n] != 0 {
        n += 1;
    }
    &c.path[..n]
}

/// The stem of `services/<stem>.dks`: None for another extension, a stem over `STEM_MAX` bytes or a file in a subfolder.
fn stem_of(path: &[u8]) -> Option<&[u8]> {
    if path.len() <= DIR.len() + EXT.len() || !path.ends_with(EXT) {
        return None;
    }
    let stem = &path[DIR.len()..path.len() - EXT.len()];
    if stem.len() > STEM_MAX || stem.contains(&b'/') {
        return None;
    }
    Some(stem)
}

#[inline(never)]
/// Check the staged file `file` as a provider and put its table, without the FW, FWNAMES and SITES entries, into slot `k` of `out`; false when the file is not a usable provider.
fn table_of(file: &[u8], k: usize, out: &mut Found, role: u32, fw: Option<[u8; 8]>) -> bool {
    let be = |p: usize| u32::from_be_bytes([file[p], file[p + 1], file[p + 2], file[p + 3]]);
    if file.len() < dkneeds::H_REG_SIZE as usize + 4 || be(dkneeds::H_ROLE as usize) != role {
        return false;
    }
    let (off, len) = (be(dkneeds::H_REG_OFF as usize) as usize, be(dkneeds::H_REG_SIZE as usize) as usize);
    if len < 8 || off > file.len() || len > file.len() - off {
        return false;
    }
    let src = &file[off..off + len];
    // The full table must parse (heads and sizes fit, the entries use it up exactly). The planner's copy is the table without its FW, FWNAMES and SITES entries: the
    // port file's table has many names and is over TABLE_MAX, and the planner never needs them (the load reads them). The entries are in ascending tag order and these
    // have the highest tags, so they are the last entries: the copy is the table up to the first of them, with fewer entries.
    let full = match Keys::parse(src) {
        Ok(k) => k,
        Err(_) => return false,
    };
    if role_ok(role, &full).is_err() {
        return false;
    }
    // a port file is for one firmware: the one whose `FW` is the image's `fw_id`
    if role == 2 && (fw.is_none() || full.fw(0) != fw) {
        return false;
    }
    let mut n_entries = u16::from_be_bytes([src[6], src[7]]) as usize;
    let mut end = len;
    if full.count(tag::FW) != 0 {
        let mut pos = 8;
        let mut e = 0;
        while e < n_entries {
            if u16::from_be_bytes([src[pos], src[pos + 1]]) >= tag::FW {
                break;
            }
            pos += 8 + u32::from_be_bytes([src[pos + 4], src[pos + 5], src[pos + 6], src[pos + 7]]) as usize;
            e += 1;
        }
        end = pos;
        n_entries = e;
    }
    if end > TABLE_MAX {
        return false; // the copy does not hold the table
    }
    let t = &mut out.tables[k];
    t[..end].copy_from_slice(&src[..end]);
    t[6..8].copy_from_slice(&(n_entries as u16).to_be_bytes());
    out.table_len[k] = end;
    true
}

/// Read provider candidate `c` into slot `out.n` of `out`; false when the file is not a usable provider.
unsafe fn read_provider<C: Cards>(cards: &mut C, c: usize, out: &mut Found, role: u32, fw: Option<[u8; 8]>) -> bool {
    let k = out.n;
    let at = path_of(&out.cands[c]).len();
    out.paths[k][..at].copy_from_slice(path_of(&out.cands[c]));
    out.path_len[k] = at;
    let mut path = [0u8; PATH_CAP + 1];
    path[..at].copy_from_slice(&out.paths[k][..at]);
    let staged = match cards.stage(path.as_ptr()) {
        Ok(s) => s,
        Err(e) => {
            out.busy |= e == dkrt_sdk::E_BUSY;
            return false;
        }
    };
    let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
    if table_of(file, k, out, role, fw) {
        return true;
    }
    // a service file of the old format is skipped, and the boot log says why
    if role == 1 {
        if let Some((code, hash)) = stale(file) {
            crate::package::resident::boot::report_provider(out.path(k), code, hash, 0);
        }
    }
    false
}

/// A service file bound to one firmware (`BIND` or `FW` items, the format before the port file): why this runtime does not take it, and the service it provides. A Rust service
/// carries the ABI fingerprint, which `FIRMWARE_REFERENCES` changed, so the refusal is `E_SVC_ABI`; a C service has the word 0 and is `E_FORMAT`.
fn stale(file: &[u8]) -> Option<(i32, u32)> {
    let be = |p: usize| u32::from_be_bytes([file[p], file[p + 1], file[p + 2], file[p + 3]]);
    if file.len() < dkneeds::HEADER as usize || be(dkneeds::H_ROLE as usize) != 1 {
        return None;
    }
    let (off, len) = (be(dkneeds::H_REG_OFF as usize) as usize, be(dkneeds::H_REG_SIZE as usize) as usize);
    if len < 8 || off > file.len() || len > file.len() - off {
        return None;
    }
    let keys = Keys::parse(&file[off..off + len]).ok()?;
    if keys.count(tag::BIND) == 0 && keys.count(tag::FW) == 0 {
        return None;
    }
    let code = if be(dkneeds::H_SVC_ABI as usize) != 0 { dkrt_sdk::E_SVC_ABI } else { dkrt_sdk::E_FORMAT };
    Some((code, keys.words(tag::PROVIDES, 0).map_or(0, |w| w.0)))
}

/// Is `path` a file `ports/<name>.dkport`: the stem within `STEM_MAX`, no subfolder.
fn is_port_path(path: &[u8]) -> bool {
    if path.len() <= PORT_DIR.len() + PORT_EXT.len() || !path.starts_with(PORT_DIR) || !path.ends_with(PORT_EXT) {
        return false;
    }
    let stem = &path[PORT_DIR.len()..path.len() - PORT_EXT.len()];
    stem.len() <= STEM_MAX && !stem.contains(&b'/')
}

/// Find the port file for the firmware `fw` (the image's `fw_id`): the first file of `ports/` that is a port file whose `FW` item equals it. Other files are skipped,
/// and a folder with more files than `CAND_MAX` sets `truncated`. The file is provider `out.n`.
unsafe fn find_port<C: Cards>(cards: &mut C, fw: Option<[u8; 8]>, out: &mut Found) {
    if fw.is_none() {
        return;
    }
    if out.n >= PROVIDER_MAX {
        // no slot left for the port file
        out.truncated |= TRUNC_TABLE;
        return;
    }
    // the port provider may be in `out` already (a live search after one that found it): it is not added twice
    let mut j = 0;
    while j < out.n {
        if is_port_path(out.path(j)) {
            return;
        }
        j += 1;
    }
    let found = cards.list(b"ports/\0".as_ptr(), out.cands.as_mut_ptr(), (CAND_MAX + 1) as u32);
    if found <= 0 {
        out.busy |= found == dkrt_sdk::E_BUSY;
        return;
    }
    let nc = if found as usize > CAND_MAX {
        out.truncated |= TRUNC_FILES;
        CAND_MAX
    } else {
        found as usize
    };
    let mut c = 0;
    while c < nc {
        if is_port_path(path_of(&out.cands[c])) && read_provider(cards, c, out, 2, fw) {
            out.n += 1;
            return;
        }
        c += 1;
    }
}

/// Find the providers for the service hashes `want` (the unresolved hard needs of the selection); `fw` is the image's `fw_id`, which picks the port file. No `services/` folder, a busy store (`E_BUSY`) or any
/// listing error finds nothing, without an error. A need of a runtime service (`is_runtime_service`) is not searched for.
pub unsafe fn find<C: Cards>(cards: &mut C, want: &[u32], fw: Option<[u8; 8]>, out: &mut Found) {
    out.n = 0;
    out.truncated = 0;
    search(cards, want, fw, out);
}

/// Find the providers for `want` and add them after the ones `out` holds (a live load whose service the boot did not look for): the providers already in `out` keep their
/// slots (their records may be up) and their files are not read again. `OK`, or `E_BUSY` when the store was busy (a provider may be on the card that was not read: the caller
/// asks again later, and does not take the result for "no such service"); any other listing error finds nothing, as in `find`.
pub unsafe fn find_more<C: Cards>(cards: &mut C, want: &[u32], fw: Option<[u8; 8]>, out: &mut Found) -> i32 {
    search(cards, want, fw, out);
    if out.busy {
        dkrt_sdk::E_BUSY
    } else {
        dkrt_sdk::OK
    }
}

unsafe fn search<C: Cards>(cards: &mut C, want: &[u32], fw: Option<[u8; 8]>, out: &mut Found) {
    out.busy = false;
    if want.is_empty() {
        return;
    }
    // the folder is listed once: the `.dks` files are remembered
    out.c_used = [false; CAND_MAX];
    let found = cards.list(b"services/\0".as_ptr(), out.cands.as_mut_ptr(), (CAND_MAX + 1) as u32);
    out.busy |= found == dkrt_sdk::E_BUSY;
    let nc = if found <= 0 {
        0
    } else if found as usize > CAND_MAX {
        out.truncated |= TRUNC_FILES;
        CAND_MAX
    } else {
        found as usize
    };
    // the files of the providers already found are taken
    let mut c = 0;
    while c < nc {
        let mut j = 0;
        while j < out.n {
            if out.path(j) == path_of(&out.cands[c]) {
                out.c_used[c] = true;
            }
            j += 1;
        }
        c += 1;
    }
    // the search list: the needs given, then the needs of each provider found
    let mut ns = 0;
    let mut i = 0;
    while i < want.len() {
        if !has(&out.search, ns, want[i]) {
            if ns < WANT_MAX {
                out.search[ns] = want[i];
                ns += 1;
            } else {
                out.truncated |= TRUNC_NEEDS;
            }
        }
        i += 1;
    }
    let mut w = 0;
    while w < ns && out.n < PROVIDER_MAX {
        let h = out.search[w];
        w += 1;
        let mut c = 0;
        while c < nc {
            let hit = !out.c_used[c] && matches!(stem_of(path_of(&out.cands[c])), Some(stem) if name_hash_bytes(stem) == h);
            if hit {
                out.c_used[c] = true;
                if read_provider(cards, c, out, 1, None) {
                    let k = out.n;
                    if let Ok(t) = Keys::parse(&out.tables[k][..out.table_len[k]]) {
                        let mut ni = 0;
                        while let Some(n) = t.words(tag::NEEDS, ni) {
                            ni += 1;
                            if n.2 & 1 != 0 && !is_runtime_service(n.0) && !has(&out.search, ns, n.0) {
                                if ns < WANT_MAX {
                                    out.search[ns] = n.0;
                                    ns += 1;
                                } else {
                                    out.truncated |= TRUNC_NEEDS;
                                }
                            }
                        }
                    }
                    out.n += 1;
                    break;
                }
            }
            c += 1;
        }
    }
    // needs left unsearched because every provider slot is taken: the table is full (the planner reports it)
    if w < ns && out.n >= PROVIDER_MAX {
        out.truncated |= TRUNC_TABLE;
    }
    // the port file is the provider of `port`, found by its firmware identity, after the services (it reuses the listing buffer)
    if has(&out.search, ns, name_hash_bytes(PORT_NAME)) {
        find_port(cards, fw, out);
    }
}

/// The store of the host tests: files by path, and a switch that makes every listing busy.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::vec::Vec;

    pub struct FakeCards {
        pub files: Vec<(Vec<u8>, Vec<u8>)>,
        pub busy: bool,
        staged: Vec<u8>,
    }

    impl Cards for FakeCards {
        unsafe fn list(&mut self, prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
            if self.busy {
                return dkrt_sdk::E_BUSY;
            }
            let n = dkrt_abi::mem::cstr_len(prefix, 64);
            let prefix = core::slice::from_raw_parts(prefix, n);
            let mut k = 0;
            for (name, bytes) in &self.files {
                if k < max as usize && name.starts_with(prefix) && name.len() < 64 {
                    let e = &mut *out.add(k);
                    e.path = [0; 64];
                    e.path[..name.len()].copy_from_slice(name);
                    e.size = bytes.len() as u32;
                    k += 1;
                }
            }
            k as i32
        }
        unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
            let n = dkrt_abi::mem::cstr_len(path, 64);
            let key = core::slice::from_raw_parts(path, n);
            match self.files.iter().find(|(name, _)| name == key) {
                Some((_, bytes)) => {
                    self.staged = bytes.clone();
                    Ok(Staged { ptr: self.staged.as_ptr(), size: self.staged.len() as u32 })
                }
                None => Err(dkrt_sdk::E_NOT_FOUND),
            }
        }
    }

    pub fn cards(files: Vec<(std::string::String, Vec<u8>)>) -> FakeCards {
        FakeCards { files: files.into_iter().map(|(n, b)| (n.into_bytes(), b)).collect(), busy: false, staged: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{cards, FakeCards};
    use super::*;
    use dkneeds::keys::{name_hash, write_table, Need, Pair, Table};
    use std::vec;
    use std::vec::Vec;

    const KEEP: Table<'static> =
        Table { needs: &[], uses: &[], keys: &[], kinds: &[], slots: &[], hooks: &[], exports: &[], resources: &[], provides: &[], bind: &[], fw: &[], names: &[], sites: &[], callback_order: None, service_imports: &[] };

    /// A role `role` file with `table` as it is.
    fn raw_file(role: u32, table: &[u8]) -> Vec<u8> {
        let off = 0xa0usize;
        let mut f = vec![0u8; off];
        f[dkneeds::H_ROLE as usize..][..4].copy_from_slice(&role.to_be_bytes());
        f[dkneeds::H_REG_OFF as usize..][..4].copy_from_slice(&(off as u32).to_be_bytes());
        f[dkneeds::H_REG_SIZE as usize..][..4].copy_from_slice(&(table.len() as u32).to_be_bytes());
        f.extend_from_slice(table);
        f
    }

    fn table_of_t(t: &Table) -> Vec<u8> {
        let mut b = vec![0u8; dkneeds::keys::table_size(t)];
        write_table(t, &mut b).unwrap();
        b
    }

    /// A provider file: a header of `role`, then the table of a service `name` that needs `needs`.
    fn file(role: u32, name: &str, needs: &[&str]) -> Vec<u8> {
        let ns: Vec<Need> = needs.iter().map(|n| Need { service: name_hash(n), min_version: 0, hard: true }).collect();
        raw_file(role, &table_of_t(&Table { needs: &ns, provides: &[Pair { a: name_hash(name), b: 0x010000 }], ..KEEP }))
    }

    fn store_with(files: &[(&str, u32, &[&str])]) -> FakeCards {
        cards(files.iter().map(|(name, role, needs)| (std::format!("services/{name}.dks"), file(*role, name, needs))).collect())
    }

    fn run(c: &mut FakeCards, want: &[&str]) -> Found {
        let want: Vec<u32> = want.iter().map(|n| name_hash(n)).collect();
        let mut out = Found::new();
        unsafe { find(c, &want, None, &mut out) };
        out
    }

    /// A port file (role 2) for the firmware `fw`: the table of the port file with one name and one site.
    fn port_file(fw: [u8; 8]) -> Vec<u8> {
        use dkneeds::keys::{NameRec, SiteRec};
        let names = [NameRec { hash: 1, addr: 0x100, n: 0, kind: 0, flags: 0 }];
        let sites = [SiteRec { hash: 2, addr: 0x200, len: 6, kind: 0, flags: 0, check: [0; 8] }];
        let t = Table { provides: &[Pair { a: name_hash("port"), b: 0x000100 }], fw: &[fw], names: &names, sites: &sites, ..KEEP };
        raw_file(2, &table_of_t(&t))
    }

    const FW_A: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    const FW_B: [u8; 8] = [9, 9, 9, 9, 9, 9, 9, 9];

    fn run_fw(c: &mut FakeCards, want: &[&str], fw: Option<[u8; 8]>) -> Found {
        let want: Vec<u32> = want.iter().map(|n| name_hash(n)).collect();
        let mut out = Found::new();
        unsafe { find(c, &want, fw, &mut out) };
        out
    }

    #[test]
    fn the_port_file_whose_fw_is_the_images_is_found_whatever_its_name_and_the_others_are_ignored() {
        let mut fs = cards(vec![
            ("ports/p-1.16.dkport".into(), port_file(FW_B)),
            ("ports/z-any-name.dkport".into(), port_file(FW_A)),
            ("ports/notes.txt".into(), port_file(FW_A)),
        ]);
        let out = run_fw(&mut fs, &["port"], Some(FW_A));
        assert_eq!(out.n, 1);
        assert_eq!(out.path(0), b"ports/z-any-name.dkport");
        // the planner's copy keeps the PROVIDES item and not the firmware entries
        let keys = Keys::parse(out.table(0)).unwrap();
        assert_eq!(keys.words(tag::PROVIDES, 0).unwrap().0, name_hash("port"));
        assert_eq!((keys.fw_count(), keys.site_count(), keys.name_count()), (0, 0, 0));
    }

    #[test]
    fn no_port_file_for_the_firmware_finds_no_provider_and_a_service_is_still_found() {
        let mut fs = cards(vec![("ports/p-1.16.dkport".into(), port_file(FW_B)), ("services/lfo.dks".into(), file(1, "lfo", &["port"]))]);
        // another firmware, and an image with no fw_id
        for fw in [Some(FW_A), None] {
            let out = run_fw(&mut fs, &["lfo"], fw);
            assert_eq!((out.n, out.path(0)), (1, &b"services/lfo.dks"[..]), "{fw:?}");
        }
        // with the matching port file the service and the port file are both providers
        fs.files.push((b"ports/p-1.17.dkport".to_vec(), port_file(FW_A)));
        let out = run_fw(&mut fs, &["lfo"], Some(FW_A));
        assert_eq!((out.n, out.path(0), out.path(1)), (2, &b"services/lfo.dks"[..], &b"ports/p-1.17.dkport"[..]));
    }

    #[test]
    fn a_service_file_in_ports_and_a_port_file_in_services_are_no_providers() {
        // a role 1 file in ports/ and a role 2 file in services/ (the stem `port`)
        let mut fs = cards(vec![("ports/a.dkport".into(), file(1, "port", &[])), ("services/port.dks".into(), port_file(FW_A))]);
        assert_eq!(run_fw(&mut fs, &["port"], Some(FW_A)).n, 0);
    }

    #[test]
    fn a_full_provider_table_sets_truncated_for_a_service_and_for_the_port_file() {
        let names: Vec<std::string::String> = (0..PROVIDER_MAX + 1).map(|i| std::format!("s{i}")).collect();
        let files: Vec<(&str, u32, &[&str])> = names.iter().map(|n| (n.as_str(), 1, &[][..])).collect();
        let want: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
        let out = run(&mut store_with(&files), &want);
        assert_eq!((out.n, out.truncated), (PROVIDER_MAX, TRUNC_TABLE), "one need too many");
        // exactly PROVIDER_MAX needs: nothing is left unsearched
        let out = run(&mut store_with(&files), &want[..PROVIDER_MAX]);
        assert_eq!((out.n, out.truncated), (PROVIDER_MAX, 0));
        // every slot taken by services and the port file wanted: it has no slot
        let mut fs = store_with(&files[..PROVIDER_MAX]);
        fs.files.push((b"ports/p.dkport".to_vec(), port_file(FW_A)));
        let mut w: Vec<&str> = want[..PROVIDER_MAX].to_vec();
        w.push("port");
        let out = run_fw(&mut fs, &w, Some(FW_A));
        assert_eq!((out.n, out.truncated), (PROVIDER_MAX, TRUNC_TABLE));
    }

    #[test]
    fn a_service_file_of_the_old_format_is_named_with_the_code_that_refuses_it() {
        let table = |abi: u32| {
            let stock = [0u8; 6];
            let binds = [dkneeds::keys::Bind { site: 1, addr: 0x1000, tramp: 0x20, stock: &stock }];
            let mut f = raw_file(1, &table_of_t(&Table { provides: &[Pair { a: name_hash("lfo"), b: 0x010000 }], bind: &binds, fw: &[[7; 8]], ..KEEP }));
            f.resize(0xa0 + f.len().max(0), 0);
            f[dkneeds::H_SVC_ABI as usize..][..4].copy_from_slice(&abi.to_be_bytes());
            f
        };
        // a Rust service carries the ABI word that the port file change moved, a C service has 0
        assert_eq!(stale(&table(0x1234_5678)), Some((dkrt_sdk::E_SVC_ABI, name_hash("lfo"))));
        assert_eq!(stale(&table(0)), Some((dkrt_sdk::E_FORMAT, name_hash("lfo"))));
        // the search does not take it, and a file of the new format is no stale file
        assert_eq!(run(&mut cards(vec![("services/lfo.dks".into(), table(1))]), &["lfo"]).n, 0);
        assert_eq!(stale(&file(1, "lfo", &[])), None);
    }

    #[test]
    fn no_services_directory_finds_nothing() {
        assert_eq!(run(&mut cards(vec![]), &["lfo"]).n, 0);
    }

    #[test]
    fn one_file_whose_stem_hashes_to_the_need_is_found_with_its_table() {
        let mut fs = store_with(&[("lfo", 1, &[]), ("other", 1, &[])]);
        let out = run(&mut fs, &["lfo"]);
        assert_eq!(out.n, 1);
        assert_eq!(out.path(0), b"services/lfo.dks");
        let keys = Keys::parse(out.table(0)).unwrap();
        assert_eq!(keys.words(tag::PROVIDES, 0).unwrap().0, name_hash("lfo"));
        assert_eq!(run(&mut fs, &["missing"]).n, 0);
    }

    #[test]
    fn the_needs_of_a_provider_are_searched_too_whatever_the_listing_order() {
        // "base" lists before "top": it is found only because the folder is listed once and remembered
        let mut fs = store_with(&[("top", 1, &["base", "runtime"]), ("base", 1, &[])]);
        let out = run(&mut fs, &["top"]);
        assert_eq!(out.n, 2);
        assert_eq!(out.path(0), b"services/top.dks");
        assert_eq!(out.path(1), b"services/base.dks");
    }

    /// The port file with `n` names: its table is over `TABLE_MAX` (12 bytes a name).
    fn big_port_file(n: usize) -> Vec<u8> {
        let names: Vec<dkneeds::keys::NameRec> =
            (0..n).map(|i| dkneeds::keys::NameRec { hash: i as u32 + 1, addr: 0x1000 + i as u32, n: 0, kind: 0, flags: 0 }).collect();
        raw_file(2, &table_of_t(&Table { provides: &[Pair { a: name_hash("port"), b: 0x000100 }], fw: &[FW_A], names: &names, ..KEEP }))
    }

    #[test]
    fn a_port_file_whose_full_table_is_over_the_table_limit_is_found_and_its_need_is_met() {
        let f = big_port_file(80);
        let full = u32::from_be_bytes(f[dkneeds::H_REG_SIZE as usize..][..4].try_into().unwrap()) as usize;
        assert!(full > TABLE_MAX, "{full}");
        let mut fs = cards(vec![("ports/big.dkport".into(), f)]);
        let out = run_fw(&mut fs, &["port"], Some(FW_A));
        assert_eq!(out.n, 1);
        // the planner's copy is the table without FW, FWNAMES and SITES (their tags are the highest), with its one PROVIDES
        let keys = Keys::parse(out.table(0)).unwrap();
        assert_eq!((keys.count(tag::FW), keys.name_count(), keys.site_count()), (0, 0, 0));
        assert_eq!(keys.count(tag::PROVIDES), 1);
        assert!(out.table(0).len() <= TABLE_MAX);
        let user = table_of_t(&Table { needs: &[Need { service: name_hash("port"), min_version: 0x000100, hard: true }], ..KEEP });
        // the graph of the user and the port file as the search kept it: the need is met
        let tables = [(dkrt_abi::package::PackageId(0), user.as_slice()), (dkrt_abi::package::PackageId(dkrt_abi::MAX_MODULES as u8), out.table(0))];
        let g = dkrt_lifecycle::PackageGraph::from_tables(&tables, &[]).unwrap();
        let order = g.bring_up_order(&[dkrt_abi::package::PackageId(0)], &dkrt_lifecycle::LiveSet::new()).unwrap();
        assert_eq!(order.as_slice(), [dkrt_abi::package::PackageId(dkrt_abi::MAX_MODULES as u8), dkrt_abi::package::PackageId(0)]);
    }


    #[test]
    fn a_stem_over_50_bytes_is_skipped_so_its_path_fits_a_selection_path() {
        let ok = "a".repeat(50);
        let long = "b".repeat(51);
        assert!(PATH_CAP < 64, "services/ + 50 + .dks + NUL fit 64 bytes");
        let mut fs = store_with(&[(&ok, 1, &[]), (&long, 1, &[])]);
        assert_eq!(run(&mut fs, &[&ok]).n, 1);
        assert_eq!(run(&mut fs, &[&long]).n, 0);
    }

    #[test]
    fn a_busy_listing_finds_nothing() {
        let mut fs = store_with(&[("lfo", 1, &[])]);
        fs.busy = true;
        assert_eq!(run(&mut fs, &["lfo"]).n, 0);
        fs.busy = false;
        assert_eq!(run(&mut fs, &["lfo"]).n, 1);
    }

    #[test]
    fn a_file_with_the_wrong_role_or_in_a_subfolder_is_skipped() {
        let mut fs = store_with(&[("lfo", 0, &[]), ("fx", 1, &[])]);
        assert_eq!(run(&mut fs, &["lfo"]).n, 0);
        assert_eq!(run(&mut fs, &["lfo", "fx"]).n, 1);
        let mut fs = cards(vec![("services/sub/lfo.dks".into(), file(1, "lfo", &[]))]);
        assert_eq!(run(&mut fs, &["lfo"]).n, 0);
    }

    fn table_bytes(provides: &[Pair]) -> Vec<u8> {
        table_of_t(&Table { provides, ..KEEP })
    }

    fn found_in(table: &[u8]) -> usize {
        run(&mut cards(vec![("services/bad.dks".into(), raw_file(1, table))]), &["bad"]).n
    }

    #[test]
    fn a_provider_with_a_malformed_or_wrong_table_is_skipped() {
        let one = [Pair { a: name_hash("bad"), b: 0x010000 }];
        assert_eq!(found_in(&table_bytes(&one)), 1);
        // a version 1 table
        let mut v1 = table_bytes(&one);
        v1[4..6].copy_from_slice(&1u16.to_be_bytes());
        assert_eq!(found_in(&v1), 0);
        // two PROVIDES items
        assert_eq!(found_in(&table_bytes(&[Pair { a: name_hash("bad"), b: 0x010000 }, Pair { a: name_hash("other"), b: 0 }])), 0);
        // a header that claims more entries than the table holds
        let mut more = table_bytes(&one);
        let n = u16::from_be_bytes([more[6], more[7]]);
        more[6..8].copy_from_slice(&(n + 1).to_be_bytes());
        assert_eq!(found_in(&more), 0);
        // bytes after the last entry
        let mut trailing = table_bytes(&one);
        trailing.extend_from_slice(&[0; 4]);
        assert_eq!(found_in(&trailing), 0);
    }

    #[test]
    fn a_folder_with_more_files_than_the_search_remembers_sets_truncated() {
        let names: Vec<std::string::String> = (0..CAND_MAX + 1).map(|k| std::format!("c{k}")).collect();
        let files: Vec<(&str, u32, &[&str])> = names.iter().map(|n| (n.as_str(), 1u32, &[][..])).collect();
        assert_eq!(run(&mut store_with(&files), &["c0"]).truncated, TRUNC_FILES);
        let mut fs = store_with(&files[..CAND_MAX]);
        assert_eq!(run(&mut fs, &["c0"]).truncated, 0);
        // more distinct needs than the search list holds
        let want: Vec<u32> = (0..WANT_MAX as u32 + 1).collect();
        let mut out = Found::new();
        unsafe { find(&mut fs, &want, None, &mut out) };
        assert_eq!(out.truncated, TRUNC_NEEDS);
    }

    #[test]
    fn the_search_stops_at_the_provider_cap() {
        let names: Vec<std::string::String> = (0..PROVIDER_MAX + 2).map(|k| std::format!("s{k}")).collect();
        let files: Vec<(&str, u32, &[&str])> = names.iter().map(|n| (n.as_str(), 1u32, &[][..])).collect();
        let want: Vec<&str> = names.iter().map(|n| n.as_str()).collect();
        assert_eq!(run(&mut store_with(&files), &want).n, PROVIDER_MAX);
    }
    /// The store of `FakeCards` with a count of the listings and files that cannot be staged.
    struct Counting {
        inner: FakeCards,
        lists: usize,
        unreadable: Vec<Vec<u8>>,
    }

    impl Cards for Counting {
        unsafe fn list(&mut self, prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
            self.lists += 1;
            self.inner.list(prefix, out, max)
        }
        unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
            let key = core::slice::from_raw_parts(path, dkrt_abi::mem::cstr_len(path, 64));
            if self.unreadable.iter().any(|p| p == key) {
                return Err(dkrt_sdk::E_NOT_FOUND);
            }
            self.inner.stage(path)
        }
    }

    fn counting(files: &[(&str, u32, &[&str])], unreadable: &[&str]) -> Counting {
        Counting { inner: store_with(files), lists: 0, unreadable: unreadable.iter().map(|n| std::format!("services/{n}.dks").into_bytes()).collect() }
    }

    fn run_counting(c: &mut Counting, want: &[&str]) -> Found {
        let want: Vec<u32> = want.iter().map(|n| name_hash(n)).collect();
        let mut out = Found::new();
        unsafe { find(c, &want, None, &mut out) };
        out
    }

    #[test]
    fn an_empty_search_does_not_list_the_folder_and_a_need_named_twice_finds_one_provider() {
        let mut c = counting(&[("lfo", 1, &[])], &[]);
        assert_eq!(run_counting(&mut c, &[]).n, 0);
        assert_eq!(c.lists, 0, "no need, no listing");
        let out = run_counting(&mut c, &["lfo", "lfo"]);
        assert_eq!((out.n, c.lists), (1, 1), "one listing, one provider");
    }

    #[test]
    fn only_a_file_services_stem_dks_is_a_candidate() {
        assert_eq!(stem_of(b"services/lfo.dks"), Some(&b"lfo"[..]));
        assert_eq!(stem_of(b"services/a.dks"), Some(&b"a"[..]));
        // another extension, no stem, a path as short as the folder and the extension, a file in a subfolder, a stem over the limit
        assert_eq!(stem_of(b"services/lfo.dkm"), None);
        assert_eq!(stem_of(b"services/lfo.dks.bak"), None);
        assert_eq!(stem_of(b"services/.dks"), None);
        assert_eq!(stem_of(b"lfo.dks"), None);
        assert_eq!(stem_of(b"services/a/b.dks"), None);
        let long = std::format!("services/{}.dks", "n".repeat(STEM_MAX + 1));
        assert_eq!(stem_of(long.as_bytes()), None);
        // a file of the folder with another extension is not found, whatever its stem
        let mut fs = cards(vec![("services/lfo.txt".into(), file(1, "lfo", &[]))]);
        assert_eq!(run(&mut fs, &["lfo"]).n, 0);
    }

    #[test]
    fn a_provider_whose_table_lies_outside_the_file_or_is_over_the_planner_limit_is_skipped() {
        let one = [Pair { a: name_hash("bad"), b: 0x010000 }];
        let good = raw_file(1, &table_bytes(&one));
        let store = |f: Vec<u8>| cards(vec![("services/bad.dks".into(), f)]);
        assert_eq!(run(&mut store(good.clone()), &["bad"]).n, 1);
        // the table offset is past the file end, and the table runs past it
        let mut past = good.clone();
        let end = past.len() as u32;
        past[dkneeds::H_REG_OFF as usize..][..4].copy_from_slice(&(end + 4).to_be_bytes());
        assert_eq!(run(&mut store(past), &["bad"]).n, 0);
        let mut runs_over = good.clone();
        let len = u32::from_be_bytes(runs_over[dkneeds::H_REG_SIZE as usize..][..4].try_into().unwrap());
        runs_over[dkneeds::H_REG_SIZE as usize..][..4].copy_from_slice(&(len + 4).to_be_bytes());
        assert_eq!(run(&mut store(runs_over), &["bad"]).n, 0);
        // a table of less than a head, and a file shorter than the header words
        let mut tiny = good.clone();
        tiny[dkneeds::H_REG_SIZE as usize..][..4].copy_from_slice(&4u32.to_be_bytes());
        assert_eq!(run(&mut store(tiny), &["bad"]).n, 0);
        assert_eq!(run(&mut store(good[..dkneeds::H_REG_SIZE as usize].to_vec()), &["bad"]).n, 0);
        // a table over the planner's limit without an entry to cut
        let needs: Vec<Need> = (0..TABLE_MAX as u32 / 8).map(|k| Need { service: 1000 + k, min_version: 0, hard: false }).collect();
        let big = table_of_t(&Table { needs: &needs, provides: &one, ..KEEP });
        assert!(big.len() > TABLE_MAX);
        assert_eq!(run(&mut store(raw_file(1, &big)), &["bad"]).n, 0);
    }

    #[test]
    fn a_file_that_cannot_be_staged_is_skipped_and_the_search_goes_on() {
        let mut c = counting(&[("a", 1, &[]), ("b", 1, &[])], &["a"]);
        let out = run_counting(&mut c, &["a", "b"]);
        assert_eq!(out.n, 1);
        assert_eq!(out.path(0), b"services/b.dks");
        assert!(out.table_len[0] > 0, "the slot of the failed read holds the next provider");
    }

    #[test]
    fn find_more_reports_a_busy_store_and_a_later_search_finds_the_provider() {
        let mut fs = store_with(&[("lfo", 1, &[])]);
        let want = [name_hash("lfo")];
        let mut out = Found::new();
        fs.busy = true;
        assert_eq!(unsafe { find_more(&mut fs, &want, None, &mut out) }, dkrt_sdk::E_BUSY);
        assert_eq!(out.n, 0);
        fs.busy = false;
        assert_eq!(unsafe { find_more(&mut fs, &want, None, &mut out) }, dkrt_sdk::OK);
        assert_eq!((out.n, out.path(0)), (1, &b"services/lfo.dks"[..]));
        // `find` keeps its rule: a busy store finds nothing without an error
        fs.busy = true;
        assert_eq!(run_fw(&mut fs, &["lfo"], None).n, 0);
    }

    #[test]
    fn a_second_search_does_not_add_the_port_provider_twice() {
        let mut fs = cards(vec![("ports/p.dkport".into(), port_file(FW_A)), ("services/lfo.dks".into(), file(1, "lfo", &["port"]))]);
        let mut out = Found::new();
        unsafe {
            find(&mut fs, &[name_hash("port")], Some(FW_A), &mut out);
            assert_eq!((out.n, out.path(0)), (1, &b"ports/p.dkport"[..]));
            // a service that needs the port is searched for afterwards: the port hash is in the search again
            assert_eq!(find_more(&mut fs, &[name_hash("lfo")], Some(FW_A), &mut out), dkrt_sdk::OK);
        }
        assert_eq!(out.n, 2);
        assert_eq!((out.path(0), out.path(1)), (&b"ports/p.dkport"[..], &b"services/lfo.dks"[..]));
    }
}
