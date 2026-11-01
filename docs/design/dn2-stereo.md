# DN2 stereo, Phase 1: spec

Status: accepted by Em on 2026-11-01 with the recommendations of section 7. Built in the lane `lane/dn2-stereo` (commits after `b9643e0d`); model tests only. The DN2 emulator proof (section 8) is not run. DN2 OS 1.12 (SHA-256 of the SYX in finding 96). Marks: **[D]** read from image bytes or measured on the model, **[O]** open, **[P]** proposal.
Addresses are SHARC short-word pcs. Offsets in `devices/fw/dsp/voice.toml` are `pc - 0x1c8ef1` (engine function entry) or `pc - 0x1c9b73` (its continuation fragment, hash `d72f9ef5...`); check: `0x1c94fd - 0x1c8ef1 = 0x60c`, the first-order site.

## 0. Idea in four lines

- A stereo machine returns `DK_STEREO` and writes R to a per-slot R buffer. `dsp-voice-dn2` checks R, scales it like the engine scales L, and stamps the slot "stereo in this block" with the block counter.
- Each pre-pan stage that has an R twin runs a second time on the R buffer, with a private R state record. L stays the stock, authoritative path. The two inline gain stages without a call (block ramp, fade) run on R in one hook before the pan loop.
- A stage without a twin clears the stamp. The pan loop then plays R = L, exactly as before.
- The pan loop is patched once, per slot: a stamped slot gets `plane B = gain B * R`, every other slot runs the stock instructions.

## 1. Static answers (all [D] unless marked)

### 1.1 Loop-branch restriction at the hook site `0x1c9af6`

A jump at `0x1c9af6` is legal. The Programming Reference (SHARC+ Core, Rev 1.5, "Restrictions on Ending Loops" p. 4-41 and "VISA-Related Restrictions on Hardware Loops" p. 4-42) limits what may stand at the end of a loop: a call in the last 5 instructions needs a return with the loop-return modifier, no `IF NOT LCE` after a `CURLCNTR` write, no `IDLE` in the last 3, the last 11 instructions are 48-bit, and the loop and PC stacks must not be pushed, popped or written inside the loop.

