# 33. Run-time machines on the Digitone II, and refusing to unload a machine a track uses (digikit runtime, Phase 5)

Plan: `docs/plans/digikit-runtime.md` (Phase 5). Design: `docs/design/digikit-registry.md`. Earlier result (Digitakt II): finding 30. Code:
`modules/core/adapters/dynreg` (`src/cf/dn2/dynreg.s`), `modules/core/runtime/src/rs/src/registry.rs`, `packages/patchkit/src/register.rs`, `tests/fixtures/modules/dyn-dn2`,
`tools/patchkit/pk_registry.py`, `c4d86e4e:tools/patchkit/pk_dynreg_rules.py`. Lane r2b, 2026-10-17, one agent, native emulator, Digitone II 1.11 and 1.12, Digitakt II 1.16
and 1.17 (stock images; nothing ran on a device). Everything is **[D]** (measured here once; no second agent has checked it against the image bytes).

## Result

**[D]** A `.dkm` that holds typed MACHINE, PARAM and PAGE records adds a machine with its own SYN page to the Digitone II machine list at run time, on both images, with
no re-patch:

| Step (DN2 1.11 and 1.12) | Result |
| --- | --- |
| patch `core/runtime` + `core/rows` (so `core/rows`, `core/rows`) | 643 656 of 25 976 832 bytes of `cf.expansion` (1.12); machine rows for types 5 to 23 (19 rows), 16 parameter rows, 8 page rows; `dynreg_info` version 2 |
| pack `test/dyn-dn2` (machine `own_dsp`, 4 parameters, 1 page with knobs A to D): `patchkit pack test/dyn-dn2 --device dn2` | 708 bytes: header, a 4-byte image, 540 bytes of records (4 PARAM, 1 MACHINE, 1 PAGE) |
| 11 edited copies (wrong device, bad version, type 9, type 24, `clone_of` a run-time type, `own_dsp` 2, position 3, page for a type not in the file, page knob a stock code, page knob naming a missing parameter, unknown record kind) | refused with -21, -5, -22, -22, -22, -5, -5, -5, -5, -5, -5; **no byte of any row, table or page row changes** |
| list getter (`FUN_4004d8b6`, called through a stub that loads `a0`) before the load | `0 2 1 3 4` (the built-in order) |
| load `dyn-dn2.dkm` | 0; row 16: `clone_of` 0, `own_dsp` 1, position 255; **only row 16, names/permission entry 16, page row 0 and parameter rows 0 to 3 changed**; `mf_dispatch(16)` returns row 16; page row 0: machine 16, codes `dp0 .. dp3, 0, 0, 0, 0, 10`, slots `25 26 27 28 0 0 0 0 0`, name `DYN`, title `Dyn Synth`; parameter rows: mirror index = slot (25 to 28), ranges and defaults as the records; the getter returns `0 2 1 3 4 16` |
| load a second file with type 16 | -23 (`E_TYPE_USED`), nothing written |
| a track of the active kit put on type 16 (headless, see below), unload | **-26 (`E_TYPE_IN_USE`)**, rows, tables, page rows and list unchanged; with the track back on type 0: 0 |
| unload | the machine rows, names, permission, page rows and parameter rows are byte for byte the start (but for the string-representation, name and short pointers of the free rows); getter back to `0 2 1 3 4` |
| load, unload again | all 0; the rows after the reload equal those after the first load |
| write watch on the rows and tables (page rows included), the runtime's code the only allowed writer | 54 events, all by `core/rows` (`mf_ensure`: two representation pointers of each of 19 machine rows and of the 8 reserved page rows); control run (runtime not allowed): 514 more events, all the runtime's |
| flush check | 0 hits |
| `uv run python -m tools.patchkit.pk_registry proof --image dn2-1.12` (and 1.11, and `--quick`: a subset of the refused files, no second reload) | all checks hold, about 10 s |

**[D]** On the panel (coupled daemon, FUNC+SRC, `pk_registry ui --image dn2-1.12`, frames pinned in `tests/patchkit/registry_frames.json`, identical on 1.11 and 1.12): after the load
the machine list shows DYNSYN after MIDI, with a separator line between the groups; YES YES puts it on track 1; SYN shows the title line "Dyn Synth" and four knobs ALPH, BETA,
GAMA, DELT with the defaults of the records (the four boxes under them empty); TRIG 1 plays without fault (silence: the DSP renders nothing for the type). The unload with the track on
the machine returns -26 and the SYN page is the same frame; with the track on the first machine of the list the unload returns 0, page row 0 is free again (`machine` =
0x7fffffff), and the list frame equals the one before the load; the reload and a new selection give the first SYN frame again. **Control:** with the registry's view pointer set to 0
the load succeeds and the list the panel shows is still the stock list (the same frame): the rebuild of the cached list is what makes the machine appear.

## What was found

- **[D]** The Digitone II list code has the same shape as the Digitakt II's, with three differences. (1) The getter `FUN_4004d8b6` (1.11 and 1.12; it is the function whose entry
  `core/rows` hooks as `cf.machines.list_entry`) copies its static list into the vector it returns through `jsr (A3)`, A3 holding the library `std::copy` (loaded at
  entry), so the Digitakt II's `retarget` of a `jsr abs.l` does not apply. The six bytes at +0xde, `jsr (A3); move.l D0,(4,A2)`, are replaced by a `hook` with
  `displaced = "drop"`: the routine (`dynreg_list_hook`) calls the same filter as the Digitakt II (`dynreg_copy`), stores the result and jumps to the instruction after. (2) There is no
  sort and no rank map: the static list is a copy of `mf_list_buf`, which `core/rows` builds in order at first use. (3) Nothing else: the reserved rows are in the list from patch time
  (position 255, in type order, after MIDI) and the filter leaves out those whose name is empty.
