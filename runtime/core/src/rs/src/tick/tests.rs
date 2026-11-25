//! The order of one UI pass: `dkrt_tick` cannot run on the host (`init_once` sets up the arena at a 32-bit address), so the test reads the table it iterates.
use super::{Step, TICK_STEPS};
use dkrt_abi::hooks::tick_phase;

fn position(step: Step) -> usize {
    TICK_STEPS
        .iter()
        .position(|s| *s == step)
        .unwrap_or_else(|| panic!("{step:?} is not in the table"))
}

/// `Click::run` of the machines service (a module's click callback) relies on LAST having run before the modules tick, and a module tick relies on the mailbox
/// (and so every other step) being over: MODULES follows LAST and the mailbox, and the mailbox follows LAST.
#[test]
fn the_modules_tick_follows_last_and_the_mailbox() {
    let last = position(Step::Hook(tick_phase::LAST, true));
    let mailbox = position(Step::Mailbox);
    let modules = position(Step::Hook(tick_phase::MODULES, false));
    assert!(last < modules, "MODULES runs after LAST");
    assert!(mailbox < modules, "MODULES runs after the mailbox");
    assert_eq!(modules, TICK_STEPS.len() - 1, "MODULES ends the pass");
}

/// Each step appears once: a repeated step would run a service phase twice per pass.
#[test]
fn every_step_runs_once() {
    for (i, step) in TICK_STEPS.iter().enumerate() {
        assert_eq!(
            TICK_STEPS.iter().position(|s| s == step),
            Some(i),
            "{step:?} is listed twice"
        );
    }
}
