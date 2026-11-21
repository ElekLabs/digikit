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

Both checks were done for DT2 1.17 and DN2 1.12: a first read and a second check against image bytes. The Syntakt sites (setter, save exit) are [O]; the paste and clear exit sites (section 5) are [V] on DT2 1.17 and DN2 1.12, the paste exit also on DN2 1.11 (`devices/fw/project.toml`).

| Site | DT2 1.17 | DN2 1.12 | First bytes | Displaced | Notes |
| --- | --- | --- | --- | --- | --- |
| Slot setter | `0x400c11ca` | `0x400bb7fa` | `73 39 <abs.l>` = `mvs.b (abs).l,D1` | 6, plain copy | same function on both (strict hash d9416ff018bb), size 50; slot argument at SP+4 (cdecl long); callers DT2 `0x400424a2`, `0x400c0d90` (project_container_decode); DN2 `0x4003eb62`, `0x400bb3c0` |
| Save | `0x40043a30` | `0x40040190` | `4f ef ff cc 48 d7 7c 1c` (`lea -0x34(SP),SP`; `movem.l D2-D4/A2-A6,(SP)`) | 8, plain copy | same function on both (strict hash cdc5ea1e1c60), size 248; stack args SP+4 Project*, SP+8 slot, SP+0xc progress; 4 callers each: DT2 call sites `0x40043b34`, `0x40043b5a`, `0x40043bea`, `0x40046c8a`; DN2 `0x40040294`, `0x400402ba`, `0x4004034a`, `0x40042ec0`. `initializeNewProject` (DT2 `0x400465e2` / DN2 `0x400428c0`) saves only the mirror slot 0x80 (`pea 0x80` at DT2 `0x400468ce`, jsr `0x40043b28` at `0x400468d4`; DN2 `0x40042b56` / `0x40042b60`). The real slot is saved by the save wrapper DT2 `0x40043b78` / DN2 `0x400402d8`: S(slot) at DT2 `0x40043ba0` (only for slot >= 0), real save `0x40043b28`(slot) at `0x40043bc0`, mirror `0x40043a30`(0x80) at `0x40043bea`, S(slot) at `0x40043c00` (DN2 `0x40040300`, `0x40040320`, `0x4004034a`, `0x40040360`) |
| Save exit | `0x40043b1e` | `0x4004027e` | `4c d7 7c 1c 4f ef 00 34` then `4e 75` | 8, plain copy (the rts stays) | save + 0xee, the only rts; success: D0.b nonzero from the card write (DT2 `0x400f5150`, DN2 `0x400f6b2c`); failure paths `clr.b D0`; the slot is in D3 at the exit. A `before` hook needs D0 and D3 in its inputs and must keep D2-D4/A2-A6 |

**[V]** (read by one agent, confirmed against image bytes by a second):
- Slot byte `cf.project.value.slot_byte`: DT2 1.17 `0x4099d5c5`, DN2 1.12 `0x405ce899`, DN2 1.11 `0x405cd899`. The addresses are checked; they are the operand of the setter's `mvs.b`, and the setter writes it with a 1-byte copy only on a change. What the byte means (the last persisted slot) is [D], below.
- DN2 counterparts by strict hash: rename callback DT2 `0x400a1246` / DN2 `0x4009b3bc`; card write `0x400f5150` / `0x400f6b2c`; record flush `0x400f4d8c` / `0x400f6768`; slot record accessor `0x400f4ff8` / `0x400f69d4`; name entry `0x40117c70` / `0x401195e4`; +Drive rename `0x400e839a` / `0x400e9d6a`. The rename entry DT2 `0x400a0f7e` has no DN2 match by hash.
- Card write: `0x400f5150` returns 0 for a slot above 0x80, else the result of `0x400f4d8c`. That returns D0.b = 1 when the write succeeds and 0 on failure, for a null record, and when the record is not loaded or not dirty (nothing written). Only the low byte is defined. `0x400f4d8c` has a second caller, `0x400f4e08`.
- Rename callback: copies the 16-byte name from `0x4099d3a0` into the slot record (`0x400f4ff8()+8`, which sets the dirty byte), calls the card write, and, when the slot is the current one, `0x40042140` (`ProjectDataChangeInfo`), which copies the name into the live project and notifies observers.
- Name character table at DT2 `0x4025dea9` / DN2 `0x40236ec3`, identical bytes: `A-Z`, `c5 c4 d6 dc df c6 d8 c7 d1` (Å Ä Ö Ü ß Æ Ø Ç Ñ), `0-9`, `~!@#$%^&()_+-=`, space, NUL. No lower case. It is referenced only from DT2 `0x4015323c` / DN2 `0x401516ec`.

