# Filter machines: readers of the stored filter type

DT2 OS 1.17 and DN2 OS 1.12. Static reading of MAIN OS bytes, the Ghidra dumps and the SHARC database; no emulator run. Used by the plan for run-time filter types. Marks: **[V]** a second agent checked it against image bytes, **[D]** read once, **[O]** open.

## Storage and accessors

- The filter type is one byte in the sound: DT2 sound+0xa3, DN2 sound+0xdf. The machine type is the byte before it (DT2 +0xa2, DN2 +0xde). **[V]**
- Getter DT2 FUN_400506ac (0x400506d0, `mvs.b (0xa3,A0)`): the value is a signed byte; no sound gives -1. 15 callers. **[D]** DN2 getter 0x4004b860.
- Setter DT2 FUN_40051886 (reads the old byte at 0x400518de, writes at 0x400518f0, no bound), then sends param 0xea (type << 7) or 0xf. DN2 FUN_4004cd7c. **[D]**
- The only commits are 0x4003696e (the chooser), the kit paste (0x40036abc) and 0x400d45c4. **[D]**
- Filter accessor DT2 FUN_400c89c6: unsigned `type <= 5` gives row 0x4293b858 + 0x2c * type, else 0x4293ba68. **[V]** DN2 FUN_400c2614: `<= 5`, base 0x424339cc, else 0x42433bd4. **[V]** The rows are RAM, filled by the init FUN_401be1be. **[D]**
- Machine accessor DT2 FUN_400c89a4: unsigned `type <= 6`, base 0x4293b960, else 0x4293ba68. DN2 FUN_400c25f2: `type <= 4`, base 0x42433b24, else 0x42433bd4. **[V]**
  - On DT2 the filter table and the machine table are one contiguous array: the filter base + 12 * 0x2c = 0x4293ba68. **[V]**
  - On DN2 the two bases differ by 0x158, which is not a multiple of 0x2c. They are two arrays that share one fallback row. **[V]**
  - Both accessors fall back to the descriptor of the last stock machine: type 6 on DT2, type 4 on DN2. **[V]**

## Hook sites on DT2 1.17

| site | address | what it does | mark |
| --- | --- | --- | --- |
| accessor | FUN_400c89c6 (callers FUN_40041fa8, FUN_4006a57a, FilterPageView::vfunc_47 twice) | type to descriptor row | **[V]** |
| graph switch | FUN_4007439e: `cmp 5; bcs default` at 0x40074580-84, jump table cases 0..5 (the getter call is at 0x40074574) | selects the response curve drawing. The default branch 0x4007461a calls a vfunc at +0xb8 and FUN_4006b01e. That it draws nothing is not established. | **[V]** switch; default behaviour **[O]** |
| list builder | FUN_400610e8 calls FUN_40052c98; static vector built once from 0x401f5440, six longs `{0,1,4,3,5,2}`; entries are 8 bytes (callback FUN_400604a8 or FUN_40060560, value = stored type) | the chooser list | **[D]** |
| name function | FUN_400da71e: `type < 6` gives table 0x4020ef30 (stride 12: long name, short name, 0), else the string "ERROR" (0x4023fbe4). Only caller 0x40060594. Names: Multi-Mode/MM, Lowpass 4/LP4, Equalizer/EQU, Comb-/COMB-, Legacy LP/HP/LEG, Comb+/COMB+ | name in the list | **[V]** |
| loader bound | FUN_400db3b2, `move.b #7` at 0x400db484 | see below | **[V]** |
| param slot | FUN_400da3a2(code, machine, type) | see below | **[V]** (structure) |

- The five new sites plus one extension of the existing `param_code` hook make six. **[D]**
- Many other readers only copy or compare the byte: FUN_40007d50, FUN_400dba94, FUN_400dd76e, the kit paste, the undo code at 0x4007798a, 0x400d3660, 0x400d45c4 and 0x400d54c2. They need no hook. **[D]** Param id 0xa3 as an id appears in FUN_40038706, FUN_400139ca and FUN_40196598 (generic by-id readers; not traced, **[O]**).
- Block copies of sound+0xa2 to DSP staging (FUN_4002d438, FUN_4002d816) carry the byte without reading it. **[D]**

