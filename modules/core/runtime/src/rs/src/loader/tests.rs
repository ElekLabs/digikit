//! The module lifecycle over fakes: `Loader<FakePlatform, FakeImages, FakeRegistry>` loads a small image, and every undo path gives back what the
//! step before it took. The image has no relocations, DSP section or records; the fakes log what the loader asks of the store, the arena, the module and
//! the registry.
use super::*;
use crate::fake_platform::FakePlatform;
use crate::sha256::Sha;
use crate::test_lock::LOCK;
use dkrt_abi::profiles;
use std::collections::BTreeMap;
use std::vec::Vec;

// the seams between the real packed artifacts (`core/runtime:test-assembly`)
mod assembly_tests;
mod dsp_provider_tests;

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
    /// `add` with the service row of a file.
    AddRow(usize, *const dkrt_abi::service::ServiceRow),
    Remove(usize),
    AbiOk(u32),
}

/// What the init of the module does.
#[derive(Clone, Copy)]
enum Init {
    Ok,
    Fails(i32),
    /// Asks for the service `core/usb-bulk` and returns `E_MISSING`.
    NeedsUsbBulk,
    /// Declares what the header table of a service says: (declare tag, first word, entry offset in the image) for a hook or an export, as `init` of a service does.
    Declare(&'static [(u16, u32, u32)]),
    /// A service file: declares `TEST_ROW`.
    DeclaresRow,
}

struct Nothing;
impl<C: dkrt_abi::core_api::CoreApi> dkrt_abi::service::Service<C> for Nothing {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_reload!();
}
/// The row a service file declares in these tests; `svc-test` is a `[[service]]` that no runtime row owns.
static TEST_ROW: dkrt_abi::service::ServiceRow = dkrt_abi::service::ServiceRow::of::<Nothing, crate::core_api::RealCore>("svc-test", &[]);

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
        let n = cstr_len(path, 64);
        let key = core::slice::from_raw_parts(path, n).to_vec();
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
        Some(Block { ptr, addr })
    }
    unsafe fn free(&mut self, addr: u32) {
        let at = self.live.iter().position(|b| b.0 == addr).expect("free of a block that is not live");
        self.live.remove(at);
        self.log.push(Ev::Free(addr));
    }
    unsafe fn list(&mut self, prefix: *const u8, out: *mut dkrt_sdk::DkrtStoreEntry, max: u32) -> i32 {
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
    unsafe fn enter_init(&mut self, rec: usize, init: u32, ctx: *const Ctx) -> i32 {
        self.log.push(Ev::Init(rec, init));
        order("init");
        match self.init {
            Init::Ok => OK,
            // (`ctx` is the context address as a u32: not a host pointer)
            Init::DeclaresRow => crate::collect::dk_service_declare_row(core::ptr::addr_of_mut!(crate::entry::DKRT_CTXS[rec]) as *mut u8, &TEST_ROW),
            Init::Fails(e) => e,
            Init::NeedsUsbBulk => {
                stub_missing("core/usb-bulk");
                E_MISSING
            }
            Init::Declare(items) => {
                // `init` is at image offset 0: its address is the base. The context pointer of the loader is 32 bits: on the host the test takes the real one.
                let _ = ctx;
                let ctx = core::ptr::addr_of_mut!(crate::entry::DKRT_CTXS[rec]) as *mut u8;
                // only a provider declares (a module in the same boot declares nothing)
                let items: &[(u16, u32, u32)] = if rec >= MAX_MODULES { items } else { &[] };
                for &(tag, word, entry) in items {
                    let at = (init + entry) as usize as *const u8;
                    let r = if tag == dkrt_abi::service::Declare::HOOK {
                        crate::collect::dk_service_declare_hook(ctx, word, at)
                    } else if tag == dkrt_abi::service::Declare::SLOT {
                        crate::collect::dk_service_declare_slot(ctx, word, 0, at)
                    } else {
                        crate::collect::dk_service_declare_export(ctx, word, at)
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
    unsafe fn add(&mut self, rec: usize, row: Option<&dkrt_abi::service::ServiceRow>) -> i32 {
        self.log.push(match row {
            None => Ev::Add(rec),
            Some(r) => Ev::AddRow(rec, r as *const _),
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
    crate::cleanup_tests::lenient_tick_drop();
    let mut files = BTreeMap::new();
    files.insert(PATH[..PATH.len() - 1].to_vec(), file);
    let imgs = FakeImages { files, staged: Vec::new(), live: Vec::new(), allocs: 0, fail_alloc: None, init, log: Vec::new() };
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

#[test]
fn a_module_loads_and_unloads_and_gives_back_every_block() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let base = FIRST_ADDR;
        let s = st();
        assert_eq!((s.modules[0].used, s.modules[0].base, s.loads), (1, base, 1));
        assert_eq!(s.modules[0].fini, base + 4);
        // the module block stays; the record block of init was given back after the commit
        assert_eq!(l.m.live.iter().map(|b| b.0).collect::<Vec<_>>(), vec![base]);
        assert_eq!(l.m.log[0], Ev::Stage);
        assert!(l.m.log.contains(&Ev::Init(0, base)));
        assert_eq!(l.r.log, vec![Ev::Add(0)]);

        assert_eq!(l.unload_rec(0), OK);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        let fini = l.m.log.iter().position(|e| *e == Ev::Fini(0, base + 4)).expect("fini ran");
        let free = l.m.log.iter().position(|e| *e == Ev::Free(base)).expect("the module block is freed");
        assert!(fini < free, "fini runs before the memory goes");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
    }
}

#[test]
fn a_failed_init_undoes_the_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Fails(E_INIT));
        assert_eq!(l.load(PATH.as_ptr()), E_INIT);
        assert!(l.m.live.is_empty(), "the module block and the record block are free");
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(st().loads, 0);
        assert_eq!(l.r.log, vec![Ev::Remove(0)], "nothing was added, the undo still asks the registry");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))), "a module whose init failed gets no fini");
    }
}

#[test]
fn a_failed_commit_undoes_the_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.r.add_result = E_NO_REGISTRY;
        assert_eq!(l.load(PATH.as_ptr()), E_NO_REGISTRY);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        // the record can be used again
        l.r.add_result = OK;
        l.m.log.clear();
        l.r.log.clear();
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(st().modules[0].used, 1);
    }
}

#[test]
fn a_need_of_init_is_returned_as_the_missing_service() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::NeedsUsbBulk);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(&l.missing()[..8], b"usb-bulk");
        assert!(l.m.live.is_empty());
        assert_eq!(l.r.log, vec![Ev::Remove(0)]);
    }
}

#[test]
fn an_arena_that_is_full_gives_no_memory_error_and_keeps_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the module block
        let mut l = loader(Init::Ok);
        l.m.fail_alloc = Some(1);
        assert_eq!(l.load(PATH.as_ptr()), E_NOMEM);
        assert!(l.m.live.is_empty());
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(st().modules[0].used, 0);
        // the record block of init: the module block goes again
        let mut l = loader(Init::Ok);
        l.m.fail_alloc = Some(2);
        assert_eq!(l.load(PATH.as_ptr()), E_NOMEM);
        assert!(l.m.live.is_empty());
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(st().modules[0].used, 0);
    }
}