**[D]** (read once):
- The slot byte holds the last persisted slot: it lies at +0x3d of a 0x100-byte record (DT2 `0x4099d588`) that is persisted to flash and read back whole at boot, so it is read before the stock project load. Init writes 0; the project-container decode calls the setter with -1 when no project data exists. Not checked on the Syntakt.
- The project name holds at most 16 bytes (`pea 0x10` before the name entry `0x40117c70` at save-as `0x400a29c6` and PROJECT NAME `0x400a322e`, DT2).
- The name entry uses the character table (through the static `0x44d2e714`, filled by `0x401d1d66` calling `0x4015323c(1)`; readers `0x4011777c`, `0x4011784a`). The link to the name entry itself is not shown.
- The rename entry `0x400a0f7e` copies the name, queues the job "Project Rename" with callback `0x400a1246`, and does no name lookup. The job queue `0x400f0c1c` is not read. Rename showed no compare against other slots' names.

**[D]** (rename and the stamp):
- Emulator, DN2 1.12: the menu rename (MANAGE PROJECTS > RENAME) of the current project from "NEW PROJECT" to "ROUTINE CLOVE" gives the project a new live stamp, and the stock save that follows writes it into the card header. The static read (rename callback `0x400a1246` / `0x4009b3bc`, finding above) shows no stamp builder call on the path of a non-current slot, so a rename of a non-current project keeps its stamp.

**[O]**:
- A `+Drive` rename of the loaded project: not checked whether it changes the stamp like the menu rename.
- The stock load call for a later step (DT2 `0x40046906`, wrapper `0x40046c38`; DN2 `0x40042b92`, wrapper `0x40042e6e`).

`0x40041c96` (DN2 `0x4003e38c`) is a `ProjectDataChangeInfo` notification: it posts a change with 0x80 to a vfunc at +0x10. It is not the stamp builder.

## 5. Stock project paths and the project service

S is the slot setter DT2 `0x400c11ca` / DN2 `0x400bb7fa` (wrapper DT2 `0x400424a2` / DN2 `0x4003eb62`). S callers not listed in section 1: DT2 `0x400334bc` (DN2 `0x4002ea7c`, boot preset upgrade, S(0)) and `0x40043f74` (load from memory).

| Path | DT2 | DN2 | Setter | Save | Stamp | Service action | Mark |
| --- | --- | --- | --- | --- | --- | --- | --- |
| New project | job callback `0x400a227a` calls `initializeNewProject`, then the save wrapper | `0x4009c3f0` | S(slot) inside `initializeNewProject` (DT2 `0x40046820`) and in the wrapper before and after the real save | mirror save in `initializeNewProject`; real slot by the wrapper | change notification `0x40041c96` at DT2 `0x4004669e` | SAVED/LOADED for the slot; no `.dkp` until there is digikit data | [V] DT2, [D] DN2 call sites |
| Unload ("no project") | `0x400a0ae6`, `0x400f4ed6` call `initializeNewProject(-1)` | `0x4009ac5c`, `0x400f68b2` | S(-1) | mirror save | none | no file change | [V] |
| Delete | callback `0x400a13f0` loops over slots, calls `0x400f4d58` | callback `0x4009b566`, `0x400f6734` | S(-1) only when the deleted slot is current (getter `0x400424ca` compare at `0x400a1444`, S at `0x400a1452`) | none | `0x400f4d58` clears the record's valid byte, then `0x401305ca` erases the card slot | no hook; the file is archived at the next save into that slot | [V] DT2, [D] DN2 |
| Clear | single `0x400a131e` (`0x400f4ff8`, `0x4004234e`, card write `0x400f5150`); multi-slot `0x400a1376` (`0x400f4ed6` per slot) | `0x4009b494`, `0x4009b4ec` | none | none | `0x4004234e` writes "NEW PROJECT" at record+8 after clearing +8..+0x17, writes +0/+4 and bulk regions from +0x200; leaves +0x18 (the save stamp) unchanged | hook at the exit of each job callback (below); archive the slot's `.dkp` (the kept stamp would restore the old modules otherwise) | [D] DT2 and DN2 (the DN2 record reset is not traced) |
| Format | `0x40131276` | `0x4012f612` (found by string, not matched by hash) | S(-1) at DT2 `0x401314ee` only when a Project exists (DN2 `0x4012f878`) | none | first erase loop 0x400000 bytes in 0x800 chunks, further erase loops follow | no hook; the digikit store lies above the firmware write guard 0x457FFF ([finding 59](59-digikit-store-flash-layout.md)), so `.dkp` files survive | [O]: that every erase loop stays below the guard |
| Load from card | wrapper `0x40046c38` calls `loadProjectFromMmc` `0x40046906` | wrapper `0x40042e6e` | S(slot) at DT2 `0x40046ca0` (DN2 `0x40042ed6`) | mirror save (`pea 0x80` at `0x40046c84`, jsr `0x40043a30` at `0x40046c8a`; DN2 `pea` at `0x40042eba`) | change notification at DT2 `0x400469d8` (DN2 `0x40042c60`); the slot header keeps its stamp until the next save | LOADED; read `<slot>-*.dkp`, compare with the stamp of the slot listing | [V] |
| Paste | callback `0x400a12f8` calls `0x400f548e`: memcpy of 0xdd9604 bytes from the clipboard (+4) to the destination record from its start (stamp and name copied), then card write | callback `0x4009b46e` calls `0x400f6e6a` with the same arguments `(cache, slot, 0)`; the function has the same 86 instructions as the DT2 one except the absolute operands (call targets and the project size 0xc4b004), so its strict hash differs; each has one caller [V] | none | none | copied from the source | hook at the exit of the function (below); copy the source `.dkp` to the destination slot | [D] DT2 and DN2 |
| Move (cut) | `0x400f539e` (copy to the clipboard, then reset via `0x4004234e`) has no callers in the static read | [O] | — | — | — | — | [O] |
| Save-as / save to a slot | `0x400a21d0` calls the wrapper | `0x4009c346` | S(slot) in the wrapper | the save exit `0x40043b1e` sees D3 = slot, and D3 = 0x80 on the mirror save (skip 0x80) | — | SAVED | [V] |
| Rename | callback `0x400a1246` writes the name into the record and card-writes it | `0x4009b3bc` | none | none | whether a rename keeps the stamp is [O] (section 4) | [O] (section 4) | [O] |
| Load from memory | `0x40043f74`; callers `0x400ce2ae`, `0x400d45c4` | `0x400403d6` | calls S | [O] | [O] | [O] (what triggers it) | [O] |

