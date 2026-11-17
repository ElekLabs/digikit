# LFO waveform parameter and its readers

DT2 OS 1.17 and DN2 OS 1.12. Static reading of MAIN OS bytes (file offset = address - 0x40000400); no emulator run. Used by the plan for run-time LFO waveforms. Marks: **[V]** a second agent checked it against image bytes, **[D]** read once, **[O]** open.

## The wave parameter

- The wave is a parameter with codes 79, 89 and 99 (one per LFO). The value is `wave << 8`. **[D]**
- T1 rows (stride 0x3c, base DT2 0x4020f59c, DN2 0x401f83a4): code 79 DT2 0x40210820, code 89 0x40210a78, code 99 0x40210cd0. Row of code 79: owner 0x1c, mirror index 5, min 0, max 0x600 (at +0xc), default 0x100. Code 89 and 99 have default 0. **[D]**
- DN2 rows: 0x401f9628, 0x401f9880, 0x401f9ad8 (max 0x600; owners 0x1a, 0x1b, 0x1c). DN2 also has T1 code 266 (max 0x600, owner 2; role **[O]**). **[D]**
- The wave words in the sound array are index 5, 13 and 21. **[D]**

## Waveform names

- DT2 formatter 0x400e1b68: `moveq 6; asr 8; cmp; bcs` then a fallback. A wave above 6 prints "ERR" (string 0x402438ca). Table 0x40213ffc, seven pointers: TRI, SINE, SQR, SAW, EXPO, RAMP, RAND. **[V]**
- DN2 has two formatters. 0x400e3610 (table 0x401fd9d0) gives four-character names. 0x4000766c (table 0x401d3984) gives three-character names: TRI SIN SQR SAW EXP RMP RND. Both have the same shape. **[V]** A plan must hook both on DN2.

## The tick

- DT2 FUN_4013961e (806 bytes). Its only caller is the vector 191 handler (0x4002dd0c), call at 0x4002e91c. DN2 FUN_40137a06 (1028 bytes), handler 0x40025e36, call at 0x400272d4. **[D]**
- Clamp: DT2 0x40139698..0x401396a8, DN2 0x40137b56..0x40137b66. Code: `mvs.b (offset,A),D3; if D3 < 0 then D3 = 0; if D3 > 6 then D3 = 6`. **[D]**
- The clamped byte is the high byte of the live wave word, so it is `value >> 8` as a signed byte. **[D]** Trace: the sound loader (DT2 FUN_400db3b2) writes sound word i to live word `idx[i] + 10` of the sound struct (`idx` = table 0x40213b64, entries of u32: LFO k of parameter n is live n, n+8, n+16), so the live words of LFO1, 2 and 3 are at struct+0x14 + 2*L with L = 1..8, 9..16, 17..24 in the order SPD, MULT, FADE, DEST, WAVE, SPH, MODE, DEPTH. The loader remaps DEST (live 4, 12, 20) and leaves WAVE (live 5, 13, 21, struct+0x1e, +0x2e, +0x3e) raw. The tick gets the struct as argument, steps A5 to the live array (`lea 0x22,A5`; the 0x8e clear in the loader is the array size and the per-track stride of the tick), and for the three LFOs of a track uses A3 = array + 0x28 - 0x10*k; `(A3)` is the DEST byte (limit 0x46) and `(2,A3)` is the WAVE byte (array + 0x2a for LFO3). DN2 has the same shape (`(0x4c,A4)`, DEST `(0x4a,A4)`, second reader `(0x2a,A4)`).
- So on stock code a wave of 7 to 127 (words 0x0700..0x7fxx) becomes 6 and runs as RAND, and a wave byte of 0x80 or more (words 0x8000 and up) becomes 0 and runs as TRI. The low byte of the word is ignored. **[D]** (static; the formatter and graph clamps show the same split by their comparisons, not traced for 0x80 and up)
- The clamped D3 also indexes three 7-entry tables with zero initial values (DT2 0x40222780, 0x4022279c, 0x402227b8; DN2 0x4020b7a4, 0x4020b7c0, 0x4020b7dc), so the clamp must keep D3 in 0..6 for any wave. A hook that leaves the clamp alone and tests the raw byte again where the code compares D3 with 6 keeps this. **[D]**
- After the clamp, wave 6 branches to the random code: `moveq 6; cmp D3,D7; beq` at 0x40139804 (DT2) and 0x40137cc6 (DN2). Otherwise the code loads the function table and calls it (DT2 `lea` 0x40139818, `jsr (A1)` 0x40139828; DN2 0x40137cda and 0x40137ce6). **[V]**
- A second compare at 0x4013986c (DT2) and 0x40137d1e (DN2) falls to the slew code. **[D]**
- A third compare of D3 with 6 at DT2 0x4013977e and DN2 0x40137c40 sets the start phase at a trigger to 0 for RAND instead of the SPH-based phase. A new waveform needs it too, or its start phase is ignored. **[D]**
- Each of the three compares is `moveq 6,Dn; cmp.l D3,Dn; beq|bne` in 6 bytes (the size of a `jmp.l`); the scratch registers at each are D0 and D7 or D2, A0 and A1, and D1 is live (the stock code saves it around the table call). **[D]**
- The stock tick never indexes the table above 5, so stock code cannot jump into data. An earlier plan claim of "no upper range check, wave 7 jumps into data" is wrong. **[V]**