#[test]
fn a_load_outside_the_boot_or_of_a_missing_file_touches_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.phase = Phase::Closed;
        assert_eq!(l.load(PATH.as_ptr()), E_BOOT_ONLY);
        assert!(l.m.log.is_empty());
        l.phase = Phase::Boot;
        assert_eq!(l.load(b"mods/none.dkm\0".as_ptr()), E_NOT_FOUND);
        assert_eq!(l.m.log, vec![Ev::Stage]);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_damaged_file_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        let f = l.m.files.get_mut(&PATH[..PATH.len() - 1].to_vec()).unwrap();
        f[IMAGE_OFF + 2] ^= 1; // the file hash no longer matches
        assert_eq!(l.load(PATH.as_ptr()), E_SHA);
        assert_eq!(l.m.allocs, 0);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_module_file_with_provides_or_hooks_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        for hooks_only in [false, true] {
            let mut table = svc_table(&[]);
            if hooks_only {
                table.provides = &[];
            }
            let mut file = service_file(&table, 0);
            put(&mut file, dkneeds::H_ROLE, 0);
            seal(&mut file);
            let mut l = loader_of(Init::Ok, file);
            assert_eq!(l.load(PATH.as_ptr()), E_FORMAT, "hooks_only={hooks_only}");
            assert_eq!(l.m.allocs, 0);
            assert!(l.r.log.is_empty());
        }
    }
}

#[test]
fn relocations_name_the_block_or_an_import_of_the_runtime() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // word at 8 holds 0x20 (relative to the block), word at 12 holds 4 (plus the import)
        let mut file = module_file_with(&[(8, 0), (12, 1 << 24)], &[b"dk_alloc"]);
        file[IMAGE_OFF + 11] = 0x20;
        file[IMAGE_OFF + 15] = 4;
        seal(&mut file);
        let mut l = loader_of(Init::Ok, file);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let mem = &l.m.live[0].1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(8)), FIRST_ADDR + 0x20);
        assert_eq!(rd32(words.as_ptr().add(12)), KNOWN_ADDR + 4);
    }
}

#[test]
fn an_import_of_a_module_that_is_not_there_names_it_and_allocates_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[b"dk_usb_bulk_open"]));
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(&l.missing()[..8], b"usb-bulk");
        assert_eq!(l.m.allocs, 0);
        // an import nobody knows is refused without a name
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[b"dk_nothing"]));
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(l.missing()[0], 0);
    }
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
/// The `FW` item of the service and the `fw_id` words of the image it matches (the first 8 bytes of the stock firmware's SHA-256, as two big endian words).
const FW_ID: [u8; 8] = [0xd1, 0x5e, 0xa5, 0xe0, 0x11, 0x22, 0x33, 0x44];
const FW_ITEMS: &[[u8; 8]] = &[FW_ID];

/// The table of a service `SVC` that exports `ping` (image offset 8) and binds one hook at `SITE` (handler at 4, trampoline at 16).
fn svc_table<'a>(bind: &'a [dkneeds::keys::Bind<'a>]) -> dkneeds::keys::Table<'a> {
    use dkneeds::keys::{name_hash, Pair, Table};
    static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    static EXPORTS: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    static HOOKS: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    Table {
        provides: PROVIDES.get_or_init(|| [Pair { a: name_hash(SVC), b: 0x000802 }]),
        exports: EXPORTS.get_or_init(|| [Pair { a: name_hash("dk_svctest_ping"), b: 8 }]),
        hooks: HOOKS.get_or_init(|| [Pair { a: name_hash("hook.a"), b: 4 }]),
        bind,
        fw: FW_ITEMS,
        ..Default::default()
    }
}

fn svc_bind(stock: &[u8]) -> Vec<dkneeds::keys::Bind<'_>> {
    vec![dkneeds::keys::Bind { site: dkneeds::keys::name_hash("hook.a"), addr: SITE, tramp: 16, stock }]
}

/// What `init` of that service declares: its hook (entry 4) and its export (entry 8).
fn svc_init() -> Init {
    use dkneeds::keys::name_hash;
    let items: Vec<(u16, u32, u32)> =
        vec![(dkrt_abi::service::Declare::HOOK, name_hash("hook.a"), 4), (dkrt_abi::service::Declare::EXPORT, name_hash("dk_svctest_ping"), 8)];
    Init::Declare(Box::leak(items.into_boxed_slice()))
}

/// A loader over a service file in the provider record `j`: the stock bytes are at the site.
unsafe fn provider_loader(file: Vec<u8>, init: Init) -> Fake {
    let l = loader_of(init, file);
    l.p.put_code(SITE, &STOCK);
    set_fw_id(&l, Some(FW_ID));
    l
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

#[test]
fn a_provider_loads_into_its_own_record_publishes_its_exports_and_installs_its_hooks_last() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 1), OK);
        let rec = MAX_MODULES + 1;
        let base = records::module(rec).base;
        assert_eq!((records::module(rec).used, l.roles[rec], st().loads), (1, 1, 0), "a provider is no module of the count");
        assert_eq!(st().modules[0].used, 0, "no module record is taken");
        // the record is the host-visible DKRT_PROVIDERS slot 1: used, named, with the load base
        let published = &*core::ptr::addr_of!(records::DKRT_PROVIDERS);
        let name = &published[1].name;
        assert_eq!((published[1].used, published[1].base), (1, base));
        assert_ne!(name[0], 0, "the record carries the file's name");
        assert_eq!(published[0].used, 0);
        // the export resolves to base + 8; the site jumps to base + 16 (4ef9 and the address, big endian)
        assert_eq!(l.exports.resolve(dkneeds::keys::name_hash("dk_svctest_ping")), Some(base + 8));
        let t = base + 16;
        assert_eq!(site_bytes(&l, 6), [0x4e, 0xf9, (t >> 24) as u8, (t >> 16) as u8, (t >> 8) as u8, t as u8]);
        assert_eq!(l.installer.count(), 1);
        // the install is the last step: it comes after the init of the provider
        let init = l.m.log.iter().position(|e| matches!(e, Ev::Init(..))).unwrap();
        assert!(init < l.m.log.len());
        assert!(l.p.calls.borrow().contains(&crate::fake_platform::Call::Mask), "the install runs under the mask");
        // the table copy stays for the record
        assert!(l.provs[1].block.is_some());
    }
}

/// A `SLOTS` item may fill a `[[slot]]` word only: the same item naming an `[[info]]` data table refuses the file (`E_FORMAT`) and writes nothing.
#[test]
fn a_slot_fill_of_an_info_data_entry_is_refused_and_writes_nothing() {
    use dkneeds::keys::{name_hash, Slot, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    static SLOTS: std::sync::OnceLock<[Slot; 1]> = std::sync::OnceLock::new();
    unsafe {
        let slots = SLOTS.get_or_init(|| [Slot { id: name_hash("the.entry"), flags: 0, entry: 4 }]);
        for (data, want) in [(false, OK), (true, E_FORMAT)] {
            let bind = svc_bind(&STOCK);
            let table = Table { slots, ..svc_table(&bind) };
            let mut items = vec![(dkrt_abi::service::Declare::SLOT, name_hash("the.entry"), 4)];
            items.extend_from_slice(match svc_init() {
                Init::Declare(i) => i,
                _ => unreachable!(),
            });
            let mut l = provider_loader(service_file(&table, 0), Init::Declare(Box::leak(items.into_boxed_slice())));
            l.p.info_entries.borrow_mut().push((name_hash("the.entry"), vec![0]));
            if data {
                l.p.info_data.borrow_mut().push(name_hash("the.entry"));
            }
            assert_eq!(l.load_provider(PATH.as_ptr(), 1), want, "data entry: {data}");
            if data {
                assert_eq!(site_bytes(&l, 6), STOCK, "the site is stock");
                assert_eq!(l.installer.count(), 0, "nothing is installed");
                assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Word { .. })), "no slot word is written");
            }
        }
    }
}

