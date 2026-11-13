# Syntakt support

Targets: Syntakt OS 1.42 (`firmware/Syntakt_OS1.42.syx`, sha256 `ded3c59f…37c1`), short id `st`. Findings:
[70](../findings/70-syntakt-1-42.md). Branches: `syntakt/bringup` (lane `sweep-gen`), `device-registry` (lane `ts-py`).

## Facts that shape the plan

- There is no SHARC. Section 7 is a ColdFire program for a second ColdFire CPU: the digital sound engine (EMAC
  oscillator kernels). It is the peer side of the MAIN mailbox ("HO"/"NO"/0xA5A5 at 0x40000eae-0x40000f40).
- The two CPUs talk through a bridge FPGA (FlexBus 0x10000000 on MAIN). MAIN uploads 64 sample slots in 2 KB chunks.
- Section 1 is the FPGA bitstream, loaded by the bootstrap. Section 8 is a Cortex-M image, byte-identical on DT2,
  DN2 and Syntakt.
- No container HMAC seal. Transport SysEx id 0x16, OS stream 0x11.

## Rules

- Never write sections 1, 6 or 7 (section 7 carries the second FPGA bitstream; the peer boots from its own NOR).
  Never emulate or reconfigure the FPGA logic. The CPU-to-CPU bridge may be modelled as a
  shared mailbox (data transport only).
- No hard-coded device lists. Device facts and capabilities come from `devices/*.toml`; code branches on
  capabilities, not on device names.
- Emulator first. Hardware only after P3 passes in the emulator.
- The analog calibration record (SPI NOR 0x300000, magic "RBOZ") is factory data. The emulator may serve a
  synthesised record from the format alone; nothing ever writes calibration data to a real unit.

## Phases

### R Device registry (lane `ts-py`, branch `device-registry`)

- [x] R0 Design: device files + `[capabilities]`/`[container]`/`[acceptance]`, `@digi/devices`, Nx halves from
      module `ports`, literal check with a shrinking baseline. Slices S0-S6.
- [x] S0 schema, readers, `@digi/devices`, device-literals check (device-registry 2590f858, 35292c12, 72ef3389).
- [x] S5 apps on the registry, faceplate component additions (merged into device-registry 8c0e4078).
- [x] S6 Python tools on the registry, port_resolve DSP by container role (merged c0a924bd).
- [ ] S1 Nx, S2 test harness, S3 emulator: one batch, one gate run.
- [ ] S4 patchkit seal and section roles (after the SDK S3 patchkit lanes merge; seal/section parts may go first).

### P0 Firmware query

- [x] `fw.py -i st-1.42` works for fn, str, search, dossier from the existing cfdb.
- [x] Peer image `st-1.42-peer` (section 7): Ghidra project, dump, cfdb; `fw.py -i st-1.42-peer` works.
- [x] `atlas build st-1.42` (atlas skips the descriptor-parameter table when absent; dt2-1.17 counts unchanged).
- [x] Image registered through the registry (S6 derives image tables from device files).
- [x] `port_resolve`: DSP stream only for a firmware with a DSP image (S6).
- [ ] One-sided `atlas catalog` (pairing with dt2-1.17 must not rewrite its catalog).
- [x] Repeatable import of a raw ColdFire section: `tools/coldfire/imagepipe.py` (dry run proven; full run not repeated).
- [x] Finding 70 corrected; peer CPU in finding 128. Anchor dry run now 81/106 rules; the old 27/172 is [O].

### P1 Emulator boots MAIN to the UI (no audio)

- [x] Inventory of boot signatures and profile fields, resolved on 1.42 (almost all match as written).
- [x] `devices/syntakt.toml` at identity level (no seal, no DSP, section roles, placeholder panel); device-profile embeds every device file (177c3b43).
- [x] Section roles come from `[container]`; a device without a DSP skips section 7 (bc109643, 0a6c0c15).
- [x] Fix `data_mask` stride (`boot/src/common.rs:103`): mask every offset; `unique_signature_data` reuses it.
      Boot crate tests pass, incl. boot anchors on dt2-1.17 and dn2-1.12 (uncommitted).