### Param slot function

- DT2 FUN_400da3a2(code, machine, filter_type): codes 0x23..0x25 (the three filter knobs) with `type <= 5` read the word at 0x4316ea8c + (3 * type + code - 0x23) * 4; for a larger type the function returns without a value (the value is not read). Machine codes 0x19..0x22 use machine < 7 and table 0x4316ead4. **[V]** structure; the table is RAM, so its contents cannot be checked in the image.
- Six callers: FUN_4000ff7e, SoundParameterSet::vfunc_20, FUN_40042fe2, the two SoundParamChangedInfo constructors, FUN_400e64dc. **[D]**
- The existing hook is `cf.machine.hook.param_code.args` plus `site.param_code.exit`. It must be extended for the filter branch. Without that, a new filter type has no slot: its three knobs do nothing. **[D]**
- DN2 FUN_400dc18e: filter slot table 0x42c65cd0 (6 types x 3, codes 0x42..0x44), machine table 0x42c65d18, init FUN_400dc634. **[D]**

## Chooser

- MachineSelectionView constructor 0x400611fe. `this[0x6a]` selects the list (0 source, 1 filter). **[D]**
- The filter row is text only: `"%-15s"` of the name. There is no glyph and no bitmap per type. **[D]**
- FLTR1 (page 8) and FLTR2 (page 9) are fixed global pages. The page count does not vary with the type. **[D]**
- Commit: Enter at 0x4006183c and 0x40061870. Machine commit goes to 0x40036798. Filter commit goes to 0x4003696e, then the setter FUN_40051886, with no range check. **[D]**

## Stock behaviour for a stored type of 6 or more

- Accessor: the shared fallback row, the descriptor of the last stock machine. The FLTR1 page shows its knob codes for fields 4 to 6. The comparison is unsigned, so -1 also falls back. **[V]** for the accessor.
- Graph: the default branch of the switch (what it draws is **[O]**). Slot function: no slot, so the knobs do nothing. List: no matching entry; the name function gives "ERROR" if drawn. **[D]**
- Loader: DT2 `sound+0xa3 = (byte)(rec[0xd3] + 1) < 7 ? rec[0xd3] : 0`. It keeps 0..5 and 0xff (-1 wraps to 0 and passes); 6 and above become 0 (SVAR). **[V]** The machine byte uses the bound `moveq 8` at 0x400db44e. **[V]**
- DN2 FUN_400dd34e: the filter byte sound+0xdf (record +0xf5) uses `move.b #7` at 0x400dd420 (store at 0x400dd43e). The byte +0xde uses `moveq 6` at 0x400dd3ea. **[V]**
- So a stock load destroys an extended type. Either patch the immediate 7 (to 32 if types are saved in kit files), or keep the type in the digikit store with a stock value in the sound. **[D]** (choice is a recommendation)
- Setter and chooser commit have no bound. Param 0xea carries type << 7, so types up to 127 fit the 14-bit value. **[D]**

## Proposed numbering

- Service filter types 16..31 (16 slots). **[O]** (proposal)
- Reasons: positive as a signed byte; never 0xff or -1 (the no-sound value and the loader quirk); well above stock 0..5; 31 << 7 = 3968, which fits param 0xea; the SHARC table window 0x2567a0..0x2567bf is 32 words wide.
- The accessor hook maps 16..31 to the service row. Types 6..15 stay stock fallback and are never written by us.
- Machines use a separate byte (+0xa2), so the numbers do not collide. Only the shared fallback row links the two.

## DSP link

