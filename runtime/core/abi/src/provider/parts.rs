//! The optional parts of the service contract. Each one is a separate tagged table in the packed row (`row::PART_*`; the tables are in `tables/`); a service adopts
//! only the parts it needs. In version 1 every callback is a method of exactly one part.
use super::{Consumer, ConsumerId, Status, WordSink};
use crate::core_api::CoreApi;
use crate::dsp::{DspSec, Job};
use crate::service::{CheckCtx, Claim, ModuleCommitCtx, Problem, RowWrite, TableFault};
use dkrt_sdk::fault::DIAG_WORDS;

/// The records of the module files that name this service's kinds. The kinds are in the part's table (the `kinds` argument of `service!`).
///
/// A module load runs: `begin` before the module's `init`; `commit` after it (a `PREPARE` and a `WRITE` pass, then `DECLARE`); `check` once per `check_step` over the
/// block the services wrote; then `apply` for each record, in file order, after every provider's `attach`. A part with a refusal stops the walk.
pub trait Records<C: CoreApi>: Sized {
    /// Check the records of kind `kind` that one module wrote. Callable without a device (`patchkit records --check` runs the same rule before load); the table of
    /// the part does not hold it. `bodies` are the bodies (after the 8-byte header) of one module's records of that kind, in file order; each fault goes to `report`.
    /// A service without rules keeps the default, which accepts.
    fn validate(_kind: u32, _bodies: &[&[u8]], _report: &mut dyn FnMut(Problem)) {}

    /// The `init` of module record `module` is about to run: forget what an earlier module collected.
    fn begin(&mut self, _core: &C, _module: usize) {}

    /// The commit of module record `module` after its `init` (`commit_phase`): check what its builder calls collected, write the service's records into `c.sink`.
    fn commit(&mut self, _core: &C, _module: usize, _c: &ModuleCommitCtx) -> Result<(), Status> {
        Ok(())
    }

    /// One step of the validation (`check_step`) of the records over the block the services wrote. Writes nothing.
    fn check(&mut self, _core: &C, _step: u32, _c: &CheckCtx) -> Result<(), Status> {
        Ok(())
    }

    /// Write one record of one of this service's kinds (`body` is its bytes after the header), in file order after every provider's `attach`.
    fn apply(
        &mut self,
        _core: &C,
        _kind: u32,
        _body: &[u8],
        _consumer: &Consumer<'_>,
    ) -> Result<(), Status> {
        Ok(())
    }
}

/// The DSP section of a module file: the parts of its tables this service owns, and the diagnostics of the DSP it reads.
pub trait DspPart<C: CoreApi>: Sized {
    /// The entries of the tables this service owns (called in list order, after the header is validated). The service adds the bytes of the state blocks its entries
    /// need behind the image (`job.state_bytes`, each block rounded to 8).
    fn check_sections(&mut self, _core: &C, _sec: &DspSec, _job: &mut Job) -> Result<(), Status> {
        Ok(())
    }
    /// Free rows for this service's entries.
    fn check_rows(&mut self, _core: &C, _sec: &DspSec) -> Result<(), Status> {
        Ok(())
    }
    /// The machine types of the voices (after every service's rows were checked).
    fn check_types(&mut self, _core: &C, _sec: &DspSec) -> Result<(), Status> {
        Ok(())
    }
    /// Refuse a reservation before anything is reserved.
    fn admit(&mut self, _core: &C, _job: &Job) -> Result<(), Status> {
        Ok(())
    }
    /// Claim this service's rows for the module of `c.rec`.
    fn claim(&mut self, _core: &C, _c: &mut Claim) {}
    /// Give back what `claim` took (the rows are cleared).
    fn release(&mut self, _core: &C, _module: usize) {}
    /// Free the rows of module `module` this service published in the DSP; false when a write failed.
    fn clear(&mut self, _core: &C, _module: usize) -> bool {
        true
    }
    /// The next word to publish for module `module`: `want` counts the steps since the first row; a row of this service takes its steps off `want`, and the row that
    /// holds the step returns its write. The word is read from the row only when `write` is set.
    fn row_step(
        &mut self,
        _core: &C,
        _module: usize,
        _want: &mut u32,
        _write: bool,
    ) -> Option<RowWrite> {
        None
    }
    /// Every row of module `module` is in the DSP.
    fn loaded(&mut self, _core: &C, _module: usize) {}
    /// The module record behind a fault site and the id the DSP reported there (`fault::SITE_*`).
    fn attribute(&mut self, _core: &C, _site: u32, _id: u32) -> Option<usize> {
        None
    }
    /// The polled DSP diagnostics block, once for each poll that returned a full block.
    fn diag(&mut self, _core: &C, _words: &[u32; DIAG_WORDS]) {}
}

