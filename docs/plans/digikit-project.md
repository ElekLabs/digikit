# digikit project

Status: steps 1 to 6 are built on `svc/project` with host proofs; step 7 (tests on a device) is open. The open items are the one list "Open items".

## Goal

A digikit project holds the digikit data that belongs to one Elektron project: the modules that project needs and each module's data for that project. Elektron project files stay stock. Stock save and load work as before, and a stock project without digikit data loads as before.

## Decisions

1. **One home for per-project data: a `.dkp` file in the digikit store**, one per Elektron project. Earlier docs named three homes (a project scope in the settings record, a `.dkp` in `~/src/digi/WORKBENCH-AND-MODULES-PLAN-2026-10-19.md`, and the "per-project selection store" in [sample-api-1.5](../design/sample-api-1.5.md)). The `.dkp` replaces all three.
2. **Modules stay unaware of projects: a project overlay over the settings API.** No module change, no new SDK call, no project event.
   - A module keeps using `dk_settings_open`, `dk_settings_get`, `dk_settings_put` and `dk_settings_remove` (keys `<module id>/<key>` in the settings record, [digikit-store](../design/digikit-store.md), "The settings record").
   - While the `project` service is loaded, the runtime puts an overlay over a module's settings handle ([digikit-project](../design/digikit-project.md) section 1). A read returns the current project's value if its `.dkp` holds the key, else the record's value. A write goes into the project image (written to the `.dkp` on the stock save) and through to the record, so the record holds the last used value and a new project starts from it. Module-owned keys only: the runtime's own keys (effect routes and sends, the selection) stay device scope. Without the service settings work exactly as before.
   - On a project load the runtime live-reloads (E5) only the modules that opened settings and whose effective values differ between the old and the new project, so their `init` re-reads the settings. The service finds the differing owners; the runtime reloads.
   - Entries use the settings-record format and limits (key 1-47 printable bytes including the module id and `/`, value at most 1024 bytes); the file holds at most 16 KiB.
3. **The runtime stores; modules decide what to store.**
   - The runtime keeps the shared sample pool mechanics: memory, loading, dedup, refcount, DT2 stock-slot borrowing. These are shared between modules and tied to DSP reserve, the DSP-link queue and the capacity checks ([module-architecture](module-architecture.md) section 3.6), and there is no module-to-module API.
   - The runtime drops its per-project pool lists: the `Proj` rows and `project_changed` handling in `modules/core/runtime/sdk/src/sample_tracks.rs`.
   - The sample picker (`sdk/src/sample_picker.c`, linked into each sampler module) keeps the module's pool list (entry index to path) with `dk_settings_put` under its keys (`pool.0` .. `pool.15`: a settings key holds no `/`) and reads it in `init`. With the service loaded the list is per project by the overlay; a project change reloads the module, whose `init` sets each entry of the list and clears the others (the order in sample-api-1.5 section 3). The picker has no project code.
   - Contract note: `sdk/include/dk/sample_track.h:74` ("the pool is kept per project") changes meaning. The SDK is 0.x, so a minor version may break; SHOT is the only user.
   - Result: new modules that need saved state use the general API and need no runtime change.
4. **Write on Elektron save.** The current project's data is held in RAM and written to the `.dkp` when the stock project save succeeds. Changes the user discards (load without save) are discarded for digikit too. A save hook refused at the byte check means no service (see "Stock safety" 4).
5. **Identity.**
   - The slot is the identity. The file is `projects/<slot>-<name>.dkp`, for example `projects/012-MY SONG.dkp` (the store belongs to one device, so the path has no device part). The service never looks a project up by name.
   - The `.dkp` header stores device, slot, name and the save stamp: the 32-bit word at card slot header +0x18, which stock writes on each save. It is a save stamp, not an identity (finding 110 and finding 39 say the word is not an identity; both are true).
   - On load, the service takes the file that starts with `<slot>-` and compares its stamp with the listing record's stamp for the slot, before the load regenerates the live stamp. Match and another name (a stock rename): rename the file to the new name, then load. Mismatch (the slot was overwritten outside digikit, or a project was copied in on the +Drive): one short warning, the file is ignored.
   - Save as to another slot: the save hook sees the new slot and writes `<new slot>-<name>.dkp` from the current data.
