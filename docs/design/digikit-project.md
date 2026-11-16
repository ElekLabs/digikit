# digikit project: the .dkp file, the module API and the events

Status: **draft for Em's review**: step 2 of [docs/plans/digikit-project.md](../plans/digikit-project.md); no code yet. It fills in what step 2 owes: the `.dkp` format, the C signatures, the events, the error codes and the size limits. It follows the plan's decisions as amended by [runtime-and-services](../plans/runtime-and-services.md) section 12: `dk_data_*` become imports, "project changed" an event table entry, and the feature preferably a loadable service. Firmware facts: [finding 110](../findings/110-project-identity.md). Where this draft proposes something the plans do not decide, it says **Proposal**, and the decisions for Em are listed at the end.

## What the firmware gives us

From finding 110 (both devices, emulator-checked where marked there):

- **Two hooks give us the events.** The slot setter fires on every change of the current slot: load, new, save-as, delete of the current slot, load from memory, boot. The save returns nonzero on success.
- **Save-as runs the setter before the write.** The new slot is set first, then the project is written. A listener on the setter alone would load the target slot's old `.dkp` for a project that is about to overwrite it.
- **Some actions change a card slot without the setter.** These are CLEAR, INIT NEW, DELETE of another slot, and SAVE TO when it copies the card slot (DT2 `0x400a1376`). The `.dkp` of that slot goes stale with no event.
- **The save stamp repeats across boots.** It comes from an LCG that nothing seeds, so it tells saves apart within a session, not across boots (finding 110, section 3).
- **A new project and a project loaded from memory have slot -1.** Their data has no file until they are saved to a slot.

## Hooks

**Proposal: a loadable service, `project.dks`,** that owns these hook sites (R12 services install their own `jmp.l` hooks at boot), in place of plan decision 7's `core/adapters/project`. A selection with no module that uses project data does not load it. The runtime's unused `dkrt_project_changed` can go as runtime-slimming plans.

| Hook | DT2 1.17 | DN2 1.12 | The service sees |
| --- | --- | --- | --- |
| Slot setter, on change (arg: slot, -1 none) | `0x400c11ca` | `0x400bb7fa` | changed(slot) |
| Save wrapper, entry and exit | `0x40043b78` | `0x400402d8` | saving(slot), save done |
| Save, exit (skip slot 0x80; only a nonzero result) | `0x40043a30` | `0x40040190` | saved(slot) |

**Proposal: also hook the save wrapper.** The plan names two hooks (setter and save). The wrapper hook marks a save-as, so the service can tell a save-as from a load:

- **Inside the wrapper,** a setter change does not read the target's `.dkp`. The project's data in RAM stays, and its slot becomes the target.
- **The save inside the wrapper** writes the `.dkp`.
- **Outside the wrapper,** a setter change is a project change: the service reads the new slot's `.dkp`.

Without the wrapper hook, the service would have to guess from the stamp. That fails, because a save-as keeps the live stamp, which differs from the target's old `.dkp`, and the plan's mismatch rule would then warn on every save-as.

The hooks only read stock state (plan decision 7). They run in the task that called the stock routine (the UI task in every run of finding 110). The setter is also reached from `project_container_decode` at boot, before the main-OS entry where services install their hooks: the service reads the slot byte once when it starts instead.

## The `.dkp` file

**Path:** `project/<device>-<slot>.dkp` in the store (store paths are relative to `digikit/`), `<slot>` as three digits: `project/dt2-004.dkp`, 19 bytes, inside the store's path rules.

**Format** (version 1; all integers big-endian, like the settings record):

