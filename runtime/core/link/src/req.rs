use dkrt_abi::link::*;

pub(crate) const R_FREE: u8 = 0;
pub(crate) const R_QUEUED: u8 = 1;
pub(crate) const R_ACTIVE: u8 = 2;
pub(crate) const R_ROLLBACK: u8 = 3;
pub(crate) const R_DONE: u8 = 4;
pub(crate) const R_FAILED: u8 = 5;
pub(crate) const R_RBFAILED: u8 = 6;

pub(crate) const K_BULK: u8 = 0;
pub(crate) const K_OWNED: u8 = 1;
pub(crate) const K_SEQ: u8 = 2;

#[derive(Clone, Copy)]
pub(crate) struct Req {
    pub(crate) state: u8,
    pub(crate) owner: u8,
    pub(crate) kind: u8,
    pub(crate) cancel: bool,
    pub(crate) win: u8,
    pub(crate) id: u16,
    pub(crate) order: u32,
    pub(crate) dst: u32,
    pub(crate) src: *const u8,
    pub(crate) len: u32,
    pub(crate) off: u32,
    pub(crate) pos: u8,
    pub(crate) rb: i8,
    pub(crate) code: u32,
    pub(crate) bytes: [u8; OWNED_MAX],
    pub(crate) seq: Sequence,
}
pub(crate) const REQ0: Req = Req {
    state: R_FREE,
    owner: 0,
    kind: K_BULK,
    cancel: false,
    win: 0,
    id: 0,
    order: 0,
    dst: 0,
    src: core::ptr::null(),
    len: 0,
    off: 0,
    pos: 0,
    rb: 0,
    code: 0,
    bytes: [0; OWNED_MAX],
    seq: Sequence::new(),
};
