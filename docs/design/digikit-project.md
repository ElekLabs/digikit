# digikit project: settings overlay, reload rule, service and `.dkp` format

Status: step 3, 4 and 6 built on `svc/project` (host proofs). Words and paths: [taxonomy](taxonomy.md). Store: [digikit-store](digikit-store.md). Hook sites: [finding 110](../findings/110-project-identity.md) section 4.

## 1. Modules stay unaware of projects

No module changes, no new module API, no project event. A module calls `dk_settings_open`, `dk_settings_get`, `dk_settings_put` and `dk_settings_remove` as before. While the `project` service is loaded the runtime puts a **project overlay** over the module's settings handle; without the service nothing changes.

The runtime offers a settings overlay: a layer over module-owned settings keys that one service provides (`modules/core/runtime/src/rs/src/settings_overlay.rs`, called from `settings::module_get`, `module_put` and `module_remove`). The runtime knows nothing of projects; the project service is the overlay's only user. The reload of modules whose settings changed is as generic (`settings_reload.rs`). It touches module-owned keys only: the key of a module handle is always `<module id>/<key>`. The runtime's own keys (effect routes and sends, the selection, sticky numbers) go through `settings::get` and `settings::put` and stay device scope; a module cannot name them.

| Call | With the service loaded |
| --- | --- |
| `dk_settings_get` | The current project's value if its `.dkp` holds the key, else the settings record's value (`DK_E_SET_NOKEY` if neither). A project value that does not fit `cap` is `DK_E_SET_VALUE`. |
| `dk_settings_put` | Three steps in this order. First the record is asked whether it can take the change (key valid, value at most 1024 bytes, room after it replaces the key's entry, settings on); if not, its `DK_E_SET_KEY`, `DK_E_SET_VALUE`, `DK_E_SET_FULL` or `DK_E_SET_OFF` is the result and nothing is written. Then the current project's image takes the value (written to the `.dkp` at the next successful stock save); `DK_E_SET_OFF` from the service (no current project) means it has nothing to layer and counts as OK, every other negative answer is the result and the record stays as it was. Last the record takes the value (the last used one, so a new project starts from it); it cannot refuse for the reasons of step one. A module that gets an error reads back what it read before. |
| `dk_settings_remove` | The same order: the record is asked (key valid, settings on), then the key leaves the current project (`DK_E_SET_OFF` is OK, another negative answer is the result and the record keeps the key), then it leaves the record. |

- Key 1-47 printable bytes including `<id>/`, value at most 1024 bytes, as the record. A `put` of the value the project holds changes nothing and sets no dirty flag. No current project (slot -1 or 0x80): the service answers `get` with `DK_E_SET_NOKEY` and `put` and `remove` with `DK_E_SET_OFF`, which the overlay treats as "the record decides".
- A load without a save discards the project image's changes; the record keeps them.
- The service gives the runtime one table, `SettingsOverlay { version, get, put, remove }` (`dkrt_abi::service`), with `dkc_settings_overlay_set` in its `init`; the runtime holds one provider's table and drops it when the service's record goes away. The service knows owner-prefixed entries and nothing of modules.

### Reload rule

A project change (LOADED, an unload to no project, a clear of the current project) installs a new image. In the same step the service compares the old and the new image for every key of either (`BoundProject::install` in `engine/bound.rs`). What a module reads under a key is the image's value, else the record's value, else nothing. A module differs when any of its keys reads another value with the new image current, or reads a value in one image and none in the other. The service calls `dkc_settings_changed(owner)` once for each such module id. The runtime marks the module if it opened its settings (`dk_settings_open` in `init`) and is loaded, and reloads the marked modules one at a time with the E5 live job (`reload::dkrt_live(LIVE_RELOAD, path)` from `ReloadQueue::poll` in `settings_reload.rs`, once per UI pass). A change that names the module while its reload runs is kept and gets its own reload (a generation count per module). The module's `init` then reads the new values through the overlay. A reload refused because the transport plays waits until it stops (load on stop) and counts for no try. A paused or unreadable transport counts as playing. The wait holds the queue: one reload job runs at a time, and a reload waiting for the transport blocks the ones behind it. That costs nothing, because every reload is refused while the transport plays, so none of the others could run either. A reload the job refuses for another reason (an open upload, a menu of the module, safe mode) or that fails is tried again after about a second, three times; then it is dropped (the module keeps running with its old values until the next restart). A module that did not open its settings, or is not loaded, is never marked.

A new project starts from the record, which holds the last used values (every put wrote through), so for the usual case, a project without a `.dkp`, nothing differs and nothing reloads.

## 2. Events

None for modules. The service writes `PROJECT_LOADED`, `PROJECT_SAVED` and `PROJECT_MODULE` to the runtime log (section 6); the runtime reloads the modules whose settings changed (section 1). `dk_project_current` and `dk_project_status` (section 3) are for the tool module `digikit/project-manager`; a module that only keeps settings calls neither.

## 3. The `project` service

`module.toml`, as `modules/core/services/lfo/dks/module.toml:1-17,80-93`:

```toml
[module]
id = "core/services/project"
role = "service"
service = "project"
type = "raw"

[[hook]]
name = "cf.project.hook.slot_set"
entry = "project_hook_slot"
[[hook]]
name = "cf.project.hook.save_exit"
entry = "project_hook_save"
[[hook]]
name = "cf.project.hook.clear_one_exit"
entry = "project_hook_clear_one"
[[hook]]
name = "cf.project.hook.clear_many_exit"
entry = "project_hook_clear_many"
[[hook]]
name = "cf.project.hook.paste_exit"
entry = "project_hook_paste"
```

- Five hooks (`devices/fw/project.toml`, mode `replace` with `tail`, users `core/services/project`): the slot setter entry, and the exits of the save, of the project clear (single and multi-slot callbacks) and of the paste. The paste and the clears are jobs of the firmware's BgWorker thread that finish their card write and listing refresh inside the job, and the service tick (UI task) can run before the job ends; a hook at the entry could read the slot record before it is final (old name and stamp, or zeroed by the refresh), so a paste would find no source file and lose its digikit data. The three hooks therefore sit before the rts of their functions [V] (finding 110 section 5; that the jobs run on the BgWorker thread stays [D]). Both devices have the setter, the save exit and the two clears (same strict hash; setter DT2 `0x400c11ca`, DN2 `0x400bb7fa`; save exit `0x40043b1e` / `0x4004027e`; clear one exit `0x400a1368` / `0x4009b4de` (function + 0x4a); clear many exit `0x400a13e6` / `0x4009b55c` (function + 0x70)). The paste exit (function + 0x11c: DT2 `0x400f55aa`, DN2 1.12 `0x400f6f86`, DN2 1.11 `0x400f6dba`) is in a function with another strict hash on each device (the project size is an immediate), so it is found from the `jsr` at +0x16 of the paste job callback, whose strict hash is the same on both (`cf_call` rule with `offset`, below) [D]. No delete or format hook: a file that no longer belongs to its slot is archived at the next write to that slot (section 5). `cf.project.fn.mmcfs` (DT2 `0x4019dbda`, DN2 `0x4018e0a8`, the getter of the card file system singleton, found by strict hash) gives `MmcFs`; the service reads the slot record in it directly (`+0x10 + slot * 0x1c`: byte 0 nonzero when the slot exists, name at +8, stamp at +0x18) [V]. The `ProjectCache` singleton is a separate 0x20-byte object and holds no records.
  - `slot_set`: the setter, 6 bytes displaced, slot at SP+4 (finding 110 section 4, "Slot setter").
  - `save_exit`: before the rts, inputs D0 and D3, keeps D2-D4/A2-A6 (finding 110 section 4, "Save exit").
- The routines are one `hooks.s` (the displaced bytes and the stacks are the same on both devices; port step 3 moves them). They store only to the service's static `Pending` (`engine/pending.rs`: the setter's argument and a flag, the slot of a successful save and a flag, one bit per cleared slot and per pasted slot). `save_exit` records only when D0.b is nonzero and D3 is at most 127; `paste_exit` records the destination slot D6 only when D0.b is nonzero; the clear exits always record (a failed clear then archives a `.dkp` that is still needed, which is recoverable; the opposite is not). The clear-many exit re-reads the vector from the job at 0x38(SP). They save the registers and the flags they use (strict liveness proves none of them dead, except D1 and the flags at the setter).
- `service!` takes `tick = path` (the service has `tick = glue::tick`): the runtime calls it once per UI pass with the module context; `ModuleDef` has the `tick` field (`dkrt-svc-rt`).
- Tick, in order: (1) a job that ended; (2) the next piece of work, first the files of cleared and pasted slots, then a save, then a load: `dk_files_list` of `projects/` with the slot's prefix, `dk_files_read` of the candidates (a load only while `dkc_live_op()` is 0: the read stage is shared with the loader), then the install (section 1: compare, name the modules whose settings changed, log LOADED); (3) a save freezes the image (16-byte aligned) and posts the write as `dk_io_post` jobs; (4) the retry timer, (5) the module restore (`ModuleRestore::tick`, section "Module list and restore").
- I/O step (`files` from `dk_files_open` in init, `files.h:1-3`): `dk_files_create(files, DK_FILES_STORE, path, used, NULL, DK_FILE_REPLACE)`, one `dk_files_write`, `dk_files_close`; for a rename `dk_files_delete` of the old name. The old file stays whole until close (`files.h:53-56`). The runtime gives a service file these calls as it gives them to a content module: the owner of its listing client (init scope only), I/O jobs and tick is its loader record, which follows the module records, and all of them end when the record is unloaded or its `init` fails.
- `DK_E_BUSY` or `DK_E_FILE_IN_USE`: return `DK_IO_MORE`, rerun after 1 ms (`files.h:64-66`); after 1000 reruns the step fails. The tick retries every second. After 30 s: one `PROJECT_DEFERRED`, image stays dirty, the next save writes. `E_INUSE`, `E_NOSTORE`, `E_FULL`, `E_WRITE`, `E_HASH` end the attempt with no retry loop; `PROJECT_SAVED` carries the code.
- Exports for `digikit/project-manager` (`provided = true`, `service = "project"`, `absent = "missing"`, as `sdk/sdk.toml:636-647`): `i32 dk_project_current(u32 *slot, u8 name[16])` (`DK_E_NOT_FOUND` without project); `u32 dk_project_status(void)` (bit 0 file loaded, 1 dirty, 2 store deferred, 3 file ignored). Bit 2 is set when the store stayed busy for 30 s and the work gave up (a listing, a read or a write), and stays set until the next write succeeds; bit 3 is set while the bound project's own file was refused for a file reason (`PROJECT_IGNORED` reasons 1 to 4), not for reasons 5 and 6. A list call is later.
- Overlay forwarding: see section 1. The service gives the runtime the `SettingsOverlay` table with `dkc_settings_overlay_set` in `init` and names modules whose settings changed with `dkc_settings_changed(owner)` (both in `dkrt_abi::core_imports`; `SVC_ABI` covers them). The runtime's names are generic: it offers a settings overlay and a reload of modules whose settings changed, and the project service is the only user. The runtime clears the table when the service record unloads, so no service `fini` is needed.
- Anchor for the paste: `cf_call` of `tools/patchkit/port_resolve.py` (`hash`, `ordinal`, `call` = bytes from the caller's entry to a `jsr (xxx).l` or `jsr d16(pc)`, `offset`, `len`): the code anchor on the function that call reaches. `offset` is added to the resolved callee (the paste exit is at +0x11c). It resolves DT2 1.17, DN2 1.12 and 1.11 without a per-device rule, and a later firmware whose callback keeps its hash.

### Lifecycle

The service is `ProjectService` (`engine/mod.rs`). Its tick reads in three steps: take the hook records, finish a job that ended, advance the stage. Each part owns its state and does its own work:

| State | Owner | Meaning |
| --- | --- | --- |
| no project | `BoundProject` (`bound.rs`), `Binding::None` | The scope is off: `get` is `E_SET_NOKEY`, `put` and `remove` are `E_SET_OFF`. The static is all zero, so it sits in `.bss`. |
| load failed | `BoundProject`, `Binding::LoadFailed { slot, retry_at }` | The store refused the load of `slot`, at the listing or at the read of the winner's file (a refusal that is no busy answer, or 30 s of busy). The scope is off as for no project, and the image is empty. The firmware stays on that slot and calls the setter no more, so a stock save into `slot` must not be taken for a save of an empty project: the queue skips that save (no write, no archive, the slot's files stay as they are), logs IGNORED with reason 6 and loads the slot again instead, so the slot's file is never archived or replaced from here. After such a save the firmware's stamp of the slot changed, so the load that follows reports a stamp mismatch (reason 4) and leaves the file in place (the mismatch rule); the data stays on the card. A load that failed is also tried again quietly every 30 s (`LOAD_RETRY_MS`) while the stage is idle and no live operation stages; a refusal of a slot that failed before is not logged or shown again. A setter call (any slot) or a save into the slot loads again; a setter call to another slot leaves the state until its load ends (`Bound`, or `LoadFailed` of that slot). |
| bound | `BoundProject`, `Binding::Bound(slot)` | A slot is current. It holds the live image, the live mirror's stamp at the bind, the source file, the change counter, `dirty` and `ignored`. It binds, unbinds (`leave` for the setter with -1 or 0x80), freezes the image for a write (`freeze_into`) and marks itself written. A plan changes it only through an `Effect`. A save into another slot takes the image with it: the slot has no file yet and the image counts as changed (`BoundView::adopted_for_save`, the one rule the planner reads and `apply` installs). |
| first bind | `BoundProject` (`StartBind`) | Whether the project that was current at the start is named yet (see Start). |
| queued work | `WorkQueue` (`queue.rs`) | The slot to load, the slot saved, and one bit per cleared and per pasted slot. `next()` gives one `Work` in this order: Clear, Paste, Save, Load, Leave. A save waits up to 2 s for the firmware's record of its slot. |
| listing | `Stage::Listing`, `SlotScan` (`scan.rs`) | The store folder is polled for the files of one slot. It ends in a `ScanResult`: the slot's files with their checks, and for a paste the source file. |
| loading, saving, clearing, pasting | `Stage::Running`, `TaskRun` (`task.rs`) | The pure planner (`plan.rs`) turned the scan result and a copy of the bound project's facts into a list of file operations (`Task`) and an `Effect`. The run does one task at a time (each prepares its own job) and logs SAVED at its end. |
| writer idle, posted, ended | `StoreWriter` (`writer.rs`), `JobPhase` | One job is posted at a time; the I/O task runs `io_step`, then `io_done` hands the status back. The writer only writes the file buffer. |
| file buffer | `FileBuffer` (`buffer.rs`) | The one 16 KiB buffer: scratch for a probe, a load and an archive or copy read, and the bytes a job writes. The UI task lends it only while no job holds it (a debug assertion checks that). |
| busy, retry, deferred | `BusyClock` (`clock.rs`) | A busy store holds the tick for the retry time (1 s); after 30 s of busy store it logs one DEFERRED and gives up, and the image stays dirty. The deferred state ends with the next successful write. |
| module list, restore | `BoundProject` (the list in the image), `ModuleRestore` (`restore.rs`) | The list is tag 1 of the live image. `ModuleRestore` owns the pending list, the entry it is at and its `Step`: `Next` (the zero state: the file of the entry is not found yet), `Queue` (the file is found: the live load is asked next), `Running` (the live job of the entry is queued or running), `Waiting { until }` (the transport played or the card was busy: the load is asked again from that time). It starts nothing in the tick that sees a new project. See "Module list and restore". |

