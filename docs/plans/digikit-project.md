# digikit project

Status: plan. Not started. Work happens on a new branch `digikit-project`, cut from main after the sampler branch (`sdk/dsp-sampler-discovery`) merges.

## Goal

A digikit project holds the digikit data that belongs to one Elektron project: the modules that project needs and each module's data for that project. Elektron project files stay stock. Stock save and load work as before, and a stock project without digikit data loads as before.

## Decisions

1. **One home for per-project data: a `.dkp` file in the digikit store**, one per Elektron project. Earlier docs named three homes (a project scope in the settings record, a `.dkp` in `~/src/digi/WORKBENCH-AND-MODULES-PLAN-2026-10-19.md`, and the "per-project selection store" in [sample-api-1.5](../design/sample-api-1.5.md)). The `.dkp` replaces all three.
2. **Module persistence API in the SDK, with two scopes.**
   - Module scope: the module's own settings, the same in every project. Stored in the settings record ([digikit-store](../design/digikit-store.md), "The settings record") under keys `<module id>/<key>`, the shape already reserved there and in [module-architecture](module-architecture.md) D19.
   - Project scope: the module's section of the current `.dkp`.
   - One API, `dk_settings`: `dk_settings_open` opens module scope, `dk_settings_open_project` opens project scope. `dk_settings_get`, `dk_settings_put` and `dk_settings_remove` act on the scope of the handle. The runtime adds the module id to each key, so a module cannot read another module's data. Without the service, `dk_settings_open_project` returns "not available".
   - Modules get a project-changed event through a named module export `dkm_project` (like `dkm_menu`), called by the `project` service. Slots cannot carry it: there are 8 single-holder slots and only slot 0 is called.
   - Project-scope entries use the settings-record entry format and its limits (key 1-47 printable bytes including the module id and `/`, value at most 1024 bytes). Larger data uses one key per item; the pool list uses `pool/0` .. `pool/15`.
3. **The runtime stores; modules decide what to store.**
   - The runtime keeps the shared sample pool mechanics: memory, loading, dedup, refcount, DT2 stock-slot borrowing. These are shared between modules and tied to DSP reserve, the DSP-link queue and the capacity checks ([module-architecture](module-architecture.md) section 3.6), and there is no module-to-module API.
   - The runtime drops its per-project pool lists: the `Proj` rows and `project_changed` handling in `modules/core/runtime/sdk/src/sample_tracks.rs`.
   - The sample picker (`sdk/src/sample_picker.c`, linked into each sampler module) keeps the module's pool list (entry index to path) in the module's project scope. On a project change and on pool lost, it first calls `sample_pool_set` for each entry of the new list, then `sample_pool_clear` for entries the list does not use, so files shared by both projects keep their entry (the order in sample-api-1.5 section 3). The re-drive must be idempotent.
   - Contract note: `sdk/include/dk/sample_track.h:74` ("the pool is kept per project") changes meaning. The SDK is 0.x, so a minor version may break; SHOT is the only user.
   - Result: new modules that need saved state use the general API and need no runtime change.
4. **Write on Elektron save.** Project-scope data is held in RAM and written to the `.dkp` when the stock project save succeeds. Changes the user discards (load without save) are discarded for digikit too. A save hook refused at the byte check means no service (see "Stock safety" 4).
5. **Identity.**
   - The slot is the identity. The file is `projects/<slot>-<name>.dkp`, for example `projects/012-MY SONG.dkp` (the store belongs to one device, so the path has no device part). The service never looks a project up by name.
   - The `.dkp` header stores device, slot, name and the save stamp: the 32-bit word at card slot header +0x18, which stock writes on each save. It is a save stamp, not an identity (finding 110 and finding 39 say the word is not an identity; both are true).
   - On load, the service takes the file that starts with `<slot>-` and compares its stamp with the listing record's stamp for the slot, before the load regenerates the live stamp. Match and another name (a stock rename): rename the file to the new name, then load. Mismatch (the slot was overwritten outside digikit, or a project was copied in on the +Drive): one short warning, the file is ignored.
   - Save as to another slot: the save hook sees the new slot and writes `<new slot>-<name>.dkp` from the current data.
