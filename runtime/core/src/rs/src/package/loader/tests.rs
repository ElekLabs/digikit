//! The module lifecycle over fakes: `Loader<FakePlatform, FakeImages, FakeRegistry>` loads a small image, and every undo path gives back what the
//! step before it took. The image has no relocations, DSP section or records; the fakes log what the loader asks of the store, the arena, the module and
//! the registry.
use super::*;
use crate::fake_platform::FakePlatform;
use dkrt_abi::sha256::Sha;
use crate::test_lock::LOCK;
use dkrt_abi::profiles;
use std::collections::BTreeMap;
use std::vec::Vec;

// the seams between the real packed artifacts (`core/runtime:test-assembly`)
pub(crate) mod assembly_tests;
mod boot_effects_assembly_tests;
mod dsp_provider_tests;
mod golden;
mod host_boot_env;
mod host_ops;
mod manager_assembly_tests;
mod patch_assembly_tests;
mod park_tests;
mod seam_tests;
mod payloads;
mod check;
mod commit;
mod live;
mod load;
mod plan;
mod port;
mod provider_reload;
mod unload;

// ---- the single-record steps the loader tests drive ------------------------------------------------------------------------------------------------------
//
// The package manager runs a record's steps in its own order (`package.rs`, `lifecycle/`); the tests of the file checks and the role steps below drive the same step functions one
// record at a time over the loader's fakes, through the one-file load that `dkrt_load` still is. A take-down, a reload and a live load run through the manager (`adopt`).
impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// Load the service file at `path` into provider record `j`.
    pub(super) unsafe fn load_provider(&mut self, path: *const u8, j: usize) -> i32 {
        self.load_file(path, Slot::Provider(MAX_MODULES + j))
    }
}

/// The package manager over `l` with the records `active` taken as up, as `lifecycle::test_up` does for the cleanup tests (the single-record steps loaded them): the live job's steps
/// (`Rt::unload`, `reload_load_with`, `live_load_with`) then run through the manager and its seams, which are the production take-down and load.
unsafe fn adopt(l: &mut Fake, active: &[usize]) -> Box<crate::package::lifecycle::Rt<FakePlatform, FakeImages, FakeRegistry>> {
    let mut rt = Box::new(crate::package::lifecycle::Rt::new(l as *mut Fake));
    rt.init(&[None; MAX_MODULES], SEL_MAX).unwrap();
    for &r in active {
        rt.manager().unwrap().assume_active(dkrt_abi::package::PackageId(r as u8));
    }
    rt
}

const PATH: &[u8] = b"mods/a.dkm\0";
const IMAGE_OFF: usize = 160;
const FIRST_ADDR: u32 = 0x0010_0000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ev {
    Stage,
    Alloc(u32, u32),
    Free(u32),
    Init(usize, u32),
    Fini(usize, u32),
    Add(usize),
    AddProvider(usize, *const dkrt_abi::provider::ProviderRow, *mut u8),
    Remove(usize),
    AbiOk(u32),
}

/// The init of a provider asks for its listing client (`files::client`) and leaves the answer here.
static PROBE_FILES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PROBED_CLIENT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// What the init of the module does.
#[derive(Clone, Copy)]
enum Init {
    Ok,
    Fails(i32),
    /// Asks for the service `core/usb-bulk` and returns `E_MISSING`.
    NeedsUsbBulk,
    /// Declares what the header table of a service says: (declare tag, first word, entry offset in the image) for a hook or an export, as `init` of a service does.
    Declare(&'static [(u16, u32, u32)]),
    /// A service file: declares `test_row()`.
    DeclaresRow,
    /// Posts a worker job and adds a poll function at its own address, as the file's code would (`dk_job_post`, `dkrt_poll_add`), and returns `OK`.
    PostsJobAndPoll,
    /// Posts a worker job and lets one UI pass submit it, so it is on the worker (`RUNNING`) when `init` returns `OK`.
    PostsRunningJob,
}

unsafe extern "C" fn probe_step(_: *mut u8) -> i32 {
    0
}

/// The row a service file declares in these tests; `svc-test` is a `[[service]]` that no runtime row owns.
fn test_row() -> &'static dkrt_abi::provider::ProviderRow {
    static ROW: std::sync::OnceLock<&'static dkrt_abi::provider::ProviderRow> = std::sync::OnceLock::new();
    ROW.get_or_init(|| crate::package::fixture_provider::Nothing::named("svc-test").row())
}
/// The instance block of that row (its owner is zero-size).
static TEST_INSTANCE: [u8; 8] = [0; 8];
fn test_instance() -> *mut u8 {
    TEST_INSTANCE.as_ptr() as *mut u8
}

