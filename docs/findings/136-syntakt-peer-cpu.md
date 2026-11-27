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
  It sets up DDR (0xFC0B8000), runs a RAM test, then acts as DSPI0
  (0xFC05C000) master: it sends command 3 and a 3-byte address (placeholder
  literal 0x12345678 at 0x80000648), reads bytes until 0xAA, reads a
  big-endian 4-byte length, copies the payload to 0x40000400 and jumps
  through `jmp *(0x40000400)`. The other end of this link is MAIN's DSPI2 in
  slave mode, which streams section 7 (see "Image upload"). **[O]** Who sets
  DSPI0 to master before stage-1 (section 6 only ORs 0xC00 into MCR), how
  section 6 itself reaches the peer, and how the command bytes meet the
  stream MAIN already sends.
- **[D]** Section 7 code ends near 0x40009f1c (about 40 KB, 132 functions).
  Above it is data, including a second Xilinx-style bitstream at 0x40041060
  that the peer loads through DSPI 0xFC03C000.
- **[D]** Memory: SDRAM 0x40000000-0x48000000, SRAM 0x80000000-0x80010000.
  The startup copies .data from 0x4004f6e0 to 0x80000000 and from 0x40057670
  to 0x80008000. It zeroes 0x4004f6e0 up to 0x4404f980.
- **[D]** Section 8 is an NXP i.MX RT Cortex-M image (IVT at 0x1000, entry
  0x60042000). It is byte-identical on DT2, DN2 and Syntakt.

## Mailbox and frames

- **[D]** Emulated without a peer CPU: the mailbox responder answers MAIN's handshake (finding 70 "Mailbox responder") and receives
  both images through channel 29. The image MAIN sends as id 7 is section 7 byte for byte (SHA-256 checked); the id 6 image is
  section 6 with byte 1 set to 0x0f by MAIN.

- **[D]** Window layout: +0 "HO" or "NO", +2 command, +4 0xA5A5, +8 state
  (1 to 0xC), +0xC chunk sequence, +0x800 a 2 KB chunk. Chunks go to
  0x4004f750 + seq * 0x800 (64 MB, probably 64 sample slots). MAIN tests the
  window with 0xAAAAAAAA and 0x55555555 and writes 0x4230 at +0. MAIN's hello
  check needs 0x484F at +0 and 0x4841 at +2.
- **[D]** Sample upload: `FUN_40082334` builds 0xAA, a length and 64 sample
  slots (read by `FUN_401133ce`) at 0x4d84f2c0; `FUN_40082f90` sends them in
  2 KB chunks through the window (+0x800).

## Image upload

- **[V]** MAIN's DSPI2 (0xEC038000) is a slave: MCR 0x00010C00 (MSTR = 0) at
  0x40082bfc. `vector_149_handler` (0x400831d6) re-arms eDMA channel 29 with
  source 0x4f84f2c1.
- **[D]** MAIN's peer thread (entry 0x40082c04, created by `FUN_40082e9c` from
  the boot thread) reads ids 6 and 7 from the OS directory in NOR at 0x80000
  ("ELE3", 16-byte entries from 0x80020: +0 id, +4 offset, +8 length; the same
  container layout as DT2/DN2) through DSPI 0xFC05C000. It builds
  0xAA | big-endian length | payload buffers: id 6 at 0x4f8af2c1 (after eight
  0xFF bytes), id 7 at 0x4f84f2c1 (length limit 0x5fffb; section 7 is 383,760
  bytes). eDMA channel 29 writes them one byte per request to the DSPI2 slave
  PUSHR (0xEC038037): first id 6 (TCD with DREQ), then `vector_149_handler`
  streams id 7 (major loop 0x6000, source keeps advancing). So MAIN sends
  section 7 to the peer at every boot.
- **[D]** After state 0xC the peer installs an ISR on vector 96 (DTIM0). Each
  frame it renders 8 voices x 32 samples with the kernels at 0x4324, 0x112c,
  0x2320, 0x168e and 0x19d6. The eDMA channel 47 (TCD 0xFC0455E0) writes
  1 KB to window + 0x1b0. There is no SSI or SAI.
  **[O]** The frame rate source, the MAIN-side per-frame writer and the
  command ids.
