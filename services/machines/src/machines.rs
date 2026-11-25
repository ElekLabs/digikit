//! The owner object of the `machines` service. It holds the state that the old statics held: the collection of the builder calls (`Builder`), the click state of the machine
//! page (`Slots`), the parking passes of absent numbers (`Parking`) and what `attach` hands to the per-record `apply` (`Applied`). The registry state, the sticky numbers, the voice
//! rows and the guard's memory are tables the service keeps by name (`registry::DKRT_REG`, `assign::DKRT_ASSIGN`, `rows::DKRT_VROWS`, `guard::DKRT_GUARD`), and the image table the
//! assembler hooks read is `info::DKRT_MF_TAB`: README.md names the reader of each.
//! `Provider` (`start`, `attach`, `detach`, `settle`, `reload_done`), `Ticker` and `Describe` are here, in one block each; `records.rs` and `dsp/part.rs` hold the other parts.
use crate::apply::Applied;
use crate::build::Builder;
use crate::native_page::PageTable;
use crate::parking::Parking;
use crate::slots::Slots;
use crate::{assign, dsp, info, registry, selection_view, unload};
use core::mem::MaybeUninit;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::WordSink;
use dkrt_abi::provider::{Consumer, ConsumerId, Describe, Provider, Status, Ticker};
use dkrt_abi::service::{tick_phase, SettleCtx, TableFault};
use dkrt_sdk::proto::{
    CAPK_MACHINE_ROWS, CAPK_MAP_ENTRIES, CAPK_PAGE_ROWS, CAPK_PARAM_ROWS, CAPK_VOICE_ROWS,
};
use dkrt_sdk::records::KINDS_MACHINES;
use dkrt_sdk::{E_FORMAT, E_NO_REGISTRY, OK};
use dkrt_svc_rows::rows_info::RegState;

/// The machines service: nothing collected, no click queued, nothing parked. The file's cell holds the one instance (`Provider::start_in`).
pub struct Machines {
    /// The registry state only the owner reads: the owners of the parameter and page rows and the strings the firmware's rows point at.
    reg: RegState,
    builder: Builder,
    clicks: Slots,
    parking: Parking,
    applied: Applied,
    /// The native pages of the registered machines (`native_page/`): the hooks of the service file read it, and its descriptors are addresses of it, so it stays where the owner
    /// is made.
    pages: PageTable,
    /// The types in the machine list `start` built (`list`).
    listed: u32,
}

impl Machines {
    /// An owner with nothing collected, no action set and the gate closed; no image table is bound (`start` binds it).
    pub fn new() -> Machines {
        Machines {
            // SAFETY: all-zero bytes are the state of a registry with no row owned
            reg: unsafe { core::mem::zeroed() },
            builder: Builder::new(),
            clicks: Slots::new(),
            parking: Parking::new(),
            applied: Applied::new(),
            pages: PageTable::new(0),
            listed: 0,
        }
    }

    /// The collection of the module whose `init` is open: the builder calls of the service file reach it here.
    pub fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    /// The registry state (the owners of the parameter and page rows, the strings of the machine, parameter and page rows).
    pub fn reg(&self) -> &RegState {
        &self.reg
    }

    /// The parking passes and the registry state, for the DSP part (`dsp/part.rs`): one borrow of each field.
    pub(crate) fn parking_and_reg(&mut self) -> (&mut Parking, &mut RegState) {
        (&mut self.parking, &mut self.reg)
    }

    /// The commit of the open module's collection: the free rows it needs are read from the registry state.
    ///
    /// # Safety
    /// `c` is the loader's commit context.
    pub(crate) unsafe fn commit_builder<C: CoreApi>(
        &mut self,
        core: &C,
        module: usize,
        c: &dkrt_abi::service::ModuleCommitCtx,
    ) -> i32 {
        self.builder.commit(core, &self.reg, module, c)
    }