struct FakeImages {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    staged: Vec<u8>,
    /// Live blocks: address and memory (16-byte units, so the host pointer is aligned).
    live: Vec<(u32, Vec<u128>)>,
    allocs: u32,
    /// The n-th alloc (from 1) returns `None`.
    fail_alloc: Option<u32>,
    init: Init,
    log: Vec<Ev>,
    /// The selection index that `BOOT_IDX` held at each `stage` (the index the services read to find the module that loads).
    boot_at_stage: Vec<u32>,
    /// Every listing answers `E_BUSY` (the store lock is taken).
    busy: bool,
    /// The n-th (from 1) `stage` of this path fails with this code, and the count of stages of it so far.
    stage_fails: Option<(Vec<u8>, u32, i32)>,
    stage_count: u32,
}

/// The one import the fake runtime has.
const KNOWN_IMPORT: &[u8] = b"dk_alloc\0";
const KNOWN_ADDR: u32 = 0x4000;
/// The one service-only import the fake runtime has.
const SVC_IMPORT: &[u8] = b"dkc_device_id\0";
const SVC_ADDR: u32 = 0x5000;

impl Images for FakeImages {
    unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
        self.log.push(Ev::Stage);
        self.boot_at_stage.push(crate::package::boot_idx::get());
        let n = cstr_len(path, 64);
        let key = core::slice::from_raw_parts(path, n).to_vec();
        if let Some((p, nth, code)) = self.stage_fails.clone() {
            if p == key {
                self.stage_count += 1;
                if self.stage_count == nth {
                    return Err(code);
                }
            }
        }
        match self.files.get(&key) {
            None => Err(E_NOT_FOUND),
            Some(f) => {
                self.staged = f.clone();
                Ok(Staged { ptr: self.staged.as_ptr(), size: self.staged.len() as u32 })
            }
        }
    }
    unsafe fn import_addr(&mut self, name: *const u8) -> u32 {
        if cstr_eq(name, KNOWN_IMPORT.as_ptr()) {
            KNOWN_ADDR
        } else {
            0
        }
    }
    unsafe fn svc_import_addr(&mut self, name: *const u8) -> u32 {
        if cstr_eq(name, SVC_IMPORT.as_ptr()) {
            SVC_ADDR
        } else {
            0
        }
    }
    unsafe fn import_needs(&mut self, name: *const u8) -> Option<&'static str> {
        if cstr_eq(name, b"dk_usb_bulk_open\0".as_ptr()) {
            Some("usb-bulk")
        } else {
            None
        }
    }
    unsafe fn alloc(&mut self, n: u32) -> Option<Block> {
        self.allocs += 1;
        if self.fail_alloc == Some(self.allocs) {
            return None;
        }
        let addr = FIRST_ADDR * self.allocs;
        let mut mem = vec![0u128; (n as usize + 15) / 16];
        let ptr = mem.as_mut_ptr() as *mut u8;
        self.live.push((addr, mem));
        self.log.push(Ev::Alloc(addr, n));
        Some(Block { ptr, addr: dkrt_abi::dev_addr::DevAddr::new(addr) })
    }
    unsafe fn free(&mut self, addr: dkrt_abi::dev_addr::DevAddr) {
        let addr = addr.get();
        let at = self.live.iter().position(|b| b.0 == addr).expect("free of a block that is not live");
        self.live.remove(at);
        self.log.push(Ev::Free(addr));
    }
    unsafe fn list(&mut self, prefix: *const u8, out: *mut dkrt_sdk::DkrtStoreEntry, max: u32) -> i32 {
        if self.busy {
            return E_BUSY;
        }
        let n = cstr_len(prefix, 64);
        let prefix = core::slice::from_raw_parts(prefix, n);
        let mut k = 0;
        for (name, bytes) in &self.files {
            if k < max as usize && name.starts_with(prefix) {
                let e = &mut *out.add(k);
                e.path = [0; 64];
                e.path[..name.len()].copy_from_slice(name);
                e.size = bytes.len() as u32;
                k += 1;
            }
        }
        k as i32
    }
    fn image_extent(&mut self, _: u32, _: u32) -> core::ops::Range<usize> {
        // the rows of a host test are statics of the test binary, in no package image
        0..usize::MAX
    }
    unsafe fn enter_init(&mut self, rec: usize, init: u32, ctx: *const Ctx) -> i32 {
        self.log.push(Ev::Init(rec, init));
        order("init");
        if rec >= MAX_MODULES && PROBE_FILES.load(std::sync::atomic::Ordering::SeqCst) {
            // what `dk_files_open` of a service file asks of the runtime: the listing client of the running record, in its `init`
            crate::dlog::set_source_for_test(rec as u8 + 1);
            PROBED_CLIENT.store(crate::package::files::client(), std::sync::atomic::Ordering::SeqCst);
            crate::dlog::set_source_for_test(0);
        }
        match self.init {
            Init::Ok => OK,
            // (`ctx` is the context address as a u32: not a host pointer)
            Init::DeclaresRow => crate::package::collect::dk_provider_declare_row(core::ptr::addr_of_mut!(crate::package::entry::DKRT_CTXS[rec]) as *mut u8, test_row(), test_instance()),
            Init::Fails(e) => e,
            Init::PostsJobAndPoll => {
                assert!(crate::worker::post(rec as u8 + 1, Some(probe_step), None, core::ptr::null_mut()) > 0);
                assert_eq!(crate::api::dkrt_poll_add(init as usize), OK);
                OK
            }
            Init::PostsRunningJob => {
                assert!(crate::worker::post(rec as u8 + 1, Some(probe_step), None, core::ptr::null_mut()) > 0);
                crate::worker::tick();
                OK
            }
            Init::NeedsUsbBulk => {
                stub_missing("core/usb-bulk");
                E_MISSING
            }
            Init::Declare(items) => {
                // `init` is at image offset 0: its address is the base. The context pointer of the loader is 32 bits: on the host the test takes the real one.
                let _ = ctx;
                let ctx = core::ptr::addr_of_mut!(crate::package::entry::DKRT_CTXS[rec]) as *mut u8;
                // only a provider declares (a module in the same boot declares nothing)
                let items: &[(u16, u32, u32)] = if rec >= MAX_MODULES { items } else { &[] };
                for &(tag, word, entry) in items {
                    let at = (init + entry) as usize as *const u8;
                    let r = if tag == dkrt_abi::service::Declare::HOOK {
                        crate::package::collect::dk_service_declare_hook(ctx, word, at)
                    } else if tag == dkrt_abi::service::Declare::SLOT {
                        crate::package::collect::dk_service_declare_slot(ctx, word, 0, at)
                    } else {
                        crate::package::collect::dk_service_declare_export(ctx, word, at)
                    };
                    assert_eq!(r, OK);
                }
                OK
            }
        }
    }
    unsafe fn enter_fini(&mut self, rec: usize, fini: u32, _: *const Ctx) {
        self.log.push(Ev::Fini(rec, fini));
    }
}

