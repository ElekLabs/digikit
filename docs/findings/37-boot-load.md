# 37. Boot-time loading: the hook, the selection record, safe mode, restart (digikit runtime, Phase 6)

Plan: `docs/plans/digikit-runtime.md` (decision 9, Phase 6). Spike: `docs/findings/36-boot-load-spike.md`. Design: `docs/design/digikit-runtime.md`
("Boot-time loading"), `docs/design/digikit-store.md` ("The selection record"). Code: `modules/core/runtime` (`boot.rs`, `dsp.rs`, `loader.rs`,
`sdk` `sel`), `packages/emulator` (the RCM soft-reset model), proof `tools/patchkit/pk_dkrt_boot.py`. Lane r4, 2026-10-17, native emulator, Digitakt II 1.16
and 1.17 and Digitone II 1.11 and 1.12 (stock images; nothing ran on a device). Everything is **[D]** (measured here once; no second agent has
checked it against the image bytes) unless marked **[O]**.

## Result

**[D]** A selection saved in the card's store range is loaded at boot, before the firmware builds its machine tables, and plays like the same
module patched in. `uv run python -m tools.patchkit.pk_dkrt_boot proof --image dt2-1.17` (also `dt2-1.16`, `dn2-1.12`, `dn2-1.11`; `--quick` runs fewer cut
cases and one restart):

| Scenario | Result |
| --- | --- |
| equivalence, DT2: `test/dyn-pluck` selected, cold boot | `results [0]`; its voice row `{18, 0xb8d740}`; the DSP area holds the relocated image byte for byte; the machine row is named; the stock machine list takes it (the track gets type 18); `view_at_boot` 0 and `view_rebuilds` 0; **PCM of the note equals the patched-in `digikit/pluck` sample for sample (95 808 samples, 0 differ)** |
| equivalence, DN2 1.12 and 1.11: `test/dyn-dn2` + `test/dyn-sine-dn2` | `results [0, 0]`; machine row 16 named; voice row 18 claimed; the DSP image byte for byte; the sine's PCM equals the patched-in `test/dn2-sine` (type forced in the DSP frame copy as in finding 32) sample for sample from the onset, the note starting one block apart (onset shift -64) as in finding 32 |
| restart | a card with no record boots with no module; the mailbox selection writes both copies (same sequence number); `dkrt_restart` + the emulator's soft reset: the next boot loads the module; select nothing + restart: the next boot loads nothing and the voice rows are free |
| power cut between the two writes | A new / B old: the boot uses A (seq 2); A torn (bad CRC) / B old: the boot uses B (seq 1) and loads what it holds; A old / B new: B; both invalid: no module; and the real case, one copy written by the runtime (`CMD_SELECT_ONE`) then restart: the new copy is used |
| safe mode, counter | a selection with a module whose `init` resets the machine: boot 1 raises the counter to 1, boot 2 to 2, boot 3 is safe (`reason` counter, counter seen 2), loads nothing, and its first UI-loop pass clears the counter to 0 with the selection kept |
| safe mode, flag | the record's flag bit 0: safe (`reason` flag), no module, the flag cleared, the selection kept |
| write-watch, flush check, card store | 0 stray writes into `cf.expansion` over every emulator of the run (cold boots, reset boots, the installed runtime; the staging buffer and the record buffer are DMA targets and left out), 0 flush hits; the only sectors in the store range the guest wrote are the two record sectors |

## What was found

- **[C]** Finding 36 put the hook just before the MainScreenView constructor and said the machine view is built after it. **Measured here: the view
  already exists there.** The call at DT2 `0x40033cf2` only uses the MainScreenView singleton getter (25 call sites); one of the singletons the task builds
  first made it, and with it `KitBrowser` and `MachineSelectionView`, which reads the machine list once. `DkrtBootInfo::view_at_boot` was non-zero at that
  hook and at the view-system-init call before it, so the registry had to call the builder again (`view_rebuilds` 1). The hook is now a `hook` op at the
  **entry of the main-OS task** (DT2 `0x400337ba`, DN2 `0x4002ed34`; anchor `dkrt.hook.boot`, the displaced `link`/`movem` run in the tail):
  `view_at_boot` is 0, no view hook and no second builder run, and the stock list shows the run-time machine.