## Function table and second reader

- DT2 table 0x402227d4, six entries: 0x40139156 TRI, 0x4013916c SINE, 0x40139138 SQR, 0x4013914a SAW, 0x401391c6 EXPO, 0x401391b6 RAMP. Slot 6 (0x402227ec) is the first long of the slew table. **[V]**
- DN2 table 0x4020b7f8, six entries: 0x4013753e TRI, 0x40137554 SINE, 0x40137520 SQR, 0x40137532 SAW, 0x401375ae EXPO, 0x4013759e RAMP. **[V]**
- Second reader of the table: DT2 FUN_401392d4 (842 bytes; caller FUN_4012c590 at 0x4012c6ec and 0x4012cc92). Same clamp at 0x401393a0..0x401393a6, table read at 0x401394e6. DN2 FUN_401376bc: clamp at 0x40137788, table read at 0x401378d0, caller FUN_4012ac88. **[V]** (DT2); DN2 **[D]**. A new waveform must patch both readers. In the second reader the three compares are at DT2 0x4013944e, 0x401394dc, 0x4013954a and DN2 0x40137836, 0x401378c4, 0x40137932; the raw wave byte is `(0x2a,A4)` on both devices, and the phase is made with `muls.l #0x32` and `mac 0x7f421f4f` before the table call. **[D]**

## Slew table

DT2 0x402227ec: 0x00000000, 0x4c6c8ea9, 0x63a9faba, 0x6cdaca2f, ... DN2 0x4020b810 (`lea` at 0x40137d2a and 0x4013793e). Q31 factors indexed by speed >> 8. Wave RAND interpolates two held random values with it. **[D]**

## Graph and unipolar table

- DT2 graph FUN_4010c5ee clamps the wave to 0..6 (negative gives 0), then indexes a 7-byte table at 0x4021bf94: `00 00 00 00 01 01 00 00`. A 1 marks a unipolar wave (EXPO and RAMP). The table is passed with `pea` at 0x4010c6ae and 0x4010c6b2. **[V]** (table bytes and `pea`); the graph clamp is **[D]**.
- DN2 graph FUN_4010df62 (from LfoPageView::vfunc_4 0x4010e394): same clamp. Table 0x40205904 (ref 0x4010e028), same bytes. **[V]** (bytes) **[D]** (clamp)
- FUN_400da25e is the per-parameter bipolar test (owners 0x1c..0x1e return true). FUN_4006aa32 is not examined. **[D]** / **[O]**

## Loaders

- Sound loader DT2 FUN_400db3b2: copies the 77 u16 words through the index table 0x40213b64 with no per-parameter clamp and no T1 lookup. Only the LFO destination words (sound+0x1c, +0x2c, +0x3c) are remapped through the table. The wave words are copied raw. **[V]**
- DN2 FUN_400dd34e: same shape, copies 0xd6 bytes raw. **[D]** (the verifier did not read the DN2 loader for this).
- Callers of the DT2 loader: FUN_400db898 (kit loader), FUN_400df95a (write_live_project), FUN_400db80c, FUN_400db830, FUN_400dfecc. **[D]**
- P-lock loader DT2 FUN_400dc60a (80 rows of 0x102 bytes): copies each u16 if it is >= 0; negative means no p-lock. Only the parameter index (< 0x4d) and the track (< 0x10) are checked. A p-lock on a wave code can carry any 15-bit value into RAM. **[D]** The DN2 equivalent is not located. **[O]**
- The generic T1 clamp is on the live set path: FUN_4003b570 (clamp to the T1 {min, max, default}, 23 callers), ParameterSet::vfunc_8 and vfunc_9. So panel edits clamp to 0x600 while the T1 max stays 0x600. Whether MIDI parameter handlers and the sequencer's p-lock apply reach it is **[O]**. **[D]**

