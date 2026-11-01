# DN2 stereo, Phase 1: spec

Status: accepted by Em on 2026-11-01 with the recommendations in section 7; nothing built yet. DN2 OS 1.12 (SHA-256 of the SYX in finding 96). Marks: [D] read from image bytes in this pass, [O] open, [P] proposal.
Addresses are SHARC short-word pcs. Call-site offsets in `devices/fw/dsp/voice.toml` are `pc - 0x1c8ef1` (engine function entry; check: `0x1c94fd - 0x1c8ef1 = 0x60c`, the existing first-order site).

## 0. Idea in four lines

- A stereo machine returns `DK_STEREO` and writes R to a per-slot R buffer. `dsp-voice-dn2` stamps the slot "stereo this block".
- Each pre-pan stage that has an R twin runs a second time on the R buffer, with a private R state record. L stays the stock, authoritative path.
- A stage without a twin marks the slot "mono for this block". The gain loop then plays R = L, exactly as today.
- The gain loop is patched once, per slot: a stereo slot gets `plane B = gain B * R`, a mono slot runs the stock instructions.

## 1. Gain-loop patch

### What the stock loop does [D]

Loop `DO 0x1c9b30 UNTIL LCE`, 16 trips, body `0x1c9aea..0x1c9b30` (last instruction `0x1c9b30`, `I5 = modify(I5, 8)`). Per slot:

```
1c9aea  R0 = dec(R9); I2 = DM(I3,M5)         ; R9 = 32; I2 = slot input pointer
1c9aed  M4 = DM(I5,M5)                       ; plane A pointer (I1 = 0, so M4 is the address)
1c9aef  R2 = DM(I4) post +7                  ; gain A start
1c9af1  I1 = 0 ; 1c9af3  I0 = 0
1c9af5  R8 = DM(I2,M6)                       ; L[0]            (16 bit)
1c9af6  F4 = F2*F8 ; modify(I1,M4)           ; A[0] = gA*L[0]   (48 bit: 0x043e60130428)
1c9af9  DM(I1,M5) = R4
1c9afa  M4 = DM(I5,M6)                       ; plane B pointer
1c9afc  R1 = DM(I4+0x69)                     ; gain B start
1c9aff  F15 = F1*F8 ; modify(I3,M6)          ; B[0] = gB*L[0]   (48 bit: 0x04bef0130f18)
1c9b02  R12 = DM(I4-5) ; 1c9b04 R14 = DM(I4+0x6b)   ; steps A, B
1c9b07  JUMP IF EQ delayed -> 1c9b30         ; EQ from R0 = dec(R9) (1 sample block)
1c9b0c  DM(I1,M5) = R15 ; 1c9b0d I1 = 0
1c9b0f  DO 0x1c9b2d UNTIL LCE (R0 = 31 trips), body 1c9b12..1c9b2d:
        F2 += F12 ; F1 += F14 ; F8 = F2*F0 ; F0 = F1*F0 ; stores to A and B planes
```

Both products use the same input sample (`F8`, later `F0`). That is the whole mono-to-pair step. `reguse` over `0x1c9aea..0x1c9b30` is exactly R0 R1 R2 R4 R8 R9 R12 R14 R15, F0 F1 F2 F8 F12 F14, I0..I5, M4..M6, LCNTR. The epilogue `0x1c9b33..0x1c9b72` reloads R3 R5 R6 R7 R9 R10 R11 R13 R14 R15 and I8..I11 I14 I15 from the frame, so those are dead inside the loop and free for the patch. I12 holds the return address (loaded at `0x1c9ad8`): do not touch it.

### Patch site [P]

One `replace` hook, same mechanism as `dsp.voice.hook.render_entry` (the 48-bit instruction becomes `JUMP _gs_slot`, the wrapper runs the displaced instruction itself):

