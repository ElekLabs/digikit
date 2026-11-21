//! The service's filter callback and state addresses, indexed by the assigned filter type.
use crate::filter::{self, FIRST, ROWS};
use dkneeds::filter::contract::{DK_FILTER_LINK_WORD, DK_FILTER_TABLE_BYTES};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::rd32le;
use dkrt_abi::dsp::WIN_VOICES;
use dkrt_abi::dsp::{L2_BYTE_BASE, L2_SW_BASE};
use dkrt_abi::link::{Owner, Sequence};
use dkrt_abi::mem::rd32;
use dkrt_abi::service::{Claim, RowWrite};

const WORDS: u32 = 4;
const _: () = assert!(ROWS as u32 * WORDS * 4 == DK_FILTER_TABLE_BYTES);

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct FilterRow {
    pub owner: u32,
    pub coef_pc: u32,
    pub process_pc: u32,
    pub desc: u32,
    pub state: u32,
    pub stride: u32,
    published: u8,
}

static mut DSP_FILTERS: [FilterRow; ROWS] = unsafe { core::mem::zeroed() };

pub unsafe fn row(index: usize) -> FilterRow {
    DSP_FILTERS[index]
}

pub(super) unsafe fn claim<C: CoreApi>(core: &C, c: &mut Claim) {
    for i in 0..c.job.nfilter {
        let at = c.job.filters.add((i * dkneeds::filter::RECORD) as usize);
        let ty = filter::resolve(core, rd32(at));
        let desc = rd32(at.add(12));
        let state_bytes = rd32le(
            c.job
                .image
                .add((desc + dkneeds::filter::contract::DK_FILTER_DESC_STATE_BYTES) as usize),
        );
        let stride = (state_bytes + 7) & !7;
        let pc = |off| L2_SW_BASE + (c.base - L2_BYTE_BASE + off) / 2;
        DSP_FILTERS[(ty - FIRST) as usize] = FilterRow {
            owner: c.rec as u32 + 1,
            coef_pc: pc(rd32(at.add(4))),
            process_pc: pc(rd32(at.add(8))),
            desc: c.base + desc,
            state: L2_BYTE_BASE + c.byte_shift + c.state_at,
            stride,
            published: 0,
        };
        c.state_at += stride * dkneeds::filter::VOICES;
    }
}

pub(super) unsafe fn release(rec: usize) {
    for row in (*core::ptr::addr_of_mut!(DSP_FILTERS)).iter_mut() {
        if row.owner == rec as u32 + 1 {
            *row = FilterRow::default();
        }
    }
}

/// Payload first, coefficient PC last. A zero coefficient PC disables both dispatch stages.
pub(super) unsafe fn row_step(rec: usize, want: &mut u32, write: bool) -> Option<RowWrite> {
    for (index, row) in (*core::ptr::addr_of_mut!(DSP_FILTERS))
        .iter_mut()
        .enumerate()
    {
        if row.owner != rec as u32 + 1 {
            continue;
        }
        if *want >= WORDS {
            *want -= WORDS;
            continue;
        }
        let (offset, value) = match *want {
            0 => (1, row.process_pc),
            1 => (2, row.state),
            2 => (3, row.stride),
            _ => (0, row.coef_pc),
        };
        let activate = offset == 0;
        return Some(RowWrite {
            win: WIN_VOICES,
            word: DK_FILTER_LINK_WORD + index as u32 * WORDS + offset,
            value,
            published: if write && activate {
                Some(&mut row.published)
            } else {
                None
            },
            last: activate,
            has_undo: activate,
            undo: 0,
        });
    }
    None
}

pub(super) unsafe fn clear<C: CoreApi>(core: &C, rec: usize) -> bool {
    for (index, row) in (*core::ptr::addr_of_mut!(DSP_FILTERS))
        .iter_mut()
        .enumerate()
    {
        if row.owner == rec as u32 + 1 && row.published != 0 {
            let mut seq = Sequence::new();
            seq.push(WIN_VOICES, DK_FILTER_LINK_WORD + index as u32 * WORDS, 0);
            if !core.link_run(Owner::Core, &seq) {
                return false;
            }
            row.published = 0;
        }
    }
    true
}
