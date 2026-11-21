# Filter dispatch on DT2 1.17 and DN2 1.12

The two stock filter stages use different loop registers on the two devices. Callback and state addresses are covered on the host by `filter_gain_reserves_one_state_per_voice_and_rejects_a_bad_callback`. That test does not exercise the loops below. See [the filter dispatch tables](148-filter-machines.md) and [the ColdFire filter rows](158-filter-descriptor-rows.md).

## Static observations

| At the hook | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Coefficient stage address load | `0x1c66e0` | `0x1c91a1` |
| Selector from the parameter record | `R9 = DM(I0 - 2)` at `0x1c66b2`, copied to M4 | `R6 = DM(I0 - 2)` at `0x1c916c`, copied to M4 |
| Delay slots before the hook | I0 advances by 0xdc bytes. F8 gets the clamped cutoff | I0 advances by 0x234 bytes. F8 gets the clamped cutoff |
| End of the coefficient dispatch | `0x1c69ed`. The store at `0x1c69f1` needs I1 and R9 | `0x1c927e`. Increments R3, copies I15 to I4, then the store at `0x1c9289` needs R6 |
| Process range check | `0x1c6c13` | `0x1c9584` |
| Process iteration, zero-based | `32 - R7`. Initialized at `0x1c6bf2`, decremented at `0x1c6eb7`, branches back at `0x1c6ec6` | `16 - R15`. Initialized at `0x1c9563`, decremented at `0x1c98b9`, branches back at `0x1c98c6` |

**[D]** DT2 initializes R13 from a saved zero at `0x1c6501`, increments it at `0x1c6a77` and tests its right shift by five at `0x1c6ac3` before the backedge to `0x1c6530`. DN2 clears R3 at `0x1c8f4d`, increments it at `0x1c927e` and tests its right shift by four at `0x1c934e` before the backedge to `0x1c9045`. The stock per-voice correspondence is measured below. **[O]** Confirm that module callbacks receive the same allocated state through the adapter once dispatch is installed.

**[D]** The five continuation anchors under `dsp.filter.site` give the instruction after the converter load and the next and skip destinations of each dispatch. All five records, including lengths and hashes, matched a fresh `tools.patchkit.port_resolve resolve` from the two analysis images and the DN2 1.11 patch port.

## Emulator observations

Use the `emulator` base set and `--store 'test/filter-gain*'` with `dk:dev`. The trailing star includes the fixture in the card's saved boot selection. Check that the fixture loaded, select its filter, and change GAIN (knob 2) while holding cutoff fixed. Compare the record around the coefficient hook before and after that change. Record the process buffer and both loop indices for the same voice before wiring dispatch. **[O]**

**[D]** On DN2 1.12 and DT2 1.17 the selected fixture faults during emulator boot with `UnsupportedGuestPc(0x00000000)`. On DN2, repeating the same command with `--store 'test/filter-gain'` (fixture installed, saved selection empty) reaches the boot screen. The selected DN2 card manifest lists the fixture, machines service and port file. This isolates a selection-dependent boot failure on DN2. The boot fault occurs before panel input.

**[D]** A DN2 cold boot with the selected fixture retained the fault registers and stack. The last ColdFire bus PC was the final `rts` in `mf_flt_list`. The stock chooser calls its vector builder at `0x4005b4de`, but the service installer enters the replacement by an absolute jump. There is no caller return address at the top of the stack. The wrapper now jumps to exit 0 (the instruction following that stock call), `0x4005b4e4` on DN2 and `0x40061174` on DT2.

**[D]** With the rebuilt machines service, both emulators booted with the fixture selected. On track 1, open FLTR, then FUNC+SYN (FUNC+SRC on DT2), scroll to FILTER GAIN and press YES. Both choosers showed the selection dot on FILTER GAIN. A direct runtime RELOAD of `modules/filter-gain.dkm` after selection returned result 0 and unloaded=true on each emulator. MODULES showed a new ColdFire base, the same free DSP byte count and zero module faults. This reloaded the same fixture bytes. Filter callback audio remains open pending DSP dispatch.

