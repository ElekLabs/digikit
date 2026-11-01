//! Per-track stereo sample table (API 1.5, docs/design/sample-api-1.5.md). Device-neutral and
//! host-testable: all effects (file inspection, the backing, the DSP records) go through `Env`.
//! The ColdFire runtime implements `Env` with the optional `dkpool_*` symbols and the DSP link.
//!
//! One `Core` serves `CL` clients (modules with a track table). It owns the 16-track tables, the
//! dedup entries (one per backing handle, refcount = tracks bound), the load state machine and the
//! in-memory per-project selection table. `tick` runs in the UI task and does one bounded step per
//! track; `select`, `unbind`, `done`, `lost` and `project_changed` only change state.
use crate::{
    E_CHANGED, E_CONTRACT, E_DSP_XFER, E_FORMAT, E_LOST, E_NOMEM, E_NOT_FOUND, E_POOL, E_SEL_PATH,
    E_TOO_BIG,
};

pub const TRACKS: usize = 16;
pub const PATH: usize = 256;
/// Selection paths kept per project (store paths are at most 159 bytes; longer ones are not remembered).
pub const SEL_PATH: usize = 160;
pub const UNUSED: i32 = 0;
pub const QUEUED: i32 = 1;
pub const LOADING: i32 = 2;
pub const READY: i32 = 3;
pub const FMT_PCM16: u32 = 1;
/// Ticks the DSP may take to drain a closed track before the track fails closed.
pub const DRAIN_GUARD: u32 = 6000;
const NO_SLOT: u32 = u32::MAX;

/// File identity snapshot (`dk_file_id` of dkrt_pool.h).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileId {
    pub root: u32,
    pub size: u32,
    pub mtime: u32,
    pub flags: u32,
    pub hash_kind: u32,
    pub hash: [u8; 32],
}
pub const ID_HASH_VALID: u32 = 1;
impl FileId {
    pub const fn empty() -> Self {
        Self { root: 0, size: 0, mtime: 0, flags: 0, hash_kind: 0, hash: [0; 32] }
    }
    /// Same file content by identity: the listing hash when both have one, else size and mtime.
    pub fn same(&self, o: &FileId) -> bool {
        if self.root != o.root || self.size != o.size {
            return false;
        }
        if self.flags & o.flags & ID_HASH_VALID != 0 {
            return self.hash_kind == o.hash_kind && self.hash == o.hash;
        }
        self.flags & ID_HASH_VALID == o.flags & ID_HASH_VALID && self.mtime == o.mtime
    }
}

/// Result of header check and size arithmetic at select.
#[derive(Clone, Copy, Debug)]
pub struct Inspect {
    pub id: FileId,
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
}

/// `dkpool_handle` of dkrt_pool.h.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Handle {
    pub id: u32,
    pub token: u32,
    pub base_l: u32,
    pub base_r: u32,
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
}

/// `dk_strack` of dk/sample_track.h, in this order.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Strack {
    pub ready: u32,
    pub token: u32,
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
    pub fmt: u32,
    pub base_l: u32,
    pub base_r: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Done,
    Queued,
    Err(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub frames: u32,
    pub rate: u32,
    pub channels: u32,
    pub token: u32,
}