6. **Validation on load.**
   - Modules missing from the card: one short warning in the firmware font. The plain Elektron project loads. No error screens.
   - Modules on the card but not selected: offer "load" as one button ([one-button flows]). With E5c (live load and unload, DIGIKIT MODULES applies live) the load is applied live. The restore uses the E5c live jobs. A restart happens only when the live load is refused (missing service, does not fit); then the same "RESTART TO APPLY?" flow as the menu runs.
   - Transport playing: the live apply is refused while playing. The project loads as stock. The offer changes to "load on stop": the apply runs when the transport stops. The restart path stays in the offer.
7. **A `project` service and an optional tool module.** Words and card paths: [taxonomy](../design/taxonomy.md).
   - `project` is a service (`services/project`, file `services/project.dks`, crate `dkrt-svc-project`). Its hooks on the stock project save and load are names in `devices/fw/project.toml` `[[hook]]` entries, resolved into the port file ([port-file](port-file.md)); they are not addresses. Before port step 3, the hook routines are per-device `.s` files in the service, as for lfo, machines and fx. Port step 3 moves them later. There are four hooks per device: slot setter, save exit, clear and paste. There is no delete or format hook. The sites are in the "Firmware facts" table and [finding 110](../findings/110-project-identity.md) sections 4 and 5.
   - The service writes and reads `projects/<slot>-<name>.dkp` in the store, beside the stock files, by the rules of decision 5. The slot is three digits, `000` to `127`. The file name maps the bytes `c5 c4 d6 dc df c6 d8 c7 d1` and any byte not safe in a store path to `_`. The header holds device, slot, name and save stamp. A project gets a `.dkp` only when there is digikit data to keep.
   - The service, not the runtime, tells modules about project changes (`dkm_project`).
   - It restores the digikit state of the project through the runtime's live job (E5c): the module list and the filter choices.
   - The optional module `digikit/project-manager` has kind `tool`. It is UI only: it imports the exports `dk_project_*` of the service. The user selects it when wanted. In the first version it only listens. Later it lists `projects/*.dkp` and loads a project from the list through the stock load.
   - The service not selected: no service, no hooks, stock behaviour.
   - Project scope holds the module list, the filter choices (extension-model E7, O6) and each module's own project-scope section from decision 2, for example the pool list of the sample picker (decision 3, step 5). FX routes and sends stay at device scope in the settings record for now (open question 4).
   - Dependencies: E5c (live restore) and the port file stack (the hook sites). Order: after E8, as in the [taxonomy](../design/taxonomy.md) order.
   - The service writes the store with the SDK file calls (`dk_files_create`, `dk_files_write`, `dk_files_close`). They keep the card checks (`block_write`, finding 46: a foreign card refuses, a blank card is claimed first) and the atomic replace. See "Step 2 design".
   - A save during an upload: the write returns `E_BUSY` and the service retries later. See "Step 2 design".
   - The hooks only read stock state. Do not change stock project storage or the `Value<T>` project-data classes (ask Em before touching storage).
   - The warning and the "load" offer stay in `digikit-menu`, or move into `digikit/project-manager` when it is selected.
8. **Later, not in this plan.**
   - Elektron project data inside the `.dkp` (maybe, after this lands).
   - A workbench bundle export: Elektron project plus `.dkp` as one backup file (host side).
   - `project-manager` loads a project from its list through the stock load.

## Architecture

Words and card paths: [taxonomy](../design/taxonomy.md). Hooks and anchors: [port-file](port-file.md).

### Parts