#[test]
fn a_service_import_is_linked_for_a_service_file_only() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the word at 12 holds 4, plus the import
        let bind = svc_bind(&STOCK);
        let base = || {
            let mut f = module_file_with(&[(12, 1 << 24)], &[b"dkc_device_id"]);
            f[IMAGE_OFF + 15] = 4;
            f
        };
        let mut svc = service_file_of(base(), &svc_table(&bind), 0);
        seal(&mut svc);
        let mut l = provider_loader(svc, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        let words = core::slice::from_raw_parts(l.m.live[0].1.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), SVC_ADDR + 4);
        // a module has no such import: the name is unknown to it, and nothing is allocated
        let mut f = base();
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT);
        assert_eq!(l.m.allocs, 0);
        assert_eq!(l.missing()[0], 0);
    }
}

/// A service file of a table with a provides pair only (no hook, no export): `init` declares just its row.
fn row_service_file() -> Vec<u8> {
    use dkneeds::keys::{name_hash, Pair, Table};
    static PROVIDES: std::sync::OnceLock<[Pair; 1]> = std::sync::OnceLock::new();
    let table = Table { provides: PROVIDES.get_or_init(|| [Pair { a: name_hash(SVC), b: 0x000802 }]), ..Default::default() };
    service_file(&table, 0)
}

#[test]
fn the_row_a_service_file_declares_is_added_at_the_commit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let rec = MAX_MODULES;
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(l.r.log, vec![Ev::AddRow(rec, &TEST_ROW)]);
        // (a provider record has no run-time unload: its row goes with a failed load, below)
        // a module declares no row: `init` cannot, the import is not in its table
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(l.r.log, vec![Ev::Add(0)]);
    }
}

#[test]
fn a_row_the_registry_refuses_undoes_the_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let rec = MAX_MODULES;
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        l.r.add_result = E_PLAN_OWNER;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_PLAN_OWNER);
        assert!(l.m.live.is_empty());
        assert_eq!(l.r.log, vec![Ev::AddRow(rec, &TEST_ROW), Ev::Remove(rec)]);
    }
}

#[test]
fn the_real_registry_puts_the_row_into_the_service_table_and_takes_it_out() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut r = RealRegistry;
        // the host tests of the runtime run with the machines row loaded (`svc_rows::test_machines`): the loaded rows start at one
        let fixed = crate::gen_services::STATIC + 1;
        assert_eq!(services().rows.len(), fixed);
        assert_eq!(r.add(0, None), OK);
        assert_eq!(r.add(2, Some(&TEST_ROW)), OK);
        let t = services();
        assert_eq!(t.rows.len(), fixed + 1);
        assert!(core::ptr::eq(t.rows[fixed], &TEST_ROW));
        // the in-crate prefix and its failed mask are as before, and one row has one owner
        assert_eq!(t.failed, services().failed);
        assert_eq!(r.add(3, Some(&TEST_ROW)), E_PLAN_OWNER);
        // (`RealRegistry::remove` also runs the registry's own remove, which the cleanup tests count: the row part alone here)
        crate::svc_rows::remove(3);
        assert_eq!(services().rows.len(), fixed + 1);
        crate::svc_rows::remove(2);
        assert_eq!(services().rows.len(), fixed);
    }
}

#[test]
fn a_hook_site_that_is_not_stock_refuses_the_provider_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        l.p.put_code(SITE + 3, &[0x99]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_HOOK_STOCK);
        assert_eq!(l.m.allocs, 0, "the check runs before the image is placed");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        assert_eq!(site_bytes(&l, 6), [0x20, 0x52, 0x48, 0x99, 0x00, 0x02]);
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
    }
}

/// A bound file holds the identity of the firmware it was bound to. A file for another firmware, or an image that has no `fw_id`, is refused as a site that is not
/// stock (`E_HOOK_STOCK`), before anything is allocated and before `init`; the stock bytes of the sites are not even looked at.
#[test]
fn a_provider_bound_to_another_firmware_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut other = FW_ID;
        other[7] ^= 1;
        for id in [Some(other), None] {
            let bind = svc_bind(&STOCK);
            let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
            set_fw_id(&l, id);
            assert_eq!(
                l.load_provider(PATH.as_ptr(), 0),
                E_HOOK_STOCK,
                "image id {id:?}"
            );
            assert_eq!(l.m.allocs, 0);
            assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
            assert_eq!(site_bytes(&l, 6), STOCK);
            assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        }
        // the identity is checked before the stock bytes: the stock check never runs when the identity differs (a non-stock site and the right id is the other test)
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        set_fw_id(&l, Some(other));
        l.p.put_code(SITE + 3, &[0x99]);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_HOOK_STOCK);
        assert!(
            !l.p.calls
                .borrow()
                .iter()
                .any(|c| matches!(c, crate::fake_platform::Call::Mask))
        );
    }
}

/// `BIND` items without an `FW` item are `E_FORMAT` (a file bound by an older patchkit), and so are two `FW` items; both come before the stock check. A file with no
/// `FW` and no `BIND` has no firmware address and loads on any image.
#[test]
fn the_fw_item_rules_are_format_errors_and_a_file_with_no_firmware_reference_loads_anywhere() {
    use dkneeds::keys::Table;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        for (fw, what) in [
            (&[][..], "BIND without FW"),
            (&[FW_ID, FW_ID][..], "two FW items"),
        ] {
            let table = Table {
                fw,
                ..svc_table(&bind)
            };
            let mut l = provider_loader(service_file(&table, 0), svc_init());
            l.p.put_code(SITE + 3, &[0x99]);
            assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT, "{what}");
            assert_eq!(l.m.allocs, 0, "{what}");
        }
        // no FW and no BIND: accepted, whatever the image's fw_id is (or even if it has none)
        let table = Table {
            bind: &[],
            hooks: &[],
            exports: &[],
            fw: &[],
            ..svc_table(&bind)
        };
        let mut l = provider_loader(service_file(&table, 0), Init::Ok);
        set_fw_id(&l, None);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
    }
}

#[test]
fn a_provider_whose_init_fails_leaves_no_export_no_site_and_no_block() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), Init::Fails(E_INIT));
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_INIT);
        assert!(l.m.live.is_empty(), "the image and the table copy are free");
        assert_eq!(records::module(MAX_MODULES).used, 0);
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        assert_eq!(site_bytes(&l, 6), STOCK);
        assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, crate::fake_platform::Call::Write { .. } | crate::fake_platform::Call::Mask)), "no site was written");
    }
}