- **[D]** EMAC density: A-line words are 3.1% of section 7 and 1.2% of DT2
  MAIN. Of 11,181 decoded instructions, 286 are `mac.l`, 73 `msac.l` and 265
  `movclr`. Wavetable-style interpolation is at 0x400038f2.

## Peer CPU model (`[emulator] peer = "coldfire"`) [D]

Code: `packages/emulator/machine/src/peer_cpu.rs` (second `coldfire::Cpu`, private SDRAM 0x40000000-0x48000000, SRAM 0x80000000-0x80010000,
its own INTC0, DTIM0, PIT1, eDMA, GPIO and DSPI registers; address 0 is MAIN's 128 KB window, shared, not copied), `periph/src/peer.rs`
(image stream, doorbell, IRQ4, frame source), `boot/src/runtime.rs` `run_peer` (lock step: the peer runs the MAIN ticks MAIN ran, in
slices of 2048, one thread). `panel_drive` steps `peer` and `peerpeek:ADDR:LEN` report it.

Protocol, read from section 6 and 7 and checked in the run:

- Two pins cross the bridge. MAIN's doorbell (GPIO port A bit 4, `FUN_40082f6a`) is the peer's DTIM0 capture event: DTER bit 0 at
  0xFC070003, INTC0 source 32, vector 96. The peer's pin (port 6 bit 2: set by a byte store to PPDSDR 0xEC09401E, cleared through PCLRR
  0xEC09402A, bit 2 = 0) is MAIN's EPORT IRQ4. Each side sends an edge and waits for the other's.