| Part | Owns | Calls | Must not |
| --- | --- | --- | --- |
| SDK settings API (`dk_settings_open`, `dk_settings_open_project`, `dk_settings_get`, `dk_settings_put`, `dk_settings_remove`) | The one module-facing API for saved data. The scope is chosen at open. | The runtime. | Add a second API. Let a module reach another module's keys. |
| Runtime | Module scope: the settings record, keys `<owner>/<key>`. The project-scope opener: one optional pointer to the project service's data entry points, set when the service loads and cleared when it unloads; it forwards to them. The live jobs (E5c). | The service's data entry points, when the pointer is set. | Know the `.dkp` format, project names or hook sites. Make modules need the service. |
| `project` service | The `.dkp` files under `projects/`. Project-scope data in RAM. The four hooks (stock project setter, stock save exit, clear, paste). The LOADED and SAVED events to modules. | Hook sites by name (port file). The store writer. The runtime live job for the restore. The module export `dkm_project`. | Change stock project storage or the `Value<T>` project-data classes. Call module code from inside a hook. |
| `digikit/project-manager` (tool module, UI only) | First version: nothing beyond selecting the service. Later: a list of `projects/*.dkp`, then a load from the list. | The service's exports `dk_project_*`. | Write `.dkp` files. Call the stock project load in the first version. |
| `digikit-menu` | The missing-module warning and the load offer, when project-manager is not selected. | The runtime live job. | Read `.dkp` files. |
| Port file | The hook sites, resolved from `devices/fw/project.toml` `[[hook]]` entries, and their byte check. | Nothing. | Hold project logic. |

### Scope at open

- `dk_settings_open(ctx, &h)` is unchanged: module scope.
- `dk_settings_open_project(ctx, &h)` is new. Its handle carries the scope. `dk_settings_get`, `dk_settings_put` and `dk_settings_remove` act on the scope of the handle.
- The runtime exports the opener and forwards to the service while the service is loaded. Without the service the opener returns "not available", and the module continues with module scope or its defaults.
- A module does not need the service: it loads with or without it.
- Alternative, not chosen: the module imports an opener from the service as an optional need. It keeps the runtime free of the pointer, but needs optional imports in the loader.
- Project-scope limits equal the settings record: key 1 to 47 bytes, value at most 1024 bytes. The runtime adds the module id to each key.

### Data flow

```text
module put/get:  module -> dk_settings_put(h) -> runtime -+- module scope  -> settings record
                                                          +- project scope -> service -> .dkp (RAM)

stock save:      stock save ends with success -> hook records slot -> service writes projects/<slot>-<name>.dkp
                 -> SAVED to modules (UI task)

stock load:      stock sets the project slot -> hook records slot -> service finds projects/<slot>-*.dkp,
                 compares the save stamp, renames the file if the project name changed
                 -> LOADED to modules (UI task) -> live job restores the project's modules
                 -> digikit-menu: missing-module warning, load offer

stock clear:     hook records the slot -> service archives projects/<slot>-*.dkp to
                 projects/archive/<name>-<stamp>.dkp (no module event)

stock paste:     hook records source and destination -> service copies the source .dkp
                 to <dest>-<name>.dkp (no module event)
```

The hooks run inside the stock save, load, clear and paste paths. They only record what happened; the service acts in its tick. The service sends events and calls module code later, from the UI task, like every other module callback.

### File name and identity

- File: `projects/<slot>-<name>.dkp`, for example `projects/012-MY SONG.dkp`. The slot is three digits, `000` to `127`.
- The slot is the identity. The service never looks a project up by name. The name is for the user to read.
- The stock name holds at most 16 bytes, from a known character table ([finding 110](../findings/110-project-identity.md) section 4). The file name maps the bytes `c5 c4 d6 dc df c6 d8 c7 d1` and any byte not safe in a store path to `_`.
- The header keeps device, slot, name and the stock save stamp (card slot header +0x18), which stock writes on each save.
- On load the service takes the file that starts with `<slot>-` and compares its stamp with the stamp of the slot:
  - Match and another name (a stock rename): rename the file to the new name, then load.
  - Mismatch (the slot was overwritten outside digikit, or a project was copied in on the +Drive): one short warning, the file is ignored.
