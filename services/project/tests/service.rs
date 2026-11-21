//! The `project` service over a fake store and a fake firmware list (docs/design/digikit-project.md section 7): load, save, rename, archive, paste, the busy store and the cut write.
use dkrt_sdk::logk::{
    PROJECT_DEFERRED, PROJECT_IGNORED, PROJECT_LOADED, PROJECT_MODULE, PROJECT_SAVED,
};
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use dkrt_sdk::{
    E_BUSY, E_CARD_BUSY, E_FORMAT, E_NOT_FOUND, E_PLAN_FIT, E_READ, E_RELOAD_PLAYING, E_SEL_COUNT,
    E_SEL_PATH, LIVE_LOAD, OK,
};
use dkrt_svc_project::dkp::{self, Ignore, Image, AREA};
use dkrt_svc_project::engine::*;
use dkrt_svc_project::name;
use std::collections::BTreeMap;

const DEV: u16 = 0x1234;
const OWNER: &[u8] = b"test/owner";

fn n16(s: &[u8]) -> [u8; 16] {
    let mut n = [0u8; 16];
    n[..s.len()].copy_from_slice(s);
    n
}

/// A project file as a store holds it: the image bytes up to `used`.
fn file_bytes(slot: u8, name: &[u8], stamp: u32, counter: u32, data: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    for (k, v) in data {
        let bare = k.strip_prefix(b"test/owner/").unwrap_or(k);
        dkp::put(&mut i.0, OWNER, bare, v).unwrap();
    }
    dkp::seal(&mut i.0, counter);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

fn path(slot: u32, name: &[u8]) -> Vec<u8> {
    name::file_path(slot, &n16(name)).as_bytes().to_vec()
}

/// The store, the firmware list and the log of the fake.
struct Fake {
    store: BTreeMap<Vec<u8>, Vec<u8>>,
    recs: BTreeMap<u32, SlotRecord>,
    live_op: u32,
    now: u32,
    // listing: rows to give, pending polls before them
    rows: Vec<Vec<u8>>,
    at: usize,
    pending_polls: u32,
    listing: bool,
    logs: Vec<(u16, [u32; 5])>,
    toasts: Vec<u32>,
    /// The settings record (device scope), by stored key, and the owners `changed` named, in order.
    record: BTreeMap<Vec<u8>, Vec<u8>>,
    changed: Vec<Vec<u8>>,
    posted: bool,
    // the store calls, in order: ("w", path) a file committed, ("d", path) a file removed
    ops: Vec<(char, Vec<u8>)>,
    session: Option<(Vec<u8>, Vec<u8>)>,
    /// `create` answers E_BUSY while this is set.
    busy: bool,
    /// `create` answers this status (a store that is full, foreign or off) and counts the tries.
    fail_create: Option<i32>,
    creates: u32,
    /// Writes and closes that were done (the write counter of the store).
    commits: u32,
    /// The step stops after the write and never calls `close`: a power cut.
    cut_before_close: bool,
    reads: u32,
    irq: std::cell::Cell<i32>,
    /// The slot the firmware has current at the start (`Env::firmware_slot`).
    firmware_slot: Option<u32>,
    /// `list_start` refuses with `refuse_status` this many times.
    refuse_list: u32,
    refuse_status: i32,
    /// A read of such a path answers `E_BUSY` this many times, then the file's own answer.
    busy_reads: BTreeMap<Vec<u8>, u32>,
    /// The next listing fails after its first poll (`ListPoll::Err`).
    fail_poll_once: bool,
    /// The answers of the next reads of a path, in order: `Some(status)` is a refusal, `None` the file's own answer; with the list used up reads answer as the file does.
    read_script: BTreeMap<Vec<u8>, std::collections::VecDeque<Option<i32>>>,
    /// The content modules loaded now, by module record: (id, store path). Their store hashes are in `hashes`.
    loaded: Vec<(Vec<u8>, Vec<u8>)>,
    hashes: BTreeMap<Vec<u8>, u32>,
    /// The paths of the live loads asked for, in order, and the answers of the next asks (`OK` when the list is used up).
    live_loads: Vec<Vec<u8>>,
    live_answers: std::collections::VecDeque<i32>,
    /// The runtime's live job: a queued job ends in the next pass of the rig with the next of `job_results` (`OK` when used up); `hold_jobs` keeps it running; `playing`
    /// makes every job end with the transport's refusal.
    job: LiveJob,
    job_results: std::collections::VecDeque<i32>,
    hold_jobs: bool,
    playing: bool,
    /// The paths added to the saved selection and the warnings shown.
    selection: Vec<Vec<u8>>,
    warnings: Vec<(ModuleWarning, Vec<u8>)>,
    /// The answers of the next `find_module_file` calls before the card's own, and the calls made (the searches read the whole store).
    find_script: std::collections::VecDeque<i32>,
    finds: u32,
    /// The store hashes read (a restore reads none) and the live loads asked, refused ones included.
    hash_reads: std::cell::Cell<u32>,
    live_asked: u32,
    /// `selection_add` answers this when set.
    selection_answer: Option<i32>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            store: BTreeMap::new(),
            recs: BTreeMap::new(),
            live_op: 0,
            now: 1000,
            rows: vec![],
            at: 0,
            pending_polls: 0,
            listing: false,
            logs: vec![],
            toasts: vec![],
            record: BTreeMap::new(),
            changed: vec![],
            posted: false,
            ops: vec![],
            session: None,
            busy: false,
            fail_create: None,
            creates: 0,
            commits: 0,
            cut_before_close: false,
            reads: 0,
            irq: std::cell::Cell::new(0),
            firmware_slot: None,
            refuse_list: 0,
            refuse_status: E_BUSY,
            busy_reads: BTreeMap::new(),
            fail_poll_once: false,
            read_script: BTreeMap::new(),
            loaded: vec![],
            hashes: BTreeMap::new(),
            live_loads: vec![],
            live_answers: Default::default(),
            job: LiveJob {
                phase: 0,
                result: 0,
                op: 0,
            },
            job_results: Default::default(),
            hold_jobs: false,
            playing: false,
            selection: vec![],
            warnings: vec![],
            find_script: Default::default(),
            finds: 0,
            hash_reads: Default::default(),
            live_asked: 0,
            selection_answer: None,
        }
    }
    /// A module file on the card, at `modules/<kind>/<author>-<name>.dkm`.
    fn card(&mut self, kind: &str, id: &str) {
        let p = format!("modules/{kind}/{}.dkm", id.replace('/', "-"));
        self.store.insert(p.into_bytes(), vec![0; 16]);
    }
    /// The runtime's job step (once per UI pass): a queued job ends, and a load that ends well leaves the module loaded.
    fn runtime_step(&mut self) {
        if self.job.phase != 1 || self.hold_jobs {
            return;
        }
        let result = if self.playing {
            E_RELOAD_PLAYING
        } else {
            self.job_results.pop_front().unwrap_or(OK)
        };
        self.job = LiveJob {
            phase: 2,
            result,
            op: LIVE_LOAD,
        };
        if result == OK {
            let path = self.live_loads.last().unwrap().clone();
            let file =
                String::from_utf8(path.rsplit(|c| *c == b'/').next().unwrap().to_vec()).unwrap();
            let id = file.trim_end_matches(".dkm").replacen('-', "/", 1);
            self.loaded.push((id.into_bytes(), path));
        }
    }
    fn loaded_ids(&self) -> Vec<Vec<u8>> {
        self.loaded.iter().map(|(id, _)| id.clone()).collect()
    }
    fn count(&self, kind: u16) -> usize {
        self.logs.iter().filter(|l| l.0 == kind).count()
    }
    fn last(&self, kind: u16) -> [u32; 5] {
        self.logs.iter().rev().find(|l| l.0 == kind).unwrap().1
    }
}

