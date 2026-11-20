# Syntakt bootstrap: MAIN staged at 0x40200000, in-place unpack margin

Date: 2026-11-19. Image: `st-1.42` bootstrap (section 2, loaded at 0x800003ec) and the MAIN streams of the stock image and of patchkit images. Context: [01](01-container-and-patching.md),
[70](70-syntakt-1-42.md), [150](151-syntakt-section7-pack.md). Marks: **[V]** checked against image bytes by a second agent, **[D]** read or measured once, **[O]** open.

## Result

A flashed full `device` set restarted and then stayed at the Elektron logo on every power cycle; `device-peer` (the peer image only) booted. The cause is in the bootstrap, before
MAIN or any runtime code runs:

1. **The stage [V].** The Syntakt bootstrap reads the packed MAIN (section 3) from the NOR flash to **0x40200000** and unpacks it **in place** to 0x40000400 with its depacker at 0x8000041c.
   The Digitakt II and Digitone II bootstraps (1.16, 1.17, 1.11, 1.12) use **0x40400000**. The constant is at 0x800005b2, 0x800005d2, 0x80000622, 0x8000155c and 0x8000157e of the Syntakt
   section 2 (file offsets 0x1c6, 0x1e6, 0x236, 0x1170, 0x1192) and at 0x800005cc, 0x800005ec, 0x8000063c, 0x80001d3e and 0x80001d5e of the others (loaded at 0x80000400) (file offsets 0x1cc, 0x1ec, 0x23c, 0x193e, 0x195e).
   (DT2 1.15C and DN2 1.10E also stage at 0x40200000; they are references only.)
2. **No bounds [D].** The depacker reads the stream a byte at a time on demand, writes each output byte at once, checks no bound or checksum, and ends only on the end marker. When the output
   passes the staged stream's unread part, the stream is overwritten before it is read and the decoder runs away. No timeout or check follows: MAIN is never entered.
3. **The margin [V].** `tools.firmware.aplibpack.inplace_margin(stream, dest = 0x40000400, stage)` (Rust: `aplib::inplace_margin`) is the least, over the stream, of the bytes consumed minus the output
   written past the stage. At the Syntakt stage 0x40200000: stock **+96,356**, `device-peer` +67,676, full `device` (runtime, menu, health, input, peer image) **-42,486** (unpacked 3,683,064, packed 1,544,457),
   a variant with the runtime only -33,885. At 0x40400000 all four streams are correct.
4. **The depacker on the native core [V].** `packages/emulator/boot/examples/depack_check.rs` runs the bootstrap's own depacker (entry 0x8000041c, section 2 at 0x800003ec) on a stream: with the stage
   0x40200000 the stock and the `device-peer` streams decode correctly (length and SHA-256 equal the expected MAIN); the full `device` stream stops at pc=0x80000566 ("Halted"). At 0x40400000 all three are correct.
5. **Why nothing saw it [D].** The emulator installs MAIN directly (`Emulator::new_patched`) and never runs the bootstrap's unpack. The old depack check and `aplib_check` hard-coded the Digitakt II
   stage. patchkit checked the in-place margin only for a repacked read-only section. `main_staged_max` of `devices/syntakt.toml` was a copy of the Digitakt II value (the comment said the staging
   of this OS was not read); it is the bound "MAIN's image may not reach the next 4 MiB boundary" on every device, not the stage.

## What was built

- **Device data.** `[patch] main_dest` (0x40000400), `main_stage` (0x40200000 Syntakt, 0x40400000 Digitakt II and Digitone II, each read from the device's section 2 above) and `main_margin_min` (default 8192:
  bytes of unread stream kept ahead of the output); `boot_load` (the bootstrap's load address, for `tools/firmware/aplib_check.py` and `depack_check --base`). Documented in `docs/design/device-data.md`.
- **Packing.** `aplib::pack_section_in_place`: the plain stream when its margin at the stage is at least the floor (or the output never reaches the stage: the Digitakt II and Digitone II images are
  byte for byte what they were); otherwise the same optimal path with the shortest tail of tokens written as literals (`aplib::emit`) that brings the margin to the floor, found by binary search over the
  tokens, with the margin of the final stream recomputed. The bootstrap's depacker reads literals like any other token. The full Syntakt `device` MAIN: tail of the last tokens, container
  2,072,768 bytes (23.3 KB under `container_max`), margin 8.0 KB. If the cap cannot be kept, `apply` says how many bytes the tail costs and to trim the runtime or move it out of MAIN.
- **Guard.** `verify` (with and without a library, so `preflight`, `flash` and the workbench path) refuses a stored MAIN stream whose margin at the stage is below the floor ("the unit would hang before MAIN
  starts"); `apply` makes the stream with the floor. `verify`, `preflight` and `flash` print `bootstrap margin 8.0 KB, container 23.3 KB free`.
- **Tools.** `depack_check` takes `--stage` and `--base`; `aplib_check` reads both from the device file.

## Proof [D]

See "Proof" in the commit message of the change: `depack_check` with the Syntakt bootstrap at 0x40200000 on the repaired `device` MAIN stream gives the expected length and SHA-256; one `panel_drive` boot of the
repaired image reaches the main UI with peer status 6.

## Open

- **[O]** Hardware: the repaired `device` image on the unit. The prediction from the margin is that it boots.
- **[O]** Who draws the logo while the bootstrap runs away (the panel MCU's power-on state or the bootstrap) is not known; either way MAIN had not started.
- **[O]** DMA, cache, stack, interrupt and watchdog checks of the investigation found no difference between the emulator and the unit that fits the symptom: no watchdog register is referenced in sections 2 or 3;
  the main-OS task stack high-water mark is 17.7 KB of 160 KB; no eDMA channel writes into the runtime region.