/// The effects of the state machine. Everything may block only briefly; no call spins.
pub trait Env {
    /// Open the file, check the header (format, 1 or 2 channels, PCM16, rate), return frames and identity.
    fn inspect(&mut self, path: &[u8]) -> Result<Inspect, i32>;
    /// Identity only (stat), for the check after a load.
    fn stat(&mut self, path: &[u8]) -> Result<FileId, i32>;
    /// `dkpool_load`. `out` stays valid until the load is terminal.
    fn load(&mut self, req: u32, id: &FileId, path: &[u8], out: &mut Handle) -> Load;
    fn release(&mut self, h: &Handle);
    /// `dkpool_check`: 0 valid.
    fn check(&mut self, h: &Handle) -> i32;
    fn free_frames(&mut self) -> u32;
    /// CLOSE the track generation: no new notes, ready = 0.
    fn close(&mut self, client: usize, track: usize);
    /// The DSP drain ack: no voice reads the track any more.
    fn drained(&mut self, client: usize, track: usize) -> bool;
    /// Write the record, clear CLOSED, `ready` last.
    fn publish(&mut self, client: usize, track: usize, rec: &Strack);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EState {
    Free,
    Loading,
    Ready,
    Failed(i32),
    /// The backing lost it: never released, freed when the last reference goes.
    Dead,
}
#[derive(Clone, Copy)]
struct Entry {
    state: EState,
    refs: u32,
    id: FileId,
    pathkey: u32,
    req: u32,
    handle: Handle,
}
const ENTRY0: Entry = Entry {
    state: EState::Free,
    refs: 0,
    id: FileId::empty(),
    pathkey: 0,
    req: 0,
    handle: Handle { id: 0, token: 0, base_l: 0, base_r: 0, frames: 0, rate: 0, channels: 0 },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Idle,
    /// Needs the preflight.
    Queued,
    /// Old generation closed, waiting for the drain ack.
    Closing,
    /// Drained: take the new reference (dedup or load), then release the old.
    Binding,
    /// Waiting for the entry in `loading`.
    Loading,
    /// `unbind`: close, drain, drop the reference.
    Unbinding,
}

#[derive(Clone, Copy)]
struct Track {
    status: i32,
    phase: Phase,
    path: [u8; PATH],
    plen: usize,
    /// The entry whose record is published (reference held).
    bound: Option<usize>,
    /// The entry being loaded or shared for the pending request (reference held).
    loading: Option<usize>,
    /// The identity this track was selected with (checked after load and on reload).
    snap: FileId,
    pend: Inspect,
    pathkey: u32,
    token: u32,
    published: Info,
    reload: bool,
    fresh: bool,
    closed: bool,
    ticks: u32,
}
const TRACK0: Track = Track {
    status: UNUSED,
    phase: Phase::Idle,
    path: [0; PATH],
    plen: 0,
    bound: None,
    loading: None,
    snap: FileId::empty(),
    pend: Inspect { id: FileId::empty(), frames: 0, rate: 0, channels: 0 },
    pathkey: 0,
    token: 0,
    published: Info { frames: 0, rate: 0, channels: 0, token: 0 },
    reload: false,
    fresh: false,
    closed: false,
    ticks: 0,
};

#[derive(Clone, Copy)]
struct Sel {
    len: u8,
    path: [u8; SEL_PATH],
}
const SEL0: Sel = Sel { len: 0, path: [0; SEL_PATH] };
#[derive(Clone, Copy)]
struct Proj<const CL: usize> {
    used: bool,
    slot: u32,
    age: u32,
    sel: [[Sel; TRACKS]; CL],
}

fn fnv(path: &[u8]) -> u32 {
    let mut h = 0x811c9dc5u32;
    for b in path {
        h = (h ^ *b as u32).wrapping_mul(0x01000193);
    }
    h
}

/// `CL` clients, `EN` dedup entries, `PR` remembered projects.
pub struct Core<const CL: usize, const EN: usize, const PR: usize> {
    open: [bool; CL],
    tracks: [[Track; TRACKS]; CL],
    entries: [Entry; EN],
    next_req: u32,
    next_token: u32,
    proj: [Proj<CL>; PR],
    cur: usize,
    clock: u32,
}

impl<const CL: usize, const EN: usize, const PR: usize> Core<CL, EN, PR> {
    pub const fn new() -> Self {
        let mut proj = [Proj { used: false, slot: NO_SLOT, age: 0, sel: [[SEL0; TRACKS]; CL] }; PR];
        proj[0].used = true;
        Self {
            open: [false; CL],
            tracks: [[TRACK0; TRACKS]; CL],
            entries: [ENTRY0; EN],
            next_req: 1,
            next_token: 1,
            proj,
            cur: 0,
            clock: 1,
        }
    }

    /// Claim a client slot; false when the client is already open or the index is out of range.
    pub fn open(&mut self, client: usize) -> bool {
        if client >= CL || self.open[client] {
            return false;
        }
        self.open[client] = true;
        true
    }
    pub fn is_open(&self, client: usize) -> bool {
        client < CL && self.open[client]
    }

    fn slot(&mut self, client: usize, track: usize) -> Result<&mut Track, i32> {
        if track >= TRACKS {
            return Err(E_CONTRACT);
        }
        if client >= CL || !self.open[client] {
            return Err(E_NOT_FOUND);
        }
        Ok(&mut self.tracks[client][track])
    }

