/// What a traced call is. The numbers are the log's (`LIFECYCLE`, docs/INSTRUMENTATION.md) and never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Step {
    Place = 1,
    Link = 2,
    Start = 3,
    Install = 4,
    Activate = 5,
    Attach = 6,
    Detach = 7,
    Deactivate = 8,
    Restore = 9,
    ExportState = 10,
    ImportState = 11,
    Stop = 12,
    Free = 13,
    DspLoad = 14,
    CanStop = 15,
    Unbind = 16,
    /// The installer found a task that is not parked and answered busy.
    ParkWait = 17,
    /// A job asks a step again on a later pass because it answered busy.
    BusyRetry = 18,
    /// The steps of a live job (`RELOAD_STATE`).
    JobCheck = 32,
    JobQuiesce = 33,
    JobUnload = 34,
    JobLoad = 35,
    JobDsp = 36,
    JobDone = 37,
}