    /// `dk_param_get` and `dk_param_set` of the file, owner half: what the call resolves to (the owner of the parameter rows is the registry state). The firmware half runs
    /// outside the owner (`paramapi::read`, `paramapi::write`).
    ///
    /// # Safety
    /// As `Slots::param_target`.
    pub unsafe fn param_target<C: CoreApi>(
        &self,
        core: &C,
        track: u32,
        word: u32,
    ) -> Result<crate::paramapi::Target, i32> {
        self.clicks
            .param_target(core, &self.reg.param_owner, track, word)
    }

    /// Give parameter row `k` an owner module, as `apply` does for a loaded module's row: the host test of the service file has no loaded module.
    #[cfg(any(test, feature = "test-support"))]
    pub fn own_row_for_test(&mut self, k: usize, owner: u8) {
        self.reg.param_owner[k] = owner;
    }

    /// Open the click flow as a click on a row of `owner` would (`take_click`): the host test of the service file has no loaded module to start one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_flow_for_test(&mut self, ctx: crate::paramapi::Ctx) {
        self.reg.param_owner[0] = ctx.owner;
        self.clicks.begin_flow(ctx);
    }

    /// Register page 0 of the first reserved type with encoder 3 bound to parameter code `code`: the host test of the service file has no loaded module that registers one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_page_for_test(&mut self, code: u32) {
        let mut bindings = [[0u32; 3]; crate::native_page::ENCODERS];
        bindings[0] = [1, code, 3];
        self.pages
            .register(0, 0, 1, 0, b"T", 1, 1, &bindings, &[], 0, false)
            .expect("register");
    }

    /// The click state of the machine page: the file's hooks and the exports `dk_param_get` and `dk_param_set` reach it here.
    pub fn clicks(&mut self) -> &mut Slots {
        &mut self.clicks
    }

    /// The click that waits for the UI pass, taken (the owner half of the file's tick; the file runs it outside, `Click::run`). Detach has dropped a click of an unloaded module.
    pub fn take_click<C: CoreApi>(&mut self, core: &C) -> Option<crate::slots::Click> {
        // SAFETY: reads the core's parameter rows
        unsafe { self.clicks.take_click(core, &self.reg.param_owner) }
    }

    /// The number of the first run-time machine type, for an owner made with `new` (`start` takes it from the image): the native pages are kept by their place in the range.
    pub fn set_first_reserved(&mut self, first: u32) {
        self.pages.set_first(first);
    }

    /// The native pages of the machines (the hooks of the service file look them up).
    pub fn pages(&self) -> &PageTable {
        &self.pages
    }

    /// The parking passes of absent numbers (read only).
    pub fn parking(&self) -> &Parking {
        &self.parking
    }

    /// One native page record of a consumer that came up (`Records::apply`): the walk's state and the last commit's callback arguments are the owner's.
    ///
    /// # Safety
    /// `body` holds the record's body (checked); `consumer` is the one `attach` filled the rows for.
    pub(crate) unsafe fn apply_native_page<C: CoreApi>(
        &mut self,
        core: &C,
        kind: u32,
        body: *const u8,
        consumer: &Consumer<'_>,
    ) -> i32 {
        self.applied.native_page(
            core,
            kind,
            body,
            consumer,
            self.builder.args(),
            &mut self.pages,
        )
    }

    /// The list builder's buffer size in bytes, 4 per listed type: the bytes of the patches `list_alloc` and `list_vector_end`.
    pub fn list_bytes(&self) -> u32 {
        self.listed * 4
    }

    /// The address of the end of the list buffer: the bytes of the patch `list_buffer_end`.
    pub fn list_end(&self) -> u32 {
        crate::list::list_address() + self.list_bytes()
    }

    /// The address of the list buffer: the bytes of the patch `list_buffer`.
    pub fn list_start(&self) -> u32 {
        crate::list::list_address()
    }

    /// What `attach` handed to the per-record `apply` (read only).
    pub fn applied(&self) -> &Applied {
        &self.applied
    }
}