**The slot listing record is in `MmcFs` [V].** The firmware lists the 129 slots (0..0x7f and 0x80, the live mirror) in the card file system singleton `MmcFs` (getter `cf.project.fn.mmcfs`: DT2 1.17 `0x4019dbda`, DN2 1.12 `0x4018e0a8`, DN2 1.11 `0x4018ddc8`). The record of slot s is at `MmcFs + 0x10 + s * 0x1c`: byte 0 is nonzero when the slot exists (the accessors DT2 `0x4012ef2e` / `0x4012f048`, DN2 `0x4012d802` / `0x4012d91c` test only this), byte 1 is 1 for a `DT1P` or `DN1P` header and 0 for the in-memory magic 0xBEEFBACE, the u32 at +4 is a record format word [D] (5 on a cleared project), the 16 name bytes at +8 have no terminator, the u32 at +0x18 is the save stamp (card header +0x18). The listing refresh `(MmcFs, slot)` (DT2 `0x4012f2fa`, DN2 `0x4012dbce`) fills the record and takes a mutex at `MmcFs + 0x1e034` (DT2) / `+0x1e030` (DN2). A header that cannot be read leaves the name "ERROR" with bytes 0 and 1 zero [D], unless the static header buffer still holds a valid magic. `ProjectCache` (singleton getter DT2 `0x4019e6f8`, DN2 1.12 `0x4018eb16`, DN2 1.11 `0x4018e836`) is a separate 0x20-byte object and holds no records.

**Paste and clear run on the BgWorker thread [D].** The menu actions only queue a job; the worker loop (DT2 `0x400f0abc`, DN2 `0x400f2498`) runs the callback. The callbacks finish their card write and listing refresh synchronously inside the job. The service tick runs in the UI task and can run before the job ends, so a hook at the entry of the job could read the slot record while it still holds the old name and stamp, or while the refresh has zeroed it; the paste then finds no matching source file and the digikit data is lost. The hooks therefore sit at the exits, after the stock code has finished:

| Exit | DT2 1.17 | DN2 1.12 | DN2 1.11 | Displaced | At the site |
| --- | --- | --- | --- | --- | --- |
| Paste (function + 0x11c; `cf_call` rule with `offset`) | `0x400f55aa` | `0x400f6f86` | `0x400f6dba` | 8: `4cee 047c ffc8 4e5e` = `movem.l -0x38(A6),D2-D6/A2; unlk A6` (the rts follows) | D0.b nonzero when the copy reached the card write (the write's own result is not returned), D6 = destination slot (also 12(A6)), A6 = frame |
| Clear one (function + 0x4a) | `0x400a1368` | `0x4009b4de` | `0x4009b37a` | 6: `4cef 0c04 001c` = `movem.l 0x1c(SP),D2/A2/A3`; then `lea 0x38(SP),SP; moveq 1,D0; rts` | SP = entry SP - 0x38, job at 0x3c(SP), slot = **job, D2 = slot; always returns 1 |
| Clear many (function + 0x70) | `0x400a13e6` | `0x4009b55c` | `0x4009b3f8` | 8: `4cd7 7c3c 4fef 0034` = `movem.l (SP),D2-D5/A2-A6; lea 0x34(SP),SP` (the rts follows) | job at 0x38(SP); the vector is re-read from the job (A2 and D5 are at its end); D0.b = AND of the per-slot results |

Each function has one rts; each site is whole instructions and not a branch target. The bytes are equal on all three images (the clear functions resolve on DN2 1.11 by the same strict hashes). The service records a pasted slot only when D0.b is nonzero (the copy reached the card write; the write's own result is not returned); it records every cleared slot whatever the result (a failed clear then archives a `.dkp` that is still needed, which is recoverable, while a cleared slot that gets its old data back is not).