#[test]
fn a_declaration_that_differs_from_the_table_refuses_the_provider_and_the_site_stays_stock() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        // init declares nothing: the HOOKS and EXPORTS of the table are not repeated
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), Init::Ok);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_DECL_KIND);
        assert!(l.m.live.is_empty());
        assert_eq!((l.installer.count(), l.exports.count()), (0, 0));
        assert_eq!(site_bytes(&l, 6), STOCK);
    }
}

#[test]
fn a_provider_in_safe_mode_installs_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        l.installer.set_armed(false);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(site_bytes(&l, 6), STOCK);
        assert_eq!(l.installer.count(), 0);
    }
}

#[test]
fn a_service_file_loads_only_as_a_provider_and_a_module_file_only_as_a_module() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let svc = service_file(&svc_table(&bind), 0);
        let mut l = provider_loader(svc.clone(), svc_init());
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        let mut l = loader(Init::Ok);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        // a provider whose ABI the registry refuses stops before anything is allocated; the module asks nothing
        let mut svc2 = svc;
        put(&mut svc2, dkneeds::H_SVC_ABI, 0xabcd_0123);
        seal(&mut svc2);
        let mut l = provider_loader(svc2, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        assert_eq!(*l.r.asked.borrow(), vec![Ev::AbiOk(0xabcd_0123)]);
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert!(l.r.asked.borrow().is_empty());
        let mut svc3 = service_file(&svc_table(&bind), 0);
        put(&mut svc3, dkneeds::H_SVC_ABI, 1);
        seal(&mut svc3);
        let mut l = provider_loader(svc3, svc_init());
        l.r.abi = false;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_SVC_ABI);
        assert_eq!(l.m.allocs, 0);
    }
}

#[test]
fn a_module_import_that_the_runtime_lacks_resolves_to_a_published_export_and_not_before() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // word at 12 holds 4 (plus the import)
        let mut file = module_file_with(&[(12, 1 << 24)], &[b"dk_svctest_ping"]);
        file[IMAGE_OFF + 15] = 4;
        seal(&mut file);
        let mut l = loader_of(Init::Ok, file);
        assert_eq!(l.load(PATH.as_ptr()), E_IMPORT, "no provider is loaded yet");
        l.exports.publish(MAX_MODULES, dkneeds::keys::name_hash("dk_svctest_ping"), 0x7000).unwrap();
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let mem = &l.m.live.last().unwrap().1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), 0x7000 + 4);
    }
}

/// A module file (role 0) whose header table is `table`, with the relocation `(12, import 0)` of the import `dk_svctest_ping`.
fn consumer_file(table: &dkneeds::keys::Table) -> Vec<u8> {
    let mut v = module_file_with(&[(12, 1 << 24)], &[b"dk_svctest_ping"]);
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

#[test]
fn the_boot_finds_the_provider_for_a_hard_need_loads_it_first_and_the_consumer_resolves_its_import() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = staged_boot(service_file(&svc_table(&bind), 0), consumer_file(&user_table()));
        let mut plan = l.plan_with_caps(1, &wide_caps());
        assert_eq!((plan.n, plan.np), (1, 1), "the search found the provider");
        assert_eq!(l.found.path(0), SVC_PATH);
        l.load_providers(&plan);
        assert_eq!(l.loaded[0], true);
        let rec = MAX_MODULES;
        assert_eq!(records::module(rec).used, 1);
        l.settle(&mut plan, 1);
        assert_eq!(plan.verdicts[0], dkrt_abi::plan::Verdict::Accepted);
        // the module stage: the import is the provider's export
        l.phase = Phase::Boot;
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let mem = &l.m.live.last().unwrap().1;
        let words = core::slice::from_raw_parts(mem.as_ptr() as *const u8, 32);
        assert_eq!(rd32(words.as_ptr().add(12)), records::module(rec).base + 8 + 4);
    }
}

#[test]
fn no_search_without_an_unmet_need_and_a_failed_provider_refuses_its_dependents_with_the_hash() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a selection with no need: no listing, no provider, the plan as before
        let mut l = staged_boot(service_file(&svc_table(&svc_bind(&STOCK)), 0), module_file());
        let allocs = l.m.allocs;
        let plan = l.plan_with_caps(1, &wide_caps());
        assert_eq!((plan.np, l.m.allocs), (0, allocs), "nothing was searched");
        // the provider's site is not stock: the provider fails, and its dependent is refused before the module stage
        let bind = svc_bind(&STOCK);
        let mut l = staged_boot(service_file(&svc_table(&bind), 0), consumer_file(&user_table()));
        l.p.put_code(SITE, &[0u8; 6]);
        let mut plan = l.plan_with_caps(1, &wide_caps());
        assert_eq!(plan.np, 1);
        l.load_providers(&plan);
        assert_eq!((l.loaded[0], records::module(MAX_MODULES).used), (false, 0));
        l.settle(&mut plan, 1);
        let hash = dkneeds::keys::name_hash(SVC);
        assert_eq!(plan.verdicts[0], dkrt_abi::plan::Verdict::Refused(dkrt_abi::plan::Reason::NeedMissing(hash)));
        assert_eq!(plan.verdicts[0].result(), E_MISSING);
        // the site stayed stock and the table holds no site
        assert_eq!(l.installer.count(), 0);
    }
}

#[test]
fn the_real_registry_accepts_zero_and_its_own_fingerprint_only() {
    let r = RealRegistry;
    assert!(r.abi_ok(0));
    assert!(r.abi_ok(dkrt_abi::service::SVC_ABI));
    assert!(!r.abi_ok(dkrt_abi::service::SVC_ABI ^ 1));
}

#[test]
fn the_role_word_picks_the_steps() {
    type Steps = RoleSteps<FakePlatform, FakeImages, FakeRegistry>;
    assert!(Steps::of(1).check as usize == Steps::SERVICE.check as usize);
    assert!(Steps::of(0).check as usize == Steps::MODULE.check as usize);
    // a word that is neither is a module, as the loader always took it
    assert!(Steps::of(7).check as usize == Steps::MODULE.check as usize);
}

// ---- interface B: the module lifecycle with a fake service ------------------------------------------------------------------------

#[test]
fn the_services_are_told_before_init_and_commit_in_three_passes_after_it() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkrt_abi::service::commit_phase::{DECLARE, PREPARE, WRITE};
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        // the module is the first block of the arena: its base is FIRST_ADDR, its image 32 bytes
        assert_eq!(l.r.svc, vec![Svc::Begin(0), Svc::Commit(0, PREPARE, FIRST_ADDR, 32), Svc::Commit(0, WRITE, FIRST_ADDR, 32), Svc::Commit(0, DECLARE, FIRST_ADDR, 32)]);
        assert_eq!(*ORDER.lock().unwrap(), ["begin", "init", "commit", "commit", "commit"]);
        // a service that wrote no record has no block of its own: the module block stays alone
        assert_eq!(l.m.live.iter().map(|b| b.0).collect::<Vec<_>>(), vec![FIRST_ADDR]);
    }
}