```text
header (64 bytes)
  0   4   magic "DKPJ"
  4   2   u16 version = 1
  6   2   u16 device       the profile id (devices/<device>.toml `id`)
  8   1   u8  slot         0..127
  9   3   zero
  12  16  name             the live name at the save, as stock keeps it (16 bytes, zero padded, no NUL guaranteed)
  28  4   u32 stamp        the live stamp at the save (live+8)
  32  4   u32 seq          incremented on every write of this file
  36  4   u32 used         bytes of the file: header, module list, sections (64 to the limit)
  40  4   u32 crc32        zlib's crc32 over bytes 0..40 and 44..used
  44  2   u16 module_count
  46  18  zero
module list: module_count entries of 40 bytes
  0   32  module id, NUL padded (a `.dkm` name is at most 31 characters)
  32  4   u32 content hash of the module file (the store table's hash slot)
  36  2   u16 module version (major << 8 | minor)
  38  1   u8  flags        bit 0: the module was selected; bit 1: the module is needed (open question 1 of the plan)
  39  1   zero
sections: from the end of the module list to `used`
  entries in the settings record's entry format, u8 key_len | key | u16 value_len | value,
  the key `<module id>/<key>` (1 to 47 bytes, printable ASCII)
```

A file is **valid** when its magic, version, device, slot, `used`, crc32 and entries hold (every entry inside `used`, every key legal, no key twice). An invalid file is treated as absent and reported once (`PROJECT_LOAD` state 3).

**Size: Proposal: at most 16 KiB per file.** That holds 8 modules (320 bytes) and, for example, 16 pool paths of 159 bytes (about 2.6 KiB) per module for several sampler modules. A `put` that would grow the file past the limit answers `E_PRJ_FULL`. The file is written in one piece, so the service keeps one 16 KiB buffer.

