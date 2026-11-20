# Syntakt OS 1.42: container, CPUs and the peer link

All claims are **[D]** (read once) unless marked **[V]** (checked by two
independent checks) or **[O]** (open). The second CPU (section 7) is in
[finding 136](136-syntakt-peer-cpu.md). Plan: [syntakt](../plans/syntakt.md).

## Container

- File: `Syntakt_OS1.42.syx`, ELE3, 2,029,486 bytes. The META stamp is
  `260930 11:17:25`, the same day as DT2 1.17 and DN2 1.12.
- sha256: `ded3c59fdb909eb9e2c3307b30a486452c8fa6947b12c14e0ce35a187b7f37c1`.
- Extract: `uv run python -m tools.firmware.extract firmware/Syntakt_OS1.42.syx -o out/sections/st-1.42`.
- The SysEx ids (transport id 0x16, OS-stream byte 0x11) are in
  [finding 01](01-container-and-patching.md).

| Section | Content | Size |
| --- | --- | --- |
| 5 | META | |
| 2 | bootstrap, dest word 0x04000000 (DT2/DN2: 0x02010000), ColdFire code at 0x8000xxxx | 15,408 packed, 30,782 |
| 1 | absent on DT2/DN2. 0xFF padding, then `aa 99 ...`, bitstream-like | 49,252 packed, 149,516 |
| 3 | MAIN at 0x40000400, same startup words as DT2 | 1,442,812 packed, 3,442,576 |
| 4 | UPDATER at 0x80000400 | 32,768 raw |
| 6 | absent on DT2/DN2. The stage-1 loader of the peer CPU, at SRAM 0x80000000 (finding 136) | 1,744 raw |
| 7 | DT2/DN2: packed SHARC loader stream. Here: the ColdFire program of the peer CPU (finding 136) | 383,760 raw |
| 8 | byte-identical to DT2/DN2 section 8 | |

- The packing codec is the same as on DT2/DN2.
- The content checksum of DT2/DN2 reproduces the stored value over the first
  2,029,392 bytes. The last 94 decoded bytes are zero padding.
- There is no HMAC trailer slot. The container (2,029,392 bytes, the value in
  its length word) ends at the end of section 8 rounded up to 16; DT2/DN2 add
  32 bytes after that. The sha256 constants are absent from sections 2, 3
  and 4.
- The per-packet checksum constant K = 0x11 verifies on all 20,094 data
  messages.
- patchkit follows `[capabilities] seal = "none"`: it builds the container
  without a trailer slot, and `patchkit rebuild` of the stock file reproduces
  it byte for byte **[V]** (sha256 equal). No key is derived.

## Images and CPUs

- MAIN is MCF5441x ColdFire code with the same reset path as DT2: reset entry
  0x400004e8, SP 0x48000000, flags word read from 4(sp).
- The Syntakt has no SHARC. Sections 6 and 7 belong to a second ColdFire CPU
  (the peer): section 6 is its loader and section 7 its sound engine. Facts
  are in [finding 136](136-syntakt-peer-cpu.md).
