# 110. Project identity for digikit stores (DT2 1.17, DN2 1.12)

Trigger: digikit stores data per project and needs a stable project identity. First read once from the images. Then, on 2026-11-16, a second check of every address against the image bytes of both devices (`m68k-linux-gnu-objdump`, `tools/coldfire/refscan.py`) and PC probes in `digi-emud` on stock images with a card image (`pc.probe.set`, ColdFire only). That check is step 1 of [docs/plans/digikit-project.md](../plans/digikit-project.md). Marks: **[V]** checked against the bytes by a second agent, **[D]** measured or read once, **[C]** corrects an earlier claim, **[O]** open.

## 1. The current project slot **[V]**

The current project slot (0..127, card slot `0x58000 + s*0x8000`, [finding 23](23-digikit-runtime-spikes.md)) is one signed global byte; `0xff` (-1) means no slot.

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Slot byte | `0x4099d5c5` | `0x405ce899` |
| Setter `(slot)`: writes only on change, then notifies | `0x400c11ca`, notifies via `0x400c102c` | `0x400bb7fa`, notifies via `0x400bb65c` **[C]** (both devices notify) |
| Setter wrapper `(Project*, slot)`: `if Project::vfunc(0x28) != 0 then setter(slot)` | `0x400424a2` | `0x4003eb62` |
| Getter `(Project*)`: -1 when `vfunc(0x28)` is 0 | `0x400424ca`, reads the byte through `0x400c1238` | `0x4003eb8a`, through `0x400bb868` |

Callers of the setter: the wrapper and `project_container_decode` (DT2 `0x400c0e2c`, DN2 `0x400bb45c`, slot -1). Callers of the wrapper (8 on each device):

| Caller | DT2 | DN2 | Slot |
| --- | --- | --- | --- |
| Save wrapper, before and after the write | `0x40043ba0`, `0x40043c00` | `0x40040300`, `0x40040360` | the target |
| `loadProjectFromMemory` | `0x400441d0` | `0x40040632` | -1 |
| `initializeNewProject` | `0x40046820` | `0x40042af6` | its argument |
| Load wrapper | `0x40046ca0` | `0x40042ed6` | the loaded slot |
| Project manager DELETE (the wrapper as a function pointer) | `0x400a13fc` in `0x400a13f0` | `0x4009b572` | -1 |
| A card routine, after `0x40130378` (DN2 `0x4012e9c4`) succeeds | `0x401314ee` | `0x4012f878` | -1 |
| Near the main-OS entry, after a name write (not reached in the boots below) | `0x40033666` | `0x4002ec26` | 0 |

## 2. Project routines **[V]**

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| `Project*` global (set by) | `0x44f370a0` (`0x40198b7e`) | `0x446488d0` (`0x4018ac5a`) |
| Live `project_t*`: `*(Project+0x10)`, read by `Value<project_t>::vfunc_10` | `0x401a61ea` | `0x40195b5a` |
| Name writer `(Project*, src)`: copies 16 bytes to `live+0xc`, notifies with flag 0x20 | `0x40042140` | `0x4003e800` |
| Stamp regenerator `(Project*)`: `live+8` = new stamp, notifies with flag 0x80 | `0x40041c96` | `0x4003e38c` |
| Save `(Project*, slot, progress)`, nonzero on success | `0x40043a30` | `0x40040190` |
| Save with slot check (slot > 0x80 refused) | `0x40043b28` | `0x40040288` |
| Save / save-as wrapper `(Project*, slot or -1, progress)` | `0x40043b78` | `0x400402d8` |
| `loadProjectFromMmc` (does not call the setter) **[C]** | `0x40046906` | `0x40042b92` |
| Load wrapper: load, save slot 0x80, setter, clear the flag of section 4 | `0x40046c38` | `0x40042e6e` |
| `initializeNewProject(Project*, slot)` | `0x400465e2` | `0x400428c0` |
| `loadProjectFromMemory(Project*)` | `0x40043f74` | `0x400403d6` |

The save wrapper runs the setter before the write only when a slot is given (not -1); after the write it sets the same slot again, a no-op. After a failed save it returns 0 and runs neither the second setter nor the flag clear. `initializeNewProject`, `loadProjectFromMemory` and `loadProjectFromMmc` each call the stamp regenerator.

## 3. The save stamp is not random across boots **[D]**

The stamp regenerator builds the stamp from two calls of an LCG: `state = state * 1103515245 + 12345`, output `(state >> 16) & 0x7fff` (DT2 `0x401524a0`, state `0x4099d688`; DN2 `0x40150950`, state `0x405ce95c`). The stamp is `(first << 16) | second`, so bits 31 and 15 are always 0.

Nothing seeds the state. The seed setter (DT2 `0x401524ea`, DN2 `0x4015099a`) has no reference in either image: no call, and its address appears nowhere in the image, as code or as data. The state starts at 0 on every power-on. The emulator agrees: from a zero state, outputs 4 and 5 of the LCG give `0x56650daa`, the stamp the boot wrote on both devices, and outputs 6 and 7 give `0x421f3ead`, the stamp after the next load or new project.

