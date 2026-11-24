//! The action of a parameter row: the callback a click on it queues.
/// The callback of parameter code `code`, or 0. `actions[k]` belongs to the parameter row `first + k`.
pub fn find(actions: &[u32], first: u32, code: i32) -> u32 {
    let k = (code as u32).wrapping_sub(first) as usize;
    if code >= 0 && k < actions.len() {
        actions[k]
    } else {
        0
    }
}

/// What a click queues: the callback of the parameter `code` and the track (asked for only when there is a callback), or None: the stock click.
pub fn resolve(
    actions: &[u32],
    first: u32,
    code: i32,
    track: impl FnOnce() -> u32,
) -> Option<(u32, u32)> {
    match find(actions, first, code) {
        0 => None,
        f => Some((f, track())),
    }
}