- **[D]** At that point the DSP cannot take code: a transfer started at the task entry was never acknowledged (the sender's 900-block timeout, `E_DSP_XFER`).
  The frames that carry the link start later (with the sound engine; the order inside the task was not traced further **[O]**). Design: a module with a
  DSP section goes live in two steps, the CF part at the hook (machine and parameter rows, `init`, slots) and the DSP part (image, voice rows) over
  the first UI-loop passes, queued behind each other (`DspState.waiting`); the result shows `LOADING` until then. A failure of the DSP part takes the
  module down again (`undo_early`). **Consequence:** until the voice row is in, a saved track on the machine plays as type 0 (the selector of a reserved
  type). By the ready point of the cold boot the DSP part is done (`at_ready` in the report).
- **[D]** The block read and write work at the task entry (the eMMC mutex and the scheduler exist). The firmware's block write
  (`FUN_4012e0c0` / `FUN_4012c780`, hash `0dd5a176...` on all four ports) has the signature `int f(sector, bytes, buf)` of the read, and takes the mutex itself.
- **[D]** Boot delay: the cold boot of the image with the module selected and without it reach the ready point at the same ColdFire instruction
  count (1 114 485 926 on DT2 1.17), i.e. the loads cost less than the resolution of the ready detector (one DSP frame, 88 000 instructions).
  The ColdFire is idle-bound during boot. **[O]** the delay on a device, where the eMMC reads take real time (about 10 sectors for the record and the
  module: the record is read twice at boot, once more for the counter write).
- **[C]** The emulator booted a patched firmware with the **stock** DSP code and applied the patched DSP section after the ready state. A boot-time DSP load
  needs the image's own DSP from the first instruction, as on a device. The coupled cold boot starts with the DSP patch applied
  (`digi_coupled::cold::ColdInputs::dsp_patch`; the ready pair's cache key includes the DSP section). **[C]** (finding 40) It was off by default, behind
  `DIGI_COLD_DSP=patched`; it is now the only behaviour, so `mise run emu -- PATCHED.syx` boots the patched DSP too.
- **[D]** The RCM soft reset is modelled: a store of `0x80` into `0xEC090000` (`Board::soft_reset_requested`, `Status::soft_reset`) makes the controller cold-boot
  the same patched firmware again on the card as the guest left it (its writes laid over the image), the DSP as above. A reset **before** the boot is ready (a
  module that resets in `init`; the firmware's exception path ends in the same store, `0x40001ab8`, but the emulator vectors an exception and flags it as a fault
  instead of running the handler, so `test/dyn-fault` writes the reset itself) restarts the boot too, up to 8 times in a row. After a soft reset the DT2
  filesystem-check worker the readiness contract waits for does not run again (the card was used), so the reset boot is ready at the main panel
  (`Emulator::relax_fs_check`).
- **[D]** Selection record: layout and write order in `docs/design/digikit-store.md`. Two copies at `end_sector - 2` and `- 1` (`0x3dffe`, `0x3dfff`), a CRC and
  a sequence number each. A selection writes A then B with `seq + 1`; the boot-counter updates write the copy that is not current. `[card.store] selection_sectors`
  is a new profile field (default 2) that the store builder must keep its files out of (lane D).
- **[D]** `dkrt_restart` is the firmware's own sequence (interrupts masked, console drained, DDR word 0 cleared, `0x80` into `RCM.RCR`).
  The unsaved-state save is `save_working_state` (finding 40).

## Open **[O]**

- **The safe-mode key** (RECORD, button 19, `SAFE_KEY_CODE`): the code peeks the UI queue (`+4` key, `+8` state of a type-0 item) for a waiting key-down.
  The layout is read from the dispatch code, not observed; on a device the held key's event may not be queued yet at the task entry; the emulator delivers no
  key before the main OS runs. The counter and the record flag are the proven routes.
- **`dkrt_restart` during a card transfer**: the check reads the eSDHC present-state register (`0xFC0CC024`, bits 0 to 2) twice, the second time with
  interrupts masked. The emulator's controller never reports a transfer busy there, so the refusal is not exercised. A task between two blocks of a
  multi-block write holds the eMMC mutex without a transfer in flight; whether the stock firmware's own reset (`FUN_400f044a`) has the same exposure is not known.
- **Saving the working state before the reset** (finding 36, question 2): done in finding 40 (serialize + boot-record write, anchored on all four ports); the panel edits tried do not survive.
- **The module store must keep clear of the record sectors** (`selection_sectors`): `plusdrive` still lets a store fill the whole range.
- **A track that names a machine whose DSP part is not in yet** plays as type 0 for the first moments of a boot; saved projects keep the type byte.
- **Hot-only code removed** (lane r4, the Phase 6 archive commit; list in docs/plans/digikit-hot-reload.md): run-time unload, in-use scan, request queue, view hook and rebuild. `DkrtBootInfo::view_rebuilds` and `view_at_boot` went with them; the rows above that mention them are the measurement before the removal. **[D]** The equivalence scenarios (stock list takes the module, track type 18, PCM equal) pass without the view hook.

## The DSP part of a boot-loaded module: when the voice row is in, both devices (lane r4, 2026-10-17)

**[D]** Measured with `equiv-dyn-pluck` (the runner records it in the report as `boot_ready`; `BootRig.start` steps one DSP block at a time from the ready point of the cold boot
until the module's result is no longer `LOADING`). DSP blocks are 64 samples at 48 kHz, 1.33 ms; a block is 88 000 ColdFire instructions.

| Image (module `test/dyn-pluck`) | At the ready point | Voice row in |
| --- | --- | --- |
| dt2-1.17 | result 0, voice row in | **0 blocks after the ready point**: the DSP part (3.5 KB image) is done before the cold boot is ready |
| dn2-1.11 and dn2-1.12 | result `LOADING` (1), no voice row | **154 blocks, 205 ms after the ready point** (the 3.5 KB image is about 76 chunks of 48 bytes, two blocks each; finding 32 measured 153) |

- **[C]** Above, "by the ready point of the cold boot the DSP part is done" holds on the Digitakt II only. On the Digitone II the DSP part finishes about 0.2 s into the
  UI loop, so a saved track on a run-time DSP machine plays as type 0 for that long after the main screen appears (the machine row, the parameters and the page are in from the
  hook on). The time from the task entry to the ready point is not stepped: the cold boot runs on the controller's worker; its length is the ColdFire boot (1.11e9 instructions on DT2 1.17), not the load.
- **[O]** On a device the eMMC reads take real time and the chunk rate of the DN2 frame tail is the same, so the DN2 figure should hold; the DT2 one depends on how far the cold boot runs past the time the frames carry code.

## Safe mode after the first UI pass: the stable period, and what the firmware does on an exception (lane r4, Phase 8 Safety, 2026-10-18)

Marks as in the header of this file. Everything measured is the native emulator (all four images unless said); nothing ran on a device.
Kept scenario: `tests/digikit/scenarios/safe-late-fault.toml` (full tier), beside the unchanged `safe-fault`.

**[C]** Above, "its first UI-loop pass clears the counter to 0" (table row "safe mode, counter") was the rule until this change. A module that
crashed or hung ten seconds into a session was never counted: the first pass had cleared the counter, so the device crash-looped without
safe mode. The rule now: **the boot counter is cleared when the session has been stable, `STABLE_BLOCKS` = 45,000 DSP blocks (30 s) after the
first UI-loop pass.** A boot that dies earlier (a hang in the UI task stops the passes that do the clearing; a fault stops everything, below)
leaves the counter raised, so the next boots count it, and the third boot after two such boots is safe mode. A safe boot loads nothing, so
nothing can be blamed: it clears the counter at its first UI-loop pass as before (`DkrtBootInfo::stable` reports the clearing in both cases).

**[O]** The counter counts from the first boot, with or without a selection (device proof pending). A fresh device has no record: the first counted boot creates one (copy A,
`seq` 1, no modules selected) and raises its counter, so a device whose runtime hangs or crash-loops from the first boot on reaches safe mode on
the third boot like any other. On a card the runtime may not write (FOREIGN, UNREADABLE or TOO_SMALL, and a device without a store), and
and while the card state is still UNKNOWN because the card driver is not ready (the write is refused then, and that boot adds no count),
nothing is written and nothing counts: such a device never reaches safe mode by counter (the key, the DSP-stop record and the
panic record still work). Cost: two record-sector writes per boot (the raise, the clear after the stable period) on a device with no selection too. A blank card's
first write is now the record, which marks the area ours, so no boot claims the empty header pair: the claim (`Gate::Claim`) is reached only by a
write of data into a blank range before any record exists, which is rare on a device. The area is then ours with no headers, and the store
code treats that as an empty store; a store dump of such a card gives `store: null`.

### The clock: the firmware's block counter **[D]**

- **Not UI-loop passes.** Measured: the passes are timer driven, 30 to 32 a second at idle on both rigs (132 passes at the ready point, then
  +16 per 500 ms), more with panel activity. A pass count is not a time, and a heavy session would reach a pass count in a fraction of the
  time. (The `Api` comment "about 120 Hz" in `dkrt-sdk` is not what was measured; left as it is.)
- **Not a hardware timer.** The emulator does not model the DMA timer counters (`DTCNn`, `dtim.rs` "not modelled"), so nothing could prove such a
  clock here, and which of DTIM2/3 the firmware leaves alone is not established.
- **The firmware's own DSP block counter.** The tail of the block interrupt handler (vector 191, the function that `core/dsp-link` and
  `core/health` find by hash: DT2 `0x4002dd0c`, DN2 `0x40025e36`, both versions of each) adds the block to its clocks:
  `addi.l #0x7080,<a>; add.l #0x20,<b>; addq.l #1,<c>`. The word `<c>` (DT2 `0x40966840`, DN2 1.11 `0x4058e8d4`, 1.12 `0x4058f8d4`) is written
  only there (the one other place that names it in each image is a getter, `move.l <c>,d0; rts`, DT2 `0x4002dcaa`, DN2 `0x40027bd4`: scan of
  the image bytes for the literal, all four images) and counts blocks from boot: +1500 in one emulated second (measured), 5,972
  at the first UI-loop pass of a coupled cold boot and 11,921 at its ready point. The anchor `dkrt.fw.block_counter_lit` is the 4-byte address
  literal of that `addq.l` (a `cf_func` rule on the handler's hash, offset `0x1654` on the Digitakt II, `0x1d4e` on the Digitone II; the bytes
  `52b9` before it are checked on all four images). `dkrt_blocks()` now reads this counter (it used to need `core/health`) and is a name a
  module may import. **[V]** (second agent, 2026-10-18: the literal, the handler at vector 191 and the two references per image; a write
through a pointer held in memory or boot BSS zeroing cannot be excluded statically; the 1500 per second rate stays **[D]**).
- **The block interrupt keeps running when the UI task hangs**, so the counter is a clock the hung task cannot stop: the scenario's hung boots
  show the UI-loop count standing still while the counter runs.
- **No block clock (the ColdFire-only emulator rig has no DSP; the counter stays 0):** the counter is then taken for dead and cleared after
  1,800 UI-loop passes (about a minute). Only the emulator rigs meet this.

### Why 30 s

- It must cover the first things that fail: the module's own slot function from the first pass, a project load (the main screen is up about
  4 s of block time after the first pass in the emulator, 6,000 blocks) and the first seconds of playback (a pattern started by the user
  exercises every machine in it). 45,000 blocks are 7 times the time the cold boot takes to reach the ready point.
- It must not annoy: a boot counts as bad when the session ends within 30 s of the UI loop starting. Two such boots in a row (a user who
  powers on, looks, powers off, twice) make the third boot safe: one extra visit to SETTINGS > DIGIKIT > MODULES, where the selection is kept
  and APPLY & RESTART loads it again. A longer period (a minute or more) would make that surprise common; a shorter one (10 s) would miss the
  failures that show at the first note. 30 s is a judgement, not a measurement on a device **[O]**: the number is one constant
  (`dkrt_sdk::STABLE_BLOCKS`).
- Not caught: a module that hangs only another task or an interrupt while the UI task keeps running for 30 s (the UI loop clears the counter);
  a failure after the period. Both are ordinary crashes of the session, not of the boot.

### What the firmware does on a CPU exception **[D]** (static, DT2 1.16; the same `halt` sequence is in all four images)

- At boot the firmware fills the 256 vectors of its table at `0x40000000` with one default handler (`0x40001252`) and installs specific
  ones after. The default handler sets `sr` to `0x2700`, calls the function pointer in the RAM word `0x40964918` with the stack pointer, and
  returns with `rte`. `FUN_4011d1be` stores `FUN_4011d0f6` there.
- `FUN_4011d0f6` formats `EXCEPTION DS %04s`, then `V%02x M%x P%08x` (vector, format, program counter of the stack frame) and twelve stack
  words, sends them through `FUN_4011d0c4`/`FUN_4011d064` (a byte sink; whether it is the OLED or the UART console was not traced **[O]**) and
  executes **`halt`** (`4ac8`) with interrupts masked. It never returns.
- The core watchdog (`CWCR` `0xFC040016`, `CWSR` `0xFC04001B`) is not referenced anywhere in the image (searched for the literals on all four
  images; MAIN OS only: the bootloader, other sections and any watchdog on the board were not checked), so nothing in the OS resets a
halted core. **[V]** for the default handler, the hook, the `halt` and the vector-66 **[C]** below (second agent, 2026-10-18). **A fault therefore hangs the whole device until the power is cycled.** A power cycle still counts
  as the bad boot it is: the counter was raised at the start of the boot and is cleared only 30 s into a stable session.
- **[C]** Finding 36 section 3 and the first part of this finding called the reset store at `0x40001ab8` "the exception path". It is the body of
  the handler that `FUN_40001c14` puts at vector 66 (`_DAT_40000108 = 0x40001a10`, INTC0 source 2): an interrupt-driven reset request (it sets
  `0x4029e9bc`, waits, clears DDR word 0 and writes SOFTRST). Who raises that interrupt was not traced **[O]**. `test/dyn-fault` therefore models
  a reset request, not a fault; the hang of `test/dyn-fault-late` (mode 1) is the right model of an exception.

### Test **[D]** (`safe-late-fault`, full tier; coupled rigs; a power cycle is a new rig on the card the last one left)

`test/dyn-fault-late` reads `cfg/fault-after.bin` from the store (blocks after its first slot call, then 1 = hang the UI task) and stops. The
card's selection record carries a shortened period (`stable_blocks` 9,000, bytes 24 to 27 of the record, 0 = 45,000; the period has to be known
before the first UI pass, which is before any rig can poke memory). The cold boot is ready about 6,000 blocks after the first pass, and a
failure earlier than that would stop the boot from ever being ready, so the periods lie above it.

