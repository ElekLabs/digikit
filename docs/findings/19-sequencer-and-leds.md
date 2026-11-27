# 19. The sequencer step clock and the front-panel LED stream

Marks as in CLAUDE.md. DT2 OS 1.16 and DN2 OS 1.11, measured 2026-10-13 in lane fastweb. Nothing here was checked
by a second agent yet: **[D]** unless marked.

## Why the sequencer stopped after one step **[D]**

Symptom: `dt2-rec-play` / `dn2-rec-play` (RECORD, four trigs, RECORD, PLAY) gave one hit and a dead LED stream; PLAY
alone behaves the same, and a longer PLAY hold gives the identical PCM hash.

The step chain, DT2 1.16 (DN2 1.11 runs the same code):

1. `TransportView::vfunc_2` (`0x400786ae`) handles the transport keys (firmware codes 0x13 RECORD, 0x14 PLAY,
   0x15 STOP). PLAY without RECORD held ends in `FUN_400d60e6` (the "start playing" routine, 1,240 bytes): it
   resets the per-track step state and sets the step countdown `_DAT_80004888` (ticks to the next step) and the
   transport clock `_DAT_80004880` / `_DAT_8000487c`. `_DAT_43164f94 = 1` is the transport state (1 after PLAY).
2. The audio-block ISR, `vector_191_handler` (`0x4002dd0c`, 1,500 per second), ends each block with
   `_DAT_80004880 += 64; _DAT_80004888 -= 64` (`DAT_402b62bc * 2`). When the countdown reaches 0 it sets
   `_DAT_fc048010 |= 0x1000`: bit 12 of INTC0 INTFRCH, a **software-forced interrupt, source 44, vector 108**
   (`0x4002f2d2`; `FUN_4011e9f0` does the same).
3. Vector 108 is `0x400d5a50` (a naked ISR Ghidra has no function for; installed by `FUN_400d766a` with ICR 0x6c
   = level 5). It clears bit 12, advances the step/pattern state, and at `0x400d5de4` forces
   `INTFRCH |= 0x02000000` (source 57, vector 121).
4. Vector 121 is `FUN_400d7a2e` (4,704 bytes, level 2, ICR 0x79): the per-step work. It clears bit 25 and reloads
   `_DAT_80004888` for the next step (`_DAT_43158f84` = 900000 per bar unit), and queues trig events through
   `FUN_4013a52a` / `FUN_4013a6b0` for `FUN_40139b78`, which drains them into the DSP-bound queue each block.