6. **The module list: recorded at the stock save, restored after the load, by the service.** There is no offer and no menu change.
   - At a stock save the service writes the content modules that are loaded at that moment into the `.dkp` module list (tag 1: id and store hash of each): machines, effects and tool modules loaded from the store, not services, not platform modules, not the runtime. A save with modules loaded and no settings data still writes a `.dkp`: the module list is digikit data.
   - At a project load, after the service has installed the project's settings image (so the `init` of a module that loads now reads the project's values), it asks the runtime to live-load each listed module that is on the card and not loaded, one at a time, with the live job of E5c (`dkc_live(LIVE_LOAD, path)`, the job DIGIKIT > MODULES runs). Rules:
     1. Add only, never unload: a module that is loaded and not in the list stays.
     2. Transport playing: the runtime refuses a live load while playing. The service waits and applies at stop, with no try count (the rule of the settings reload).
     3. A listed module that is not on the card: one short warning in the firmware font and a log line. The project loads.
     4. A load the runtime refuses (does not fit, not compatible): a warning and a log line, then the next module. No restart offer: that flow stays in the menu.
     5. Order: settings first, then the loads. A later project change while loads are pending replaces the pending list with the new project's.
   - A load that ended well is added to the saved selection, as the menu saves the selection of a module it loads live (the runtime's live load does not touch the selection; `dkc_selection_add`). A store hash that differs from the file on the card loads the file on the card without a warning.
7. **A `project` service and an optional tool module.** Words and card paths: [taxonomy](../design/taxonomy.md).
   - `project` is a service (`services/project`, file `services/project.dks`, crate `dkrt-svc-project`). Its hooks on the stock project save and load are names in `devices/fw/project.toml` `[[hook]]` entries, resolved into the port file ([port-file](port-file.md)); they are not addresses. Before port step 3, the hook routines are per-device `.s` files in the service, as for lfo, machines and fx. Port step 3 moves them later. There are five hooks per device: slot setter, save exit, clear (one and many) and paste. There is no delete or format hook. The sites are in the "Firmware facts" table and [finding 110](../findings/110-project-identity.md) sections 4 and 5.
   - The service writes and reads `projects/<slot>-<name>.dkp` in the store, beside the stock files, by the rules of decision 5. The slot is three digits, `000` to `127`. The file name maps the bytes `c5 c4 d6 dc df c6 d8 c7 d1` and any byte not safe in a store path to `_`. The header holds device, slot, name and save stamp. A project gets a `.dkp` only when there is digikit data to keep.
   - Modules are told nothing. The service names the modules whose settings changed (`dkc_settings_changed`); the runtime reloads them with the E5 live job. The service restores the module list through the E5c live job (decision 6); the filter choices follow later.
   - The optional module `digikit/project-manager` has kind `tool`. It is UI only: it imports the exports `dk_project_current` and `dk_project_status` of the service (which also makes the service load when it is selected). The user selects it when wanted. In the first version it only listens. Later it lists `projects/*.dkp` and loads a project from the list through the stock load.
   - The service not selected: no service, no hooks, stock behaviour.
   - A `.dkp` holds the module list, the filter choices (extension-model E7, O6) and the entries of each module's settings that it wrote in this project (decision 2), for example the pool list of the sample picker (decision 3, step 5). FX routes and sends stay at device scope in the settings record for now (Open items).
   - Dependencies: E5c (live restore) and the port file stack (the hook sites). Order: after E8, as in the [taxonomy](../design/taxonomy.md) order.
   - The service writes the store with the SDK file calls (`dk_files_create`, `dk_files_write`, `dk_files_close`). They keep the card checks (`block_write`, finding 46: a foreign card refuses, a blank card is claimed first) and the atomic replace. See "Step 2 design".
   - A save during an upload: the write returns `E_BUSY` and the service retries later. See "Step 2 design".
   - The hooks only read stock state. Do not change stock project storage or the `Value<T>` project-data classes (ask Em before touching storage).
   - The warnings of the restore come from the service, not from `digikit-menu` or `digikit/project-manager`.
8. **Later, not in this plan.**
   - Elektron project data inside the `.dkp` (maybe, after this lands).
   - A workbench bundle export: Elektron project plus `.dkp` as one backup file (host side).
   - `project-manager` loads a project from its list through the stock load.

## Architecture

Words and card paths: [taxonomy](../design/taxonomy.md). Hooks and anchors: [port-file](port-file.md).

### Parts

| Part | Owns | Calls | Must not |
| --- | --- | --- | --- |
| SDK settings API (`dk_settings_open`, `dk_settings_get`, `dk_settings_put`, `dk_settings_remove`) | The one module-facing API for saved data. Unchanged. | The runtime. | Gain a project call or a project event. Let a module reach another module's keys. |
| Runtime | The settings record, keys `<owner>/<key>`. A settings overlay over module settings handles (generic: the project service is its only user): one optional pointer to the provider's table (`get`, `put`, `remove`), set when the service loads and cleared when it unloads; reads prefer the overlay, writes go to the overlay first and then the record. The reload of the modules whose settings changed (E5 live job). The live jobs. | The service's table, when the pointer is set. | Know the `.dkp` format, project names or hook sites. Make modules need the service. Put runtime keys (routes, selection) through the overlay. |
| `project` service | The `.dkp` files under `projects/`. The current project's data in RAM, as owner-prefixed entries. The five hooks (stock project setter, stock save exit, clear one and many, paste). The comparison that names the modules whose settings changed. The module list of the project and its restore (`ModuleRestore`). | Hook sites by name (port file). The store writer. `dkc_settings_overlay_set` and `dkc_settings_changed`. `dkc_module_path`, `dkc_store_find`, `dkc_live`, `dkc_live_state` and `dkc_selection_add` for the module list. | Change stock project storage or the `Value<T>` project-data classes. Call module code. Know individual modules beyond owner prefixes and ids. Unload a module. |
| `digikit/project-manager` (tool module, UI only) | First version: nothing beyond selecting the service. Later: a list of `projects/*.dkp`, then a load from the list. | The service's exports `dk_project_*`. | Write `.dkp` files. Call the stock project load in the first version. |
| `digikit-menu` | The module list, the selection and the live apply of DIGIKIT > MODULES. No project role. | The runtime live job. | Read `.dkp` files. Know projects. |
| Port file | The hook sites, resolved from `devices/fw/project.toml` `[[hook]]` entries, and their byte check. | Nothing. | Hold project logic. |

### Scope at open

- Module code is unchanged. The overlay is in the runtime ([digikit-project](../design/digikit-project.md) section 1): module-owned keys read the project first, write to project and record; the runtime's own keys stay device scope.
- A module does not need the service: it loads with or without it, and without it its settings are what they were.
- Limits equal the settings record: key 1 to 47 bytes, value at most 1024 bytes, the file at most 16 KiB.

### Data flow

```text
module put/get:  module -> dk_settings_put(h) -> runtime -+- settings record (last used value)
                                                          +- overlay -> service -> .dkp (RAM)
                 get: the project's value if the .dkp holds the key, else the record's

stock save:      stock save ends with success -> hook records slot -> service writes projects/<slot>-<name>.dkp
                 -> "saved" in the runtime log

stock load:      stock sets the project slot -> hook records slot -> service finds projects/<slot>-*.dkp,
                 compares the save stamp, renames the file if the project name changed
                 -> install image; service names the modules whose settings differ
                 -> runtime reloads those that opened settings (E5 job)
                 -> service: live job (E5c) loads the listed modules that are on the card and not loaded,
                    one at a time; missing or refused: warning and log line

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
- Archive rule: the service never deletes a `.dkp` for good. A file that no longer belongs to its slot moves to `projects/archive/<name>-<stamp>.dkp` (stamp as 8 hex digits): at a save into a reused slot (the existing file has another stamp) and at a stock clear. Stock paste copies the source `.dkp` to the destination slot. A stamp mismatch on load ignores the file and leaves it in place. A rename's old path (also when the rename of the current project changed the stamp: the file the bound project came from) and the loser of the two-file rule are superseded copies of the same project and may be removed. Design: [digikit-project](../design/digikit-project.md) section 5.

### Events

Save (hook at the single exit of the stock save, success only, slot 0x80 skipped):
1. The service records the loaded content modules in the module list of the image, then writes the `.dkp` from RAM, when there is digikit data (a module list with a module is data).
2. It writes a "saved" event to the runtime log.

Load (hook on the stock slot setter, on change):
1. No `.dkp` for the slot: nothing happens.
2. Stamp mismatch: one short warning in the firmware font. The `.dkp` is ignored.
3. Match: rename the file if the name changed. Read the sections. Install the image: the service names the modules whose settings differ and the runtime reloads those that opened settings. Write a "loaded" event to the runtime log.
4. The service restores the module list (and later the filter choices): for each listed module that is on the card and not loaded, one live load, one after the other, in a tick after the install. Add only.
5. A module missing from the card: one warning and a `PROJECT_MODULE` event (what 3). The project loads.
6. A load the runtime refuses (does not fit, not compatible): one warning and a `PROJECT_MODULE` event (what 4), then the next module. No restart offer.
7. Transport playing: the live load is refused while playing; the service asks again every second until the transport stops. A project change replaces the pending list. A load that ended well is added to the saved selection (`PROJECT_MODULE` what 2).

Clear (hook on the stock clear, single and multi-slot): the hook records each slot. The service archives the slot's `<slot>-*.dkp` to `projects/archive/<name>-<stamp>.dkp`. No module event.

Paste (hook on the stock paste): the hook records source and destination. The service copies the source `.dkp` to `<dest>-<name>.dkp`. No module event.

### Stock safety

1. Service not selected: no hooks, pure stock.
2. Service selected, the project has no `.dkp`: nothing happens. No popup, no warning, no change.
3. A `.dkp` with a stamp mismatch: one short warning, the file is ignored.
4. A hook refused at the port-file byte check: no service. Stock load and save are unaffected.
5. The hooks only read stock state. Stock project storage and the `Value<T>` classes are never changed.

### Open items

The one list of what is open. [O] marks an unchecked firmware fact.

1. [D] Does a stock rename keep the save stamp? The menu rename of the current project changes the stamp (seen on the emulator, DN2 1.12, [finding 110](../findings/110-project-identity.md) section 4); a rename of a non-current project keeps it (static read). No rename hook: the service follows by its binding. A save of the bound project into its own slot deletes the file it was loaded from or last written to, whatever its stamp, and archives the other files with another stamp. Open: a +Drive rename of the loaded project.
2. [O] Firmware facts of [finding 110](../findings/110-project-identity.md) section 5: whether format's erase loops all stay below the write guard `0x457FFF`; the clear and paste record reset on DN2 (traced on DT2 only; the DN2 paste site itself is [V]); stock move (cut), no callers found; what triggers load from memory (`0x40043f74`); power-down autosave, none found.
3. [O] The Syntakt sites of the project hooks (`devices/ports/st-1.42.toml`): the setter and the save exit are unchecked there.
4. Whether stock refuses duplicate names (no check seen; the slot in the file name makes this harmless).
5. FX routes and sends stay at device scope in the settings record unless Em wants them per project.
6. DT2 stock project load also resets the borrowed pool slots ([finding 109](../findings/109-dt2-stock-pool-borrowing.md)). The re-drive comes from the module (decision 3): it re-reads its list in `init` when the project change reloads it, and on pool lost. Check the order of the pool-lost handling and the settings reload.
7. Order of the settings reloads against the live job of the module list during a restore: the restore starts its first load a tick after the install, but the runtime's reload of the changed settings is queued by its own poll, so a load can take the live job first (a module that loads reads the project's values in `init` either way, and the reload waits behind it). A device test shows whether this matters.
8. A load from project-manager's list calls the stock project load: a later step that needs its own firmware facts (task, locks, a real progress callback).
9. Two limits of the restore are known and kept: two ids that map to one card file name (`a-b/c` and `a/b-c`), and a newer `.dkp` uploaded between the listing and the read, which binds the slot empty ([digikit-project](../design/digikit-project.md) "Module list and restore", Limits).

The service's RAM, measured: packed `project.dks` 55264 bytes (DT2 and DN2 alike), bss 38052 bytes in the committed object `modules/core/services/project/dks/build/project.dko.json` (two 16 KiB images and the state). Fallback 8 KiB files if that is too much.

## Step 2 design: the .dkp format and the write path

### Write path

- The service writes with the SDK file calls that every module can import: `dk_files_create` with root `DK_FILES_STORE` and flag `DK_FILE_REPLACE`, `dk_files_write`, `dk_files_close` (`sdk/include/dk/platform/files.h:46-60`). They run in an I/O step (`dk_io_post`). The runtime serves a service file like a content module here: the listing client of `dk_files_open` (init scope only), the I/O jobs, the store's write session and the tick all belong to the loader record, and the owner index is the record index for both kinds (a service file's record follows the module records). The ABI is unchanged, and there is no new core writer.
- The calls inherit the card checks (a foreign or read-only card refuses with `E_INUSE`; a blank range is claimed first, through `block_write`) and the atomic replace (`DK_FILE_REPLACE` keeps the old file whole until close swaps the table).
- The SDK path has its own buffer. It does not use the upload buffers. It shares the one store write session with SysEx upload, so a write during an upload returns `E_BUSY`.
- Reads use `dk_files_list` of `projects/` (prefix match on `<slot>-`) and `dk_files_read`, from the service's UI tick, never while a live job stages (the read stage is shared with the loader).
- The service has a UI tick: the `service!` macro takes a `tick =` entry (`glue::tick`).

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
- Size: a file holds at most 16 KiB. The service keeps two images in RAM: the live one (puts from the UI task) and a frozen copy that the I/O step writes (16-byte aligned). A put over the limit returns `E_SET_FULL`. Key and value rule breaches return `E_SET_KEY` and `E_SET_VALUE`, the same codes as module scope. The service's RAM is measured in "Open items".
- A bad CRC, another device or an unknown version on load: ignore the file and write one warning, as for a stamp mismatch.

### Rename and duplicates

- A stock rename: write the file under the new name, then delete the old one (for the current project also when the rename changed the stamp: the bound file is deleted, not archived). A cut between the two leaves two files with the prefix `<slot>-`. The loader keeps the one with the higher write counter and deletes the other.
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

Static reads on DT2 OS 1.17 and DN2 OS 1.12. [D] from one read, then checked by a second agent against disassembly: all rows confirmed, with the corrections included here. The hook sites (setter, save, save exit) are [V] in [finding 110](../findings/110-project-identity.md) section 4, with first bytes and displaced byte counts; the paste site is [V] on DT2 1.17, DN2 1.12 and DN2 1.11 (`devices/fw/project.toml`). The rows marked unchecked below are open items. Commands: `uv run python -m tools.coldfire.fw {decomp|disasm|callers|xref} ADDR -i dt2-1.17|dn2-1.12`.

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
| Per-slot listing record at `MmcFs + 0x10 + slot*0x1c` (slots 0..0x80, slot 0x80 the live mirror) [V]: byte 0 nonzero when the slot exists (the only test the accessors make), byte 1 is 1 for a `DT1P`/`DN1P` header and 0 for the in-memory magic 0xBEEFBACE, u32 +4 a record format word (5 on a cleared project) [D], name +8..+0x17 (no terminator written), save stamp +0x18 (copied from card header +0x18 by the listing refresh DT2 `0x4012f2fa` / DN2 `0x4012dbce`, which takes a mutex at `MmcFs + 0x1e034` (DT2) / `+0x1e030` (DN2) and also runs after each save). `MmcFs` comes from the getter `cf.project.fn.mmcfs` (DT2 `0x4019dbda`, DN2 `0x4018e0a8`). `ProjectCache` is a separate 0x20-byte object with no records. `FUN_4012f048(MmcFs, slot)` returns record+8 (or the static `0x44e406d4` when invalid) | `0x4012f048` | `0x4012d91c` |
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
| Clear, paste | No setter, no save (clear `0x400a131e`, `0x400a1376`; paste callback `0x400a12f8` on DT2, `0x4009b46e` on DN2, and the paste function it calls, DT2 `0x400f548e` / DN2 `0x400f6e6a`). Hooked. |
| Move (cut) | [O]: `0x400f539e` has no callers in the static read. |
| Power-down autosave | [O], none found. |

Correction to [finding 110](../findings/110-project-identity.md): `loadProjectFromMmc` does not call the setter; its wrapper does.

## Storage

- The settings record (8 KiB per copy, value at most 1024 bytes, shared with `fx/route/<id>` and `fx/send/<id>`) is enough for the last used values. It is too small for a project's data (the `.dkp`): one pool list is 16 paths of up to 159 bytes, about 2.6 KiB.
- The `.dkp` is a store file. Store format and table: [digikit-store](../design/digikit-store.md), code in `modules/core/runtime/sdk/src/store.rs`. The service writes `.dkp` files with the SDK file calls; no new core writer is needed. The format and the write path are in "Step 2 design".
- Write only on save, so card writes stay rare.

## ABI and versions

- No module-facing entry is added for the overlay. The exports `dk_project_current` and `dk_project_status` of the service (for `project-manager`) have `since = "0.11.0"`, the runtime minor of this change.
- The changed meaning of `sample_track.h:74` plus the new entry is a runtime 0.x MINOR (the runtime version at build time; module-architecture section 12).
- A store format change (the `.dkp` file) is also a MINOR.

## Steps

Estimate: about 1.5 days of agent time.

1. [x] Byte check of the firmware facts, both devices ([finding 110](../findings/110-project-identity.md) sections 4 and 5), except the [O] items listed under Open items. (1 h)
2. [x] Design doc [digikit-project](../design/digikit-project.md): the `.dkp` format and write path of "Step 2 design", events, error codes. Modules get no API. (1 h)
3. [x] Service `project` with five hooks per device, no delete or format hook: slot setter, save exit, clear one, clear many and paste ([finding 110](../findings/110-project-identity.md) sections 4 and 5). The service archives and copies `.dkp` files by the archive rule (Architecture "File name and identity"); it deletes none for good. Hook entries in `devices/fw/project.toml` `[[hook]]` (the paste site by the new anchor kind `cf_call`, found through its caller on both devices); routines in one `hooks.s` before port step 3. The service writes "loaded" and "saved" events to the runtime log after the stock load and save finish; device tests wait on them instead of the fixed 3.5 s and 6 s waits in `tests/regression/runtime/project-kit.ts`. Then the optional tool module `digikit/project-manager`. (0.5 day) Built on `svc/project` (host proofs only): `modules/core/services/project` (crate, `dks/`), `service!` with `tick =`.
4. [x] Runtime and service: the service writes the `.dkp` with the SDK file calls, reads it on project change, writes it on save; the runtime puts the project overlay over module settings (decision 2) through the `SettingsOverlay` table and reloads the modules whose settings changed. Module code is unchanged. (0.5 day) Built: `settings_overlay.rs`, `settings_reload.rs` and `settings.rs` in the runtime, `dkc_settings_overlay_set` and `dkc_settings_changed`, `dk_project_current` and `dk_project_status`; unit, contract and assembly tests; no device test yet (step 7).
5. [x] Pool: remove the per-project lists from `sample_tracks.rs`; the sample picker keeps its list with `dk_settings_put` (per project through the overlay) and re-reads it in `init`, which a project change reloads; it re-drives on pool lost. (3 h) Docs to update: sample-api-1.5 (lines ~45, 65, 116-118, 137-138, 168 and its TODO on store persistence), `sdk/include/dk/sample_track.h:74`, the `sample_tracks.rs` header comment and the tests calling `project_changed` (`sample_tracks/tests.rs`), module-architecture 3.10 and section 12, digikit-store "Keys by owner" and the line that only upload writes the store. Built: `sdk/src/sample/picker.c` keeps `pool.0` .. `pool.15` in the settings and `dk_sample_picker_init(pool, files, ui, settings)` restores them (sets, then clears); `Proj`, `project_changed` and the `PR` parameter are gone from `sample_tracks.rs`; `Core::lost` already reloads from the bound paths, so pool lost needs no picker code; host tests in `dkui-host` (`picker.rs`).
6. [x] Module list: recorded at the stock save, restored after the load by the service (decision 6), with no menu change. (2 h) Built: `engine/restore.rs` (`ModuleSet`, `ModuleRestore`); the list is tag 1 of the image (`BoundProject::record_modules` at a save, `ModuleRestore::replace` at a project change); core imports `dkc_module_path`, `dkc_store_find`, `dkc_selection_add` (with `dkc_live` and `dkc_live_state` from step 4); log kind `PROJECT_MODULE`; unit tests in `tests/service.rs`. The warnings are toasts (`dk_ui_open` and `dk_ui_toast`, optional imports of `core/ui`): `MODULE MISSING`, `DOES NOT FIT`, `NOT LOADED`, `NOT KEPT`; the assembly test loads the service file with both imports bound; `PROJECT_MODULE` carries the facts. The file search reads the whole store (`store::find_name`) and waits while a live job stages. No device test yet (step 7).
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