- Stage 1 (section 6): DDR init, one toggle (pin high: MAIN's IRQ4 #1), wait for the capture event (polled), read section 7 from the
  stream, `jmp *(0x40000400)`. Section 7 at entry (0x40001070): copies .data, clears 64 MB (0x4004f6e0..0x4404f980), sets CS0, runs PIT1
  delays, decompresses the second bitstream and pushes it through DSPI 0xFC03C000, polls INIT (0xEC09401C bit 0) and DONE (0xEC09401D
  bit 7), builds a 256-entry vector table at 0x4004005c (all vectors to an RTE), then walks window+8 through 1 to 0xC with a toggle and a
  wait for the next capture event at each step (hello `HO` `HA` at state 4, 0xA5A5 at +4 at state 8, window test with 0xAAAAAAAA and
  0x55555555, sample chunks). At state 0xC it installs the frame handler 0x40000934 on vector 96 and spins.
- The program's first toggle clears the pin (its flag word is 1 in the image), so the pin must start high: stage 1 raised it.
- Frame handler (0x40000934), per capture event: copy window[0..0x1b0) to 0x80008000, render 8 voices x 32 samples (kernels 0x4324,
  0x112c, 0x2320, 0x168e, 0x19d6) into 0x80003840, program eDMA ch 47 (SADDR 0x80003840, DADDR window + 0x1b0, 16-byte elements, CITER
  0xde40 = linked, 64 minor loops) and start it with SSRT (0xFC04401E). The next call first waits for DONE (CSR bit 7). It toggles the
  pin when 0x40040050 is set.
- MAIN side: the audio frame handler is the eDMA channel 54 major-loop interrupt (vector 174 = INTC1 source 46, handler 0x400a2058). It
  toggles the doorbell when 0x43bdd824 is set (set by the IRQ4 handler at status 6), then forces INTC1 source 63 (INTFRCH1 bit 31) to
  run the engine tick (vector 191, handler 0x400a38ce, 7550 bytes). The tick writes the parameter block into the window and programs
  MAIN's own eDMA ch 47 (window + 0x1b0 to 0x80003d50, same TCD shape) with SSRT, then polls DONE. Its other waits: DSPI1 SR bit 28 (0xFC03C02C)
  and the scatter/gather chain of ch 30 (CSR bit 4 clears).

What replaces hardware (data transport and CPU-side peripherals only; no FPGA logic, the bitstream is never loaded or read):

- The mailbox model still streams the images and raises IRQ4 #1 at the end of the stream: that is stage 1's toggle. Stage 1 itself does not
  run. It is equivalent because its only effects are the DDR (zero in the model), the toggle, and the wait for the first doorbell edge; the
  program starts at MAIN's first doorbell edge after the stream (SP in SRAM, the stack word the entry copies is 0).
- Stubbed status bits: GPIO 0xEC09401C bit 0 (INIT), 0xEC09401D bit 7 (DONE), DSPI1 (0xFC03C000, RM Table 40-2) SR bits 31 and 28. DSPI2's
  forced TCF|EOQF applies only while the guest runs it as a master: EOQF does not work in slave mode (RM p.1189), and MAIN's DSPI2 is a
  slave here. The DSPI MCR resets to 0x4001, a slave (p.1181). DSPI1's flags stay forced: MAIN's first boot code polls its EOQF before any
  MCR write, and the board's boot stage that sets it before is not modelled.
- The peer's INTC0 resets with every source masked (IMRH/IMRL all ones, no mask-all bit: IMRL bit 0 is source 0, not used) and level masks
  0xF; SIMR/CIMR take the source in bits 5:0 and SALL/CALL in bit 6 (RM 17.2, p.338-345).
- The frame source is host timed: one eDMA ch 54 interrupt per frame, at most one pending. The converter runs from its own 48 kHz clock
  (SSI1 is a slave), a frame is 32 samples, so the frame rate is 1500 Hz of emulated time. The period in MAIN ticks is derived:
  `ips * frame_samples / sample_rate` (`[emulator.peer_audio] sample_rate, frame_samples`, `periph::clock::frame_ticks`): 126,619 ticks at
  the device's 189,928,000 instructions a second. The peer and MAIN share the tick rate.
- Instruction rate: the base model runs one instruction per 132 MHz bus tick (CPI 2.0 against the 264 MHz core). MAIN's measured busy CPI is
  about 1.39 (`devices/cycle-factors.json`), so the device sets `[emulator] instructions_per_second = 189_928_000` (264 MHz / 1.39). It is
  the default of `DIGI_CF_IPS`; an explicit `DIGI_CF_IPS` wins (`periph::clock::cf_ips`). PIT, DTIM, DTCN, the MIDI wire time, the USB frame
  and the controller's pacing all read that one rate. MAIN needs about 117,000 instructions a frame: that did not fit in 88,000 and does fit
  in 126,619.
- MAIN's eDMA: software start (SSRT) of a channel runs its whole transfer; a TCD started with E_SG reloads the descriptor chain at once
  (the ring data of ch 30 is not moved); DSPI1 SR (0xFC03C02C) reads 0x90000000. Vector 191 forced by the guest is delivered. All of this
  is on only for `peer = "coldfire"`.
- The 64 MB byte-clear loop (`clr.b (a0)+; cmp.l a0,d0; bne`) is replaced by one fill (all but the last iteration), counted as 3
  instructions per byte.

Result (cold boot, `panel_drive --after 100M`, step `peer`):

- MAIN status 0x41bbf448 = 6 and state 0x41bb32b0 = 6 from the peer's own writes (window 0x4230 0003 a5a5 0000 000c: MAIN's check word, command 3,
  the peer's 0xA5A5, state 0xC). The peer reaches its frame handler spin after 6 IRQ4 (5 peer toggles + stage 1) and 6 doorbell edges.
- With the audio frame on, every MAIN frame gives one peer frame. Without a note window + 0x1b0 (1 KB) stays zero over 300 M ticks;
  after trig 1 (code 1) and PLAY (code 37) it holds a smooth 32-bit ramp from the first sample (0x030cf208, 0x031439c5, ...) and 244 to 376
  non-zero bytes of each 1 KB sample. The screen shows track 1 "ADRIANA" at 135 BPM.
- Cost: the peer's frame handler runs 86,500 instructions on an idle frame and 87,300 (maximum 88,900) with a note. The frame
  period is 88,000 instructions at 132 MHz, so the program uses the whole frame (about 0.99). On the host the peer core ran about 86 M
  instructions a second (interpreter, EMAC): 0.65 of real time for the peer alone, before MAIN's own load.
- **[O]** The real frame rate and its source (the ch 54 period), the peer clock against MAIN's, the peer state in snapshots (not saved: a
  state restored with `peer = "coldfire"` has no running peer), the mixing of the 8 voices (the parameter block names it).

## Audio: MAIN's own mix [D]

MAIN mixes the 8 voices of the peer itself; the emulator reads MAIN's output, so no mix is guessed.