impl Default for Machines {
    fn default() -> Machines {
        Machines::new()
    }
}

/// The result of an old-style check, apply or commit: 0 or a status code.
pub(crate) fn status(code: i32) -> Result<(), Status> {
    if code == OK {
        Ok(())
    } else {
        Err(code)
    }
}

/// The number of the first run-time machine type of the image's registry (the pages of the machines are kept by their place in that range).
///
/// # Safety
/// `core` gives the image tables.
unsafe fn first_reserved<C: CoreApi>(core: &C) -> u32 {
    let inf = dkrt_svc_rows::rows_info(core);
    if inf.is_null() {
        0
    } else {
        dkrt_abi::mem::iw(inf, dkrt_svc_rows::rows_info::w::M_DYN0)
    }
}

impl<C: CoreApi> Provider<C> for Machines {
    /// Not a start path: `init` points page-row strings into the state, so the state must not move after it is built, and `start` returns it by move. Every caller
    /// uses `start_in` (the file's `init`, the test driver); this refuses (`E_FORMAT`) so a move can never produce a state whose rows dangle.
    fn start(_core: &C) -> Result<Machines, Status> {
        Err(E_FORMAT)
    }

    /// The state is made where it lives: all-zero bytes are valid for every field, and the gate is closed after (`Builder::init`) and the click state is written to its start
    /// value (the detector and the held flow are not zero). The file's `init` (bind the registry words into `DKRT_MF_TAB`, the hooks read it; `E_NO_REGISTRY` when the image has no usable `core/rows`) is this function. This is a fill and a few words, and no copy of the 5 KiB collection through the stack.
    fn start_in(core: &C, slot: &mut MaybeUninit<Machines>) -> Result<(), Status> {
        // SAFETY: as in `start`
        status(unsafe { info::bind(core) })?;
        let listed = unsafe { info::build_list(core) }.ok_or(E_NO_REGISTRY)? as u32;
        let at = slot.as_mut_ptr();
        // SAFETY: every field of `Machines` is valid as zero bytes (integers, `None`, closed flags) once `init` has written the words that are not zero
        unsafe {
            at.write_bytes(0, 1);
            Builder::init(core::ptr::addr_of_mut!((*at).builder));
            core::ptr::addr_of_mut!((*at).clicks).write(Slots::new());
            (*at).listed = listed;
            (*at).pages.set_first(first_reserved(core));
            // no module has filled a row yet
            crate::pagerows::init(core, &mut (*at).reg);
        }
        Ok(())
    }

    /// A module's records are in: the reserved parameter, action, page and machine rows are filled (the native pages follow, record by record: `Records::apply`).
    fn attach(&mut self, core: &C, consumer: &Consumer<'_>) -> Result<(), Status> {
        // SAFETY: the registry words come from the image through the core
        let inf = unsafe { dkrt_svc_rows::rows_info(core) };
        if inf.is_null() {
            return if KINDS_MACHINES
                .iter()
                .all(|&(kind, _)| consumer.records(kind).next().is_none())
            {
                Ok(())
            } else {
                Err(E_NO_REGISTRY)
            };
        }
        // SAFETY: `consumer` holds a checked section; `inf` is the registry's table
        status(unsafe {
            self.applied.fill_rows(
                core,
                consumer,
                inf,
                self.builder.args(),
                &mut self.reg,
                &mut self.parking,
                &mut self.clicks,
            )
        })
    }

    // A machines module is torn down like any other (`can_stop` keeps its default): the loader's `DspPart::clear` (the voice row parked as a placeholder while the number is held)
    // and `detach` (the machine, page and parameter rows) are the teardown.
    fn detach(&mut self, core: &C, consumer: ConsumerId) {
        self.builder.end();
        // SAFETY: the rows are the registry's; `consumer` is a module record + 1
        unsafe {
            unload::remove(
                core,
                &mut self.reg,
                consumer.get(),
                &mut self.parking,
                &mut self.clicks,
                &mut self.pages,
            )
        }
    }

