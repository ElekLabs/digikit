# Syntakt support

Targets: Syntakt OS 1.42 (`firmware/Syntakt_OS1.42.syx`, sha256 `ded3c59f…37c1`), short id `st`. Findings:
[70](../findings/70-syntakt-1-42.md). Branches: `syntakt/runtime` (lane `pages-deploy`, the integration branch), `device-registry` (draft PR 437); `syntakt/bringup` is merged into it.

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
- [x] Finding 70 corrected; peer CPU in finding 136. Anchor dry run now 81/106 rules; the old 27/172 is [O].

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
- [x] The peer gets section 7 from MAIN: DSPI2 slave + eDMA ch 29 stream NOR ids 6/7 (finding 136 corrected, 646a6cdb).
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

- [x] Static patch dry run of core/expansion on 1.42, byte-level, nothing run ([finding 137](../findings/137-syntakt-patch-dry-run.md)):
      only section 3 differs; `tools/patchkit/image_report.py` is the second reader. core/runtime is not in it yet (next item).
- [x] Patchkit: no-seal path, optional DSP section, refuse sections 1, 2, 4, 6, 7, 8 explicitly (from `[container]`, not names).
- [x] Runtime target on st: no device-specific values in the runtime. A per-device data object `dkrt_dev` (struct `DkrtDevice` of
      `modules/core/runtime/abi.toml`: magic, version, size, capability bits HAS_DSP, HAS_STORE, HAS_SDHC, HAS_SAFE_KEY, then console UART, DDR window and
      alias, eSDHC, safe key, keep area, DSP cycles per block, run-time machine types) is written per device by the `gas-dkdv` gen kind and placed in
      cf.expansion with the runtime; the runtime and `tramp.s` read it, and `usb-bulk` too. The static array sizes are constants of the runtime ABI file. A manifest may list
      `optional_anchors`: a missing one is linked to 0, its hook skipped, reported as `disabled`. DT2 and DN2: `image_report` shows only the runtime,
      trampoline and device objects and the relocations into them changed, and the struct values equal the old constants.
  - [x] Syntakt runtime facts, static (finding 138): UART, eSDHC, DDR, keep area, safe key, PEEK ranges (store location: proposal only).
  - [x] Device struct, gen kind, capability bits, optional anchors (host tests for each missing capability and each optional-anchor path).
  - [x] Syntakt `[digikit]`, `[board]`, `[limits]`; dry run with core/runtime + core/expansion passes `image_report` (finding 137); it boots in the emulator to the main
        screen and the runtime is ready (boot record, event ring, INFO over USB MIDI).
  - [x] Module store on st: `[card]` and `[card.store]` (the area is sectors 0x200000 to 0x300000, 1 GiB to 1.5 GiB of the card, checked against the driver's capacity, [finding 154](../findings/154-syntakt-card-capacity-store.md); [finding 138](../findings/138-syntakt-runtime-facts.md) "Store location": an emulator session wrote at most sector 0x81f7f), a card layout without kit pool, a card the emulator seeds from device data. digikit/debug uploaded over emulated USB MIDI loads (MOD_LOAD result 0) and its log is read with `dklog --emu`.
- [x] Runtime anchor proposals on 1.42 [D] (scratchpad st-1.42-anchors.tsv): 111 anchors, 73 high, 22 med/med-high,
      3 low, 15 miss. Misses: DSP link ISR (no SHARC; health loses the DSP measurement), source page hooks,
      7 storage path/directory functions (file layer rewritten), compact_set_rect, shared_ptr_as_view, popup_make.
- [x] Region proposals [D]: `cf.flash_cave` 0x40339730..0x4033a000 (after the 136-entry init table; the cave
      tool's two rejections are false positives); `cf.expansion` 0x478c0000..0x47bf0000 (finding 138: below the 4 MiB ring MAIN clears at boot).
- [x] Anchor rules on 1.42: 25 `rule.st` entries (runtime 12, menu 5, ui 3, input 2, health 2); 90 of 93
      st-applicable anchors resolve; DT2/DN2 unchanged (`rule.<short>` is resolved generically).
- [x] Port resolve for st (`devices/ports/st-1.42.toml`, 107 anchors): SHARC rules and hooks are skipped on a device without a DSP;
      the 9 shared ColdFire rules that miss list their devices; the two regions are kept by hand in the device file.
- [x] Workbench runtime path on st: `[patch_sets] emulator` (core/runtime), snapshot rate recorded per device, no boot cache with a running peer;
      runtime patched, main screen, INFO, debug module upload and log with the responder ([finding 139](../findings/139-syntakt-runtime-in-the-workbench.md)).
- [x] Duplicate replies with the peer program running: a capture artifact, fixed (finding 139 "Duplicate replies").
- [ ] Debug module on st: `ports`/`[targets]` add st-1.42; PEEK ranges and `sound_words` from the device file;
      `dk_time_blocks` has no SHARC block source (report unsupported); Sound layout and machine ids unchecked.
- [ ] Runtime services without a Syntakt equivalent return "unsupported" (e.g. `dk_debug_track_type` -> -2, which
      digikit/debug already handles).
- [x] core/ui, core/health, core/input, core/digikit-menu and core/introblank run on st [D] (finding 137 "Adapters on 1.42"): the SETTINGS hook,
      the idle hook, `[input]` and the input device object; the block interrupt anchors are optional (no DSP: no health windows).
- [ ] Modules declare their devices; tool and UI modules run on `st`.

### P4 Digital audio (later)

- [x] Spike [D]: section 6 is the peer's stage-1 loader (SRAM 0x80000000); it reads section 7 from the peer's own SPI
      NOR (DSPI 0xFC05C000) to 0x40000400. Peer FlexBus CS0 at 0 = MAIN's 0x10000000 window (128 KB, likely FPGA
      dual-port RAM). Peer code is about 40 KB (132 functions); the rest is data incl. a second FPGA bitstream at
      0x40041060. Per frame (DTIM0 capture, vector 96) the peer copies window[0..0x1b0) as the parameter block, runs
      8 voices x 32 samples, and eDMA ch 47 writes 1 KB to window+0x1b0. No SSI/SAI. Open: who writes the peer NOR,
      the MAIN-side per-frame writer, the frame rate source.
- Design (finding 136 "Peer CPU model"): a second `coldfire::Cpu` with its own map (private SDRAM and SRAM, its INTC0, DTIM0, PIT1, eDMA,
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
      finding 136 "Audio". The launcher builds `cf-audio` for such a device and sets `DIGI_EMU_PEER_AUDIO=1`; the real workbench is not checked yet.
- [x] Instruction rate per device (`[emulator] instructions_per_second`, default of `DIGI_CF_IPS`) and the frame period derived from it and the
      converter's sample rate and frame size (finding 136 "Peer CPU model"): no frame overrun.
- [x] Native kernel slice (finding 140): `cfaot` translates the voice oscillator 0x4000290e of the peer program to Rust; a kernel runs when the PC is at its
      entry, exactly as the interpreter would (compared on 47.3 M instructions of a real run and on random states), 3.4-3.6x faster on its own, 10 percent
      of the peer's wall time, 1.75 percent of the whole run's host instructions. Next kernels and MAIN's vector 191 mixer: see the projection in the finding.
      Design (P4 section of this plan, "Native kernels"):
      - Input: a data file per firmware image (`devices/cf-native/*.toml`: image SHA-256, load address, function entries). `cfaot` (Rust, the
        interpreter's own decoder, in `packages/emulator/coldfire`) follows the function's flow and writes `kernels.rs` into `out/native/cf-native/NAME`
        (firmware-derived, never committed: same rule as `SHARC_GEN_DIR`); `CF_NATIVE_DIR` makes `coldfire/build.rs` include it. Rust, not Python, because the
        decoder and every flag and EMAC helper exist once, in Rust; the SHARC generator is Python because its core semantics are.
      - Output: `kernel_N(cpu, bus, insns, budget)` per function. Specialised forms (move, lea, add, sub, shifts, mac and msac with load, movclr, ACC moves,
        branches, rts) call the interpreter's own helpers (`flags_*`, `mac_op`, `acc_to_reg`, `shift`); every other form runs through `native_exec`, the
        interpreter's step after the fetch. CCR writes that nothing reads before the next full write and that no stop point can observe are dropped.
      - Stop points: the budget at each block start, a failed bus access (before the instruction), a write over the kernel's own code. The interpreter
        continues at `pc` with the state it would have there; `icount` is exact, so emulated time does not change.
      - Match: entry address and FNV-1a of the code bytes, read through the bus at the first entry and again after any write over the guarded range
        (`DecodeCache::guard`), so a different image or modified code never runs a kernel. No device branches: the data file is the key.
      - Proof: `Cpu::check_native` (kernel and interpreter from one state on journalled buses: registers, CCR, EMAC, instruction count, every read and write in
        order), used by `DIGI_CF_NATIVE=check` over a whole run and by `coldfire/tests/native_kernels.rs` on random states.
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
  - [x] Stock-firmware hashes in the browser bundle are allowed: the web app ships device files with their `[[firmware]]` hashes to
        identify a user's firmware. patchkit's build embeds the device files whole (the stripping had no other purpose).
  - [x] `verify` requires the stock base image for a device whose `[container]` has read-only sections and fails with a clear message
        without it; only an image whose SHA-256 is a known stock image needs no base. `inspect` identifies a file and proves nothing.

### Follow-ups after the integration (branch `syntakt/runtime`, 2026-11-14)

Order: runtime safety items first, then the Nx cache gap, speed, restructure, design follow-ups.

- Open runtime items:
  - [ ] Store write safety: the runtime's writes are bounded (`card::admit`). The stock Data API takes the byte count from the host and has no slot bound
        (findings 145, 142 section 6). Add a guard at the eMMC write and erase entries in `core/runtime` before any hardware use of the Data API.
  - [ ] The `core/ui` HEALTH and MEMORY pages are unverified on the Syntakt.
  - [ ] `dk_input_turn` is unverified on the Syntakt.
- Nx cache gap: `core/input:test-device--dt2` replayed its cached result after `modules/core/adapters/ui/build/ui.dko.json` changed.
  A declared input is missing; find it with `pnpm nx:pilot show target inputs core/input:test-device--dt2 --json` and add it.
- Literal baseline: `tools/nx/device-literals.baseline.json` grew with the code merged from main; the owners of those files burn it down.
- Speed (now 0.37 of real time with audio on the Syntakt; the peer thread and MAIN run in one thread in lock step):
  - [ ] One thread per CPU (about 1.7x).
  - [ ] Peer RAM fast path: the peer bus folds bytes on every read (about 1.25x).
  - [ ] More native kernels (finding 140): the peer's other hot buckets, and MAIN's vector 191 mixer (handler 0x400a38ce).
- Emulator restructure (one slice at a time, the Syntakt run and the DN2/DT2 pinned scenarios must stay identical):
  - [ ] A `Companion` enum on the board (none, mailbox responder, ColdFire peer, DSP) instead of the peer special cases.
  - [ ] `companion/` crates: the peer CPU model and the SHARC link as siblings of the board, not inside `machine`.
  - [ ] Board options from the device file (instruction rate, frame source, audio path) in one struct.
  - [ ] One audio session for every companion (the DSP session's output path takes PCM from the peer too).
  - [ ] A framed handoff between the CPU threads (a frame of work per message, so the threads need no lock step per slice).
  - [ ] Kernel sets: `devices/cf-native/*.toml` selected by image hash from the device file, loaded at run time or built in.
  - [ ] Peer state in snapshots, and a ready snapshot that includes the peer.
- Design follow-ups (from the review):
  - [ ] Boot signatures as per-firmware rules in `devices/fw`.
  - [ ] Split `[emulator]` into device facts and emulator choices.
  - [ ] Nx per-device targets from the registry (registry slices S1 to S3).
  - [ ] One eDMA TCD helper in `periph`.
  - [ ] Document the zero-time shortcuts (image stream, whole major loops, the 1500 Hz frame source).

### Later

- Syntakt machines need a peer-ColdFire module contract (not SHARC). Not planned.
- Hardware flashing beyond the sets below.

### Hardware bring-up (branch `st/peer-pack`)

State on Em's unit: the `device-peer` image and the repaired full `device` image boot. Section 7 packing and MAIN's unpack run on the hardware ([finding 151](../findings/151-syntakt-section7-pack.md)). The runtime, the digikit menu and HEALTH run. Where the card store lives and how it is checked is in [finding 154](../findings/154-syntakt-card-capacity-store.md); the store and a content module are proven on the unit (finding 154 section 6): INFO `card_state` 1, `store put`, `ls` and `rm`, and `digikit/remote` loaded from the store with working keys.

- [x] Section 7 packed by patchkit (`[container] repack`), unpacked in MAIN by `core/peer-image`; verify checks a packed section; the image fits the OS-upgrade limit.
- [x] Bootstrap MAIN stage at 0x40200000 (`[patch] main_stage`) and the in-place margin guard: patchkit refuses an image whose margin is below the floor, and prints the budget ([finding 152](../findings/152-syntakt-bootstrap-main-stage.md)).
- [x] Hardware sets: `device` (the full set) and `device-peer` (`core/peer-image` only, the optional first flash); the workbench skips the runtime follow-ups for an image without the runtime.
- [x] The flash flow: the INFO failure of step A is confirmed through a Ping; the Ping reply layout of the Syntakt (length byte before the blob) is read at both offsets.
- [x] The MODULES freeze: `core/ui` compared key events with the Digitakt II ids; `[input.keys]` of the device file holds the Syntakt's ids and flag bits, and a module that reads a device table declares it with `needs_device` ([finding 153](../findings/153-syntakt-view-keys.md)).
- [x] HEALTH shows the card capacity (INFO `card_sectors`, `card_state`); the runtime refuses a card whose capacity is unknown.
- [x] The store at 0x200000..0x300000 of the second ColdFire's flash window, checked against the driver's capacity ([finding 154](../findings/154-syntakt-card-capacity-store.md)).
- [x] The site publishes only the devices of `SITE_DEVICES` (`apps/web/src/data/site-devices.mjs`): the build, the pages, the gallery and the emulator page follow it.
- [x] The store area and a content module on the unit: INFO `card_state` 1 and `free_bytes` 536851968, the first upload writes the headers, HEALTH STORE USED 1 KB and FREE 511.9 MB, `digikit/remote` installed, selected, applied and restarted, the Remote pane's keys work (finding 154 section 6). INFO `card_sectors` 3817472 (0x3a4000, the 0x15 `4FTE4R` part with the enhanced area set) covers the store area.
- [ ] The stock Data API write guard: Transfer must not write samples or sounds into the stock slots (design in progress). Until it lands, back up the +Drive card and do not use Transfer for them.
- [ ] CPU REL without a DSP (design in the `core/health` README).
- [ ] A kept Syntakt emulator test for `core/peer-image`: the packed boot, and a corrupted stream refused.
- [ ] Add `st` to `SITE_DEVICES` (`apps/web/src/data/site-devices.mjs`) after the hardware proof is complete.
- Recovery: hold FUNC at power-on, TRIG 4 (OS UPGRADE), send the stock file over 5-pin MIDI DIN ([PATCHING.md](../PATCHING.md) "Flashing a Syntakt").

Later:

- A digikit sample pool on the Syntakt, in the digikit store and split from the 64 stock slots (which stay Elektron's). It needs a playback contract on the second ColdFire (the Syntakt has no SHARC) and is bounded by the second CPU's sample RAM (the stock firmware loads up to 32 MB).
