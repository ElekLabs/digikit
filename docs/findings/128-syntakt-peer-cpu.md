# Syntakt OS 1.42: the second ColdFire CPU (peer)

The Syntakt has no SHARC. Its digital sound engine runs on a second ColdFire
CPU, called the peer here. MAIN, the container and the FPGA are in
[finding 70](70-syntakt-1-42.md). Marks: **[V]** checked against image bytes by
two independent checks, **[D]** read once, **[O]** open.

Sections are numbered as in finding 70. Extract:
`uv run python -m tools.firmware.extract firmware/Syntakt_OS1.42.syx -o out/sections/st-1.42`.

## Verified

- **[V]** No section holds SHARC code or a SHARC loader stream. Section 7
  starts `40001070 4e73` (DT2: `015091ad`). There are no SHARC strings.
- **[V]** Section 7 is the peer side of the MAIN mailbox. It does
  `move.w #0x4e4f` or `#0x484f` to (A2) and `#0xa5a5` at 0x40000eae to
  0x40000f40. A2 is 0: the area at 0x4004f6e0 is zeroed by init and never
  written.
- **[V]** The peer FlexBus chip select 0 is set at 0x40000560-0x4000057e:
  CSAR0 cleared (base 0), CSMR0 0x10001, CSCR0 0x100780. MAIN's CS0 is
  0x10000000 with the same mask. The peer window at address 0 is therefore
  MAIN's 0x10000000 window (128 KB).
- **[V]** The peer frame ISR at 0x40000934 copies 0x1b0 bytes from window 0
  to 0x80008000 on each frame.

## Loader and memory

- **[D]** Section 6 is the peer's stage-1 loader, linked at SRAM 0x80000000.
  It sets up DDR (0xFC0B8000), runs a RAM test, reads SPI NOR through DSPI
  0xFC05C000 (command 3), checks a 0xAA marker, reads a 4-byte length, copies
  to 0x40000400 and jumps through `jmp *(0x40000400)`. A flash offset literal
  0x12345678 at 0x80000648 is a placeholder. **[O]** Who writes the NOR.
- **[D]** Section 7 code ends near 0x40009f1c (about 40 KB, 132 functions).
  Above it is data, including a second Xilinx-style bitstream at 0x40041060
  that the peer loads through DSPI 0xFC03C000.
- **[D]** Memory: SDRAM 0x40000000-0x48000000, SRAM 0x80000000-0x80010000.
  The startup copies .data from 0x4004f6e0 to 0x80000000 and from 0x40057670
  to 0x80008000. It zeroes 0x4004f6e0 up to 0x4404f980.
- **[D]** Section 8 is an NXP i.MX RT Cortex-M image (IVT at 0x1000, entry
  0x60042000). It is byte-identical on DT2, DN2 and Syntakt.

## Mailbox and frames

- **[D]** Window layout: +0 "HO" or "NO", +2 command, +4 0xA5A5, +8 state
  (1 to 0xC), +0xC chunk sequence, +0x800 a 2 KB chunk. Chunks go to
  0x4004f750 + seq * 0x800 (64 MB, probably 64 sample slots). MAIN tests the
  window with 0xAAAAAAAA and 0x55555555 and writes 0x4230 at +0.
- **[D]** MAIN uploads 64 sample slots read from flash at boot.
  `FUN_40082334` builds the buffer and `FUN_40082f90` sends the 2 KB chunks.
  MAIN has no reference to section 7.
- **[D]** After state 0xC the peer installs an ISR on vector 96 (DTIM0). Each
  frame it renders 8 voices x 32 samples with the kernels at 0x4324, 0x112c,
  0x2320, 0x168e and 0x19d6. The eDMA channel 47 (TCD 0xFC0455E0) writes
  1 KB to window + 0x1b0. There is no SSI or SAI.
  **[O]** The frame rate source, the MAIN-side per-frame writer and the
  command ids.
- **[D]** EMAC density: A-line words are 3.1% of section 7 and 1.2% of DT2
  MAIN. Of 11,181 decoded instructions, 286 are `mac.l`, 73 `msac.l` and 265
  `movclr`. Wavetable-style interpolation is at 0x400038f2.

## Hardware (hearsay)

- **[D]** Forum PCB photos: two ColdFire CPUs, two Xilinx Spartan-3A
  (XC3S200A named), DDR2 Nanya NT5TU128M8HE. Vendor text: 128x64 OLED.

## Analysis image

`st-1.42-peer`: Ghidra project `out/ghidra-projects/st-1.42-peer-emac`, dump
`out/ghidra/st-1.42-peer-emac` (132 functions), cfdb
`out/cfdb/st-1.42-peer-emac.sqlite`. Use `fw.py -i st-1.42-peer`. The command
that builds it is in finding 70, "Analysis databases".