- MAIN reads sections 6 and 7 (ids 6 and 7 of the OS directory in NOR) and
  streams them to the peer through DSPI2 in slave mode (finding 136, "Image
  upload").

## FPGA

- MAIN configures the FPGA from section 1 **[D]**. `FUN_400bd74e` (0x400bd75e) reads id 1 of the OS directory, allocates a heap block for it,
  unpacks it with `FUN_401181ec` to 0x43be315c and stores its length at 0x43be3158. `FUN_400bcb4e` (registered at the end of `FUN_400bd74e`)
  sets the GPIO pins and DSPI 0xFC03C000 (MCR 0x80030c00), waits for INIT (0xEC09401C bit 0) and shifts the bytes from 0x43be315c out.
  Section 1 is 149,516 bytes unpacked.
- In the boot thread `FUN_40082578`, `_DAT_454e4c14 = DAT_4029a778 >> 12 & 0xf` at 0x40082798. `DAT_4029a778` is the bootstrap flags word.
- The self-test screen `FUN_40080646` (0x40081e62) decodes the value: 1 INITB never went high, 2 INITB low during data, 3 INITB low after
  data, 4 DONE never went high. This is Xilinx-style INIT_B/DONE configuration.
- The bootstrap uses the same peripherals: DSPI 0xFC03C000 (MCR 0x80030c00) at 0x80001242-0x80001276 and an eDMA TCD (0xFC0451E0..0xFC0451FE)
  that feeds PUSHR 0xFC03C034 from buffer 0x80007af0, at 0x80001066 (file offset 0 = 0x800003ec). That buffer is a double-buffered DSPI command
  queue (two halves of 0x830 bytes, frames of 0x8000AAAA .. 0x08005555), not data. Its routines have no static caller **[O]**.
- Section 7 is not read for the FPGA by MAIN. It holds a second bitstream at 0x40041060 that the peer loads through its own DSPI 0xFC03C000
  (finding 136). The peer must therefore receive the exact stock bytes ([finding 151](151-syntakt-section7-pack.md)).

## Peer link ("DSP boot")

- Status word `_DAT_41bbf448`, read by `FUN_40082a50`. Values: 0 WAIT, 1 init
  fail, 2 TIMEOUT, 3 BUFSIZE, 4 DSP_MEM, 5 CF_MEM, 6 OK.
- Poll callback `FUN_400828cc`: in state 0 it counts ticks. Above 0x12 it sets
  status 2.
- Handshake handler `vector_68_handler` (0x40083034), EPORT IRQ4, cleared by
  writing 0x10 to 0xFC090006.
- Doorbell `FUN_40082f6a` toggles GPIO PA bit 4 (PPDSDR_A = 0x10, PCLRR_A =
  0xEF).
- Mailbox in the FlexBus window at 0x10000000. State variable: 0x41bb32b0.
  - Peer magic at 0x10000000: 0x484F ("HO") with 0x4841 at 0x10000002 is
    hello. 0x4E4F ("NO") sets status 4.
  - 0x10000004 must read 0xA5A5 in state 5 for status 6.
- `FUN_400829f4` sends progress bytes.
- `FUN_40082f90` copies 2 KB chunks to 0x10000800. The sequence number is at
  0x1000000C and command 2 is at 0x10000002. The source is the sample buffer
  at 0x4d84f2c0 (0xAA, length at +1, data from +5).
- `vector_149_handler` (0x400831d6) re-arms DSPI2 (0xEC038000, slave) with
  eDMA channel 29 from the id-7 transmit buffer 0x4f84f2c1 (section 7).
- The peer thread (0x40082c04) runs only after the NOR directory reads
  succeed; the poll callback is registered last. A zeroed window never
  reaches status 6.
- Consumer: the main OS task at 0x4000abd6. It shows "DSP BOOT FAILURE
  (TIMEOUT)" at 0x4000af8c, "(CF)" for 5, "(DSP)" for 4, "(BUF)" for 3
  (strings at 0x4024bd6a-0x4024bdc3).
- Peer state names at 0x4025303e-0x402530df: STATE_LOAD_OS, INIT, BOOT,
  RUNNING, UPLOAD_SAMPLES; FAILURE_TIMEOUT, BUFSIZE, CF_MEM, DSP_MEM;
  BOOT_SUCCESS.
- The peer is the section 7 ColdFire **[V]** (finding 136). The exact reply
  sequence: **[O]**.
- To boot in the emulator, stub: the hello magic and 0xA5A5 on the window,
  IRQ4, and DSPI2 eDMA channel 29 data. This is done by the mailbox responder
  (next section).

## Anchors on 1.42

Two counts, with different scope:

- Dry run of the cf `[[anchor_rule]]` set in `devices/fw` against an in-memory
  `st-1.42` (`port_resolve.resolve_cf_anchor` per rule, `regions.IMAGES` entry
  added in memory only): 81 of 106 rules hit, 25 miss. Rules that name
  devices other than `st` are skipped. Controls with the same harness: DT2
  1.17 225/225, DN2 1.12 180/180.
- Anchor proposals for the in-scope runtime modules (111 anchors): 65 hit by a
  unique strict hash whose callees and strings also match, 15 miss, the rest
  are per-device rules or regions.

| Subsystem | Hit | Rules |
| --- | --- | --- |
| rtos | 14 | 14 |
| menu | 21 | 24 |
| storage | 6 | 6 |
| usb | 9 | 12 |
| draw, midi, text, page, project | 5, 4, 3, 1, 1 | same |
| mem | 5 | 6 |
| string | 4 | 7 |
| window | 4 | 8 |
| input | 2 | 4 |
| view | 2 | 9 |
| param | 0 | 2 |

- Misses are mostly the view, window, string (`shared_ptr`) and usb functions.
  The DSP link has no equivalent: there is no SHARC.
- A hit only means the hash is unique on 1.42. Semantic sameness: **[O]**.
- An earlier count of 27 hits of 172 rules is not reproducible: the rule set
  has grown (DT2 control: 225 rules) and the harness is not kept. It is
  replaced by the two counts above **[O]** for the exact cause.
- `cfmatch match dt2-1.17-emac st-1.42-emac` (no `--write`) pairs 9,966
  functions: 67.0% of DT2 and 76.2% of Syntakt. Methods: strict 2,947, loose
  580, strings 200, vtable 2,280, callee 1,200, caller 375, gap 2,372.
- The caller and gap stages are wrong in places. Example: the DT2 boot thread
  0x400cc9c8 is paired with the Syntakt FPGA status code.

## Emulator boot notes from OS 1.41 [D]

- Source: an old Python-emulator boot of OS 1.41 (PR #42, not merged). Measured
  once, not verified on 1.42. Re-derive every byte signature on the 1.42 image.
- The 1.41 boot reached the main screen ("A01") after about 331 M instructions
  with no FancyErrorView.
- There was no DSP model. The DSP loader gave up at about 690 M instructions
  and the UI then showed "OOPS". The audio-frame interrupt was not modelled.
- The OS busy-waits on DTCN2 (DMA timer 2 counter, loop at 0x40000f78 on 1.41).
  The bootloader leaves the timer running and the OS never writes DTMR2. The
  emulator must serve DTCN2 as a free-running counter.
- Worker pends that must stay real. A faked release ran jobs that were never
  posted and starved lower-priority tasks (1.41 names from the PR):
  - `pump_wait2`: second worker pump, prio 7. Waits on a job semaphore.
  - `hw1_wait`, `hw2_wait`, `hw3_wait`: three prio-5 hardware workers (SPI NOR
    via GPIO 0xEC0940xx; a FlexBus device at 0x10000000; a copy worker). The PR
    does not say which name is which worker. Each waits on a semaphore posted
    by an ISR.
  - `display_frame_wait`: the display task's per-frame wait on display_sem,
    posted by the display PIT3 ISR. Faked, the task spins at 100% CPU.
- Panel wire (measured by use, 1.41):
  - Button message `2c mask`: `mask` is the whole state of channel c.
  - Encoder message `3c dd`: `dd` is a signed delta.
  - Channel 0 bits 0-7 = trig keys 1-8. Channel 1 bits 0-7 = trig keys 9-16. A press of 1-12 also selects that track.
  - Channel 2 bit 7 = LFO page. Channel 3 bit 3 = NO. The full table is in "Panel sweep" below.
  - Encoders A-H = channels 0-7. LEVEL = 8.
  - The first encoder message after a page or parameter change only shows the
    value. Each later one moves by delta/6 positions.

## Cold boot in the Rust emulator [D]

- `boot_gate firmware/Syntakt_OS1.42.syx --max N --after M --frame OUT.pbm` (release build of the
  `elektron-native-boot` example). The device file `devices/syntakt.toml` gives the free-running
  DTCN2 (`[emulator] free_running_dtcn = [2]`); no other model change was needed.
- 600 M instructions: black screen, the boot thread has finished and spins at 0x4008288e.
  The main screen is reached at 609,984,465 instructions (about 5 s of host time): project
  A01 "ADRIANA" at 135.0 BPM with the dialog "RUN WARM-UP CALIBRATION, ARE YOU SURE?" on top.
  Why that dialog shows: the NOR record at 0x300000 is zero (see "Emulated NOR" below).
- Through 1.21 G instructions the display stayed the same and "DSP BOOT FAILURE" had not appeared (the peer mailbox was not modelled).
  Without the responder the poll callback sets status 2 (TIMEOUT) after 0x12 periods, at about 1.04 G instructions.

## Mailbox responder (`[emulator] peer = "mailbox-responder"`, the fallback) [D]

The default is now `peer = "coldfire"`: the peer's program runs on a second core and answers by itself ([finding 136](136-syntakt-peer-cpu.md) "Peer CPU model"). The responder below keeps the image stream and IRQ4 #1 in both modes.

- Model: `packages/emulator/periph/src/peer.rs` (state machine, no bus access), wired by `machine/src/board.rs` (channel 29, GPIO) and
  `boot/src/common.rs` `service_peer` (interrupt delivery). It runs no peer code and no FPGA logic: data transport only.
- Image stream: when MAIN arms eDMA channel 29 (DSPI2 slave PUSHR 0xEC038037), the board runs the major loops at once and feeds the bytes
  to the peer (a free-running loop stops when the second image is complete). The first transfer is the stage-1 loader: MAIN's buffer holds
  eight 0xFF bytes, then section 6 with byte 1 changed from 0x01 to 0x0f by MAIN (0x4f8af2ca), then 4 more bytes (1748 bytes). Its
  TCD has DREQ and INT_MAJOR: the peer raises the channel interrupt, INTC1 source 21, vector 149 (ICR level 1). The second transfer is
  `0xAA | length | payload` (the TCD has no DREQ and no interrupt): **its payload is section 7 byte for byte** (383,760 bytes, same SHA-256
  as `section_7_BLOB.bin`). The peer keeps both images for a later peer CPU (not in snapshots, only their lengths).
- Replies: the second image complete gives IRQ4 #1 (EPORT IRQ4, INTC0 source 4, vector 68, ICR level 6, EPIER 0xFC090003 bit 4 must be
  set). After that every doorbell edge (a change of GPIO port A pin 4: PPDSDR_A 0xEC094018 with bit 4, PCLRR_A 0xEC094024 with bit 4
  clear) gets one IRQ4 after 50,000 instructions. Before IRQ4 #2 and #3 the window holds `HO`, `HA`, 0xA5A5 at +0, +2, +4: the RAM test
  of thread 0x40083288 overwrites +0 and +4 with 0xAAAAAAAA and 0x55555555 between them, so the block is written again before #3.
- Sequence seen on 1.42 (one sample chunk, the sample buffer length is 0x100): IRQ4 #1 at 329.41 M instructions (state 0 to 1), #2 at 329.47 M
  (hello, state 2), then the thread's RAM test and its sleeps (2 s emulated, 264 M instructions), #3 to #6 at 597.05 M to 597.24 M
  (state 2 to 3, one chunk, state 4, state 5, state 6). Doorbell edges 5, IRQ4 delivered 6.
- Result: status word 0x41bbf448 == 6 and state 0x41bb32b0 == 6 after boot (read at 740 M and at 2.03 G instructions); the main screen has
  no "DSP BOOT FAILURE" at 2.03 G. Before: status 0 and the poll callback's counter would have passed 0x12 at about 1.04 G (status 2).
- Window: a 1 MiB RAM page at 0x10000000 (the window is its first 128 KB). `panel_drive` step `peer` reports bytes, image lengths and
  SHA-256, doorbell edges and IRQ4 delivered.
- Ready snapshot: `panel_drive firmware/Syntakt_OS1.42.syx --after 130M --save-state out/ready/st-1.42/main.state --steps wait:1M`
  writes the state at 741 M instructions (status 6); `--state` resumes it in 0.2 s. The peer's counters and queue are in the state
  (section `PEER`), the image bytes are not.

## Emulated NOR and the calibration dialog [D]

- The emulator already served MAIN's NOR through its `flash_read` hook (`FUN_40116d7a`, the same signature as DT2 and DN2): the ELE3 container
  of the syx at 0x80000, zero elsewhere. The calibration record at 0x300000 therefore read as zeros.
- Boot task 0x4000abd6 shows "RUN WARM-UP CALIBRATION" when `FUN_4006edca` is negative or boot flag 0x80000 is set. `FUN_4006ecb8` reads
  0x22d40 bytes from NOR 0x300000. The record is valid when long[0] == 0x52424F5A ("RBOZ"), long[+0xC] equals the sum over i = 1..0x22d30
  of `i ^ byte[0x10 + i - 1]` (32 bit, `FUN_4006eb5c`), u16[+0x10] == 1, and the bytes +0x17, +0x18, +0x19, +0xda, +0x14f, +0x15c, +0x1885
  and +0x2fae are nonzero. The firmware's own default record (+4 = 0x22d30, +8 = 1) fails the u16 test, so a blank NOR always asks.
- `[emulator] calibration_record = "synthetic"` makes the emulator build such a record from the format alone (`boot/src/nor_records.rs`):
  header, checksum, the 8 marker bytes set to 1, every table zero. It is synthetic emulator data; nothing comes from a unit, and no tool
  writes calibration data to a real unit. The OS upgrade path in the emulator rewrites the container area only below 0x380000; the
  record is outside the 2 MB container.
- The calibration dialog hid a second dialog: "TOO OLD HARDWARE FOR THIS OS". The boot task reads a PCB revision byte at address 0
  (FlexBus CS1: CSAR1 = 0, CSMR1 0x10001, set up by `FUN_400824ea`) and the self-test prints it as `PCBA0046%c%d` (letter = bits 0-4,
  number - 1 = bits 5-7). Letter below D is "too old"; 0x23, 0x43 and 0x82 are "will not be compatible". `FUN_4008248c` also reads it
  (letter >= E: newer board). `[emulator] board_revision = 0x04` serves that byte as a constant (`install_forced_mmio`); 0x04 is the
  smallest value that passes both checks, an emulator choice, not read from a unit. What else CS1 holds is **[O]**; no logic behind it
  is modelled.
- Result: cold boot reaches the main screen ("A01 ADRIANA") with no dialog at 630 M instructions (about 5 s host).

## Panel sweep (OS 1.42) [D]

- Method: `tools/emulator/panel_sweep.py` (frames of one tap per code, LED records) on the cold-booted image, after NO closes the
  calibration dialog; encoders by `turn:N` with `panel_drive` (the turn shows after about 100 M instructions). Controls were named by the
  screen they open or the value they show; the faceplate legends of the photo give the names.
- The decoder is `FUN_40103094` (key decoder anchor of `devices/fw/input.toml`, hash unique on 1.42). It reads a table of 32 bytes per
  channel at 0x401d7a48. Unlike DT2 and DN2 the table is not the identity, so the code of `devices/syntakt.toml` is the wire
  position (`channel * 8 + bit + 1`, 1-48; 49 = channel 6 bit 0). Channel 6 has one key; `button_channels = 7`.
- Firmware has no control-name table (no "ENCODER A" strings); only the UI test screens name a few controls.

| Code | Control | Code | Control | Code | Control |
| --- | --- | --- | --- | --- | --- |
| 1-16 | trig keys 1-16 | 25 | settings (gear) | 36 | RECORD |
| 17 | FUNC | 26 | FX | 37 | PLAY |
| 18 | TRK | 27 | TEMPO | 38 | STOP |
| 19 | TRIG page | 28 | NO | 39 | YES |
| 20 | SYN page | 29 | LEFT | 40 | UP |
| 21 | wired to nothing | 30 | DOWN | 41-48 | push G, F, E, B, A, D, C, H |
| 22 | FLTR page | 31 | RIGHT | 49 | LEVEL push |
| 23 | AMP page | 32 | PAGE | | |
| 24 | LFO page | 33 | BANK, 34 PTN | | |

- Code 35 is the key with the three dots: it opens the SONG MODE popup. Whether the printed name is Sound or Song is **[O]**.
- Checked by effect: 19-20, 22-28, 30, 33-35, 39 and 40 (menu cursor), 41-48 (the value of the encoder's parameter), encoders 1-9 =
  A-H, LEVEL. By order in the firmware's key enumeration only **[O]**: 17 FUNC and 18 TRK (FUNC lights LED 0-15, TRK lights LED 0-11 and 13-15),
  29 LEFT and 31 RIGHT (a held press moves the tempo down and up), 36 RECORD, 37 PLAY, 38 STOP, 32 PAGE (it lights the pattern-page LEDs).
- LED protocol (the Syntakt has 44 RGB LEDs, not the `b9 idx val` records of DT2 and DN2):
  - `b5 n r g b` sets palette entry n (41 entries, 5 bits per channel); `b0..b3 led n` sets the palette entry of a LED's state 0-3.
  - `0x20 + k, v` sets LEDs 4k..4k+3: two bits each, the state of the LED (3 is the idle state).
  - LED 0-15 are the trig keys 1-16 (the selected track's key is lit, FUNC lights all 16); LED 40-43 are the pattern-page LEDs
    1:4-4:4 (order **[O]**). The first sweep found no LED for PLAY, RECORD and the page keys because it read only the `b9` records of
    the other devices; the decoder below corrects that.
  - Decoder: `[emulator] led_protocol = "packed-states"` selects it in `panel_leds.rs` (the other devices keep `b9 idx val`). It keeps the
    palette (`b5`), the palette entry of each LED state (`b0..b3`) and each LED's state (`0x20 + k`), and reports per LED the colour of
    its current state in `rgb` (5 bits per channel) and, for the web faceplate, the value `leds` (0x01 off, colour mask in bits 4-7, 1
    bright or 0x0a dim: a channel at 0x08 or more counts as on). The state is in `panel_drive --save-state` snapshots (section `LEDS`).
    Records seen on 1.42: `b5` 41 entries, `b3` 44 (idle state), `b2` 4 (state 2), `0x20..0x2a` 11 groups; `b0` and `b1` not seen.
  - Second pass with the decoder (`panel_drive --state out/ready/st-1.42/main.state`, one key press each, LED values before and during
    the press) [D]: trig keys 1-12 select a track and move the dim red LED (idle trig LEDs of a track are palette 4, half red); PLAY lights
    LED 37 green and the playhead (LED 0 bright red, pattern LED 43); RECORD lights LED 36 red; the page keys are a radio group on
    LEDs 18-22 (TRIG, SYN, FLTR, AMP, LFO; SYN is lit on the main screen); FUNC lights all 16 trig LEDs white; PAGE sets the four pattern
    LEDs 40-43. Many keys change several LEDs at once (a view or a mode): the map names only the ones above.
    `devices/syntakt-leds.toml` now has the page keys, RECORD and PLAY besides the 16 trig LEDs and the four indicators.

## Analysis databases

- Ghidra project: `out/ghidra-projects/st-1.42-emac` (language
  `68000:BE:32:ColdfireEMAC`, separate from the 1.16 project).
- Dump: `out/ghidra/st-1.42-emac`. cfdb: `out/cfdb/st-1.42-emac.sqlite` with
  13,073 functions, 1,007 typeinfos and 12 decompile failures.
- Seeds and RTTI come from `codeseeds` and `rttiscan` with
  `--code 0x40000400 0x401d0000 --code 0x40336000 0x40338000`. Without these
  ranges there are 9,498 functions.
- Pipeline: `tools/coldfire/ghidra.sh` import + `McfLabels.java`, `codeseeds`,
  `rttiscan`, `ghidraapply seeds --analyze`, `ghidraapply rtti`,
  `ghidradump`, `cfdb build`, `cfdb fts`.
- `ghidradump` and `ghidraapply` need pyghidra, which the project venv does
  not include. The run used
  `uv run --offline --with pyghidra==3.1.0 --with jpype1==1.5.2`.
- Peer image `st-1.42-peer` (section 7): Ghidra project
  `out/ghidra-projects/st-1.42-peer-emac`, dump `out/ghidra/st-1.42-peer-emac`,
  cfdb `out/cfdb/st-1.42-peer-emac.sqlite`. Build it with
  `uv run python -m tools.coldfire.imagepipe st-1.42-peer out/sections/st-1.42/section_7_BLOB.bin --base 0x40000400 --code 0x40000400 0x4005df10`.
  The tool runs the pipeline above for any raw ColdFire section. It skips a
  step whose output exists, runs `apply` only after an import in the same call,
  and `--dry-run` prints the commands. `--steps` runs named steps again.
- `atlas build st-1.42` works. The firmware has no DT2/DN2 parameter table
  reader, so the `params` table is empty and the atlas meta row `params` says
  `absent`. The descriptor chain is found (82 rows, SYN machines).
- There is no Syntakt device file in `devices/` yet. `port_resolve`,
  `tools/coldfire/regions.py` `IMAGES` and `cf.load` need a registered name.

## Open questions

- Who writes the peer's NOR (finding 136).
- The mailbox reply semantics.
- Which bootstrap code streams section 1.
- Whether 0xFC05C000 is a DSPI.
- How many ColdFire anchors can be re-derived from cfmatch strict and vtable
  pairs.

## Scope

- Support is limited to the ColdFire images.
- Sections 1, 6 and 7 and the bootstrap are never written.
- The FPGA is never emulated or reconfigured (risk to the analog circuitry).
  The emulator may stub the FPGA status and the peer interface only.
- Plan: [syntakt](../plans/syntakt.md).
