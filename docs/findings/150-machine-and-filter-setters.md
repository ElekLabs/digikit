# Machine and filter type setters

DT2 OS 1.17 and DN2 OS 1.12. Static reading (fw.py disasm, decomp and dossier, catalog emit-rule); no emulator run. Marks: **[V]** a second agent checked it against image bytes, **[D]** read once, **[O]** open.

## Machine type setter

- DT2 FUN_40051712 (size 372, anchor `cf_func hash=bf67b53a44935f39ee958c3360206699`, unique on the image). Args: (track object *, int type, int track, char flag). They are read at 4/8/0xc/0x13 off SP after its 0x20-byte frame; arg 4 is the low byte only. **[V]**
- It returns 0 if the track object's vfunc 0x28 (get sound) returns 0. The permit check FUN_400da514(type, FUN_400da5ba(track)) returns 0 if the type is not allowed on that track. **[V]**
- It writes the new byte with `move.b D2b,(0xa2,A0)` at 0x4005179e, but only if FUN_4004c6f8(settings) != 2. When the value is 2 it skips the write and the change notice and goes to the send block (0x40051874 -> 0x400517d0). On a change it builds a SoundParamChangedInfo at 0x400517b0 (pushes 0, old, 1, obj), then calls vfunc 0x10 with 0. **[V]**
- Send: only when flag != 0 and the mode value is not 0. The mode comes from FUN_4004c674(settings). Mode 0 sends parameter 0xea with type << 7 through FUN_40121c74 (`pea 0xea` at 0x40051832). Mode 1 sends (0xf, type) through FUN_40121c2c (0x4005186c). Other values send nothing. 0xf is a parameter id here, not a MIDI CC number. **[V]**
- Callers: 0x40036798 (call at 0x40036848) and 0x40052c46 (call at 0x40052c7c, no callers in the database). **[V]**
- DN2 FUN_4004cc08 (size 372, anchor hash eaf9a3569b24c104b820e2dabf59ff3c) writes `move.b D2b,(0xde,A0)` at 0x4004cc94, builds the notice at 0x4004cca6 and sends parameter 0xea through FUN_4012361c at 0x4004cd2e. **[V]** It also calls FUN_40048040, FUN_4004868c and FUN_4003e40e. **[D]** It is paired with DT2 by structure, size and callers; there is no catalogue pair. **[D]**

## KitActive set-machine wrapper (what the machine chooser calls)

- DT2 0x40036798 (Ghidra name KitActiveSettingsChangedInfo::ctor_dtor; size 280; anchor hash 32441e7a8a2c46c0561dcb6063bdac45). Args: (KitActive *k, track, type, flag4, flag5). **[V]**
  - Type 5 calls FUN_4002f3da(track, 0x7fff) and sets bit `track` in the 16-bit mask at KitActive data +0x552e. The bit is cleared for other types. **[V]** Type 5 is probably the MIDI machine. **[O]**
  - It clamps track to 0..15 and calls the setter on `k + 0x6c + track * 0x3c0`. On a mask change it calls FUN_4003674c (walks an observer list at k+0x60..k+0x64) and notifies through vfunc 0x50 with the object at 0x401f20cc. **[V]**
- The chooser: MachineSelectionView::vfunc_2 (DT2 0x40061678) commits with the call at 0x40061830. It pushes (0x84,A2) = KitActive, D2 = track, D0 = type (from FUN_40060df2, also stored at this+0x19c), 1, 1. Afterwards it only calls FUN_400362be(KitActive, track) at 0x4006183c and stores the result at this+0x1a0. The filter chooser in the same function commits at 0x40061870 to 0x4003696e. **[V]**
- So a call to the wrapper with (KitActive, track, type, 1, 1) matches a chooser commit. What the observer notification, the vfunc 0x10 listeners and the machines-service hooks do inside this path is not traced. **[O]**
- DN2 0x40031880 (size 280; anchor hash 42f12722dd5b838c1c8d8563e44870a3) has the same structure. The special type is 4 (FUN_40027c60(track, 0x7fff), mask word at +0x5cda). It uses the same 0x6c + track*0x3c0 addressing. MachineSelectionView::vfunc_2 (0x4005b9e8) calls it at 0x4005bba0 with the same pushes, then calls FUN_400313a6(KitActive, track). **[V]**

## Filter type setter (relation)

- DT2 FUN_40051886 and DN2 FUN_4004cd7c (finding 148) are a copy of the machine setter. They differ in the stored byte (0xa3 / 0xdf). They have no track or permit check and no change-notice ctor (only vfunc 0x10 with 0). **[V]**
- They send the same parameter 0xea (`pea 0xea` at 0x40051976, with `lsl #7`) and (0xf, type) (`pea 0xf` at 0x400519a4). **[V]**
- A filter-type change therefore goes out as parameter 0xea, which finding 149 identifies as row 6 "Machine Type". The receiving side was not examined, so the effect on receive is open. **[O]**

## Track to sound

- A track object is `k + 0x6c + track * 0x3c0` on DT2 and DN2. Its vfunc 0x28 returns the sound pointer or 0. **[D]**
- The filter-type getter (DT2 FUN_400506ac, DN2 FUN_4004b860) takes the track object. The wrapper FUN_400362be (DT2) / FUN_400313a6 (DN2) takes (KitActive, track). It returns -1 if the track's bit is set in the mask (+0x552e / +0x5cda), else the filter type. **[D]**

## Getting KitActive outside a view

- KitActive = `cf.project.fn.active_kit(cf.project.fn.singleton())`. The MachineSelectionView constructor stores exactly that at this+0x84: DT2 0x40061268 `jsr` singleton, 0x4006127c `jsr 0x40041d24` (the active kit), 0x4006128e `move.l D0,(0x84,A2)`; DN2 0x4005b5e2, 0x4005b5ec, 0x4005b5fe. **[D]**
- FUN_40041d18 is not a KitActive accessor. It returns arg + 0x30 (a settings accessor taking a Project pointer). **[V]**
- The Project singleton comes from 0x40198b7e, which creates DAT_44f370a0 on first use. The setter reaches project and settings through it (FUN_4004c6f8, FUN_4004c3da, FUN_4004ca26). **[V]**
- The SETUP device test (modules/tools/debug/tests/setup.device.test.ts) calls `set_kit` with this KitActive from the UI task on DT2 and DN2, and the firmware's own MACHINE_CHANGE follows each change. **[D]**

## Use

- The debug module's SETUP command is built (command 0x0a, event SETUP_DONE; modules/tools/debug/README.md "Setup"). It sets a track's machine type through `cf.machine.fn.set_kit` and its filter type through `cf.filter.fn.set_kit` (DT2 0x4003696e, DN2 0x40031a56), both with `(KitActive, track, type, 1, 1)`, from the module's tick in the UI task. It reads the type back through `track_type` or the filter getter and reports the status in SETUP_DONE.
- It never writes the byte directly. Finding 148: a raw write to sound+0xa3 did not reach the DSP.