impl Env for Fake {
    fn device(&self) -> u16 {
        DEV
    }
    fn live_op(&self) -> u32 {
        self.live_op
    }
    fn now_ms(&self) -> u32 {
        self.now
    }
    fn irq_mask(&self) -> u32 {
        self.irq.set(self.irq.get() + 1);
        0
    }
    fn irq_restore(&self, _: u32) {
        self.irq.set(self.irq.get() - 1);
    }
    fn slot_record(&self, slot: u32) -> Option<SlotRecord> {
        self.recs.get(&slot).copied()
    }
    fn firmware_slot(&self) -> Option<u32> {
        self.firmware_slot
    }
    fn list_start(&mut self) -> i32 {
        if self.refuse_list > 0 {
            self.refuse_list -= 1;
            return self.refuse_status;
        }
        if !self.store.keys().any(|k| k.starts_with(b"projects/")) {
            return E_NOT_FOUND;
        }
        self.rows = self
            .store
            .keys()
            .filter(|k| k.starts_with(b"projects/") && !k[9..].contains(&b'/'))
            .cloned()
            .collect();
        // the archive folder is a row too
        if self
            .store
            .keys()
            .any(|k| k.starts_with(b"projects/archive/"))
        {
            self.rows.push(b"projects/archive".to_vec());
        }
        self.at = 0;
        self.pending_polls = 1;
        self.listing = true;
        OK
    }
    fn list_next(&mut self, row: &mut ListRow) -> ListPoll {
        assert!(self.listing);
        if self.pending_polls > 0 {
            self.pending_polls -= 1;
            return ListPoll::Pending;
        }
        if self.fail_poll_once {
            self.fail_poll_once = false;
            self.listing = false;
            return ListPoll::Err(E_BUSY);
        }
        if self.at >= self.rows.len() {
            self.listing = false;
            return ListPoll::End;
        }
        let r = &self.rows[self.at];
        self.at += 1;
        row.bytes[..r.len()].copy_from_slice(r);
        row.len = r.len();
        ListPoll::Row
    }
    fn read(&mut self, p: &[u8], buf: &mut [u8]) -> i32 {
        self.reads += 1;
        if let Some(Some(status)) = self.read_script.get_mut(p).and_then(|q| q.pop_front()) {
            return status;
        }
        if let Some(n) = self.busy_reads.get_mut(p) {
            if *n > 0 {
                *n -= 1;
                return E_BUSY;
            }
        }
        match self.store.get(p) {
            Some(b) if b.len() <= buf.len() => {
                buf[..b.len()].copy_from_slice(b);
                b.len() as i32
            }
            Some(_) => -2,
            None => E_NOT_FOUND,
        }
    }
    fn post(&mut self) -> i32 {
        assert!(!self.posted, "one job at a time");
        self.posted = true;
        OK
    }
    fn log(&mut self, kind: u16, args: [u32; 5]) {
        self.logs.push((kind, args));
    }
    fn toast(&mut self, reason: u32) {
        self.toasts.push(reason);
    }
    fn record_get(&mut self, key: &[u8], out: &mut [u8]) -> i32 {
        match self.record.get(key) {
            None => dkrt_sdk::proto::E_SET_NOKEY,
            Some(v) => {
                out[..v.len()].copy_from_slice(v);
                v.len() as i32
            }
        }
    }
    fn changed(&mut self, owner: &[u8]) {
        self.changed.push(owner.to_vec());
    }
    fn loaded_module(&self, record: usize) -> Option<LoadedModule> {
        let (id, path) = self.loaded.get(record)?;
        Some(LoadedModule {
            id: ModuleId::new(id)?,
            path: ModulePath::new(path)?,
        })
    }
    fn store_hash(&self, path: &[u8]) -> u32 {
        self.hash_reads.set(self.hash_reads.get() + 1);
        self.hashes.get(path).copied().unwrap_or(0)
    }
    fn find_module_file(&mut self, file: &[u8]) -> Result<ModulePath, i32> {
        assert_eq!(self.live_op, 0, "the store is read between live jobs only");
        self.finds += 1;
        if let Some(status) = self.find_script.pop_front() {
            return Err(status);
        }
        self.store
            .keys()
            .find(|k| k.starts_with(b"modules/") && k.rsplit(|c| *c == b'/').next() == Some(file))
            .and_then(|k| ModulePath::new(k))
            .ok_or(E_NOT_FOUND)
    }
    fn live_load(&mut self, path: &ModulePath) -> i32 {
        self.live_asked += 1;
        self.live_loads.push(path.as_bytes().to_vec());
        let answer = if self.job.phase == 1 {
            E_BUSY
        } else {
            self.live_answers.pop_front().unwrap_or(OK)
        };
        if answer == OK {
            self.job = LiveJob {
                phase: 1,
                result: 0,
                op: LIVE_LOAD,
            };
        } else {
            self.live_loads.pop();
        }
        answer
    }
    fn live_job(&self) -> LiveJob {
        self.job
    }
    fn selection_add(&mut self, path: &ModulePath) -> i32 {
        if let Some(status) = self.selection_answer {
            return status;
        }
        self.selection.push(path.as_bytes().to_vec());
        OK
    }
    fn warn_module(&mut self, warning: ModuleWarning, id: &ModuleId) {
        self.warnings.push((warning, id.as_bytes().to_vec()));
    }
}

impl Files for Fake {
    fn create(&mut self, p: &[u8], size: u32) -> i32 {
        self.creates += 1;
        if let Some(e) = self.fail_create {
            return e;
        }
        if self.busy {
            return E_BUSY;
        }
        assert!(self.session.is_none(), "one write session");
        assert!(size as usize <= AREA);
        self.session = Some((p.to_vec(), Vec::with_capacity(size as usize)));
        1
    }
    fn write(&mut self, h: i32, buf: &[u8]) -> i32 {
        assert_eq!(h, 1);
        self.session.as_mut().unwrap().1.extend_from_slice(buf);
        OK
    }
    fn close(&mut self, h: i32) -> i32 {
        assert_eq!(h, 1);
        if self.cut_before_close {
            // the power goes: the bytes were written, the table was not changed
            return dkrt_sdk::proto::E_WRITE;
        }
        let (p, b) = self.session.take().unwrap();
        self.commits += 1;
        self.ops.push(('w', p.clone()));
        self.store.insert(p, b);
        OK
    }
    fn abort(&mut self, _: i32) {
        self.session = None;
    }
    fn delete(&mut self, p: &[u8]) -> i32 {
        self.commits += 1;
        self.ops.push(('d', p.to_vec()));
        if self.store.remove(p).is_some() {
            OK
        } else {
            dkrt_sdk::proto::E_NOFILE
        }
    }
}

/// The service, its hooks' record and the fake: `tick` and the I/O task by hand.
struct Rig {
    p: Box<ProjectService>,
    pend: Pending,
    f: Fake,
}

impl Rig {
    fn new() -> Rig {
        let mut f = Fake::new();
        // the live mirror's stamp
        f.recs.insert(
            0x80,
            SlotRecord {
                name: n16(b"live"),
                stamp: 0xaaaa_0001,
            },
        );
        Rig {
            p: Box::new(ProjectService::new()),
            pend: Pending::new(),
            f,
        }
    }
    fn rec(&mut self, slot: u32, name: &[u8], stamp: u32) {
        self.f.recs.insert(
            slot,
            SlotRecord {
                name: n16(name),
                stamp,
            },
        );
    }
    fn hook_slot(&mut self, s: u32) {
        self.pend.slot_val = s;
        self.pend.slot_flag = 1;
    }
    fn hook_save(&mut self, s: u32) {
        self.pend.save_slot = s;
        self.pend.save_flag = 1;
    }
    fn hook_clear(&mut self, s: u32) {
        self.pend.clear[(s / 8) as usize] |= 1 << (s % 8);
    }
    fn hook_paste(&mut self, s: u32) {
        self.pend.paste[(s / 8) as usize] |= 1 << (s % 8);
    }
    /// One UI pass, then the I/O task runs the job that was posted.
    fn pass(&mut self) {
        self.p.tick(&mut self.f, &self.pend);
        self.pump();
        self.f.runtime_step();
    }
    fn pump(&mut self) {
        if self.f.posted {
            self.f.posted = false;
            let mut n = 0;
            let st = loop {
                let r = self.p.io_step(&mut self.f);
                if r != IO_MORE {
                    break r;
                }
                n += 1;
                assert!(n <= BUSY_RERUNS + 2);
            };
            if self.f.cut_before_close {
                return;
            }
            self.p.io_done(st);
        }
    }
    /// Passes until the service has nothing to do (time does not pass).
    fn settle(&mut self) {
        for _ in 0..200 {
            self.pass();
            if !self.p.busy() && !self.f.posted {
                // one more pass to take what a hook left
                self.p.tick(&mut self.f, &self.pend);
                self.pump();
                if !self.p.busy() {
                    return;
                }
            }
        }
        panic!("the service did not settle");
    }
    /// Make `slot` current as a stock load does: the record exists, the setter ran.
    fn load(&mut self, slot: u32) {
        self.hook_slot(slot);
        self.settle();
    }
    fn put(&mut self, k: &[u8], v: &[u8]) -> i32 {
        self.p.scope_put(OWNER, k, v)
    }
    fn get(&self, k: &[u8]) -> Vec<u8> {
        let mut out = [0u8; 64];
        let n = self.p.scope_get(OWNER, k, &mut out);
        assert!(n >= 0, "get {k:?}: {n}");
        out[..n as usize].to_vec()
    }
}

#[test]
fn a_project_without_a_file_loads_empty_with_no_toast_no_write_and_no_delete() {
    let mut r = Rig::new();
    r.rec(5, b"NEW PROJECT", 0x1111_0005);
    r.load(5);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.f.count(PROJECT_LOADED), 1);
    assert_eq!(&r.f.last(PROJECT_LOADED)[..4], &[5, 0, 0, 0]);
    assert!(r.f.toasts.is_empty() && r.f.ops.is_empty() && r.f.store.is_empty());
    assert!(r.f.changed.is_empty(), "no module settings changed");
    assert_eq!(r.p.status(), 0);
    // the scope is on and empty
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // a stock save of a project nobody put anything into writes nothing either
    r.hook_save(5);
    r.settle();
    assert!(r.f.ops.is_empty() && r.f.store.is_empty());
    assert_eq!(
        r.f.count(PROJECT_SAVED),
        1,
        "SAVED ends the save also when there was nothing to write"
    );
    assert_eq!(&r.f.last(PROJECT_SAVED)[..4], &[5, 0, 0, 0]);
}

#[test]
fn a_file_with_the_stamp_of_the_slot_is_installed_and_the_loaded_event_follows_the_data() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.f.store.insert(
        path(12, b"MY SONG"),
        file_bytes(
            12,
            b"MY SONG",
            0x1234_5678,
            3,
            &[(b"test/owner/pool", b"abc")],
        ),
    );
    r.load(12);
    assert_eq!(r.get(b"pool"), b"abc");
    let l = r.f.last(PROJECT_LOADED);
    assert_eq!((l[0], l[3]), (12, 3), "slot and write counter");
    assert!(l[1] > 42);
    assert_eq!(r.p.status(), ST_FILE);
    assert!(
        r.f.ops.is_empty(),
        "a load of a file with the right name writes nothing"
    );
}