- ColdFire side: FUN_4002d438 copies sound+0xa2 (0x9a bytes) to 0x80003cd0 + t * 0x9a; byte 1 of the row is the filter type. The frame builder (vector 191 handler, 0x4002dd0c) reads it once: `move.b (0x991,A0),D1` at 0x4002ebe8, `ext.w`, `move.w D1,(0xb4,A3)` at 0x4002ec00. **[V]** The word is at 0xb4 + 2 * track; the `+2*track` part is **[D]** (A3 is the per-track frame pointer, not shown in the excerpt). The machine type goes to word 0x94. **[D]**
- SHARC side: DM 0x255990 = 0x2558dc + 0xb4 (the filter word of the frame copy; this mapping comes from finding 04 and was not rechecked, **[D]**). The only reference to the literal 0x255990 is FUN_1c24e9: `I1=0x255990; M0=DM(I1,M4)` (M4 = track), then `I4=0x2567a0; S3=DM(I4,M0)` at 0x1c26e2. **[V]** The mapped S3 is stored to the per-track record (`DM(I5-11)=S3` at 0x1c2869). **[D]** The raw type is not stored.
- The machine type indexes a neighbouring table at 0x2567c0 the same way (literal at 0x1c2731). **[D]**
- Corrected earlier reading: the routines 0x1cdbb2, 0x1cd286, 0x1cc79e, 0x1cbf07 and 0x1ccd96 are not five filter types. 0x1cd286, 0x1cc79e, 0x1cbf07 (twice) and 0x1ccd96 are consecutive stages in FUN_1c71ec (calls at 0x1c73a2, 0x1c73bd, 0x1c73d5, 0x1c7434, 0x1c73f2). 0x1cdbb2 is called once from FUN_1c642a at 0x1c6ead. **[D]**
- The 7-entry table at blob address 0x8055c874 holds `{0,0,1,2,3,4,0}`, followed by `{5,1}`. FUN_1c642a bounds its index at 7 (`R1=7; compu; JUMP IF GE 0x1c6eb7` at 0x1c6c10) and loads `I12=DM(I4,M4)`. **[V]** That the index is the machine type (7 types, 0..6) is **[O]**: R2 was not traced to its field. The table is not an exponential curve.
- Table at DM 0x2567a0 after boot on DT2 1.17 (read once from the running emulator, run of 2026-11-18, **[D]**): words 0-15 are
  `1 3 2 4 5 6 0 0 0 1 2 3 4 0 5 0`; words 16 and up are DSP addresses (`0x254478 0x2544f8 0x254578 0x2545f8 0x253f78 0x253ff8 0 0
  0x253e78 ...`). So filter types 0-5 map to S3 = 1, 3, 2, 4, 5, 6, and types 6 and 7 map to 0. What words 8-15 and the address words
  are for is **[O]**.
- Open: what S3 selects downstream. **[O]** On DN2 the DSP side and the frame builder word are not examined. **[O]**

## Hook budget

- The plan's limit of 64 is `EXPORTS_MAX` (loader/exports.rs:9) and `BIND_STOCK_MAX` (stock bytes per bind site). The limits that bind are `bind_max = 24` (BIND items per provider file) and `install_sites = 48` (sites installed at once over all providers). `sdk.toml` lines 1801 and 1802, `gen_limits.rs`, `bind.rs:883` (`hooks.len() > BIND_MAX`) and `role.rs:145`. **[V]**
- machines.dks today (`modules/core/services/machines/dks/module.toml`): 13 declared `[[hook]]` (the union over devices), 12 `[[export]]`, 1 `[[fill]]`. Per device: DT2 binds 7 hooks (clone.5, param_code.args, srcpage, four source_*), DN2 binds 10 (four source_*, group, type_page.count, type_page.desc, param_code.map, type_reader.load, owner_type_load). One BIND item per hook. Patchkit bind checks the 13-hook union. **[V]**
- The figure "about 21" is the installer's design allowance ("machines 21 + fx 9"), not a count.
- Adding six filter hooks gives 19 declared hooks, under `bind_max = 24`. The earlier estimate of 27 (over the limit) is wrong. **[V]** The install total (13 + 9 fx) is far under 48.