Other code draws from the same LCG (11 references on DT2, 9 on DN2: random functions of the UI), so a stamp depends on how many draws ran since power-on. Two different projects, each loaded or created as the first action after a boot and then saved, get the same stamp. The stamp tells a stale save from a newer one in the same session, but it is not an identity across boots.

## 4. A project flag that save and load clear **[D]**

A second byte after the slot byte (DT2 `0x4099d5c6`, DN2 `0x405ce89a`) has its own setter: DT2 `0x400c11fc`, DN2 `0x400bb82c`. It stores 0 or 1 and notifies like the slot setter. `FUN_40042522(Project*, flag)` sets it: DT2 `0x40042522`, DN2 `0x4003ebe2`.

- **Cleared (0):** by the save wrapper, the load wrapper, `initializeNewProject`, `loadProjectFromMemory` and `project_container_decode`.
- **Set (1):** by its only caller with 1: `0x40063d8c` on DT2 (the call at `0x40063da4`), the call at `0x4005e116` on DN2.
- **In the emulator:** renaming the current project set it, and CREATE NEW left it at 1.

That fits an unsaved-changes mark. Turning encoders in the emulator did not set it, and the values did not change on screen, so this run cannot say whether parameter edits set it. Whether stock asks to save before LOAD PROJECT is **[O]** too.

## 5. When the setter runs (emulator, stock images) **[D]**

Runs on DN2 1.12 and DT2 1.17 in `digi-emud`, with stock firmware and a card image made by `mise run dev:plusdrive -- build`. PC probes were on every routine of sections 1 and 2. Each action ran from a fresh boot, except where the table names one.

| Action (SETTINGS > PROJECT) | Setter | What runs | Devices |
| --- | --- | --- | --- |
| Boot with no project on the card | -1 | decode, then the regenerator (stamp 0) and a background save of slot 0x80 | both |
| SAVE PROJECT AS to an empty slot | the target, before and after the write | name writer, save wrapper, save(slot), save(0x80) | DN2 |
| LOAD PROJECT, or MANAGE > LOAD FROM | the loaded slot | load wrapper, `loadProjectFromMmc`, regenerator, save(0x80) | DN2 |
| LOAD PROJECT > CREATE NEW | -1 | `initializeNewProject(Project*, -1)`, regenerator, save(0x80) | both |
| MANAGE > SAVE TO, empty slot, no slot current | the target, before and after | save wrapper | DT2 |
| MANAGE > SAVE TO, empty slot, slot 0 current | DN2: the target, before and after (save wrapper). DT2: none; `0x400a1376` writes the card slot through `0x400f5150`, and slot 0 stays current | | both (paths differ, **[O]**) |
| MANAGE > RENAME of the current project | none | `0x4009b3bc` (DN2), name writer; the section 4 flag becomes 1 | DN2 |
| MANAGE > DELETE of the current project | -1 | `0x400a13f0` (DT2), the wrapper | both |
| MANAGE > DELETE of another slot | none | listing refresh of that slot (`0x4012ecb6`) | DN2 |
| MANAGE > CLEAR of the current project | none | listing refresh of the slot; the live project keeps its name and stamp | DN2 |
| MANAGE > INIT NEW on an empty slot | none | listing refresh of the slot; `initializeNewProject` does not run, the live project is unchanged | DN2 |

On both devices the project manager's menu for a filled slot is CLEAR, DELETE, RENAME, LOAD FROM, SAVE TO, TOGGLE (lock), plus PURGE ALL for the current one. For an empty slot it is INIT NEW, SAVE TO, TOGGLE.

What the table means for per-project data:
- CLEAR, INIT NEW, DELETE of another slot and the DT2 SAVE TO copy change a card slot without the setter. Data kept per slot outside the project file goes stale without an event.
- In a save-as the setter runs before the write, with the target slot. A listener on the setter sees the change before the project is on the card.

## 6. Still open **[O]**

- What makes SAVE TO copy the card slot (DT2 `0x400a1376`) rather than save the live project, and whether DN2's twin `0x4009b4ec` does the same.
- The setter call near the main-OS entry (DT2 `0x40033666`): which boot or reset path reaches it.
- The card routine before DT2 `0x401314ee` (`0x40130378`, DN2 `0x4012e9c4`): likely FORMAT; not run.
- +Drive browser rename (DT2 `0x400e839a`, DN2 `0x400e9d6a`), copy and move: not run.
- Power-down autosave: none found.
- Whether a parameter change sets the section 4 flag, and whether stock asks to save before LOAD PROJECT.

## 7. What digikit exposes today **[D]**

digikit has no project identity and no project-change event. It has only the `devices/fw/project.toml` singleton, `active_kit`, `track_type` and `serialize`. `dkrt_track_type` returns -2 when there is no project.

`ReloadAllSamples` callers are not a project signal. Most are kit, boot or repair paths ([finding 109](109-dt2-stock-pool-borrowing.md)).
