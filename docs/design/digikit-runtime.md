# The digikit runtime, v0 (`core/runtime`)

Status: built, 2026-10-16; DSP loader added 2026-10-17 (finding 32); emulator only (DT2 1.16, 1.17 and DN2 1.11, 1.12; nothing ran on a device). Plan:
`docs/plans/digikit-runtime.md` (Phase 3). Results: `docs/findings/29-runtime-v0.md`. File format:
`docs/design/digikit-dkm.md`. Store: `docs/design/digikit-store.md`.

## What it is

A patchkit module (`modules/core/runtime`, requires `core/expansion`) written in Rust for the ColdFire (rustc_codegen_gcc,
finding 26) with two assembly files. Everything is in `cf.expansion` and part of the module's own object, so patchkit places it
like any static object:

| Part | Size (bytes) | What |
| --- | --- | --- |
| code and read-only data | 10 054 code (+ about 1 KiB data) | loader, store reader, allocator, SHA-256, API |
| `DKRT_STAGE` (bss) | 65 536 | staging buffer for the store directory and one module file |
| `DKRT_ARENA` (bss) | 524 288 | the module arena, 16-byte aligned blocks |
| `DKRT_STATE`, `DKRT_MAILBOX` (bss) | 904 + 108 | module records, log ring; the host mailbox |
| trampoline, device id | 78 + 4 | `src/cf/tramp.s`, `src/cf/<dev>/device.s` |

Total 602 324 bytes of the 1 396 736 in `cf.expansion` on DT2 1.16 (794 412 left for other modules); 26 MB regions on DN2. The
arena is the object's bss, so patchkit reserves it with the code and no static object of any module can overlap it; the reset-path
loader zeroes it before the firmware runs. Other statics are exported by name (`DKRT_STATE`, `DKRT_MAILBOX`, `DKRT_ARENA`,
`DKRT_STAGE` in `plan.json` symbols).

## Hook

One firmware site: the call at the top of the UI task's event loop that pops the next event from the UI queue. DT2 1.16
(`FUN_400337ba`, 0x40033d00; 1.17 the same address; DN2 1.11 and 1.12 0x4002f178 in code Ghidra has not made into a function):

    caseD_2:
    40033d00  pea   (0x40966f3c).l        ; the UI queue
    40033d06  jsr   FUN_40001928.l        ; pop: pends until an item arrives, returns it in d0
    40033d0c  addq.l 0x4,SP
    40033d0e  moveq 0x29,D1
    40033d10  movea.l D0,A2
    40033d12  mvz.b (A2),D0               ; the item type, then the switch (0 key, 1.., 5 timer tick ...)

A `retarget` op changes the `jsr` target to `dkrt_loop_hook`, which saves d0/d1/a0/a1, calls `dkrt_tick` (C ABI, so d2-d7 and
a2-a6 are kept) and jumps to the stock pop with the stack as the caller left it. The site is found by a masked signature of the
six words around it (`cf_words` with `as = "code"`), the same rule on both images. Why here: it runs in task context (the UI
task, which also calls the card filesystem and block read, finding 23 S3), once per event (key, timer tick, ...; about 0.2 passes
per million instructions in an idle emulator, **[D]** finding 29), before the task blocks, and nothing live is in d0/d1/a0/a1 there.
It is not an interrupt, so the loader may block in the card read.

`dkrt_tick` (a) polls the host mailbox for a command (selection, restart) and (b) calls the `tick` of every loaded module (`entry::tick_all`),
with the module as the source of what it logs and counting each call in `tick_calls`.

## C ABI (stable names; declared in `modules/core/runtime/sdk/src/lib.rs`, crate `dkrt-sdk`)