## Stock result for wave 7 or more

- A loaded wave of 7 or more is not clamped by the loader. The consumers clamp. The MOD page shows ERR, the graph draws it as a RAND-class curve, and the sound runs like RAND. The stored value (for example 0x700) stays in the sound and in saved files. **[V]** for the consumers and the formatter.
- **[O]** Other readers of the wave word (modulation destination lists, MIDI feedback, mirror copy to the DSP frame). None appear among the data references of the T1 table, but code that indexes the parameter array by a literal offset would not show there.

## Interrupt context and rate

- The tick runs inside the vector 191 handler. Vector 191 is forced by software from the DMA completion interrupt of channel 50 (SSI0 TX). 96,000 requests per second / 64 = 1,500 Hz: one 32-sample block at 48 kHz (finding 04). Interval about 667 us. The 48 kHz assumption is **[O]** there. Whether every entry of the handler reaches the call is **[O]**.
- The tick evaluates 16 tracks x 3 LFOs = 48 times per call, 72,000 per second. **[D]**
- A table wave with 32-entry linear interpolation costs about 20 to 30 ColdFire instructions per LFO, so about 1,000 to 1,500 instructions per tick if all 48 use it (1.5 to 2 M instructions per second). This is an estimate and a ceiling. **[O]** Measure it with instruction counts.

## Output scale

- A waveform function takes a 32-bit phase at 4(A7) (phase = (accumulator * 0x636ba875 >> 32) << 2) and returns signed Q31 in D0. **[D]**
  - SQR 0x40139138: 0x7fffffff if phase < 0x80000000, else 0x80000000.
  - SAW 0x4013914a: phase ^ 0x7fffffff.
  - TRI 0x40139156: folded (phase + 0x40000000) with bit 31 flipped; full-scale triangle.
  - SINE 0x4013916c: MAC polynomial, shifted left by 6; full-scale sine.
  - RAMP 0x401391b6: phase if its top bit is 0, else 0; unipolar 0..0x7fffffff.
  - EXPO 0x401391c6: exponential through helper 0x401362d8, minus 0x00080000; unipolar decay (constants **[O]**).
- The result is scaled by depth (`mac.l A6,D0`; A6 is the depth clamped to 0..0x3fffffff, then doubled). The destination gets ((word at LFO block +8) - 0x4000) * 2 times D0, added to the destination's base word, and the sum is clamped to 0..0x7f00. DEST is at most 0x46 on DT2 and up to 100 on DN2. A table wave must return Q31 from a 32-bit phase; unipolar shapes return 0..0x7fffffff. **[D]**; the MAC fractional scale is **[O]**.

## DN2 site table

| site | DN2 1.12 | DT2 1.17 |
| --- | --- | --- |
| tick | FUN_40137a06 | FUN_4013961e |
| wave clamp in tick | 0x40137b56..0x40137b66 | 0x40139698..0x401396a8 |
| `moveq 6` and table call | 0x40137cc6, 0x40137cda, 0x40137ce6 | 0x40139804, 0x40139818, 0x40139828 |
| second compare | 0x40137d1e | 0x4013986c |
| function table | 0x4020b7f8 | 0x402227d4 |
| slew table | 0x4020b810 | 0x402227ec |
| second reader | FUN_401376bc (clamp 0x40137788) | FUN_401392d4 (clamp 0x401393a0) |
| formatters | 0x400e3610 and 0x4000766c | 0x400e1b68 |
| graph and unipolar table | FUN_4010df62, 0x40205904 | FUN_4010c5ee, 0x4021bf94 |
| T1 base | 0x401f83a4 | 0x4020f59c |
| sound loader | FUN_400dd34e | FUN_400db3b2 |
| p-lock loader | not located **[O]** | FUN_400dc60a |

## What a new waveform needs

1. The tick clamp (the upper bound 6) and the function-table call. The table has no slot for a new index, so the call site needs a hook.
2. The second reader of the table (DT2 FUN_401392d4, DN2 FUN_401376bc), with its own clamp.
3. The name formatter: one on DT2, two on DN2.
4. The graph clamp and the 7-byte unipolar table.
5. The T1 max (0x600) of codes 79, 89 and 99, so that the panel accepts the new value.

The new function must return Q31 from the 32-bit phase, as above.
