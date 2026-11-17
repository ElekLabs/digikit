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
- Each of the three compares is `moveq 6,Dn; cmp.l D3,Dn; beq|bne` in 6 bytes (the size of a `jmp.l`). Dn is not the same register on every site and device: see "Registers". **[D]**
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
- Cost of an extended shape, counted from the built object (`lfo.dko.json`, decoded with `--decode`; ColdFire instructions on the path that finds a shape): the stub (site `jmp.l`, `moveq`, `cmp.l`, the branch around the slot test, `move.l slot,d0`, `beq`, `move.l d0,-(sp)`, `rts`) 8, the routine 18 (DT2 and DN2 `tick_call`: the raw byte test 5, the saves 3, the phase 4, the argument and `jsr` 2, the restores and `jmp` 4; the second reader's routine is 20, it multiplies by 0x32 first), `dk_lfo_eval` 28 (a shape found: the table index, one cell load, `muls.l`, shift, add) in all 54 instructions per LFO in the tick. A stock table wave costs 15 instructions around the call plus the function: TRI 7, SQR and SAW about 5, RAMP 7, SINE 27, so 22 (TRI) to 42 (SINE), EXPO more; against TRI and SINE the extended path adds 32 and 12. A wave of 0 to 5 with the slot filled costs 2 `jmp.l` more than stock. A RAND LFO with the slot filled costs 9 more (the stub 8 and the routine's reject path 4, against the stock 3). The slew and start sites cost about 14 each (stub and the routine's test and `jmp`) on the cycles they are reached. The second reader is not in the vector 191 handler (caller FUN_4012c590 at DT2 0x4012c6ec and 0x4012cc92; its rate is **[O]**). Budget and result: docs/plans/extension-model.md E6, item 2. **[D]**
- Stack of the extended path in the interrupt (from the routine's entry, which the stub reaches with `rts`, so the stub adds none): the saves of D1, A0, A1 (12 bytes; DN2 tick 8), the wave and the phase arguments (8), the return address (4) and the saved D2 of `dk_lfo_eval` (4): 28 bytes (DN2 tick 24). The deepest stock path, EXPO, has the phase and its return address (8), its saved D2, argument and return address of the helper (12): 20 bytes at least. The size of the interrupt stack is **[O]**. **[D]**

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

## Registers at the six sites

Found with `uv run python -m tools.coldfire.liveness PORT SITE` (docs/DEVELOPMENT.md, "ColdFire register liveness"): it decodes the stock bytes with the repo's SLEIGH language, follows fall-through and branch targets from a start address (300 instructions per path at most) and calls a register **dead** when every path writes the whole register before any read, call, return or indirect jump. That is the strict model. All lines are **[D]**: one tool run on the image, not yet checked by a second agent against the image bytes.

| site | address | displaced | stock branch | dead where "clamped wave is 6" continues | dead at the exit of an extended shape |
| --- | --- | --- | --- | --- | --- |
| DT2 tick_call | 0x40139804 | moveq 6,D7 | beq 0x40139832 | 0x40139832: D0, N Z V C X | 0x4013989e: D2 D3 D5 D7, N Z V C X |
| DT2 tick_slew | 0x4013986c | moveq 6,D2 | bne 0x4013989e | 0x40139872: D0 D2 D3 D5 D7 A0 A1, N Z V C | 0x4013989e: D2 D3 D5 D7, N Z V C X |
| DT2 tick_start | 0x4013977e | moveq 6,D0 | beq 0x4013979a | 0x4013979a: D1 D2 D7, N Z V C | 0x40139784: D0 D1 D2 D7, N Z V C |
| DT2 read_call | 0x401394dc | moveq 6,D0 | beq 0x40139512 | 0x40139512: D0, N Z V C X | 0x4013957a: D2 D3 A6, N Z V C X |
| DT2 read_slew | 0x4013954a | moveq 6,D2 | bne 0x4013957a | 0x40139550: D0 D2 D3 A1 A6, N Z V C | 0x4013957a: D2 D3 A6, N Z V C X |
| DT2 read_start | 0x4013944e | moveq 6,D2 | beq 0x4013946a | 0x4013946a: D2 D6, N Z V C | 0x40139454: D0 D2 D6, N Z V C |
| DN2 tick_call | 0x40137cc6 | moveq 6,D0 | beq 0x40137cec | 0x40137cec: D0, N Z V C X | 0x40137d52: D1 D2, N Z V C X |
| DN2 tick_slew | 0x40137d1e | moveq 6,D1 | bne 0x40137d52 | 0x40137d24: D0 D1 D2 A0 A1, N Z V C | 0x40137d52: D1 D2, N Z V C X |
| DN2 tick_start | 0x40137c40 | moveq 6,D0 | beq 0x40137c5c | 0x40137c5c: D1 D2 D7, N Z V C | 0x40137c46: D0 D1 D2 D7, N Z V C |
| DN2 read_call | 0x401378c4 | moveq 6,D0 | beq 0x401378fa | 0x401378fa: D0, N Z V C X | 0x40137962: D2 D3 A6, N Z V C X |
| DN2 read_slew | 0x40137932 | moveq 6,D2 | bne 0x40137962 | 0x40137938: D0 D2 D3 A1 A6, N Z V C | 0x40137962: D2 D3 A6, N Z V C X |
| DN2 read_start | 0x40137836 | moveq 6,D2 | beq 0x40137852 | 0x40137852: D2 D6, N Z V C | 0x4013783c: D0 D2 D6, N Z V C |

- The third column of dead registers is where an extended shape (raw byte 7 to 14) continues: the code after the table call for the two `call` sites, the shared "done" code for the two `slew` sites (the stock `bne` target), the SPH phase code (site + 6) for the `start` sites. The stub scratch has to be dead in both columns of its site, and the displaced `Dn` is not always dead: DT2 tick_call (D7), DT2 tick_start and both `start` sites of DN2 (D0) are live (strict), so the stub does not use them. Chosen scratch: DT2 d0, d2, d2, d0, d2, d2 and DN2 d0, d1, d2, d0, d2, d2 for tick_call, tick_slew, tick_start, read_call, read_slew, read_start. The data register stub (`move.l slot,dN; beq.b +4; move.l dN,-(sp); rts`) changes N Z V C and never X; the routines use only `mvs.b`, `cmpi.l` and branches before they leave for a stock path, so X is untouched there.
- The `slew` sites are reachable by an extended shape. The stock code reaches the slew compare (DT2 0x4013986c, DN2 0x40137d1e, and in the second reader DT2 0x4013954a, DN2 0x40137932) from two places: after the RAND code, and by a branch that skips the wave call (`--preds FUNC ADDR` lists the ways in): DT2 `0x40139802: beq 0x4013986c` (after `tst.b (A2)` and `tst.l D1`, 0x401397fc to 0x40139802), DN2 `0x40137cc4: beq 0x40137d1e`, second reader DT2 `0x401394da` and DN2 `0x401378c2`. A shape has the clamped wave 6, and a call routine of the `call` site leaves for the done code; but on this branch the `call` site is not reached, so the `slew` compare sees the clamped 6 and the stock code would run the RAND slew code on a shape. These `slew` stubs and routines are needed, and they stay: the routine sends a raw byte of 7 to 14 to the done code, as for every other table wave. **[D]** (`--preds` finds one way in on each of the four functions besides the fall-through of the RAND code; the meaning of the tested bytes `(A2)` and D1 is not read.)
- Calling convention. Strict treats every call and return as a possible read of every register, and no decision below rests on a calling-convention model. `--abi gcc` (a call clobbers D0, D1, A0, A1 and the flags) is printed for orientation only: it is a guess about compiled code, not a fact of the image. The scratch registers above come from strict liveness. A stock call path keeps D1 in its frame (DT2 0x40139824, 0x4013982c), so the routines that call `dk_lfo_eval` push D1 (DT2 tick and both readers; DN2 tick: D1 is dead at its exit, strict). Strict liveness does not prove A0 and A1 dead at the code after the table call (tick_done, read_done: both are live in the strict table above, because a path reaches a call or return with them untouched), so every routine that calls `dk_lfo_eval` saves and restores A0 and A1 around the call, as it does D1 where D1 is live. `dk_lfo_eval` (built object, decoded with `--decode`) writes D0, D1, A0, A1 and the flags, and saves D2 itself: after the saves and restores a routine changes D0 (the result), D2 (the phase, as the stock code does), the flags, and on DN2 D1. **[D]**
- The six table functions write only D0, D1, A0, ACC0 and the flags (`--writes ADDR`; the EXPO helper is resolved through its `lea`, the reporter of the MACSR check is left out with `--skip`): DT2 0x40139156 TRI, 0x4013916c SINE, 0x40139138 SQR, 0x4013914a SAW, 0x401391c6 EXPO, 0x401391b6 RAMP; DN2 0x4013753e, 0x40137554, 0x40137520, 0x40137532, 0x401375ae, 0x4013759e. **[D]**
- D1 is live at the code after the call on DT2 (tick) and on both devices in the second reader: the stock call path saves it in its frame, so the routines of those sites push and pop it around `dk_lfo_eval`. D1 is dead at the same place of the DN2 tick (strict, written at 0x40137d6e): that routine saves nothing. **[D]**
- The raw wave byte: `mvs.b 2(a3),d3` at DT2 0x40139698 and `mvs.b 0x4c(a4),d3` at DN2 0x40137b56 (the tick), `mvs.b 0x2a(a4),d3` at DT2 0x4013938c and DN2 0x40137774 (the second reader). The routines read the same field again, so the base register must hold the same block at each site. `--stable FROM TO REG --skip FAULT` lists the calls on the paths from the read to a site and runs `writes_of` on each callee (a call is a writer only when its callee, with its direct callees, writes the register; `jsr (An)` after one `lea (ABS).l,An` is resolved; no calling convention is assumed). Result for every pair (the read to each of the six sites of its function; paths that go round the loop pass the read again and are cut there): no writer of A3 (DT2 tick) or A4 (DN2 tick, both second readers). The calls on the paths are DT2 0x401396c2 to 0x401390a4 (the EXPO function: writes A0, ACC0, D0, D1, D2 and the flags), 0x401397a4 and 0x401397be to 0x40139120 (writes D0 and the flags), and in the second reader 0x40139472 to 0x40139120; DN2 0x40137b80 to 0x4013748c, 0x40137c66 and 0x40137c80 to 0x40137508, and 0x4013785a to 0x40137508 (the same functions). One callee is left out with `--skip`: the EXPO function 0x401390a4 (DN2 0x4013748c) calls a reporter (DT2 0x40188098, DN2 0x4017d190) only on the branch where `MACSR & 0xf0` is not 0x20 (the accumulator mode check at its entry); that reporter is not followed, so the claim holds for every path that does not take that error branch. **[D]**
- The phase of the stock fall-through, from the bytes: tick `move.l #0x636ba875,d0; mac.l d0,d2,acc0; movclr.l acc0,d2; lsl.l #2,d2` (bytes `203c636ba875 a4000800 a1c2 e58a`, DT2 0x4013980a, DN2 0x40137ccc); second reader `move.b #0x32,d0; ...; muls.l d0,d2; move.l #0x7f421f4f,d0; mac.l d0,d2,acc0; movclr.l acc0,d2; lsl.l #2,d2` (the `move.b` keeps the zero upper bits of the moveq's 6, so d0 is 0x32; bytes `4c002800 203c7f421f4f a4000800 a1c2 e58a`, DT2 0x401394f0, DN2 0x401378d8). `mac.l`, `movclr.l` and `muls.l` assemble to these bytes with the GNU assembler (`mac.l %d0,%d2,%acc0` is `a4000800`, `movclr.l %acc0,%d2` is `a1c2`, `muls.l %d0,%d2` is `4c002800`). The second reader's call path stores D1 at 0x30(sp) after pushing the phase; the tick's DT2 path at 0x34(sp); DN2's tick path stores nothing. **[D]**

## The interrupt mask and the vector 191 level

`dkrt_irq_mask` sets SR to 0x2700 (modules/core/runtime/src/cf/tramp.s: `move.w #0x2700,%sr`), which masks interrupt levels 1 to 6. The vector 191 handler is installed with `moveq #5,d0; move.b d0,ICR1_63` on both images (the literal 0xfc04c07f follows the vector slot literal 0x400002fc at DT2 1.17 0x400cf05c and at DN2 1.12 0x40025586 and 0x400d137c), and the ICR holds the interrupt level in its low three bits (MCF5441x RM, INTC chapter, ICRn: "Interrupt level", 7 the highest). Level 5 is below 7, so a write under `irq_mask` cannot be interrupted by the LFO tick. **[D]**
