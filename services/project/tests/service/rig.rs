//! The service with its hooks' record and the fake.
use crate::*;
use dkrt_svc_project::engine::*;

/// The service, its hooks' record and the fake: `tick` and the I/O task by hand.
pub(crate) struct Rig {
    pub(crate) p: Box<ProjectService>,
    pub(crate) job: Box<StoreJob>,
    pub(crate) pend: Pending,
    pub(crate) f: Fake,
}

impl Rig {
    pub(crate) fn new() -> Rig {
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
            job: Box::new(StoreJob::new()),
            pend: Pending::new(),
            f,
        }
    }
    pub(crate) fn rec(&mut self, slot: u32, name: &[u8], stamp: u32) {
        self.f.recs.insert(
            slot,
            SlotRecord {
                name: n16(name),
                stamp,
            },
        );
    }
    pub(crate) fn hook_slot(&mut self, s: u32) {
        self.pend.slot_val = s;
        self.pend.slot_flag = 1;
    }
    pub(crate) fn hook_save(&mut self, s: u32) {
        self.pend.save_slot = s;
        self.pend.save_flag = 1;
    }
    pub(crate) fn hook_clear(&mut self, s: u32) {
        self.pend.clear[(s / 8) as usize] |= 1 << (s % 8);
    }
    pub(crate) fn hook_paste(&mut self, s: u32) {
        self.pend.paste[(s / 8) as usize] |= 1 << (s % 8);
    }
    /// One UI pass, then the I/O task runs the job that was posted.
    pub(crate) fn pass(&mut self) {
        self.p.tick(&mut self.f, &self.pend, &self.job);
        self.pump();
        self.f.runtime_step();
    }
    pub(crate) fn pump(&mut self) {
        if self.f.posted {
            self.f.posted = false;
            let mut n = 0;
            let st = loop {
                // the I/O task: the step works on the job's cell and the fake's files only
                let r = self.job.io_step(&mut self.f);
                if r != IO_MORE {
                    break r;
                }
                n += 1;
                assert!(n <= BUSY_RERUNS + 2);
            };
            if self.f.cut_before_close {
                return;
            }
            self.job.done(st);
        }
    }
    /// Passes until the service has nothing to do (time does not pass).
    pub(crate) fn settle(&mut self) {
        for _ in 0..200 {
            self.pass();
            if !self.p.busy(&self.job) && !self.f.posted {
                // one more pass to take what a hook left
                self.p.tick(&mut self.f, &self.pend, &self.job);
                self.pump();
                if !self.p.busy(&self.job) {
                    return;
                }
            }
        }
        panic!("the service did not settle");
    }
    /// Make `slot` current as a stock load does: the record exists, the setter ran.
    pub(crate) fn load(&mut self, slot: u32) {
        self.hook_slot(slot);
        self.settle();
    }
    pub(crate) fn put(&mut self, k: &[u8], v: &[u8]) -> i32 {
        self.p.scope_put(OWNER, k, v)
    }
    pub(crate) fn get(&self, k: &[u8]) -> Vec<u8> {
        let mut out = [0u8; 64];
        let n = self.p.scope_get(OWNER, k, &mut out);
        assert!(n >= 0, "get {k:?}: {n}");
        out[..n as usize].to_vec()
    }
}
