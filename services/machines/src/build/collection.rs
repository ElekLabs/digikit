//! What `init` declared of the machines: the collected machines, pages and parameters (`Build`).
use super::*;
use dkrt_sdk::*;
use dkrt_svc_params::builder::{fail, find, put_text, room};

/// The most machines, pages and records (machines, pages, parameters and click actions together) of one module.
pub const MAX_M: usize = limits::MACHINE_ROWS;
pub const MAX_PG: usize = 16;
pub const MAX_PA: usize = dkneeds::MAX_RECORDS as usize;
pub(super) const TEXT: usize = 24;
pub(super) const KNOBS: usize = limits::PAGE_KNOBS;
pub(super) const KNOB_PARAM: u32 = 0x8000_0000;
pub(super) const POS_END: u32 = 255;

#[derive(Clone, Copy)]
pub struct CMachine {
    /// the machine's index in the module (`DK_MACHINE_ID_<ID>`): the position of its key in the keys table
    pub index: u32,
    pub name: [u8; TEXT],
    pub short: [u8; TEXT],
}
#[derive(Clone, Copy)]
pub struct CPage {
    pub owner: u8,
    pub name: [u8; TEXT],
    /// the tab label of the page record (`short_name` of `dk_machine_add_page`)
    pub short: [u8; TEXT],
    pub canvas: bool,
    pub flags: u32,
    pub draw: u32,
    pub event: u32,
    pub arg: u32,
    pub nbind: u8,
    /// (kind 1 param / 2 local, collection param index or local id, encoder slot)
    pub binds: [[u32; 3]; 8],
    pub nparam: u8,
}
#[derive(Clone, Copy)]
pub struct CParam {
    pub page: u8,
    pub name: [u8; 32],
    pub short: [u8; 16],
    /// raw = unit * 256
    pub min: i32,
    pub max: i32,
    pub def: i32,
    pub glyph: u32,
    pub latched: bool,
    pub click: u32,
    pub click_arg: u32,
}

const M0: CMachine = CMachine {
    index: 0,
    name: [0; TEXT],
    short: [0; TEXT],
};
const G0: CPage = CPage {
    owner: 0,
    name: [0; TEXT],
    short: [0; TEXT],
    canvas: false,
    flags: 0,
    draw: 0,
    event: 0,
    arg: 0,
    nbind: 0,
    binds: [[0; 3]; 8],
    nparam: 0,
};
const P0: CParam = CParam {
    page: 0,
    name: [0; 32],
    short: [0; 16],
    min: 0,
    max: 127 * 256,
    def: 0,
    glyph: 0,
    latched: false,
    click: 0,
    click_arg: 0,
};

/// What `init` declared of the machines.
pub struct Build {
    pub err: i32,
    pub machines: [CMachine; MAX_M],
    pub nm: usize,
    pub pages: [CPage; MAX_PG],
    pub np: usize,
    pub params: [CParam; MAX_PA],
    pub nparam: usize,
    records: usize,
}

impl Build {
    pub const fn new() -> Build {
        Build {
            err: 0,
            machines: [M0; MAX_M],
            nm: 0,
            pages: [G0; MAX_PG],
            np: 0,
            params: [P0; MAX_PA],
            nparam: 0,
            records: 0,
        }
    }

    /// Nothing was declared.
    pub fn is_empty(&self) -> bool {
        self.nm + self.np + self.nparam == 0
    }

    /// The records of the collection so far (machines, pages, parameters, click actions).
    pub fn records(&self) -> usize {
        self.records
    }

    fn fail(&mut self, e: i32) {
        fail(&mut self.err, e);
    }

    /// One more record of the bounded collection; false (error kept) when full.
    fn room(&mut self) -> bool {
        room(&mut self.err, &mut self.records, MAX_PA)
    }

    pub fn add_machine(&mut self, index: u32, name: *const u8, short: *const u8) -> *mut u8 {
        if self.nm >= MAX_M || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.nm;
        let mut m = M0;
        m.index = index;
        let mut e = 0;
        unsafe {
            put_text(&mut m.name, name, &mut e);
            put_text(&mut m.short, short, &mut e);
        }
        self.fail(e);
        self.machines[i] = m;
        self.nm += 1;
        &mut self.machines[i] as *mut CMachine as *mut u8
    }