#[test]
fn a_service_that_refuses_the_commit_undoes_the_load_for_every_service() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkrt_abi::service::commit_phase::{PREPARE, WRITE};
        // in the first pass: nothing was written, no second pass
        let mut l = loader(Init::Ok);
        l.r.service.refuse = Some((PREPARE, E_TYPE_USED));
        assert_eq!(l.load(PATH.as_ptr()), E_TYPE_USED);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)], "the registry takes everything of the record down");
        assert!(!l.r.svc.iter().any(|e| matches!(e, Svc::Commit(_, WRITE, ..))));
        // in the second pass, with the record block allocated: the block is freed too
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 0, refuse: Some((WRITE, E_PARAM_FULL)) };
        assert_eq!(l.load(PATH.as_ptr()), E_PARAM_FULL);
        assert!(l.m.live.is_empty(), "the module block and the record block are free");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        assert_eq!(st().loads, 0);
        // the record can be used again
        l.r.service = FakeService { record: 0, skew: 0, refuse: None };
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(st().modules[0].used, 1);
    }
}

#[test]
fn the_records_a_service_writes_are_one_block_that_the_walker_checks() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a record of a kind no service owns: the block was allocated, written and refused by the walker, and everything is given back
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 0, refuse: None };
        let r = l.load(PATH.as_ptr());
        assert_ne!(r, OK);
        assert!(l.m.live.is_empty());
        assert_eq!(l.m.log.iter().filter(|e| matches!(e, Ev::Alloc(..))).count(), 3, "module block, collect block, record block");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        // a service that writes more in the second pass than it measured is refused before the walker runs
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 8, refuse: None };
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert!(l.m.live.is_empty());
    }
}

// ---- refusals of the checks and the undo of the service role ----------------------------------------------------------------------

/// Resets the loader's `TABLE_BAD` flag when a test that sets it ends, also when it fails.
struct TableBadGuard;

impl Drop for TableBadGuard {
    fn drop(&mut self) {
        unsafe { *core::ptr::addr_of_mut!(crate::invariants::DKRT_TABLE_BAD) = 0 };
    }
}

#[test]
fn no_load_starts_once_the_tables_were_marked_bad() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _reset = TableBadGuard;
        let mut l = loader(Init::Ok);
        *core::ptr::addr_of_mut!(crate::invariants::DKRT_TABLE_BAD) = 1;
        assert_eq!(l.load(PATH.as_ptr()), E_TABLE);
        assert!(l.m.log.is_empty(), "the file is not even read");
        assert_eq!(st().modules[0].used, 0);
    }
}

#[test]
fn an_import_name_without_a_nul_is_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let name = vec![b'x'; dkneeds::CF_IMPORT_LEN as usize];
        let mut l = loader_of(Init::Ok, module_file_with(&[], &[&name]));
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        assert_eq!(l.missing()[0], 0);
    }
}

#[test]
fn a_bad_relocation_a_register_section_outside_the_file_and_a_bad_keys_table_are_refused_before_anything_is_allocated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a relocation past the last word of the image, and one of an unknown kind
        for relocs in [[(30u32, 0u32)], [(8, 2 << 24)]] {
            let mut l = loader_of(Init::Ok, module_file_with(&relocs, &[]));
            assert_eq!(l.load(PATH.as_ptr()), E_FORMAT, "{relocs:?}");
            assert_eq!(l.m.allocs, 0);
        }
        // the keys table lies outside the file
        let mut f = module_file();
        let end = f.len() as u32;
        put(&mut f, dkneeds::H_REG_OFF, end + 16);
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // the keys table has another magic: a file of the format before the table
        let mut f = module_file();
        let at = f.len() as u32 - 8;
        put(&mut f, at, 0);
        seal(&mut f);
        let mut l = loader_of(Init::Ok, f);
        assert_eq!(l.load(PATH.as_ptr()), E_CONTRACT);
        assert_eq!(l.m.allocs, 0);
        assert!(l.r.log.is_empty());
    }
}

#[test]
fn a_provider_record_in_use_and_a_second_module_of_the_same_name_are_refused_as_loaded() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader_of(Init::DeclaresRow, row_service_file());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), OK);
        let allocs = l.m.allocs;
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_LOADED);
        assert_eq!(l.m.allocs, allocs, "the second load takes nothing");
        assert_eq!(records::module(MAX_MODULES).used, 1, "the first one stays");

        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        let allocs = l.m.allocs;
        assert_eq!(l.load(PATH.as_ptr()), E_LOADED);
        assert_eq!((l.m.allocs, l.m.live.len()), (allocs, 1));
        assert_eq!((st().modules[0].used, st().modules[1].used, st().loads), (1, 0, 1));
    }
}

#[test]
fn capture_keeps_a_table_that_fits_and_ignores_a_short_file_a_bad_index_an_empty_table_and_one_over_the_limit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = loader(Init::Ok);
        l.reset_plan();
        let file = module_file();
        l.capture(0, &file);
        assert_eq!(l.table_len[0], 8);
        assert_eq!(l.tables[0][..8], file[file.len() - 8..]);
        // nothing is kept for a file shorter than the header, an index past the selection, an empty table or one that lies outside the file
        l.reset_plan();
        l.capture(1, &file[..dkneeds::HEADER as usize - 4]);
        l.capture(SEL_MAX, &file);
        l.capture(usize::MAX, &file);
        let mut empty = file.clone();
        put(&mut empty, dkneeds::H_REG_SIZE, 0);
        l.capture(2, &empty);
        let mut outside = file.clone();
        let end = outside.len() as u32;
        put(&mut outside, dkneeds::H_REG_OFF, end - 4);
        l.capture(3, &outside);
        assert_eq!(l.table_len, [0; SEL_MAX]);
        // a table of the planner's limit is kept, one byte more is not (the loader decides about such a file)
        let mut long = file.clone();
        long.resize(long.len() + crate::plan::TABLE_MAX, 0);
        put(&mut long, dkneeds::H_REG_SIZE, crate::plan::TABLE_MAX as u32);
        l.capture(4, &long);
        assert_eq!(l.table_len[4], crate::plan::TABLE_MAX);
        put(&mut long, dkneeds::H_REG_SIZE, crate::plan::TABLE_MAX as u32 + 1);
        l.capture(5, &long);
        assert_eq!(l.table_len[5], 0);
    }
}

#[test]
fn a_provider_the_plan_refused_is_reported_and_never_staged_and_its_dependents_are_refused() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = staged_boot(service_file(&svc_table(&bind), 0), consumer_file(&user_table()));
        let mut plan = l.plan_with_caps(1, &wide_caps());
        assert_eq!(plan.np, 1);
        plan.provider_verdicts[0] = dkrt_abi::plan::Verdict::Refused(dkrt_abi::plan::Reason::Fit(dkrt_sdk::proto::CAPK_MACHINE_ROWS));
        l.m.log.clear();
        let allocs = l.m.allocs;
        l.load_providers(&plan);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Stage)), "the refused file is not read");
        assert_eq!((l.loaded[0], l.m.allocs, records::module(MAX_MODULES).used), (false, allocs, 0));
        assert!(l.phase == Phase::Closed);
        l.settle(&mut plan, 1);
        let hash = dkneeds::keys::name_hash(SVC);
        assert_eq!(plan.verdicts[0], dkrt_abi::plan::Verdict::Refused(dkrt_abi::plan::Reason::NeedMissing(hash)));
    }
}