#[test]
fn a_stamp_mismatch_gives_one_warning_and_leaves_the_files_alone() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    let old = file_bytes(
        12,
        b"MY SONG",
        0x0bad_f00d,
        1,
        &[(b"test/owner/pool", b"old")],
    );
    r.f.store.insert(path(12, b"MY SONG"), old.clone());
    r.load(12);
    assert_eq!(r.f.toasts, [Ignore::Stamp as u32]);
    assert_eq!(r.f.last(PROJECT_IGNORED)[..2], [12, 4]);
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.f.store[&path(12, b"MY SONG")], old);
    assert!(r.f.ops.is_empty());
    // the image is empty and the project is current
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"pool", &mut out), E_SET_NOKEY);
    assert_eq!(r.p.status(), ST_IGNORED);
}

#[test]
fn a_damaged_file_a_file_of_another_device_and_a_newer_version_are_not_used() {
    for (what, reason) in [
        ("crc", Ignore::Format),
        ("device", Ignore::Device),
        ("version", Ignore::Version),
    ] {
        let mut r = Rig::new();
        r.rec(3, b"X", 7);
        let mut b = file_bytes(3, b"X", 7, 1, &[(b"test/owner/k", b"v")]);
        match what {
            "crc" => {
                let n = b.len();
                b[n - 1] ^= 1;
            }
            // a file of another device or version is whole: its crc32 is right
            "device" => {
                b[6..8].copy_from_slice(&0x7777u16.to_be_bytes());
                dkp::seal(&mut b, 1);
            }
            _ => {
                b[4..6].copy_from_slice(&2u16.to_be_bytes());
                dkp::seal(&mut b, 1);
            }
        }
        r.f.store.insert(path(3, b"X"), b.clone());
        r.load(3);
        assert_eq!(r.f.toasts, [reason as u32], "{what}");
        assert_eq!(r.f.store[&path(3, b"X")], b, "{what}: left in place");
    }
}

#[test]
fn a_save_writes_the_file_with_the_header_of_the_slot_and_a_counter_that_grows() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.load(12);
    assert_eq!(r.put(b"pool", b"abc"), OK);
    assert_eq!(r.p.status(), ST_DIRTY);
    r.hook_save(12);
    r.settle();
    let p = path(12, b"MY SONG");
    let b = &r.f.store[&p];
    let h = dkp::check(b, DEV).unwrap();
    assert_eq!(
        (h.slot, h.stamp, h.counter, h.device),
        (12, 0x1234_5678, 1, DEV)
    );
    assert_eq!(&h.name[..h.name_len as usize], b"MY SONG");
    assert_eq!(r.f.ops, [('w', p.clone())]);
    assert_eq!(r.f.count(PROJECT_SAVED), 1);
    assert_eq!(r.f.last(PROJECT_SAVED)[..4], [12, h.used, 1, 0]);
    assert_eq!(r.p.status(), ST_FILE);
    // a save with nothing new writes nothing (no card write for the stock save of a project nobody touched)
    let commits = r.f.commits;
    r.hook_save(12);
    r.settle();
    assert_eq!(r.f.commits, commits);
    // a put of the stored value is no change, a changed value is
    assert_eq!(r.put(b"pool", b"abc"), OK);
    assert_eq!(r.p.status(), ST_FILE);
    assert_eq!(r.put(b"pool", b"abd"), OK);
    r.hook_save(12);
    r.settle();
    assert_eq!(dkp::head_of(&r.f.store[&p]).counter, 2);
    assert_eq!(r.f.store.len(), 1);
}

#[test]
fn a_load_without_a_save_discards_the_puts() {
    let mut r = Rig::new();
    r.rec(12, b"A", 1);
    r.rec(13, b"B", 2);
    r.f.store.insert(
        path(12, b"A"),
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"saved")]),
    );
    r.load(12);
    assert_eq!(r.put(b"k", b"unsaved"), OK);
    r.load(13);
    assert_eq!(
        r.f.store[&path(12, b"A")],
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"saved")])
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"saved");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn the_scope_is_off_without_a_current_project() {
    let mut r = Rig::new();
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    assert_eq!(r.p.scope_remove(OWNER, b"k"), E_SET_OFF);
    // the live mirror and -1 mean no project
    r.rec(4, b"X", 1);
    r.load(4);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), OK);
    r.load(0x80);
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    r.load(u32::MAX);
    assert_eq!(r.p.slot(), -1);
    assert!(r.p.current().is_err());
}

