# digikit project: module API, events, service and `.dkp` format

Status: design for step 2 of [the plan](../plans/digikit-project.md). Words and paths: [taxonomy](taxonomy.md). Store: [digikit-store](digikit-store.md). Hook sites: [finding 110](../findings/110-project-identity.md) section 4.

## 1. Module API

```c
/* Open the project scope of this module. Context: init. Since runtime: next minor. */
int32_t dk_settings_open_project(dk_module_ctx *ctx, dk_settings **out);
```

- Results as `dk_settings_open` (`sdk/sdk.toml:426-431`): `DK_OK`, `DK_E_NOT_FOUND` outside init, `DK_E_CONTRACT` for a bad context; `*out` is untouched on an error.
- Service not loaded: `DK_E_MISSING` (-73, `sdk/sdk.toml:2043`; absent services follow `sdk/include/dk/core/module.h:35-37`). No new code.
- The handle is a second `dk_settings` object, the same every time, valid until unload.
- `dk_settings_get/put/remove` (`sdk/sdk.toml:434-455`) act on the scope of the handle. The runtime passes module id and bare key; the service stores `<id>/<key>`.
  - Key 1-47 printable bytes including `<id>/`: else `DK_E_SET_KEY` (`sdk/sdk.toml:1734`).
  - Value at most 1024 bytes: else `DK_E_SET_VALUE`. `get` with a small `cap` returns it too.
  - File at most 16 KiB: `put` over it returns `DK_E_SET_FULL`, image unchanged.
  - No current project (slot -1 or 0x80): `get` gives `DK_E_SET_NOKEY`; `put` and `remove` give `DK_E_SET_OFF`.
  - A `put` of the stored value changes nothing and sets no dirty flag.
- Changes go into the RAM image at once and reach the file at the next successful stock save. A load without a save discards them.

`sdk.toml`: `[[import]] name = "dk_settings_open_project"`, `service = "runtime"`, `context = "init"`, `since` = next minor (runtime is 0.10.0, `sdk/sdk.toml:9`).

## 2. Events

`void dkm_project(dk_module_ctx *ctx, uint32_t event)`, context ui. `dkm_menu` is gone: the packer refuses it and entry points live in `dk_module` (`docs/design/digikit-dkm.md:71`, `packages/patchkit/src/dkm.rs:599`). That record has four fields and the header has no free word (`sdk/include/dk/core/module.h:47-53`, `packages/dkneeds/src/gen_limits.rs:78-92`). So `dkm_project` is an `EXPORTS` entry of the header table (`packages/dkneeds/src/keys.rs:25`), written by the packer from the C symbol (open item 4).

| Event | Value | Sent when |
| --- | --- | --- |
| `DK_PROJECT_LOADED` | 1 | The current project changed. The scope already holds the new data (empty without a `.dkp`). |
| `DK_PROJECT_SAVED` | 2 | A stock save succeeded and the write step ended or was deferred. |