| module | failure | boot 1 | boot 2 | boot 3 |
| --- | --- | --- | --- | --- |
| EARLY: hangs 8,000 blocks after the first pass (1,000 before the period ends) | counter at boot start / cleared / record counter after | 0 / no / 1 | 1 / no / 2 | **safe mode (counter)**, module not loaded, UI keeps running, counter cleared at its first pass, selection kept |
| LATE: hangs 10,000 blocks after the first pass (1,000 after the period) | same | 0 / yes / 0 | 0 / yes / 0 | 0 / yes / 0: **no boot is safe**, the module loads every time |

Passes on all four ports (seconds with four jobs on a loaded machine: DN2 1.11 124, DT2 1.16 287, DT2 1.17 300, DN2 1.12 328; alone, DT2 1.17 takes
87 to 123 s; six cold boots per pair, the late and early sequences in two threads). The unchanged `safe-fault` (a module that resets in `init`,
three boots, safe mode, counter cleared by the safe boot's first pass) still passes on all four (47 to 69 s).

### Changes the others will meet

- `DkrtBootInfo::stable` (appended): 1 once the counter is back to 0. `ui_reached` keeps its meaning (the first pass ran). The runtime's
  `dkrt_tick` calls `boot::ui_pass` instead of `ui_reached`; the pass re-reads the record before it clears the counter (the menu may have saved
  a selection since the boot), and `selection_set` carries `stable_blocks` over.
- The record field `stable_blocks` (store spec): read at the boot hook, 0 = default. Nothing in the menu writes it.
- Test knobs: `DKRT_STABLE_BLOCKS` (4-byte aligned; the card value overrides it at boot), `DKRT_UI_START` (the block count at the first pass).
