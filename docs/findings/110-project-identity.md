# 110. Project identity for digikit stores (DT2 1.17, DN2 1.12)

Trigger: digikit stores data per project and needs a stable project identity. Results are read once from the images. No device run. Marks: **[V]** verified here, **[D]** measured or read once, **[O]** open.

## 1. The current project slot **[D]**

The current project slot (0..127, card slot `0x58000 + s*0x8000`, [finding 23](23-digikit-runtime-spikes.md)) is one global byte.

| Device | Byte | Setter | Notes |
| --- | --- | --- | --- |
| DT2 | `0x4099d5c5` (change signal `0x4099d5c4`) | `0x400c11ca(slot)` | writes only on change, notifies via `0x400c102c` |
| DN2 | `0x405ce899` | `0x400bb7fa` | `0x4003eb62` pairs `0x400424a2` |

On DT2 the setter is reached from `0x400424a2(Project*, slot)` (8 callers) and from `project_container_decode` `0x400c0d90`. On DN2, `0x400424a2` has 8 callers.

DT2 project paths:

| Function | Address |
| --- | --- |
| `Project::loadProjectFromMmc(slot)` | `0x40046906` (wrapper `0x40046c38`, from ProjectManager `0x400a227a`) |
| `Project::initializeNewProject(slot)` | `0x400465e2` |
| `Project::loadProjectFromMemory` | `0x40043f74` |

All three call `0x400424a2`, then `0x40042522(Project*, 0)`. The DN2 addresses of `loadProjectFromMmc` and `initializeNewProject` are **[O]**.

## 2. No stronger identity **[D]**

- There is no UUID or hash field. The header word at slot offset 0x18 is not an identity ([finding 39](39-digikit-boot-tests.md)).
- The slot byte alone is weak: delete, overwrite, save as, load from and save to can reuse a slot.
- A key of (device, slot, project name) is safer. The RAM name accessor is **[O]**.

## 3. What digikit exposes today **[D]**

digikit has no project identity and no project-change event. It has only the `devices/fw/project.toml` singleton, `active_kit`, `track_type` and `serialize`. `dkrt_track_type` returns -2 when there is no project.

`ReloadAllSamples` callers are not a project signal. Most are kit, boot or repair paths ([finding 109](109-dt2-stock-pool-borrowing.md)).

## 4. Hook sites for the project service **[V]**

Both checks were done: a first read and a second check against image bytes.

| Site | DT2 1.17 | DN2 1.12 | First bytes | Displaced | Notes |
| --- | --- | --- | --- | --- | --- |
| Slot setter | `0x400c11ca` | `0x400bb7fa` | `73 39 <abs.l>` = `mvs.b (abs).l,D1` | 6, plain copy | same function on both (strict hash d9416ff018bb), size 50; slot argument at SP+4 (cdecl long); callers DT2 `0x400424a2`, `0x400c0d90` (project_container_decode); DN2 `0x4003eb62`, `0x400bb3c0` |
| Save | `0x40043a30` | `0x40040190` | `4f ef ff cc 48 d7 7c 1c` (`lea -0x34(SP),SP`; `movem.l D2-D4/A2-A6,(SP)`) | 8, plain copy | same function on both (strict hash cdc5ea1e1c60), size 248; stack args SP+4 Project*, SP+8 slot, SP+0xc progress; 4 callers each: DT2 call sites `0x40043b34`, `0x40043b5a`, `0x40043bea`, `0x40046c8a`; DN2 `0x40040294`, `0x400402ba`, `0x4004034a`, `0x40042ec0`. Real slot saves come through DT2 `0x40043b28` / DN2 `0x40040288` (save-as and initializeNewProject); the others save the mirror slot 0x80 |
| Save exit | `0x40043b1e` | `0x4004027e` | `4c d7 7c 1c 4f ef 00 34` then `4e 75` | 8, plain copy (the rts stays) | save + 0xee, the only rts; success: D0.b nonzero from the card write (DT2 `0x400f5150`, DN2 `0x400f6b2c`); failure paths `clr.b D0`; the slot is in D3 at the exit. A `before` hook needs D0 and D3 in its inputs and must keep D2-D4/A2-A6 |

**[D]** facts (read once, not second-checked): the project name holds at most 16 bytes (`pea 0x10` before the name entry `0x40117c70` at save-as `0x400a29c6` and PROJECT NAME `0x400a322e`, DT2). Rename `0x400a1246` showed no compare against other slots' names.

**[O]**:
- The name character set (candidate table DT2 `0x4025dee6`, DN2 `0x40236f00`, link to the name entry not shown).
- Whether rename keeps the save stamp.
- What the card write returns as nonzero.
- The stock load call for a later step (DT2 `0x40046906`, wrapper `0x40046c38`; DN2 `0x40042b92`, wrapper `0x40042e6e`).