    /// Queue a load. No file IO here: the path is copied and the preflight runs in `tick`.
    pub fn select(&mut self, client: usize, track: usize, path: &[u8]) -> i32 {
        let n = path.iter().position(|b| *b == 0).unwrap_or(path.len());
        if n == 0 || n >= PATH {
            return E_SEL_PATH;
        }
        let r = self.select_at(client, track, &path[..n], false);
        if r == 0 {
            self.remember(client, track, Some(&path[..n]));
        }
        r
    }

    fn select_at(&mut self, client: usize, track: usize, path: &[u8], reload: bool) -> i32 {
        let t = match self.slot(client, track) {
            Ok(t) => t,
            Err(e) => return e,
        };
        t.path[..path.len()].copy_from_slice(path);
        t.plen = path.len();
        t.pathkey = fnv(path);
        t.status = if reload { LOADING } else { QUEUED };
        t.reload = reload;
        t.fresh = false;
        t.ticks = 0;
        // A pending load of an older request keeps its entry reference until `tick` drops it.
        t.phase = Phase::Queued;
        0
    }

    /// Close the generation and drop the references; the status is UNUSED at once.
    pub fn unbind(&mut self, client: usize, track: usize) -> i32 {
        let r = self.unbind_at(client, track);
        if r == 0 {
            self.remember(client, track, None);
        }
        r
    }
    fn unbind_at(&mut self, client: usize, track: usize) -> i32 {
        let t = match self.slot(client, track) {
            Ok(t) => t,
            Err(e) => return e,
        };
        t.status = UNUSED;
        t.reload = false;
        t.ticks = 0;
        t.phase = if t.bound.is_some() || t.loading.is_some() { Phase::Unbinding } else { Phase::Idle };
        0
    }

    pub fn status(&self, client: usize, track: usize) -> i32 {
        if track >= TRACKS {
            return E_CONTRACT;
        }
        if !self.is_open(client) {
            return E_NOT_FOUND;
        }
        self.tracks[client][track].status
    }

    /// Frames, rate, channels and token of a READY track.
    pub fn info(&self, client: usize, track: usize) -> Result<Info, i32> {
        let s = self.status(client, track);
        if s != READY {
            return Err(if s < 0 { s } else { E_NOT_FOUND });
        }
        Ok(self.tracks[client][track].published)
    }

    /// The backing finished load `req`. A completion with an unknown or stale `req` is ignored.
    pub fn done(&mut self, req: u32, result: i32) {
        if req == 0 {
            return;
        }
        for e in self.entries.iter_mut() {
            if e.state == EState::Loading && e.req == req {
                e.state = if result == 0 { EState::Ready } else { EState::Failed(result) };
                return;
            }
        }
    }

    /// The backing lost everything (DT2 stock reset): every bound track reloads from its path.
    pub fn lost(&mut self) {
        for i in 0..EN {
            if matches!(self.entries[i].state, EState::Loading | EState::Ready) {
                self.invalidate(i);
            }
        }
    }

    /// Mark an entry dead. Idle tracks bound to it reload from their path; tracks in the middle of a
    /// new request notice at their next step (their old reference is dropped without a release).
    fn invalidate(&mut self, e: usize) {
        self.entries[e].state = EState::Dead;
        for c in 0..CL {
            for k in 0..TRACKS {
                let t = &mut self.tracks[c][k];
                if t.phase != Phase::Idle || t.bound != Some(e) {
                    continue;
                }
                t.ticks = 0;
                t.closed = false;
                t.fresh = false;
                if t.status < 0 {
                    // The path is no longer the bound file's: give up quietly.
                    t.status = E_LOST;
                    t.phase = Phase::Unbinding;
                } else {
                    t.status = LOADING;
                    t.reload = true;
                    t.phase = Phase::Queued;
                }
            }
        }
    }

    fn remember(&mut self, client: usize, track: usize, path: Option<&[u8]>) {
        let s = &mut self.proj[self.cur].sel[client][track];
        match path {
            Some(p) if p.len() < SEL_PATH => {
                s.path[..p.len()].copy_from_slice(p);
                s.len = p.len() as u8;
            }
            _ => s.len = 0,
        }
    }

