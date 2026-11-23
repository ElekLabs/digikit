# 30. Run-time machines and parameters (digikit runtime, Phase 5, ColdFire side)

Plan: `docs/plans/digikit-runtime.md` (Phase 5). Design: `docs/design/digikit-registry.md`. Code: `modules/core/adapters/dynreg`,
`modules/core/runtime/src/rs/src/registry.rs`, `packages/patchkit/src/register.rs`, `tests/fixtures/modules/dyn-xslice`, `tests/fixtures/modules/dyn-params`. Lane r2b,
2026-10-16, one agent, native emulator, Digitakt II 1.16 and 1.17 (stock images; nothing ran on a device). Everything is **[D]** (measured here once; no
second agent has checked it against the image bytes). Digitone II and the unload refusal: finding 33.

## Result

**[D]** A `.dkm` that holds only typed records adds a machine to the SRC machine list at run time, on both DT2 images, with no re-patch:

| Step (DT2 1.16 and 1.17) | Result |
| --- | --- |
| patch `core/runtime` + `core/dynreg` (so `core/machines`, `core/rows`) | 633 512 of 1 396 736 bytes of `cf.expansion`; machine rows for types 7 to 23, 16 parameter rows; `dynreg_info` present |
| pack `test/dyn-xslice` (a data-only clone of MANUAL SLICE, one parameter): `patchkit pack test/dyn-xslice --device dt2` | 372 bytes: header, a 4-byte image, 204 bytes of records |
| eight edited copies of the file (wrong device, bad version, type 9, type 24, `clone_of` a run-time type, knob naming a missing parameter, knob not a stock code, position 3) | refused with -21, -5, -22, -22, -22, -5, -5, -5; **no byte of any row or table changes** |
| the firmware's list getter (`FUN_40052a08`, called through a stub that loads `a0`) before the load | `0 1 2 3 6 4 5` (the built-in order); the static sorted vector holds 15 entries including 16 to 23 |
| load `xslice-d.dkm` | 0; row 16: `clone_of` 6, knobs `f8 f9 fa fb fc fd <param code 0x113> fe`, level 10; entry 16 of the name, permission and per-type tables written (the clone's permission and per-type byte); parameter row 0: mirror index 33, 0x80 + slot 6, range 0..0x7f00, default 0x2000; **only row 16 and parameter row 0 changed**; `mf_dispatch(16)` returns row 16; the getter returns `... 5 16` |
| load a second file with type 16 | -23 (`E_TYPE_USED`), nothing written |
| unload | 0; the machine rows, names, permission, per-type and parameter tables are byte for byte the start (but for the string-representation, name and short pointers of the free rows); the getter is back to `... 5` |
| load, unload again; then `test/dyn-params` (eight parameters, one per slot) with `xslice-d` | all 0; the eight parameter rows equal the patch-time formulas of `core/rows` for every slot 0 to 7 (word by word, but the two string pointers); a third machine whose eight parameters do not fit (9 + 8 > 16): -24 (`E_PARAM_FULL`), nothing written |
| write watch on the rows and tables, the runtime's code the only allowed writer | 34 events, all by `core/machines` (`mf_ensure`: the two representation pointers of each of 17 rows, at first use); control run (runtime not allowed): 648 more events, all the runtime's |
| flush check | 0 hits (1.03 M CPUSHL) |

**[D]** On the panel (coupled daemon, FUNC+SRC, `tools/patchkit/pk_registry.py ui`, frames pinned in `tests/patchkit/registry_frames.json`): after the
load the machine list shows XSLICE-D after MIDI (a separator between the stock and the new group); YES puts it on track 1 (type byte 16); the SRC
page shows the clone's knobs and the module's own parameter (label SPARE on knob 6); TRIG 1 plays without fault. After the unload (the track keeps type 16)
the list is the stock list again (frame equal to the one before the load), and the SRC page shows eight empty boxes: **the same frame as a track on
type 11, a gap row nobody ever filled**. After the reload the SRC page is the machine's again (frame equal to the first). Health (fault, DSP halt, stall) checked after
every input. **[C]** (finding 33) The unload with the track on the machine is now refused (-26), so in the proof the track is put on type 0 for the unload and the type byte is poked back to 16 afterwards (a project that names an unloaded machine); the frames above are unchanged, the reload is followed by a selection through the list, and a refused unload with the track on the machine leaves the same frame.

## What was found

- **[D]** The machine list the user sees is **built once at boot** and cached. `FUN_40052a08` (the getter; it copies a static list that it sorts once, with a
  `std::map` rank built once) is called a single time during boot (counter in the filter routine: 1 before any panel input, still 1 after FUNC+SRC),
  from `FUN_40060f50`, which fills the machine selection view's item list (`view + 0x184`) in the view's constructor. A list that changes at run
  time needs that function run again (it starts with `FUN_401ae154`, a vector clear). `dynreg_view_hook` records the view (the hook at the entry of
  `FUN_40060f50`, function hash `aba88709...` on both images); the registry calls the function again after every change. Without it the load succeeds, the
  rows are right, the getter returns the machine, and the panel shows nothing new. A patched machine never met this: it is in the list at boot.
- **[D]** The reserved rows are in the static list, the rank map and the tables from patch time (positions 255, in type order), so none of them is rebuilt
  at run time. What changes with a load is the row, its table entries, and visibility: the getter's final `std::copy` is retargeted (`dynreg_copy`; one `jsr`, anchor
  by function hash `7c680e42...`, offset 0x13e, the same on both images) and leaves out the types whose row has an empty name.
