//! The order of one UI pass: `dkrt_tick` cannot run on the host (`init_once` sets up the arena at a 32-bit address), so the test reads the table it iterates.
use super::{Step, TICK_STEPS};
use dkrt_abi::hooks::tick_phase;

/// The pass runs EARLY, the file jobs, LATE and LAST, then settings, the watch and the mailbox, and the modules' tick ends the table (the order the pass always had).
/// `Click::run` of the machines service relies on LAST having run before the modules tick, and a module tick relies on every other step being over.
#[test]
fn the_steps_run_in_order_and_the_modules_tick_follows_the_mailbox() {
    assert_eq!(
        TICK_STEPS,
        [
            Step::Hook(tick_phase::EARLY, true),
            Step::Hook(tick_phase::FILES, false),
            Step::Hook(tick_phase::LATE, true),
            Step::Hook(tick_phase::LAST, true),
            Step::Settings,
            Step::Watch,
            Step::Mailbox,
            Step::Hook(tick_phase::MODULES, false),
        ]
    );
}
