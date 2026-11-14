//! The health report: the measurements of `core/health` with the arena and DSP area (`dkrt_health`), and the block clock.

use crate::fw_blocks;
use crate::platform::st;
use crate::state::init_once;
use crate::{arena, dsp, store};
use dkrt_sdk::*;

extern "C" {
    // Symbols of core/health; each resolves to 0 when the module is not in the image (`[object] optional`).
    fn health_read();
    fn health_heap();
}

/// What core/health fills (`HealthRaw` of modules/core/adapters/health/src/cf/health.c): 20 words.
#[repr(C)]
struct HealthRaw {
    magic: u32,
    version: u32,
    blocks: u32,
    windows: u32,
    idle_total: u32,
    idle_win: u32,
    idle_ref: u32,
    dsp_avg: u32,
    dsp_max: u32,
    dsp_n: u32,
    dsp_last: u32,
    flags: u32,
    heap_total: u32,
    heap_free: u32,
    heap_largest: u32,
    heap_allocs: u32,
    heap_free_blocks: u32,
    heap_walked: u32,
    pad: [u32; 2],
}
const RAW_MAGIC: u32 = 0x484c_5448;
const RAW_WINDOW: u32 = 1;
const RAW_HEAP: u32 = 2;
const RAW_CUT: u32 = 4;
const RAW_BAD: u32 = 8;



/// The last result of the mailbox command `CMD_HEALTH` (the proof reads it).
#[no_mangle]
pub static mut DKRT_HEALTH: DkrtHealth = unsafe { core::mem::zeroed() };
/// DSP blocks counted since boot (1500 per second; wraps): the firmware's own counter (anchor `cf.link.site.block_isr.counter_lit`), so a clock for
/// a UI that refreshes at a bounded rate and for the stable period of safe mode. 0 when the anchor does not point at a counter and the
/// image has no `core/health` (whose own count is the fallback). Any task; reads one word.
#[no_mangle]
pub unsafe extern "C" fn dkrt_blocks() -> u32 {
    if let Some(b) = fw_blocks() {
        return b;
    }
    if health_fn(health_read) == 0 {
        return 0;
    }
    let mut r: HealthRaw = core::mem::zeroed();
    let read: unsafe extern "C" fn(*mut HealthRaw) = core::mem::transmute(health_fn(health_read));
    read(&mut r);
    if r.magic == RAW_MAGIC {
        r.blocks
    } else {
        0
    }
}



fn health_fn(f: unsafe extern "C" fn()) -> usize {
    core::hint::black_box(f as usize)
}

/// Is `core/health` in the image?
pub fn health_present() -> bool {
    health_fn(health_read) != 0
}

/// Fill `out` (a `DkrtHealth`): the measurements of core/health (CPU idle counter, the DSP's block cycle count, the heap) with the
/// runtime's arena and DSP area. Heap figures come from walking the firmware's free lists (bounded, under its mutex): ask about twice
/// a second at most. Without core/health the flag `HEALTH_MODULE` is 0 and only the arena and DSP area are filled. Task context.
#[no_mangle]
pub unsafe extern "C" fn dkrt_health(out: *mut DkrtHealth) {
    init_once();
    dkrt_health_light(&mut *out, true);
    store::figures(&mut *out);
}



/// `dkrt_health` without the store figures (which can read the card) and, when `walk_heap` is false, without the heap walk: what the log's
/// snapshot uses. The store figures are left 0 and `HEALTH_STORE` clear.
pub unsafe fn dkrt_health_light(o: &mut DkrtHealth, walk_heap: bool) {
    let s = st();
    let (afree, alargest) = arena::stats();
    o.magic = HEALTH_MAGIC;
    o.version = 3;
    o.flags = 0;
    o.arena_total = s.arena_hi - s.arena_lo;
    o.arena_free = afree;
    o.arena_largest = alargest;
    o.dsp_area_total = dsp::area_bytes();
    let (dfree, dlargest) = dsp::area_stats();
    o.dsp_area_free = dfree;
    o.dsp_area_largest = dlargest;
    o.dsp_period = DSP_BLOCK_CYCLES;
    o.store_total = store::layout().limit() / 2;
    if health_fn(health_read) == 0 {
        return;
    }
    let mut r: HealthRaw = core::mem::zeroed();
    let read: unsafe extern "C" fn(*mut HealthRaw) = core::mem::transmute(health_fn(health_read));
    read(&mut r);
    if r.magic != RAW_MAGIC {
        return;
    }
    let mut fl = HEALTH_MODULE;
    o.blocks = r.blocks;
    o.windows = r.windows;
    o.idle_win = r.idle_win;
    o.idle_ref = r.idle_ref;
    // load = 100 % - the window's idle iterations as a share of the best window; valid from the second window, when the reference
    // is a window with the loop running
    if r.flags & RAW_WINDOW != 0 && r.windows >= 3 && r.idle_ref >= 1000 {
        let share = r.idle_win / (r.idle_ref / 1000);
        o.cpu_x10 = 1000u32.saturating_sub(share.min(1000));
        fl |= HEALTH_CPU;
    }
    if r.flags & RAW_WINDOW != 0 && r.dsp_n != 0 {
        o.dsp_avg = r.dsp_avg;
        o.dsp_max = r.dsp_max;
        o.dsp_avg_x10 = (r.dsp_avg.min(DSP_BLOCK_CYCLES * 4) * 1000) / DSP_BLOCK_CYCLES;
        o.dsp_max_x10 = (r.dsp_max.min(DSP_BLOCK_CYCLES * 4) * 1000) / DSP_BLOCK_CYCLES;
        fl |= HEALTH_DSP;
    }
    if walk_heap && health_fn(health_heap) != 0 {
        let heap: unsafe extern "C" fn(*mut HealthRaw) = core::mem::transmute(health_fn(health_heap));
        heap(&mut r);
        if r.flags & RAW_HEAP != 0 {
            o.heap_total = r.heap_total;
            o.heap_free = r.heap_free;
            o.heap_largest = r.heap_largest;
            o.heap_allocs = r.heap_allocs;
            o.heap_free_blocks = r.heap_free_blocks;
            fl |= HEALTH_HEAP;
            if r.flags & RAW_CUT != 0 {
                fl |= HEALTH_HEAP_CUT;
            }
            if r.flags & RAW_BAD != 0 {
                fl |= HEALTH_HEAP_BAD;
            }
        }
    }
    o.flags = fl;
}
