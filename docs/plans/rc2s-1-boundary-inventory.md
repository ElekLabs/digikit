# RC2s-1 boundary inventory

Date: 2026-11-22. Input to step RC2s-1 of [resident-core-and-system-package.md](resident-core-and-system-package.md). Static read of the tree at `c3af7d3a1`; the call lists come from a read-only scan, so `check:layers` on the real tree is the proof. It measures more crossings than the design estimated: 28 hooks (the design said 4) and about 80-88 table functions (the design said about 65 plus the Platform methods); RC2s-1 groups the hooks and narrows the Platform view to the 17 methods package code uses.


Source: branch rc/rc2b-runtime-seams at c3af7d3a1 (lane fx-route-policy, read only). All paths below are under `runtime/core/src/rs/src/` unless they start with another folder. Line numbers are from that commit. Calls were found with a read-only scan of `crate::` paths, `use crate::{..}` imports and bare re-exported names (lib.rs has glob re-exports of `state`, `platform`, `api`, `health`, `imports`, `report`, `tick`), with `#[cfg(test)]` items removed. The scan does not see calls through a trait object (none exist) or through generic `P: Platform` (section 2.1 covers those).

Method note: a scan of this kind misses a call spelled in a way I did not think of. The layer rule of section 4 is the real proof; run it on the real tree first (it will list what this file missed).

## 0. Findings that change the design numbers

1. The design counts 4 hooks and about 65 table functions. The tree needs about 28 hook entries and about 88 table functions plus 8 data pointers and 39 import rows. Reasons: package code exports C-ABI symbols that resident image code or the menu call (`dkrt_scan`, `dkrt_live`, `dkrt_capacity`, `dkrt_fit`, `dkrt_module`, `dkrt_fx` ...), and resident code reads package state (watch, health, report).
2. The package uses only 17 of the 44 required `Platform` methods (section 2.1). A table of 17 is enough; the design's "45 Platform methods become table entries" is not needed.
3. `BOOT_IDX` (selection.rs:28) is only written and read by package code (loader/package.rs:108-110, lifecycle/rt.rs:153, core_api.rs:37). It moves to the package; no table entry.
4. There is no in-crate service: `gen_services.rs` has `STATIC = 0`. Every service is a `.dks` row via `svc_rows.rs`. So `handlers::services()` is purely a package function, and the resident callers of it (watch.rs, report.rs, tick.rs) need hooks.
5. The loader is already generic over three seams (`Platform`, `Images`, `Registry`: loader/seams.rs:23-50). Most package-to-resident calls already go through those traits; the work is to implement the traits over the table instead of over `crate::` functions.

## 1. Classification used (module to side)

