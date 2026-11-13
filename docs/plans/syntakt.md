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
- [ ] Emulated NOR: MAIN's OS directory at 0x80000 ("ELE3", ids 6 and 7) and a synthesised calibration record.
- [ ] Peer responder for MAIN (no peer CPU): hello 0x484F/0x4841 at +0/+2, 0xA5A5 at +4, IRQ4 after each doorbell,
      DSPI2 eDMA ch 29 completion; status word 0x41bbf448 reaches 6.
- [x] The peer gets section 7 from MAIN: DSPI2 slave + eDMA ch 29 stream NOR ids 6/7 (finding 128 corrected, 646a6cdb).
- [ ] Ready snapshot for st.
- [ ] Later (audio, not UI): EPORT IRQ4, mailbox responder at 0x10000000. Unmapped pages are zero RAM today.

### P2 Workbench and faceplate

- [ ] Device chosen by firmware hash (no file-name regex).
- [x] Panel codes from an emulator sweep (finding 70 "Panel sweep"); LED map for the trig keys and the four page LEDs.
- [ ] The other key LEDs and a `panel_leds` decoder for the Syntakt LED records.
- [x] Faceplate TOML from the Syntakt photo: `apps/web/src/data/faceplates/syntakt.toml`, ±0.5 mm (uncommitted).
- [x] Faceplate component: Sound and Routing icons, Modifier Setup glyph, extra LED (TRK), accent colour on knob
      sub-labels and split labels (registry S5).
- [x] Panel codes from the sweep replace the DN2 placeholders.

### P3 Patchkit, runtime and modules in the emulator

First milestone: stock 1.42 patched with core/runtime + core/expansion, then digikit/debug loaded and its event log
read over USB MIDI.

- [x] Static patch dry run of core/expansion on 1.42, byte-level, nothing run ([finding 129](../findings/129-syntakt-patch-dry-run.md)):
      only section 3 differs; `tools/patchkit/image_report.py` is the second reader. core/runtime is not in it yet (next item).
- [x] Patchkit: no-seal path, optional DSP section, refuse sections 1, 2, 4, 6, 7, 8 explicitly (from `[container]`, not names).
- [ ] Runtime target on st (blocks the dry run with core/runtime and digikit/debug): `patchkit gen` needs DSP facts that a device
      without a DSP may not have, and the runtime's shared constants have no Syntakt value (finding 129 "Runtime target").
      Design: no device-specific values in the runtime. A per-device data object ("DKDV" struct, versioned, generated per
      device, placed in cf.expansion like `dkrt_device_id`) holds the facts and capability bits (has DSP, store, eSDHC, safe
      key). The runtime keeps only what it uses itself (console UART, DDR and keep RAM, safe key, eSDHC); services read their
      own facts (DSP timing and area: DSP link; machine type range: machines; store layout: store) from the same object.
      `[limits]` sizes static arrays and moves to the runtime ABI. Modules may list `optional_anchors`: a missing one is
      linked to 0, its hook skipped, and reported. Proof replaces byte identity: image_report shows only runtime/device
      objects changed, struct values equal the old constants, runtime device tests pass on DT2 and DN2.
  - [ ] Syntakt runtime facts, static (finding 130): UART, eSDHC, DDR, keep area, store location, safe key, PEEK ranges.
  - [ ] Device struct and gen kind, capability bits, optional anchors: after the SDK S3 branch merges (it changes the same
        runtime and patchkit files), aligned with the services split.
  - [ ] Syntakt `[digikit]`, then the dry run with core/runtime + core/expansion + digikit/debug.
- [x] Runtime anchor proposals on 1.42 [D] (scratchpad st-1.42-anchors.tsv): 111 anchors, 73 high, 22 med/med-high,
      3 low, 15 miss. Misses: DSP link ISR (no SHARC; health loses the DSP measurement), source page hooks,
      7 storage path/directory functions (file layer rewritten), compact_set_rect, shared_ptr_as_view, popup_make.
- [x] Region proposals [D]: `cf.flash_cave` 0x40339730..0x4033a000 (after the 136-entry init table; the cave
      tool's two rejections are false positives); `cf.expansion` 0x47e8b000..0x47fe0000 (static evidence only).
- [x] Anchor rules on 1.42: 25 `rule.st` entries (runtime 12, menu 5, ui 3, input 2, health 2); 90 of 93
      st-applicable anchors resolve; DT2/DN2 unchanged (`rule.<short>` is resolved generically).
- [x] Port resolve for st (`devices/ports/st-1.42.toml`, 107 anchors): SHARC rules and hooks are skipped on a device without a DSP;
      the 9 shared ColdFire rules that miss list their devices; the two regions are kept by hand in the device file.
- [ ] Debug module on st: `ports`/`[targets]` add st-1.42; PEEK ranges and `sound_words` from the device file;
      `dk_time_blocks` has no SHARC block source (report unsupported); Sound layout and machine ids unchecked.
- [ ] Runtime services without a Syntakt equivalent return "unsupported" (e.g. `dk_debug_track_type` -> -2, which
      digikit/debug already handles).
- [ ] Modules declare their devices; tool and UI modules run on `st`.

### P4 Digital audio (later)

- [x] Spike [D]: section 6 is the peer's stage-1 loader (SRAM 0x80000000); it reads section 7 from the peer's own SPI
      NOR (DSPI 0xFC05C000) to 0x40000400. Peer FlexBus CS0 at 0 = MAIN's 0x10000000 window (128 KB, likely FPGA
      dual-port RAM). Peer code is about 40 KB (132 functions); the rest is data incl. a second FPGA bitstream at
      0x40041060. Per frame (DTIM0 capture, vector 96) the peer copies window[0..0x1b0) as the parameter block, runs
      8 voices x 32 samples, and eDMA ch 47 writes 1 KB to window+0x1b0. No SSI/SAI. Open: who writes the peer NOR,
      the MAIN-side per-frame writer, the frame rate source.
- [ ] Peer as a second ColdFire core started at 0x40000400 (skip stage 1), shared 128 KB window, DTIM0 frame tick,
      FPGA DONE bit stubbed. Digital voice output read from window+0x1b0. Analog tracks stay silent; events logged.

### Later

- Syntakt machines need a peer-ColdFire module contract (not SHARC). Not planned.
- Hardware flashing.