/// What the fake services were asked at a module's lifecycle (`module_begin`, `module_commit`): the record number, and for a commit its phase, base and image size.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Svc {
    Begin(usize),
    Commit(usize, u32, u32, u32),
}

/// The order of lifecycle steps across the registry and the images: the fakes of both push here.
static ORDER: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

fn order(step: &'static str) {
    ORDER.lock().unwrap_or_else(|e| e.into_inner()).push(step);
}

/// What the fake service does in `module_commit`.
#[derive(Clone, Copy)]
struct FakeService {
    /// Body bytes of the one record (kind 0xdead) it writes; 0: no record.
    record: usize,
    /// More body bytes in the `WRITE` pass than it measured.
    skew: usize,
    /// Refuse in this phase with this code.
    refuse: Option<(u32, i32)>,
}

struct FakeRegistry {
    abi: bool,
    add_result: i32,
    log: Vec<Ev>,
    asked: core::cell::RefCell<Vec<Ev>>,
    svc: Vec<Svc>,
    service: FakeService,
}

impl Registry for FakeRegistry {
    fn abi_ok(&self, fingerprint: u32) -> bool {
        // `abi_ok` takes `&self`: the log is behind a cell
        self.asked.borrow_mut().push(Ev::AbiOk(fingerprint));
        self.abi
    }
    unsafe fn add(&mut self, rec: usize, row: Option<crate::package::provider_rows::RowSource<'_>>, _order: Option<u32>) -> i32 {
        self.log.push(match row {
            None => Ev::Add(rec),
            Some(crate::package::provider_rows::RowSource { row, instance, .. }) => Ev::AddProvider(rec, row as *const _, instance),
        });
        self.add_result
    }
    unsafe fn remove(&mut self, rec: usize) {
        self.log.push(Ev::Remove(rec));
    }
    unsafe fn module_begin(&mut self, rec: usize) {
        order("begin");
        self.svc.push(Svc::Begin(rec));
    }
    unsafe fn module_commit(&mut self, rec: usize, c: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
        order("commit");
        self.svc.push(Svc::Commit(rec, c.phase, c.base, c.image_size));
        let f = self.service;
        if let Some((phase, code)) = f.refuse {
            if phase == c.phase {
                return code;
            }
        }
        if f.record != 0 {
            let n = if c.phase == dkrt_abi::service::commit_phase::WRITE { f.record + f.skew } else { f.record };
            let sink = &mut *c.sink;
            sink.head(0xdead, 8 + f.record);
            sink.bytes(&vec![0u8; n]);
        }
        OK
    }
}

fn put(v: &mut [u8], off: u32, val: u32) {
    v[off as usize..off as usize + 4].copy_from_slice(&val.to_be_bytes());
}