#[test]
fn a_stock_rename_writes_the_new_name_first_and_removes_the_old_file_after() {
    let mut r = Rig::new();
    r.rec(12, b"OLD", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(12);
    // the stock rename keeps the stamp
    r.rec(12, b"NEW", 9);
    r.hook_save(12);
    r.settle();
    assert_eq!(r.f.ops, [('w', path(12, b"NEW")), ('d', path(12, b"OLD"))]);
    let h = dkp::check(&r.f.store[&path(12, b"NEW")], DEV).unwrap();
    assert_eq!(
        (h.counter, &h.name[..h.name_len as usize]),
        (5, &b"NEW"[..])
    );
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_rename_of_the_current_project_with_a_new_stamp_replaces_the_bound_file_unarchived() {
    let mut r = Rig::new();
    r.rec(1, b"A", 9);
    r.f.store.insert(
        path(1, b"A"),
        file_bytes(1, b"A", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(1);
    // the stock rename of the current project gives it a new name and a new stamp
    r.rec(1, b"B", 77);
    r.hook_save(1);
    r.settle();
    assert_eq!(r.f.ops, [('w', path(1, b"B")), ('d', path(1, b"A"))]);
    let h = dkp::check(&r.f.store[&path(1, b"B")], DEV).unwrap();
    assert_eq!((h.stamp, h.counter), (77, 5));
    assert!(archived(&r).is_empty(), "{:?}", archived(&r));
    assert_eq!(r.f.store.len(), 1);
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_file_of_the_slot_with_another_stamp_that_was_not_bound_is_still_archived() {
    let mut r = Rig::new();
    r.rec(1, b"A", 9);
    r.f.store.insert(
        path(1, b"A"),
        file_bytes(1, b"A", 9, 4, &[(b"test/owner/k", b"v")]),
    );
    r.load(1);
    let stale = file_bytes(1, b"C", 5, 2, &[(b"test/owner/k", b"c")]);
    r.f.store.insert(path(1, b"C"), stale.clone());
    r.hook_save(1);
    r.put(b"k", b"w");
    r.settle();
    assert_eq!(r.f.store[&archive(b"C", 5)], stale);
    assert!(!r.f.store.contains_key(&path(1, b"C")));
    assert!(r.f.store.contains_key(&path(1, b"A")));
}

#[test]
fn a_cut_between_the_new_name_and_the_removal_leaves_two_files_and_the_higher_counter_wins() {
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 4, &[(b"test/owner/k", b"old")]),
    );
    r.f.store.insert(
        path(12, b"NEW"),
        file_bytes(12, b"NEW", 9, 5, &[(b"test/owner/k", b"new")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"new");
    assert_eq!(
        r.f.ops,
        [('d', path(12, b"OLD"))],
        "the loser is removed, a superseded copy of the same project"
    );
    assert!(r.f.toasts.is_empty());
    // the same with the old name at the higher counter: the file is renamed on load
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 8, &[(b"test/owner/k", b"old")]),
    );
    r.f.store.insert(
        path(12, b"NEW"),
        file_bytes(12, b"NEW", 9, 5, &[(b"test/owner/k", b"new")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"old");
    assert_eq!(
        r.f.ops,
        [
            ('d', path(12, b"NEW")),
            ('w', path(12, b"NEW")),
            ('d', path(12, b"OLD"))
        ]
    );
    assert_eq!(dkp::head_of(&r.f.store[&path(12, b"NEW")]).counter, 9);
    assert_eq!(r.f.store.len(), 1);
}

#[test]
fn a_load_renames_a_file_whose_project_was_renamed_outside_digikit() {
    let mut r = Rig::new();
    r.rec(12, b"NEW", 9);
    r.f.store.insert(
        path(12, b"OLD"),
        file_bytes(12, b"OLD", 9, 2, &[(b"test/owner/k", b"v")]),
    );
    r.load(12);
    assert_eq!(r.get(b"k"), b"v");
    assert_eq!(r.f.ops, [('w', path(12, b"NEW")), ('d', path(12, b"OLD"))]);
    assert_eq!(r.f.store.len(), 1);
    assert!(r.f.toasts.is_empty());
}

fn archive(name: &[u8], stamp: u32) -> Vec<u8> {
    name::archive_path(&n16(name), stamp).as_bytes().to_vec()
}

#[test]
fn a_save_into_a_reused_slot_archives_the_old_file_before_the_new_one_is_written() {
    let mut r = Rig::new();
    // slot 20 held another project; it was deleted and a new one saved there (a new stamp)
    let old = file_bytes(20, b"OLD ONE", 0x0000_00aa, 3, &[(b"test/owner/k", b"old")]);
    r.f.store.insert(path(20, b"OLD ONE"), old.clone());
    r.rec(20, b"FRESH", 0x0000_00bb);
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.hook_slot(20);
    r.hook_save(20);
    r.put(b"k", b"mine");
    r.settle();
    // the project that was current went to slot 20 (a save-as), the stale file was archived
    assert_eq!(
        r.f.ops,
        [
            ('w', archive(b"OLD ONE", 0xaa)),
            ('d', path(20, b"OLD ONE")),
            ('w', path(20, b"FRESH"))
        ]
    );
    assert_eq!(
        r.f.store[&archive(b"OLD ONE", 0xaa)],
        old,
        "the archive copy is the old file's bytes"
    );
    let h = dkp::check(&r.f.store[&path(20, b"FRESH")], DEV).unwrap();
    assert_eq!((h.slot, h.stamp, h.counter), (20, 0xbb, 1));
    assert_eq!(r.p.slot(), 20);
}

#[test]
fn a_stock_clear_archives_the_slots_file_and_empties_the_image_when_the_slot_is_current() {
    let mut r = Rig::new();
    r.rec(9, b"DOOMED", 0x77);
    let b = file_bytes(9, b"DOOMED", 0x77, 2, &[(b"test/owner/k", b"v")]);
    r.f.store.insert(path(9, b"DOOMED"), b.clone());
    r.load(9);
    assert_eq!(r.get(b"k"), b"v");
    // the clear keeps the stamp and writes NEW PROJECT as the name
    r.rec(9, b"NEW PROJECT", 0x77);
    r.hook_clear(9);
    r.settle();
    assert_eq!(
        r.f.ops,
        [('w', archive(b"DOOMED", 0x77)), ('d', path(9, b"DOOMED"))]
    );
    assert_eq!(r.f.store[&archive(b"DOOMED", 0x77)], b);
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // a clear of another slot touches only that slot's file
    r.rec(30, b"OTHER", 5);
    r.f.store.insert(
        path(30, b"OTHER"),
        file_bytes(30, b"OTHER", 5, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.ops.clear();
    r.hook_clear(30);
    r.settle();
    assert!(r.f.ops.contains(&('d', path(30, b"OTHER"))));
    assert!(r.f.store.contains_key(&archive(b"OTHER", 5)));
    // the next stock save of the cleared project must not bring the old data back
    r.hook_save(9);
    r.settle();
    assert!(!r.f.store.keys().any(|k| k.starts_with(b"projects/010")));
}

#[test]
fn a_clear_of_several_slots_archives_each_one() {
    let mut r = Rig::new();
    for s in [3u32, 40, 90] {
        r.rec(s, b"P", 100 + s);
        r.f.store.insert(
            path(s, b"P"),
            file_bytes(s as u8, b"P", 100 + s, 1, &[(b"test/owner/k", b"v")]),
        );
        r.hook_clear(s);
    }
    r.settle();
    assert!(!r
        .f
        .store
        .keys()
        .any(|k| k.starts_with(b"projects/0") && !k.starts_with(b"projects/archive")));
    for s in [3u32, 40, 90] {
        assert!(r.f.store.contains_key(&archive(b"P", 100 + s)));
    }
}

#[test]
fn a_paste_copies_the_source_file_to_the_destination_with_its_own_header() {
    let mut r = Rig::new();
    // slot 2 holds the source; slot 50 was pasted over (stamp and name are the source's)
    r.f.store.insert(
        path(2, b"SONG"),
        file_bytes(2, b"SONG", 0x42, 6, &[(b"test/owner/k", b"data")]),
    );
    // a stale file in the destination slot
    r.f.store.insert(
        path(50, b"OLDER"),
        file_bytes(50, b"OLDER", 0x99, 1, &[(b"test/owner/k", b"stale")]),
    );
    r.rec(50, b"SONG", 0x42);
    r.hook_paste(50);
    r.settle();
    let h = dkp::check(&r.f.store[&path(50, b"SONG")], DEV).unwrap();
    assert_eq!((h.slot, h.stamp, h.counter), (50, 0x42, 1));
    assert!(
        r.f.store.contains_key(&path(2, b"SONG")),
        "the source stays"
    );
    assert!(r.f.store.contains_key(&archive(b"OLDER", 0x99)));
    assert!(!r.f.store.contains_key(&path(50, b"OLDER")));
    // loading the copy gives the source's data
    r.load(50);
    assert_eq!(r.get(b"k"), b"data");
    assert!(r.f.toasts.is_empty());
}

#[test]
fn a_paste_with_no_source_file_copies_nothing() {
    let mut r = Rig::new();
    r.f.store.insert(
        path(2, b"SONG"),
        file_bytes(2, b"SONG", 0x41, 1, &[(b"test/owner/k", b"d")]),
    );
    r.rec(50, b"SONG", 0x42);
    let before = r.f.store.clone();
    r.hook_paste(50);
    r.settle();
    assert_eq!(r.f.store, before);
}

#[test]
fn a_busy_store_defers_after_30_seconds_keeps_the_data_dirty_and_the_next_save_writes() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"v");
    r.f.busy = true;
    r.hook_save(12);
    // each pass the job answers E_BUSY 1000 times, fails, and the tick tries again a second later
    let mut deferred_at = None;
    for _ in 0..100 {
        r.pass();
        if r.f.count(PROJECT_DEFERRED) > 0 && deferred_at.is_none() {
            deferred_at = Some(r.f.now);
        }
        r.f.now += 500;
        if r.f.count(PROJECT_SAVED) > 0 {
            break;
        }
    }
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "once");
    let waited = deferred_at.unwrap() - 1000;
    assert!((30_000..=32_000).contains(&waited), "{waited}");
    assert_eq!(r.f.last(PROJECT_DEFERRED)[1] as i32, E_BUSY);
    assert_eq!(
        r.f.last(PROJECT_SAVED)[3] as i32,
        E_BUSY,
        "SAVED carries the code"
    );
    assert_eq!(
        r.p.status() & (ST_DIRTY | ST_DEFERRED),
        ST_DIRTY | ST_DEFERRED
    );
    // no retry loop after the event: more passes do not touch the store
    for _ in 0..10 {
        r.pass();
        r.f.now += 1000;
    }
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    // the store is free: the next save writes
    r.f.busy = false;
    r.hook_save(12);
    r.settle();
    assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_busy_store_that_frees_within_a_second_writes_without_an_event() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"v");
    r.f.busy = true;
    r.hook_save(12);
    r.pass();
    assert!(r.f.store.is_empty());
    r.f.busy = false;
    r.f.now += 1000;
    r.settle();
    assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    assert_eq!(r.f.last(PROJECT_SAVED)[3], 0);
}

#[test]
fn a_cut_in_the_middle_of_a_write_keeps_the_old_file() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    let old = file_bytes(12, b"MY SONG", 5, 1, &[(b"test/owner/k", b"old")]);
    r.f.store.insert(path(12, b"MY SONG"), old.clone());
    r.load(12);
    r.put(b"k", b"new");
    r.f.cut_before_close = true;
    r.hook_save(12);
    for _ in 0..5 {
        r.pass();
    }
    // the file session is open and never closed: the power is gone
    assert!(r.f.session.is_some());
    assert_eq!(r.f.store[&path(12, b"MY SONG")], old);
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
}

#[test]
fn a_save_as_keeps_the_data_and_a_new_project_does_not() {
    // save-as: the setter and the save name slot 30, the firmware's live stamp is the one the image was bound to
    let mut r = Rig::new();
    r.rec(7, b"WORK", 0x10);
    r.load(7);
    r.put(b"k", b"mine");
    r.rec(30, b"WORK", 0x10);
    r.hook_slot(30);
    r.hook_save(30);
    r.settle();
    assert_eq!(r.p.slot(), 30);
    assert_eq!(
        dkp::check(&r.f.store[&path(30, b"WORK")], DEV)
            .unwrap()
            .slot,
        30
    );
    assert_eq!(r.get(b"k"), b"mine");
    // a new project: the live stamp changed (the firmware made a new one), so the data does not follow
    let mut r = Rig::new();
    r.rec(7, b"WORK", 0x10);
    r.load(7);
    r.put(b"k", b"mine");
    r.f.recs.insert(
        0x80,
        SlotRecord {
            name: n16(b"live"),
            stamp: 0xbbbb_0002,
        },
    );
    r.rec(31, b"NEW PROJECT", 0x20);
    r.hook_slot(31);
    r.hook_save(31);
    r.settle();
    assert_eq!(r.p.slot(), 31);
    assert!(r.f.store.is_empty(), "no data, no file");
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
}

#[test]
fn the_mirror_slot_and_a_slot_past_127_never_write() {
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    for s in [0x80u32, 200, u32::MAX] {
        r.hook_save(s);
        r.settle();
    }
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
}

#[test]
fn a_load_waits_while_a_live_operation_runs() {
    let mut r = Rig::new();
    r.rec(12, b"A", 1);
    r.f.store.insert(
        path(12, b"A"),
        file_bytes(12, b"A", 1, 1, &[(b"test/owner/k", b"v")]),
    );
    r.f.live_op = 3;
    r.hook_slot(12);
    for _ in 0..5 {
        r.pass();
    }
    assert_eq!(r.f.reads, 0);
    assert_eq!(r.p.slot(), -1);
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.get(b"k"), b"v");
}

#[test]
fn a_save_waits_for_the_firmwares_record_of_its_slot_and_drops_after_two_seconds() {
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    r.hook_save(7);
    r.f.recs.remove(&7);
    r.pass();
    assert!(r.f.store.is_empty());
    r.f.recs.insert(
        7,
        SlotRecord {
            name: n16(b"WORK"),
            stamp: 1,
        },
    );
    r.settle();
    assert!(r.f.store.contains_key(&path(7, b"WORK")));
    // no record for longer than the wait: the save is dropped
    let mut r = Rig::new();
    r.rec(7, b"WORK", 1);
    r.load(7);
    r.put(b"k", b"v");
    r.f.recs.remove(&7);
    r.hook_save(7);
    r.pass();
    r.f.now += REC_WAIT_MS + 1;
    r.settle();
    assert!(r.f.store.is_empty());
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [7, Ignore::SaveNoRecord as u32, 0, 0, 0],
        "the dropped save is logged"
    );
}

#[test]
fn the_hooks_record_is_taken_with_interrupts_masked_and_cleared() {
    let mut r = Rig::new();
    r.rec(4, b"X", 1);
    r.hook_slot(4);
    r.pass();
    assert_eq!(r.f.irq.get(), 0, "masks are balanced");
    assert_eq!(r.pend.slot_flag, 0);
    assert_eq!(r.pend.clear, [0; 16]);
}

