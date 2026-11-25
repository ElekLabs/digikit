//! The order of one UI pass. `dkrt_tick` cannot run on the host (`init_once` sets up the arena at a 32-bit address), so the test reads its source.

/// The body of `dkrt_tick`, without the comments.
fn body() -> String {
    let src = include_str!("../tick.rs");
    let from = src.find("pub unsafe extern \"C\" fn dkrt_tick()").expect("dkrt_tick");
    let end = src[from..].find("\n}\n").expect("end of dkrt_tick");
    src[from..from + end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The service phases run EARLY, then the file jobs, LATE and LAST, and the modules' tick comes after all of them. `Click::run` of the machines service (the click
/// callback of a module, run outside the owner) relies on LAST having run before the modules tick, and a module tick relies on every service phase being over.
#[test]
fn the_service_phases_run_in_order_and_the_modules_tick_after_the_last_phase() {
    let b = body();
    let at = |needle: &str| b.find(needle).unwrap_or_else(|| panic!("`{needle}` is not in dkrt_tick"));
    let order = [
        "phase(tick_phase::EARLY)",
        "hooks::tick(tick_phase::FILES)",
        "phase(tick_phase::LATE)",
        "phase(tick_phase::LAST)",
        "hooks::tick(tick_phase::MODULES)",
    ];
    let places: Vec<usize> = order.iter().map(|n| at(n)).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]), "the pass runs {order:?} in this order, found at {places:?}");
    assert_eq!(b.matches("tick_phase::MODULES").count(), 1, "the modules tick once in a pass");
}