/// Set the size field and the file hash after a change.
fn seal(v: &mut [u8]) {
    let n = v.len() as u32;
    put(v, dkneeds::H_SIZE, n);
    let mut sha = Sha::new();
    let mut got = [0u8; 32];
    unsafe {
        sha.update(v.as_ptr(), dkneeds::H_SHA);
        sha.update_zeros(32);
        sha.update(v.as_ptr().add(dkneeds::H_SHA as usize + 32), n - (dkneeds::H_SHA + 32));
        sha.finish(got.as_mut_ptr());
    }
    v[dkneeds::H_SHA as usize..dkneeds::H_SHA as usize + 32].copy_from_slice(&got);
}

/// A 32-byte image with `init` at 0 and `fini` at 4, an empty header table, the relocations `relocs` (offset, kind word) and the import names `imports`.
fn module_file_with(relocs: &[(u32, u32)], imports: &[&[u8]]) -> Vec<u8> {
    let reloc_off = IMAGE_OFF + 32;
    let import_off = reloc_off + relocs.len() * 8;
    let keys_off = import_off + imports.len() * dkneeds::CF_IMPORT_LEN as usize;
    let mut v = vec![0u8; keys_off + 8];
    put(&mut v, dkneeds::H_MAGIC, dkneeds::MAGIC);
    put(&mut v, dkneeds::H_FORMAT, 1);
    put(&mut v, dkneeds::H_HDRSIZE, dkneeds::HEADER);
    v[dkneeds::H_NAME as usize] = b'a';
    put(&mut v, dkneeds::H_IMAGE_OFF, IMAGE_OFF as u32);
    put(&mut v, dkneeds::H_IMAGE_SIZE, 32);
    put(&mut v, dkneeds::H_ALIGN, 4);
    put(&mut v, dkneeds::H_RELOC_OFF, reloc_off as u32);
    put(&mut v, dkneeds::H_RELOC_N, relocs.len() as u32);
    put(&mut v, dkneeds::H_IMPORT_OFF, import_off as u32);
    put(&mut v, dkneeds::H_IMPORT_N, imports.len() as u32);
    put(&mut v, dkneeds::H_INIT, 0);
    put(&mut v, dkneeds::H_FINI, 4);
    put(&mut v, dkneeds::H_MENU, dkneeds::NONE);
    put(&mut v, dkneeds::H_TICK, dkneeds::NONE);
    put(&mut v, dkneeds::H_REG_OFF, keys_off as u32);
    put(&mut v, dkneeds::H_REG_SIZE, 8);
    for (i, (off, kind)) in relocs.iter().enumerate() {
        put(&mut v, (reloc_off + i * 8) as u32, *off);
        put(&mut v, (reloc_off + i * 8 + 4) as u32, *kind);
    }
    for (i, name) in imports.iter().enumerate() {
        let at = import_off + i * dkneeds::CF_IMPORT_LEN as usize;
        v[at..at + name.len()].copy_from_slice(name);
    }
    put(&mut v, keys_off as u32, dkneeds::keys::MAGIC);
    put(&mut v, keys_off as u32 + 4, (dkneeds::keys::VERSION as u32) << 16);
    seal(&mut v);
    v
}

fn module_file() -> Vec<u8> {
    module_file_with(&[], &[])
}

type Fake = Loader<FakePlatform, FakeImages, FakeRegistry>;

/// A loader in the boot phase over one file, with the runtime tables cleared.
unsafe fn loader(init: Init) -> Fake {
    loader_of(init, module_file())
}

unsafe fn loader_of(init: Init, file: Vec<u8>) -> Fake {
    *st() = core::mem::zeroed();
    records::reset();
    *dsp::ds() = core::mem::zeroed();
    dsp::init_once();
    CALLED_NEED = [0; NEEDS_LEN];
    crate::package::cleanup_tests::lenient_tick_drop();
    let mut files = BTreeMap::new();
    files.insert(PATH[..PATH.len() - 1].to_vec(), file);
    let imgs = FakeImages { files, staged: Vec::new(), live: Vec::new(), allocs: 0, fail_alloc: None, init, log: Vec::new(), boot_at_stage: Vec::new(), busy: false, stage_fails: None, stage_count: 0 };
    let reg = FakeRegistry {
        abi: true,
        add_result: OK,
        log: Vec::new(),
        asked: Default::default(),
        svc: Vec::new(),
        service: FakeService { record: 0, skew: 0, refuse: None },
    };
    ORDER.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let mut l = Loader::new(FakePlatform::new(profiles::DT2), imgs, reg);
    l.phase = Phase::Boot;
    l
}

/// A service file (role 1) of `table` (a v2 header table), with `dsp` as its DSP section size word.
fn service_file(table: &dkneeds::keys::Table, dsp: u32) -> Vec<u8> {
    service_file_of(module_file(), table, dsp)
}

