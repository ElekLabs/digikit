//! Where the firmware calls a hook from, for the host tests of the Owner guard (`dkrt_svc_rt::Owner`).
//!
//! A hook of a service runs in the context of the firmware code it replaces: the UI task with the owner idle, the UI task inside a part call of the same service (a module's
//! callback that reaches the key walker, so the owner is busy), or an interrupt that can arrive while the owner is busy. Which hook runs in which context is a fact of the
//! port (the table "Hook calling contexts" in `ports/fw/README.md`); this file lets a test fire a hook the way the firmware does and read what the guard did.
//! A service file passes the closure that holds its owner busy (`DK_SVC_OWNER.run(..)`) and the reader of its refusal count (`DK_SVC_OWNER.refused()`).

/// The context a hook is called in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookContext {
    /// The UI task, the owner idle: the key walker, the page draw.
    UiTask,
    /// The UI task inside a part call of the same service: the owner is busy.
    InPartCall,
    /// An interrupt (the LFO tick, vector 191, IPL 5): it may preempt a part call, so the owner can be busy. A hook of this context must never enter the owner.
    Interrupt,
}

/// What firing a hook gave: its result and how many calls the owner's guard refused while it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fired<R> {
    pub result: R,
    pub refused: u32,
}

impl HookContext {
    /// Whether the owner is inside a call when the hook runs.
    pub fn owner_busy(self) -> bool {
        self != HookContext::UiTask
    }

    /// Run `hook` as the firmware would in this context. `hold` runs its argument with the owner busy (`DK_SVC_OWNER.run(..)`), `refused` reads the owner's refusal count.
    pub fn fire<R>(
        self,
        hold: &dyn Fn(&mut dyn FnMut()),
        refused: &dyn Fn() -> u32,
        hook: &mut dyn FnMut() -> R,
    ) -> Fired<R> {
        let before = refused();
        let mut result = None;
        if self.owner_busy() {
            hold(&mut || result = Some(hook()));
        } else {
            result = Some(hook());
        }
        Fired { result: result.expect("the hook ran"), refused: refused() - before }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn a_hook_runs_with_the_owner_busy_in_every_context_but_the_idle_ui_task() {
        let busy = Cell::new(false);
        let hold = |f: &mut dyn FnMut()| {
            busy.set(true);
            f();
            busy.set(false);
        };
        for (ctx, expect) in [
            (HookContext::UiTask, false),
            (HookContext::InPartCall, true),
            (HookContext::Interrupt, true),
        ] {
            let seen = ctx.fire(&hold, &|| 0, &mut || busy.get());
            assert_eq!((ctx, seen.result, seen.refused), (ctx, expect, 0));
        }
    }
}