#[test]
fn any_status_but_busy_ends_the_attempt_with_no_retry_and_the_data_stays_dirty() {
    for code in [
        dkrt_sdk::proto::E_FULL,
        dkrt_sdk::proto::E_INUSE,
        dkrt_sdk::proto::E_NOSTORE,
        dkrt_sdk::proto::E_WRITE,
    ] {
        let mut r = Rig::new();
        r.rec(12, b"MY SONG", 5);
        r.load(12);
        r.put(b"k", b"v");
        r.f.fail_create = Some(code);
        r.hook_save(12);
        r.settle();
        assert_eq!(r.f.creates, 1, "{code}: one try");
        assert_eq!(r.f.last(PROJECT_SAVED)[3] as i32, code);
        assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
        for _ in 0..5 {
            r.f.now += 2000;
            r.pass();
        }
        assert_eq!(r.f.creates, 1, "{code}: no retry loop");
        assert_eq!(r.p.status(), ST_DIRTY);
        // the next save writes
        r.f.fail_create = None;
        r.hook_save(12);
        r.settle();
        assert_eq!(r.p.status(), ST_FILE);
        assert!(r.f.store.contains_key(&path(12, b"MY SONG")));
    }
}

#[test]
fn a_put_after_the_freeze_keeps_the_image_dirty_for_the_next_save() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 5);
    r.load(12);
    r.put(b"k", b"one");
    r.hook_save(12);
    // up to the post of the job (list, probe), then a put before the job ends
    for _ in 0..4 {
        r.p.tick(&mut r.f, &r.pend);
        if r.f.posted {
            break;
        }
    }
    assert!(r.f.posted);
    r.put(b"k", b"two");
    r.pump();
    r.settle();
    assert_eq!(r.p.status(), ST_FILE | ST_DIRTY);
    r.hook_save(12);
    r.settle();
    assert_eq!(r.p.status(), ST_FILE);
    assert_eq!(dkp::head_of(&r.f.store[&path(12, b"MY SONG")]).counter, 2);
}

/// A project file with entries of several owners: (owner, bare key, value).
fn file_owned(slot: u8, name: &[u8], stamp: u32, data: &[(&[u8], &[u8], &[u8])]) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    for (o, k, v) in data {
        dkp::put(&mut i.0, o, k, v).unwrap();
    }
    dkp::seal(&mut i.0, 1);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

fn changed(r: &mut Rig) -> Vec<String> {
    let mut v: Vec<String> =
        r.f.changed
            .drain(..)
            .map(|o| String::from_utf8(o).unwrap())
            .collect();
    v.sort();
    v
}

#[test]
fn only_the_modules_whose_effective_settings_differ_are_named_when_a_project_loads() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(6, b"TWO", 2);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(
            5,
            b"ONE",
            1,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"2")],
        ),
    );
    r.f.store.insert(
        path(6, b"TWO"),
        file_owned(
            6,
            b"TWO",
            2,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"9")],
        ),
    );
    // the first project: no value in the record, so both modules' values change from their defaults
    r.load(5);
    assert_eq!(changed(&mut r), ["test/a", "test/b"]);
    // the record holds what the last project wrote (a write goes to both)
    r.f.record.insert(b"test/a/k".to_vec(), b"1".to_vec());
    r.f.record.insert(b"test/b/k".to_vec(), b"2".to_vec());
    // the second project holds the same value for a and another for b
    r.load(6);
    assert_eq!(changed(&mut r), ["test/b"], "a reads 1 in both projects");
}

#[test]
fn a_project_without_a_file_changes_only_what_differs_from_the_record() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(7, b"NEW", 3);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(
            5,
            b"ONE",
            1,
            &[(b"test/a", b"k", b"1"), (b"test/b", b"k", b"2")],
        ),
    );
    r.f.record.insert(b"test/a/k".to_vec(), b"1".to_vec());
    r.f.record.insert(b"test/b/k".to_vec(), b"2".to_vec());
    r.load(5);
    changed(&mut r);
    // a new project starts from the record: nothing reads another value
    r.load(7);
    assert!(changed(&mut r).is_empty());
    // the record moved on (another project wrote it): the project that held the old value differs now
    r.f.record.insert(b"test/a/k".to_vec(), b"5".to_vec());
    r.load(5);
    assert_eq!(
        changed(&mut r),
        ["test/a"],
        "b equals the record, a does not"
    );
}

#[test]
fn a_key_only_one_project_holds_and_a_cleared_or_unloaded_project_count_too() {
    let mut r = Rig::new();
    r.rec(5, b"ONE", 1);
    r.rec(6, b"TWO", 2);
    r.f.store.insert(
        path(5, b"ONE"),
        file_owned(5, b"ONE", 1, &[(b"test/a", b"k", b"1")]),
    );
    r.f.store.insert(
        path(6, b"TWO"),
        file_owned(6, b"TWO", 2, &[(b"test/c", b"j", b"x")]),
    );
    r.f.record.insert(b"test/a/k".to_vec(), b"0".to_vec());
    r.load(5);
    changed(&mut r);
    // 6 holds c/j only; 5's a/k (1) against the record (0) differs, and c/j against nothing differs
    r.load(6);
    assert_eq!(changed(&mut r), ["test/a", "test/c"]);
    // no current project: every key reads the record again
    r.load(0x80);
    assert_eq!(changed(&mut r), ["test/c"]);
    // clearing the current project empties its image the same way
    r.load(6);
    changed(&mut r);
    r.rec(6, b"NEW PROJECT", 2);
    r.hook_clear(6);
    r.settle();
    assert_eq!(changed(&mut r), ["test/c"]);
}

/// The files of the archive folder.
fn archived(r: &Rig) -> Vec<Vec<u8>> {
    r.f.store
        .keys()
        .filter(|k| k.starts_with(b"projects/archive/"))
        .cloned()
        .collect()
}

#[test]
fn a_project_that_was_current_at_the_start_is_bound_without_a_setter_call_and_its_save_writes_it() {
    let mut r = Rig::new();
    r.rec(5, b"MY SONG", 0x0000_0777);
    let file = file_bytes(5, b"MY SONG", 0x777, 2, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"MY SONG"), file);
    r.f.firmware_slot = Some(5);
    // no hook ran: the stock setter is not called for a project that was already current
    r.settle();
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.p.status(), ST_FILE);
    // the first save writes the bound project under its own slot: it is no save-as, nothing is archived
    r.put(b"k", b"changed");
    r.hook_save(5);
    r.settle();
    assert!(archived(&r).is_empty(), "{:?}", archived(&r));
    let head = dkp::check(&r.f.store[&path(5, b"MY SONG")], DEV).unwrap();
    assert_eq!((head.slot, head.stamp, head.counter), (5, 0x777, 3));
    assert_eq!(r.get(b"k"), b"changed");
}

#[test]
fn a_setter_call_before_the_first_read_names_the_current_project_and_the_read_stays_off() {
    let mut r = Rig::new();
    r.rec(5, b"A", 1);
    r.rec(6, b"B", 2);
    r.f.firmware_slot = Some(5);
    r.hook_slot(6);
    r.settle();
    assert_eq!(
        r.p.slot(),
        6,
        "the setter's slot wins over the read at the start"
    );
    assert_eq!(r.f.count(PROJECT_LOADED), 1);
}

#[test]
fn a_project_whose_record_is_not_listed_yet_is_bound_when_the_record_appears() {
    let mut r = Rig::new();
    r.f.firmware_slot = Some(5);
    r.settle();
    assert_eq!(r.p.slot(), -1, "no record of the slot yet");
    r.rec(5, b"LATE", 9);
    r.settle();
    assert_eq!(r.p.slot(), 5);
}

#[test]
fn two_archives_of_the_same_name_and_stamp_keep_both_files() {
    let mut r = Rig::new();
    // a paste copies the name and the stamp: two slots hold a project of the same name and stamp (different data)
    r.rec(5, b"PROJ", 0x55);
    r.rec(6, b"PROJ", 0x55);
    let first = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"first")]);
    let second = file_bytes(6, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"second")]);
    r.f.store.insert(path(5, b"PROJ"), first.clone());
    r.f.store.insert(path(6, b"PROJ"), second.clone());
    r.hook_clear(5);
    r.settle();
    r.hook_clear(6);
    r.settle();
    let first_copy = archive(b"PROJ", 0x55);
    let mut second_copy = first_copy.clone();
    second_copy.truncate(second_copy.len() - 4);
    second_copy.extend_from_slice(b"-2.dkp");
    let mut expected = [first_copy.clone(), second_copy.clone()];
    expected.sort();
    assert_eq!(archived(&r), expected);
    assert_eq!(r.f.store[&first_copy], first);
    assert_eq!(r.f.store[&second_copy], second);
    assert!(
        !r.f.store.contains_key(&path(5, b"PROJ")) && !r.f.store.contains_key(&path(6, b"PROJ"))
    );
}

#[test]
fn an_archive_name_with_no_free_suffix_leaves_the_file_where_it_is() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    for copy in 1..=name::ARCHIVE_COPIES_MAX {
        let taken = name::archive_path_copy(&n16(b"PROJ"), 0x55, copy);
        r.f.store
            .insert(taken.as_bytes().to_vec(), b"older".to_vec());
    }
    r.hook_clear(5);
    r.settle();
    assert_eq!(
        r.f.store[&path(5, b"PROJ")],
        file,
        "never delete what was not archived"
    );
    for copy in 1..=name::ARCHIVE_COPIES_MAX {
        let taken = name::archive_path_copy(&n16(b"PROJ"), 0x55, copy);
        assert_eq!(
            r.f.store[taken.as_bytes()],
            b"older",
            "copy {copy} is untouched"
        );
    }
}

