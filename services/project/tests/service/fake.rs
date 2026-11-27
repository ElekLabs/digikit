//! The fake store, firmware list and log.
use crate::*;
use dkrt_sdk::{E_BUSY, E_NOT_FOUND, E_RELOAD_PLAYING, LIVE_LOAD, OK};
use dkrt_svc_project::dkp::AREA;
use dkrt_svc_project::engine::*;
use std::collections::BTreeMap;

/// The store, the firmware list and the log of the fake.
pub(crate) struct Fake {
    pub(crate) store: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) recs: BTreeMap<u32, SlotRecord>,
    pub(crate) live_op: u32,
    pub(crate) now: u32,
    // listing: rows to give, pending polls before them
    pub(crate) rows: Vec<Vec<u8>>,
    pub(crate) at: usize,
    pub(crate) pending_polls: u32,
    pub(crate) listing: bool,
    pub(crate) logs: Vec<(u16, [u32; 5])>,
    pub(crate) toasts: Vec<u32>,
    /// The settings record (device scope), by stored key, and the owners `changed` named, in order.
    pub(crate) record: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(crate) changed: Vec<Vec<u8>>,
    pub(crate) posted: bool,
    // the store calls, in order: ("w", path) a file committed, ("d", path) a file removed
    pub(crate) ops: Vec<(char, Vec<u8>)>,
    pub(crate) session: Option<(Vec<u8>, Vec<u8>)>,
    /// `create` answers E_BUSY while this is set.
    pub(crate) busy: bool,
    /// `create` answers this status (a store that is full, foreign or off) and counts the tries.
    pub(crate) fail_create: Option<i32>,
    pub(crate) creates: u32,
    /// Writes and closes that were done (the write counter of the store).
    pub(crate) commits: u32,
    /// The step stops after the write and never calls `close`: a power cut.
    pub(crate) cut_before_close: bool,
    pub(crate) reads: u32,
    pub(crate) irq: std::cell::Cell<i32>,
    /// The slot the firmware has current at the start (`Env::firmware_slot`).
    pub(crate) firmware_slot: Option<u32>,
    /// `list_start` refuses with `refuse_status` this many times.
    pub(crate) refuse_list: u32,
    pub(crate) refuse_status: i32,
    /// A read of such a path answers `E_BUSY` this many times, then the file's own answer.
    pub(crate) busy_reads: BTreeMap<Vec<u8>, u32>,
    /// The next listing fails after its first poll (`ListPoll::Err`).
    pub(crate) fail_poll_once: bool,
    /// The answers of the next reads of a path, in order: `Some(status)` is a refusal, `None` the file's own answer; with the list used up reads answer as the file does.
    pub(crate) read_script: BTreeMap<Vec<u8>, std::collections::VecDeque<Option<i32>>>,
    /// The content modules loaded now, by module record: (id, store path). Their store hashes are in `hashes`.
    pub(crate) loaded: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) hashes: BTreeMap<Vec<u8>, u32>,
    /// The paths of the live loads asked for, in order, and the answers of the next asks (`OK` when the list is used up).
    pub(crate) live_loads: Vec<Vec<u8>>,
    pub(crate) live_answers: std::collections::VecDeque<i32>,
    /// The runtime's live job: a queued job ends in the next pass of the rig with the next of `job_results` (`OK` when used up); `hold_jobs` keeps it running; `playing`
    /// makes every job end with the transport's refusal.
    pub(crate) job: LiveJob,
    pub(crate) job_results: std::collections::VecDeque<i32>,
    pub(crate) hold_jobs: bool,
    pub(crate) playing: bool,
    /// The paths added to the saved selection and the warnings shown.
    pub(crate) selection: Vec<Vec<u8>>,
    pub(crate) warnings: Vec<(ModuleWarning, Vec<u8>)>,
    /// The answers of the next `find_module_file` calls before the card's own, and the calls made (the searches read the whole store).
    pub(crate) find_script: std::collections::VecDeque<i32>,
    pub(crate) finds: u32,
    /// The store hashes read (a restore reads none) and the live loads asked, refused ones included.
    pub(crate) hash_reads: std::cell::Cell<u32>,
    pub(crate) live_asked: u32,
    /// `selection_add` answers this when set.
    pub(crate) selection_answer: Option<i32>,
}

impl Fake {
    pub(crate) fn new() -> Fake {
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
    pub(crate) fn card(&mut self, kind: &str, id: &str) {
        let p = format!("modules/{kind}/{}.dkm", id.replace('/', "-"));
        self.store.insert(p.into_bytes(), vec![0; 16]);
    }
    /// The runtime's job step (once per UI pass): a queued job ends, and a load that ends well leaves the module loaded.
    pub(crate) fn runtime_step(&mut self) {
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
    pub(crate) fn loaded_ids(&self) -> Vec<Vec<u8>> {
        self.loaded.iter().map(|(id, _)| id.clone()).collect()
    }
    pub(crate) fn count(&self, kind: u16) -> usize {
        self.logs.iter().filter(|l| l.0 == kind).count()
    }
    pub(crate) fn last(&self, kind: u16) -> [u32; 5] {
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
