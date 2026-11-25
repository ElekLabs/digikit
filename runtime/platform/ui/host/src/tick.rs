use crate::*;

pub(crate) type TickFn = extern "C" fn(Ctx);

/// A `dk_ui_tick` timer: the model keeps time in milliseconds, which only `driver::advance_ms` moves.
#[derive(Clone, Copy)]
pub(crate) struct Tick {
    pub f: TickFn,
    pub ctx: Ctx,
    pub ms: u32,
    pub last: u64,
}

#[no_mangle]
pub extern "C" fn dk_ui_tick(
    _ui: *mut c_void,
    f: Option<TickFn>,
    ctx: Ctx,
    period_ms: u32,
) -> c_int {
    with(|s| {
        let Some(f) = f else { return -1 };
        if !s.has(CAP_TICK) {
            return -1;
        }
        s.log(format!("tick {period_ms}"));
        let now = s.now_ms;
        let at = s
            .ticks
            .iter()
            .position(|t| t.f as usize == f as usize && t.ctx == ctx);
        match (at, period_ms) {
            (Some(i), 0) => {
                s.ticks.remove(i);
            }
            (Some(i), ms) => {
                s.ticks[i].ms = ms.min(1_000_000);
                s.ticks[i].last = now;
            }
            (None, 0) => {}
            (None, ms) => {
                if s.ticks.len() >= TICKS {
                    return -1;
                }
                s.ticks.push(Tick {
                    f,
                    ctx,
                    ms: ms.min(1_000_000),
                    last: now,
                });
            }
        }
        0
    })
}