- [x] `resolve_uart`: expected bytes built from the resolved rx_isr, not a DT2 literal (uncommitted).
- [x] Five fixed addresses by signature on all four images (00abe314).
- [x] `[emulator] free_running_dtcn` in device data (DN2 [0], DT2 [], Syntakt [2]) replaces `dtcn0_on`.
- [x] Stock cold boot reaches the main screen at about 610 M instructions (5 s host): A01, with a "RUN WARM-UP CALIBRATION" dialog.
- [x] Calibration dialog [D]: boot task 0x4000abd6 shows it when the NOR record at 0x300000 (0x22d40 bytes, magic
      0x52424F5A, sum checksum at +0xC, u16 +0x10 == 1, 8 marker bytes nonzero) is invalid, or boot flag 0x80000.
- [x] Emulated NOR: the ELE3 container at 0x80000 (already served) and a synthesised calibration record at 0x300000; PCB revision byte at
      address 0 (`board_revision`). No dialog on a cold boot ([finding 70](../findings/70-syntakt-1-42.md) "Emulated NOR").
- [x] Peer responder for MAIN (no peer CPU): hello 0x484F/0x4841 at +0/+2, 0xA5A5 at +4, IRQ4 after each doorbell,
      DSPI2 eDMA ch 29 completion; status word 0x41bbf448 reaches 6 ([finding 70](../findings/70-syntakt-1-42.md) "Mailbox responder").
- [x] The peer gets section 7 from MAIN: DSPI2 slave + eDMA ch 29 stream NOR ids 6/7 (finding 128 corrected, 646a6cdb).
- [x] Ready snapshot for st: `out/ready/st-1.42/main.state` from `panel_drive --save-state` (finding 70 "Mailbox responder").
- [x] EPORT IRQ4 and the mailbox window at 0x10000000 (responder, no audio).

### P2 Workbench and faceplate

- [x] Device chosen by firmware hash (no file-name regex): launcher `launch_device` (hash, patch record), desktop `deviceOfImage`.
- [x] Launcher on device data: no DSP (`capabilities.dsp`) runs the ColdFire-only build with no audio inputs or PGO; no `[digikit]`
      starts stock with a notice; no seedable card (`capabilities.card`) gets no card options; `module:pack-all` skips non-runtime devices.
- [x] Desktop workbench runs the Syntakt: faceplate, screen, trig keys and encoder A work, LEDs lit. Transfer, Remote and the runtime
      tick are disabled with "no digikit runtime" for a device without one.
- [x] Panel codes from an emulator sweep (finding 70 "Panel sweep"); LED map for the trig keys and the four page LEDs.
- [x] `panel_leds` decoder for the Syntakt LED records (`led_protocol = "packed-states"`); page keys, RECORD and PLAY mapped.
- [ ] The other key LEDs (FUNC, TRK, PTN, BANK, arrows: several LEDs change at once, needs per-LED naming from the firmware).
- [x] Faceplate TOML from the Syntakt photo: `apps/web/src/data/faceplates/syntakt.toml`, ±0.5 mm (uncommitted).
- [x] Faceplate component: Sound and Routing icons, Modifier Setup glyph, extra LED (TRK), accent colour on knob
      sub-labels and split labels (registry S5).
- [x] Panel codes from the sweep replace the DN2 placeholders.

Run it (Em):

```sh
mise run workbench -- firmware/Syntakt_OS1.42.syx   # first run builds the ColdFire-only app (about 4 min), then boots cold
mise run workbench -- firmware/Digitakt_II_OS1.17.syx   # unchanged (run `mise run emu:ready -- dt2 --syx ...` once before)
```

The Syntakt needs no `emu:ready`: the app boots cold to the main screen in about 20 s and the CF-only build has no way to load
`out/ready/st-1.42/main.state` (that file is for `panel_drive --state`).

### P3 Patchkit, runtime and modules in the emulator

First milestone: stock 1.42 patched with core/runtime + core/expansion, then digikit/debug loaded and its event log
read over USB MIDI.

- [x] Static patch dry run of core/expansion on 1.42, byte-level, nothing run ([finding 129](../findings/129-syntakt-patch-dry-run.md)):
      only section 3 differs; `tools/patchkit/image_report.py` is the second reader. core/runtime is not in it yet (next item).
