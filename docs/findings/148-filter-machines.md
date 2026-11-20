# Filter machines: readers of the stored filter type

DT2 OS 1.17 and DN2 OS 1.12. Static reading of MAIN OS bytes, the Ghidra dumps and the SHARC database. The DSP section also comes from a static read of the SHARC dispatch (2026-11-20), checked against image bytes by a second agent. Used by the plan for run-time filter types. Marks: **[V]** a second agent checked it against image bytes, **[D]** read once, **[O]** open.

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
| graph switch | FUN_4007439e: `cmp 5; bcs default` at 0x40074580-84, jump table cases 0..5 (the getter call is at 0x40074574) | selects the response curve drawing by a mapped value 0..5 (case 0 FUN_4006b6f2, cases 3 and 5 FUN_4006b90e, others FUN_4006b442). Any other value goes to `default` at 0x4007461a, a shared tail (vfunc +0xb8, FUN_4006b01e, then FUN_4006b02e page dots when the count is above 1) that every type runs. So a type outside 0..5 draws no response curve and the graph area stays blank. | **[V]** |
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

### Graph area

- The response graph rectangle is x 21..71, y 7..23 (51 x 17 px) on the 128 x 64 screen, from FUN_4006b6f2 / FUN_4006b442: x from `col*0x1a + 0x15` to `col*0x1a + 0x47` with col 0, y from `0x22/0x32 + row*-0x1b` with row 1. **[D]**
- The filter envelope graph FUN_4006ab50 is drawn before the switch for every type, in x 21..71, y 34..50 (row 0). **[D]**
- DN2: FUN_4006604e has the same rectangle constants; FUN_4006eabe is the DN2 switch. **[V]**
- The graph is drawn into a new 128 x 64 Bitmap blitted at (0,0). **[D]**

## Stock behaviour for a stored type of 6 or more

