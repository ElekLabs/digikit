# Syntakt analog voices: the DSPI1 control frame

MAIN drives the four analog voices itself. It builds a 1000-byte control frame in SRAM each audio block and sends it over DSPI `0xFC03C000`
by eDMA. The peer CPU has no part in it. Image `st-1.42` (OS 1.42). Marks: **[V]** checked against image bytes by a second agent, **[D]** read once in the decompiled MAIN and the cfdb datarefs,
**[O]** open. No emulator or device was used. Context: [141](141-syntakt-analog-and-nor-paths.md), [136](136-syntakt-peer-cpu.md),
[70](70-syntakt-1-42.md). Manual: Syntakt OS 1.42, section 6.3.1 and appendix A.3, A.4.

## Result

- **[D]** Tracks 1-8 are digital (the peer renders them, [136](136-syntakt-peer-cpu.md)). Tracks 9-11 are Analog Drum, track 12 is Analog Cymbal.
  The analog engine of MAIN runs on tracks index 8..11 (table `0x402a2138` = `08 09 0a 0b 00 01 02 03`; the engine tick uses the first four). The table and the tick dispatch are **[V]**.
- **[D]** Chain: UI parameter set -> parameter block `0x800021b0` -> per-track analog handler -> control frame `0x80003160` -> eDMA ch 15 -> DSPI1
  PUSHR -> board. All in MAIN, in the audio tick `vector_191_handler` (`0x400a38ce`).
- **[V]** The peer is not on this path. In the peer image only `FUN_40000706` touches DSPI1 (`0xFC03C000-0xFC03C034`). Peer GPIO writes are pin setup
  (`FUN_40000548`, `FUN_40000db4`) and the G2 doorbell toggle (`FUN_4000090e`). The peer frame ISR `0x40000934` touches neither DSPI1 nor GPIO.
- **[V]** MAIN sends the peer a frame of `0x1b0` bytes (`0x80002000-0x800021af`). The parameter block at `0x800021b0` is not sent to the peer.
- **[O]** What sits behind DSPI1 PCS0 (an FPGA, DAC and pot chips, or a mix) is not visible in the code. The frame is a list of DAC words, switch bits and
  8-byte serial commands; the board decodes it.

## Writer 1: parameter block at 0x800021b0

`FUN_400a30ca(value, track, index)` stores one 16-bit parameter: `*(u16 *)(0x800021b0 + (index + 0xe + track * 0x47) * 2) = value`. **[V]**
Callers: `SoundParameterSet::vfunc_30` and `vfunc_13`, `FxParameterSet::vfunc_30` and `vfunc_13`, `FUN_40019458` and five more. So the sound and
FX parameter sets of the sequencer and UI are the main writer, for all tracks. **[D]**

- Per track: 0x8e bytes (0x47 halfwords) from byte `0x1c`; track `n` is at `0x800021b0 + 0x1c + n * 0x8e`. Tracks 0-11. **[D]**
- Track 12 (the FX track, Analog FX block and sends) is at `0x800021b0 + 0x6c4` (halfword `0x362`). `FUN_400a30ca` takes `track == 0xc` as this case. **[D]**
- Header: 13 halfwords at `0x800021b0` (copied by `FUN_400a32ae` from the pattern or kit record, `0x1a` bytes). **[D]**
- Per track flags at `0x800021b0 + 0xea0 + 4 * track` (trig requests, set by `FUN_400a5780`, cleared by `FUN_400a354e`) and the sound pointer at `+ 0xf10 + 4 * track`
  (`FUN_400a321a`, copies the 0x8e-byte record from the sound). **[D]**
- Each tick `FUN_400979ca(0x800021b0)` makes a working copy: it returns `0x80006348` (the state base, `A2` in the tick), builds a 4 KB ramp table and copies
  per-track words. The analog handlers read this copy. **[D]**

## Writer 2: the analog handlers

The tick (`0x400a4e8c`-`0x400a4f2c`) loops over the four analog tracks. For each it takes the machine id from the byte table `0x80002f50[track]`. Valid ids are
0x2c (the tick accepts ids up to 0x2c; the table entry for 0x2c is 0). It calls the handler from the table at `0x402a1e84 + 4 * id` with `(voice, state + 0x3e + 0x8e * track, 0x80003160, tick value)`. **[V]**