- site `dsp.voice.site.gain_slot` = function `09fe195e...` (the engine function, hash already used by the existing sites), offset `0xC05` (= `0x1c9af6`), len 6. Displaced instruction: `F4 = F2*F8; modify(I1,M4)`. Continue-at for mono: `0x1c9af9` (offset `0xC08`), a second `site`.
- Why `0x1c9af6` and not a per-sample patch: a jump per sample in the inner loop costs 31 jumps per slot and the loop end must not hold a branch. One entry check per slot costs one jump.
- Why not `0x1c9aea` (first loop instruction): the first instructions of a hardware loop have branch restrictions in the sequencer; `0x1c9af6` is the seventh instruction. [O] confirm against the Programming Reference (loop restrictions on jumps) before coding; the DT2 gate-1 patch has no equivalent.
- Slot index `v = 16 - LCNTR` read at the hook (the inner `DO` at `0x1c9b0f` has not started yet). [O] check LCNTR value at entry in the model test.

### Wrapper `_gs_slot` [P]

1. `R3 = ASTATX` (save flags: `0x1c9b07` branches on the AZ from `R0 = dec(R9)`; restore before leaving). R3, R5, R6, R7, R10, R11, R13 are free.
2. `R5 = 16 - LCNTR`; `R6 = _gs_stamp[R5]`; compare with `_dv_diag[0]` (the heartbeat the adapter increments once per block in slot 0) and with `_gs_bad[R5]`.
3. Mono (stamp differs, or bad): restore flags, run `F4 = F2*F8; modify(I1,M4)`, `JUMP 0x1c9af9`. The slot then runs the original bytes: same results, only 6 to 10 more cycles per slot. Mono slots are bit-identical because nothing else changes.
4. Stereo: `I10 = _gs_rbuf + 32*v` (words). Run a copy of `0x1c9af6..0x1c9b2d` with two changes: the B products take the R sample (`R7 = DM(I10,M6)` ahead of each `F15 = F1*R7`, `F0 = F1*Rnext`), and the B stream uses its own ramp registers as in stock. Then restore flags and `JUMP 0x1c9b30` (the loop-end instruction, so the outer `DO` still counts). The copy uses its own inner `DO`; it nests inside the outer loop like the stock one. First-sample and 1-sample-block cases are copied as in stock (EQ path).
5. The pan gains stay the stock gA, gB. The DT2 stock gate-1 path gives `A = L*gA, B = R*gB` (finding 112); the DN2 loop gives the same shape, so a stereo slot with R = L reproduces the stock pair bit for bit. This is the first regression check (section 5).

Stereo mark: `_gs_stamp[v]` (16 words, `.PKDATA` in the dsp-voice DN2 object) holds the block counter when `dsp-voice-dn2` returned `DK_STEREO` for slot v. A stale flag expires by itself (a slot that changed to an FM type, a finished voice, a disabled machine). No clear is needed.

## 2. Phase 1 R chain

Rule for every twin (replay-proven in finding 96): copy the shared control words L to R before the stock L call, then run the L call, then run the same stock function on the R record and the R buffer. The R pass never publishes control state to L. Control ramps advance once per block in the L pass; an R pass on advanced state gives zero difference, so the capture must come first.

Hooks are call-site retargets (`mode = "retarget"`, as in `core/dsp-stereo-proof`). Each wrapper starts with `stamp == heartbeat && !bad` for slot v; if not, it is a pass-through to the stock function (as `_sp_render` does for the SIMD/fallback case). Slot v comes from the L record pointer (`R4 - base) / stride`, as the proof does with `engine_base + 0xe088`) or from the R-state table index.

| Stage | Site | L record (stride) | R twin | Capture before L | R state |
| --- | --- | --- | --- | --- | --- |
| first-order `0xb80c39 -> 0xb809f2` | call `0x1c94fd` (exists) | `0x24f320 + 0x70 v` | stock function on R record | words 21, 22, 23 (coefficients) | words 24, 25 histories; 112 B per slot, keep 28 words as the proof does |
| U/Y `0xb81368` | calls `0x1c959f`, late (exist) | `0x2533e0 + 0x20 v` | stock function on R record | words 2..5 | words 6, 7; 32 B |
| final gain ramp `0xb80345` | new site (call after the selected stages) | `0x2547a0 + 0x2c v` | stock function on R buffer | all 11 words (0..7, 9, 10) before L | the whole record; 44 B. It is audio-independent: R state evolves in step with L only because the capture restarts from L each block, so R output equals L's gain trajectory |
| case 5 `0xb81526` + scratch blend `0x1c9c12..0x1c9c44` | dispatch case 5 call; blend inside the continuation | `0x253a60 + 0xd4 v` | filter leaf on R record writing a private R scratch; copy of the blend loop on R | the ramp words of the record (11 + 7k, 13 + 7k, k = 0..5) | words 9, 10 histories; 212 B. The blend loads its ramp registers from the I0 record and stores nothing back to it (no store to the record in `0x1c9c12..0x1c9c44` [D]), so it needs no capture; it needs R scratch (128 B per slot) in place of `DM(0x254aa0)` |