A scan that the store refuses at `list_start`, that fails half way (`ListPoll::Err`) or whose read of a file answers busy (a busy read is never "no file") with a busy status (`E_BUSY`, `E_FILE_IN_USE`) goes back to the queue and starts again after the retry time, for up to 30 s. One rule for every busy answer, of a listing, a read or a write: after 30 s the work gives up, and the deferral logs one DEFERRED event (slot, status, seconds), once until a write succeeds (`BusyClock`). The read of the winner's file at the bind is a second read after the listing's: its busy answer takes the same retry path, and any other refusal ends the load like a refused listing (`Applied::Refused`). Only `E_NOT_FOUND`, or a file that fails the checks or has another header than the listing saw (it changed or vanished between the two reads), binds the slot empty without an event. Any other status is an answer, not a deferral: it logs IGNORED with reason 5 (slot, 5, status), resets the busy time (`answered`) and ends the work. A load that ends either way leaves the project `LoadFailed` and warns (`Env::toast`, reason 5: `PROJECT DATA NOT READ`, once per failure, not per retry); a clear or a paste is dropped. A scan of a paste with no record of its slot is dropped.

Two rules of the tick: the DEFERRED slot argument is the slot of a save, and 0 for a run of other work (a refused scan names its own slot); and while the stage is idle the tick starts no work before the retry time of a piece of work that was put back, and it does not reset the busy time on the way, so the 30 s count runs over the retries.

