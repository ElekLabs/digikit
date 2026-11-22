//! The fx service as the runtime has it: a service file (`fx.dks`), not a row of the image. A host test of the DSP section, the effect rows or the attribution of
//! an effect needs its row: this adds it to the table as `Registry::add` does at the commit of the loaded file, and takes it out as `Registry::remove` does at the unload.
//! Take the test lock first (`test_lock::LOCK`): the row table is a static.
use crate::core_api::RealCore;
use crate::svc_rows;
use dkrt_abi::service::ServiceRow;
use dkrt_sdk::records::KINDS_FX;
use dkrt_sdk::OK;
use dkrt_svc_fx::records::Fx;

/// The module record of the service file in these tests.
const FILE_REC: usize = 7;

/// The fx service's effect rows (the service's own static, not the loader's state): a host test of the core's cleanup or invariants seeds and reads them here.
pub(crate) fn frows() -> &'static mut dkrt_svc_fx::rows::FxRows {
    dkrt_svc_fx::rows::frows()
}

/// Back to the start state (all rows free) of the services that hold DSP rows: the machines service's voice rows and the fx service's effect rows, with their guard memory.
pub(crate) fn reset_rows() {
    dkrt_svc_machines::rows::reset();
    dkrt_svc_fx::rows::reset()
}

/// The row of the fx service file as the host tests have it.
pub(crate) fn row() -> &'static ServiceRow {
    static ROW: ServiceRow = ServiceRow::of::<Fx, RealCore>("fx", &KINDS_FX);
    &ROW
}

/// The builder call `dk_module_ctx_add_fx` of the module whose context is `ctx` (its `init` is open), for the effect of `index` of its header table.
pub(crate) unsafe fn add_fx(ctx: *mut u8, index: u32) {
    dkrt_svc_fx::build::add_fx(&RealCore, ctx, index, b"Test\0".as_ptr(), b"TST\0".as_ptr());
}

pub(crate) struct LoadedFx;

impl LoadedFx {
    pub(crate) fn new() -> LoadedFx {
        assert_eq!(unsafe { svc_rows::add(FILE_REC, row(), None) }, OK);
        LoadedFx
    }
}

impl Drop for LoadedFx {
    fn drop(&mut self) {
        unsafe { svc_rows::remove(FILE_REC) };
    }
}