Root cause **[D]**: the emulator never delivered INTC software-forced interrupts. `packages/emulator/periph/src/intc.rs`
had `hw_intfrc_bit` and no caller; only INTC1 source 63 (vector 191) had a model (the SSI model, `on_intfrch1`).
Evidence from a run to 6 s with `--peek`: INTFRCH0 (`0xfc048010`) reads `0x00001000` at 0.5 s (before PLAY), 0.8 s
and 6 s, vector 108 holds `0x400d5a50`, `_DAT_43158f7c` (step time base) stays `0x12da1300` and the transport state
is 1. So the transport keeps "playing" and the step clock never fires. The one hit at PLAY comes from the start
routine itself. (This also explains finding 04's "PLAY reaches the scheduler once, the pool looks empty".)

Fix **[D]**: `Time::service_with` (packages/emulator/machine/src/time.rs) offers the highest-level forced, unmasked INTC source
above the live IPL (`IntcBank::forced_candidate`, skipping vector 191), after PIT and DTIM, with the existing
`blocked` / `idle_window` caches covering it. In this initial experiment it was opt-in (`DIGI_INTC_FORCE=1`) because it
changed sequencer PCM: `exactness-100m`, `trig1-noqa-40m`, `trig1-taps` and `trig1-tap-release` changed
(`idle`, `trig1-hold`, `kb-chord16` had no pin). With it off all pins and the 61 oracles passed. See the dated follow-up
below for the default change.

Result with `DIGI_INTC_FORCE=1` **[D]**: INTFRCH0 reads 0; the step playhead runs on the trig LEDs (0x11 on the
current step, 0x71 on programmed trigs, 16.5M instructions per step on average, 17.6/13.2 alternating = swing);
`dn2-rec-play` plays a hit every 0.5 s (window RMS period of 5 windows of 100 ms); `dt2-rec-play --dt2-card` plays
from 0.6 s on; PLAY alone steps too.

### The same chain on DN2 1.11, and a unit **[D]**

Found apart from the above (dn2_firmware, 2026-10-11). On DN2 1.11 the stages are at these addresses:

| Stage | Where | What it does |
| ----- | ----- | ------------ |
| Audio ISR | `0x40025e36`, internal-clock path `0x40027aba` | Adds `2 * [0x402a0dec]` to `0x80005398` and subtracts it from the countdown `0x800053a0`. At or below zero it stores `0x7fffffff` there and sets INTFRCH bit 12 (`0x40027afc`). |
| Source 44 | vector 108, ICR level 5, handler `0x400d7864` | Clears bit 12, re-arms, and forces source 57 (`orl #1<<25, 0xFC048010` at `0x400d7c02`). |
| Source 57 | vector 121, ICR level 2, handler `0x400d987a` | Clears bit 25, adds 900000 to `0x42c4e900` and runs the per-step work. The step count `0x4463ed14` goes up at `0x400da54c`. |

The routine that ends at `0x400d95d0` installs both handlers: `0x400d987a` at `0x400001e4` with level 2 at
`0xFC048079`, and `0x400d7864` at `0x400001b0` with level 5 at `0xFC04806C`. The handler at `0x400d987a` has no
direct caller, so a call-graph search for it finds nothing. The clock-mode word is `0x402a0df0`: `0x4013706c`
returns the index of its highest set bit, and `0x4013707c` sets the mode and the period. Mode 1 and the external
clock (`0x40138034` returns 3) branch away at `0x40027b04` and `0x40027a80`.

With forced delivery on, DN2 1.11 from reset with PLAY steps `0x446483d0` from 0 to 15, and `0x4463ed14` rises 18
per 300 M instructions: 7.9 steps a second at 132 MHz. A Digitone II on 1.11, on PLAY with its internal clock, read
about 8 steps a second on the same two words through a USB memory probe.

## The LED stream **[D]**

`FUN_40133d64(logical, value)` builds the record `b9 idx val` (3 bytes through `FUN_4000220c`, one UART8 queue):

- `idx = *(0x4020a9a8 + 4 * logical)`, which is `logical - 1` for logical 1..49 (a second table follows at
  `0x4020aa70` with 1..49). The logical ids are the firmware's LED-name enumeration at `0x4020a8e0` (DT2):
  1 TRACK, 2 KEYBOARD, 3 FUNCTION, 4 PATTERN, 5 SONG, 6 PRESET MENU, 7 GLOBAL, 8 SAMPLE (DN2: VOICE), 9 TEMPO,
  10 RECORD, 11 PLAY, 12 STOP, 13 YES, 14 NO, 15 UP, 16 DOWN, 17 LEFT, 18 RIGHT, 19 PAGE, 20-25 PAGE TRIG..MOD,
  26-41 GRID01..16, 42-49 PAGE1..8; DN2 adds 50 ARP, 51 PLUS, 52 STACK, 53 MINUS (names at `0x401f352c`).
  `FUN_40133c16` accepts logical < 0x33.
- State arrays: shadow value `0x47e283cc[idx]`, dirty bitmap `0x44e4ae80`, **pulse value** `0x47e283fe[idx]` with
  pulse flag `0x47e28430[idx]` and countdown `0x44e4adb8[idx]` (not a blink phase): `FUN_40133b7c(logical, ticks,
  val)` shows `val` for `ticks` ticks, then the base value returns (`FUN_40133afa`). `FUN_40133a24` flushes a
  dirty row; a record is only sent when the value changes.
- Setter: `FUN_400308e0(LedManager, logical, layer, value)` (the `LedManager` singleton, 0x280 bytes). `layer`
  is stored at `+0x28 + 4 * logical` (a nonzero layer blocks the next set), `value` at `+0xf0`; layer 1 sends 1.
  Constants passed at call sites: 0x71 (most), 0x81, 0x11, 0x41, 0x21, 0x6a, 0x5a, 0x1a, 1.
- Value byte, from those constants and the wire values (0x01 0x02 0x06 0x0a 0x11 0x1a 0x21 0x41 0x5a 0x66 0x6a 0x71
  0x86): **inference [O]**, there is no panel-MCU firmware to read. Bits 4-7 are a colour mask (0x10 red, 0x20
  green, 0x40 blue, 0x80 a fourth channel; 0x71 = all of 4-6 = white), bits 0-3 a mode: 1 = steady, 0x0a / 6 / 2 =
  dimmed or pulsing variants. 0x01 is "off". Colours on the device: RECORD red (0x11) and PLAY green (0x21) match the
  bit positions; the rest of the names are not confirmed.

Sweep (`dt2-led-sweep`, `dn2-led-sweep`: every code pressed 6M instructions, 12M apart, after NO; capture with
`DIGI_PANEL_TX_LOG`, read with `tools/emulator/led_map.py`; 4M spacing was too tight, the UI queue lags by up to about
one key): the key-to-LED map is `devices/digitakt-ii-leds.toml` and `devices/digitone-ii-leds.toml`.

- Trig key code c (25-40) has LED idx c. Pressed: 0x11. Page keys 1-6 are a radio group on LEDs 0x13-0x18 (0x71).
  RECORD 0x09 = 0x11, PLAY 0x0a = 0x21 (STOP 0x0b goes 0x71 -> 0x01 while PLAY is on), TRK 0x00, KEYBOARD 0x01,
  PTN 0x03, SONG 0x04, PRESET 0x05, SETTINGS 0x06, SAMPLING 0x07 (DN2 VOICE), TEMPO 0x08. TRK held lights the
  trig keys 0x6a, FUNC held 0x1a. PAGE1..8 LEDs are 0x29..0x30 (0x0a current, 0x02 others; RECORD makes PAGE1
  0x21).
- Keys with no LED change in a plain press: YES, NO, UP, DOWN, LEFT, RIGHT (a view lights them, e.g. LEFT and
  RIGHT 0x71 under PATTERN), FUNC, PAGE; encoder pushes 41-48 and LEVEL 49 have no LED in the table. DN2: ARP
  0x31, PLUS 0x32, STACK 0x33, MINUS 0x34; STACK turns PLUS and MINUS to 0x66 and the stacked steps 0x86.
- The firmware writes every record twice back to back; `tools/emulator/led_map.py` drops records that do not change the
  shadow value.
- `DIGI_PANEL_TX_LOG` also logs `icount @button CODE d|u` at each injected button, so a press can be placed in the
  LED stream.

## Open **[O]**

- The meaning of the low nibble (modes 2, 6, 0xa) and bit 7 needs the panel MCU's side or a device recording.
- Whether vector 108 at level 5 and vector 121 at level 2 interleave exactly as on the device (the model offers a
  forced source once per boundary above the IPL; PIT/DTIM keep their existing order).

Forced-interrupt delivery is on by default since 2026-10-13 (`DIGI_INTC_FORCE=0` turns it off natively; always on in wasm), with the four pins re-pinned, Em-approved.