/// Work in the UI-loop tick; `phase` is one of `service::tick_phase`.
pub trait Ticker<C: CoreApi> {
    fn tick(&mut self, core: &C, phase: u32);

    /// Work of the same pass that must run before the owner is entered: the table's tick function calls it first, outside the owner's guard (no `&mut self`, so nothing forms a
    /// reference to the object). It is for a call into foreign code that delivers events to the owner (`Owner::post` runs them at once, with the owner idle, in the order the
    /// callbacks fire) ahead of the tick that depends on them. It runs also when the owner is not live (before `start`, after `stop`): its events then meet `Post::NoOwner`. The default does nothing.
    fn pre_tick(_core: &C, _phase: u32) {}
}

/// A synchronous request from a resident app, in the UI task. The service owns operation numbers and layouts.
pub trait Query {
    /// The length of the reply written to `reply`.
    fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> Result<usize, Status>;
}

/// What the service tells the core about itself: its capacity counts, the check of its tables and the words a consumer contributes to diagnostics.
pub trait Describe<C: CoreApi>: Sized {
    /// `(limit, used)` of capacity kind `kind` (`proto::CAPK_*`) if this service counts it; a kind two services count is the sum of their counts.
    fn cap(&self, _core: &C, _kind: u8) -> Option<(u32, u32)> {
        None
    }
    /// The service's tables against the module records: bit `i` of `used` is set when module record `i` is used. 0 when the tables agree; else a number (not 0) and the
    /// first mismatch in `out`.
    fn table_check(&self, _core: &C, _used: u32, _out: &mut TableFault) -> u32 {
        0
    }
    /// The words this service holds for consumer `who` (a module record + 1): read-only, opaque to the core, which copies them into a section of the MODULES reply. It has no
    /// object: it runs in the task of the caller (the MODULES command handler), outside the owner's busy state, so it answers while the owner is busy in another call. It reads state
    /// the service published for that purpose in a `Shared` cell (or a table whose reads tolerate a task switch), and does no I/O and no call out.
    fn describe(_core: &C, _who: ConsumerId, _out: &mut WordSink) {}
}

/// The state a service carries over a reload of itself: the runtime calls `export` on the old copy after its entries are restored and before `stop`, and `import` on the
/// new copy after `start`, before its entries are installed. The bytes live in the runtime in between and never reach the card.
///
/// A handoff needs `ACCEPTS_FROM <= old VERSION <= VERSION`; else, or when nothing was exported, the new copy gets `import(None)` and decides what to start with. `import` changes
/// fields in place (never replaces `*self`), gets no `Outside` token, and an `Err` fails the start.
pub trait State<C: CoreApi>: Sized {
    /// The layout of the bytes this copy exports.
    const VERSION: u16;
    /// The oldest `VERSION` that `import` takes.
    const ACCEPTS_FROM: u16;
    /// The most bytes `export` writes (the runtime reserves this much).
    fn export_max(&self, core: &C) -> usize;
    /// Write the state into `out` (at least `export_max` long): the count written.
    fn export(&mut self, core: &C, out: &mut [u8]) -> usize;
    /// Take the old copy's bytes, or `None` when there are none to take.
    fn import(&mut self, core: &C, from: Option<&[u8]>) -> Result<(), Status>;
}

/// The events a service takes while one of its own calls is running (the part `Reentry` of `service!`; it has no table in the row).
///
/// A callback of the service (a pool event, a hook) may fire inside a call the object made (`tick`, `release`, ...). The owner (`Owner` of `dkrt-svc-rt`) is busy then
/// and never forms a second `&mut Self`: `Owner::post` queues the event, and the owner hands it to [`Reentry::apply`] right after the outer call returns, in the same
/// task, in order.
pub trait Reentry<C: CoreApi> {
    /// One event: plain data (what the callback was given), small, copied into the queue.
    type Event: Copy;
    /// How many events wait at most while a call runs. A full queue refuses the event and counts it (`Owner::refused`).
    const QUEUE: usize;
    /// Take `event`: runs after the call it arrived in, with the object free again.
    fn apply(&mut self, core: &C, event: Self::Event);
}