Resident (R): platform.rs, dev.rs, boot.rs (record writers, level decision, `dkrt_boot`), selection.rs, safe_mode.rs, safe_screen.rs, fault.rs, watch.rs, storage/* (card, card_guard, store, store_writer, sdk_files, route, lock, layout, backends, storage_adapter), hash32.rs, sha256.rs, settings.rs, arena.rs, dlog.rs, state.rs, api.rs (shell), mailbox.rs, tick.rs, transport.rs, store_cmds.rs, admin_cmds.rs (INFO, SETTINGS, FAULT, RESTART), loader/image.rs, resident_query.rs (C export for image code; one hook), speed.rs, io.rs, worker.rs, aux_task.rs, intro.rs, health.rs (with one hook), report.rs `dkrt_status`/`dkrt_diag` (see 5.4).

Package (P): `lifecycle/*`, loader/{mod, discover, exports, install, live, package, port, records, role, seams}, reload.rs, settings_reload.rs, settings_overlay.rs, svc_rows.rs, svc_imports.rs, imports.rs, entry.rs, dsp/mod.rs, files.rs, scan.rs, admission.rs, collect.rs, registry.rs, link.rs, module_cmds.rs (MODULES, LOADS, host_register; SELECT moves to resident), core_api.rs, handlers.rs, gen_services.rs, invariants.rs, `dkrt-lifecycle`, `dkrt-fit`, `dkrt-link` crates.

The design table is silent on these; my proposal is in brackets, Em/architect to confirm: io.rs 912 lines, worker.rs 611, aux_task.rs 215, intro.rs 302, speed.rs [resident: platform tasks, called by upload and the store writer; module-facing entries become import rows]; dsp_stats.rs 284 [decision: package with one resident stub `dkrt_dsp_stats_feed`, or resident with 2 link hooks]; imports.rs/svc_imports.rs [package, plus import rows from the resident, section 3.4]; invariants.rs [package].

## 2. Calls across the line

### 2.1 Resident into package: these become the HOOKS

Grouped by caller. `n` is the hook entry that replaces it (section 3.2).

tick.rs `dkrt_tick` (one hook, `tick(phase)`, eight phase codes, order kept):
- tick.rs:42 `reload::poll`, :43 `settings_reload::poll` -> phase POLL_JOB
- tick.rs:48 `link::pump`, :49 `lifecycle::dsp_step` -> phase POLL_DSP
- tick.rs:15 via `phase(EARLY/LATE/LAST)` -> `services().tick(p)` -> phases EARLY, LATE, LAST (the span wrap `dlog::spanned` moves with it)
- tick.rs:53 `files::tick` -> phase FILES (between EARLY and LATE)
- tick.rs:85 `entry::tick_all` -> phase MODULES
- tick.rs:35 `resident_query::note_ui_task` is resident-internal once resident_query.rs is resident (no hook).
- tick.rs:30-60 mailbox: `dkrt_load` (api.rs:20 -> `loader::load`) -> hook `load(path)`. CMD_SELECT, CMD_RESTART, CMD_HEALTH, CMD_CARD_PROBE, settings stay resident.

boot.rs `dkrt_boot` (one hook, `boot(level, record)`):
- boot.rs:184 `loader::set_safe` (safe level: package gets level, loads nothing)
- boot.rs:202 `loader::find_providers`, :205 `lifecycle::boot`, :213 `loader::reset_plan`, :219 `loader::capture` (read_tables, boot.rs:212-224), `state.rs:149` `handlers::check_declarations` (init_once) -> all inside the package's boot
- constants only: boot.rs:338-372 `loader::LOG_NAME` (12), :416 `loader::PROVIDER_IDX` (0xffff_fffe): move to dkrt-abi.

api.rs (C exports that read package state; one hook each):
- api.rs:95 `entry::ctx_owner` -> `ctx_owner(ctx)`
- api.rs:231,233 `dsp::ds().mod_len[i]`, `entry::ctx_addr(i)` (in `dkrt_module`) -> `module_extra(i, out)` (dsp_size and ctx)
- api.rs:248 `handlers::exports::fx_entry` -> `fx_entry(index, out)`
- api.rs:208 `scan::module_header` -> `module_header(first, out)`
- exports defined in package files that outside code calls: scan.rs:161 `dkrt_scan`, :256 `dkrt_scan_reads`, :262 `dkrt_fit`; report.rs:183 `dkrt_capacity`; reload.rs:431 `dkrt_live`, :441 `dkrt_live_state`. Until the menu moves (RC2s-3) the resident keeps a stub symbol per export that calls the hook -> `scan`, `scan_reads`, `fit`, `capacity`, `live`, `live_state`.

transport.rs / admin_cmds.rs (Transfer):
- transport.rs:243 `module_cmds::begin_cmd`, :254 `end_cmd`, :328 `service(cmd)` -> hooks `command(cmd, seq, body, blen, out) -> i32` and `command_known(cmd) -> bool`. The command (table CMDS, transport.rs:286-310) split: resident handlers INFO, LIST, BEGIN, DATA, END, DELETE, MKDIR, RENAME, READ, VERIFY, SETTINGS, CLEAR, ABORT, FAULT, SELECT, RESTART (16); package handlers MODULES, CAPACITY, LOADS, RELOAD, RELOAD_STATE, LOAD, UNLOAD (7) plus module-registered commands (`dk_host_register`).
- admin_cmds.rs:198 `reload::cmd_queue`, :221 `reload::state`: inside the package half of the command table.
- admin_cmds.rs:41 `loader::role::image_fw_id(&RealPlatform)`: a pure function of `Platform::image_id`; move to dkrt-abi (no hook).

watch.rs / health.rs / report.rs (diagnostics):
- watch.rs:247 `services().diag`, :339 and :342 `services().attribute(site, off)`, :361 `entry::has_tick(i)` -> hooks `watch_diag(buf)`, `watch_attribute(site, off) -> u32`, `module_has_tick(i) -> bool`.
- health.rs:113,117 and report.rs:156-158 `dsp::area_bytes`, `dsp::area_stats`, `dsp::state` -> one hook `dsp_area(out)` (total, free, largest, state).
- report.rs:75 `module_cmds::machine_type_loaded` -> hook `machine_type_loaded(ty) -> bool`.
- report.rs:208 `loader::planned_dsp` and report.rs:211 `services()` are inside `dkrt_capacity`, which moves whole (hook `capacity`).
- report.rs:6 `handlers::services` also reads in `kind_value` (:166), inside `dkrt_capacity`.

settings.rs (broker; overlay and reload are package):
- settings.rs:387 and intro.rs:143 `files::in_init(owner)` -> hook `files_in_init(owner) -> bool`
- settings.rs:391 `settings_reload::note_opened` -> hook `settings_opened(rec)`
- settings.rs:439,468,490 `settings_overlay::get/put/remove` -> hooks `overlay_get`, `overlay_put`, `overlay_remove`

others:
- resident_query.rs:37-52 `loader::resident_query(provider, minimum)` -> hook `provider_query(provider, minimum) -> u32 (addr or negative code)`
- dsp_stats.rs:41,69 `link::lk().set_stats_ctl` (2 call sites): hooks `link_stats_ctl(slot, value)` if dsp_stats stays resident; none if it moves.
- platform.rs:316 `dsp::fn_addr`, storage/ (card.rs:405) `scan::last_part_is`: pure helpers; move to dkrt-abi (no hook).
- fault.rs:54 `dsp::SITE_*` is a comment only.

Hook entries: boot 1, tick 1, load 1, command 2, module_extra 1, ctx_owner 1, fx_entry 1, module_header 1, scan 3, capacity 1, live 2, dsp_area 1, machine_type_loaded 1, watch 3, files_in_init 1, settings_opened 1, overlay 3, provider_query 1, link_stats_ctl 1 = **28**. The design's `settings_rows(menu)` and `boot_report()` come in RC2s-3: 30 then.

Reduction options (decide in RC2s-1): (a) package publishes data pointers instead of functions for `module_extra` (the `DKRT_CTXS` table and `DspState`), which removes 3 hooks; (b) `scan/scan_reads/fit/capacity/live/live_state/module_header/fx_entry` collapse into one `api_call(id, a, b, c, d)` hook with a call-id enum (smaller table, weaker types). I recommend keeping typed entries; they are plain function pointers and the table size is small.

### 2.2 Package into resident: the RESIDENT TABLE

Package code reaches the resident through (i) the `Platform` trait for generic code, (ii) `crate::` functions, (iii) statics. Grouped by owner object; `n` is the function count of the group.

**platform** (n = 19). Generic code takes `P: Platform` (loader/mod.rs:130, lifecycle/session.rs:34, lifecycle/rt.rs:22, reload.rs:13,58). The package calls only: `alloc` (loader/mod.rs:156, scan.rs:26), `free` (loader/mod.rs:458), `blocks` (reload.rs:235), `irq_mask` (loader:145, svc_imports.rs:81, module_cmds.rs:199), `irq_restore`, `flush` (loader:179), `info` (loader:145, svc_imports.rs:151), `info_slot` (loader:431), `info_dsp_slot` (loader:420), `read_code` (loader:165), `write_code` (loader:166), `write_word` (loader:178), `play_state` (reload.rs:62), `device_id` (loader:202, svc_imports.rs:49), `read_all` (entry.rs:222), plus `stat` and `stat_head` (core_api.rs:68-73, through `CoreApi::storage_stat`). Free functions: `platform::fw_blocks` (reload.rs:562), `platform::profile()`/`device()` (scan.rs:65, registry.rs:12, lifecycle/rt.rs:451), `st()` (loader/mod.rs:19, scan.rs:198, invariants.rs:34), `init_once` (scan.rs:162). 17 trait methods + `fw_blocks` + `init_once` = 19 functions; `profile()` is a `DeviceProfile` copied once at package entry (data, not a call). The 27 other required methods (tasking 9, sample pool 3, staged writer 1, storage 4, `image_id`, `blocks`..., midi, track, soft_reset, hook install) stay resident only. All defaulted methods (`has_dsp`, `dsp_headroom` ...) derive from `profile()`, so the package computes them from its copy.

**boot_record** (n = 11): `note_boot_path` (lifecycle/rt.rs:117), `report_module` (rt.rs:240, reload.rs:526), `report_provider` (rt.rs:207, loader/discover.rs:224, loader/mod.rs:1010), `report_dsp` (rt.rs:390,398,421, loader/mod.rs:1110), `report_taken_down` (rt.rs:372), `begin_live` (reload.rs:516), `restore_live` (reload.rs:535), `unloaded` (reload.rs:531), `live_index` (reload.rs:464), `loads_view` (module_cmds.rs:106), `selection_set` (module_cmds.rs:152; SELECT moves resident, so this one goes away: n = 10). Data: `info()` and `states()` (rt.rs:215-236, reload.rs:35,525, loader/mod.rs:1213) are views over `DKRT_BOOT`: one data pointer `boot_info`; `BootStates` is built package-side from it (type in dkrt-abi/state.rs).

**log** (n = 14): `ev` (core_api.rs:115, loader/mod.rs:1008, invariants.rs:28, handlers.rs:37, link.rs:54), `current_source` (core_api.rs:118, entry.rs:112, files.rs:112, settings_overlay.rs:112, module_cmds.rs:302), `current_phase` (resident_query.rs:36; resident-internal if that file is resident), `enter_module`/`leave_module` (entry.rs:126,129, files.rs:567,569), `span_t0`/`span_end` (entry.rs:127,132), `call_init`/`call_fini` (loader/mod.rs:108,119), `release_sinks` (loader/mod.rs:951), `sink_drop` (loader/mod.rs:1184), `sink_on`, `sink_present` (link.rs:50), `log_at` (imports.rs:279). `dlog::spanned` (a generic closure helper) is rebuilt package-side from `span_t0`/`span_end`.

**store** (n = 17): `store::find` (loader/seams.rs:63), `load_to_stage`, `stage` (data pointer, :68), `list` (:71, scan.rs:164), `find_name` (scan.rs:251), `read_sector` (scan.rs:33); `store_writer::is_open`, `is_locked` (reload.rs:433,473), `retire` (loader/mod.rs:1778); `card::writable` (module_cmds.rs:117); SDK writer front end `storage::sdk_files::sdk_create, _write, _close, _abort, _delete, _commit, _pending` (entry.rs:231-255). If those seven only forward in `entry.rs`, they can be served as resident import rows (3.4) and cost no table entry; then n = 10.

**arena** (n = 3): `alloc` (loader/seams.rs:74, dsp/mod.rs:377, files.rs:166, imports.rs:273), `free`, `stats` (module_cmds.rs:41). Data: `DKRT_ARENA` base and length (invariants.rs:36) as one pointer pair.

**settings** (n = 6): `get`, `put` (core_api.rs:86,89), `module_client`, `module_get`, `module_put`, `module_remove` (entry.rs:183-195).

**selection** (n = 3): `selection_add` (core_api.rs:106), `legacy_state(out)` (reads `selection::REC`, core_api.rs:46), `legacy_state_clear` (calls `selection::update` with `REC`, `REC_COPY`, `REC_VALID`, core_api.rs:49-58).

**tasks** (n = 10): `worker::post`, `cancel` (core_api.rs:121,124; imports), `worker::retire` (loader/package.rs:496), `io::retire` (loader/mod.rs:1775), `io::current_task`, `intro::commit`, `intro::discard` (loader/mod.rs:939,947), `sha256` init/update/finish (loader/image.rs, port.rs:199, install.rs:390; or compile `sha256.rs` into both: it is 218 lines of pure code, and the design lists it as resident because stage 1 needs it).

**fault** (n = 1): `fault::panic` (svc_imports.rs:204 `dkc_panic`; also the package's `#[panic_handler]`, section 6).

**api** (n = 3): `api::notify_unload`, `api::poll_drop_range` (loader/mod.rs:1188,1191), `api::card_busy` (reload.rs:48,474). Data: `DKRT_MENU_OBJS`, `DKRT_MENUS`, `DKRT_PATHS`, `DKRT_MODULE_RUNTIME` (loader/records.rs:5,28-45, reload.rs:468,501, module_cmds.rs:76, scan.rs:202).

Totals: platform 19, boot_record 10, log 14, store 10-17, arena 3, settings 6, selection 3, tasks 10, fault 1, api 3, init/others 0 = **79 to 86 functions** (about 88 with the SELECT-less boot count as above; the range depends on the two choices marked). Data pointers (8): `boot_info`, `state` (`DKRT_STATE`), `arena`, `stage`, `paths`, `menus`, `menu_objs`, `module_runtime`.

Statics read or written directly across the line, and how each would be reached:
| Static | Owner today | Used by | How in the split |
| --- | --- | --- | --- |
| `DKRT_STATE` via `st()` | state.rs:66 | loader, scan, invariants, report, api | data pointer `state`; `st()` facade returns `&mut *table.state` |
| `DKRT_ARENA`, `DKRT_STAGE` | state.rs:25,27 | invariants.rs:36; storage | data pointers; storage side stays resident |
| `DKRT_PATHS`, `DKRT_MENUS`, `DKRT_MENU_OBJS`, `DKRT_MODULE_RUNTIME` | state.rs:69-88 | loader/records.rs, scan.rs, reload.rs, module_cmds.rs; api.rs (resident) | stay resident-owned (host tools peek the symbols); package gets data pointers |
| `BOOT_IDX` | selection.rs:28 | package only | moves into the package (a package static) |
| `REC`, `REC_VALID`, `REC_COPY` | selection.rs:19-21 | core_api.rs:46,49 | `legacy_state*` table entries |
| `DKRT_BOOT` | boot.rs:29 | via `boot::info()` | data pointer `boot_info` |
| `RBODY` | transport.rs:54 | module_cmds.rs:40,105 (reply body) | `command` hook gets `out` pointer as argument; no static |
| `DKRT_CTXS`, `DKRT_TICKS`, `DKRT_PROVIDERS`, `DKRT_DSP`, `DKRT_TABLE_BAD` | entry.rs:31,34; loader/records.rs:13; dsp/mod.rs:58; invariants.rs:20 | package, plus resident readers (api.rs:233, watch.rs:361) | package-owned; resident reads go through hooks `module_extra`, `module_has_tick` |
| `DKRT_REG`, `DKRT_ASSIGN`, `LOADER` | (`DKRT_REG`, `DKRT_ASSIGN` are in the machines service, not this crate; `LOADER` is loader/mod.rs:198) | package only | no crossing |

### 2.3 Calls that cannot cross as written

See section 6 for the list (closures, tuples, `Result`, slices, `DeviceProfile` by value, borrowed views).

## 3. Proposed Rust layout

### 3.1 Where things live (follow the layout rule: shared interface types in `runtime/core/abi`, the runtime crate keeps the implementation)

- `runtime/core/abi/src/resident.rs` (new): the `#[repr(C)]` types only: `ResidentTable`, the group structs, `PackageHooks`, version words, the level enum. `#![deny(improper_ctypes_definitions)]` already applies in that crate and enforces C-compatible signatures. Add `pub mod resident;` to `runtime/core/abi/src/lib.rs`.
- `runtime/core/src/rs/src/resident/` (new): `mod.rs` builds the one `static RESIDENT: ResidentTable`; one small file per group (`platform.rs` ... ) holding the `extern "C"` thunks that call the existing resident functions. Also `hooks.rs`: `static mut HOOKS: PackageHooks` (all null) and the typed callers the resident code uses (`hooks::tick(phase)`, `hooks::boot(..)`); a null hook returns the stock value.
- `runtime/core/src/rs/src/package/` (new, RC2s-1 moves the package files here): `mod.rs` with `pub unsafe fn entry(table: &ResidentTable, level: u32) -> *const PackageHooks`; `resident.rs` (the facade: one function per table entry with the original name and signature, so package code changes only its import path); `table_platform.rs` (`TablePlatform`, implements `Platform` over the group); `table_images.rs` (`TableImages: Images`). Moved folders: `lifecycle/`, `loader/` (minus the resident files), `dsp/`, and the package files of section 1.
- RC2s-2 then makes `package/` its own crate (`runtime/core/package`, same pattern as `runtime/core/lifecycle`); RC2s-1 leaves it a module so the object diff stays the indirection only.
- Pure helpers used by both sides move to dkrt-abi: `image_fw_id`, `fn_addr`, `last_part_is`, `pick_index`, `path_hash`, `shown_name`, constants `LOG_NAME`, `PROVIDER_IDX`, `REC_MAX`, and the types `Module`, `Ctx`, `FilesEventFn`, `BootStates`.

### 3.2 Table sketch (signatures use C types only; out-parameters in place of `Result`, `Option`, tuples)

```rust
// runtime/core/abi/src/resident.rs
pub const RESIDENT_MAJOR: u16 = 1;
pub const RESIDENT_MINOR: u16 = 0;      // a new function appended to a group: minor + 1; removed or changed: major + 1
pub const HOOKS_MAJOR: u16 = 1;
pub const HOOKS_MINOR: u16 = 0;

#[repr(C)]
pub struct Header { pub magic: u32, pub major: u16, pub minor: u16, pub size: u32 }   // size = bytes of the struct the producer built

#[repr(C)]
pub struct ResidentTable {
    pub header: Header,
    pub level: u32,                       // 0 normal, 1 safe, 2 core-only
    pub profile: *const DeviceProfile,    // copied by the package at entry
    pub views: Views,                     // the 8 data pointers
    pub platform: *const PlatformFns, pub boot_record: *const BootRecordFns, pub log: *const LogFns,
    pub store: *const StoreFns, pub arena: *const ArenaFns, pub settings: *const SettingsFns,
    pub selection: *const SelectionFns, pub tasks: *const TaskFns, pub fault: *const FaultFns, pub api: *const ApiFns,
    pub imports: *const ImportRows,       // 39 rows served by the resident (3.4)
}
// every group starts with `size: u32`; the package reads a field only when `size >= offset_of(field) + 4`, so a group grows by appending

#[repr(C)] pub struct PlatformFns {
    pub size: u32,
    pub alloc: unsafe extern "C" fn(u32) -> *mut u8,        pub free: unsafe extern "C" fn(*mut u8),
    pub blocks: unsafe extern "C" fn(*mut u32) -> i32,       // 0 = a clock, value in *out; else none
    pub irq_mask: extern "C" fn() -> u32,                    pub irq_restore: extern "C" fn(u32),
    pub flush: unsafe extern "C" fn(u32, u32),
    pub info: unsafe extern "C" fn(u32) -> *const u32,       pub info_slot: unsafe extern "C" fn(u32) -> *const u32,
    pub info_dsp_slot: unsafe extern "C" fn(u32, *mut u32) -> i32,
    pub read_code: unsafe extern "C" fn(u32, *mut u8, u32),  pub write_code: unsafe extern "C" fn(u32, *const u8, u32),
    pub write_word: unsafe extern "C" fn(u32, u32),
    pub play_state: unsafe extern "C" fn(*mut u32) -> i32,
    pub device_id: extern "C" fn() -> u32,
    pub read_all: unsafe extern "C" fn(u32, *const u8, u32, *mut u8, u32) -> i32,    // root, path, path_len, dst, cap: bytes or negative code
    pub stat: unsafe extern "C" fn(u32, *const u8, u32, *mut StatWire) -> i32,
    pub stat_head: unsafe extern "C" fn(u32, *const u8, u32, *mut StatWire, *mut u8) -> i32,
    pub fw_blocks: extern "C" fn() -> u32, pub init_once: unsafe extern "C" fn(),
}
#[repr(C)] pub struct BootRecordFns { pub size: u32,
    pub note_boot_path: unsafe extern "C" fn(u32, *const u8),
    pub report_module: unsafe extern "C" fn(u32, *const u8, i32, *const u8, *const u8),   // k, path, result, missing, name
    pub report_provider: unsafe extern "C" fn(*const u8, u32, i32, u32, u32),
    pub report_dsp: unsafe extern "C" fn(u32, i32), pub report_taken_down: unsafe extern "C" fn(u32, i32),
    pub begin_live: unsafe extern "C" fn(u32), pub restore_live: unsafe extern "C" fn(u32), pub unloaded: unsafe extern "C" fn(u32),
    pub live_index: unsafe extern "C" fn(*const u8, *const bool, *mut u32) -> i32,
    pub loads_view: unsafe extern "C" fn(*mut LoadsRaw), }
#[repr(C)] pub struct LogFns { pub size: u32,
    pub ev: unsafe extern "C" fn(u16, u16, *const u32),       // five words by pointer
    pub current_source: extern "C" fn() -> u8, pub current_phase: extern "C" fn() -> u8,
    pub enter_module: extern "C" fn(u32, u8) -> u16, pub leave_module: extern "C" fn(u16),
    pub span_t0: unsafe extern "C" fn(*mut u32) -> i32, pub span_end: unsafe extern "C" fn(u32, u32),
    pub call_init: unsafe extern "C" fn(u32, unsafe extern "C" fn(*const Ctx) -> i32, *const Ctx) -> i32,
    pub call_fini: unsafe extern "C" fn(u32, unsafe extern "C" fn(*const Ctx), *const Ctx),
    pub release_sinks: unsafe extern "C" fn(u32), pub sink_drop: unsafe extern "C" fn(u32, u32),
    pub sink_on: extern "C" fn(u32) -> bool, pub sink_present: extern "C" fn() -> bool,
    pub log_at: unsafe extern "C" fn(u32, *const u8, u32), }
// StoreFns { find, load_to_stage, list, find_name, read_sector, writer_is_open, writer_is_locked, writer_retire, card_writable, sdk_create .. sdk_pending }
// ArenaFns { alloc, free, stats }   SettingsFns { get, put, module_client, module_get, module_put, module_remove }
// SelectionFns { add, legacy_state, legacy_state_clear }   FaultFns { panic }   ApiFns { notify_unload, poll_drop_range, card_busy }
// TaskFns { worker_post, worker_cancel, worker_retire, io_retire, io_current_task, intro_commit, intro_discard, sha_init, sha_update, sha_finish }
// Views { boot_info: *mut DkrtBootInfo, state: *mut State, arena: *mut u8, arena_len: u32, stage: *mut u8, paths, menus, menu_objs, module_runtime }

#[repr(C)]
pub struct PackageHooks {
    pub header: Header,                   // null entries mean "not loaded": the resident keeps stock behaviour
    pub boot: Option<unsafe extern "C" fn(level: u32, rec: *const Record)>,
    pub tick: Option<unsafe extern "C" fn(phase: u32)>,                       // phases: POLL_JOB, POLL_DSP, EARLY, FILES, LATE, LAST, MODULES (+ 1 spare)
    pub load: Option<unsafe extern "C" fn(path: *const u8) -> i32>,
    pub command: Option<unsafe extern "C" fn(cmd: u8, seq: u32, body: *const u8, blen: u32, out: *mut u8) -> i32>,
    pub command_known: Option<unsafe extern "C" fn(cmd: u8) -> bool>,
    pub module_extra: Option<unsafe extern "C" fn(i: u32, out: *mut ModuleExtra)>,
    pub ctx_owner: Option<unsafe extern "C" fn(ctx: usize) -> i32>,
    pub fx_entry: Option<unsafe extern "C" fn(index: u32, out: *mut DkrtFx) -> i32>,
    pub module_header: Option<unsafe extern "C" fn(first: u32, out: *mut DkrtModuleHeader) -> i32>,
    pub scan: Option<unsafe extern "C" fn(out: *mut DkrtScan, max: u32) -> i32>,
    pub scan_reads: Option<unsafe extern "C" fn() -> u32>,
    pub fit: Option<unsafe extern "C" fn(/* as dkrt_fit */) -> i32>,
    pub capacity: Option<unsafe extern "C" fn(out: *mut DkrtCap, max: u32) -> u32>,
    pub live: Option<unsafe extern "C" fn(op: u32, path: *const u8) -> i32>,
    pub live_state: Option<unsafe extern "C" fn(out: *mut DkrtLive)>,
    pub dsp_area: Option<unsafe extern "C" fn(out: *mut DspArea)>,
    pub machine_type_loaded: Option<unsafe extern "C" fn(ty: u32) -> bool>,
    pub watch_diag: Option<unsafe extern "C" fn(buf: *mut u8)>,
    pub watch_attribute: Option<unsafe extern "C" fn(site: u32, off: u32) -> u32>,
    pub module_has_tick: Option<unsafe extern "C" fn(i: u32) -> bool>,
    pub files_in_init: Option<unsafe extern "C" fn(owner: u32) -> bool>,
    pub settings_opened: Option<unsafe extern "C" fn(rec: u32)>,
    pub overlay_get: Option<unsafe extern "C" fn(id: *const u8, id_len: u32, key: *const u8, key_len: u32, buf: *mut u8, cap: u32) -> i32>,
    pub overlay_put: Option<unsafe extern "C" fn(/* id, key, value, lens */) -> i32>,
    pub overlay_remove: Option<unsafe extern "C" fn(/* id, key, lens */) -> i32>,
    pub provider_query: Option<unsafe extern "C" fn(provider: u32, minimum: u32) -> u32>,
    pub link_stats_ctl: Option<unsafe extern "C" fn(slot: u32, value: u32)>,  // only if dsp_stats stays resident
}
```

Version words: `ResidentTable.header.{major, minor, size}` and `PackageHooks.header.{major, minor, size}`, plus a `group.size` word per group. Stage 1 compatibility (design section 3): the package header carries the table version it was built against; stage 1 accepts when `major ==` and `package_minor <= core_minor`. RC2s-1 constants live in `resident.rs` and are covered by one host test (a table built by `resident::build()` has every field non-null and `header.size == size_of`).

### 3.3 TablePlatform

`package/table_platform.rs` defines `struct TablePlatform;` and `impl Platform for TablePlatform` (and its supertraits `Tasking`, `SamplePool`, `StagedWriter`, `Storage`). The 17 used methods forward to the table; the 27 others are `unreachable!()`-free: they return the resident table's values where cheap, or the neutral value the fake uses, behind one doc line "not used by package code". Alternative (cleaner, recommended for RC2s-2): a narrower trait `PackagePlatform` with the 17 methods, change the generic bound `P: Platform` to `P: PackagePlatform` in loader/, lifecycle/, reload.rs (mechanical, `FakePlatform` implements both). Decide in RC2s-1.

### 3.4 Import rows

`imports.rs` `import_table!` (imports.rs:70-135) has 64 names: 39 point at resident functions (memcpy, memset, memcmp, `__muldi3`, `__udivdi3`, `__umoddi3`, dk_alloc/free/log/log_event/sink_set/span_sink_set/debug_log_sink_set, dk_time_blocks, dk_fw_*, dk_io_*, dk_helper_*, dk_job_*, dk_hash32_*, dk_intro_take, dk_dsp_stats_*, dk_debug_*, dk_device_track_count, dk_selected_track) and 25 at package functions (`entry::*` 17, `collect::*` 6, `imp_host_register`, ...). The `IMPORTS` static is built in a `const` from function items, which the package cannot name. Proposal: the resident exports `ImportRows` (a const array of `{name, addr}` for its 39), the package's `import_addr(name)` searches its own `IMPORTS` first, then `table.imports`, then `optional::table()`. `optional_imports!()` (imports.rs, linked by patchkit against the runtime's `optional` list in module.toml) stays in the resident object; `import_needs` too (resident exports it as a table function: +2 entries, or `optional::table()` pointer in `Views`).

## 4. Enforcing the boundary with check:layers in one binary

Add `check_runtime_boundary(root)` to `tools/devtools/layercheck.py` next to `check_runtime_arrows` (same `RT_SRC`, `rust_without_comments`, `RT_CRATE_REF`, `Violation`, `is_runtime_test_file`, `without_test_modules`), and add it to the list in `check()`. Rule name `boundary`.

```python
# Sides of the resident core / system package split (docs/plans/resident-core-and-system-package.md, RC2s-1).
RT_PACKAGE_DIRS = ("package",)                       # runtime/core/src/rs/src/package/**  (after the files move)
RT_PACKAGE_ROOT_FILES = ("lifecycle", "loader", "dsp")  # folders; loader/image.rs is resident (STAGE1_FILES)
RT_STAGE1_FILES = ("loader/image.rs", "loader/place.rs")  # resident, inside a package folder
# What each side may name of the other. Everything else is a violation.
RT_HOOK_FACADE = "hooks"          # resident -> package: only crate::hooks::* (hooks.rs), and `package::entry` in stage1/lib.rs
RT_RESIDENT_FACADE = "resident"   # package -> resident: only crate::resident::* (package/resident.rs facade over the table)
RT_SHARED = {"MAX_MODULES", "REC_MAX", "ARENA_BYTES", "cstr_len", "cstr_eq", "rd32", "wr32", "rd16", "memcpy"}  # the library both sides compile
RT_TABLE_FILES = ("resident/",)   # the resident side builds the table: may name package types only in abi (resident.rs)
```

Rules:
1. `boundary-hook`: a file of the resident side (every `.rs` under RT_SRC that is not package, not a test file) names `crate::M` where `M` is a package module (lifecycle, loader (except `loader::image`/`loader::place`), reload, settings_reload, settings_overlay, svc_rows, svc_imports, imports, entry, dsp, files, scan, admission, collect, registry, link, module_cmds, core_api, handlers, gen_services, invariants) -> "resident code calls package code". Instead: "add a hook in `PackageHooks` (abi/resident.rs) and call it through `crate::hooks`". Allowed in `lib.rs` (module list) and `stage1.rs`/`resident/mod.rs` (`package::entry`).
2. `boundary-table`: a file under `package/` (or a package folder) names `crate::M` where `M` is a resident module (platform, boot, selection, safe_mode, fault, watch, storage, settings, arena, dlog, state, api, tick, transport, store_cmds, admin_cmds, io, worker, aux_task, intro, sha256, hash32, health, report, speed, mailbox, resident_query, dev, store, store_writer, card, ...) and `M` is not `resident` or in `RT_SHARED` -> "package code calls resident code". Instead: "add an entry to the owner group of `ResidentTable` and call it through `crate::resident`".
3. Names: because lib.rs re-exports `state::*`, `platform::*`, `api::*`, `health::*`, `imports::*`, `report::*`, `tick::*` as `crate::NAME`, the check resolves bare `crate::NAME` through a map built from the `pub fn|static|const|struct|enum|type NAME` of those globbed files (as `RT_BARE_*` does for `services::`). Better: remove the glob re-exports first (RC2s-1 step 0); then `crate::NAME` is always a module path, and RT_SHARED is the only exempt list.
4. `use crate::{a, b::c}` statements are checked item by item (the same group split that `check_runtime_arrows` does with `RT_CRATE_REF` group 1), so `use crate::{boot, dlog}` followed by `boot::x()` in a package file is reported at the `use` line.
5. Test files (`is_runtime_test_file`, inline `#[cfg(test)] mod` blocks via `without_test_modules`) are exempt in both directions: host tests drive resident statics (`FakePlatform`, `RealPlatform`, `cleanup_tests`).
6. `boundary-abi`: a function-pointer field of `resident.rs` types is `extern "C"` and uses only integers, pointers, `bool`, `Option<fn>` and `#[repr(C)]` structs (the crate's `deny(improper_ctypes_definitions)` checks most; the rule adds: no `&`, no slice, no tuple, no `Result`).
7. Dependency check in the style of `check_service_crates`: when `package/` becomes a crate (RC2s-2), its Cargo.toml may depend only on dkrt-abi, dkrt-sdk, dkrt-fit, dkrt-lifecycle, dkrt-link, dkneeds; not on the runtime crate (`SERVICE_RUNTIME_REF` pattern) and not on dkfw (the `platform` rule already names only `platform.rs` and `fw.rs`).

The allowed crossings, as one list the rule file states: resident to package: `crate::hooks::*` and `package::entry` (one call, in the boot path); package to resident: `crate::resident::*` (facade), `RT_SHARED`, the pure types of dkrt-abi.

## 5. loader/mod.rs and reload.rs split by concept

### 5.1 loader/mod.rs (1,781 lines; test files loader/tests.rs 2,393, tests/assembly_tests.rs 2,147)

| New file | Contents (old line ranges in loader/mod.rs) | Side |
| --- | --- | --- |
| `mod.rs` (about 280) | consts and header words (25-51), `mod` list, `PROVIDER_IDX` (moves to abi), `ProvRec`, `Phase`, `Slot`, `Loader` struct (130-188), TORN_*, `Real`, `loader()`, `real()` and the thin free-function wrappers (196-360, 380-413) | P |
| `check.rs` | `Checked`, `Placed`, `sel_of`, `port_keys`, `port_lookup`, `check_file`, `check_head`, `resolve_imports`, `check_body` (415-452, 455-520, 541-715) | P |
| `load.rs` | `load`, `load_file`, `load_inner`, `claim_record`, `complete`, `init_and_commit`, `go_live`, `fail`, `cannot_retire` (524-540, 716-960) | P |
| `place.rs` (new) | `place_image`'s copy-relocate-flush core (828-872) as a free function over `(staged, block, pass, &impl Platform)`: the piece stage 1 needs | **R, stage 1** (with `image.rs`) |
| `commit.rs` | `commit`, `commit_services`, `dsp_advance`, `dsp_loaded`, `provided_hash` (968-1121) | P |
| `unload.rs` | `uninstall_cf`, `uninstall_dsp`, `stop_tick_once`, `quiesce_service`, `unload_body`, `drop_tick_cb`, `retire_sdk` (1123-1218, 1742-1781) | P |
| `live.rs` (exists, 130 lines) | takes `reload_check*`, `reload_quiesce`, `reload_abort`, `reload_prepare`, `live_check_load*`, `live_prepare`, `retire_index`, `number_keys`, `live_caps`, `own_use`, `live_capacity` (236-355, 1248-1520) | P |
| `plan.rs` (new) | `capture`, `keys_of`, `reset_plan`, `find_providers`, `admit_package`, `number_boot_keys`, `settle_ctx`, `live_settle_ctx`, `table_span`, `table_of_file`, `table_keys`, `service_import_owner`, `parse_provs`, `unmet_needs` (354-410, 1504-1740); the providers search stays in `discover.rs` | P |
| `image.rs` (exists, 347) | head, bounds and relocation checks | **R, stage 1** |
| `install.rs`, `role.rs`, `exports.rs`, `port.rs`, `package.rs`, `records.rs`, `seams.rs`, `discover.rs` | as today (install.rs 634 lines is the hook installer; role.rs 624) | P |

Test files follow the concept: `loader/tests.rs` splits into `tests/check.rs`, `tests/load.rs`, `tests/commit.rs`, `tests/unload.rs`, `tests/live.rs`, `tests/plan.rs`; `assembly_tests.rs` stays by seam (already named). The split is mechanical (one `impl Loader` block per file; fields stay `pub(super)` in `mod.rs`). Resident stage 1 (about 2-3 KB per the design) is `image.rs` + `place.rs` + `store::find/stage` + `sha256` + `arena`; `place.rs` is the only new code, and it needs `place_image` to stop reaching `self.images`/`self.p` (it already takes `blk` and `pass`; the Platform use is `flush`, `read_code`).

### 5.2 reload.rs (1,372 lines; production code lines 1-566, tests 567-1372)

| New file | Contents | Side |
| --- | --- | --- |
| `reload/mod.rs` | `PHASE`, `phase()`, `set_phase`, `note_reloaded` (21-40); re-exports | P |
| `reload/refusal.rs` | `DRAIN_BLOCKS`, `drained`, `Facts`, `refusal` (7-79) | P |
| `reload/job.rs` | `Ops` trait, `step`, `Job`, `queue`, `poll`, `check`, `end` (81-400) | P |
| `reload/real.rs` | `JOB`, `job()`, `cmd_queue`, `State`, `state()`, `dkrt_live`, `dkrt_live_state`, `poll`, `RealOps` (402-566) | P |
| `reload/tests.rs` (+ `tests/` by operation: load, unload, reload, refusals) | 567-1372 | P |

`RealOps` (reload.rs:456-565) is where reload.rs calls the resident (`boot::*`, `store_writer::*`, `platform::fw_blocks`, `api::card_busy`, `DKRT_MENU_OBJS`): after RC2s-1 it calls `crate::resident::*`.

### 5.3 Other files that need a split before they move

- `boot.rs` (785): keep resident the record writers and `dkrt_boot`'s level decision; its tail `read_tables` (212-224) moves into the package boot. Pure helpers (`pick_index`, `path_hash`, `shown_name`, `live_index`) move to dkrt-abi.
- `admin_cmds.rs` (258): INFO, SETTINGS, FAULT, RESTART resident; CAPACITY (`h_capacity`) and RELOAD/RELOAD_STATE/LOAD/UNLOAD (lines about 190-258) package (`package/module_admin.rs`).
- `module_cmds.rs` (414): `cmd_select`, `h_select`, `SELBUF` (113-170) to resident (`store_cmds.rs` or `selection.rs`); the rest P.
- `transport.rs` (331): the command table `CMDS` (286-310) becomes two tables: the resident one (16) and the `command` hook for the rest.
- `report.rs` (383): `dkrt_status` (131) mixes arena/DSP; `dkrt_capacity` (183) and `kind_value` (166) go to the package; `dkrt_diag` (245) needs a look (`services()` at 211 is `dkrt_capacity`'s).
- `api.rs` (333): resident, but `dkrt_load`, `dkrt_module`, `dkrt_fx`, `dkrt_module_header` become thin hook callers.
- `state.rs` (152): `init_once` (141) calls `handlers::check_declarations` (149): move that call into the package boot.

## 6. Risks and unknowns

1. **Types that cannot cross a C ABI.** `Result<Found, i32>` (`store::find`, `store::load_to_stage`), `Option<u32>` (`blocks`, `play_state`, `info_dsp_slot`, `track_type`), tuples (`Option<(usize, usize)>`, `selected_sound`), slices (`&[u8]` in `read_code`, `write_code`, `read_all`, `stat`, `selection_add`, settings get/put), `[u32; 5]` by value (`dlog::ev`), `[u8; 8]` return (`image_id`), `DeviceProfile` by value (big), `BootStates<'static>` and `&'static mut State` returns (`st()`, `boot::info()`), `Stat` structs (`StatWire` already exists for this, abi/storage.rs:39). Each needs an out-pointer shape (section 3.2). The abi crate's `deny(improper_ctypes_definitions)` will flag most.
2. **Generic functions and closures.** `dlog::spanned<R>(owner, f: impl FnOnce)` (dlog.rs:363; used in tick.rs:15,26) cannot cross: split into `span_t0` and `span_end` (both exist, dlog.rs:328,352). `Loader<P, M, R>`, `Rt<P, M, R>`, `Session`, `reload::refusal<P>`, `files::list_with<P>`, `scan` helpers are generic over `P: Platform`; they monomorphize inside the package with `TablePlatform`, which is fine, but `Real` aliases (`loader/mod.rs:196`, `lifecycle/rt.rs:458`) and `static LOADER: Real` (loader/mod.rs:198) change type: the package's `static` is then sized by `TablePlatform` (a ZST) and `RealImages`/`RealRegistry` (ZSTs), so the layout does not change. `setup_args_ok(.., &machine_type_loaded, ..)` (report.rs:75) takes a function reference: becomes the hook.
3. **The `Platform` trait.** 44 required methods, 17 used by the package. A full forwarding impl wastes code (each forwarder is a thunk plus a table slot); the narrow trait `PackagePlatform` is the cleaner cut but touches every generic bound. `FakePlatform` (fake_platform.rs, 851 lines) must keep implementing the full trait for resident host tests.
4. **Panics.** `panic = "abort"` and `#[panic_handler] fn panic -> fault::panic()` (lib.rs, `not(test)`). A separately linked package needs its own handler that calls `table.fault.panic`, and it will carry the compiler's bounds-check and `unwrap` panic paths (no `core::fmt`; the runtime's rule). In RC2s-1 one binary: unchanged. Note for RC2s-2: the package object must not define `rust_begin_unwind` twice at link.
5. **Symbols that tools read by name.** 16 `#[no_mangle] static` objects belong to package files (`DKRT_CTXS`, `DKRT_TICKS`, `DKRT_PROVIDERS`, `DKRT_DSP`, `DKRT_TABLE_BAD`, loader/records.rs:13-14 and others); dk-session, the device tests, `patchkit size` groups and the emulator daemon peek by symbol. In RC2s-1 (one object) nothing moves; in RC2s-2 their addresses leave the runtime symbol table. Keep the state.rs statics resident-owned (my proposal) so most peeks stay valid; list the rest for RC2s-2.
6. **`IMPORTS` and `optional`.** `IMPORTS` is a `const` array of function addresses (imports.rs:70-135); `optional_imports!()` links names to patchkit-provided symbols of other image modules (core/ui, core/input). The package cannot hold those addresses at build time. Plan: import rows exported by the resident (3.4). Risk: the `const _: DkAllocFn = imp_alloc` type checks (imports.rs:137-205) assert each import's type against sdk.toml: they must move to the side that holds the function, or the rows file keeps them.
7. **"Runtime object code unchanged except the indirection" (design RC2s-1 proof).** About 80 table calls add a thunk and a pointer load each, and the hooks add 28 call sites; `patchkit objdiff` will show it. Stack depth: the manager's worst boot chain is about 24 KB (RC2b-2s); each hook or table call adds a frame; check the boot frame (6,740 B) after the change. The resident-table statics sit in `.rodata`; runtime objects already have relocated fn-pointer tables (CMDS, IMPORTS), so patchkit handles them, but a fn-pointer table in a separately relocated package (RC2s-2) needs stage 1 to relocate it (the design says stage 1 relocates the package).
8. **ColdFire constraints.** No jump tables (the repo uses `if` chains: tick.rs:50, `admin_cmds`); the new group readers and `command` dispatch must follow. Struct returns by value from `extern "C"` on rustc_codegen_gcc for m5441x are the most likely miscompile; the out-pointer shapes avoid them.
9. **Tests that cross the line.** `cleanup_tests.rs` (853), `contract_tests.rs`, `spans_tests.rs`, `store_tests.rs`, `loader/tests*` and the `handlers/` fixtures drive resident statics next to package code. The rule exempts them. Some host tests build `Real` loaders with `RealPlatform`; they must keep working with the facade over a table that points at the real functions (`resident::build()` is the same in host tests).
10. **Name clashes and order of moves.** Moving files into `package/` changes every `crate::loader::` path in tests (about 74 refs in loader/tests.rs); do the move as one mechanical commit before the facade, so a diff of code vs paths stays readable. `check:layers` currently allows `core_api` as the one core module a service names (arrows rule): `core_api.rs` is a package file, so the services-to-core arrow now points into the package; the services' tests that name core modules stay exempt.
11. **Items the design leaves open (listed in section 1):** io/worker/aux/intro, dsp_stats, imports. Also: where `Module`, `State`, `Ctx` types live (proposal: abi); whether `sha256` is a table group or a library compiled into both (design says both resident and library: the table entry is smaller than 218 lines of code in each binary only if stage 1 is the sole user).
12. **Estimate for RC2s-1.** The scan above lists the calls; the table and hooks are mechanical, the facade is about 80 one-line functions, the file moves and splits are the bulk. I did not measure sizes (no `patchkit size`); the design's 108 KB / 167 KB figures are the only numbers.