Start: the stock setter is not called for a project that was already current when the service starts. Until a setter call or this read names the current project, each tick reads the firmware's slot byte (`cf.project.value.slot_byte`, the absolute operand of the setter's first instruction, anchor kind `cf_literal`; the address is checked on DT2 1.17, DN2 1.12 and 1.11, and the byte holds the last persisted slot [D], finding 110) and, when the firmware's list holds the record of that slot, queues a load. Without it the first save would look like a save-as and archive the project's file.

Archive names: `projects/archive/<name>-<stamp>.dkp`. A paste copies name and stamp, so two files can archive to the same path. An archive file is never replaced: the task chooses its path once, when it first runs, and keeps it. The probe reads each candidate whole, like every read of the device: `E_NOT_FOUND` means free; a file that passes `dkp::check` and has the source's write counter and crc32 is the archive copy already (a cut after the copy was written, so only the source is removed); a busy answer means try again later, never "taken"; any other file (also one whose header matches and whose body is damaged: it must not stand for the source) or status is taken, and `-2` to `-9` follow in turn (`<name>-<stamp>-2.dkp`; one digit keeps the longest path at 48 bytes). With none free the file stays in its slot and is archived by a later write to that slot.

A save that archives the stale file of the current slot (the file that made the project `ignored`) clears `ignored`. `install` names a module once per project change; past 16 owners it looks at the earlier entries of the same pass.