- The service calls it from its tick (UI task) after the hook recorded the change. Hooks never call module code.
- Load order: read file, install image, LOADED to the modules loaded now, then start the live job. A module the job loads reads the installed image in `init` and gets no LOADED.
- Allowed inside: `dk_settings_*` on its handles, `dk_log`, UI calls. No blocking; `dk_files_*` writes are I/O-step only (`sdk/include/dk/platform/files.h:46-47`). A `put` in SAVED is written at the next save.

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
name = "cf.project.hook.clear"
entry = "project_hook_clear"
[[hook]]
name = "cf.project.hook.paste"
entry = "project_hook_paste"
```

- Four hooks per device: the slot setter, the save exit, the project clear (DT2 callbacks `0x400a131e` single, `0x400a1376` multi-slot; DN2 `0x4009b494`, `0x4009b4ec`) and the paste (DT2 `0x400f548e`; DN2 `0x400f6e6a` matched by caller shape only, to be matched by hash). No delete or format hook: a file that no longer belongs to its slot is archived at the next write to that slot (section 5). `devices/fw/project.toml` has no `[[hook]]` yet (`:4-72`). Step 3 adds them, for both devices, in the form of `devices/fw/lfo.toml:284-293`, and `cf.project.fn.slot_record` (DT2 `0x4012f048`, DN2 `0x4012d91c`).
  - `slot_set`: the setter, 6 bytes displaced, slot at SP+4 (finding 110:44).
  - `save_exit`: before the rts, inputs D0 and D3, keeps D2-D4/A2-A6 (finding 110:46).
- Routines are per-device `.s` files. They store only to the service's static `Pending { slot, ok }`. `save_exit` records only when D0.b is nonzero and D3 is not 0x80.
- `service!` gets `tick = path`: the macro sets `tick: None` today (`modules/core/runtime/svc-rt/src/lib.rs:380-385`); `ModuleDef` has the field (`lib.rs:228-234`).
- Tick, in order: (1) pending load: `dk_files_list` of `projects/` with prefix `<slot>-`, `dk_files_read` of the candidates, only while `dkc_live_op()` is 0 (`lib.rs:100`; the read stage is shared with the loader); (2) install, log, LOADED, start the live job; (3) pending save: freeze the image (16-byte aligned) and `dk_io_post` (`sdk/include/dk/platform/io.h:6-17`); (4) retry timer.
- I/O step (`files` from `dk_files_open` in init, `files.h:1-3`): `dk_files_create(files, DK_FILES_STORE, path, used, NULL, DK_FILE_REPLACE)`, one `dk_files_write`, `dk_files_close`; for a rename `dk_files_delete` of the old name. The old file stays whole until close (`files.h:53-56`).
- `DK_E_BUSY` or `DK_E_FILE_IN_USE`: return `DK_IO_MORE`, rerun after 1 ms (`files.h:64-66`); after 1000 reruns the step fails. The tick retries every second. After 30 s: one `PROJECT_DEFERRED`, image stays dirty, the next save writes. `E_INUSE`, `E_NOSTORE`, `E_FULL`, `E_WRITE`, `E_HASH` end the attempt with no retry loop; `PROJECT_SAVED` carries the code.
- Exports for `digikit/project-manager` (`provided = true`, `service = "project"`, `absent = "missing"`, as `sdk/sdk.toml:636-647`): `i32 dk_project_current(u32 *slot, u8 name[16])` (`DK_E_NOT_FOUND` without project); `u32 dk_project_status(void)` (bit 0 file loaded, 1 dirty, 2 write deferred, 3 file ignored). A list call is later.
- Opener forwarding: the runtime holds one `Option<&'static ProjectScope>`, a `repr(C)` table `{version, get, put, remove}` called in the UI task. The service sets it with a new `dkc_project_scope_set` in `dkrt_abi::core_imports` (`lib.rs:29`; changes `SVC_ABI`). The runtime clears it when the service record unloads, so no service `fini` is needed.

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
- Stamp equal, name different (stock rename): write under the new name with counter+1, then delete the old file. After a cut two files carry the prefix: the loader keeps the one that passes CRC and stamp with the higher counter and deletes the other. These removed files are superseded copies of the same project, not orphans.
- Stock New saves only the mirror slot 0x80 in `initializeNewProject`; the real slot is saved by the save wrapper, so the save exit hook sees the slot ([finding 110](../findings/110-project-identity.md) section 5). `0x40041c96` (DN2 `0x4003e38c`) is only a change notification, not a hook site.
- No orphan `.dkp` is deleted for good. A file that no longer belongs to its slot moves to `projects/archive/<name>-<stamp>.dkp` (stamp as 8 hex digits): (a) at a save into a slot whose existing file has another stamp (the slot was deleted, formatted or overwritten and a new project saved there): archive the old file, then write the new one; (b) at a stock clear (hook; clear keeps the stamp, so without the hook the cleared project would get the old modules back). The archive write uses the safe order of a rename: write the archive copy, then remove the old path.
- At a stock paste (hook) the service copies the source slot's `.dkp` to `<dest>-<name>.dkp` (paste copies stamp and name), so the copy keeps its digikit data. A stock move (cut and paste) gives the source the clear-like reset (archived) and the destination a copy.
- At a load with a stamp mismatch the file is ignored and left in place (no restore).
- Purpose of the archive: when the `.dkp` later holds the Elektron project content as well, project-manager can list `projects/archive/` and rebuild a project into a slot.
- Stamp different: ignore all files of the slot, leave them on the card. Save-as writes `<new slot>-<name>.dkp`. A file is written only when the module list is not empty or a section exists; no `.dkp` means no digikit data.