### Encoder F and stock cutoff in the DSP parameter records

**[D]** On track 1, compare stock Multi-Mode and FILTER GAIN at low and high encoder F values, then turn encoder E (cutoff) alone. Read the parameter array with `dsp.peek` during the same TRIG 1 input after each edit. The offsets below are **bytes from the start of one record**. The stock and module filters changed the same two float fields. With the fixture the gain field went from 0 to 0.203125 (panel value 26), while cutoff stayed fixed. Moving cutoff left the gain field at 0.203125. These are the control inputs to the adapter; the module consumes the SDK callback contract.

| Firmware | Array base | Stride | Encoder E (stock cutoff) | Encoder F (module knob 2) |
| --- | --- | --- | --- | --- |
| DT2 1.17 | `0x2506ec` | `0xdc` | `+0x6c` | `+0x70` |
| DN2 1.12 | `0x2554b8` | `0x234` | `+0x1cc` | `+0x1d0` |

On DT2 the change appeared in the first two records. The corresponding halves of the input frame word at `0x2558dc + 0xfc` changed independently. On DN2 the caller loads R8 with the parameter-array base at `0x1c303b` before the call at `0x1c3044`; the encoder F change appeared at the same offset in all sixteen records. The standard fixture has one module knob. The temporary variant below adds the second.

### Third filter control and a changed-pack live reload

**[D]** In a temporary copy of the gain fixture, append an SDK page parameter named Q Probe after Gain, set the filter descriptor parameter count and the two device parameter-row budgets to 2, and keep the original module ID and filter key. Pack for each device. In the emulator booted with the ordinary gain fixture, upload and verify the new pack at the same card path and issue RELOAD directly. Both returned result 0 with unloaded=true. On FLTR page 1 the new knob appeared on encoder G. The fixture DSP still implements only the gain operation.

With track 1 selected, turn G low and high and capture the array during the same TRIG 1 input after each edit. Then turn F while holding G fixed. At panel Q Probe = 20, the third-control field was float bits `0x3e214285` (about 0.15748), up from zero. The F change affected the previously identified spread field while the third-control field stayed fixed. These values describe transport to the SDK adapter; the resonator kernel maps the third control to musical Q.

| Firmware | Third control (byte offset) | Records showing the change |
| --- | --- | --- |
| DT2 1.17 | `+0x68` | First two records, stride `0xdc` |
| DN2 1.12 | `+0x1c8` | All sixteen, stride `0x234` |

On DN2 a single record's word at `+0x1dc` also changed during the repeated note input. It did not follow the third control across all records.

### Matching a voice between stock coefficient and audio processing

**[D]** Captured SHARC registers before the coefficient dispatch, at the selector store, and before the audio loop's selector load. Run the coupled emulator from the stock ready snapshot with a 0.12 s scenario, `SHARC_PCDUMP` set to the three PCs in the table, `SHARC_PCDUMP_LIMIT=4096`, interpreter execution (`SHARC_AOT=0`, empty fast-region and block-translation lists). DT2 used `dt2-sample-trig-noqa`; DN2 used `trig1-hold`. Compare complete blocks only; register samples precede the instruction at their PC.

| Firmware | Coefficient PC and ordinal | Selector store PC and byte address | Process load PC, ordinal and byte address | Complete blocks |
| --- | --- | --- | --- | --- |
| DT2 1.17 | `0x1c66e0`, R13 | `0x1c69f1`, I1 − 65 × 4 | `0x1c6c0f`, 32 − R7, I1 | 42 (1,344 pairs) |
| DN2 1.12 | `0x1c91a1`, R3 | `0x1c9289`, I4 − 17 × 4 | `0x1c9580`, 16 − R15, I2 | 85 (1,360 pairs) |

For every pair, both ordinals equalled the zero-based slot and both byte addresses matched. Adjacent slots had distinct selector addresses four bytes apart (DT2 `0x24eeac..0x24ef28`, DN2 `0x254ad4..0x254b10`). This identifies the indexing rule for the adapter. A module call still needs to demonstrate that its coefficient write is consumed by the process callback at the same allocated state address.