- The body `0x1c9aea..0x1c9b30` has 29 instructions: 18 before the inner loop, the 10 of the inner `DO 0x1c9b2d`, and the loop end `0x1c9b30`. `0x1c9af6` is the 7th, the 23rd from the end: outside every window of the rules above.
- The patch is a plain `JUMP` out and back. It touches neither stack. The wrapper's own `DO` nests as the stock inner `DO` does (stack depth 2 of 6); the stereo path ends with a `JUMP` to the loop-end instruction `0x1c9b30`, as the stock `JUMP IF EQ` at `0x1c9b07` does. The mono path returns to `0x1c9af9`.
- The hook site is a 48-bit instruction (Type 7a), as the replacement `JUMP` is. The next instruction `0x1c9af9` is 16 bits.
- Model check: the full block `0x1c99de..0x1c9b33` runs 16 slots with the hook; all 16 slots are processed (loop count intact) and a mono slot is bit-identical to the stock block. No hardware claim: the model does not model the sequencer's pipeline restrictions **[O]**, the emulator test covers the integrated run.
- The slot index is not read from `LCNTR`: the hook takes it from `I3` (the slot's entry of the table of voice buffers, `DM(I6 - 24)` is its base): `4 v = I3 - DM(I6 - 24)`. The twins take it from the voice buffer `R8` in the same table.

### 1.2 Steal and retrigger: trigger edge and reset site

- Allocator `0x4002a6a2` (called from the frame builder `0x40025e36`, vector-191 handler) reuses a slot in place when it steals or retriggers: it overwrites the slot's record (`0x40288300 + 0x14 v`: track, note, age), clears the old owner's bitmaps and returns the slot in the allocated mask (low half of the 64-bit result). Steal mode `_DAT_4059da30` 0 (oldest), 1, 2 and 3 differ only in which slot is chosen. The high half is the voices of the same note that were not reused; the builder frees them and sets their release bit.
- The builder ORs the bit of every returned slot into `local_b0` (one OR for each slot of the allocated mask, in the loop over the result), and writes it to `_DAT_80005e82` (`0x40027498`), the frame mask `0x11` that is `ADM_TRIG_HW`. `local_b0` is zeroed at every entry of the builder (every frame): the bit is a one-frame pulse.
- So a steal or retrigger gives the same pulse in the same slot as a fresh note-on: a rising edge of bit v, unless the slot was also triggered in the previous frame (two note-ons in consecutive frames; then the bit stays set and the adapter sees no edge, as for every module today). The adapter's primary R clear at the note-on block is therefore the steal and retrigger reset. **[D]** static reading of the decompiled builder and allocator (second check pending); the emulator test covers it with a real steal.
- The stock reset of the L stage states happens in the control part of the same block as the pulse (before the audio chain), at `0x1c9642..0x1c96d1` (entered from `0x1c9343`): first-order reset `0xb80b93` at `0x1c96cb`, U/Y reset `0xb8135a` at `0x1c96aa`, case 5 reset `0xb818c7` at `0x1c9673`, case 3 `0xb811f1` at `0x1c9692`, case 2 `0xb81c8d` at `0x1c9684`, case 1 `0xb81d6d` at `0x1c964b`, ring reset `0xb805d8` at `0x1c96b3`. The R mirror is built for the first two (sites `0x1c96cb`, `0x1c96aa`). The case 5 R record is cleared at the note-on block only: a stock reset of case 5 in the middle of a note is not mirrored **[O]**.

### 1.3 The constant gain of the chain (`0x1c9448..0x1c94fd`)

The loop `0x1c9478..0x1c94c5` multiplies every voice buffer by the constant `0x3fcade01` (about 1.5898, loaded at `0x1c946b` and `0x1c948c`) in SIMD pairs (`MODE1` PEYEN set at `0x1c9484`, cleared at `0x1c94b0`). It runs for all 16 buffers right after the render-hook loop (`0x1c9453..0x1c9467`), skipped only for a block count of zero. Its multiplications are plain 32-bit products, so a scalar multiply of R gives the same bits. `_gs_post` scales R by `0x3fcade01` after the guard.

### 1.4 Two inline stages the first spec did not list

`0xb80345` is the envelope generator, not a "final gain ramp". After the gain loop `0x1c99bd..0x1c99d6` two more inline stages multiply the L buffers, with no call, so the twins cannot reach them:

- **Block ramp** `0x1c99ed..0x1c9a31`: for every slot, `out[k] = r_k * in[k]`, `r_0 = DM(rec)`, `r_{k+1} = r_k + DM(rec + 2)`; the record is `engine + 0x1395c + 28 v` (7 words). Unconditional.
- **Conditional fade** `0x1c9a59..0x1c9aac`: for slot v when byte v of the array `engine + 0x138fc` (a byte access, Type 3b with `(l, x, w) = (0, 0, 0)`) and the word `engine + 0x1351c + 44 v` (word 5 of the `0xb80345` record) are both nonzero: `r` runs from 1.0 with the step `-1/31` (`0xbd042108`, the value of the stock division `0x1c06ba` of -1.0 by 31.0; tested against the stock block).
- The metadata clear `0x1c9aaf..0x1c9acf` zeroes the fade flags before the pan loop. A pan-loop hook cannot read them; the R fade needs a hook before the clear (section 2.3).

Both stages are linear per-sample gains, so R needs the same multiplications in the same order.

### 1.5 Exact call sites of the stages (the `mark_bad` and twin retargets)

The dispatch table at `0x8052dbc0` holds `0x1c98b9` (case 0 and above 6: skip), `0x1c98ab` (1), `0x1c9ba9` (2), `0x1c9bc4` (3), `0x1c9bdf` (4), `0x1c9bf7` (5), `0x1c9c45` (6). The first spec called `0x1c9c4f` the case 5 continuation: it is case 6's ring call.

| Stage | Call site (pc) | Rule: function + offset | Kind |
| --- | --- | --- | --- |
| first-order `0xb80c39` | `0x1c94fd` | engine `0x60c` | twin (existing site) |
| first-order reset `0xb80b93` | `0x1c96cb` | engine `0x7da` | mirror |
| U/Y `0xb81368` early | `0x1c959f` | engine `0x6ae` | twin |
| U/Y late | `0x1c9b7a` | fragment `0x7` | twin |
| U/Y reset `0xb8135a` | `0x1c96aa` | engine `0x7b9` | mirror |
| held sample `0xb8265b` early / late | `0x1c95b4` / `0x1c9b8f` | engine `0x6c3` / fragment `0x1c` | bad |
| shaper `0x1cdb56` early / late | `0x1c95c9` / `0x1c9935` | engine `0x6d8` / `0xa44` | bad |
| case 1 `0xb82342` | `0x1c98af` | engine `0x9be` | bad |
| case 2 `0xb81a16` | `0x1c9bb6` | fragment `0x43` | bad |
| case 3 `0xb80f2e` | `0x1c9bd1` | fragment `0x5e` | bad |
| case 4 ring `0xb806f5` | `0x1c9be9` | fragment `0x76` | bad |
| case 5 `0xb81526` | `0x1c9c08` | fragment `0x95` | twin + blend |
| case 6 ring `0xb806f5` | `0x1c9c4f` | fragment `0xdc` | bad |
| gain `0xb80345` | `0x1c99cc` | engine `0xadb` | twin |
| pre-pan hook | `0x1c9aaf` | engine `0xbbe` (len 6; next `0x1c9ab2`, `0xbc1`, len 4) | replace |
| pan hook | `0x1c9af6` | engine `0xc05` (next `0x1c9af9`, `0xc08`, len 2; end `0x1c9b30`, `0xc3f`) | replace |

All of them are call instructions with both delay slots, retargeted as the existing sites. The stages `0xb8265b` (entry inside the function at `0xb8258f`, offset `0xcc`) and the dispatch stages have one caller each in the table above; the other callers of the shared functions are not retargeted. Every call has R8 = the voice buffer, the same table entry as the slot's pointer in the pan loop.

## 2. Phase 1 as built

### 2.1 Adapter (`dsp-voice-dn2.s`, `_gs_post` in `dsp-voice-dn2-stereo.s`)

- `out_r = _gs_rbuf + 128 v` (16 x 32 floats). `_dv_rscr` and the TODO are gone. The status bit `DK_STEREO` is kept in `_dv_c + 48`.
- After the guard of L: `CALL _gs_post`. For a mono return it is 4 instructions. For a stereo return: guard of R (finite, at most 8.0; else no stamp, no fault: the slot plays R = L for the block), the cap, R times `0x3fcade01`, the stamp.
- Cap `_GS_MAX_STEREO` = 8: the first 8 stereo slots of a block are stamped (counter `_gs_ns` of the block `_gs_nhb`); the others play R = L. A slot that stops being stereo frees its place in the next block.
- Stamp: `_gs_stamp[v] = heartbeat` (`_dv_diag` word 0, once per block, slot 0). It expires with the next block.
- R stage records (first-order 28 words, U/Y 8, gain 11, case 5 53) are cleared at the note-on block and whenever the stamp of the previous block is not the previous heartbeat (a gap: a stage without a twin cleared it, the cap, a bad R). R buffers are never cleared: a stereo machine writes all 32 samples.
- Memory (`.PKDATA`, loaded as zeros): stamps 64 B, R buffers 2048, first-order 1792, U/Y 512, gain 704, case 5 3392, case 5 R scratch 2048, counters 12: about 10.4 KiB for 16 slots.

### 2.2 Twins (`_gs_fo`, `_gs_uy`, `_gs_gain`, `_gs_c5`)

Rule (replay-proven in finding 96): copy the shared control words L to R before the stock L call, run the L call, run the same stock function on the R record and the R buffer. The R pass never publishes control state to L.

| Stage | L record (stride) | Captured before L | R state kept |
| --- | --- | --- | --- |
| first-order | `engine + 0xe088 + 0x70 v` | words 21 to 23 | histories 24, 25 (record 112 B) |
| U/Y | `engine + 0x12148 + 0x20 v` | words 2 to 5 | histories 6, 7 (32 B) |
| gain `0xb80345` | `engine + 0x13508 + 0x2c v` | all 11 words | none: the record is a copy of L's pre-call state, so R follows L's gain trajectory |
| case 5 `0xb81526` | `engine + 0x127c8 + 0xd4 v` | words 11 to 48 (38) | histories 9, 10 (212 B) |

- A wrapper starts with the stamp check; an unstamped slot costs 13 instructions and reaches the stock function with R4, R8, R12 and the stack unchanged. The stereo path needs L3, L4, L5 = 0 and a block count of 32 (else it clears the stamp and calls the stock function: R = L). MODE1 PEYEN is clear at every call site: the four SIMD sections of the engine between `0x1c9440` and `0x1c9ad2` (`0x1c9484..0x1c94b0`, `0x1c98f9..0x1c9926`, `0x1c9948..0x1c9975`, `0x1c9aaf..0x1c9ad2`) hold no call and are cleared before the next one.
- Registers: the wrappers clobber only R0, R1, R2, I4, I12 and the flags (the C convention, `sdk/dsp/call.inc`); the callee-saved ones are kept and tested.
- Case 5: the scratch block R12 of the L call is the firmware's (`DM(engine + 0x13808)`); the R pass writes the slot's R scratch. The blend that follows (`0x1c9c12..0x1c9c44`) runs for R inside the wrapper, reading the same ramp record `I0` (the caller's I0, which the stock call keeps) and writing nothing to it. The L blend stays the stock continuation.
- Stages without a twin (`_gs_bad_*`): clear the stamp of the slot and jump to the stock function. `_gs_nbad` counts the stamps cleared.

### 2.3 Hooks in the pan section

- Pre-pan hook `_gs_prepan` at `0x1c9aaf` (before the metadata clear): for every stamped slot, the block ramp and the fade on its R buffer, in place, as in section 1.4. Live in: I0, M13, M5, M7, R9, R11, I6; everything else is dead (`regs` in `voice.toml`). It needs R9 = 32.
- Pan hook `_gs_slot` at `0x1c9af6`, once per slot. Mono path (no stamp of this block, or R9 not 32): `I1 = 0; R0 = R9 - 1` (the register and the flags of the hook), the displaced `F4 = F2 * F8; modify(I1, M4)` as two instructions, jump to `0x1c9af9`. Stereo path: plane A as the stock loop, plane B from R, 31 trips of a 5-instruction loop; I3 is advanced by one word, as the stock `modify(I3, M6)` does (written `MODIFY(I3, 4)`: the assembler's `MODIFY(I3, M6)` is a raw 1-byte step, the stock instruction is the word-scaled Type 7a form); jump to `0x1c9b30`. Dead registers used: R1, R3 to R7, R10 to R15, I0, I1, I3 (a DM access takes I0 to I7).
- The pan gains are the stock gA, gB records (`TABLE + 0x354 + 28 v`, words 0, 2 and 112, 114). A stamped slot with R = L reproduces the stock pair bit for bit (tested).

### 2.4 Reset mirrors

`_gs_fo_reset` (site `0x1c96cb`) and `_gs_uy_reset` (`0x1c96aa`): zero the R histories (words 24 to 27, words 6 and 7) of the slot whose L record is R4, whatever the stamp says, then the stock reset. A pointer that is not the start of a slot's record only reaches the stock reset.

## 3. Cost (instruction counts on the model, 1 instruction = 1 cycle, no stalls) and ceiling

Measured with the stock image on the model rig (`tests/support/dn2_stereo_rig.py`). Per call of a stereo slot, stock-plus-R pass against the stock call alone (the wrapper is included):

| Part | Spec estimate | Measured: stock call | Measured: twin call | Twin minus stock (R pass + wrapper) |
| --- | --- | --- | --- | --- |
| first-order | 170 | 657 | 1386 | 729 |
| U/Y (per site; early or late) | 240 | 204 | 480 | 276 |
| gain `0xb80345` | 140 | 237 | 616 | 379 |
| case 5 + blend | 550 | 489 | 1553 | 1064 |
| block ramp (pre-pan, per stamped slot) | | | | 128 (+ 128 with the fade) |
| pan hook: stereo slot against a mono slot | +70 | | | +15 (the stereo loop is 5 instructions per sample) |
| adapter `_gs_post` (guard, scale, stamp) | 120 | | | about 230; +162 at a note-on block (record clear) |

- Unstamped slot: 13 instructions per wrapper call, 14 for the pan hook and about 12 for the pre-pan loop: 423 instructions for the 16 slots of a block with no stereo slot (6.3 % of the stock block `0x1c99de..0x1c9b33`, 6673 instructions; 0.06 % of the 666,667 instructions of a DSP block at 1 GHz).
- A stereo slot with all four stages: 729 + 276 + 379 + 1064 + 15 + 230 about 2700 instructions (spec: 1300). Typical (first-order, gain, ramp, pan, adapter): about 1350 (spec: 500). The first spec estimates were low because the stock stages are longer than assumed (first-order is 657 instructions, 20 per sample).
- 8 stereo slots with all stages: about 21.6 k instructions per block, 3.2 % of a block's 666,667 (stalls would add 15 to 30 %). 16 slots would be 6.5 %. The stock block runs at about 50 % (finding 06, "Real cycles per DSP block"). The cap of 8 is not needed for the budget; it stays as the safety valve until the A/B.
- Ceiling and A/B on the emulator are still to do (section 8).

## 4. What breaks, and the guards

- **Sends, FX:** they read A/B after the loop and already take two planes. A stereo slot writes both planes in full; tested with L != R.
- **Steal and retrigger:** section 1.2. R records are per physical slot, never keyed by track; no history sharing (tested: records of other slots are untouched).
- **Polyphony:** unchanged, 16 mono slots; a stereo voice uses one slot.
- **Stage state shared by two paths:** the R pass runs inside the call site of its stage, so it runs only when L runs.
- **Exceptional floats:** a bad R block (not finite, over 8.0) gives no stamp: R = L for that block, no fault.
- **Unsupported context:** L3, L4 or L5 nonzero, or a block count other than 32, clears the stamp and calls the stock function (R = L).
- **Registers and flags:** the pan hook uses only registers the epilogue reloads (section 2.3); the mono path restores R0 and the flags as at the hook.
- **32 KiB ring (`0xb806f5`, cases 4 and 6):** not twinned; a `_gs_bad_ring` family, the slot plays R = L. A copied record would share the ring through word 16; never copy that word.
- **Other callers of the stock functions:** only the sites of section 1.5 are retargeted.
- **Module and DT2:** nothing changes on DT2 (the new ops and claims are `devices = ["dn2"]`); `dsp-voice-dn2.s` and `dsp-voice-dn2-stereo.s` are the only DN2 files with stereo behaviour.
- **`core/dsp-stereo-proof`:** stays the private observer. It claims the first-order and U/Y sites too, so patchkit refuses both modules together (tested).

## 5. Files and tests

- `modules/core/adapters/dsp-voice/src/dsp/dsp-voice-dn2.s`, `dsp-voice-dn2-stereo.s` (new): one object, `module.toml` lists both sources.
- `devices/fw/dsp/voice.toml`, `modules/core/adapters/dsp-voice/module.toml`, `devices/ports/dn2-1.12.toml`, `modules/core/fw/reference.json`, `build/dsp-voice-dn2-dsp.dko.json`. The DN2 1.11 port was not regenerated: it was already stale for other anchors.
- `tests/support/dn2_stereo_rig.py` (stock image plus the stereo object, patch overlays, synthetic engine memory), `tests/support/machine_rig.py` (the adapter now has two sources; R buffer in the write set).

Model tests:

1. `tests/patchkit/test_dsp_stereo_gain_slot.py`: the stock block `0x1c99de..0x1c9b33` with both hooks against the stock block on L and on R: mono slots bit-identical, stereo slots A = stock(L).A and B = stock(R).B, with block ramp and fade (including the flags per slot), loop counts, flags (block of one sample), live-out registers.
2. `tests/patchkit/test_dsp_stereo_twins.py`: each twin against two isolated mono channels over 4 to 9 blocks with changing controls, callee-saved registers, unstamped slot is the stock call; negative controls (shared history, controls copied after the L call).
3. `tests/patchkit/test_dsp_stereo_lifecycle.py`: reset mirrors (real stock resets), the six `_gs_bad_*` wrappers, unsupported contexts.
4. `tests/patchkit/test_dsp_voice_stereo_dn2.py`: the adapter on the machine rig: stamp, R scaling, bad R, note-on clear, gap, retrigger, cap of 8, mono unchanged, the proof module cannot be installed with it.

## 6. Phase 2 (outline)

- Twin the remaining families one at a time, each with a replay test first: `0x1cdb56` (two control groups, no audio history; its x16 and x1/16 scalings `0x1c98f9..0x1c9975` run on R too), `0xb8265b` (held samples, shared phase), `0xb82342`, `0xb81a16`, `0xb80f2e` (shared preparation block writes {17,18,19} + 7k).
- Ring stage `0xb806f5`: R needs its own 32 KiB ring and a constructor call (`0xb80575`); 16 x 32 KiB does not fit, so cap the number of stereo slots using it (for example 2) from a pool.
- Mirror the case 5 reset (`0xb818c7` at `0x1c9673`) and the other stage resets.
- Raise or remove the cap from the measured ceiling.
- Optional: a patch of the pair gate / balance mapping if the DN2 pan differs from gate-1 on DT2.

## 7. Decisions of Em (2026-11-01)

1. The twins live in the `dsp-voice` DN2 object set (one adapter, one claim owner); `dsp-stereo-proof` stays a private observer.
2. Cap of 8 stereo slots at first, lifted after the A/B.
3. Steal edge resolved statically before the reset code (section 1.2).
4. Per-slot gain hook at `0x1c9af6` (section 1.1).
5. R = L whenever a non-twinned stage is active.
6. Static cycle count first (section 3), one A/B later.

## 8. What the DN2 emulator test must check after integration

The model has the stock image's engine bytes but not the real engine state, the allocator or the audio path.

- The patched image boots and plays a stock DN2 voice with the hooks installed: the audio is bit-identical to the unpatched image (golden hash), 16 voices included.
- A stereo machine on a track (`tests/fixtures/dsp/stereo-machine.s`): a tone in L and another in R with the first-order filter active and the pan at the extremes and the centre. L != R in the master and send planes; hard-left: plane B carries the R tone only.
- Two voices on different slots with different R tones; retrigger one slot and steal the other (a 17th note): each slot's R history restarts at its note-on block, the other slot's output is unchanged. This confirms section 1.2 on the real allocator.
- One voice with a non-twinned stage active (filter types that dispatch to cases 1 to 4 or 6, the shaper or the held sample) plays R = L: `_gs_nbad` counts it, the planes equal the mono result.
- The fade: end a stereo voice (release) and check that R fades with L in the last block (no click). The block ramp: a level change on a held stereo note moves R and L together.
- The busy-cycle ceiling with 16 sounding voices, and one paired A/B (`tools/devtools/coupled_ab.py`) with 8 stereo voices: read the busy-cycle delta; compare with the 21.6 k instructions per block of section 3.
- The pan hook on the real core: all 16 slots are processed with the hook (loop count), no sequencer stop.
- Hardware only after all of the above.