### Module list and restore

The module list is tag 1 of the image (section 4): the content modules that were loaded at a stock save. The runtime's module records hold them; a service file's record is never one (the records of `dkc_module_path` are the content modules, the provider records are another range).

- **Save.** When a save scan ends and the firmware's record of the slot is there, `BoundProject::record_modules` reads the loaded modules (`ModuleSet::of_loaded`: `Env::loaded_module` for each module record, in record order, with the store hash of the file from `Env::store_hash`) and replaces the image's list with `dkp::set_modules` when it differs (the id order and the hashes count). A changed list marks the image dirty, so the save writes. The planner then sees a list with a module as data (`dkp::has_data`): a project with loaded modules and no settings gets a file. With no loaded module and no data the save writes nothing, as before. An image too full for the list keeps its old list.
- **Load.** `BoundProject` counts the installs of an image (`epoch`). `ModuleRestore::tick` (one line of the service tick) sees another count, replaces its pending list with the list of the image that is bound now (`ModuleSet::of_image`: none for a stock project, no project or a failed load) and starts nothing in that pass, so the restore never begins in the tick of the install. In the next passes it takes the entries in order, one at a time:
  1. A module that is loaded already (`Env::loaded_module` of a record has the id; no store read): nothing, next. A loaded module that the list does not name is never touched; the service has no unload call.
  2. The file: the id with `/` as `-` and `.dkm` (`ModuleId::file`), searched once per entry under `modules/` by `Env::find_module_file` (`dkc_store_find`, which reads the whole store table; the kind folder is not the service's business). The search reads the store, so it waits while a live job stages (`Env::live_op` is not 0: the staging buffer is shared with the loader). `E_NOT_FOUND` after the whole table: `PROJECT_MODULE` what 3, `Env::warn_module(Missing)`, next. `E_BUSY` (an upload holds the store): the same entry again in the next pass. Any other status (a read error): what 4 with the status, `warn_module(Refused)`, next; the module is not said to be missing. The path found stays in `ModuleRestore::file` for the entry's retries. The hash in the list is not compared: the file on the card loads.
  3. `Env::live_load(path)` (`dkc_live(LIVE_LOAD, path)`). `OK`: `Step::Running`, what 1. `E_BUSY` (another live job: the menu's, or the reload of changed settings) or `E_STATE` (an upload or a store write): the same load again in the next pass, no count. `E_CARD_BUSY` (the card is being written): asked again after `CARD_WAIT_MS` (1 s), at most `CARD_TRIES_MAX` (3) times for the entry, as `settings_reload.rs` counts other refusals; then it is a refusal. `E_RELOAD_PLAYING`: `Step::Waiting`, asked again after `PLAY_WAIT_MS` (1 s), until the transport stops, no count (the job reports it at its check step, the queue call may too). `E_LOADED`: next. Any other status: what 4 with the status, `warn_module(NoFit)` for a fit refusal (`E_NOMEM`, `E_TOO_BIG`, `E_POOL`, `E_PLAN_FIT`, `E_DSP_NOSPACE`), else `warn_module(Refused)`, next; no restart is offered.
  4. `Running`: `Env::live_job()` (`dkc_live_state`). A job that is queued or running is waited for. When it ended and the module is loaded: `Env::selection_add(path)` (`dkc_selection_add`), what 2 with the status of that call, next. When it ended with a refusal of a load: step 3 with that status. A state that is another job's (not a load) means the load is asked again.
- **Limits.** Two ids can map to one file name: `a-b/c` and `a/b-c` are both `a-b-c.dkm` in the card layout (`<author>-<name>.dkm`, taxonomy). The restore searches by file name and loads the first file in table order for either id, so the file that loads may belong to the other id: the entry then asks again, the runtime answers `E_LOADED`, and the entry ends with no warning while the module the list names stays unloaded. A `.dkp` that a newer upload replaces between the listing and the read binds the slot empty (the header the read sees differs from the listing's; the mismatch rule of section 5 applies), so the module list of that file is not restored.
- A project change replaces the list (also one that arrives while a load runs: that job goes on in the runtime and the next load waits for it with `E_BUSY`).
- **Selection.** The runtime's live load updates neither the selection nor the settings record. `digikit-menu` does: `apply` in `menu.c` calls `dkrt_selection_set` (and `dkrt_settings_flush` for the route changes) before it queues the jobs. A load of the service behaves the same: the module ends in the saved selection (`dkc_selection_add`, which writes the selection as `selection_set` does, no write when the path is in it, `E_SEL_COUNT` when all eight places are taken), so a restart keeps it. The service adds it after the load ended well, not before: a module the runtime refuses stays out of the selection (the menu saves first and offers a restart on a refusal; the service has no such offer). The live load takes a path of up to 63 bytes, the selection one of up to 55 (`SEL_PATH`) and eight places. A path that is too long is known when the load is queued: one `PROJECT_MODULE` what 5 (status `E_SEL_PATH`) and the warning `NotKept`, and the module loads live all the same. A full selection or an unreadable record is known only at `dkc_selection_add`: the same what 5 and warning after the load, with its status.
- **Warning.** `Env::warn_module` shows one toast (`dk_ui_toast`, upper case, at most 31 characters) with the module's own name (the id after the author, as the menu shows it): `MODULE MISSING: <NAME>`, `<NAME> DOES NOT FIT` (a fit refusal, the menu's wording) and `<NAME> NOT LOADED` (any other refusal, also a store that could not be searched) and `<NAME> NOT KEPT` (the module is loaded, but the selection cannot hold it: it is not loaded again at the next boot). `dk_ui_open` and `dk_ui_toast` are optional imports of `core/ui` (`requires` in `sdk/sdk.toml`, so the need is optional): `setup` opens the handle, and without `core/ui` the handle stays null and only `PROJECT_MODULE` carries the facts.

## 4. `.dkp` format

Big-endian, version 1, header 42 bytes.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | magic `DKPJ` |
| 4 | 2 | version (1) |
| 6 | 2 | device id (`[digikit] id`) |
| 8 | 1 | slot (0-127) |
| 9 | 1 | name length (0-16) |
| 10 | 16 | name, zero padded, as read |
| 26 | 4 | stock save stamp (listing record +0x18) |
| 30 | 4 | write counter (1 for a new file, +1 per write) |
| 34 | 4 | `used`: 42 to 16384 |
| 38 | 4 | CRC-32 (zlib) over 0..38 and 42..`used` |

- Sections from 42: `u8 tag | u16 length | body`; unknown tags are skipped.
  - Tag 1, module list: `u8 id length | id | u32 hash` (store hash of the file, `files.h:26-30`).
  - Tag 2, module data: `u8 owner length | owner | entries`; entry `u8 key length | key | u16 value length | value`, key starts with `<owner>/` (`digikit-store.md` "Record").
  - Later: filter choices.
- Codec: `dkrt_sdk::settings` encode and decode take magic, header length and size limit as parameters; its vectors (`modules/core/runtime/vectors/settings-vectors.json`) stay, with new `.dkp` vectors.
- Two 16-byte aligned images in RAM: live and frozen. The create size is `used`; the store pads the tail sector (`files.h:61-62`).
- Bad CRC, other device, unknown version or bad bounds: file ignored, `PROJECT_IGNORED`.

## 5. File naming

`projects/<slot>-<name>.dkp`, slot `000`-`127`. A store path is 1-159 bytes of 0x20-0x7E (`digikit-store.md:74`); the longest file name is 33 bytes.

- Name and stamp come from the slot listing record (`MmcFs + 0x10 + slot*0x1c`, name +8, stamp +0x18), because the live name stores bytes >= 0x80 as 0 (plan "Firmware facts"). The tick runs after the stock save returns (open item 5).
- Stock table bytes stay: `A-Z`, `0-9`, `~!@#$%^&()_+-=`, space. `c5 c4 d6 dc df c6 d8 c7 d1`, 0x00 inside the name, `/` and any byte outside 0x20-0x7E become `_`. Trailing spaces and zeros are trimmed. An empty name gives `012-.dkp`.
- Stamp equal, name different (stock rename of a project that is not current): write under the new name with counter+1, then delete the old file. A rename of the current project also gives it a new stamp (finding 110 section 4); the save of the bound project into its own slot treats the file it was loaded from or last written to as a superseded copy of the same project whatever its stamp: it is deleted with the write, not archived. After a cut two files carry the prefix: the loader keeps the one that passes CRC and stamp with the higher counter and deletes the other. These removed files are superseded copies of the same project, not orphans.
- Stock New saves only the mirror slot 0x80 in `initializeNewProject`; the real slot is saved by the save wrapper, so the save exit hook sees the slot ([finding 110](../findings/110-project-identity.md) section 5). `0x40041c96` (DN2 `0x4003e38c`) is only a change notification, not a hook site.
- No orphan `.dkp` is deleted for good. A file that no longer belongs to its slot moves to `projects/archive/<name>-<stamp>.dkp` (stamp as 8 hex digits): (a) at a save into a slot whose existing file has another stamp (the slot was deleted, formatted or overwritten and a new project saved there): archive the old file, then write the new one; the one exception is the file the bound project came from or was last written to (`BoundView.file`) in a save into its own slot, which is a superseded copy and is deleted (the service never deletes a file it did not bind from); (b) at a stock clear (hook; clear keeps the stamp, so without the hook the cleared project would get the old modules back). The archive write uses the safe order of a rename: write the archive copy, then remove the old path.
- At a stock paste (hook) the service copies the source slot's `.dkp` to `<dest>-<name>.dkp` (paste copies stamp and name), so the copy keeps its digikit data. The paste exit hook sees only the destination, after the stock copy, card write and listing refresh are done: the source is the usable file of another slot whose name part is the destination's name and whose header stamp is the destination's stamp (the highest write counter if several; none: nothing is copied). The copy's header carries the destination's slot, name and stamp and counter 1; the destination's old files with another stamp are archived first. A stock move (cut and paste) gives the source the clear-like reset (archived) and the destination a copy.
- At a load with a stamp mismatch the file is ignored and left in place (no restore).
- Purpose of the archive: when the `.dkp` later holds the Elektron project content as well, project-manager can list `projects/archive/` and rebuild a project into a slot.
- Stamp different: ignore all files of the slot, leave them on the card. Save-as writes `<new slot>-<name>.dkp`. A file is written only when the module list is not empty or a section exists; no `.dkp` means no digikit data. A save of a project whose data was all removed archives the slot's file (it would bring the data back at the next load).
- A save writes when the data changed or the header the file carries changed (stamp, name, slot): the stock load makes a new live stamp, the next stock save writes it into the slot, so a file must follow it even when no module put anything.
- Save-as and New project look alike to the hooks (the setter with a new slot, then the save of that slot). Save-as keeps the data, a New project starts empty. The tick tells them by the live mirror's stamp (slot 0x80 of the firmware list, the live project's stamp after each stock save): the image remembers it when it was bound, and when it still equals the mirror's stamp the project is the same (save-as: the image is written to the new slot), else a new project was made (load of the slot: no file, empty). When the mirror cannot be read the data follows. [O]: unproven on a device; one place (`image_matches_live` in `engine/bound.rs`).

## 6. Errors and log events

No new status code. Used: `DK_E_SET_*` (`sdk/sdk.toml:1734-1738`), `DK_E_BUSY`, `DK_E_HOOK_STOCK` (-82, `sdk/sdk.toml:2024`: the service is refused before init).

New `[[log.kind]]` ids 48-51 (last id is 47, `sdk/sdk.toml:1625`):

| Name | Args | Emitted |
| --- | --- | --- |
| `PROJECT_LOADED` | slot, bytes, modules, counter | After the install, also for a slot without file (bytes 0). |
| `PROJECT_SAVED` | slot, bytes, counter, result | When the write step ends. |
| `PROJECT_IGNORED` | slot, reason (1 CRC or format, 2 device, 3 version, 4 stamp, 5 store, 6 save skipped, 7 save dropped), status (reason 5 only) | When a file is not used, also `dk_project_status` bit 3 (reasons 1 to 4). Reason 6 is no file's reason either: a stock save into the slot whose load failed was skipped, and its load goes again. Reason 7 is no file's reason either: the save was dropped because the firmware's listing had no record of the slot within 2 s. Reason 5 is no file's reason: the store refused the slot's listing or read with a status that is no busy answer, so no bit. The toast (`Env::toast`) is `PROJECT DATA IGNORED` for reasons 1 to 4, once per load, and `PROJECT DATA NOT READ` for reason 5, once per failure; reason 6 shows none. |
| `PROJECT_DEFERRED` | slot, last status, seconds | Once after 30 s of busy store (a write, a listing or a read), and not again until a write succeeds (`dk_project_status` bit 2). |
| `PROJECT_MODULE` (52) | slot, what, status, index, id hash | One event per step of the restore of a listed module that is not loaded: `what` 1 the live load was queued, 2 it ended well (`status`: the result of adding the module to the saved selection), 3 the module is not on the card, 4 the load was refused or failed, or the store could not be searched (`status`: the refusal), 5 the module is loaded and cannot be kept in the saved selection (`status`: `E_SEL_PATH`, `E_SEL_COUNT` or the read error). `index` is the place in the list, `id hash` the FNV-1a (32 bits) of the module id (`name_hash` of the SDK). A module that is loaded already has no event. Device tests wait on what 2 for each module they listed. |

Device tests wait on LOADED and SAVED, not on fixed times.

## 7. Invariants and proofs

| Invariant | Proof |
| --- | --- |
| Service not selected: stock bytes at every site; module settings are the record alone. | Assembly: card without the service, no site written. Unit (runtime): overlay absent, record only. |
| No `.dkp`: no toast, write or delete; empty image. | Unit with fake store; device: no popup. |
| Stamp mismatch: one warning, files untouched. | Unit. |
| Hook refused at the byte check: no service, stock works. | Assembly with a changed site byte: `E_HOOK_STOCK`. |
| Hooks write only the service's `Pending` and call nothing. | Assembly: run routines over fake firmware memory and a recording fake core. |
| Write only after a successful save, never slot 0x80. | Unit: D0.b zero, slot 0x80. |
| A save never waits and loses no data. | Unit: busy fake store gives the deferred event; next save writes. |
| A cut during the write keeps the old file. | Unit: abort mid-write; device: save during an upload ends with the file. |
| Load without save discards puts. | Unit. |
| Overlay: read precedence, write-through to record and project, remove, runtime keys stay device scope, no service means the record alone. | Unit in the runtime (`settings.rs`, `settings_overlay.rs`). |
| Only modules whose effective values differ are named; only loaded modules that opened their settings are reloaded. | Unit in the service (`tests/service.rs`) and the runtime (`settings_reload.rs`). |
| A module's data in the project outlives its unload and nothing else does. | The contract suite with `Fixture::survives` (`tests/contract.rs`). |

## 8. Open items

1. Does a stock rename keep the save stamp? One emulator check; if not, hook the rename callback (DT2 `0x400a1246`, DN2 `0x4009b3bc`). The service handles a name change with the same stamp (write the new name, remove the old file); a changed stamp is a mismatch.
2. RAM, measured: packed `project.dks` 55264 bytes (DT2 and DN2 alike), bss 38052 bytes in the committed object `modules/core/services/project/dks/build/project.dko.json` (two 16 KiB images and the state). Fallback 8 KiB files if that is too much.
3. Done: `dk_files_read` has `context = "ui"` (it already ignored the context; the tick reads).
4. Gone: modules get no event (decision: modules stay unaware of projects).
5. Confirm the listing record holds the new name and stamp at the first tick after a save. The service waits up to 2 s for the record of a saved slot and drops the save after that.
6. Firmware rows are in finding 110 section 5 (New, unload, delete, clear, format, load, paste, save-as). Still open: the DN2 paste and clear record reset (traced on DT2), stock move (no callers found), what triggers `loadProjectFromMemory` (`0x40043f74`), and whether format's later erase loops stay below the write guard `0x457FFF` (finding 59).
7. The paste exit (`cf_call` with `offset` 0x11c, section 3) is [V] with the clear exits: checked against image bytes on DT2 1.17, DN2 1.12 and DN2 1.11 (paste function `0x400f548e`, `0x400f6e6a`, `0x400f6c9e`); the displaced bytes are equal, and each image has one caller of the function. D0.b nonzero at the paste exit means the copy reached the card write; the write's own result is not returned. The hook routine is the same on all.
8. The reload of a module whose settings changed is proven on the host only (the rule, the queue). A device test drives it later (step 7).