    pub fn machine_add_page(&mut self, m: *mut u8, name: *const u8, short: *const u8) -> *mut u8 {
        let owner = match find(&self.machines, m) {
            Some(i) => i,
            None => return placeholder(),
        };
        if self.np >= MAX_PG || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.np;
        let mut g = G0;
        g.owner = owner as u8;
        let mut e = 0;
        unsafe {
            put_text(&mut g.name, name, &mut e);
            put_text(&mut g.short, short, &mut e);
        }
        self.fail(e);
        self.pages[i] = g;
        self.np += 1;
        &mut self.pages[i] as *mut CPage as *mut u8
    }

    /// `dk_page_add_param`: `def` is in knob units. A handle of another service (an effect's page) is a machine parameter on an effect page: refused.
    pub fn page_add_param(
        &mut self,
        p: *mut u8,
        name: *const u8,
        short: *const u8,
        def: u32,
    ) -> *mut u8 {
        let pg = match find(&self.pages, p) {
            Some(i) => i,
            None => {
                if gate().foreign(p) {
                    self.fail(E_FORMAT);
                }
                return placeholder();
            }
        };
        if self.nparam >= MAX_PA || !self.room() {
            self.fail(E_DECL_FULL);
            return placeholder();
        }
        let i = self.nparam;
        let mut q = P0;
        q.page = pg as u8;
        q.def = (def as i32).wrapping_mul(256);
        let mut e = 0;
        unsafe {
            put_text(&mut q.name, name, &mut e);
            put_text(&mut q.short, short, &mut e);
        }
        self.fail(e);
        if self.pages[pg].nparam as usize >= KNOBS {
            self.fail(E_FORMAT);
        } else {
            self.pages[pg].nparam += 1;
        }
        self.params[i] = q;
        self.nparam += 1;
        &mut self.params[i] as *mut CParam as *mut u8
    }

    pub fn param_range(&mut self, h: *mut u8, min: u32, max: u32) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].min = (min as i32).wrapping_mul(256);
            self.params[i].max = (max as i32).wrapping_mul(256);
        }
    }

    pub fn param_glyph(&mut self, h: *mut u8, glyph: u32) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].glyph = glyph;
        }
    }

    pub fn param_latched(&mut self, h: *mut u8) {
        if let Some(i) = find(&self.params, h) {
            self.params[i].latched = true;
        }
    }

    /// A click action on an effect parameter (a handle of the fx service) is refused at the commit.
    pub fn param_on_click(&mut self, h: *mut u8, f: u32, arg: u32) {
        match find(&self.params, h) {
            Some(i) => {
                if self.params[i].click == 0 && f != 0 && !self.room() {
                    return;
                }
                self.params[i].click = f;
                self.params[i].click_arg = arg;
            }
            None => {
                if gate().foreign(h) {
                    self.fail(E_FORMAT);
                }
            }
        }
    }

    pub fn page_canvas(&mut self, p: *mut u8, flags: u32, draw: u32, event: u32, arg: u32) {
        if let Some(i) = find(&self.pages, p) {
            let g = &mut self.pages[i];
            g.canvas = true;
            g.flags = flags;
            g.draw = draw;
            g.event = event;
            g.arg = arg;
        }
    }

    fn bind(&mut self, p: *mut u8, kind: u32, value: u32, slot: u32) {
        if let Some(i) = find(&self.pages, p) {
            let g = &mut self.pages[i];
            if g.nbind as usize >= 8 || slot >= 8 {
                self.fail(E_FORMAT);
                return;
            }
            let n = g.nbind as usize;
            g.binds[n] = [kind, value, slot];
            g.nbind += 1;
        }
    }

    pub fn page_bind(&mut self, p: *mut u8, prm: *mut u8, slot: u32) {
        match find(&self.params, prm) {
            Some(i) => self.bind(p, 1, i as u32, slot),
            None => {
                if find(&self.pages, p).is_some() {
                    self.fail(E_FORMAT); // a param that is not ours (a placeholder after an earlier error: the error is kept already)
                }
            }
        }
    }

    pub fn page_bind_local(&mut self, p: *mut u8, local: u32, slot: u32) {
        self.bind(p, 2, local, slot);
    }
}