- The table has 32 slots (ids 8..0x14, 0x16..0x23, 0x27..0x2b) and 31 distinct handlers (ids 0x22 and 0x23 share `0x4009b5bc`). The handler table bytes are **[V]**: `0x40098a72`, `0x40097d02`, `0x40098386`, `0x40099734`, `0x4009911e`,
  `0x40099f5a`, `0x4009dd82`, `0x4009d256`, `0x4009d81a`, `0x4009e302`, `0x4009cd16`, `0x4009c82a`, `0x4009f1be`, `0x4009bc1c`, `0x4009c2d8`, `0x4009aeb4`, `0x4009a888`,
  `0x4009ba10`, `0x4009c0e8`, `0x4009ac18`, `0x4009a5ec`, `0x4009b1da`, `0x4009bea0`, `0x4009b73c`, `0x4009b61c`, `0x4009b5bc` (ids 0x22, 0x23), `0x4009f9fe`, `0x400a010e`,
  `0x400a0cb2`, `0x4009e852`, `0x4009c4b8`. Mapping of ids to the machine names of the manual (BD HARD, BD CLASSIC, BD SILKY, BD SHARP, BD ACOUSTIC, SD CLASSIC, SD HARD,
  SD ACOUSTIC, SY DUAL VCO; CH CLASSIC, OH CLASSIC, HH LAB, CY CLASSIC, CY METALLIC, MIDI) is **[O]**. 31 handlers for about 16 machines: some ids may be variants.
- Example `FUN_40097d02` (voices 0-2 share one body, the voice picks the frame offsets): it reads waveform, decay, tune and level words of the track record
  (`record + 2, 4, 6, 8, 0xa, 0xc, 0xe`), sets and clears switch bits in the frame bit array (bit numbers from the tables `0x401e5fb8`, `0x401e5fe0`, `0x401e6008`),
  writes two DAC halfwords at `frame + 0x24 / 0x2c / 0x34` (voices 0, 1, 2), and fills the 32-byte serial command slot at `frame + 0x118 / 0x138 / 0x158`
  (0x7f / 0x80 / 0xf0 bytes, a 20-bit field in the low bits of a word, nibble `0x10` / `0x90` command tags). **[D]**
- Per-voice state at `0x80005160 + 0x20 * voice` (`FUN_400a286c` sets `+0` to `-1`, idle): `+0` phase counter, `+4` a tick or sample count that counts down by 16 per
  tick. Referenced by 24 of the handlers. **[D]**
- The Analog FX block (drive, multimode filter, amplifier) is `FUN_40090442(frame, state, ...)`, called once per tick at `0x400a4fc4` after the handlers. It reads
  the FX track record (`state + 0x70a .. 0x74e`: `+0x70a`, `+0x70e`, `+0x71a`, `+0x71c` mode 5, `+0x736 .. 0x740`, `+0x746`, `+0x748`, `+0x74a`, `+0x74c`) and writes
  DAC halfwords `frame + 0x08, 0x12, 0x46, 0x48` (clamped to 0xffff, through the exp table `FUN_400a1792` and Q31 `emacSaturate` products) and two 8-byte commands at `frame + 0x1a0` and
  `+ 0x1a8` (a 3-byte data word and a command word `0xb0xxxxxx`, `0x30xxxxxx`, `0xb01xxxxx`; a 7-state machine in the word `0x403d0ec4` shapes it). **[D]**

## Frame layout (base 0x80003160, 1000 bytes)

Built by `FUN_40090bc0` (a full reset, called from the `vector_191_handler` copy at `0x40091342` and from `FUN_400918e6`) and `FUN_400a29ce` (the same at boot, from
`FUN_400a2e90`). Both zero the 1000 bytes (`0x3e8`) and write the idle values (`0x4000` at `0x80003174`). The reset in `FUN_40090bc0` is **[V]**.

