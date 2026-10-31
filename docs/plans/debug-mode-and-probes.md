# Debug mode and hardware probes (plan, 2026-10-31)

Goal: evidence from the real device to fix emulator gaps, with a small runtime. Debug logging becomes a loadable module, `digikit/debug`; the
runtime keeps fault capture. Agreed with Em on 2026-10-31. Sources: the sampler gap map (the 2026-10-31 hardware-observation handover on the sampler branch), [debug finding](../findings/46-digikit-debug.md), [dsp-link](../../modules/core/adapters/dsp-link/README.md),
[capabilities](capabilities.md), [workbench log](workbench-device-log.md).

## Rules

- The runtime keeps only what field faults need: the fault record, `dk/fault`, safe mode, the DSP hang watch and a small error and boot ring.
- A debug feature costs at most one pointer test on a hot path when `digikit/debug` is not loaded.
- Kept probes are test modules or features of `digikit/debug`. Hooks for one experiment go into a separate probe image, never into the default
  `[patch_sets]`.
- Hardware answers fixed facts: instruction results, reset values, interrupt masks, cacheable ranges, cycles per block. Races are checked in the
  emulator, where all interleavings can be forced.
- DT2 evidence holds for DT2 only.
- No flash until the change works in the emulator. Phase 1 needs one flash.
- Branches: a fix to code that exists only on the sampler branch goes on a branch from it and merges back. A fix to code on `main` is its
  own PR to `main`.

## Phase 0: static checks and measurements (no device)

- [x] IRQSTATEN **[D]**: no code in DT2 1.17 or DN2 1.12 reads or writes `0xfc0cc034` (byte scan of all sections; no base-register form). The
      firmware still depends on its enable bits: the polled driver waits for TC with IRQSIGEN cleared, and the ISR tests the CC, TC and error
      bits. Because nothing writes it, it holds the power-on value `0x117F_013F` whether RSTA resets it or keeps it. The emulator uses that value
      after RSTA; no device read is needed. The fix goes on the sampler branch, with an update to its finding 100, after a second byte check.
- [x] Runtime size (`runtime.dko.json`, 2026-10-31): text 110,290, data 3,547, bss 671,140 bytes, mostly the module arena (512 KiB), the stage
      (64 KiB) and the upload buffer (36 KiB). Log code is 4.2 KB, the log ring 16 KiB, the LOG, PEEK and LOGCFG handlers 0.9 KB. The split
      saves about 20 KB; its main value is that debug and probe code lives outside the runtime. Parts of `dlog.rs` are not logging and stay:
      module-source tracking (the owner of command ranges), the DSP heartbeat watch, step addresses and guard faults.
- [x] Probe path: a module cannot import `dl_send` or `dl_diag` (they are not in `sdk/sdk.toml`). Service modules run 30-32 times a second,
      so a handshake in slot 0 moves only 15-30 words a second. The per-block hook `dl_tick` (interrupt context, 1500 Hz) sees the previous
      reply each block. Phase 1 therefore needs a small runtime change and one flash.
- [ ] DN2 patched runtime speed. Frozen 3 s replay (2026-10-27, DN2 1.11 port): stock 1.128, runtime 1.451 DSP s per audio s; 17.9 M
      instructions interpreted against 1.9 M on stock. 68.7 % of them are `core/dsp-voice`: `_dv_scan` walks the type table for all 16 slots
      every block, also when no module is registered. 11.7 % are the barred hook helper `FUN_b8251b`, 2.6 % dsp-link. Steps: (1) dsp-voice
      skips the scan when no module type is registered and otherwise looks only at slots with a module type (one compare when unused, which
      also follows the capability rule on the device); (2) a fast-tier root at `_dv_entry`, with its data on its own page; (3) a PGO profile
      for the two-core build (`tools/emulator/native_emu.py` makes none today). Fallback roots for barred stock code give about 5 %. Own PR to
      `main`.

## Phase 1: probe kit (one small runtime change, one flash)

- [ ] Runtime: one import, `dl_set_tick_cb`, that sets a callback pointer in `dl_tick`. The callback gets the previous reply record each block.
      Off: one pointer test per block. Proven in the emulator before the flash.
- [ ] `test/sharc-probe`, an effect module (it runs every block): it runs a fixed sequence of RECIPS and RSQRTS over all 128 table indices plus
      boundaries, signs and special cases under each MODE1 setting, and returns one framed result word per block (magic, sequence, data)
      through word 25. It stays as a test module and runs on DN2 too.
- [ ] A ColdFire probe service module (it becomes `digikit/debug` in phase 3): collects the words in the callback with a sequence check and
      returns records through its own SysEx range (reserved in `sdk/sdk.toml`). About 750-1500 words a second.
- [ ] On the device: identity (firmware, runtime hash, loaded modules, a screenshot, one remote-control roundtrip), then the seed tables and
      DSP cycles per block from the health adapter (real headroom).

## Phase 2: emulator fixes

- [x] Seed tables in `sharc_core`; remove `approx_recips`; regression tests from the captured tables; one batched core regeneration ([finding 102](../findings/102-recips-rsqrts-seed-tables.md)).
- [ ] An optional cache checker that flags reads of DMA-written lines without cache maintenance. Test builds only, never the real-time path.

## Phase 3: debug mode (next planned flash)

- [ ] Split the log. The runtime keeps the fault record, `dk/fault`, safe mode, the DSP hang watch (moved out of `dlog.rs`) and a ring of about
      32 error and boot events, and holds one log-sink pointer. `digikit/debug` holds the full ring, level filtering, LOGCFG, PEEK, the track
      watches, the probe collector and their SysEx commands. It copies the boot events when it starts.
- [ ] dsp-link page mode: while `digikit/debug` asks for it, the 32 diagnostic words carry a sequence number, a page index, a checksum and 28
      payload words. The ColdFire per-block hook passes each page to the module through a pointer. Off: one flag test per block on the SHARC and
      one pointer test on the ColdFire. On: about 100 SHARC instructions per block.
- [ ] Run-time ColdFire hooks, if [capabilities](capabilities.md) step 2 shows they are possible.

## Workbench (independent of the phases)

- [ ] Log controls in the Log drawer: Debug on or off, the default level, and a level for each module. Needs a LOGCFG setter in
      `packages/dk-session`. Works with the current runtime; after the split the controls show only when the device reports LOGCFG.
- [ ] Default modules: flashing from the Firmware pane uploads and enables `digikit/remote` and `digikit/debug` in one flow. The emulator does the
      same when it loads patched firmware. A small drop-down at the firmware file chooser lists the default modules, ticked by default. Desktop
      app only, not the web app. The exact UI needs Em's OK first.

## Later

- Interrupt order traces for storage (a probe image).
- Sampler cache and reader-lifetime experiments over page mode, after the emulator checks.

## Docs to correct

- `modules/core/adapters/dsp-link/README.md` says "Emulator only" and `modules/core/adapters/health/README.md` says it has not run on a device.
  Finding 91 records a device run of `fx/freqshift` on DT2 1.17 (2026-10-30).