- **[D]** Running the item builder again while the machine selection view exists but is not open ran without fault in the emulator, in
  every proof run (a load, an unload and a reload); the cursor and heap use across many rebuilds were not examined.
- **[C]** Finding 02 says a stored type that names a gap row "loads and plays as type 0 (SAMPLE or FM TONE)". That holds for the permission, per-type byte
  and DSP selector word. The SRC page is not SAMPLE's: the descriptor fields of a gap row are zero, so the page shows eight empty knob boxes (the frame above).
- **[D]** `core/dynreg` keeps every existing image byte-identical: `test/xslice` on DT2 1.16 is still `5c026883...` (finding 27's pin; the
  `typed` pins of `packages/patchkit/tests/typed.rs` run in the gate). Images with the runtime alone change only by the runtime's new code
  (`registry.rs`: about 6 KB, 2 KB of it state); with the optional link unresolved they are otherwise as before.
- **[D]** A flush-check hit appeared on 1.17 only, at first: a variable of `core/dynreg` (the view pointer) shared a 16-byte line with a hook tail.
  The variable sits alone in a line now (no code shares a line the CPU writes to; harmless in itself, but the instrument cannot tell).
- **[D]** Cost: `dynreg_copy` is 96 bytes, `dynreg_view_hook` 20, `dynreg_info` 200, the reserved rows 8 x 64 + 16 x (60 + 28) bytes; the
  runtime object grew by about 6 KB (`registry.rs`, 2 KB state). The filter adds a few instructions to each list build (once per boot, plus one per registry change).

## Open **[O]**

- **Digitone II.** Done, see finding 33 (page rows, the list getter hook at `jsr (A3)`, the view hook, a DN2 parameter spec).
- **Audio of a run-time machine.** The DSP's type -> selector table is part of the DSP image; the selector word of every reserved type is type 0's, so a run-time
  clone of SLICE plays as SAMPLE (the audition ran without fault; PCM not compared). Options in the design note (a reserved row with a fixed selector, or the SHARC
  side of Phase 5 loading selector words).
- **The view pointer** is kept for as long as the firmware keeps the view (it builds it once, at boot); if a firmware path destroyed the view without building a
  new one, the registry would call the builder on a stale pointer. Not seen; not excluded.
- **Unloading a machine a track uses** is refused since finding 33 (`E_TYPE_IN_USE`, -26; the active kit's 16 tracks, read through three firmware function anchors). A track that holds an unloaded type (a project load, another kit) behaves as on a gap row, as described above.
- `[[voice]]` records; more than 8 machines or 16 parameters; a machine with `position` (list order is fixed at patch time). Pages and `own_dsp`: finding 33.
- Hardware: nothing flashed. The registry only writes data (no code, no cache maintenance needed), but the real D-cache and the UI task's use of the
  tables were not exercised outside the emulator.
- The refusal codes are in `dkrt-sdk` since finding 33 (-20 to -26).