## Dispatch through an SDK filter

**[D]** On DT2 1.17, load `test/sine` and `test/filter-gain`, select the sine machine on track 1, then select Filter Gain on FLTR. Turning encoder F to its lower limit wrote zero to the gain state of the first two voice slots. Raising it wrote 0.3464567 to both. The attached 250 ms note captures had 24,064 stereo sample values each. At zero gain all were zero. At the higher setting, 24,062 were nonzero, RMS was 0.015271, peak was 0.022046, and all were finite. An empty sample track gave zero PCM at both gain settings even though the state changed.

**[D]** On DN2 1.12, the emulator reached the gain process entry with type 16, a 32-sample block and a voice-state pointer equal to the first row's state base. Its live audio buffer was in the external byte-address window (one capture used `0x804acf90`). Filter code that walks a block with a DAG can keep the supplied pointer. A blanket shift right by two aliases this buffer incorrectly.

**[D]** With the SDK resonator bank selected on DT2 and the SDK sine as its input, the first two voice slots initialized (mask 3). Holding FREQ at 32258 and Q at zero, turning SPRD from 0 to 12544 changed the sine note's RMS from 0.003305 to 0.011880. Both 250 ms captures were finite and the filter fault count stayed zero. The module occupied 20,096 DSP bytes, including 32 voice states.

**[D]** In the DN2 emulator, the loaded coefficient-bound instruction at SW `0x1c917f` had bytes `8d0f2000`, which set R13 to 32. The interpreter traced 32 at the unsigned compare and at the lookup. The default accelerated run instead traced R13 equal to 7 on 6,808 visits to the lookup, all for a stock selector. Disabling generated execution restored the gain response. Generated blocks starting before the modified instruction include it inside overlapping straight-line sequences. The current region invalidation estimate in `packages/emulator/sharc/src/code_diff.rs` ends a block near the next entry and misses some of these overlaps.

**[D]** With generated blocks disabled on DN2 (`SHARC_AOT=0`), the SDK bank and sine module also ran together. Increasing SPRD from 0 to 12800 with FREQ at 32258 and Q at zero raised the captured RMS from 0.001565 to 0.005777. Both 250 ms captures were finite and the fault counter stayed at zero. The standard DT2 gain regression passed. The same DN2 regression under default acceleration reached full gain on the panel but failed its restored-audio assertion.

The identical DN2 Vitest gain regression passed with `SHARC_AOT=0`. A DT2 reload of the selected `modules/resonator-bank.dkm` returned result 0 with `unloaded=true`. The next note initialized both voice slots and the fault count remained zero. The bank's `test-rig` target exercises the captured DN2 external audio address and checks that two voices retain independent histories.

## Accelerated playback and the FLTR response display

**[C]** The DN2 gain failure above was traced to generated SHARC blocks with overlapping instruction ranges. Invalidating only to the next entry left an earlier compiled copy of the bound at 7. The emulator now invalidates overlapping candidates inside the affected code range. The ordinary DN2 gain regression passes with acceleration enabled. With the bank active, the same two sine captures on DN2 gave RMS 0.001565 and 0.005777 as in the interpreted run.

**[D]** The machines service dispatches the filter's registered draw callback at the FLTR response switch. On DT2 1.17 and DN2 1.12 the view is in A2, the destination bitmap in D3 and the selected filter type in D4. Stock cases resume at the jump table; the callback rejoins the shared page tail. In top-down OLED coordinates the graph covers x21..71 and y40..56. The SDK frame supplies the displayed cutoff, spread and Q from the current view, so an idle track redraws when its controls change.

The bank's device test selects it through the chooser, checks the response rectangle at FREQ 127, SPRD 29 and Q 3, and turns each of the three encoders independently. The native Workbench's module list and its memory readout alone cannot establish that the firmware drew the curve or ran the audio callbacks.