## 6. Errors and log events

No new status code. Used: `DK_E_MISSING`, `DK_E_SET_*` (`sdk/sdk.toml:1734-1738`), `DK_E_BUSY`, `DK_E_HOOK_STOCK` (-82, `sdk/sdk.toml:2024`: the service is refused before init).

New `[[log.kind]]` ids 48-51 (last id is 47, `sdk/sdk.toml:1625`):

| Name | Args | Emitted |
| --- | --- | --- |
| `PROJECT_LOADED` | slot, bytes, modules, counter | After the install, also for a slot without file (bytes 0). |
| `PROJECT_SAVED` | slot, bytes, counter, result | When the write step ends. |
| `PROJECT_IGNORED` | slot, reason (1 CRC or format, 2 device, 3 version, 4 stamp) | When a file is not used; one toast with `dk_ui_toast` when core/ui is present (`sdk/sdk.toml:285-292`). |
| `PROJECT_DEFERRED` | slot, last status, seconds | Once after 30 s of busy store. |

Device tests wait on LOADED and SAVED, not on fixed times.

## 7. Invariants and proofs

| Invariant | Proof |
| --- | --- |
| Service not selected: stock bytes at both sites; opener gives `DK_E_MISSING`. | Assembly: image without the service, compare site bytes. |
| No `.dkp`: no toast, write or delete; empty image. | Unit with fake store; device: no popup. |
| Stamp mismatch: one warning, files untouched. | Unit. |
| Hook refused at the byte check: no service, stock works. | Assembly with a changed site byte: `E_HOOK_STOCK`. |
| Hooks write only the service's `Pending` and call nothing. | Assembly: run routines over fake firmware memory and a recording fake core. |
| Write only after a successful save, never slot 0x80. | Unit: D0.b zero, slot 0x80. |
| A save never waits and loses no data. | Unit: busy fake store gives the deferred event; next save writes. |
| A cut during the write keeps the old file. | Unit: abort mid-write; device: save during an upload ends with the file. |
| Load without save discards puts. | Unit. |

## 8. Open items

1. Does a stock rename keep the save stamp? One emulator check; if not, hook the rename callback (DT2 `0x400a1246`, DN2 `0x4009b3bc`).
2. RAM: two 16 KiB images are 32 KiB; fallback 8 KiB files. Measure the service BSS.
3. `dk_files_read` has `context = "init"` (`sdk/sdk.toml:397`) but the tick reads. Relax it to `ui` in the same minor.
4. Confirm the packer and loader accept `EXPORTS` in a content module, and name the runtime call that gives the service the entry of a loaded module.
5. Confirm the listing record holds the new name and stamp at the first tick after a save.
6. Firmware rows are in finding 110 section 5 (New, unload, delete, clear, format, load, paste, save-as). Still open: the DN2 paste and clear record reset (traced on DT2), stock move (no callers found), what triggers `loadProjectFromMemory` (`0x40043f74`), and whether format's later erase loops stay below the write guard `0x457FFF` (finding 59).
7. Hook `mode` names for both entries, from `devices/fw/lfo.toml`.