- The engine tick (vector 191 handler 0x400a38ce, `FUN_4008f242`) takes the voices that MAIN's eDMA ch 47 copied from window + 0x1b0 to
  0x80003d50 (8 voices x 32 samples x 4 bytes), applies per-voice level, pan and send gains (parameter block words, `emacSaturate`
  products, Q31), runs the effect buses (delay ring at 0x4fc00000) and writes stereo to the output buffer 0x80004760 (32 frames x 20
  words; left word 0, right word 1 of the first pair). The code ends in `FUN_4008ef42` calls that clamp and convert, and a copy loop.
- The output is not read from that buffer: the converter ring is. eDMA ch 54 (TCD 0xFC0456C0) streams SRAM 0x80000000, 0x40 loops of 0x20
  bytes (64 frames of 8 words, 2 KB), to the SSI TX FIFO 0xFC0C8000; CSR 0x0006 (INT_HALF, INT_MAJOR). The half-way interrupt is the 1500 Hz
  audio frame (32 frames per half, 48 kHz). The tick fills the second half (the model's TCD never advances, so the tick always picks it).
  Measured after trig 1 and PLAY: words 3 and 5 of every frame carry the main left and right; they equal word 1 and word 0 of the buffer
  0x80004760 shifted right by 8 (0x029c2460 gives 0x029c25); word 6 holds a small DC-like value; the other words are zero.
- Sample scale: the mixer works in Q31 (unity constant 0x80000000, saturation to 0x7fffffff); the converter words are the same value
  shifted right by 8: 24-bit signed, right-aligned in 32 bits. One full-scale sample is 2^23. The WAV scale is 2^-23, left from word 5.
  The left/right order follows the buffer (word 0 is left, word 1 right, and the ring carries word 0 in 5 and word 1 in 3) **[O]**.
- Model (`[emulator.peer_audio]` in `devices/syntakt.toml`: channel 54, slots 5 and 3, 24 bits): at MAIN's doorbell edge (the frame
  handler, before the next tick) `capture_peer_audio` reads the second half named by the channel's TCD, so the PCM is what the DMA would send,
  32 stereo samples per frame, 48 kHz. `Emulator::take_pcm` hands it to `digi_coupled::cf_audio::CfAudio`, which uses the DSP session's own
  output path (`pcm_play::PcmPlayer`, then the `live-audio` ring) and the same `audio_status` report. `DIGI_EMU_PEER_AUDIO=1` turns playback on
  (the launcher sets it); `panel_drive` steps `pcmreset` and `wav:PATH` write a float WAV.
- Analog tracks have no output here: the analog voices are the board's, not MAIN's mix of the peer.
- Proof (`syntakt-digital.wav`, 3.9 s, headless): trig 1 and PLAY give peak 0.185 and RMS 0.049; the same run without keys gives exact
  zero (2.9 s). The note is a kick: the spectrum of the loudest window peaks at 70 Hz and the envelope repeats every 1.75 s (16 steps at
  135 BPM). Real time: 0.30 of real time for the whole emulator on this host (3.9 s of audio in 12.9 s after the boot); the peer core runs about
  80 M instructions a second on the host, against about 190 M a second that it needs at 87,000 instructions in each 126,619-tick frame
  (1500 frames a second at 189.928 M ticks a second).
- At the 132 MHz one-instruction-a-tick model the period was 88,000 ticks and MAIN's own tick work did not fit in it (73 frame resyncs in
  400 M ticks, frames slower than real time). With the device's instruction rate (above) the same run, trig 1 and PLAY (`tap:1`, `wait:200M`,
  `tap:37`, `wait:200M`, headless cold boot) gives 3,475 frames in 440 M ticks (nominal 3,475.1), no resync inside the window (30 during
  the boot), a WAV of 111,200 frames (2.3167 s, as many seconds as 440 M ticks at 189.928 M) and the same WAV hash in two runs. **[D]**
- **[O]** Other output words (cue, FX out) and the later stages of the tick, the level knob path, snapshot of the peer.

## Hardware (hearsay)

- **[D]** Forum PCB photos: two ColdFire CPUs, two Xilinx Spartan-3A
  (XC3S200A named), DDR2 Nanya NT5TU128M8HE. Vendor text: 128x64 OLED.

## Analysis image

`st-1.42-peer`: Ghidra project `out/ghidra-projects/st-1.42-peer-emac`, dump
`out/ghidra/st-1.42-peer-emac` (132 functions), cfdb
`out/cfdb/st-1.42-peer-emac.sqlite`. Use `fw.py -i st-1.42-peer`. The command
that builds it is in finding 70, "Analysis databases".