#[test]
fn a_clear_whose_listing_the_store_refuses_runs_again_after_the_retry_time() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = 1;
    r.hook_clear(5);
    r.settle();
    assert!(r.f.ops.is_empty(), "nothing yet: the clear waits");
    r.settle();
    assert!(
        r.f.ops.is_empty(),
        "and it waits for the clock, not for the passes"
    );
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(archived(&r), [archive(b"PROJ", 0x55)]);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}

#[test]
fn a_load_whose_listing_fails_half_way_does_not_bind_from_partial_rows() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.f.store.insert(
        path(12, b"MY SONG"),
        file_bytes(
            12,
            b"MY SONG",
            0x1234_5678,
            3,
            &[(b"test/owner/pool", b"abc")],
        ),
    );
    r.f.fail_poll_once = true;
    r.load(12);
    assert_eq!(r.p.slot(), -1, "the project is not bound with no file");
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(r.p.slot(), 12);
    assert_eq!(r.get(b"pool"), b"abc");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_save_that_archives_the_refused_file_ends_the_ignored_state() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    let stale = file_bytes(
        12,
        b"MY SONG",
        0x0bad_f00d,
        1,
        &[(b"test/owner/pool", b"old")],
    );
    r.f.store.insert(path(12, b"MY SONG"), stale);
    r.load(12);
    assert_eq!(r.p.status(), ST_IGNORED);
    r.put(b"k", b"mine");
    r.hook_save(12);
    r.settle();
    assert_eq!(archived(&r), [archive(b"MY SONG", 0x0bad_f00d)]);
    assert_eq!(
        r.p.status(),
        ST_FILE,
        "written, clean and no longer ignored"
    );
}

#[test]
fn every_module_is_named_once_however_many_modules_differ() {
    let mut r = Rig::new();
    // 24 owners with two keys each: more than the table of named owners holds
    let owners: Vec<Vec<u8>> = (0..24)
        .map(|i| format!("test/own{i:02}").into_bytes())
        .collect();
    let keys: Vec<(&[u8], &[u8], &[u8])> = owners
        .iter()
        .flat_map(|o| {
            [
                (o.as_slice(), &b"a"[..], &b"1"[..]),
                (o.as_slice(), &b"b"[..], &b"2"[..]),
            ]
        })
        .collect();
    r.rec(9, b"BIG", 0x99);
    r.f.store
        .insert(path(9, b"BIG"), file_owned(9, b"BIG", 0x99, &keys));
    r.load(9);
    let named = changed(&mut r);
    assert_eq!(named.len(), 24, "each owner once: {named:?}");
    let mut once = named.clone();
    once.dedup();
    assert_eq!(once, named);
}

/// Passes with the clock moving `RETRY_MS` and a little more each time, `rounds` times.
fn settle_over(r: &mut Rig, rounds: u32) {
    for _ in 0..rounds {
        r.f.now += RETRY_MS + 1;
        r.settle();
    }
}

#[test]
fn a_load_whose_store_stays_busy_gives_up_after_30_seconds_and_leaves_no_project_bound() {
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(5, b"NEW", 2);
    r.f.store.insert(
        path(4, b"OLD"),
        file_bytes(4, b"OLD", 1, 1, &[(b"test/owner/k", b"old")]),
    );
    r.load(4);
    assert_eq!(r.p.slot(), 4);
    r.f.refuse_list = u32::MAX;
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), 4, "while it retries the old project stays");
    settle_over(&mut r, 29);
    assert_eq!(r.p.slot(), 4, "29 seconds are not 30");
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    settle_over(&mut r, 3);
    assert_eq!(r.p.slot(), -1, "no project: module writes go to the record");
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), dkrt_sdk::proto::E_SET_OFF);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "one event");
    assert_eq!(r.f.last(PROJECT_DEFERRED)[..2], [5, E_BUSY as u32]);
    assert_eq!(r.f.toasts, [Ignore::Store as u32], "one warning");
    // it does not ask again
    let left = r.f.refuse_list;
    settle_over(&mut r, 5);
    assert_eq!(r.f.refuse_list, left);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
}

#[test]
fn a_load_the_store_refuses_for_another_reason_ends_at_once_unbound_with_one_event_and_one_warning()
{
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(5, b"NEW", 2);
    r.load(4);
    r.f.refuse_list = 2;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0, "an answer is no deferral");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    assert_eq!(
        r.f.last(PROJECT_IGNORED)[..3],
        [5, Ignore::Store as u32, dkrt_sdk::proto::E_NOSTORE as u32]
    );
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
    assert_eq!(r.p.status() & (ST_DEFERRED | ST_IGNORED), 0);
    settle_over(&mut r, 3);
    assert_eq!(r.f.refuse_list, 1, "no retry");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
}

#[test]
fn a_clear_the_store_refuses_for_another_reason_is_logged_and_dropped() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = 2;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 3);
    assert_eq!(r.f.refuse_list, 1, "no retry");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    assert!(r.f.toasts.is_empty(), "only a load warns");
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
}

#[test]
fn an_archive_probe_that_meets_a_busy_store_waits_and_never_takes_the_path() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.busy_reads.insert(archive(b"PROJ", 0x55), 1);
    r.hook_clear(5);
    for _ in 0..5 {
        r.pass();
    }
    assert!(
        r.f.ops.is_empty() && archived(&r).is_empty(),
        "nothing yet: the probe waits"
    );
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(
        archived(&r),
        [archive(b"PROJ", 0x55)],
        "the plain path, not a suffix"
    );
    assert_eq!(r.f.store[&archive(b"PROJ", 0x55)], file);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}

#[test]
fn a_file_that_is_archived_already_is_only_removed_from_its_slot() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    // a cut after the archive copy was written and before the source was removed
    r.f.store.insert(archive(b"PROJ", 0x55), file.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(archived(&r), [archive(b"PROJ", 0x55)], "no second copy");
    assert_eq!(r.f.store[&archive(b"PROJ", 0x55)], file);
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
    assert_eq!(
        r.f.ops,
        [('d', path(5, b"PROJ"))],
        "one removal and no write"
    );
}

/// A rig whose project 5 has a file, and whose first load of it is refused by the store with a status that is no busy answer.
fn failed_load_of_5() -> (Rig, Vec<u8>) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = 1;
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    r.load(5);
    assert_eq!(r.p.slot(), -1);
    (r, file)
}

#[test]
fn a_save_into_the_slot_whose_load_failed_keeps_its_file_and_loads_it_again() {
    let (mut r, file) = failed_load_of_5();
    // the scope is off while the load is failed
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    assert_eq!(r.p.scope_remove(OWNER, b"k"), E_SET_OFF);
    let mut out = [0u8; 8];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    // the firmware stays on slot 5 and saves into it: nothing is archived, nothing is written
    r.hook_save(5);
    r.settle();
    assert_eq!(
        r.f.store[&path(5, b"PROJ")],
        file,
        "the file stays in its slot"
    );
    assert!(archived(&r).is_empty());
    assert!(r.f.ops.is_empty(), "no write and no removal");
    assert_eq!(r.f.count(PROJECT_SAVED), 0);
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [5, Ignore::SaveSkipped as u32, 0, 0, 0],
        "the skipped save is logged, once"
    );
    assert_eq!(
        r.f.count(PROJECT_IGNORED),
        2,
        "the refused load, the skipped save"
    );
    // the save asked for the load again, and it bound the slot
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.p.status(), ST_FILE);
}

#[test]
fn a_setter_call_loads_the_slot_whose_load_failed_again() {
    let (mut r, _) = failed_load_of_5();
    r.hook_slot(5);
    r.settle();
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
}

#[test]
fn a_setter_call_to_another_slot_leaves_the_failed_load_until_that_load_ends() {
    let (mut r, file) = failed_load_of_5();
    r.rec(6, b"OTHER", 0x66);
    // the load of slot 6 waits for a live operation: slot 5 stays failed, and its save still loads instead of archiving
    r.f.live_op = 1;
    r.hook_slot(6);
    r.pass();
    r.hook_save(5);
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty());
    assert_eq!(
        r.p.slot(),
        6,
        "the setter's load ran, the save wrote nothing"
    );
}

#[test]
fn a_load_that_stays_busy_fails_and_a_save_into_the_slot_loads_it_again() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.refuse_list = u32::MAX;
    r.hook_slot(5);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    r.f.refuse_list = 0;
    r.hook_save(5);
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty() && r.f.ops.is_empty());
    assert_eq!(r.p.slot(), 5);
}

#[test]
fn a_busy_read_while_probing_a_file_is_no_missing_file_and_binds_no_empty_project() {
    let mut r = Rig::new();
    r.rec(12, b"MY SONG", 0x1234_5678);
    r.f.store.insert(
        path(12, b"MY SONG"),
        file_bytes(
            12,
            b"MY SONG",
            0x1234_5678,
            3,
            &[(b"test/owner/pool", b"abc")],
        ),
    );
    r.f.busy_reads.insert(path(12, b"MY SONG"), 1);
    r.load(12);
    assert_eq!(r.p.slot(), -1, "not bound while the store is busy");
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0);
    r.f.now += RETRY_MS + 1;
    r.settle();
    assert_eq!(r.p.slot(), 12);
    assert_eq!(r.get(b"pool"), b"abc");
}

#[test]
fn one_deferral_is_logged_once_by_every_kind_of_work_until_a_write_succeeds() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.rec(6, b"NEXT", 0x66);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = u32::MAX;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
    assert_eq!(r.p.status() & ST_DEFERRED, ST_DEFERRED);
    // a load that gives up in the same deferral logs nothing more, but it warns
    r.hook_slot(6);
    r.settle();
    settle_over(&mut r, 33);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1, "once");
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
}

