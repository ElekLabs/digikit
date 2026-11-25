//! The order of one UI pass: `dkrt_tick` cannot run on the host (`init_once` sets up the arena at a 32-bit address), so the test reads the table it iterates.
use super::SERVICE_PHASES;
use dkrt_abi::hooks::tick_phase;

/// The service phases run EARLY, then the file jobs, LATE and LAST, and the modules' tick comes after all of them (it is no entry of the table, and `dkrt_tick` runs it
/// once after the loop). `Click::run` of the machines service relies on LAST having run before the modules tick, and a module tick relies on every service phase being over.
#[test]
fn the_service_phases_run_in_order_and_the_last_one_ends_the_table() {
    let phases: Vec<u32> = SERVICE_PHASES.iter().map(|&(p, _)| p).collect();
    assert_eq!(
        phases,
        [
            tick_phase::EARLY,
            tick_phase::FILES,
            tick_phase::LATE,
            tick_phase::LAST
        ]
    );
    assert!(
        !phases.contains(&tick_phase::MODULES),
        "the modules tick after the table, once"
    );
}