/// The bytes of a header table that has only `needs`.
fn needs_table(needs: &[dkneeds::keys::Need]) -> Vec<u8> {
    let t = dkneeds::keys::Table { needs, ..Default::default() };
    let mut b = vec![0u8; dkneeds::keys::table_size(&t)];
    dkneeds::keys::write_table(&t, &mut b).unwrap();
    b
}

#[test]
fn the_search_list_keeps_the_first_24_distinct_hard_needs_the_runtime_does_not_meet() {
    use dkneeds::keys::{name_hash, Need};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let need = |n: &str, hard: bool| Need { service: name_hash(n), min_version: 0, hard };
    let svc = |i: usize| std::format!("svc{i}");
    let mut tables = vec![needs_table(&[need("runtime", true), need("soft", false)]), needs_table(&[need(&svc(0), true), need(&svc(1), true)])];
    for i in 2..30 {
        tables.push(needs_table(&[need(&svc(i), true), need(&svc(0), true)]));
    }
    let mods: Vec<Option<Keys>> = tables.iter().map(|b| Some(Keys::parse(b).unwrap())).collect();
    let mut out = [0u32; WANT_MAX];
    let n = unmet_needs(&mods, &mut out);
    assert_eq!(n, WANT_MAX);
    let first: Vec<u32> = (0..WANT_MAX).map(|i| name_hash(&svc(i))).collect();
    assert_eq!(out.to_vec(), first, "in the order of the selection, each once; the runtime's service and the soft need are not in it");
    // a module with no table adds nothing
    let none: [Option<Keys>; 2] = [None, None];
    assert_eq!(unmet_needs(&none, &mut out), 0);
}

/// A service whose table holds `slots` (one `SLOTS` item of the entry `entry`) and whose `init` declares what the table says.
unsafe fn slot_provider(entry: u32) -> Fake {
    use dkneeds::keys::{name_hash, Slot, Table};
    let slots: &'static [Slot] = Box::leak(Box::new([Slot { id: name_hash("the.entry"), flags: 0, entry }]));
    let bind = svc_bind(&STOCK);
    let table = Table { slots, ..svc_table(&bind) };
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

#[test]
fn a_service_file_with_a_table_outside_the_file_or_of_the_wrong_shape_is_refused_before_anything_is_allocated() {
    use dkneeds::keys::{Pair, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        // the table lies outside the file
        let mut f = service_file(&svc_table(&bind), 0);
        let end = f.len() as u32;
        put(&mut f, dkneeds::H_REG_OFF, end + 64);
        seal(&mut f);
        let mut l = provider_loader(f, svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_eq!(l.m.allocs, 0);
        // no PROVIDES item, and two of them: not the shape of a service
        const TWO: [Pair; 2] = [Pair { a: 1, b: 0 }, Pair { a: 2, b: 0 }];
        for table in [Table::default(), Table { provides: &TWO, ..Default::default() }] {
            let mut l = provider_loader(service_file(&table, 0), svc_init());
            assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
            assert_eq!(l.m.allocs, 0);
            assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))));
        }
    }
}

#[test]
fn a_provider_with_an_export_past_the_image_or_a_table_copy_that_does_not_fit_is_refused_and_undone() {
    use dkneeds::keys::{name_hash, Pair, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the export entry is the image size (32): past the last byte
        let exports: &'static [Pair] = Box::leak(Box::new([Pair { a: name_hash("dk_svctest_ping"), b: 32 }]));
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&Table { exports, ..svc_table(&bind) }, 0), svc_init());
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Init(..))), "place runs before init");

        // the arena has room for the image but not for the copy of the table
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        l.m.fail_alloc = Some(2);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_NOMEM);
        assert_eq!(l.m.allocs, 2);
        assert_provider_undone(&l, 0);
    }
}

#[test]
fn a_slot_fill_past_the_image_or_of_a_slot_the_image_lacks_undoes_the_load() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // the entry is the image size
        let mut l = slot_provider(32);
        l.p.info_entries.borrow_mut().push((dkneeds::keys::name_hash("the.entry"), vec![0]));
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
        // the image has no such slot
        let mut l = slot_provider(4);
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_FORMAT);
        assert_provider_undone(&l, 0);
    }
}

#[test]
fn an_export_name_another_provider_published_undoes_the_load_and_keeps_the_other_export() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let bind = svc_bind(&STOCK);
        let mut l = provider_loader(service_file(&svc_table(&bind), 0), svc_init());
        l.exports.publish(MAX_MODULES + 1, dkneeds::keys::name_hash("dk_svctest_ping"), 0x7000).unwrap();
        assert_eq!(l.load_provider(PATH.as_ptr(), 0), E_PLAN_OWNER);
        assert_provider_undone(&l, 1);
        assert_eq!(l.exports.resolve(dkneeds::keys::name_hash("dk_svctest_ping")), Some(0x7000));
    }
}

#[test]
fn settle_hands_the_services_the_keys_of_accepted_modules_only() {
    use dkneeds::keys::{write_table, KeyGroup, Table};
    use dkrt_abi::plan::{Reason, Verdict};
    let keys = [0x100u32, 0x200];
    let t = Table { keys: &[KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &keys }], ..Default::default() };
    let mut bytes = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut bytes).unwrap();
    let mut tables = [[0u8; crate::plan::TABLE_MAX]; SEL_MAX];
    let mut lens = [0usize; SEL_MAX];
    for k in 0..2 {
        tables[k][..bytes.len()].copy_from_slice(&bytes);
        lens[k] = bytes.len();
    }
    // module 1 repeats the keys of module 0: the plan refused it (E_PLAN_KEY), so its keys never reach the sticky assignment
    let mut plan = Plan {
        n: 2,
        verdicts: [Verdict::Accepted; SEL_MAX],
        order: [0; SEL_MAX],
        dsp_load: (0, 0),
        np: 0,
        provider_verdicts: [Verdict::Accepted; dkrt_sdk::PROVIDER_MAX],
        provider_order: [0; dkrt_sdk::PROVIDER_MAX],
    };
    plan.verdicts[1] = Verdict::Refused(Reason::DuplicateKey(dkneeds::KIND_MACHINE, 0x100));
    let sel = settle_ctx(&plan, &tables, &lens, 2);
    assert_eq!((sel.count, sel.nkeys[0], sel.nkeys[1]), (2, 2, 0));
    assert_eq!(sel.keys[0][..2], keys);
}

