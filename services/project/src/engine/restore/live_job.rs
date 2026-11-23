//! The runtime's live job as the restore reads it.

/// The state of the runtime's live job as the restore reads it (`dkrt_live_state`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveJob {
    /// 0 no job yet, 1 queued or running, 2 ended.
    pub phase: u32,
    /// The refusal or failure of an ended job (0: none).
    pub result: i32,
    /// The job's operation (`LIVE_LOAD`, ...).
    pub op: u32,
}