    /// The current project changed. Selections of the new project are queued first, then the
    /// tracks it does not use are unbound, so files both projects use keep their pool entry.
    /// A first call with the staging table (selections made before the project was known) adopts it.
    pub fn project_changed(&mut self, slot: u32) {
        if self.proj[self.cur].slot == slot {
            return;
        }
        if self.proj[self.cur].slot == NO_SLOT {
            let seen = self.proj.iter().position(|p| p.used && p.slot == slot);
            if seen.is_none() {
                self.proj[self.cur].slot = slot;
                return;
            }
        }
        self.clock += 1;
        let idx = match self.proj.iter().position(|p| p.used && p.slot == slot) {
            Some(i) => i,
            None => {
                let mut victim = None;
                for i in 0..PR {
                    if !self.proj[i].used {
                        victim = Some(i);
                        break;
                    }
                }
                let victim = victim.unwrap_or_else(|| {
                    let mut best = if self.cur == 0 { 1 } else { 0 };
                    for i in 0..PR {
                        if i != self.cur && self.proj[i].age < self.proj[best].age {
                            best = i;
                        }
                    }
                    best
                });
                self.proj[victim] = Proj { used: true, slot, age: 0, sel: [[SEL0; TRACKS]; CL] };
                victim
            }
        };
        self.proj[self.cur].age = self.clock;
        if idx == self.cur {
            return;
        }
        if self.proj[self.cur].slot == NO_SLOT {
            // The staging table stays as an ordinary project of unknown slot: drop it.
            self.proj[self.cur].used = false;
        }
        self.cur = idx;
        let proj = self.proj[idx];
        for c in 0..CL {
            if !self.open[c] {
                continue;
            }
            for k in 0..TRACKS {
                let s = &proj.sel[c][k];
                if s.len != 0 {
                    self.select_at(c, k, &s.path[..s.len as usize], false);
                }
            }
            for k in 0..TRACKS {
                if proj.sel[c][k].len == 0 && (self.tracks[c][k].status != UNUSED || self.tracks[c][k].bound.is_some()) {
                    self.unbind_at(c, k);
                }
            }
        }
    }

    /// The release itself happens when the entry is reaped (after a pending load is terminal).
    fn unref(&mut self, e: usize) {
        let en = &mut self.entries[e];
        en.refs = en.refs.saturating_sub(1);
    }

    fn reap(&mut self, env: &mut impl Env) {
        for e in self.entries.iter_mut() {
            if e.refs != 0 {
                continue;
            }
            match e.state {
                EState::Ready => {
                    env.release(&e.handle);
                    e.state = EState::Free;
                }
                EState::Failed(_) | EState::Dead => e.state = EState::Free,
                _ => {}
            }
        }
    }

    /// One bounded step per track; entries without a reference are released first.
    pub fn tick(&mut self, env: &mut impl Env) {
        self.reap(env);
        for e in 0..EN {
            if self.entries[e].state == EState::Ready && self.entries[e].refs != 0 {
                let h = self.entries[e].handle;
                if env.check(&h) != 0 {
                    self.invalidate(e);
                }
            }
        }
        for c in 0..CL {
            if !self.open[c] {
                continue;
            }
            for k in 0..TRACKS {
                self.step(c, k, env);
            }
        }
    }

    fn fail(&mut self, c: usize, k: usize, code: i32) {
        if let Some(e) = self.tracks[c][k].loading.take() {
            self.unref(e);
        }
        let t = &mut self.tracks[c][k];
        t.status = code;
        t.phase = Phase::Idle;
        t.reload = false;
    }

    fn step(&mut self, c: usize, k: usize, env: &mut impl Env) {
        match self.tracks[c][k].phase {
            Phase::Idle => {}
            Phase::Queued => self.step_queued(c, k, env),
            Phase::Closing => {
                if self.drain(c, k, env) {
                    self.tracks[c][k].phase = Phase::Binding;
                }
            }
            Phase::Binding => self.step_bind(c, k, env),
            Phase::Loading => self.step_loading(c, k, env),
            Phase::Unbinding => {
                // A superseded load's reference goes first; it never reached the DSP.
                if let Some(e) = self.tracks[c][k].loading.take() {
                    self.unref(e);
                }
                if self.tracks[c][k].bound.is_some() && !self.drain(c, k, env) {
                    return;
                }
                if let Some(e) = self.tracks[c][k].bound.take() {
                    self.unref(e);
                }
                let t = &mut self.tracks[c][k];
                t.phase = Phase::Idle;
                t.closed = false;
                t.published = Info { frames: 0, rate: 0, channels: 0, token: 0 };
            }
        }
    }

