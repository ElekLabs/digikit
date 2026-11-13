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

## Phases

### R Device registry (lane `ts-py`, branch `device-registry`)

- [ ] R0 Design: inventory of hard-coded device lists, registry schema with capabilities, Nx per-device targets
      from module-declared devices, PR slices.
- [ ] R1+ Slices per the agreed design (behaviour-preserving for dt2/dn2).

### P0 Firmware query

- [x] `fw.py -i st-1.42` works for fn, str, search, dossier from the existing cfdb.
- [x] Peer image `st-1.42-peer` (section 7): Ghidra project, dump, cfdb; `fw.py -i st-1.42-peer` works.
- [x] `atlas build st-1.42` (atlas skips the descriptor-parameter table when absent; dt2-1.17 counts unchanged).
- [ ] Register the image through the registry (no `regions.IMAGES` row by hand).
- [ ] `port_resolve`: build the DSP stream only for a device with a DSP.
- [ ] One-sided `atlas catalog` (pairing with dt2-1.17 must not rewrite its catalog).
- [x] Repeatable import of a raw ColdFire section: `tools/coldfire/imagepipe.py` (dry run proven; full run not repeated).
- [x] Finding 70 corrected; peer CPU in finding 128. Anchor dry run now 81/106 rules; the old 27/172 is [O].

### P1 Emulator boots MAIN to the UI (no audio)

- [x] Inventory of boot signatures and profile fields, resolved on 1.42 (almost all match as written).
- [ ] Profile row `devices/syntakt.toml` (after registry S0); `readiness_contract = main-panel-v1` (no fs worker).
- [ ] Section 7 is not parsed as a SHARC stream on a device without a DSP.
- [x] Fix `data_mask` stride (`boot/src/common.rs:103`): mask every offset; `unique_signature_data` reuses it.
      Boot crate tests pass, incl. boot anchors on dt2-1.17 and dn2-1.12 (uncommitted).
- [x] `resolve_uart`: expected bytes built from the resolved rx_isr, not a DT2 literal (uncommitted).
- [ ] Five fixed addresses (task_create, uart init, queue_td, usb tx, usb register) by signature.
- [ ] DTCN free-running channels as profile data (replaces `dtcn0_on` "dn2"); Syntakt needs channel 2.
- [ ] Stock cold boot for a device without a DSP, then a ready snapshot.
- [ ] Later (audio, not UI): EPORT IRQ4, mailbox responder at 0x10000000. Unmapped pages are zero RAM today.

### P2 Workbench and faceplate

- [ ] Device chosen by firmware hash (no file-name regex).
- [ ] Panel codes and LED map from an emulator sweep.
- [x] Faceplate TOML from the Syntakt photo: `apps/web/src/data/faceplates/syntakt.toml`, ±0.5 mm (uncommitted).
- [ ] Faceplate component: per-device wordmark, Sound and Routing icons, Modifier Setup glyph, extra LED (TRK),
      accent colour on knob sub-labels and split labels (registry S5).
- [ ] Panel codes from the sweep replace the DN2 placeholders (TODO(panel sweep) in the file).

### P3 Patchkit, runtime and modules in the emulator

First milestone: stock 1.42 patched with core/runtime + core/expansion, then digikit/debug loaded and its event log
read over USB MIDI.

- [ ] Static patch dry run on 1.42 as soon as the anchors exist (byte-level, before any emulator run).
- [ ] Patchkit: no-seal path, optional DSP section, refuse sections 1, 2, 4, 6, 7, 8 explicitly.
- [x] Runtime anchor proposals on 1.42 [D] (scratchpad st-1.42-anchors.tsv): 111 anchors, 73 high, 22 med/med-high,
      3 low, 15 miss. Misses: DSP link ISR (no SHARC; health loses the DSP measurement), source page hooks,
      7 storage path/directory functions (file layer rewritten), compact_set_rect, shared_ptr_as_view, popup_make.
- [x] Region proposals [D]: `cf.flash_cave` 0x40339730..0x4033a000 (after the 136-entry init table; the cave
      tool's two rejections are false positives); `cf.expansion` 0x47e8b000..0x47fe0000 (static evidence only).
- [ ] Anchor rules on 1.42: about 50 rules hit as written; the rest need `rule.st` entries (after registry S0/S6).
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