- **[D]** The machine selection view's item builder is `FUN_4005b2c0(view)` (called by `MachineSelectionView::ctor_dtor`, which fills `view + 0x184`), **the same function as the
  Digitakt II's `FUN_40060f50`**: the same `h_strict` hash `aba88709...` on all four images, so one anchor rule serves both (`cf.dynreg.msv_build`, devices dt2 and dn2). The view is built
  once at boot and keeps its list: the registry's view pointer is set before any panel input and is the same after FUNC+SRC. The control above shows that without the rebuild the
  machine does not appear.
- **[D]** The page rows are what `core/rows` already uses for a patched machine with pages (`mf_typemap0/1/2`, `mf_reader`): they are looked up by the row's `machine` field at each
  call, so a page row written at run time needs no cache maintenance. A free page row's `machine` is **0x7fffffff, not -1**: a track without a sound reads the type as -1
  (`track_type` returns it for track 16, and the sound reader `mvs.b` sign-extends 0xff), and `mf_pg_count` compares the unsigned value with each row.
- **[D]** The Digitone II parameter row is the same 60 bytes; the lowering spec of `dynreg_info` differs from the Digitakt II's in two words (the mirror index is the sound-array slot
  itself, spec word 0x108, and the template words 0x18 and 0x1c are copied instead of computed). The spec is data (offsets 120 to 180 of `dynreg_info`); the runtime names no device.
- **[D]** `own_dsp` of a MACHINE record is written to the row's `O_OWN`; `mf_frametype` then sends the machine's own type to the DSP, which the FM engine treats as MIDI (renders nothing).
- **[D]** The firmware's own accessors give the machine type of a track of the active kit, on both devices, with the same code shape: `project()` (the lazily built Project
  singleton: `FUN_4018ac5a` on DN2 1.12, `FUN_40198b7e` on DT2 1.17, 1030 callers), `kit_of(project)` (`move.l 4(sp),d0; addi.l #0xf4,d0; rts`: `FUN_4003e41a` on DN2, `FUN_40041d24` on DT2, the
  same `h_strict` hash on both) and `track_type(kit, t)` (`FUN_4003134e` on DN2, `FUN_40036266` on DT2: -1 for track 16, 4 (DN2) or 5 (DT2) when the track's bit in a mask at kit
  data + 0x5cda is set, else the sound reader on the sound object `kit + 0x6c + t * 0x3c0`, whose virtual function at vtable + 0x28 returns the record that holds the type byte, at +0xde on
  DN2 and +0xa2 on DT2, read with `mvs.b`). The machine selection view's constructor calls exactly this chain (`FUN_4003134e(kit, FUN_40046bfe(settings))`). The anchors
  are rules of `core/rows` (`cf.dynreg.project`, `kit_of`, `track_type`; `pk_dynreg_rules --check` resolves them on all four images, each in one function). The runtime calls them
  (addresses in `dynreg_info` bytes 244 to 252; the 16 tracks in byte 256) instead of knowing a layout. On DN2 with `core/rows` the `track_type` tail jumps to the unhooked copy of the reader, so it
  returns the real type of a clone too.
- **[D]** The proof makes a track hold the machine without the panel: a 60-byte stub in free RAM calls `project`, `kit_of` and the sound's virtual getter and writes the type byte
  of track 5 (the layout above is only in the proof; the runtime does not use it). The stub reads back what it wrote (type 16), so the refusal is checked against a value the
  firmware's own accessors return.
- **[C]** Finding 30 lists the refusal as needing "a scan of the 16 tracks' type bytes, which is firmware layout and not an anchor yet": it is not a layout scan; three function anchors
  replace it.
- **[D]** Cost: `dynreg_info` 264 bytes; `dynreg.s` DN2 432 bytes in all; the runtime's registry grew by the page code and `in_use` (the registry state by 8 pages x (2 x 24 + 2 x 48) bytes
  = about 1.2 KB of bss). `RUNTIME_VERSION` stays 1 on purpose (a bump rebuilds every object that includes `dkrt-sdk`, which other lanes own): a runtime without PAGE records refuses such a file with `E_FORMAT`; the integrator may bump it once.
- **[D]** Images without `core/rows` are byte-identical to before (`cargo test` of `packages/patchkit` and `mise run verify`; the types table change adds `[dn2.dynamic]` and the
  `core/rows:dyn16..23` numbers, which only `core/rows` uses).

## Open **[O]**

- **Audio.** A Digitone II run-time machine renders silence until a DSP routine for its type is loaded (lane A: `core/dsp-link`, `[[voice]]` records). The Digitakt II selector-word problem of finding 30 stays open.
- **Only the active kit is scanned** by the unload refusal. A project's other 128 kits and the sound pool hold their own type bytes; one that names an unloaded type loads as a free row
  (Digitakt II: the SRC page shows empty boxes, finding 30; Digitone II: the machine behaves as a clone of type 0 with no page, not traced further).
- The refusal does not consider a machine currently selected in the open machine list (the view rebuilds its items; the cursor and heap use across many rebuilds were not examined).
- Digitone II 1.11: the chain `project`, `kit_of`, `track_type` and the list hook are resolved by hash on 1.11 and the proofs pass there; the panel proof ran on both.
- A machine with `position`, a second page per machine (the page rows allow up to 8 per file; one page was run), a page knob that is a stock code (the packer refuses it), and more than 8
  machines, 16 parameters or 8 pages at a time.
- Hardware: nothing flashed; only data is written at run time, but the real D-cache and the UI task's use of the tables were not exercised outside the emulator.