All three are for the UI task (or another task context); not from an interrupt; not reentrant. Results are `i32`; no small-struct
results.

    int32_t dkrt_load(const char *path);            // "modules/dyn-demo.dkm": store-relative; 0 or a negative E_* code. Only the
                                                    //   boot hook loads: any other caller gets E_BOOT_ONLY (-45)
    void    dkrt_status(DkrtStatus *out);           // magic 'DKRT', runtime version, device, arena base/size/free/largest,
                                                    //   module count, ticks, last result, loads, slot calls
    int32_t dkrt_module(uint32_t index, DkrtModule *out);   // name, version, base, size, slot mask, store path; E_NOT_FOUND past the end
    void    dkrt_health(DkrtHealth *out);   // CPU load, DSP block time, heap, arena, DSP area (finding 41; needs core/health for the first three)
    uint32_t dkrt_capacity(DkrtCap *out, uint32_t max);   // the CAPACITY entries (kind, limit, used, largest); returns how many (0.2.3, finding 57)
    int      dkrt_settings_get(const char *key, uint8_t *buf, uint32_t cap);      // digikit's settings record (0.5.1, finding 63): the value's length, or a negative E_SET_* code
    int      dkrt_settings_put(const char *key, const uint8_t *value, uint32_t len);  // 0 or E_SET_*; written to the card 2 s after the last change
    int      dkrt_settings_remove(const char *key);                               // 0 (also when the key was not there)
    int      dkrt_settings_flush(void);                                           // write the record to the card now if it has unwritten changes (a menu does this before a restart)
    int      dkrt_fx(uint32_t index, DkrtFx *out);                                // the loaded effects that have a page: tab, module id, allowed routes, default, route in force, saved route (FX ROUTES, FX routing step 4); E_NOT_FOUND past the end
    uint32_t dkrt_blocks(void);             // DSP blocks since boot (1500 per second), 0 without core/health: a clock for a UI
    void    dkrt_poll_set(uintptr_t fn);    // fn() is called at the end of every UI-loop pass in the UI task (0 removes it); one caller

On an error nothing is left behind. Codes: `E_NOT_FOUND` -1 (no store / no such file or module), `E_BAD_STORE` -2, `E_READ` -3,
`E_SHA` -4, `E_FORMAT` -5, `E_RUNTIME` -6, `E_IMPORT` -7, `E_NOMEM` -8, `E_INIT` -9, `E_LOADED` -10, `E_SLOT` -11, `E_BUSY` -12,
`E_TOO_BIG` -13, `E_TABLE_FULL` -14 (`E_RUNTIME` -6 is no longer returned; `E_CONTRACT` -48: built for another SDK contract, rebuild the module). The run-time registry's codes are in the same crate and the same table, -20 to -29 (`E_NO_REGISTRY` -20, `E_REG_DEVICE` -21,
`E_TYPE_RANGE` -22, `E_TYPE_USED` -23, `E_PARAM_FULL` -24, `E_PAGE_FULL` -25, `E_TYPE_IN_USE` -26: reserved; it was the unload refusal of hot reload, which is archived,
docs/design/digikit-registry.md); -30 to -39 are the DSP side's. Codes are only added, never renumbered; `result_text` (menu text)
covers -1 to -14 and shows `ERROR` for the others until a menu text is added. The addresses of these functions are symbols of the patch plan (`plan.json`
`symbols`: `core/runtime@VERSION:dkrt_load`, ...). The DIGIKIT menu (`core/digikit-menu`, which `requires` this module) names
the four as undefined symbols of its own object and patchkit links them: there is no table and no stand-in. `DkrtStatus`,
`DkrtModule`, the result codes with `result_text` (menu text) and the store reader (`dkrt_sdk::store`: header, table CRC, path and
file entries over a caller's block read and buffer) are in `dkrt-sdk`, used by both crates. The runtime keeps the store path of each
module in `DKRT_PATHS` (apart from `State`) and returns it in `DkrtModule.path`, so a menu can mark the rows that are loaded.

`dkrt_tick` is also exported, for the hook only.

### Imports (the `IMPORTS` table of `lib.rs`; `[[import]]` of `sdk/sdk.toml`)

A module reaches the runtime through named imports: the loader resolves each name of the module's import table to an address of the runtime's `IMPORTS` table, or, for a
service another module provides (`requires`), to that module's function or to a stub that answers `DK_E_MISSING`. The imports are grouped by service (`runtime`,
`samples`, `machines`, `fx`, `ui`, `input`, `usb_bulk`, `dsp_link`) and each has a `since` and a `context` (docs/plans/sdk.md section 5.1). Memory is `dk_alloc`/`dk_free` (the
arena; 16-byte aligned; not zeroed), `dk_files_read` reads a store file (through the staging buffer, so at most 64 KiB), `dk_log` writes 24 bytes of message and one word into a
16-entry ring in `DKRT_STATE` (no `core::fmt`). The helpers `memcpy`, `memset`, `memcmp`, `__muldi3`, `__udivdi3`, `__umoddi3` are the runtime's own copies, so a C module need not carry
them. A Rust module built with the repo's pipeline still links its own copies of the compiler helpers (`compiler_builtins`) into its object **[O]**.