- Accessor: the shared fallback row, the descriptor of the last stock machine. The FLTR1 page shows its knob codes for fields 4 to 6. The comparison is unsigned, so -1 also falls back. **[V]** for the accessor.
- Graph: no response curve; the rectangle stays blank, the envelope graph and the tail still draw. **[V]** Slot function: no slot, so the knobs do nothing. List: no matching entry; the name function gives "ERROR" if drawn. **[D]**
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
- Converter FUN_1c24e9 (DT2): `I1=0x255990` at 0x1c26c0, `M0=DM(I1,M4)` at 0x1c26c5 (the filter-type frame word of the track), `I4=0x2567a0` at 0x1c26df, `S3=DM(I4,M0)` at 0x1c26e2, stored by `DM(I5-11)=S3` at 0x1c2869 at word +31 of the per-voice record (record base 0x2506ec + voice * 0xdc; I5 is base + 42 there after post-modifies +27 at 0x1c26eb and +15 at 0x1c2756). The machine selector S1/S2 goes to word +16 (0x1c274f). **[V]**
- The machine type indexes the neighbouring table at 0x2567c0 the same way (literal `I3=0x2567c0` at 0x1c2731). **[D]**
- The 0x2567a0 table is not in the image (no loader data for 0x256780..0x2567c0, no static writer found); it is filled at run time. Read once from a running emulator (2026-11-18): words 0-15 `1 3 2 4 5 6 0 0 0 1 2 3 4 0 5 0`, words 16 and up DSP addresses. Filter types 0-5 give S3 = 1, 3, 2, 4, 5, 6. **[D]**
- Stock dispatch in FUN_1c642a, two table jumps on S3, both bounded at 7:
  - Coefficient stage (per voice per block): `R5=0x7` at 0x1c66be, `I4=0x8055c858` at 0x1c66cc, `R0=compu(R9,R5)` at 0x1c66d6, `JUMP IF GE` to 0x1c69ed at 0x1c66d8, `I12=DM(I4,M4)` at 0x1c66e0, indirect jump at 0x1c66ec. Table 0x8055c858: `1c69ed 1c69c6 1c6d1e 1c6d51 1c6d7a 1c6dd6 1c6e4e` (entry 0 is the skip target). **[V]** R9 at the jump comes from `DM(I0-2)` at 0x1c66b2; that it is S3 is **[O]**. F8 there is the cutoff clamped by `F12=fmax(F8,F12)` (F12 from `R12=M5`, 0.0 when M5 = 0 as the DSP convention assumes) and `F8=fmin(F12,F6)` with F6 = 1.0 **[V]** for the clamp; that F8 is the normalized cutoff with the filter envelope and key tracking added (0x1cc1dd runs the envelope; key tracking `(note - 48)/12` through an exp2 approximation) is **[D]**. Coefficient routines per S3: 0x1cd906 (1), 0x1cd3b5/0x1cd4fd (2), 0x1cc9b4/0x1cca61 (3), 0x1cbe19/0x1cbdea (4, 6), 0x1ccfa4/0x1cd137 (5) **[D]**.
  - Process stage (one block per voice): `R1=0x7` at 0x1c6c0d, `R2=DM(I1,M6)` (S3) at 0x1c6c0f, `compu` at 0x1c6c10, `JUMP IF GE` to 0x1c6eb7 at 0x1c6c13, `I4=0x8055c874` at 0x1c6c16, `I12=DM(I4,M4)` at 0x1c6c19, `R12=I3` / `R8=R12` (buffer from `I3=DM(I1+32)` at 0x1c6c0b), indirect jump at 0x1c6c25. Table 0x8055c874: `1c6eb7 1c6ea9 1c7395 1c73b0 1c73cb 1c73e3 1c742a`; the coefficient table runs straight into it. **[V]**
  - Process routines per S3: 1 -> 0x1cdbb2 (state-variable style), 2 -> 0x1cd286, 3 -> 0x1cc79e, 4 and 6 -> 0x1cbf07, 5 -> 0x1ccd96. S3 1, 2, 3, 4, 6 run in place (R8 = R12 = the voice buffer); S3 5 writes to the constant buffer 0x8045c3c0. **[V]**
  - Block length: each stage pushes R15 as a stack argument (0x1c6eac, 0x1c739f, 0x1c73d2, 0x1c73ea); R15 is loaded at 0x1c6501 from the word the saved R12 argument points to (32 per finding 53, **[D]**). The DO loops read it as `DM(I6,M6)`: 0x1cd286 (DO 0x1cd339), 0x1cc79e (DO 0x1cc862), 0x1cbf07 (DO 0x1cbf9c); 0x1cdbb2 runs n - 2 (DO 0x1cdc48); 0x1ccd96 has a literal 31 **[V]** (assuming M6 = +1).
  - Static cost per call with n = 32 (`fw.py cost`, upper bound, no stalls): 0x1cdbb2 98 + 17n = 642; 0x1cd286 116 + 32n = 1140; 0x1cc79e 153 + 67n = 2297; 0x1cbf07 131 + 47n (+ 2*R12) about 1635; 0x1ccd96 489 **[D]**.
