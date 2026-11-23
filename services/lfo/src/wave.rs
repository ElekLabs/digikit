//! `lfo` service, the WAVE maximum: the rows of the stock parameter table that hold the maximum of the LFO WAVE parameter, in the RAM copy the page and the encoder read.
use crate::shapes::highest;
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::*;
use dkrt_svc_rows::{dynreg, set_stock_range, stock_range};

/// The largest value of the LFO WAVE parameter (`wave << 8`) while shapes are loaded: the maximum of the WAVE rows follows the highest loaded wave number, so the
/// encoder reaches no number above it. 0 when no shape is loaded: the stock maximum stays (finding 149, "Direct readers of the parameter table").
pub fn wave_max() -> u32 {
    highest() << 8
}

/// The three WAVE rows of the parameter table (one for each LFO): the stock addresses of the table and of the rows, which the port file resolves (`cf.param.data.descriptor_table`,
/// `cf.lfo.data.wave_row.N`), and the pair (minimum, maximum) each row had before the first shape was loaded. Words 0: a host build with no firmware, which has no rows to write.
pub(crate) struct Wave {
    table: u32,
    row: [u32; 3],
    /// the rows hold the shape maximum; `stock` has what to put back
    armed: bool,
    stock: [(u32, u32); 3],
}
static mut WAVE: Wave = Wave {
    table: 0,
    row: [0; 3],
    armed: false,
    stock: [(0, 0); 3],
};

/// The firmware addresses of the WAVE rows (`Lfo::start_in`, from the words `bind_firmware` resolved): the stock table and the rows of the three LFOs. Not called by a host
/// build; with `table` 0 the service writes no row and refuses nothing.
pub fn set_firmware(table: u32, rows: [u32; 3]) {
    let w = unsafe { &mut *addr_of_mut!(WAVE) };
    w.table = table;
    w.row = rows;
    w.armed = false;
}

/// True when `set_firmware` gave the stock table (an image build); a host build has none.
pub(crate) fn has_firmware() -> bool {
    // SAFETY: `WAVE` is written by `set_firmware` at start only.
    unsafe { (*addr_of!(WAVE)).table != 0 }
}

/// The pairs (minimum, maximum) the three WAVE rows hold now, or `None` when the image cannot take the maximum: it has no registry table, or a row is not a stock row of its
/// parameter table. Nothing is written.
pub(crate) unsafe fn wave_rows<C: CoreApi>(core: &C) -> Option<[(u32, u32); 3]> {
    let w = &*addr_of!(WAVE);
    let inf = dynreg(core);
    let mut now = [(0, 0); 3];
    let mut i = 0;
    while i < 3 {
        now[i] = stock_range(core, inf, w.table, w.row[i])?;
        i += 1;
    }
    Some(now)
}

/// Make the maximum of the WAVE rows in the RAM copy of the parameter table equal `wave_max()` while a shape is loaded, and put the stock pair back when the last one
/// is gone. The page and the encoder read the row through the firmware's range accessor, which reads the copy. `E_NO_REGISTRY` when the image cannot take it (`wave_rows`);
/// then nothing was written.
pub(crate) unsafe fn sync_range<C: CoreApi>(core: &C) -> i32 {
    let w = &mut *addr_of_mut!(WAVE);
    if w.table == 0 {
        return OK;
    }
    let now = match wave_rows(core) {
        Some(n) => n,
        None => return E_NO_REGISTRY,
    };
    let inf = dynreg(core);
    let max = wave_max();
    if !w.armed {
        if max == 0 {
            return OK;
        }
        w.stock = now;
        w.armed = true;
    }
    let mut i = 0;
    while i < 3 {
        let (min, top) = if max == 0 {
            w.stock[i]
        } else {
            (w.stock[i].0, max)
        };
        set_stock_range(core, inf, w.table, w.row[i], min, top);
        i += 1;
    }
    w.armed = max != 0;
    OK
}