- Save as to another slot: the save hook sees the new slot and writes `<new slot>-<name>.dkp` from the current data, so the digikit data follows the save-as.
- A project gets a `.dkp` only when there is digikit data to keep. A new project writes none.
- Archive rule: the service never deletes a `.dkp` for good. A file that no longer belongs to its slot moves to `projects/archive/<name>-<stamp>.dkp` (stamp as 8 hex digits): at a save into a reused slot (the existing file has another stamp) and at a stock clear. Stock paste copies the source `.dkp` to the destination slot. A stamp mismatch on load ignores the file and leaves it in place. A rename's old path and the loser of the two-file rule are superseded copies of the same project and may be removed. Design: [digikit-project](../design/digikit-project.md) section 5.

### Events

Save (hook at the single exit of the stock save, success only, slot 0x80 skipped):
1. The service writes the `.dkp` from RAM, when there is digikit data.
2. It sends SAVED to modules through `dkm_project`, from the UI task.
3. It writes a "saved" event to the runtime log.

Load (hook on the stock slot setter, on change):
1. No `.dkp` for the slot: nothing happens.
2. Stamp mismatch: one short warning in the firmware font. The `.dkp` is ignored.
3. Match: rename the file if the name changed. Read the sections. Send LOADED to modules from the UI task. Write a "loaded" event to the runtime log.
4. The service starts the live job for the project's module list (and later the filter choices). The runtime applies it live.
5. Modules missing from the card: `digikit-menu` shows one warning. Modules on the card but not selected: it shows the "load" offer.
6. A refused live apply (missing service, does not fit): "RESTART TO APPLY?", as in the menu flow.
7. Transport playing: the live apply is refused while playing. The project loads as stock. The offer changes to "load on stop": the apply runs when the transport stops. The restart path stays in the offer.

Clear (hook on the stock clear, single and multi-slot): the hook records each slot. The service archives the slot's `<slot>-*.dkp` to `projects/archive/<name>-<stamp>.dkp`. No module event.

Paste (hook on the stock paste): the hook records source and destination. The service copies the source `.dkp` to `<dest>-<name>.dkp`. No module event.

### Stock safety

1. Service not selected: no hooks, pure stock.
2. Service selected, the project has no `.dkp`: nothing happens. No popup, no warning, no change.
3. A `.dkp` with a stamp mismatch: one short warning, the file is ignored.
4. A hook refused at the port-file byte check: no service. Stock load and save are unaffected.
5. The hooks only read stock state. Stock project storage and the `Value<T>` classes are never changed.

### Open items

- The service's RAM budget for two 16 KiB `.dkp` images (32 KiB, live and frozen); fallback 8 KiB files ("Step 2 design").
- Does a stock rename keep the save stamp? One emulator check: rename a project and read the slot header stamp before and after. If the stamp changes, the service hooks the rename callback (DT2 `0x400a1246`, DN2 `0x4009b3bc`; both devices) and moves the file at once ([finding 110](../findings/110-project-identity.md) section 4).
- Whether stock refuses duplicate names (no check seen; the slot in the file name makes this harmless).
- Firmware [O] items of [finding 110](../findings/110-project-identity.md) section 5: whether format's erase loops all stay below the write guard `0x457FFF`; the DN2 paste callback and the DN2 clear record reset (not traced); stock move (cut), no callers found; what triggers load from memory (`0x40043f74`); whether a rename keeps the stamp (section 4).
- A load from project-manager's list calls the stock project load: a later step that needs its own firmware facts (task, locks, a real progress callback).
- Order of the `dkm_project` calls against the live job during a restore.

## Step 2 design: the .dkp format and the write path

### Write path