/// `service_file` over the module file `v`.
fn service_file_of(mut v: Vec<u8>, table: &dkneeds::keys::Table, dsp: u32) -> Vec<u8> {
    let keys_off = v.len() - 8;
    v.truncate(keys_off);
    let mut t = vec![0u8; dkneeds::keys::table_size(table)];
    dkneeds::keys::write_table(table, &mut t).unwrap();
    v.extend_from_slice(&t);
    put(&mut v, dkneeds::H_REG_SIZE, t.len() as u32);
    put(&mut v, dkneeds::H_ROLE, 1);
    put(&mut v, dkneeds::H_DSP_SIZE, dsp);
    seal(&mut v);
    v
}

const SITE: u32 = 0x400a_5eac;
const STOCK: [u8; 6] = [0x20, 0x52, 0x48, 0x78, 0x00, 0x02];
const SVC: &str = "svc-test";
/// The `fw_id` words of the image (the first 8 bytes of the stock firmware's SHA-256, as two big endian words) and of the port file made for it.
const FW_ID: [u8; 8] = [0xd1, 0x5e, 0xa5, 0xe0, 0x11, 0x22, 0x33, 0x44];

/// The table of a service `SVC` that exports `ping` (image offset 8) and takes one hook (`hook.a`, entry 4); the site comes from the port file.
fn svc_table<'a>() -> dkneeds::keys::Table<'a> {
    use dkneeds::keys::{name_hash, Pair, Table};
    static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    static EXPORTS: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    static HOOKS: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    Table {
        provides: PROVIDES.get_or_init(|| [Pair { a: name_hash(SVC), b: 0x000802 }]),
        exports: EXPORTS.get_or_init(|| [Pair { a: name_hash("dk_svctest_ping"), b: 8 }]),
        hooks: HOOKS.get_or_init(|| [Pair { a: name_hash("hook.a"), b: 4 }]),
        ..Default::default()
    }
}

/// What `init` of that service declares: its hook (entry 4) and its export (entry 8).
fn svc_init() -> Init {
    use dkneeds::keys::name_hash;
    let items: Vec<(u16, u32, u32)> =
        vec![(dkrt_abi::service::Declare::HOOK, name_hash("hook.a"), 4), (dkrt_abi::service::Declare::EXPORT, name_hash("dk_svctest_ping"), 8)];
    Init::Declare(Box::leak(items.into_boxed_slice()))
}

/// A loader over a service file in the provider record `j`: the stock bytes are at the site, and the port file of the image is loaded in the last provider record
/// (put there by hand: no block of the arena is taken, so the counts of the tests stay those of the service file alone). It lists the site of `hook.a`.
unsafe fn provider_loader(file: Vec<u8>, init: Init) -> Fake {
    let mut l = loader_of(init, file);
    l.p.put_code(SITE, &STOCK);
    set_fw_id(&l, Some(FW_ID));
    put_port(&mut l, &[(("hook.a", SITE, STOCK.len() as u16, dkneeds::keys::SITE_RAW, check8(&STOCK)), 0)]);
    l
}

/// The names of the task facts (`park.rs`) flagged absent, sorted as the port file lists them: a firmware without those tasks.
fn absent_task_names() -> Vec<dkneeds::keys::NameRec> {
    use dkneeds::keys::{name_hash_bytes, NameRec, FLAG_ABSENT, NAME_FW};
    let mut v: Vec<NameRec> = park::TASK_NAMES.iter().map(|n| NameRec { hash: name_hash_bytes(n), addr: 0, n: 0, kind: NAME_FW, flags: FLAG_ABSENT }).collect();
    v.sort_by_key(|n| n.hash);
    v
}

/// A port file in provider record `PROVIDER_MAX - 1` with the sites `((hook, address, length, kind, check value), flags)` and no name: its table copy is leaked.
unsafe fn put_port(l: &mut Fake, sites: &[((&str, u32, u16, u8, [u8; 8]), u8)]) {
    // the firmware of these tests has neither the BgWorker nor the console task: the installer has no task to wait for (the gate has its own tests)
    put_port_names(l, sites, &absent_task_names());
}

/// [`put_port`] with the names `names` (sorted by hash) in the table.
unsafe fn put_port_names(l: &mut Fake, sites: &[((&str, u32, u16, u8, [u8; 8]), u8)], names: &[dkneeds::keys::NameRec]) {
    use dkneeds::keys::{name_hash, write_table, Pair, SiteRec, Table};
    let recs: Vec<SiteRec> = sites.iter().map(|&((h, addr, len, kind, check), flags)| SiteRec { hash: name_hash(h), addr, len, kind, flags, check }).collect();
    let t = Table { provides: &[Pair { a: name_hash("port"), b: 0x000100 }], fw: &[FW_ID], names, sites: &recs, ..Default::default() };
    let mut bytes = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut bytes).unwrap();
    let j = PROVIDER_MAX - 1;
    let bytes: &'static mut [u8] = Box::leak(bytes.into_boxed_slice());
    l.provs[j] = ProvRec { block: Some(Block { ptr: bytes.as_mut_ptr(), addr: dkrt_abi::dev_addr::DevAddr::NULL }), len: bytes.len() as u32, image_size: 16, ..ProvRec::EMPTY };
    l.roles[MAX_MODULES + j] = 2;
    records::module(MAX_MODULES + j).used = 1;
    records::module(MAX_MODULES + j).base = 0x7000_0000;
}