### The entry point and the tick

A module defines `dk_module` (`init`, `fini`, `tick`, `menu`); the loader reads the four offsets from the header, binds a `dk_module_ctx` (`DKRT_CTXS[rec]`) and calls
`init(ctx)` once after the copy. After a successful `init` it publishes `tick` in `DKRT_TICKS[rec]` with one aligned 32-bit store; `dkrt_tick` calls it once per UI-loop
pass as `void (*)(dk_module_ctx *)`. The take-down of a boot-time DSP failure clears the tick first and then runs `fini(ctx)`; it runs in the UI task, the task that calls
`tick`, so no call can be in flight. The crash record names what the module code was doing (`dlog::PHASE_*`: `init`, `tick`, `fini` or a callback). The earlier slot table
(`SLOTS = 8`, `dkm_slot_<n>`, `entered[n]`) is gone with runtime 0.6.0.

### Mailbox (emulator, tests)

`DKRT_MAILBOX {cmd, seq, result, arg[96]}`: write the NUL-terminated path or name into `arg`, then `cmd` (1 load, which now answers `E_BOOT_ONLY`; 3 to 5 selection and
restart; 2 was unload and is answered `E_FORMAT`); the
next `dkrt_tick` runs it, stores `result`, adds one to `seq` and zeroes `cmd`. The emulator control socket does this with
`mem.cf.poke` (`tools/patchkit/pk_dkrt.py` uses the `cf_script` example for the same pokes). The menu calls `dkrt_selection_get`/`set`, `dkrt_restart`, `dkrt_boot_info`, `dkrt_status` and `dkrt_module` directly (finding 38).

## Arena allocator

First fit over an address-ordered free list; blocks of 16-byte multiples with a 16-byte header (size, next-free or `USED`); a free
block is merged with its neighbours. 524 288 bytes minus 16 at start. `free` ignores a pointer that is not a live block.

## Safety rules kept

- The loader reads and checks everything before the first write to the arena (`E_SHA`, `E_IMPORT`, `E_FORMAT`, ... leave the
  arena untouched; checked in `pk_dkrt proof` by the first module landing at the arena's first block after three refused files).
- Code written through the data cache is pushed (`cpushl bc`, every 16-byte line of the block) before it can be fetched; the
  emulator's flush check reports 0 hits, and reports hits when the flush is disabled (finding 29).
- Writers into `cf.expansion` are the runtime's code and code running from the arena; the write watch sees nothing else.
- The tick is published after `init` and cleared before `fini`; the runtime never runs a partly loaded module.
- There is no run-time unload: the module set is fixed per session (hot reload is archived, docs/plans/digikit-hot-reload.md, tag
  `digikit-hot-reload-v1`; `git revert` of the Phase 6 archive commit restores it).

## Limits (v0)

Module file at most 64 KiB; 8 modules; imports at most 64; no streaming; one task context; no autoload; no safe-mode key.
DN2: the flush check sees few CPUSHL on DN2 images (finding 25), so the 0-hit result there is weaker evidence.

## Registry (machines and parameters)

A `.dkm` with `register` records fills machine and parameter rows that `core/dynreg` reserved at patch time. The runtime links to
`core/dynreg`'s `dynreg_info` table optionally; `registry.rs` holds the code and its state (`DKRT_REG`). Design, record format, what is derived
state and the write order: `docs/design/digikit-registry.md`.

## DSP code of a module (state machine)

A `.dkm` with a DSP section (`docs/design/digikit-dkm.md`) loads in two parts. `src/rs/src/dsp/mod.rs` holds the DSP loader (voice, effect and sample parts: `services/*/dsp/`), `loader.rs` the
shared tail of a load. Link: `dl_send`, `dl_send_win`, `dl_poll` of `core/dsp-link` and `dyndsp_info` of `core/dyndsp`, all optional
(`[object] optional`): an image without them refuses a file that has a DSP section with `E_NO_DSP`.

1. **Synchronous part** (inside the load, in the boot hook): the file's checks, now including the DSP section (`dsp::check`: device,
   bounds, imports resolve in `dyndsp_info`, free voice rows, voice types (a machine key resolved to its assigned type, or a pinned number) in 16..23 and unused). Then `dsp::reserve` takes a range of the
   DSP area (first fit over the per-module table `DKRT_DSP.mod_off/mod_len`, 64-byte granules, aligned for the object), copies the image
   into an arena block and applies the relocations there (`abs24`/`abs32`, shifts for the placement, import values), and claims the
   voice rows. The ColdFire image is copied, relocated and flushed, the machine and parameter rows, `init` and the tick go live at once
   (the DSP frames do not flow yet at the task entry; see "Boot-time loading"), and the load returns `LOADING` (1).