**Write:** the service writes the whole file through the store writer (step 2's open question; see "Decisions for Em"), only on saved(slot) for slot 0..127, and only when the save succeeded. A write that fails is logged (`PROJECT_WRITE` error) and not retried: the data stays in RAM and is written at the next save.

## The current project's data in RAM

| Event | What the service does |
| --- | --- |
| Service start (after the main-OS entry) | Reads the slot byte; for a slot, reads its `.dkp` as for a load |
| `changed(slot)` outside a save | Reads `<slot>.dkp` (none: empty data). Then the checks below, then the module event |
| `changed(slot)` inside a save | Keeps the data; the slot becomes `slot` |
| `changed(-1)` (new, delete of the current slot, load from memory) | Empty data, slot -1; the module event |
| `saved(slot)` | Sets name and stamp from the live project, `seq + 1`, writes `<slot>.dkp` |

Data of a project with slot -1 lives only in RAM until a save-as gives it a slot. A module that is not loaded keeps its section: the service rewrites sections it does not own unchanged.

**Checks on load (plan decision 5, with Proposals).** A `.dkp` belongs to the loaded project when:

1. **Its stamp equals the slot's listing stamp** (`MmcFs + 0x10 + slot*0x1c + 0x18`, the stamp the card holds; the load regenerates the live stamp, not this one).
2. **Its name equals the listing name. Proposal.** The stamp alone repeats across boots (finding 110). The name check catches most of the remaining cases: another project saved to this slot under stock firmware, with a stamp that happens to repeat.

On a mismatch the service ignores the file. It does not delete it, and it shows no warning (Proposal: a stale file is normal after CLEAR, INIT NEW or a stock save, so a warning there would be noise). When only the name differs and the stamp matches, plan decision 5 says the project was renamed: the service updates the name and keeps the data. With check 2 in place, a stamp match plus a name mismatch is ambiguous; this draft follows the plan and treats it as a rename.

**Modules (plan decision 6).**
- **A module in the list but not on the card:** one line in the firmware font, `MISSING <module>`, and the plain Elektron project loads.
- **A module on the card but not selected:** the DIGIKIT menu offers `LOAD + RESTART`, one button. It writes the selection and restarts.

## The module API (imports in `sdk/sdk.toml`)

Project scope mirrors the settings handle a module already has (`dk_settings_open`, `_get`, `_put`, `_remove`): module scope **is** that API, so plan decision 2's `dk_data_*(scope, ...)` becomes one new handle type.

```c
/* init: this module's project data; *out is the same handle every time. 0, DK_E_NOT_FOUND outside init, DK_E_CONTRACT. */
i32 dk_project_open(dk_module_ctx *ctx, dk_project **out);
/* ui: the key's value in the current project's data (own key space); its length, or DK_E_SET_NOKEY, DK_E_SET_VALUE, DK_E_SET_KEY, DK_E_PRJ_OFF. */
i32 dk_project_get(dk_project *p, const char *key, void *out, u32 cap);
/* ui: 0 to 1024 bytes; held in RAM and written at the next stock save. 0, DK_E_SET_KEY, DK_E_SET_VALUE, DK_E_PRJ_FULL, DK_E_PRJ_OFF. */
i32 dk_project_put(dk_project *p, const char *key, const void *value, u32 len);
/* ui: 0, also when the key was not there; DK_E_SET_KEY, DK_E_PRJ_OFF. */
i32 dk_project_remove(dk_project *p, const char *key);
/* ui: the current project: slot (0..127, -1 none) and its live name (16 bytes, as stock keeps it). 0 or DK_E_PRJ_OFF. */
i32 dk_project_info(dk_project *p, i32 *slot, char name[16]);
```

- **Errors.** Keys and values follow the settings record's rules and reuse its codes. Two codes are new: `E_PRJ_FULL` (the file would pass its limit) and `E_PRJ_OFF` (no store, a store that is off, or the service is not loaded). They take the next free numbers in `sdk.toml`.
- **Versions.** Each import carries `since` of the runtime minor that adds it (runtime-and-services section 12). The store gains a file type, also a minor.
- **The event.** "Project changed" and "project saved" are entries of the ordered event tables (runtime-and-services, "Ordered event tables"), `{event, order, fn}` records, with `fn` taking a `const dk_project_event *`:

```c
typedef struct {
    u8 kind;        /* DK_PROJECT_CHANGED 1, DK_PROJECT_SAVED 2 */
    i8 slot;        /* 0..127, or -1 */
    u8 from_file;   /* CHANGED: 1 when a .dkp was taken for this slot */
    u8 reserved;
} dk_project_event;
```

**Proposal:** if projects land before the event tables, use a named module export, `dkm_project`, like `dkm_menu`, and move it into the table with the other events.

- **When the events run.** `CHANGED` comes after the service has read the new data, so `dk_project_get` already sees it. `SAVED` comes after the file is written. Both run in the UI task.

## Log events and capacity

- `PROJECT_LOAD`: state 0 no file, 1 taken, 2 ignored (stamp or name), 3 invalid; argument: slot.
- `PROJECT_WRITE`: 0 written, or the store writer's error; argument: slot.
- CAPACITY kind `PROJECT`: limit 16384; used: the current data's file size.

## Tests (plan step 7)

- **Host:** the format's vectors (`modules/core/runtime/vectors/project-vectors.json`: valid files and one refused file per rule, like the settings vectors), and the RAM rules of the table above as unit tests over a fake store.
- **Emulator, both devices:**
  - save-as to a slot, then load: the data comes back;
  - save-as over a slot that has an older `.dkp`: no warning, the new data is written;
  - CREATE NEW: empty data, slot -1, then save-as writes it;
  - CLEAR of the current slot, then load: the data is not taken (this needs the stamp a cleared slot gets, which finding 110 leaves open);
  - a project whose module is missing: the line is shown and the project loads.

## Decisions for Em

1. **A service, not an adapter.** Should `project.dks` own the hooks (runtime-and-services' recommendation), or should `core/adapters/project` (plan decision 7)?
2. **The save wrapper as a third hook.** Is a save-as told apart this way, or another way?
3. **Identity: stamp plus name, or stamp alone.** Option: the service seeds the stock LCG once at boot from a free-running counter. Stamps would then differ across boots, but stock's random functions would no longer repeat across boots: a change to stock behaviour.
4. **No warning on a stale file.** Or one line, as for missing modules?
5. **The 16 KiB limit and one RAM buffer.**
6. **The store writer the service uses** (plan, decision 7: `store_upload`'s writer through a declared API, or a store-write API in the core).
7. **Card-only actions** (CLEAR, INIT NEW, DELETE of another slot, the DT2 SAVE TO copy). Leave their `.dkp` stale and rely on the checks, or hook the card write (`0x400f5150` on DT2) to delete or copy the file?