- [x] Patchkit: no-seal path, optional DSP section, refuse sections 1, 2, 4, 6, 7, 8 explicitly (from `[container]`, not names).
- [x] Runtime target on st: no device-specific values in the runtime. A per-device data object `dkrt_dev` (struct `DkrtDevice` of
      `modules/core/runtime/abi.toml`: magic, version, size, capability bits HAS_DSP, HAS_STORE, HAS_SDHC, HAS_SAFE_KEY, then console UART, DDR window and
      alias, eSDHC, safe key, keep area, DSP cycles per block, run-time machine types) is written per device by the `gas-dkdv` gen kind and placed in
      cf.expansion with the runtime; the runtime and `tramp.s` read it, and `usb-bulk` too. The static array sizes are constants of the runtime ABI file. A manifest may list
      `optional_anchors`: a missing one is linked to 0, its hook skipped, reported as `disabled`. DT2 and DN2: `image_report` shows only the runtime,
      trampoline and device objects and the relocations into them changed, and the struct values equal the old constants.
  - [x] Syntakt runtime facts, static (finding 130): UART, eSDHC, DDR, keep area, safe key, PEEK ranges (store location: proposal only).
  - [x] Device struct, gen kind, capability bits, optional anchors (host tests for each missing capability and each optional-anchor path).
  - [x] Syntakt `[digikit]`, `[board]`, `[limits]`; dry run with core/runtime + core/expansion passes `image_report` (finding 129); it boots in the emulator to the main
        screen and the runtime is ready (boot record, event ring, INFO over USB MIDI).
  - [ ] Debug module on st: needs a module store: `[card.store]` for st (finding 130 store proposal) and an eMMC image that the emulator seeds, or another way to load.
- [x] Runtime anchor proposals on 1.42 [D] (scratchpad st-1.42-anchors.tsv): 111 anchors, 73 high, 22 med/med-high,
      3 low, 15 miss. Misses: DSP link ISR (no SHARC; health loses the DSP measurement), source page hooks,
      7 storage path/directory functions (file layer rewritten), compact_set_rect, shared_ptr_as_view, popup_make.
- [x] Region proposals [D]: `cf.flash_cave` 0x40339730..0x4033a000 (after the 136-entry init table; the cave
      tool's two rejections are false positives); `cf.expansion` 0x478c0000..0x47bf0000 (finding 130: below the 4 MiB ring MAIN clears at boot).
- [x] Anchor rules on 1.42: 25 `rule.st` entries (runtime 12, menu 5, ui 3, input 2, health 2); 90 of 93
      st-applicable anchors resolve; DT2/DN2 unchanged (`rule.<short>` is resolved generically).
- [x] Port resolve for st (`devices/ports/st-1.42.toml`, 107 anchors): SHARC rules and hooks are skipped on a device without a DSP;
      the 9 shared ColdFire rules that miss list their devices; the two regions are kept by hand in the device file.
- [ ] Debug module on st: `ports`/`[targets]` add st-1.42; PEEK ranges and `sound_words` from the device file;
      `dk_time_blocks` has no SHARC block source (report unsupported); Sound layout and machine ids unchecked.
- [ ] Runtime services without a Syntakt equivalent return "unsupported" (e.g. `dk_debug_track_type` -> -2, which
      digikit/debug already handles).
- [x] core/ui, core/health, core/input, core/digikit-menu and core/introblank run on st [D] (finding 129 "Adapters on 1.42"): the SETTINGS hook,
      the idle hook, `[input]` and the input device object; the block interrupt anchors are optional (no DSP: no health windows).
- [ ] Modules declare their devices; tool and UI modules run on `st`.

### P4 Digital audio (later)

- [x] Spike [D]: section 6 is the peer's stage-1 loader (SRAM 0x80000000); it reads section 7 from the peer's own SPI
      NOR (DSPI 0xFC05C000) to 0x40000400. Peer FlexBus CS0 at 0 = MAIN's 0x10000000 window (128 KB, likely FPGA
      dual-port RAM). Peer code is about 40 KB (132 functions); the rest is data incl. a second FPGA bitstream at
      0x40041060. Per frame (DTIM0 capture, vector 96) the peer copies window[0..0x1b0) as the parameter block, runs
      8 voices x 32 samples, and eDMA ch 47 writes 1 KB to window+0x1b0. No SSI/SAI. Open: who writes the peer NOR,
      the MAIN-side per-frame writer, the frame rate source.