#[test]
fn keys_of_gives_the_keys_of_a_kind_for_an_accepted_module_only() {
    use dkneeds::keys::{write_table, KeyGroup, Table};
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (shapes, machines) = ([0x380u32, 0x400, 0x500], [0x100u32]);
    let t = Table {
        keys: &[
            KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &machines },
            KeyGroup { kind: dkneeds::KIND_LFO_SHAPE, keys: &shapes },
        ],
        ..Default::default()
    };
    let mut bytes = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut bytes).unwrap();
    unsafe {
        let mut l = loader(Init::Ok);
        l.reset_plan();
        for k in 0..2 {
            l.tables[k][..bytes.len()].copy_from_slice(&bytes);
            l.table_len[k] = bytes.len();
        }
        // before settle nothing is accepted
        let mut out = [0u32; 8];
        assert_eq!(l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
        l.accepted[0] = true;
        assert_eq!(l.keys_of(0, dkneeds::KIND_LFO_SHAPE, &mut out), 3);
        assert_eq!(out[..3], shapes);
        // another kind, a kind the table does not list, a module the plan refused and an index past the selection
        assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), 1);
        assert_eq!(l.keys_of(0, dkneeds::KIND_EFFECT, &mut out), 0);
        assert_eq!(l.keys_of(1, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
        assert_eq!(l.keys_of(SEL_MAX as u32, dkneeds::KIND_LFO_SHAPE, &mut out), 0);
    }
}

// ---- reload of a live module (docs/plans/extension-model.md E5) ------------------------------------------------------------------------------------

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
    crate::selection::BOOT_IDX = k as u32;
    let staged = l.m.stage(path.as_ptr()).unwrap();
    let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize).to_vec();
    l.capture(k, &file);
    l.accepted[k] = true;
    assert_eq!(l.load(path.as_ptr()), OK);
    crate::selection::BOOT_IDX = u32::MAX;
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

#[test]
fn a_reload_check_accepts_a_file_that_keeps_the_keys_and_changes_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let (live, allocs, base) = (l.m.live.len(), l.m.allocs, st().modules[0].base);
        // the same keys and a new one
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[2, 1, 7]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        assert_eq!((l.m.live.len(), l.m.allocs, st().modules[0].used, st().modules[0].base), (live, allocs, 1, base));
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))));
        assert_eq!(l.table_len[0], dkneeds::keys::table_size(&dkneeds::keys::Table { keys: &[dkneeds::keys::KeyGroup { kind: dkneeds::KIND_MACHINE, keys: &[1, 2] }], ..Default::default() }), "the old table stays");
    }
}

#[test]
fn a_reload_check_refuses_a_missing_record_a_dropped_key_another_name_and_a_damaged_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let key = |l: &mut Fake, f: Vec<u8>| {
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), f);
            l.reload_check(0, PATH.as_ptr(), &wide())
        };
        assert_eq!(l.reload_check(1, PATH.as_ptr(), &wide()), E_NOT_FOUND, "a record that is not live");
        assert_eq!(l.reload_check(MAX_MODULES, PATH.as_ptr(), &wide()), E_NOT_FOUND, "a provider record is no module");
        assert_eq!(key(&mut l, keyed_file(b'a', &[1])), E_RELOAD_KEYS);
        assert_eq!(key(&mut l, keyed_file(b'z', &[1, 2])), E_FORMAT);
        let mut bad = keyed_file(b'a', &[1, 2]);
        bad[0] ^= 1;
        assert_eq!(key(&mut l, bad), E_FORMAT);
        l.m.files.remove(&PATH[..PATH.len() - 1].to_vec());
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), E_NOT_FOUND, "the new file is not in the store");
        // the old module is still there, whole
        assert_eq!((st().modules[0].used, l.m.live.len()), (1, 1));
    }
}

#[test]
fn a_reload_check_runs_the_planner_over_the_live_set_with_the_file_in_the_old_slot() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        // a second live module `b` has key 9; a new `a` that also has key 9 clashes with it (the old `a` did not)
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 9]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), E_PLAN_KEY);
        // without the clash it passes, and the old key set of `a` itself is not a clash with its own old file
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 10]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        // a live set the capacity cannot hold with the new file is refused
        let mut tight = wide();
        let mods = tight.iter().position(|c| c.kind == dkrt_sdk::proto::CAPK_MODULES as u32).unwrap();
        tight[mods].limit = 1;
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &tight), E_PLAN_FIT);
    }
}

#[test]
fn a_second_unload_after_busy_runs_fini_and_the_undo_once_and_an_aborted_reload_gives_the_tick_back() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let base = st().modules[0].base;
        assert_eq!(crate::entry::tick_of(0), base + 8);
        // the DSP stays retained: the teardown stops at the quiesce, after the tick was stopped
        dsp::ds().phase = dsp::PH_RETAINED;
        assert_eq!(l.unload_rec(0), E_BUSY);
        assert_eq!(crate::entry::tick_of(0), 0);
        assert_eq!(st().modules[0].used, 1, "the old module is not released");
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))));
        // a reload that gives up before `fini` gives the tick back
        assert!(l.reload_abort(0));
        assert_eq!(crate::entry::tick_of(0), base + 8);
        // and the teardown can be tried again: the DSP is free this time, `fini` and the undo run once
        dsp::ds().phase = dsp::PH_IDLE;
        assert_eq!(l.unload_rec(0), OK);
        let n = |v: &Vec<Ev>, e: Ev| v.iter().filter(|x| **x == e).count();
        assert_eq!((n(&l.m.log, Ev::Fini(0, base + 4)), n(&l.r.log, Ev::Remove(0))), (1, 1));
        assert_eq!(l.torn[0], 0, "the mask is clear once the record is free");
        // a teardown that was interrupted after `fini` ran does not run it again, and cannot give the tick back
        let mut l = live_a();
        l.torn[0] = TORN_TICK | TORN_FINI;
        assert!(!l.reload_abort(0));
        assert_eq!(l.unload_rec(0), OK);
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..))), "fini is not run twice");
    }
}

#[test]
fn a_reload_loads_the_new_file_into_the_old_record_and_can_be_repeated() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        for round in 0..2u32 {
            let keys: &[u32] = if round == 0 { &[1, 2, 5] } else { &[1, 2, 5, 6] };
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', keys));
            assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
            assert_eq!(l.unload_rec(0), OK);
            assert_eq!(st().modules[0].used, 0);
            assert_eq!(l.reload_load(0, PATH.as_ptr()), OK, "round {round}");
            // the same record, owner and selection index; the table is the new one; the phase is closed again
            assert_eq!((st().modules[0].used, l.sel_of_rec[0], l.phase == Phase::Closed), (1, 0, true));
            assert_eq!(crate::entry::tick_of(0), st().modules[0].base + 8);
            let mut out = [0u32; 8];
            assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), keys.len());
            assert_eq!(out[..keys.len()], *keys);
            assert_eq!(st().modules[1].used, 1, "the other module is untouched");
        }
        assert_eq!(l.m.live.len(), 2, "one block each: nothing leaked over two reloads");
        // a reload is the only load after the boot: the same call in the closed phase through `load` is refused
        assert_eq!(l.load(PATH.as_ptr()), E_BOOT_ONLY);
        assert_eq!(l.unload_rec(0), OK);
        assert_eq!(l.reload_load(MAX_MODULES, PATH.as_ptr()), E_NOT_FOUND, "a provider record is not reloaded");
    }
}