2. **Asynchronous part**, one step per UI-loop pass (`dsp::step`, called from `dkrt_tick`): `PH_SEND` starts `dl_send` of the image to the
   reserved range and polls `dl_poll`; at DONE (the DSP has the bytes and has invalidated its instruction caches) `PH_ROWS` publishes each
   voice row with two one-word `dl_send_win(1, ...)` transfers (pc, then type) and waits for each; then the boot report gets the result.
   Any failure (`dl` error, 6000 passes without progress) clears the rows it published, takes the module down again
   (`loader::undo_early`: tick, `fini`, machine rows, range, arena block) and leaves `last_result` at `E_DSP_XFER`.
   Several modules queue behind each other in `DspState.waiting`, one transfer at a time.

(Hot reload archived: a load while the firmware ran used `used = 2`, a request queue of four paths, `loader::start_queued`,
`loader::complete_pending` and an unload that freed the voice rows and the range in reverse order. Those are described in
docs/plans/digikit-hot-reload.md and restored by reverting the archive commit.)

Reporting: `DkrtStatus.state` (`STATE_IDLE` 0, `STATE_LOADING` 1), `dsp_size`, `dsp_free`, `dsp_largest`;
`DkrtModule.state` (`MOD_LOADED`, `MOD_LOADING`) and `dsp_size`; `last_result` is `LOADING` (1) from the accept until the final result.
`DKRT_DSP` (a symbol of the plan) is the loader's state for tools: phase, step, counters (`transfers`, `row_writes`, `failures`), per-module
range, per-row type/owner.

Cost: the loader is about 3 KB of code and 540 bytes of state; the DSP runs the same stub as before plus a window select (about 8
instructions per chunk, none when idle).

## Boot-time loading (plan decision 9; finding 37)

The module set is fixed for a session. The selection is a record in two sectors at the end of the store range
(`docs/design/digikit-store.md`: layout, write order, +Drive round trip); the runtime loads it at boot and never writes anywhere else.

**Hook.** `dkrt_boot_hook` (`src/cf/tramp.s`) is a `hook` op at the entry of the main-OS task (anchor `cf.rtos.site.main_os_task`, found by the words
of the prologue; the displaced `link`/`movem` run in the tail). Why there: the block read exists and nothing of the UI does. The MainScreenView, and with it the machine selection view that reads the machine list **once**, is made by one of
the singletons the task builds first: the first design hooked the call that uses the MainScreenView (finding 36) and measured the view
already built there, so the registry had to rebuild its list. At the task entry the view does not exist yet: the stock builder reads the
rows, and the view hook and the rebuild (`dynreg_view_hook`, `registry::refresh_view`) are gone with the archive commit. (Proof that the boot
path does not need them: the equivalence scenarios of tests/digikit, where the stock list takes the run-time machine and the track gets its
type, pass without them; before the removal `view_rebuilds` was 0 at every boot, finding 37.)

