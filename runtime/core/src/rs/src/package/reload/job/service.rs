//! The reload of a service in the live job: its one step after the checks and the drain.
use super::*;

impl Job {
    /// The UNLOAD step of a service reload: one call over the closure (`Ops::reload_service`), asked again each pass while it answers `E_BUSY` (a restore waits for a task to park;
    /// the same call finishes it). `OK` ends the job with the closure back from the new file. A refusal or failure ends it too: when no member went down (`Ops::abort` gives every
    /// member its tick back) the old service runs on, else the closure is down and the job says so (`torn`). Nothing is rolled back.
    pub(super) unsafe fn reload_service<O: Ops>(&mut self, o: &mut O) {
        let r = o.reload_service(self.rec, self.path.as_ptr());
        if r == OK {
            self.torn = true;
            self.end(o, OK, true);
        } else if r == E_BUSY && !self.busy(o) {
            // nothing changed, or a restore waits: the call runs again on a later pass
        } else {
            self.torn = !o.abort(self.rec);
            self.end(o, r, false);
        }
    }
}