#[test]
fn a_reload_waits_for_commands_in_flight_and_two_blocks_after_it_closed_the_entries() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let base = st().modules[0].base;
        l.p.clock.set(Some(100));
        crate::module_cmds::test_set_in_flight(1, 1);
        // the first call closes the entries: the tick stops, the commands of the module are refused, and it waits
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        assert_eq!(crate::entry::tick_of(0), 0);
        // blocks pass, but a command of the module is still in flight
        l.p.clock.set(Some(105));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        crate::module_cmds::test_set_in_flight(1, 0);
        // the command returned: one block since the entries were closed is not enough, two are
        l.p.clock.set(Some(101));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        l.p.clock.set(Some(102));
        assert_eq!(l.reload_quiesce(0), OK);
        // nothing was freed and no row touched: the old module is whole
        assert_eq!((st().modules[0].used, l.m.live.len()), (1, 1));
        assert!(!l.m.log.iter().any(|e| matches!(e, Ev::Fini(..)) || *e == Ev::Free(base)));
        // the teardown that follows finishes the record and clears the mask
        assert_eq!(l.unload_rec(0), OK);
        assert_eq!(l.torn[0], 0);
    }
}

#[test]
fn the_block_wait_survives_a_counter_wrap_and_a_platform_without_a_clock_waits_for_nothing() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.p.clock.set(Some(u32::MAX));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        l.p.clock.set(Some(0));
        assert_eq!(l.reload_quiesce(0), E_BUSY, "one block");
        l.p.clock.set(Some(1));
        assert_eq!(l.reload_quiesce(0), OK, "two blocks, over the wrap");
        assert!(l.reload_abort(0));
        // no block clock: the entries close and the wait is over at once
        l.p.clock.set(None);
        assert_eq!(l.reload_quiesce(0), OK);
    }
}

unsafe extern "C" fn noop_cmd(_: u32, _: u32, _: *const u8, _: u32, _: *mut u8) -> i32 {
    0
}

#[test]
fn an_aborted_reload_gives_back_the_tick_and_the_commands_and_the_sinks_stay_released() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let base = st().modules[0].base;
        // module `a` (source 1) owns the span sink
        crate::dlog::test_reset_sinks();
        let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_CALLBACK);
        assert_eq!(crate::dlog::dk_sink_set(crate::dlog::SINK_SPAN, 0x1000 as *mut u8), OK);
        crate::dlog::leave_module(prev);
        assert!(crate::dlog::sink_on(crate::dlog::SINK_SPAN));
        l.p.clock.set(Some(0));
        assert_eq!(l.reload_quiesce(0), E_BUSY);
        assert!(!crate::dlog::sink_on(crate::dlog::SINK_SPAN), "the sink is cleared under the mask when the entries close");
        // a command of the module is refused while it drains, and runs again after the abort
        let prev = crate::dlog::enter_module(0, crate::dlog::PHASE_CALLBACK);
        assert_eq!(crate::module_cmds::host_register(0x38, 1, Some(noop_cmd)), OK);
        crate::dlog::leave_module(prev);
        assert!(crate::module_cmds::begin_cmd(0x38).is_err());
        assert!(l.reload_abort(0));
        assert_eq!(crate::entry::tick_of(0), base + 8);
        let (h, owner) = crate::module_cmds::begin_cmd(0x38).unwrap().expect("registered");
        let _ = h;
        assert_eq!((owner, crate::module_cmds::in_flight(1)), (1, 1), "counted while it runs");
        crate::module_cmds::end_cmd(owner);
        assert_eq!(crate::module_cmds::in_flight(1), 0);
        crate::module_cmds::release_cmds(1);
        crate::dlog::test_reset_sinks();
    }
}

#[test]
fn a_reloads_settle_gets_the_keys_of_the_live_modules_and_the_new_file_in_its_slot() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        l.m.files.insert(PATH_B[..PATH_B.len() - 1].to_vec(), keyed_file(b'b', &[9]));
        booted(&mut l, 1, PATH_B);
        l.phase = Phase::Closed;
        // module `a` is taken down for its reload and the new file's table is kept in its slot: `b` and the slot of `a` are live
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 5]));
        assert_eq!(l.unload_rec(0), OK);
        let staged = l.m.stage(PATH.as_ptr()).unwrap();
        l.capture(0, core::slice::from_raw_parts(staged.ptr, staged.size as usize));
        let sel = live_settle_ctx(&l.tables, &l.table_len, &l.live_at(0));
        assert_eq!(sel.count, 2);
        assert_eq!((sel.nkeys[0], sel.keys[0][..3].to_vec()), (3, vec![1, 2, 5]));
        assert_eq!((sel.nkeys[1], sel.keys[1][0]), (1, 9));
        // a selection index with no live module hands over nothing, and the count stops at the last live one
        assert_eq!(l.live_at(SEL_MAX), [false, true, false, false, false, false, false, false], "the unloaded record is not live");
        let mut only_b = [false; SEL_MAX];
        only_b[1] = true;
        let sel = live_settle_ctx(&l.tables, &l.table_len, &only_b);
        assert_eq!((sel.count, sel.nkeys[0], sel.nkeys[1]), (2, 0, 1));
    }
}

#[test]
fn a_module_over_half_the_arena_is_not_refused_for_counting_itself_twice() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        let own = own_use(0);
        assert!(own.0 >= dkneeds::arena_block(32).unwrap(), "the image block of the module is counted");
        // an arena that the module fills to 60 percent
        let limit = own.0 * 5 / 3;
        let arena = |used| DkrtCap { kind: dkrt_sdk::proto::CAPK_ARENA as u32, limit, used, largest: limit };
        let mut caps = vec![arena(own.0)];
        caps.extend(wide().into_iter().filter(|c| c.kind != dkrt_sdk::proto::CAPK_ARENA as u32));
        live_caps(&mut caps, own);
        assert_eq!(caps[0].used, 0, "the old module's own blocks are given back");
        // without the subtraction the same arena counts the module that is about to leave
        let mut still = vec![arena(own.0)];
        live_caps(&mut still, (0, 0));
        assert_eq!(still[0].used, own.0);
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 7]));
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &caps), OK);
    }
}

#[test]
fn a_reload_load_checks_the_name_and_the_keys_of_the_bytes_it_loads() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a();
        assert_eq!(l.reload_check(0, PATH.as_ptr(), &wide()), OK);
        assert_eq!(l.unload_rec(0), OK);
        let table_before = (l.table_len[0], l.tables[0]);
        // the file is replaced while the module drains: one that drops a key, one of another module, one whose table is gone
        let mut hollow = keyed_file(b'a', &[1, 2]);
        put(&mut hollow, dkneeds::H_REG_SIZE, 0);
        seal(&mut hollow);
        for (what, f, want) in [
            ("a dropped key", keyed_file(b'a', &[2]), E_RELOAD_KEYS),
            ("another name", keyed_file(b'z', &[1, 2]), E_FORMAT),
            ("no table", hollow, E_FORMAT),
        ] {
            l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), f);
            assert_eq!(l.reload_load(0, PATH.as_ptr()), want, "{what}");
            assert_eq!(st().modules[0].used, 0, "{what}: nothing loaded");
            assert_eq!((l.table_len[0], l.tables[0]), table_before, "{what}: the kept table is the old one");
        }
        // the bytes that pass the rules load
        l.m.files.insert(PATH[..PATH.len() - 1].to_vec(), keyed_file(b'a', &[1, 2, 3]));
        assert_eq!(l.reload_load(0, PATH.as_ptr()), OK);
    }
}