/// The `fw_id` entry of the image's info table (a data table of two words): the first 8 bytes of the stock firmware's SHA-256; `None`: the image has none.
fn set_fw_id(l: &Fake, id: Option<[u8; 8]>) {
    let h = dkneeds::keys::name_hash("fw_id");
    l.p.info_entries.borrow_mut().retain(|(x, _)| *x != h);
    if let Some(id) = id {
        let w = |i: usize| u32::from_be_bytes([id[i], id[i + 1], id[i + 2], id[i + 3]]);
        l.p.info_entries.borrow_mut().push((h, vec![w(0), w(4)]));
        l.p.info_data.borrow_mut().push(h);
    }
}


fn site_bytes(l: &Fake, n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    unsafe { l.p.read_code(SITE, &mut out) };
    out
}

/// A service file of a table with a provides pair only (no hook, no export): `init` declares just its row.
fn row_service_file() -> Vec<u8> {
    use dkneeds::keys::{name_hash, Pair, Table};
    static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    let table = Table { provides: PROVIDES.get_or_init(|| [Pair { a: name_hash(SVC), b: 0x000802 }]), ..Default::default() };
    service_file(&table, 0)
}

/// A module file (role 0) whose header table is `table`, with the relocation `(12, import 0)` of the import `dk_svctest_ping`.
fn consumer_file(table: &dkneeds::keys::Table) -> Vec<u8> {
    consumer_file_with_import(table, b"dk_svctest_ping")
}

fn consumer_file_with_import(table: &dkneeds::keys::Table, name: &[u8]) -> Vec<u8> {
    let mut v = module_file_with(&[(12, 1 << 24)], &[name]);
    let keys_off = v.len() - 8;
    v.truncate(keys_off);
    let mut t = vec![0u8; dkneeds::keys::table_size(table)];
    dkneeds::keys::write_table(table, &mut t).unwrap();
    v.extend_from_slice(&t);
    put(&mut v, dkneeds::H_REG_SIZE, t.len() as u32);
    v[IMAGE_OFF + 15] = 4;
    seal(&mut v);
    v
}

/// The boot record of the selection `paths` (NUL terminated store paths), the boot info as the hook leaves it, and the package manager over `l`: the boot has run when this returns.
/// Boxed, so the seams' pointers into the lifecycle stay valid.
unsafe fn boot_selection(l: &mut Fake, paths: &[&[u8]]) -> Box<crate::package::lifecycle::Rt<FakePlatform, FakeImages, FakeRegistry>> {
    let mut rec = dkrt_sdk::sel::Record::empty();
    rec.count = paths.len() as u32;
    for (k, p) in paths.iter().enumerate() {
        rec.paths[k][..p.len()].copy_from_slice(p);
    }
    *crate::boot::info() = core::mem::zeroed();
    crate::boot::info().count = paths.len() as u32;
    crate::boot::states().begin();
    let mut rt = Box::new(crate::package::lifecycle::Rt::new(l as *mut Fake));
    rt.boot_with(&rec, &wide_caps());
    rt
}

/// A capacity with room for everything the tests take.
fn wide_caps() -> Vec<DkrtCap> {
    use dkrt_sdk::proto::*;
    [CAPK_MODULES, CAPK_SELECTION, CAPK_MACHINE_ROWS, CAPK_PARAM_ROWS, CAPK_PAGE_ROWS, CAPK_ARENA]
        .iter()
        .map(|k| DkrtCap { kind: *k as u32, limit: 1 << 20, used: 0, largest: 1 << 20 })
        .collect()
}

const SVC_PATH: &[u8] = b"services/svc-test.dks";

/// A loader in the boot phase whose store holds the provider `svc` at `services/svc-test.dks` and whose selection is the one module `consumer` at `PATH`.
unsafe fn staged_boot(svc: Vec<u8>, consumer: Vec<u8>) -> Fake {
    let mut l = provider_loader(consumer.clone(), svc_init());
    let mut at = SVC_PATH.to_vec();
    l.m.files.insert(at.clone(), svc.clone());
    at.clear();
    l.reset_plan();
    l.capture(0, &consumer);
    l
}

fn user_table() -> dkneeds::keys::Table<'static> {
    static NEEDS: std::sync::OnceLock<[dkneeds::keys::Need; 1]> = std::sync::OnceLock::new();
    dkneeds::keys::Table {
        needs: NEEDS.get_or_init(|| [dkneeds::keys::Need { service: dkneeds::keys::name_hash(SVC), min_version: 0x000802, hard: true }]),
        ..Default::default()
    }
}

/// Resets the loader's `TABLE_BAD` flag when a test that sets it ends, also when it fails.
struct TableBadGuard;