- The service writes with the SDK file calls that every module can import: `dk_files_create` with root `DK_FILES_STORE` and flag `DK_FILE_REPLACE`, `dk_files_write`, `dk_files_close` (`sdk/include/dk/platform/files.h:46-60`). They run in an I/O step (`dk_io_post`). No runtime or ABI change is needed, and there is no new core writer.
- The calls inherit the card checks (a foreign or read-only card refuses with `E_INUSE`; a blank range is claimed first, through `block_write`) and the atomic replace (`DK_FILE_REPLACE` keeps the old file whole until close swaps the table).
- The SDK path has its own buffer. It does not use the upload buffers. It shares the one store write session with SysEx upload, so a write during an upload returns `E_BUSY`.
- Reads use `dk_files_list` of `projects/` (prefix match on `<slot>-`) and `dk_files_read`, from the service's UI tick, never while a live job stages (the read stage is shared with the loader).
- The service gets a UI tick: the `service!` macro takes a `tick =` entry (today it sets none).

### The .dkp format

Big-endian, as the store.

| Offset | Field |
| --- | --- |
| 0 | magic `DKPJ` |
| 4 | u16 version (1) |
| 6 | u16 device id |
| 8 | u8 slot |
| 9 | u8 name length |
| 10 | name, 16 bytes |
| 26 | u32 stock save stamp (card slot header +0x18) |
| 30 | u32 write counter (+1 per write) |
| 34 | u32 bytes used |
| 38 | u32 CRC-32 over the file except this field (the settings record's CRC) |

- Sections follow the header: `u8 tag | u16 length | body`. An unknown tag is skipped. Tag 1: the module list (`u8 id length | id | u32 hash` per module). Tag 2: one module's project data (`u8 owner length | owner | entries` in the settings-record entry format, key 1-47 bytes including the owner prefix, value at most 1024 bytes). Later tags: the filter choices.
- The codec reuses the settings-record encode and decode (`dkrt_sdk::settings`) and its vectors, with the new magic and header.
- Size: a file holds at most 16 KiB. The service keeps two images in RAM: the live one (puts from the UI task) and a frozen copy that the I/O step writes (16-byte aligned). A put over the limit returns `E_SET_FULL`. Key and value rule breaches return `E_SET_KEY` and `E_SET_VALUE`, the same codes as module scope. Open: the service's RAM budget for 32 KiB; fallback 8 KiB files.
- A bad CRC, another device or an unknown version on load: ignore the file and write one warning, as for a stamp mismatch.

### Rename and duplicates

- A stock rename: write the file under the new name, then delete the old one. A cut between the two leaves two files with the prefix `<slot>-`. The loader keeps the one with the higher write counter and deletes the other.
- A later option: `dk_files_rename` over the store's table-only rename, one atomic commit, if renames turn out common (an SDK minor version).

### Save during an upload, and timing

- The save hook only records: dirty, slot, stamp. A second save before the write overwrites the pending record.
- The service tick freezes the image and posts the write step. On `E_BUSY` the step retries (the runtime reruns it after 1 ms, up to 1 s), then the tick retries every second. After 30 s it writes one "project write deferred" event and keeps the data dirty in RAM; the next save writes it. The stock save never waits and no data is dropped.
- Cost of a 16 KiB write (32 sectors, read-back hash, table commit with two header writes): expected tens of ms, at most 200 ms. Measure on the emulator and record the number.

### Tests

These add to step 7.

- Unit, in the service with a fake store: encode and decode with the vectors; CRC and device mismatch; oversize `E_SET_FULL`; owner prefix; rename (write new, delete old); the two-file case by write counter; `E_BUSY` queue and the deferred event.
- Assembly: the packed service loads with the test port file; a service I/O step creates, writes and closes a store file (and the step's owner is the service record); a foreign card refuses with `E_INUSE`.
- Device (the one test per device): also the file bytes through a store dump, and a save during an upload ends with the file present.

### Effort

About 10-12 agent hours: format and codec 3 h, writer, queue and tick 4 h, `service!` tick 1 h, tests 3 h.

## State today

No digikit state is per project today:

- Module selection: the card's selection record.
- FX routes `fx/route/<id>` and send amounts `fx/send/<id>`: settings record, device scope. The send amount is never written to Elektron files.
- LFO shape numbers `lfo.shapes` and machine type numbers: sticky, device scope.

A stock save loses nothing that digikit holds. The job of the project service is to make the chosen parts project-scoped.

Module absent when a project loads (what the device does today):

| Absent part | Result |
| --- | --- |
| Custom machine | The type number stays in the kit. The row is a gap row and plays the sound of the first stock machine (DT2 sample machine, DN2 FM TONE). The DT2 SRC page shows empty boxes. |
| Effect module | Nothing runs. Kit values of the absent effect read back as defaults. Route and sends stay in settings. |
| LFO shape, no lfo service | WAVE >= 7 shows ERR and plays like RAND. The value survives save and load. |
| LFO shape, lfo service present, shape module missing | Shows "----" and plays TRI. |
| Module filter (E7) | The sound has the stock fallback filter. |

A later extension-model item, "MISSING and silent for absent modules", would change the machine row.

## Firmware facts

Static reads on DT2 OS 1.17 and DN2 OS 1.12. [D] from one read, then checked by a second agent against disassembly: all rows confirmed, with the corrections included here. The hook sites (setter, save, save exit) are [V] in [finding 110](../findings/110-project-identity.md) section 4, with first bytes and displaced byte counts. Step 1 still checks the rows marked unchecked below. Commands: `uv run python -m tools.coldfire.fw {decomp|disasm|callers|xref} ADDR -i dt2-1.17|dn2-1.12`.

| Item | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Current slot byte | `0x4099d5c5` | `0x405ce899` |
| Slot setter (writes only on change; DT2 notifies via `0x400c102c`) | `0x400c11ca` | `0x400bb7fa` |
| Setter wrapper `(Project*, slot)`: `if Project::vfunc(0x28) != 0 then setter(slot)` | `0x400424a2` | `0x4003eb62` |
| Slot getter (`0xffffffff` when no project) | `0x400424ca` | `0x4003eb8a` (reads the slot byte through `0x400bb868`) |
| `Project*` global | `0x44f370a0` (set by `0x40198b7e`) | `0x446488d0` (set by `0x4018ac5a`) |
| Live `project_t*` | `*(Project+0x10)` (`Value<project_t>::vfunc_10` at `0x401a61ea`) | same (vfunc_10 at `0x40195b5a`) |
| Live name: 16 bytes ASCII, zero padded, bytes >= 0x80 stored as 0, no guaranteed NUL | `*(Project+0x10) + 0xc` | same |
| Name writer `ProjectDataChangeInfo(Project*, src)`, flag 0x20 | `0x40042140` | `0x4003e800` |
| Live stamp (built by `0x401524ce` / `0x4015097e` from two 15-bit LCG steps `0x401524a0` / `0x40150950`; bits 31 and 15 are always 0) | `*(Project+0x10) + 8` | same |
| `ProjectDataChangeInfo` notification with 0x80 (posts a change to a vfunc at +0x10; not the stamp builder) | `0x40041c96` | `0x4003e38c` |
| Save `(Project*, slot, progress)`, nonzero on success. Slot 0x80 is the live mirror: skip it | `0x40043a30` | `0x40040190` |
| Save / save-as wrapper `(Project*, slot or -1, progress)`: setter(slot) (or the getter for -1), then `FUN_40043b28(slot)` (rejects slot > 0x80, calls the save), then save slot 0x80, then setter(slot) again, then `FUN_40042522(Project*, 0)` | `0x40043b78` | `0x400402d8` |
| `loadProjectFromMmc` (does not call the setter itself) | `0x40046906` | `0x40042b92` |
| Load wrapper (calls the setter). Order: load, save slot 0x80, setter | `0x40046c38` | `0x40042e6e` |
| `initializeNewProject` | `0x400465e2` | `0x400428c0` |
| `loadProjectFromMemory` | `0x40043f74` | `0x400403d6` |
| Per-slot listing record at `MmcFs + 0x10 + slot*0x1c` (slots 0..0x80): valid byte +0, name +8..+0x17, save stamp +0x18 (copied from card header +0x18 by the listing refresh DT2 `0x4012f2fa` / DN2 `0x4012dbce`, which also runs after each save). `FUN_4012f048(MmcFs, slot)` returns record+8 (or the static `0x44e406d4` when invalid) | `0x4012f048` | `0x4012d91c` |
| Live to card header copy (flag 0x80 writes header +0x18 = live stamp) | `0x400df66c` | `0x400e15f8` |

When the setter runs:

| Action | Setter runs |
| --- | --- |
| Load | Yes, in the load wrapper. Name and stamp are in place before it. |
| New | Yes, inside `initializeNewProject` and in the save wrapper ([finding 110](../findings/110-project-identity.md) section 5). |
| Load from memory | Yes (`0x40043f74` calls it; what triggers it is open). |
| Save-as to another slot | Yes, before and after the write. Name unchanged. |
| Save to the current slot | No change (the setter is a no-op). |
| Delete | Only if the deleted slot is current: setter(-1); no save. |
| Format, reset, `project_container_decode` | setter(-1) (format only when a Project exists); no save. |
| Rename (DT2 `0x400a1246`, DN2 `0x4009b3bc`) | No. Updates the live name when the slot is current. |
| Rename in the +Drive browser (DT2 `0x400e839a`, DN2 `0x400e9d6a`) | No. Patches the card name and the listing only, not the live name (unchecked). |
| Clear, paste | No setter, no save (clear `0x400a131e`, `0x400a1376`; paste `0x400a12f8`). Hooked. |
| Move (cut) | [O]: `0x400f539e` has no callers in the static read. |
| Power-down autosave | [O], none found. |

Correction to [finding 110](../findings/110-project-identity.md): `loadProjectFromMmc` does not call the setter; its wrapper does.

## Storage

- The settings record (8 KiB per copy, value at most 1024 bytes, shared with `fx/route/<id>` and `fx/send/<id>`) is enough for module scope. It is too small for project scope: one pool list is 16 paths of up to 159 bytes, about 2.6 KiB.
- The `.dkp` is a store file. Store format and table: [digikit-store](../design/digikit-store.md), code in `modules/core/runtime/sdk/src/store.rs`. The service writes `.dkp` files with the SDK file calls; no new core writer is needed. The format and the write path are in "Step 2 design".
- Write only on save, so card writes stay rare.

## ABI and versions

- The new entry `dk_settings_open_project` is appended to the `Api` table in `sdk/sdk.toml`. It raises the API version at build time; its `since` is that API version.
- The changed meaning of `sample_track.h:74` plus the new entry is a runtime 0.x MINOR (the runtime version at build time; module-architecture section 12).
- A store format change (the `.dkp` file) is also a MINOR.

## Open questions

1. What counts as a needed module. Proposal: a machine module used by any track, or an FX module with an active route. Fallback: the modules selected while the project was active.
2. Does a stock rename keep the save stamp? Open: [finding 110](../findings/110-project-identity.md) section 4 [O]. Fallback: hook the rename callback (DT2 `0x400a1246`, DN2 `0x4009b3bc`; both devices) and move the file at once. Check that a +Drive rename of the loaded project behaves the same as a menu rename.
3. Remaining [O] of finding 110 section 5: stock move (cut) has no callers in the static read; the DN2 paste callback is not traced. Clear and paste are covered by the clear and paste hooks and the archive rule.
4. FX routes stay global in the settings record unless Em wants them per project.
5. DT2 stock project load also resets the borrowed pool slots ([finding 109](../findings/109-dt2-stock-pool-borrowing.md)). The re-drive comes from the module (decision 3), on pool lost and on project changed. Check the order of the two events, and the order of the `dkm_project` calls against the live job during a restore.
6. Power-down autosave: none found; confirm.
7. The service's RAM budget for two 16 KiB `.dkp` images (32 KiB, live and frozen); fallback 8 KiB files.
8. Whether stock refuses duplicate names (no check seen; the slot in the file name makes this harmless).
9. A load from project-manager's list calls the stock project load: a later step that needs its own firmware facts (task, locks, a real progress callback).

## Steps

Estimate: about 1.5 days of agent time.

1. [x] Byte check of the firmware facts, both devices ([finding 110](../findings/110-project-identity.md) sections 4 and 5), except the [O] items listed under Open items. (1 h)
2. [x] Design doc [digikit-project](../design/digikit-project.md): the `.dkp` format and write path of "Step 2 design", C signatures of the module API, events, error codes. (1 h)
3. [ ] Service `project` with four hooks per device, no delete or format hook. The sites are in [finding 110](../findings/110-project-identity.md) sections 4 and 5: the setter (on change; stack argument at SP+4, -1 = no project), the save exit (the single exit of the save; the hook reads success from D0.b and the slot from D3; slot 0x80 is skipped), clear and paste. The service archives and copies `.dkp` files by the archive rule (Architecture "File name and identity"); it deletes none for good. Hook entries in `devices/fw/project.toml` `[[hook]]`; routines as per-device `.s` files in the service before port step 3. The service emits its own loaded and saved signals to modules (the export `dkm_project`, decision 2); the runtime has no `dkrt_project_*` calls for it. The service also emits project "saved" and "loaded" events (runtime log) after the stock save and load finish; device tests wait on them instead of the fixed 3.5 s and 6 s waits in `tests/regression/runtime/project-kit.ts`. Then the optional tool module `digikit/project-manager`. (0.5 day)
4. [ ] Runtime and service: the service writes the `.dkp` with the SDK file calls, reads it on project change, writes it on save; the opener `dk_settings_open_project` in `sdk/sdk.toml` (API minor), forwarded by the runtime to the service. Module scope uses the settings record. (0.5 day)
5. [ ] Pool: remove the per-project lists from `sample_tracks.rs`; the sample picker keeps its list in project scope and re-drives on change and on lost. (3 h) Docs to update: sample-api-1.5 (lines ~45, 65, 116-118, 137-138, 168 and its TODO on store persistence), `sdk/include/dk/sample_track.h:74`, the `sample_tracks.rs` header comment and the tests calling `project_changed` (`sample_tracks/tests.rs`), module-architecture 3.10 and section 12, digikit-store "Keys by owner" and the line that only upload writes the store.
6. [ ] Validation: missing-module warning and the "load" offer (applied live, restart only on refusal) in `digikit-menu`. (2 h)
7. [ ] Tests. (3 h)
   - Unit tests in the service with fixtures: `.dkp` encode and decode, identity and rename, mismatch, size limits, save-success-only, scope and owner prefix, the opener without the service. The step 2 cases are listed in "Step 2 design", Tests.
   - One case in the shared contract suite: a project change drives the live load and unload through the same job.
   - Assembly: the service with the test port file, one fit and one refusal per seam.
   - Device, at most one per device, driving firmware paths: save, `projects/<slot>-<name>.dkp` written, SAVED and LOADED events in the runtime log; a plain stock project with no `.dkp` loads with no popup and no change.
8. [ ] Later: move FX send amounts and F4 effect values out of kit spare halfwords into the `.dkp` (per kit).

## References

- [digikit-store](../design/digikit-store.md): store layout and the settings record.
- [module-architecture](module-architecture.md): layers, D12, D13, D19, section 3.6.
- [sample-api-1.5](../design/sample-api-1.5.md): pool API and project handling today.
- [finding 110](../findings/110-project-identity.md): project identity.
- [finding 109](../findings/109-dt2-stock-pool-borrowing.md): DT2 pool borrowing and stock resets.
- [finding 63](../findings/63-settings-store.md): settings store.
- `~/src/digi/WORKBENCH-AND-MODULES-PLAN-2026-10-19.md` (outside the repo): the earlier `.dkp` item.

[one-button flows]: ../design/ui-rules.md
