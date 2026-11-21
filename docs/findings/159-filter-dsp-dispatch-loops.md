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

**[D]** DT2 initializes R13 from a saved zero at `0x1c6501`, increments it at `0x1c6a77` and tests its right shift by five at `0x1c6ac3` before the backedge to `0x1c6530`. DN2 clears R3 at `0x1c8f4d`, increments it at `0x1c927e` and tests its right shift by four at `0x1c934e` before the backedge to `0x1c9045`. These are candidate coefficient-state indices. **[O]** Confirm that coefficient and process calls for a sounding voice select the same state and that consecutive voices select different states.

**[D]** The five continuation anchors under `dsp.filter.site` give the instruction after the converter load and the next and skip destinations of each dispatch. All five records, including lengths and hashes, matched a fresh `tools.patchkit.port_resolve resolve` from the two analysis images and the DN2 1.11 patch port.

## Emulator observation still needed

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

On DT2 the change appeared in the first two records. The corresponding halves of the input frame word at `0x2558dc + 0xfc` changed independently. On DN2 the caller loads R8 with the parameter-array base at `0x1c303b` before the call at `0x1c3044`; the encoder F change appeared at the same offset in all sixteen records. **[O]** The fixture has no third knob: its Q input and the pairing of coefficient and audio state still need measurement.