#[test]
fn an_answer_that_is_no_busy_ends_the_busy_streak() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    r.f.store.insert(
        path(5, b"PROJ"),
        file_bytes(5, b"PROJ", 0x55, 1, &[(b"test/owner/k", b"x")]),
    );
    r.f.refuse_list = u32::MAX;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 25);
    // the store answers, with a refusal that is no busy answer
    r.f.refuse_status = dkrt_sdk::proto::E_NOSTORE;
    settle_over(&mut r, 1);
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    // busy again: the 30 seconds start over from here, not from the first busy answer
    r.f.refuse_status = E_BUSY;
    r.hook_clear(5);
    r.settle();
    settle_over(&mut r, 20);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 0, "20 seconds are not 30");
    settle_over(&mut r, 12);
    assert_eq!(r.f.count(PROJECT_DEFERRED), 1);
}

#[test]
fn an_archive_candidate_with_the_header_of_the_source_and_a_corrupt_body_is_taken_not_archived_already(
) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 7, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    // the first archive name holds a file with the source's header and a damaged body (a cut or a bad block)
    let mut damaged = file.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    let name = n16(b"PROJ");
    let first = name::archive_path_copy(&name, 0x55, 1).as_bytes().to_vec();
    r.f.store.insert(first.clone(), damaged.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(
        r.f.store[&first], damaged,
        "the damaged file is never replaced"
    );
    let second = name::archive_path_copy(&name, 0x55, 2).as_bytes().to_vec();
    assert_eq!(r.f.store[&second], file, "the source went to the next name");
    assert!(
        !r.f.store.contains_key(&path(5, b"PROJ")),
        "and left its slot"
    );
}

#[test]
fn a_file_that_passes_the_check_is_the_archive_copy_already_and_only_the_source_goes() {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 7, &[(b"test/owner/k", b"x")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    let name = n16(b"PROJ");
    let first = name::archive_path_copy(&name, 0x55, 1).as_bytes().to_vec();
    r.f.store.insert(first.clone(), file.clone());
    r.hook_clear(5);
    r.settle();
    assert_eq!(archived(&r), [first], "no second copy");
    assert!(!r.f.store.contains_key(&path(5, b"PROJ")));
}

#[test]
fn a_save_into_another_slot_adopts_the_image_with_no_file_and_dirty() {
    let mut r = Rig::new();
    r.rec(4, b"OLD", 1);
    r.rec(9, b"COPY", 2);
    r.f.store.insert(
        path(4, b"OLD"),
        file_bytes(4, b"OLD", 1, 1, &[(b"test/owner/k", b"data")]),
    );
    r.load(4);
    r.hook_save(9);
    r.settle();
    assert_eq!(r.p.slot(), 9);
    let head = dkp::check(&r.f.store[&path(9, b"COPY")], DEV).unwrap();
    assert_eq!(
        (head.slot, head.counter),
        (9, 1),
        "written although it was never changed"
    );
    assert_eq!(r.p.status(), ST_FILE);
}

/// A rig whose project 5 has a file; the listing finds it and then the read of the load answers as `script` says (the listing's own read comes first).
fn load_of_5_with_reads(script: &[Option<i32>]) -> (Rig, Vec<u8>) {
    let mut r = Rig::new();
    r.rec(5, b"PROJ", 0x55);
    let file = file_bytes(5, b"PROJ", 0x55, 4, &[(b"test/owner/k", b"kept")]);
    r.f.store.insert(path(5, b"PROJ"), file.clone());
    r.f.read_script
        .insert(path(5, b"PROJ"), script.iter().copied().collect());
    r.load(5);
    (r, file)
}

#[test]
fn a_refused_second_read_of_a_load_fails_it_and_a_later_save_keeps_the_file() {
    let status = dkrt_sdk::proto::E_NOSTORE;
    let (mut r, file) = load_of_5_with_reads(&[None, Some(status)]);
    assert_eq!(r.p.slot(), -1, "no empty project is bound");
    assert_eq!(
        r.f.last(PROJECT_IGNORED),
        [5, Ignore::Store as u32, status as u32, 0, 0]
    );
    assert_eq!(r.f.toasts, [Ignore::Store as u32]);
    assert_eq!(r.f.count(PROJECT_LOADED), 0);
    r.hook_save(5);
    r.settle();
    assert_eq!(r.f.store[&path(5, b"PROJ")], file);
    assert!(archived(&r).is_empty() && r.f.ops.is_empty());
}

#[test]
fn a_busy_second_read_of_a_load_is_tried_again_like_a_busy_listing() {
    let (mut r, _) = load_of_5_with_reads(&[None, Some(E_BUSY)]);
    assert_eq!(r.p.slot(), -1, "waits for the retry time");
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
}

#[test]
fn a_file_that_is_gone_at_the_second_read_binds_the_slot_empty() {
    let (r, _) = load_of_5_with_reads(&[None, Some(E_NOT_FOUND)]);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(&r.f.last(PROJECT_LOADED)[..4], &[5, 0, 0, 0]);
    assert_eq!(r.f.count(PROJECT_IGNORED), 0);
    assert!(r.f.toasts.is_empty());
}

#[test]
fn a_failed_load_is_tried_again_every_30_seconds_without_new_events() {
    let (mut r, _) = failed_load_of_5();
    // the retry is refused again: nothing new is logged or shown
    r.f.refuse_list = 1;
    settle_over(&mut r, 29);
    assert_eq!(r.p.slot(), -1, "29 seconds are not 30");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), -1, "the retry ran and was refused");
    assert_eq!(r.f.refuse_list, 0, "it asked the store");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1, "no second IGNORED");
    assert_eq!(r.f.toasts.len(), 1, "no second toast");
    // a live operation holds the retry back
    r.f.live_op = 1;
    settle_over(&mut r, 31);
    assert_eq!(r.p.slot(), -1);
    r.f.live_op = 0;
    settle_over(&mut r, 1);
    assert_eq!(r.p.slot(), 5);
    assert_eq!(r.get(b"k"), b"kept");
    assert_eq!(r.f.count(PROJECT_IGNORED), 1);
}

// ---- the module list: recorded at a save, restored after a load -------------------------------------------------------------------

/// A project file with a module list.
fn file_with_modules(
    slot: u8,
    name: &[u8],
    stamp: u32,
    counter: u32,
    modules: &[(&[u8], u32)],
) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    dkp::set_modules(&mut i.0, modules).unwrap();
    dkp::seal(&mut i.0, counter);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

fn id_hash(id: &[u8]) -> u32 {
    ModuleId::new(id).unwrap().hash()
}

/// The module events of the log: (what, status, index, id hash), in order.
fn module_events(r: &Rig) -> Vec<(u32, i32, u32, u32)> {
    r.f.logs
        .iter()
        .filter(|l| l.0 == PROJECT_MODULE)
        .map(|l| (l.1[1], l.1[2] as i32, l.1[3], l.1[4]))
        .collect()
}

/// The module list of the file the service wrote for `slot`.
fn written_modules(r: &Rig, slot: u32, name: &[u8]) -> Vec<(Vec<u8>, u32)> {
    let bytes = &r.f.store[&path(slot, name)];
    dkp::check(bytes, DEV).unwrap();
    dkp::modules(bytes)
        .map(|(id, hash)| (id.to_vec(), hash))
        .collect()
}

/// A rig with project 7 current and the modules `a/x` and `b/y` loaded.
fn rig_with_two_loaded() -> Rig {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.load(7);
    for (id, kind) in [("a/x", "machine"), ("b/y", "fx")] {
        let p = format!("modules/{kind}/{}.dkm", id.replace('/', "-")).into_bytes();
        r.f.hashes.insert(p.clone(), id_hash(id.as_bytes()) ^ 1);
        r.f.loaded.push((id.as_bytes().to_vec(), p));
    }
    r
}

#[test]
fn a_save_writes_the_loaded_modules_into_the_list_and_a_project_with_only_modules_gets_a_file() {
    let mut r = rig_with_two_loaded();
    // no put: the module list alone is digikit data
    r.hook_save(7);
    r.settle();
    assert_eq!(r.f.count(PROJECT_SAVED), 1);
    assert_eq!(
        written_modules(&r, 7, b"SONG"),
        [
            (b"a/x".to_vec(), id_hash(b"a/x") ^ 1),
            (b"b/y".to_vec(), id_hash(b"b/y") ^ 1)
        ],
        "the ids in record order, each with the store hash of its file"
    );
    // the same modules again: nothing changed, nothing is written
    let writes = r.f.ops.len();
    r.hook_save(7);
    r.settle();
    assert_eq!(r.f.ops.len(), writes);
    // a module that came: the list follows
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    r.hook_save(7);
    r.settle();
    assert_eq!(written_modules(&r, 7, b"SONG").len(), 3);
}

#[test]
fn a_save_with_no_loaded_module_and_no_data_writes_no_file() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.load(7);
    r.hook_save(7);
    r.settle();
    assert!(r.f.store.is_empty() && r.f.ops.is_empty());
}

#[test]
fn a_load_queues_the_live_loads_of_the_listed_modules_in_order_after_the_install_and_adds_only() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    // a module the list does not name stays loaded
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    r.load(7);
    assert_eq!(
        r.f.live_loads,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/fx/b-y.dkm".to_vec()
        ]
    );
    assert_eq!(
        module_events(&r),
        [
            (1, 0, 0, id_hash(b"a/x")),
            (2, 0, 0, id_hash(b"a/x")),
            (1, 0, 1, id_hash(b"b/y")),
            (2, 0, 1, id_hash(b"b/y"))
        ]
    );
    assert_eq!(
        r.f.loaded_ids(),
        [b"c/z".to_vec(), b"a/x".to_vec(), b"b/y".to_vec()],
        "the extra module is still loaded: the restore never unloads"
    );
    assert_eq!(
        r.f.selection,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/fx/b-y.dkm".to_vec()
        ],
        "a load the service asked for is saved in the selection like one from the menu"
    );
    assert!(r.f.warnings.is_empty());
    // the loads came after the install (LOADED comes first in the log)
    let loaded_at = r.f.logs.iter().position(|l| l.0 == PROJECT_LOADED).unwrap();
    let first_module = r.f.logs.iter().position(|l| l.0 == PROJECT_MODULE).unwrap();
    assert!(loaded_at < first_module);
}