    /// The loader has put the firmware's sites back (the list patches and the rank hook restored) before `stop`: the machine selection view still caches the list it built with
    /// the patched list builder, so it is built again from the stock one. Nothing to do while the view does not exist.
    fn stop(&mut self, core: &C) {
        // SAFETY: the rows are the registry's; `refresh_view` reads the image's tables through the core and calls the firmware's builder
        unsafe {
            // no module is loaded: the descriptor strings of the rows were this file's buffers, which go with it
            let inf = dkrt_svc_rows::rows_info(core);
            if !inf.is_null() {
                registry::forget_descriptor_strings(core, inf);
            }
            selection_view::refresh_view(core)
        }
    }

    fn settle(&mut self, core: &C, s: &SettleCtx) {
        // a new plan of the numbers: the parking state of the earlier one is not carried over
        self.parking.plan_changed();
        // SAFETY: `s` is the loader's plan of the numbers
        unsafe { assign::plan(core, s) }
    }

    fn reload_done(&mut self, core: &C) {
        // SAFETY: the rows and the registry are this service's; the core gives the image tables
        unsafe {
            // the placeholder rows whose number the sticky map no longer holds
            dsp::free_placeholders(core);
            // the machine rows of a pinned number whose module did not come back (a MISSING row with no voice row would play the first stock machine's sound): gap rows. A
            // number the map holds stays MISSING (E5d), and `tick` parks its voice row if it has none.
            let inf = dkrt_svc_rows::rows_info(core);
            if !inf.is_null() {
                let mut k = 0;
                while k < registry::MAX_MACH {
                    if self.parking.missing_rows() & (1 << k) != 0
                        && registry::mach_owner(core, k) == 0
                    {
                        unload::reset_machine_row(core, &mut self.reg, inf, k, false);
                    }
                    k += 1;
                }
            }
            self.parking.reload_over();
            // the machine selection view caches the list: build it again, after a failed load too (the cached list would keep an entry the rows no longer have)
            selection_view::refresh_view(core);
        }
    }
}

impl<C: CoreApi> Ticker<C> for Machines {
    fn tick(&mut self, core: &C, phase: u32) {
        if phase == tick_phase::LAST {
            // SAFETY: the UI task runs the tick (the queued click is taken by the file's tick, outside the owner)
            unsafe {
                self.parking.park_absent(core, &mut self.reg);
            }
        }
    }
}

impl<C: CoreApi> Describe<C> for Machines {
    fn cap(&self, core: &C, kind: u8) -> Option<(u32, u32)> {
        // SAFETY: reads the image's tables through the core and the registry this crate owns
        unsafe {
            // an if chain, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
            if kind == CAPK_MACHINE_ROWS {
                Some(registry::counts(core, &self.reg)[0])
            } else if kind == CAPK_VOICE_ROWS {
                Some(dsp::rows_counts(core))
            } else if kind == CAPK_PARAM_ROWS {
                Some(registry::counts(core, &self.reg)[1])
            } else if kind == CAPK_PAGE_ROWS {
                Some(registry::counts(core, &self.reg)[2])
            } else if kind == CAPK_MAP_ENTRIES {
                Some((dkrt_sdk::sel::MAP_N as u32, assign::map_used()))
            } else {
                None
            }
        }
    }

    fn table_check(&self, _core: &C, used: u32, out: &mut TableFault) -> u32 {
        // SAFETY: reads the rows this crate owns
        unsafe { dsp::table_check(used, out) }
    }

    /// The five words of the MODULES reply for module record `who` - 1 (`modules::describe`); none for a module with no voice row, no machine type and no guard count.
    fn describe(core: &C, who: ConsumerId, out: &mut WordSink) {
        crate::modules::describe(core, who.get(), out)
    }
}