    /// CLOSE once, then wait for the ack. After the guard the track fails closed and keeps its
    /// reference: PCM that a voice may still read is never released.
    fn drain(&mut self, c: usize, k: usize, env: &mut impl Env) -> bool {
        let t = &mut self.tracks[c][k];
        if t.bound.is_none() {
            return true;
        }
        if !t.closed {
            env.close(c, k);
            t.closed = true;
            t.ticks = 0;
        }
        if env.drained(c, k) {
            return true;
        }
        t.ticks = t.ticks.saturating_add(1);
        if t.ticks > DRAIN_GUARD {
            t.status = E_DSP_XFER;
            t.phase = Phase::Idle;
            t.reload = false;
            if let Some(e) = t.loading.take() {
                self.unref(e);
            }
        }
        false
    }

    fn step_queued(&mut self, c: usize, k: usize, env: &mut impl Env) {
        // A superseded load drops its reference before the new request starts.
        if let Some(e) = self.tracks[c][k].loading.take() {
            self.unref(e);
        }
        let (plen, reload) = (self.tracks[c][k].plen, self.tracks[c][k].reload);
        let mut path = [0u8; PATH];
        path[..plen].copy_from_slice(&self.tracks[c][k].path[..plen]);
        let info = match env.inspect(&path[..plen]) {
            Ok(i) => i,
            Err(e) => {
                // The old binding stays untouched: it is still published and plays.
                let code = if reload { E_LOST } else { e };
                let t = &mut self.tracks[c][k];
                t.status = code;
                t.phase = Phase::Idle;
                t.reload = false;
                return;
            }
        };
        if info.frames == 0 || !(1..=2).contains(&info.channels) || info.rate == 0 {
            self.tracks[c][k].status = E_FORMAT;
            self.tracks[c][k].phase = Phase::Idle;
            self.tracks[c][k].reload = false;
            return;
        }
        if reload && !self.tracks[c][k].snap.same(&info.id) {
            self.fail(c, k, E_CHANGED);
            return;
        }
        let t = &mut self.tracks[c][k];
        t.pend = info;
        t.status = if reload { LOADING } else { QUEUED };
        t.closed = false;
        t.ticks = 0;
        // CLOSE the old generation now; other tracks are untouched.
        t.phase = if t.bound.is_some() { Phase::Closing } else { Phase::Binding };
        if t.phase == Phase::Closing {
            self.step(c, k, env);
        }
    }

    fn find(&self, id: &FileId, pathkey: u32) -> Option<usize> {
        self.entries.iter().position(|e| {
            matches!(e.state, EState::Loading | EState::Ready)
                && e.id.same(id)
                && (id.flags & ID_HASH_VALID != 0 || e.pathkey == pathkey)
        })
    }

    fn step_bind(&mut self, c: usize, k: usize, env: &mut impl Env) {
        let (info, pathkey) = (self.tracks[c][k].pend, self.tracks[c][k].pathkey);
        // Same file already in the pool: share it, take the reference before the old one goes.
        if let Some(e) = self.find(&info.id, pathkey) {
            self.entries[e].refs += 1;
            self.tracks[c][k].loading = Some(e);
            self.tracks[c][k].fresh = false;
            if let Some(o) = self.tracks[c][k].bound.take() {
                self.unref(o);
            }
            self.tracks[c][k].status = LOADING;
            self.tracks[c][k].phase = Phase::Loading;
            return;
        }
        if let Some(o) = self.tracks[c][k].bound.take() {
            self.unref(o);
            self.reap(env);
        }
        let need = info.frames.saturating_mul(info.channels);
        if need > env.free_frames() {
            self.fail(c, k, E_TOO_BIG);
            return;
        }
        let Some(slot) = self.entries.iter().position(|e| e.state == EState::Free) else {
            self.fail(c, k, E_POOL);
            return;
        };
        let Some(next) = self.next_req.checked_add(1) else {
            self.fail(c, k, E_NOMEM);
            return;
        };
        let req = self.next_req;
        self.next_req = next;
        let plen = self.tracks[c][k].plen;
        let mut path = [0u8; PATH];
        path[..plen].copy_from_slice(&self.tracks[c][k].path[..plen]);
        self.entries[slot] = Entry { state: EState::Loading, refs: 1, id: info.id, pathkey, req, handle: Handle::default() };
        let r = env.load(req, &info.id, &path[..plen], &mut self.entries[slot].handle);
        let reload = self.tracks[c][k].reload;
        match r {
            Load::Done => self.entries[slot].state = EState::Ready,
            Load::Queued => {}
            Load::Err(e) => {
                self.entries[slot] = ENTRY0;
                self.fail(c, k, if reload { E_LOST } else { e });
                return;
            }
        }
        let t = &mut self.tracks[c][k];
        t.loading = Some(slot);
        t.fresh = true;
        t.status = LOADING;
        t.phase = Phase::Loading;
    }