#[test]
fn a_listed_module_that_is_loaded_already_is_not_asked_for_and_a_stock_project_asks_for_nothing() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.rec(8, b"PLAIN", 0x88);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.loaded
        .push((b"a/x".to_vec(), b"modules/machine/a-x.dkm".to_vec()));
    r.load(7);
    assert!(r.f.live_loads.is_empty() && module_events(&r).is_empty());
    // a project with no .dkp: nothing is queued, whatever is loaded
    r.f.loaded.clear();
    r.load(8);
    assert!(r.f.live_loads.is_empty() && module_events(&r).is_empty());
}

#[test]
fn a_load_refused_while_the_transport_plays_waits_for_the_stop_and_counts_for_no_try() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.playing = true;
    r.hook_slot(7);
    // many refusals, far more than a retry count would allow: one every PLAY_WAIT_MS
    for _ in 0..12 {
        for _ in 0..6 {
            r.pass();
        }
        r.f.now += PLAY_WAIT_MS + 1;
    }
    assert!(r.f.loaded.is_empty() && r.f.warnings.is_empty());
    assert!(
        module_events(&r).iter().all(|e| e.0 == 1),
        "queued again and again, never refused"
    );
    // the last wait ran out: one more refusal, then the wait begins again
    for _ in 0..6 {
        r.pass();
    }
    let asked = r.f.live_loads.len();
    assert!(asked >= 10, "{asked}");
    // within the wait nothing is asked
    for _ in 0..10 {
        r.pass();
    }
    assert_eq!(r.f.live_loads.len(), asked);
    // the transport stops
    r.f.playing = false;
    r.f.now += PLAY_WAIT_MS + 1;
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(module_events(&r).last().unwrap().0, 2);
}

#[test]
fn a_module_that_is_not_on_the_card_is_one_log_line_and_one_warning_and_the_others_still_load() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/gone", 1), (b"b/y", 2)]),
    );
    r.f.card("fx", "b/y");
    r.load(7);
    assert_eq!(r.p.slot(), 7, "the project loads");
    assert_eq!(r.f.warnings, [(ModuleWarning::Missing, b"a/gone".to_vec())]);
    assert_eq!(
        module_events(&r)[0],
        (3, 0, 0, id_hash(b"a/gone")),
        "MISSING for the first, then the second goes on"
    );
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
    assert_eq!(r.f.live_loads, [b"modules/fx/b-y.dkm".to_vec()]);
}

#[test]
fn a_load_the_runtime_refuses_is_a_warning_and_a_log_line_and_the_next_one_continues() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    // the job of the first ends with a refusal; the second works
    r.f.job_results.push_back(E_PLAN_FIT);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::NoFit, b"a/x".to_vec())]);
    let events = module_events(&r);
    assert_eq!(events[0], (1, 0, 0, id_hash(b"a/x")));
    assert_eq!(events[1], (4, E_PLAN_FIT, 0, id_hash(b"a/x")));
    assert_eq!(events[3], (2, 0, 1, id_hash(b"b/y")));
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
    assert_eq!(
        r.f.selection,
        [b"modules/fx/b-y.dkm".to_vec()],
        "a refused module is not saved in the selection"
    );
}

#[test]
fn a_refusal_that_is_not_a_fit_refusal_is_the_other_warning() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 1, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.job_results.push_back(E_FORMAT);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
}

#[test]
fn a_project_change_while_the_restore_runs_replaces_the_pending_list() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.rec(8, b"OTHER", 0x88);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.store.insert(
        path(8, b"OTHER"),
        file_with_modules(8, b"OTHER", 0x88, 2, &[(b"c/z", 3)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.card("tool", "c/z");
    // the first load is queued and stays running
    r.f.hold_jobs = true;
    r.hook_slot(7);
    for _ in 0..40 {
        r.pass();
    }
    assert_eq!(r.f.live_loads, [b"modules/machine/a-x.dkm".to_vec()]);
    // another project becomes current
    r.hook_slot(8);
    for _ in 0..40 {
        r.pass();
    }
    // the job of the old list ends in the runtime; the service goes on with the new list only
    r.f.hold_jobs = false;
    r.settle();
    assert_eq!(
        r.f.live_loads,
        [
            b"modules/machine/a-x.dkm".to_vec(),
            b"modules/tool/c-z.dkm".to_vec()
        ],
        "b/y of the first project is never asked for"
    );
    assert!(r.f.loaded_ids().contains(&b"c/z".to_vec()));
    assert!(!r.f.loaded_ids().contains(&b"b/y".to_vec()));
}

#[test]
fn a_store_that_is_busy_when_the_file_is_searched_asks_the_same_entry_again_and_is_never_missing() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.find_script.extend([E_BUSY, E_BUSY, E_BUSY]);
    r.load(7);
    assert!(r.f.warnings.is_empty(), "{:?}", r.f.warnings);
    assert_eq!(r.f.finds, 4);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
}

#[test]
fn a_store_that_cannot_be_read_is_a_refusal_and_not_a_missing_module() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.find_script.push_back(E_READ);
    r.load(7);
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
    assert_eq!(module_events(&r)[0], (4, E_READ, 0, id_hash(b"a/x")));
    assert_eq!(
        r.f.loaded_ids(),
        [b"b/y".to_vec()],
        "the next entry goes on"
    );
}

#[test]
fn the_store_is_not_read_while_a_live_job_stages() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.hook_slot(7);
    // the project installs, then a live job stages for many passes: no search (the fake asserts it)
    while r.f.count(PROJECT_LOADED) == 0 {
        r.pass();
    }
    r.f.live_op = 5;
    for _ in 0..30 {
        r.pass();
    }
    assert_eq!(r.f.finds, 0);
    assert!(r.f.loaded.is_empty());
    r.f.live_op = 0;
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
}

#[test]
fn a_busy_card_is_asked_again_a_few_times_and_then_a_refusal() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    // twice busy, then it works
    r.f.live_answers.extend([E_CARD_BUSY, E_CARD_BUSY]);
    r.hook_slot(7);
    for _ in 0..20 {
        r.pass();
        r.f.now += CARD_WAIT_MS + 1;
    }
    assert!(r.f.warnings.is_empty());
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(r.f.live_asked, 3);
    // busy for ever: CARD_TRIES_MAX more asks, then one warning
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1), (b"b/y", 2)]),
    );
    r.f.card("machine", "a/x");
    r.f.card("fx", "b/y");
    r.f.live_answers.extend([E_CARD_BUSY; 4]);
    r.hook_slot(7);
    for _ in 0..40 {
        r.pass();
        r.f.now += CARD_WAIT_MS + 1;
    }
    assert_eq!(r.f.warnings, [(ModuleWarning::Refused, b"a/x".to_vec())]);
    assert_eq!(
        r.f.live_asked,
        4 + 1,
        "the first four asks of a/x, then b/y"
    );
    assert_eq!(r.f.loaded_ids(), [b"b/y".to_vec()]);
}

#[test]
fn a_retry_pass_reads_no_hash_and_searches_the_store_once_per_entry() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.loaded
        .push((b"c/z".to_vec(), b"modules/tool/c-z.dkm".to_vec()));
    // the runtime is busy for ten asks
    r.f.live_answers.extend([E_BUSY; 10]);
    r.hook_slot(7);
    r.f.hash_reads.set(0);
    for _ in 0..30 {
        r.pass();
    }
    r.settle();
    assert_eq!(r.f.loaded_ids(), [b"c/z".to_vec(), b"a/x".to_vec()]);
    assert_eq!(r.f.finds, 1, "the file of an entry is searched once");
    assert_eq!(
        r.f.hash_reads.get(),
        0,
        "the loaded ids are compared without a store read"
    );
}

#[test]
fn a_module_whose_path_the_selection_cannot_hold_loads_and_is_told_as_not_kept() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    // 63 bytes: the live load takes it, the selection does not
    let kind = "k".repeat(63 - "modules//a-x.dkm".len());
    r.f.card(&kind, "a/x");
    r.load(7);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()], "loaded live");
    assert!(r.f.selection.is_empty());
    assert_eq!(r.f.warnings, [(ModuleWarning::NotKept, b"a/x".to_vec())]);
    let events = module_events(&r);
    assert_eq!(events[1], (5, E_SEL_PATH, 0, id_hash(b"a/x")));
    assert_eq!(events.len(), 3, "queued, not kept, done");
}

#[test]
fn a_full_selection_is_told_as_not_kept_after_the_load() {
    let mut r = Rig::new();
    r.rec(7, b"SONG", 0x77);
    r.f.store.insert(
        path(7, b"SONG"),
        file_with_modules(7, b"SONG", 0x77, 2, &[(b"a/x", 1)]),
    );
    r.f.card("machine", "a/x");
    r.f.selection_answer = Some(E_SEL_COUNT);
    r.load(7);
    assert_eq!(r.f.loaded_ids(), [b"a/x".to_vec()]);
    assert_eq!(r.f.warnings, [(ModuleWarning::NotKept, b"a/x".to_vec())]);
    assert_eq!(module_events(&r).last().unwrap().1, E_SEL_COUNT);
}