Before each stage the R buffer already holds the R signal after the earlier twins. Final gain also covers the "gain ramps" of the request; the constant-gain multiply at the start of the chain is a scalar on the buffer: apply the same scalar to R (read it where the L multiply reads it; [O] locate the exact instruction, `0x1c9448..0x1c94fd`).

R state memory (`dsp.l2`, `.PKDATA` in a new object of the adapter): first-order 1792 B, U/Y 512 B, final gain 704 B, case 5 3392 B, R buffers 2048 B, R scratch 2048 B, tables ~300 B. About 11 KiB for 16 slots. The proof already holds 3.8 KiB of this (`_sp_states` 1792, `_sp_audio` 2048); reuse them.

### Reset, steal, retrigger [P]

- Primary: the adapter clears the slot's R records and R buffer on the machine's note-on block (`DK_IN_BLOCK == 0`, the same edge that zeroes the module's own state). R history then has the lifetime of the machine, which is what a stereo user hears.
- Secondary: mirror the firmware reset calls. Existing sites `0x1c96cb` (first-order reset) and `0x1c96aa` (U/Y reset, `0xb8135a`, `R4 = R5 + R10` in the delay slot [D]) get the R twin: the stock reset on the R record (zero the histories). The proof's `_sp_reset` does this already.
- Do not use the proof's clone-on-first-use (it copies 28 words of L state into R) in production: R must start at zero.
- Steal and retrigger both appear as a note-on edge for the slot. [O] confirm that a steal in mode 0/1/2 of `_DAT_4059da30` produces a rising edge of bit v in `ADM_TRIG_HW`, not a held bit. The model test covers it with a forced edge; the emulator test with real steal.

### Mono fallback [P]

Families without an R twin in Phase 1: `0x1cdb56` (conditional early/late), `0xb8265b` (held sample), cases 1..4, 6 (`0xb82342`, `0xb81a16`, `0xb80f2e`, `0xb806f5`). Each of their call sites gets a `mark_bad` retarget: if the slot is stamped stereo this block, write `_gs_bad[v] = heartbeat`, then tail into the stock function. The call executes only when the family is active for the slot, so the mark equals "this slot uses a stage without a twin". The gain loop reads `_gs_bad` and plays R = L (stock path). Cost: one compare and one store per call of those families. `_gs_bad` must be checked after all stages, which holds: the gain loop is the last step. [O] the dispatch cases are reached by indirect jump from table `0x8052dbc0`; if a retarget of the call site is not possible for a case, mark at the dispatch entry (one site, reads the case number) instead.

## 3. Cost and ceiling

Estimate per active stereo slot per block, 32 samples [P, not measured]:

| Part | cycles |
| --- | --- |
| first-order twin | 170 |
| U/Y twin | 240 |
| final gain twin | 140 |
| case 5 filter + blend twin | 550 |
| stereo gain loop (replaces the stock slot; one extra load per sample) | +70 |
| wrappers, capture copies | 120 |
| Total, all four active | about 1300 |
| Typical (first-order + final gain + gain loop) | about 500 |

Mono slots: 8 to 10 cycles for the gain hook plus 4 cycles per twin wrapper that falls through. 16 slots all stereo and all four stages active: about 21 k cycles per block. I did not verify the SHARC clock or the stock block load [O]; do not turn these numbers into a percentage before the ceiling is measured.

Measure, in order:

1. Ceiling first: the stock DN2 engine's busy cycles per block with 16 sounding voices (emulator SHARC cycle counter around the engine function `0x1c8ef1`, bounded with `--seconds`) against the block budget (32 samples at 48 kHz). The free cycles per block are the cap.
2. One paired A/B: `uv run python -m tools.devtools.coupled_ab A=BIN B=BIN --patch B=<stereo patch> -s <16-voice stereo scenario>`, mono 1.1 machine in A, the same machine as stereo in B. Read the busy-cycle delta, not wall time.

Cap [P]: `_GS_MAX_STEREO` = 8 stereo slots per block to start (the adapter stamps only the first 8 stereo slots it renders; the rest play R = L). Raise it from the ceiling number. No cap is needed for memory in Phase 1 (11 KiB for 16).

## 4. What breaks, and the guards

- **Sends, FX:** they read A/B after the loop (`0x1c1738`, `0x1c1f1e`) and already take two planes. Guard: a stereo slot writes both planes in full (32 samples); the test checks send levels with L != R.
- **Steal and retrigger:** section 2. Guard: R records are per physical slot, never keyed by track; no history sharing (test below).
- **Polyphony:** unchanged, 16 mono slots; a stereo voice uses one slot.
- **Stage state shared by two paths:** the R pass must not run on a record whose L pass skipped because of the stage's own conditional; the twin sits inside the same call site, so it runs only when L runs.
- **Exceptional floats:** the adapter's block guard (finite, at most 8.0) must check R as well; a bad R block clears the stamp and the slot plays R = L for that block.
- **SIMD or linked-frame entry:** the proof's wrappers fall back to the stock call when SIMD is on or L3/L4/L5 are nonzero; keep that, counted.
- **Registers and flags:** gain wrapper preserves ASTAT; R3/R5..R7/R10/R11/R13/I8..I11/I14/I15 are dead in the loop (epilogue reloads them [D]), so the wrapper needs no spill.
- **32 KiB ring (`0xb806f5`, cases 4 and 6, also in the case-5 continuation call `0x1c9c4f`):** Phase 1 does not twin it. It is a `mark_bad` family, so a slot using it plays R = L. A copied record would share the ring through word 16 (`0x804ad790 + 0x8000 v`); never copy that word.
- **Other callers of the stock functions:** retarget only the listed call sites, not the function entries.
- **Module and DT2:** nothing changes on DT2; `dsp-voice-dn2.s` is the only existing file with DN2 stereo behaviour.

## 5. Files, tests

Change / add:

- `modules/core/adapters/dsp-voice/src/dsp/dsp-voice-dn2.s`: replace `_dv_rscr` by `_gs_rbuf` (16 x 32 floats); set `out_r = _gs_rbuf + 128 v` (bytes); after the module returns, if status has `DK_STEREO` write `_gs_stamp[v]` and run the guard on R; on note-on clear the R records and the R buffer; remove the TODO at line 469. Slot stamping, the cap and the heartbeat read are here.
- New object `modules/core/adapters/dsp-voice/src/dsp/dsp-voice-dn2-stereo.s` (or a role-`adapter` module `core/dsp-stereo` that `requires` dsp-voice): `_gs_slot`, the four twins, `mark_bad` wrappers, reset mirrors. Decision below.
- `devices/fw/dsp/voice.toml`: new `site`/`hook` entries for the gain slot (`0xC05`, `0xC08`), final gain call, case 5 call, and the `mark_bad` sites; reuse the existing first-order/U-Y/reset ones. All rules by `reloc_hash` of `09fe195e...` plus offset; add the entries for the DT2 only if the DT2 path ever needs them (it does not).
- `modules/core/adapters/dsp-voice/module.toml`: objects, `[[op]]` hooks, `[[claim]]` for each site (as in `dsp-stereo-proof/module.toml`).
- `core/dsp-stereo-proof`: keep as the private observer (identity/polarity proof, diagnostics). Its states and wrappers are the starting code for the first-order and U/Y twins. Production code must not install both: the proof module claims the same call sites, so the claims already forbid it.

Model tests (shadow-replay style, `tests/patchkit/`, fixtures in `tests/fixtures/dsp/`):

1. `test_dsp_stereo_gain_slot.py`: the patched gain loop on the image bytes. Mono slots: A/B planes bit-identical to the stock loop for random inputs and gains. Stereo slot with R = L: bit-identical to stock. Stereo with R != L: `A == gA*L` and `B == gB*R` per sample over the ramp, flags preserved (the `JUMP IF EQ` at `0x1c9b07` still sees the old AZ), outer loop count intact for all 16 slots.
2. Extend `test_dsp_stereo_shadow_replay.py` with the ordered chain first-order -> U/Y -> final gain -> case-5 blend using the production wrappers: two isolated mono references (L and R inputs), three changing-control blocks, stores bounded to declared state. Negative controls: capture after L, shared record, clone-on-first-use.
3. Lifecycle model: reset mirror and the note-on clear zero R histories; a retrigger mid-tail gives block-zero output again.
4. `mark_bad`: slot using a non-twinned family gets `_gs_bad`, plays R = L.

One DN2 emulator test (`tests/regression/modules/`, TypeScript, ceiling-bounded run via the scenario runner; fixture machine of `tests/fixtures/dsp/stereo-machine.s`, as the DT2 `test_dsp_voice_stereo.py` does):

- Machine writes distinct L and R (for example a tone in L and a different tone in R), first-order filter and the final gain stage active on the track, pan at the extremes and the centre. Capture the master/send planes. Assert: L != R through the filter and pan; separation (hard-left pan: R plane energy near the stock gA/gB ratio; the gB path carries only the R tone); mono machine unchanged (golden hash).
- Two voices on different slots with different R tones, then retrigger one slot and steal the other: each slot's R history restarts from zero, the other slot's output is unchanged (no history sharing).
- Phase 1 fallback: one voice with a non-twinned stage active plays R = L (count `_gs_bad`).

Gate: `mise run module:check core/dsp-voice`, `mise run test:host` for nothing; the owner's checks only. Smoke matrix at merge (existing rule). SHARC golden hashes are unaffected (no core change). Pathcheck for the new doc references.

## 6. Phase 2 (outline)

- Twin the remaining families one at a time, each with a replay test first: `0x1cdb56` (two control groups, no audio history), `0xb8265b` (held samples, shared phase), `0xb82342`, `0xb81a16`, `0xb80f2e` (shared preparation block writes {17,18,19} + 7k).
- Ring stage `0xb806f5`: R needs its own 32 KiB ring and a constructor call (`0xb80575`); 16 x 32 KiB does not fit, so cap the number of stereo slots using it (for example 2) from a pool; allocator and release belong to the adapter; the ring-fixture proof exists.
- Raise or remove the mono fallback and the stereo cap from the measured ceiling.
- Optional: a patch of the pair gate / balance mapping if the DN2 pan differs from gate-1 on DT2.

## 7. Open questions for Em (recommendation first)

1. Where does the code live? Recommend: put the twins in the `dsp-voice` DN2 object set (one adapter, one claim owner), keep `dsp-stereo-proof` as a private observer. Alternative: a new `core/dsp-stereo` module that requires dsp-voice. Recommend against, because the gain hook and the stamp are tied to the adapter.
2. Cap of 8 stereo slots at first: recommend yes, then lift after the A/B.
3. Steal edge semantics (section 2, [O]): recommend resolving statically from the ColdFire allocator before coding the reset; if a steal holds the trigger bit, hook the mirror reset.
4. Per-slot gain hook at `0x1c9af6` (my choice) versus the per-sample patch you suggested at `0x1c9af5..0x1c9aff`: recommend per-slot; the instruction at `0x1c9af6` is inside your range. It needs the loop-branch restriction check.
5. Is "R = L whenever a non-twinned stage is active" acceptable for the first release? Recommend yes: it is exactly today's sound, and Phase 2 widens the set.
6. Use an A/B at the end only, or also a pre-build static cycle count of each twin? Recommend static count first (cheap), A/B once.

Estimate (agent time): gain hook and model test 3 to 4 h; first-order, U/Y, final gain twins with production wrappers and tests 4 to 6 h; case 5 + blend 3 h; mark_bad sites and dispatch audit 2 to 3 h; emulator test and A/B 3 h. About 2 days in total. Outcome risk: medium. Main risks: the loop-branch restriction at `0x1c9af6` (low, has an alternative), the steal/retrigger edge (medium), unlocated constant-gain instruction and exact dispatch call sites (low), cycle budget unknown until the ceiling is measured (medium; the cap is the safety valve). Confidence that mono is unaffected: high (same bytes, flags preserved). Payoff: true stereo for every machine on DN2 for the stages listed, with polyphony unchanged.