impl Drop for TableBadGuard {
    fn drop(&mut self) {
        unsafe { *core::ptr::addr_of_mut!(crate::package::invariants::DKRT_TABLE_BAD) = 0 };
    }
}

/// The bytes of a header table that has only `needs`.
fn needs_table(needs: &[dkneeds::keys::Need]) -> Vec<u8> {
    let t = dkneeds::keys::Table { needs, ..Default::default() };
    let mut b = vec![0u8; dkneeds::keys::table_size(&t)];
    dkneeds::keys::write_table(&t, &mut b).unwrap();
    b
}

/// A service whose table holds `slots` (one `SLOTS` item of the entry `entry`) and whose `init` declares what the table says.
unsafe fn slot_provider(entry: u32) -> Fake {
    use dkneeds::keys::{name_hash, Slot, Table};
    let slots: &'static [Slot] = Box::leak(Box::new([Slot { id: name_hash("the.entry"), flags: 0, entry }]));
    let table = Table { slots, ..svc_table() };
    let mut items = vec![(dkrt_abi::service::Declare::SLOT, name_hash("the.entry"), entry)];
    items.extend_from_slice(match svc_init() {
        Init::Declare(i) => i,
        _ => unreachable!(),
    });
    provider_loader(service_file(&table, 0), Init::Declare(Box::leak(items.into_boxed_slice())))
}

/// What a refused provider leaves behind: nothing allocated, nothing published or installed, the site stock and the record free.
unsafe fn assert_provider_undone(l: &Fake, published: usize) {
    assert!(l.m.live.is_empty(), "every block is free");
    assert_eq!((l.installer.count(), l.exports.count()), (0, published));
    assert_eq!(site_bytes(l, 6), STOCK);
    assert_eq!(records::module(MAX_MODULES).used, 0);
    assert!(l.provs[0].block.is_none());
}

/// The service of `slot_provider` with the flags of its one `SLOTS` item given, and a `init` that declares the slot only when `declared`.
unsafe fn masked_slot_provider(flags: u32, declared: bool) -> Fake {
    use dkneeds::keys::{name_hash, Slot, Table};
    let slots: &'static [Slot] = Box::leak(Box::new([Slot { id: name_hash("the.entry"), flags, entry: 4 }]));
    let table = Table { slots, ..svc_table() };
    let mut items = vec![];
    if declared {
        items.push((dkrt_abi::service::Declare::SLOT, name_hash("the.entry"), 4));
    }
    items.extend_from_slice(match svc_init() {
        Init::Declare(i) => i,
        _ => unreachable!(),
    });
    provider_loader(service_file(&table, 0), Init::Declare(Box::leak(items.into_boxed_slice())))
}

const PATH_B: &[u8] = b"mods/b.dkm\0";

/// A module file named `name` with a tick at image offset 8 and the machine keys `keys` in its header table.
fn keyed_file(name: u8, keys: &[u32]) -> Vec<u8> {
    use dkneeds::keys::{write_table, KeyGroup, Table};
    let mut v = module_file();
    v.truncate(v.len() - 8);
    let t = Table { keys: &[KeyGroup { kind: dkneeds::KIND_MACHINE, keys }], ..Default::default() };
    let at = v.len();
    v.resize(at + dkneeds::keys::table_size(&t), 0);
    write_table(&t, &mut v[at..]).unwrap();
    let n = (v.len() - at) as u32;
    put(&mut v, dkneeds::H_REG_SIZE, n);
    put(&mut v, dkneeds::H_TICK, 8);
    v[dkneeds::H_NAME as usize] = name;
    seal(&mut v);
    v
}

/// What a boot leaves: module `file` loaded at selection index `k`, its table kept and the plan's acceptance set.
unsafe fn booted(l: &mut Fake, k: usize, path: &[u8]) {
    l.phase = Phase::Boot;
    crate::package::boot_idx::set(k as u32);
    let staged = l.m.stage(path.as_ptr()).unwrap();
    let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize).to_vec();
    l.capture(k, &file);
    l.accepted[k] = true;
    assert_eq!(l.load(path.as_ptr()), OK);
    crate::package::boot_idx::set(u32::MAX);
}

/// A loader with module `a` (keys 1, 2) live in record 0 at selection index 0 and the Boot phase over.
unsafe fn live_a() -> Fake {
    let mut l = loader_of(Init::Ok, keyed_file(b'a', &[1, 2]));
    l.reset_plan();
    booted(&mut l, 0, PATH);
    l.phase = Phase::Closed;
    l
}

fn wide() -> Vec<DkrtCap> {
    let mut c = wide_caps();
    live_caps(&mut c, (0, 0));
    c
}

unsafe extern "C" fn noop_cmd(_: u32, _: u32, _: *const u8, _: u32, _: *mut u8) -> i32 {
    0
}

const PATH_C: &[u8] = b"mods/c.dkm\0";