- Consequence for new types: a type of 16..31 reads words 16..31 of the 0x2567a0 table (other data), so S3 is 0 or 7 and above; both stages skip and the voice plays unfiltered, with no fault. **[D]** Types 8..14 would map to S3 0..5 (a stock filter): never write them.
- Candidate stub sites (DT2): the converter load at 0x1c26e2 (give types 16..31 their own S3, 7 and up); the two bounds and table literals (0x1c66be/0x1c66cc and 0x1c6c0d/0x1c6c16) or the two `JUMP IF GE` skips (0x1c66d8, 0x1c6c13) to a range check that calls the module's coefficient and process entries. **[D]** (proposal)
- DN2 1.12: converter inline in FUN_1c2712 (type word `M0=DM(I5,M4)` at 0x1c2b37, `S0=DM(I5,M0)` at 0x1c2c43 with table 0x25d728, stored `DM(I1-5)=S0` at 0x1c2c7a) **[D]**. Dispatch in FUN_1c8ef1: `R1=0x7` at 0x1c957e, compare 0x1c9581, skip `JUMP IF GE` to 0x1c98b9 at 0x1c9584, `I4=0x8052dbc0` at 0x1c9587, `I12=DM(I4,M4)` at 0x1c958a, indirect jump 0x1c9595; process table 0x8052dbc0 `1c98b9 1c98ab 1c9ba9 1c9bc4 1c9bdf 1c9bf7 1c9c45`; coefficient table 0x8052dba4 `1c927e 1c9253 1c9722 1c9753 1c977b 1c97d9 1c984e` **[V]**. Process routines b82342, b81a16, b80f2e, b806f5, b81526, b806f5 in S3 order **[D]**.
- Modulation: LFOs and the six MIDI modulators are added on the ColdFire into the knob value array before the frame is built (finding 04), so a knob's frame word carries them. **[D]** The filter envelope and key tracking are added on the SHARC in the coefficient stage (above).
- Open **[O]**: which record words carry the three type knobs (candidates +26, +28, +30, read as `DM(I0-62/-60/-58)` at the coefficient jump); the (type, knob) to code table at 0x4316ea8c (RAM, fill not found); whether anything else reads S3 at record +31.

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
| graph rectangle function | FUN_4006b6f2 | FUN_4006604e |
| FilterPageView::vfunc_47 | 0x40074858 | 0x4006ef78 (accessor calls 0x4006efd0, 0x4006f008) |
| param slot | FUN_400da3a2, 0x4316ea8c | FUN_400dc18e, 0x42c65cd0 |
| chooser ctor, vfunc_2 | 0x400611fe, 0x40061678 | 0x4005b56e, 0x4005b9e8 |
| filter list builder | FUN_400610e8 / FUN_40052c98, table 0x401f5440 | FUN_4005b458 / FUN_4004dae2, table 0x401de150..0x401de168 |
| setter | FUN_40051886 | FUN_4004cd7c (read 0x4004cdd4, write 0x4004cde6) |
| loader bound | 0x400db484 | 0x400dd420 in FUN_400dd34e |
| name | FUN_400da71e | pair 0x400dc508 |
| DSP staging copy | FUN_4002d438 | not found by pair **[O]** |
| SHARC converter | FUN_1c24e9, load at 0x1c26e2 | FUN_1c2712, load at 0x1c2c43 |
| SHARC coefficient table | 0x8055c858 | 0x8052dba4 |
| SHARC process table | 0x8055c874 | 0x8052dbc0 |
| SHARC process dispatch | 0x1c6c0d..0x1c6c25 | 0x1c957e..0x1c9595 |

All **[D]** except the rows marked in the sections above. DN2 anchors for the machine hooks exist already (`cf.machine.hook.param_code.map` 0x400dc196, `sound_load_type_bound` 0x400dd3ea). Extra DN2 copies of (0xde, 0xdf) at 0x4000bb9c, 0x4000c7f0, 0x4001428c and a byte copy at 0x4000a8da are copies only. **[D]**

## What the emulator still has to show

One short confirmation run, or the first dk:dev iteration of E7, with a daemon built with the generated core:
1. A filter type of 16 plays unfiltered.
2. Which record words carry knobs 4-6 (turn each knob, read record +26/+28/+30).
3. The run-time contents of 0x2567a0.

The daemon needs the generated core and the profile feature. A plain `cargo build` without `SHARC_GEN_DIR` gives a DSP stub that traps on every instruction (`native-trap: no decoded instruction`), and `sharc.profile` counts nothing. Build with:
`SHARC_GEN_DIR=out/native/dt2-audio/gen node tools/nx/rust-env.mjs --gen --variant sharc-profile -- cargo build --release -p digi-control --bin digi-emud --features sharc-profile`
(run `cargo check` first). Check that the core is present (the daemon file is hundreds of MB, and `strings` finds the generated symbols) and that `counted` is non-zero on a busy routine. Use `SHARC_AOT=0` for exact per-PC counts. Change the filter type through the UI chooser (the setter FUN_40051886), not by a raw byte write: a raw write to sound+0xa3 at the probe address did not reach the staged row. **[D]**