**Sequence** (`boot.rs`, `dkrt_boot`): read both copies (`sel`), pick the valid one with the higher sequence number; decide safe mode (flag
bit 0, the key, or a boot counter of 2 or more); raise the boot counter (one copy, the one that is not current, `seq + 1`) before
anything of a module runs; load each selected path with the loader of old (the same checks; a module that fails is skipped and
reported in `DkrtBootInfo::results`, the others load). The counter is cleared by `boot::ui_pass` (called from `dkrt_tick`) once the session has been stable: 45,000 DSP blocks (30 s of the firmware's block counter) after the first UI-loop pass, at the first pass in a safe boot (`DkrtBootInfo::stable`).

**DSP code.** Measured: a transfer started at the task entry is never acknowledged (finding 37); the cause is not traced. A module with a DSP section is
therefore published in two steps: the CF part at once (machine and parameter rows, `init`, tick; `used = 1`), the DSP part queued in the
state machine of finding 32 (`Pending.early`), which sends the image and publishes the voice rows over the first UI-loop passes. Jobs
queue (`DspState.waiting`), one transfer at a time. The result of a module shows `LOADING` until then; a failure of the DSP part takes the
module down again (`loader::undo_early`, the only user of the take-down). Until the voice row is in, a saved track on the machine plays as type 0.

**API** (C ABI, `dkrt-sdk`): `dkrt_selection_get(out *mut DkrtSelection) -> i32`, `dkrt_selection_set(sel *const DkrtSelection) -> i32`
(0, `E_SEL_PATH`, `E_SEL_COUNT`, `E_SEL_WRITE`), `dkrt_restart() -> i32` (returns only when it refuses: `E_CARD_BUSY` when the eSDHC
present-state register shows a transfer, checked again with interrupts masked), `dkrt_boot_info(out *mut DkrtBootInfo)`. Mailbox: `CMD_SELECT`,
`CMD_SELECT_ONE` (one copy, for the power-cut test), `CMD_RESTART`, `CMD_HEALTH` (6: `dkrt_health` into the exported `DKRT_HEALTH`), `CMD_CARD_PROBE` 7, `CMD_DEBUG` 8, `CMD_SETTINGS_PUT`/`_GET`/`_REMOVE`/`_FLUSH` 9 to 12 (the settings store's test hooks). `dkrt_status` and `dkrt_module` keep their meaning for this session.

**Restart.** `dkrt_restart`: refuse during a card transfer; write a pending settings record (finding 65: a settings change younger than the 2 s delay survives the restart); save the working state the way the firmware does before an OS update
(`save_working_state`, `DKRT_SAVE_MODE`: the project into the arena, then the arena into the boot record; finding 40); mask interrupts; the firmware's own
sequence: wait (bounded) for the console UART8 to empty, clear DDR word 0 through the uncached alias, 25 nops, `0x80` into `RCM.RCR`
(`0xEC090000`). A restart loses the unsaved edits that the firmware itself would lose at that point; finding 40 measured it: panel pattern and sound edits do not reach the arena, so the menu asks the player to save the project first.

**Safe mode.** A boot counter in the record: raised before the loads, cleared after the stable period (`STABLE_BLOCKS`, 45,000 DSP blocks = 30 s
after the first UI-loop pass; the clock is the firmware's own block counter, anchor `cf.link.site.block_isr.counter_lit`; finding 37). After `SAFE_AFTER` (2) boots in a row
that did not stay up that long (a fault halts the CPU and the device needs a power cycle; a hang stops the UI loop), a boot loads nothing (`reason` counter). The record's flag bit 0 asks for one safe boot (cleared by the boot that
honours it). The key `SAFE_KEY_CODE` (RECORD, button 19) held at power-on is looked for in the UI queue's waiting key events: **[O]**
untested on a device, and the emulator delivers no key before the main OS runs.

**Crash capture and card safety** (finding 46). The firmware's default exception handler calls a function pointer in a RAM word and its own function halts
the core; the runtime points the word at its hook, which records the frame, the registers, 32 stack words, the module and the last events in the keep area
(`[digikit] keep_base`; RAM that the firmware does not touch and, **[O]** on the device, a soft reset may keep) and soft-resets as `dkrt_restart` does. The next
boot logs the record and counts the crashed boot as bad (a crash after the stable period leaves one bad boot counted until a boot with no crash behind it has
run its stable period). Every card write is gated by `card::admit` (store range; ours or blank), see `docs/findings/46-digikit-debug.md`.

**Emulator.** A store of `0x80` into `RCM.RCR` is a soft reset: the controller cold-boots the same patched firmware again on the card as the
guest left it (its writes laid over the image), the DSP as a device boots it (the image's own DSP code from the first
instruction, the default since 2026-10-17). A reset before the boot is ready (a module that resets in `init`, as the firmware's exception path does) restarts the boot
the same way, up to 8 times. The range `[digikit] keep_base .. + keep_bytes` of the device file is carried across the reset (a model of a device that keeps RAM),
and the RPC `cpu.exceptions {deliver: true}` makes a CPU exception run the guest's handler instead of failing the run (finding 46).