- Design (finding 128 "Peer CPU model"): a second `coldfire::Cpu` with its own map (private SDRAM and SRAM, its INTC0, DTIM0, PIT1, eDMA,
  GPIO, DSPI), FlexBus CS0 at 0 mapped to the same 128 KB window MAIN sees at 0x10000000 (no copy: the peer's bus calls the board). One thread,
  lock step: after every slice of 2048 MAIN ticks the peer runs the same count, so a run is deterministic. Start: section 7 as MAIN streamed it
  (SHA-256 equal to the syx section), at 0x40000400, at its entry, on MAIN's first doorbell edge after the stream; stage 1 is skipped because its
  only effects are the DDR, one pin toggle (the mailbox model raises MAIN's IRQ4 #1 for it) and that wait. `peer = "mailbox-responder"` stays as the
  fallback. Pins: MAIN's doorbell is the peer's DTIM0 capture event; the peer's pin is MAIN's IRQ4. FPGA: stub of INIT, DONE and the DSPI flags.
- [x] Slice proof: the peer runs from entry through init and the real handshake (MAIN status 6 from its own writes), installs the frame handler,
      and renders 8 voices per frame; window + 0x1b0 is zero without a note and non-zero with trig 1 and PLAY (cost 86-89 k instructions a frame,
      0.65 of real time on the host).
- [x] Audio out: MAIN mixes the voices itself; the emulator reads MAIN's converter ring (eDMA ch 54, 24-bit words, 32 frames per audio frame, 48 kHz)
      from device data (`[emulator.peer_audio]`) and feeds the DSP session's output path (`cf_audio`). Analog tracks stay silent. WAV proof:
      finding 128 "Audio". The launcher builds `cf-audio` for such a device and sets `DIGI_EMU_PEER_AUDIO=1`; the real workbench is not checked yet.
- [x] Instruction rate per device (`[emulator] instructions_per_second`, default of `DIGI_CF_IPS`) and the frame period derived from it and the
      converter's sample rate and frame size (finding 128 "Peer CPU model"): no frame overrun.
- [ ] Peer state in snapshots; other output words (cue, FX out); one instruction rate for all devices.

### R Review before merge

- [ ] Independent review of all changes on `syntakt/bringup` against main (after P4 audio merges): correctness and bugs,
      architecture (layers, services split), code quality, and device decoupling: no device-name branches or tables in
      the emulator, patchkit, tools, apps or runtime; every device fact from `devices/*.toml`. Parallel reviewers by area
      (device data and registry, emulator, patchkit and Python tools, web and desktop), one cross-cutting pass for device
      decoupling; findings verified before fixing.

- [x] Review of syntakt/bringup at abbca772: five reviewers (device data, emulator, patchkit and tools, apps,
      device decoupling); no critical findings; no path by which patchkit writes a read-only section.
- [ ] Batch A fixes (correctness and safety): Rust in branch review/rust-fixes, apps/tools/docs in review/app-fixes.
- [ ] Design follow-ups from the review:
  - Boot signatures (task_create, UART init, USB, the older *_SIG) as per-firmware rules in devices/fw, not code.
  - Split `[emulator]`: device facts (peer wiring: window, channels, vectors; peer audio path; LED protocol;
    calibration record format) apart from emulator choices (peer model, synthetic calibration, board revision).
  - A `ready_snapshot` capability instead of inferring it from "has a DSP"; peer substitute as its own flag.
  - Nx per-device halves and ready env from the registry (registry slice S1).
  - One eDMA TCD helper in periph instead of two copies; document the zero-time shortcuts (image stream, whole
    major loops, the 1500 Hz frame source) and fix them when touched.
  - Decisions for Em: stock-firmware hashes in the browser bundle (existing conflict with patchkit's rule);
    `verify` requiring the base image for devices with read-only sections.

### Later

- Syntakt machines need a peer-ColdFire module contract (not SHARC). Not planned.
- Hardware flashing.