| Offset | Content | Evidence |
| --- | --- | --- |
| `+0x00 .. +0x13` | header halfwords, zero at reset; `+0x08`, `+0x12` FX filter pair | reset, `FUN_40090442` |
| `+0x14 .. +0x1b` | four halfwords, idle `0x4000` | `0x80003174..7a` reset |
| `+0x24, +0x2c, +0x34` | DAC halfword pairs of the drum voices 0, 1, 2 | `FUN_40097d02` |
| `+0x46, +0x48` | FX block DAC pair (drive and filter) | `FUN_40090442` |
| `+0xa0 .. +0xa5` | switch bit array; bit numbers from the tables above (bits `0x1e .. 0x31` are set at reset); `+0xa4` low 3 bits set by `FUN_400927f8` | `FUN_40090bc0`, `FUN_40090b6e` |
| `+0xa6` | flags: `0x40` set before send, `0x10` and `0x20` FX state | tick `0x400a5384` |
| `+0xa8 .. +0x1b7` | records of 8 bytes: 2 halfword data, byte `+4` (low nibble `0xf0` idle tag) and bytes `+2, +3` = `0x80` or `0x7f` (mid-scale pot values); the tick clears byte `+4` of records 21..76 after the send | reset, tick `0x400a539a` |
| `+0x118, +0x138, +0x158` | 32-byte command slots of drum voices 0, 1, 2 | `FUN_40097d02` |
| `+0x1a0 .. +0x1af` | FX block commands (two of 8 bytes) | `FUN_40090442` |
| `+0x298 .. +0x33d` | 40 halfwords at a 4-byte stride, idle `0x4000` (voice DAC channels; calibration tables adjust them) | reset |
| `+0x3d8 .. +0x3e3` | six halfwords from the calibration record (`record + 0x150 .. 0x15a`), if `record + 0x14f != 0` | `FUN_4009ef18` |
| `+0x3d9 .. +0x3e7` | constants `0x92`, `0xa3`, `0x80` at odd bytes | reset |

The exact meaning of each field (which pot or DAC it is) is **[O]**: it needs the board schematic or the bridge FPGA, which are not in the image.

## Transfer: eDMA ch 15 to DSPI1 PUSHR

- `FUN_401021b4(src)` (called at the end of the tick, `0x400a538a`, and from the other three tick copies): toggles the buffer index `0x443db260` (`^= 1`), then fills the buffer at
  `0x800051ec + 0x830 * index` with one PUSHR word `0x8000aaaa` (word 0: CONT, 16-bit data `0xaaaa`, no chip select), 500 words `0x80010000 | halfword` (CONT, PCS0,
  the 500 halfwords of the frame, in order), and an end word `0x08005555` (EOQ, `0x5555`). The `or.l #0x4000000,(0x10,A1)` sets CTCNT on word 1, the first PCS0 data word, not on the `0xaaaa` word.
  That is 502 words. The marker words `0xaaaa` and `0x5555` frame the data. **[V]**
- `FUN_4010223e` (before the buffer swap, so the previous buffer goes out): waits for the DSPI1 end-of-queue flag, SR bit 28 (`FUN_4010222a`), starts eDMA channel 15 (`DAT_fc044019 = 0xf`), sets MCR bit 11,
  clears SR bit 28 (write one), sets TCD SADDR `0x800051ec + 0x830 * index`. The TCD (`FUN_401020f4`): 32-bit transfers, SOFF 4, NBYTES 4, DADDR PUSHR `0xFC03C034`, CITER and BITER `0x206`
  (518 words, 16 more than the 502 built; **[O]**: the extra words come from after the end word, EOQ stops the queue first), no interrupt. The chain `FUN_401021b4` / `FUN_4010223e` to eDMA ch 15, TCD `0xFC0451E0`, DADDR `0xFC03C034` is **[V]**.
- The frame goes once per audio block (1500 Hz in the emulator model, [136](136-syntakt-peer-cpu.md)). A block is 32 samples. **[D]**
- Separate small writes with 8-bit frames on **PCS1** (`FUN_400a37d2` at counter `0x43`, `FUN_400a3870` at counter 0, counter word `0x402a1e7c` reload 0x46 in `FUN_400a2e90`):
  the byte sequence `01 f8 76 ff 04 00` then, three blocks later, `00` (PUSHR `| 0x38020000`: CTAS 3, EOQ, PCS1). The reads come back from POPR. So PCS1 is a second device,
  addressed about every 70 blocks (about 21 Hz); it looks like a periodic converter or status access. Its role is **[O]**; the index argument of `FUN_400a1d34` is not sent.