## DN2 1.12 address table

| item | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| getter | 0x400506ac | 0x4004b860 (`mvs.b (0xdf,A0)` at 0x4004b884) |
| wrapper (8 callers) | 0x400362be | 0x400313a6 |
| filter accessor | 0x400c89c6 | 0x400c2614 |
| machine accessor | 0x400c89a4 | 0x400c25f2 |
| graph switch | FUN_4007439e | FUN_4006eabe (wrapper call 0x4006ec6e, getter call 0x4006ec94) |
| FilterPageView::vfunc_47 | 0x40074858 | 0x4006ef78 (accessor calls 0x4006efd0, 0x4006f008) |
| param slot | FUN_400da3a2, 0x4316ea8c | FUN_400dc18e, 0x42c65cd0 |
| chooser ctor, vfunc_2 | 0x400611fe, 0x40061678 | 0x4005b56e, 0x4005b9e8 |
| filter list builder | FUN_400610e8 / FUN_40052c98, table 0x401f5440 | FUN_4005b458 / FUN_4004dae2, table 0x401de150..0x401de168 |
| setter | FUN_40051886 | FUN_4004cd7c (read 0x4004cdd4, write 0x4004cde6) |
| loader bound | 0x400db484 | 0x400dd420 in FUN_400dd34e |
| name | FUN_400da71e | pair 0x400dc508 |
| DSP staging copy | FUN_4002d438 | not found by pair **[O]** |

All **[D]** except the rows marked in the sections above. DN2 anchors for the machine hooks exist already (`cf.machine.hook.param_code.map` 0x400dc196, `sound_load_type_bound` 0x400dd3ea). Extra DN2 copies of (0xde, 0xdf) at 0x4000bb9c, 0x4000c7f0, 0x4001428c and a byte copy at 0x4000a8da are copies only. **[D]**

## The decisive check still open

One emulator run settles the DSP side: change the filter type through the firmware's own path (FUNC+SYN on the FLTR page, select a type,
which runs the setter FUN_40051886) and compare the SHARC execution profile per type. If different routines run per S3 value, the filter
stage is dispatched on S3 and a stub at the dispatch takes new types. Otherwise the filter is inline, and the service needs a stub at
0x1c26e2 (replace the S3 load) plus a slot.

The first attempt (2026-11-18, DT2 1.17, coupled session) did not settle it:
- A raw write of sound+0xa3 at the address the getter's probe gave (0x44fb9da0 + 0xa3) read back correctly but never reached the staged
  row (0x80003cd0 + 1) or DM 0x255990. That address is probably not the live sound record. **[D]**
- The daemon of that run had no generated DSP core. It was built with a plain `cargo build` without `SHARC_GEN_DIR`, so the DSP stub traps on
  every instruction: the session ended with `native-trap: no decoded instruction at 0xb86372` when the DSP first left idle, and `counted`
  stayed zero because no instruction ran in a counted tier. `sharc.profile` does count the native tier (fast, generated block and interpreter
  dispatches). **[D]** (read in the source; the binary of that run was not checked)

The retry needs:
- A daemon built with the generated core and the profile feature:
  `SHARC_GEN_DIR=out/native/dt2-audio/gen node tools/nx/rust-env.mjs --gen --variant sharc-profile -- cargo build --release -p digi-control --bin digi-emud --features sharc-profile`
  (run `cargo check` first; the build takes the benchlock through the launcher).
- A check that the core is present before the run (the daemon file is hundreds of MB, not 26 KB, and `strings` finds the generated symbols).
- A proof that the profile counts: `counted` is non-zero on a busy routine before the filter comparison starts.
- `SHARC_AOT=0` for exact per-PC counts. A dispatch is credited to its entry PC, so two filter types that differ inline inside one region show
  equal counts at that entry.
- The filter type changed through the UI chooser (the setter FUN_40051886), not by a raw byte write.