    fn step_loading(&mut self, c: usize, k: usize, env: &mut impl Env) {
        let Some(e) = self.tracks[c][k].loading else {
            self.tracks[c][k].phase = Phase::Idle;
            return;
        };
        match self.entries[e].state {
            EState::Loading => {}
            EState::Failed(code) => {
                let code = if self.tracks[c][k].reload { E_LOST } else { code };
                self.fail(c, k, code);
            }
            EState::Dead => {
                // Lost while loading: start the request again from the path.
                self.tracks[c][k].phase = Phase::Queued;
            }
            EState::Free => self.fail(c, k, E_LOST),
            EState::Ready => {
                let h = self.entries[e].handle;
                if self.tracks[c][k].fresh {
                    let plen = self.tracks[c][k].plen;
                    let mut path = [0u8; PATH];
                    path[..plen].copy_from_slice(&self.tracks[c][k].path[..plen]);
                    let snap = if self.tracks[c][k].reload { self.tracks[c][k].snap } else { self.tracks[c][k].pend.id };
                    match env.stat(&path[..plen]) {
                        Ok(id) if id.same(&snap) => {}
                        _ => return self.fail(c, k, E_CHANGED),
                    }
                }
                if h.token == 0 || env.check(&h) != 0 || h.frames == 0 {
                    // A handle that is not valid now counts as a lost backing.
                    self.invalidate(e);
                    return;
                }
                let Some(next) = self.next_token.checked_add(1) else {
                    return self.fail(c, k, E_NOMEM);
                };
                let token = self.next_token;
                self.next_token = next;
                let rec = Strack {
                    ready: 1,
                    token,
                    frames: h.frames,
                    rate: h.rate,
                    channels: h.channels,
                    fmt: FMT_PCM16,
                    base_l: h.base_l,
                    base_r: if h.channels == 1 { h.base_l } else { h.base_r },
                };
                env.publish(c, k, &rec);
                let t = &mut self.tracks[c][k];
                t.loading = None;
                t.bound = Some(e);
                t.snap = t.pend.id;
                t.token = token;
                t.published = Info { frames: rec.frames, rate: rec.rate, channels: rec.channels, token };
                t.status = READY;
                t.phase = Phase::Idle;
                t.reload = false;
                t.closed = false;
            }
        }
    }

    /// Module unload: unbind all tracks of `client`. True when no transfer can still touch module memory.
    pub fn teardown(&mut self, client: usize, env: &mut impl Env) -> bool {
        if client >= CL || !self.open[client] {
            return true;
        }
        for k in 0..TRACKS {
            if self.tracks[client][k].phase != Phase::Unbinding {
                self.unbind_at(client, k);
            }
        }
        self.tick(env);
        let done = (0..TRACKS).all(|k| self.tracks[client][k].phase == Phase::Idle && self.tracks[client][k].bound.is_none());
        if done {
            self.open[client] = false;
            self.tracks[client] = [TRACK0; TRACKS];
            for p in self.proj.iter_mut() {
                p.sel[client] = [SEL0; TRACKS];
            }
        }
        done
    }

    /// Entries in use (for the UI and tests).
    pub fn entries_used(&self) -> usize {
        self.entries.iter().filter(|e| e.state != EState::Free).count()
    }
    pub fn refs(&self, client: usize, track: usize) -> Option<u32> {
        self.tracks[client][track].bound.map(|e| self.entries[e].refs)
    }
}

#[cfg(test)]
mod tests;