## Calibration record (RBOZ)

- RAM copy at `0x4060d970`: magic `0x52424f5a` ("RBOZ"), `+4` length `0x22d30`, `+8` version 1, `+0xc` checksum; total `0x22d40`. NOR copy at `0x300000` (backup at `0x340000`). **[D]**
- Getter `FUN_4006ecb8` (locks mutex `0x406533f0`, loads from NOR, checks magic and checksum, else builds an empty record; returns `0x4060d970`). `FUN_4006ed76` releases the lock
  and optionally writes back (`FUN_4006eb7e`). **[D]**
- Readers that apply it to the analog control values: `FUN_4009ef18(frame)` (from `FUN_400a286c`, boot): frame `+0x3d8..` words, a per-voice correction table at `0x4550f268`
  (5 x 0x5ac bytes, with fallback constants when `record + 0x10 == 0`, "not calibrated"); `FUN_40095c7e(0x800033f8)` (from `FUN_400a29ce`): filter and VCO tables at `0x43bdd048`
  and `0x43bdd000`; `FUN_4008ee30` (from the init `FUN_400a2e90`): 8 + 8 gain words at `0x454e4c18` from `record + 0x9a / 0xba` (default `0x7fffffff`). Other callers
  are the calibration menu, the self-test `FUN_40080646` and the analog range functions `FUN_4009176c`, `FUN_400918e6` and `FUN_40091aec`. **[D]**

## GPIO (0xEC094000, PPDSDR at 0x18 + port, PCLRR at 0x24 + port, ports A..K)

Pin helpers read a board kind (`DAT_402d731c` or `DAT_402d7320`, must be 2) before they write. **[D]**

| Pin | Helper | Used by |
| --- | --- | --- |
| port G bit 2 (`0xEC09401E` / `0xEC09402A`) | `FUN_40114038` | peer-link doorbell pin (the peer toggles it too, `FUN_4000090e`; [136](136-syntakt-peer-cpu.md)); callers `FUN_4008af20` / `FUN_4008af38` (with port I bit 1), tick timer `0x80003138` |
| port G bit 0 | `FUN_401140a2` | pulse with countdown `0x80003134` in `FUN_400956b8` (every tick) |
| port H bit 7 (`0xEC09401F`) | `FUN_4011407e` | pulse with countdown `0x80003130` in `FUN_400956b8` |
| port I bit 1 (`0xEC094020`) | `FUN_4011405c` | together with port G bit 2 |
| port E bit 0 / port F bit 7 | none | FPGA INIT and DONE in `FUN_400bcb4e` |
| port G bit 4 (NOR chip select, [141](141-syntakt-analog-and-nor-paths.md)) | `FUN_40114288` | NOR driver |

Port G bit 0, port H bit 7 and port I bit 1 are driven from the transport events `FUN_4008b080` (start) and `FUN_4008b224` (stop, called by `TransportView::vfunc_2`).
The roles of these pins are **[O]**: the label "transport pulses" comes from their callers only. A clock or sync output is a guess. No analog reset, enable or mute pin was found. **[O]**

## Other windows

No other peripheral carries analog control. The analog code (`0x40090000-0x400a2f00`) references only DSPI1, eDMA (`0xFC044xxx`, `0xFC045xxx`), INTC (`0xFC04C0xx`) and DMA timer 2
(`0xFC07800C`, the audio frame timer). It touches no I2C, PWM, DAC or other FlexBus window. The FlexBus window `0x10000000` carries the digital voice frame and the peer link only
([141](141-syntakt-analog-and-nor-paths.md)). So no new `[protect]` window follows from this finding: DSPI1 `0xFC03C000` and the GPIO block are already in the table. **[D]** Basis: checked by absolute-address datarefs and direct call edges over the transitive callee closure of the tick and all 31 handlers
(261 functions). Base-register accesses and indirect calls are not excluded.

## Excluded and open

- Excluded: the peer as analog driver; FlexBus CS0 as analog path; I2C, PWM, DAC and other DSPI blocks.
- **[O]** Field meaning of the frame (pot, DAC or relay per bit), machine id to machine name, the board kind values of `DAT_402d731c / 7320`, the PCS1 device, the TCD count 518.