/// What a live job leaves of module `a` in a closed loader: a loader with `a` live at index 0 and a second file `b` (key 9) in the store at `PATH_B`.
unsafe fn live_a_with_b() -> Fake {
    let mut l = live_a();
    l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
    l
}

const PORT_PATH: &[u8] = b"ports/x-1.17.dkport\0";
const IMP_PATH: &[u8] = b"services/imp.dks\0";
/// The hook and the name the port file lists.
const HOOK_H: &str = "hook.a";
const NAME_H: &str = "cf.thing";

/// The first 8 bytes of the SHA-256 of `b`: the check value of a site.
fn check8(b: &[u8]) -> [u8; 8] {
    let mut sha = Sha::new();
    let mut out = [0u8; 32];
    unsafe {
        sha.update(b.as_ptr(), b.len() as u32);
        sha.finish(out.as_mut_ptr());
    }
    out[..8].try_into().unwrap()
}

/// A port file for `FW_ID`: the name `NAME_H` at 0x1234, an absent name, and the sites `HOOK_H` (a tail copy of `STOCK` at `SITE`), a raw one and an absent one.
/// The image is the `rts` and 30 bytes; `bss` holds the copy.
fn port_file_with(bss: u32, tweak: impl Fn(&mut Vec<dkneeds::keys::SiteRec>)) -> Vec<u8> {
    use dkneeds::keys::{name_hash, NameRec, Pair, SiteRec, Table, FLAG_ABSENT, SITE_RAW, SITE_TAIL};
    let names = [
        NameRec { hash: name_hash(NAME_H), addr: 0x1234, n: 0, kind: 0, flags: 0 },
        NameRec { hash: name_hash("cf.other"), addr: 0, n: 0, kind: 0, flags: FLAG_ABSENT },
        NameRec { hash: name_hash(HOOK_H), addr: SITE + 6, n: 0, kind: 1, flags: 0 },
    ];
    let mut sites = vec![
        SiteRec { hash: name_hash(HOOK_H), addr: SITE, len: 6, kind: SITE_TAIL, flags: 0, check: check8(&STOCK) },
        SiteRec { hash: name_hash("hook.raw"), addr: 0x400b_0000, len: 6, kind: SITE_RAW, flags: 0, check: [0; 8] },
        SiteRec { hash: name_hash("hook.gone"), addr: 0, len: 0, kind: SITE_RAW, flags: FLAG_ABSENT, check: [0; 8] },
    ];
    tweak(&mut sites);
    let provides = [Pair { a: name_hash("port"), b: 0x000100 }];
    let table = Table { provides: &provides, fw: &[FW_ID], names: &names, sites: &sites, ..Default::default() };
    let mut v = service_file_of(module_file(), &table, 0);
    put(&mut v, dkneeds::H_ROLE, 2);
    put(&mut v, dkneeds::H_BSS, bss);
    put(&mut v, dkneeds::H_INIT, dkneeds::NONE);
    put(&mut v, dkneeds::H_FINI, dkneeds::NONE);
    v[IMAGE_OFF] = 0x4e;
    v[IMAGE_OFF + 1] = 0x75;
    seal(&mut v);
    v
}

fn port_file() -> Vec<u8> {
    port_file_with(12, |_| {})
}

/// A loader with the port file at `PORT_PATH` and the stock bytes at the site.
unsafe fn port_loader(file: Vec<u8>) -> Fake {
    let mut l = loader_of(Init::Ok, Vec::new());
    l.m.files.clear();
    l.m.files.insert(PORT_PATH[..PORT_PATH.len() - 1].to_vec(), file);
    l.p.put_code(SITE, &STOCK);
    set_fw_id(&l, Some(FW_ID));
    l
}

/// A service file of `provides` only, with the firmware imports `names` (each word of the 32-byte image at 4, 8, ... holds 4 and is relocated against the import).
fn importing_service(names: &[&str]) -> Vec<u8> {
    use dkneeds::keys::{name_hash, Pair, Table};
    static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    let table = Table { provides: PROVIDES.get_or_init(|| [Pair { a: name_hash(SVC), b: 0x000802 }]), ..Default::default() };
    let imports: Vec<Vec<u8>> = names.iter().map(|n| n.as_bytes().to_vec()).collect();
    let refs: Vec<&[u8]> = imports.iter().map(|v| &v[..]).collect();
    let relocs: Vec<(u32, u32)> = (0..names.len()).map(|i| (4 + 4 * i as u32, 1 << 24 | i as u32)).collect();
    let mut f = module_file_with(&relocs, &refs);
    for i in 0..names.len() {
        f[IMAGE_OFF + 7 + 4 * i] = 4;
    }
    service_file_of(f, &table, 0)
}

fn fw_import(prefix: &str, name: &str) -> String {
    std::format!("{prefix}{:08x}", dkneeds::keys::name_hash(name))
}
