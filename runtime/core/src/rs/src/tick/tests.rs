//! The order of one UI pass: `dkrt_tick` cannot run on the host (`init_once` sets up the arena at a 32-bit address), so the test reads the table it iterates.
use super::TICK_STEPS;
use dkrt_abi::hooks::tick_phase;

/// The hook phases run EARLY, then the file jobs, LATE and LAST, and the modules' tick ends the table. `Click::run` of the machines service relies on LAST having run before
/// the modules tick, and a module tick relies on every service phase being over.
#[test]
fn the_hook_phases_run_in_order_and_the_modules_tick_follows_the_last_one() {
    let phases: Vec<u32> = TICK_STEPS.iter().map(|&(p, _)| p).collect();
    assert_eq!(
        phases,
        [
            tick_phase::EARLY,
            tick_phase::FILES,
            tick_phase::LATE,
            tick_phase::LAST,
            tick_phase::MODULES
        ]
    );
}
