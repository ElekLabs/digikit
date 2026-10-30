# 49. Where the DSP uses a track's machine type, and the emulator with unwritten memory filled: rung C still passes

Lane b, 2026-10-18. Question: on the Digitakt II 1.17 device a track on a run-time machine type (`test/dyn-pluck`, type 18) stops the SHARC
(finding 47, "Rung C"), while `core/dsp-voice`'s hook never sees the type (`dispatches=0`), and the same flow passes in the emulator (scenario
`rung-c`). Emulator and static work only; nothing was sent to a device. Marks as in AGENTS.md: **[D]** measured or read once here, **[O]** open;
nothing is **[V]**. **No cause was found.** This finding gives the sites, rules out two causes in the emulator, narrows where the halt is, and
lists the next steps.

## 1. What the device log fixes (reading of finding 47 and `out/runs/device-logs/rungC-3.log`) **[D]**

- The last full block shows the wrapper's stage-1 breadcrumb (`HOOK_ENTERED voice=31 type=0`) and the two reply records differ by one block
  (beats 233,838 and 233,839). Stages 2 to 7 write their word **straight into both reply records** (finding 48). Had the first type-18 voice
  (voice 2, track 1) reached stage 2, that word and a non-zero `dispatches` would sit in the records, and the DMA keeps sending them. They do
  not. So the core stopped **before the stage-2 write of the first type-18 voice of its block**: in the block handler's frame handling, in the
  stock voice code of voices 0 and 1 (track 0), or in the wrapper between its entry and the stage-2 `CALL` (about 30 instructions, section 5).
  Stopping inside the routine, or on the way into it, is excluded by the same argument (stage 4 writes the record before `JUMP (M13, I12)`).
- The ColdFire block counter (`blocks` in every log line) keeps rising at 1,500 per second after the stop (233,093 at the stop, 257,094 at
  171 s), so the ColdFire keeps its audio block cycle and its frame exchange running. The frozen beats are the DSP's, not a stalled ColdFire
  task. **[O]** A stalled frame builder with a still-running block counter cannot be excluded from the log alone.
- `MOD_LIST ... slots=0` at the stop is the module's ColdFire **callback slot mask** (`loader.rs`, `slot_mask`), which `dyn-pluck` does not use.
  It says nothing about the machine row.

## 2. Where the DSP code uses a track's machine type, DT2 1.17 **[D]**

Found with `sharcdb` (`literals`, `dataref`, `readers`/`writers` of `tools/sharc_dev/sharc.py`) and `sharcfn --listing` of the functions
around each site; the loader image gives the table bytes (the table at 0x2567c0 is initialised data of the boot stream: its words are the same
on the device). `type` is `cache[t]`, the short word at 0x255970 + 2t (the DSP's copy of frame word 0x94 + 2t); "selector" is the word the stock
code stores at voice-record +0x4c.

| # | where | what it does with the type | table | entries it covers | extended by our modules? | what a type 16 to 23 reads or does |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `FUN_1c2b24` (frame handler), 0x1c2ccb | copies frame word 0x94 + 2t into the frame mirror | none | any value | no | copied unchanged |
| 2 | `FUN_1c2b24`, 0x1c33c1 to 0x1c33f1 | change detector: `cache[t]` against the live word (`comp`); on a change stores constants (M13/M14) into the per-track arrays 0x24f0ac and 0x24f0ec and calls 0x1c60a2 (sets byte +0x1b9 of voice record t) | none | any value | no | nothing is indexed by the value |
| 3 | `FUN_1c24e9` (parameter converter), 0x1c26ce (`M2 = cache[t]`) and 0x1c273c (`S2 = DM(I3, M2)`, `I3 = 0x2567c0`) | selector = word `0x2567c0 + 4 * type`, stored in the track's voice records (+0x4c) | 0x2567c0 | **8 words belong to it**: 0 1 2 3 4 0 5 and word 7, a flag (1 in the image, cleared by 0x1c2c8b in the first frame). Words 8 to 13, 16 to 25 are pointers of a neighbouring structure (0x2567e0 on, set up by the code at 0x1c1d22), words 14, 15, 26 on are 0 | no | type 16: 0x253e78, 17: 0x253ef8, **18: 0x254178**, 19: 0x2541f8, 20: 0x254278, 21: 0x2542f8, 22: 0x254878, 23: 0x2548f8. All are 7 or more, so every guard below skips the voice |
| 4 | `FUN_1c642a`, 0x1c6553 | `compu(selector, 6)`; below 6: `I12 = DM(0x8055c840 + 4 * selector)`, `JUMP (M13, I12)` at 0x1c6579 (stage A of the sample path) | PM table 0x8055c840 | 6 | no | `JUMP IF GE` at 0x1c6561: stage A skipped, no table read |
| 5 | `FUN_1c642a`, 0x1c66d6 | `compu(selector, 7)`; below 7: table read and `JUMP (M13, I12)` at 0x1c66ec | 0x8055c858 | 7 | no | `JUMP IF GE` at 0x1c66d8: skipped |
| 6 | `FUN_1c642a`, 0x1c6c10 | `compu(selector, 7)`; below 7: table read and `JUMP (M13, I12)` at 0x1c6c25 | 0x8055c874 | 7 | no | `JUMP IF GE` at 0x1c6c13: skipped |
| 7 | `FUN_1c71ec`, 0x1c7511 | `lshift(R2, -2)` non-zero skips; below 4: table read and `JUMP (M13, I12)` at 0x1c7521 | 0x25f7b0 | 4 | no | **[O]** `R2` is a word of a record whose source was not traced; it is not shown to be the type |
| 8 | `FUN_1c24e9`, 0x1c26c5 and 0x1c26e2 (`M0 = frame word 0xb4 + 2t`, `S3 = DM(I4, M0)`, `I4 = 0x2567a0`) | indexes by the **next per-track word** (the ColdFire's second byte of the track record, finding 04), not by the type | 0x2567a0 | 8 words: 1 3 2 4 5 6 0 0 | no | the index is whatever the ColdFire stores in that byte for a run-time machine **[O]** (not read in the emulator, see section 6) |
| 9 | `core/dsp-voice` wrapper, `_dv_scan` | the registry scan (rows `{type, pc}`, 0 ends the table) | `_dv_table` | the rows `core/dsp-voice` and `core/dyndsp` define (8 free rows `0xffffffff`, then an empty row) | yes, this is the extension | a matching row is a dispatch; no match and type 7 or more is stage 3 and the stock path |

The sites run before the wrapper's hook (1 to 3, the frame handler and the parameter converter) read the type but do not branch on it beyond the
selector of site 3, and every consumer of the selector (4 to 6) guards against a value of 7 or more. A run-time type therefore never reaches a
stock jump table. This agrees with finding 06 ("Selector collision") and adds the guards for sites 5 and 6 and the full list of words an added
type reads at site 3.

## 3. The emulator with unwritten memory filled **[D]**

`SHARC_UNINIT=<hex seed>` (default off; `packages/emulator/sharc-rt/src/rt/bnd.rs`, `uninit_value`): a read of a plain-RAM byte that neither the boot
stream nor a write has set returns a seeded non-zero pattern instead of zero (little-endian per byte, so any width and alignment agrees, one
function of the byte address and the seed). The emulator's generated cores are built with `explicit_memory_model = false`, where such a read gave an
*unknown* value, not zero (`_dm_read_full`); the setting covers both cases. `SHARC_UNINIT_LOG=FILE` appends the first 24 and every power-of-two
read (`pid N read #K of W byte(s) at ADDR`). Render cache: `SHARC_UNINIT` stays in the key (the `SHARC_` prefix is kept, so a pattern run never hits
a zero run), `SHARC_UNINIT_LOG` is on the denylist of `tools/devtools/render_cache.py` (output only). Scenario `rung-c-uninit`
(`tests/digikit/scenarios/rung-c-uninit.toml`, `[rungc] uninit = "5a17"`) runs rung C with it.

- **Result: rung C passes with the pattern on** (`rung-c` and `rung-c-uninit`, DT2 1.17, full tier, about 4 minutes each): type 18 on track 1, 19,574
  dispatches and as many returns, the note equals the patched-in pluck (0 samples differ), heartbeats rise, no `DSP_STOPPED`. It does **not** halt
  the way the device did.
- **What it executes: almost nothing reads unwritten memory.** In the whole run each emulator process made fewer than 32 such reads: four
  one-byte reads at the starts of patched code blocks in L2 (0x2001ab80, 0x2009afd8, 0x2009afe8, 0x2009b5e0) and five two-byte reads at
  0x8422b5c8 to 0x8422b5d0 (SDRAM, a sample area). Nothing in the type path reads memory that no block loaded or the code did not write first: the
  firmware's tables and its zero-filled areas are blocks of the boot stream, and none of the wrapper's or the link's data words (`_dv_*`, `_dl_*`)
  was read before it was written, so the emulator's zeros there are loaded or written zeros, which the device's boot ROM loads as well. **The hypothesis "the device reads uninitialised SRAM where the emulator reads zero"
  is not supported for the DSP**; it stays open only for SDRAM and L2 words *outside* the stream, of which there are about nine reads, none in
  the type path. **[O]** The ColdFire's own RAM (SDRAM) was not given the same treatment; the ColdFire writes the frame the DSP reads (section 6).
- With the setting off the read path returns what it did (zero with the explicit model, unknown without). `verify --only build,pins --dt2` (seven DN2 PCM pins),
  `dt2-tone` and `dt2-check` (DT2 scenario oracles) pass on the edited tree. The `bnd.rs` edit rebuilds the generated core (about 2 minutes here, warm).

## 4. Hardware PC and loop stack depth in the emulator **[D]**

A candidate on silicon but not in the emulator: the wrapper is the first code in this context that executes `CALL`/`RTS` (`_dv_crumb`, and the
routines' `CALL _dv_exp2`) and a `DO` loop (the odd-voice copy); the model's PC stack holds 256 entries, the SHARC+ core's much fewer. `SHARC_PCDUMP=0`
now also records the deepest PC, loop, status and call stack of any interpreted step and the pc where each was reached (a `#stack-max` line;
`{pid}` in `SHARC_PCDUMP_OUT` becomes the process id, the rows carry the two depths, `docs/INSTRUMENTATION.md`).

DN2 1.12 (`equiv-dyn-pluck`, quick tier; interpreted steps are the patched code and the parts the model does not compile): PC stack at most 2 to 3,
loop stack at most 1 to 2, status stack 1, call stack 1. DT2 1.17 (`equiv-dyn-pluck` and the full `rung-c`, where the registered path and the
pluck routine run for 19,574 dispatches): `#stack-max pc_stack=2 loops=2 status=1 call=1`, the deepest steps at stock addresses (0x1c0bba, 0x1c2973).
So the wrapper and the routine add nothing to the stock maximum, far below the hardware's depths. **A PC-stack or loop-stack overflow is not
the cause** unless the device's firmware context carries a much deeper stack than the emulator's, which nothing in the model suggests. The
measure covers interpreted steps only (the patched code always runs interpreted here, the stock code that runs as compiled blocks does not
report its depth). **[O]** the hardware depths were not looked up in the public manual in this lane.

## 5. What the wrapper does between its entry and the stage-2 write **[D]** (reading of `dsp-voice.s`)

For a registered type: the scan (about 12 instructions), `MODIFY(I7, -40) (NW)`, 20 register stores at `DM(-6 .. -25, I6)`, 13 stores of
the context registers (`DM(-26 .. -38, I6)`), six `M` register loads, `L0-L3, L5 = 0`, `BIT CLR MODE1 0x20A000`, `BIT SET MODE1 0x10000`,
`NOP`, the breadcrumb word, then `CALL _dv_crumb` (stage 2). The same `CALL _dv_crumb` is stage 3 for an unregistered type of 7 or more; that is
the case at a reboot with track 1 still on the added type and no module loaded yet, and Em saw the DSP dead then too (finding 47). So the
common part of all failing cases is: type 7 or more, and the first `CALL` of the wrapper. The common part of all working cases (hook only,
stock voices) is that the wrapper takes the stock path, which has no `CALL`, no `MODE1` write and no stores below `I6 - 6`.

## 6. Candidates, ranked, and the experiments that separate them **[O]**

1. **The stock frame handling for a type of 7 or more, before any voice code (sites 1 to 3 and 8).** The device halts at the moment the type
   appears in a frame (finding 47: selection, not the trig). Nothing in sites 1 to 3 differs between device and emulator in code or constant
   data. What can differ is a value the ColdFire puts in the frame for a run-time machine: the next per-track word 0xb4 + 2t (site 8, an index
   into an 8-word table) and the 0x60-stride machine-parameter block (0xda + 0x60t). The emulator reads them from its ColdFire's RAM, which is
   zero where the firmware did not write it; the device's SDRAM is not. Cheapest check: in the rung C flow, read frame words 0x94, 0xb4 and
   the first words of 0xda + 0x60 for track 1 from the DSP (the type cache and mirror, base 0x255970 in the DSP's own addressing; my `dsp_peek`
   attempts through the daemon returned zeros, so the address form for an L1 word still needs to be found) and look whether the byte after the
   type is a clean 0 or 1. Then fill the ColdFire's unwritten SDRAM the same way as `SHARC_UNINIT` does for the DSP and rerun.
2. **The wrapper's first `CALL`/`RTS` (and the loop) in the voice context.** The emulator's PC stack depth is small (section 4), so a stack
   overflow is unlikely; the other `CALL` hazards on silicon (a pending interrupt, the VISA return address alignment of code in L2, a conditional
   `JUMP` directly before `RTS` in `_dv_crumb`) are not visible in the model. Experiment: a wrapper build whose breadcrumb is expanded inline at the
   two stages that matter (no `CALL`, no `RTS`), and whose registered path does not write `MODE1` (the bits are already clear or set in the
   context: PEYEN is 0 in the measured MODE1 0x39011cf8). If the halt stays, this candidate is gone.
3. **A fault outside our code on a stock pipeline step for a run-time type, found by bisecting with breadcrumbs placed in the stock path.** Only
   this gives the answer the log cannot: hook the change detector (0x1c33d7), the end of `FUN_1c2b24`, and the entry of `FUN_1c24e9` with the
   existing straight-into-the-record breadcrumb write. The record shows then in which of the four steps of the first type-18 block the core
   stopped. This needs three more hook sites in the patch plan and the pins of the stock path to stay equal (no stock behaviour changes).

A device test that needs no new code and separates 1 from 2: the rung C image **without** `core/dsp-voice`'s registered path, i.e. a ColdFire-only
machine of type 18 (a typed machine and parameter rows, no DSP routine) selected on a track. The finding-45 report lists the framework-only image
as working, but not with an added type selected. If that halts, the cause is in the stock pipeline and the wrapper is cleared.

## 7. Files

- `packages/emulator/sharc-rt/src/rt/bnd.rs`: `SHARC_UNINIT`, `SHARC_UNINIT_LOG`.
- `packages/emulator/sharc/src/pc_dump.rs`: stack depth columns and the `#stack-max` line, `{pid}` in the output path.
- `tools/devtools/render_cache.py`: `SHARC_UNINIT_LOG` on the denylist.
- `tools/patchkit/pk_digikit_rungc.py`, `tests/digikit/scenarios/rung-c-uninit.toml`: the scenario with the pattern on.
- `docs/INSTRUMENTATION.md`: the two settings.
- Pre-existing and unrelated: `tests/devtools/test_render_inputs.py::test_runtime_file_reads_are_reviewed` fails in this tree on file-read counts
  of `plusdrive`/`patchkit` (untracked `packages/desktop`, `packages/web` are present); the environment-variable checks of that file pass.

## 8. Follow-up: the type >= 7 path against the public manuals, and the bisect build (Lane b, 2026-10-18, later)

Question: candidate 2 of section 6 (the wrapper's own path for types of 7 or more). Static check against the public SHARC+ manual (`out/refs/sharc-plus-prm`, "PRM"
below; page = printed page, file `pages/pNNNN.txt`; the hardware reference `sc58x-2158x-prm` has nothing on these rules), the emulator's handling of each rule,
then a bisect build that removes the path and its proof. Marks: **[D]** read or measured once here, **[O]** open; nothing is **[V]**.

### 8.1 The context the wrapper runs in **[D]**

MODE1 at the hook is `0x39011cf8` (finding 45; the device log of rung B shows the same value): decoded with PRM Table 28-19 (28-48 to 28-51, files p0613 to p0616)
it is CBUFEN (24), IPERREN, DPERREN, SPERREN (29, 28, 27), RND32 (16), IRPTEN (12), NESTM (11) and all six register-bank selects (SRRFL 10, SRRFH 7, SRD2L 6, SRD2H 5,
SRD1L 4, SRD1H 3). PEYEN (21), TRUNCATE (15), ALUSAT (13), SSE (14), BR0, BR8 are 0. So: the voice code runs on the secondary register files of both DAGs and
the data file; interrupts are enabled and nest (a higher-priority interrupt may run between any two wrapper instructions, except inside a delayed branch and its two
slots, PRM 4-18); the interrupt entry pushes MODE1 on the status stack and RTI restores it (PRM 4-9, 4-73, 28-45), and `CALL`/`RTS` never touch the status stack.
**The four bit writes of the wrapper (`BIT CLR MODE1 0x20A000`, `BIT SET MODE1 0x10000`) and the restore (`MODE1 = R0`) write the value MODE1 already has**: PEYEN,
TRUNCATE and ALUSAT are already 0, RND32 already 1, the bank selects are not in the masks and the restore writes back the saved full word. On the measured context they
change no bit. They would matter only in a context whose MODE1 differs (finding 45 draws such contexts in the unit rig; none was measured on a device).

### 8.2 Each instruction of the type >= 7 path and the rule it could break **[D]**

"Stage 3" is the path of an added type that no module registered (the case of a reboot with the track still on the type and no module loaded): the common prefix of
all failing cases. "Registered" is the further path of a type in the table.

| path | instructions | rule (PRM page) | could it break it? | emulator |
| --- | --- | --- | --- | --- |
| stage 3 | `DM(_dv_sv3) = R3`, `DM(_dv_sv5) = R5`, `DM(_dv_bcw) = R0`, ALU on R0 to R3 | none | no; the same kinds of access run on the stock-type path (`_dv_sv_l4`, `_dv_diag`) on the device | yes (functional) |
| stage 3 | `CALL _dv_crumb` (non-delayed) | PC stack is 30 deep, the almost-full interrupt SOVFI fires at 29 entries (4-8, 4-9, p0111, p0112); a non-delayed branch flushes the 11-stage pipeline (4-17 to 4-18); branch predictor: after every branch at least twelve valid 48-bit instructions that do not cross a memory boundary, and an RTS in the pipeline with a CALL or RTS is not predicted (4-13, 4-14, p0116, p0117) | depth: measured 2 to 3 in the emulator against 30 allowed (section 4), so no; the twelve instructions after the `CALL` (the stock jump follows after 6, then `_dv_crumb`) and after the `RTS` (`_dv_exp2`, 21 instructions) are code, the block boundary was not checked **[O]**; the other rules cannot be broken by the sequence | stack push/pop yes (the reference core has 30 entries and raises on overflow; the runtime core has 256 and never raises SOVFI); predictor, pipeline and stalls no |
| stage 3 | `_dv_crumb`: `I4 = _dv_diag` then `DM(12, I4) = R3`; `I4 = _dv_rec`, `R5 = DM(0, I4)`, `R5 = PASS R5`, `IF EQ JUMP`; `I4 = R5`, `DM(192, I4) = R3` ... | an index register loaded by the previous instruction stalls the next access, handled by the hardware (4-64, p0167); a store to an address outside memory raises ILADI (6-31, p0215) | the stall is interlocked; the pointers `_dv_rec` hold are the reply records at 0x266220 and 0x267220 (L1, finding 48) or 0 (skipped) | yes; the first store of this core into the reply record (outside the block tail) has run on no device **[O]** |
| stage 3 | `RTS` (non-delayed), reached by `IF EQ JUMP _dv_cc` | same as `CALL` | see above | yes |
| registered | `MODIFY(I7, -40) (NW)` and the 33 stores `DM(-6 .. -38, I6)` | I7 is a circular-buffer register: a modify that crosses B7 wraps by L7, and L7 = 0 with CBUFEN would raise CB7I at every change (6-30, 6-31, p0214, p0215) | no: B7 = 0x2ca5a0 (L7 = 0x3e2) is 0xd20 words below I7 = 0x2cb2c0; the stores stay within the 40 words reserved (I6 - 48 .. I6) | yes (no circular wrap in range) |
| registered | `M5 = 0`, `M6 = 1`, `M7 = -1`, `M13 = 0`, `M14 = 1`, `M15 = -1`, `L0-L3, L5 = 0` | register loads used by the next instruction stall (4-64); CBUFEN is on, an L of 0 is a linear register | no | yes |
| registered | `BIT CLR MODE1`, `BIT SET MODE1`, `NOP` | a MODE1 write takes effect one instruction later (alternate register sets: 6-29, p0213; 2-7, p0057; a `NOP` follows a MODE1 write in the manual's examples, 6-21, p0205); IRPTEN and NESTM up to one cycle (p0178) | two read-modify-writes in a row: if the second read the old MODE1 it would write the old bits back and undo the first; harmless on the measured MODE1 (8.1). An interrupt between them is harmless (RTI restores MODE1) | bank-select bits: yes, the change is visible after the following instruction (`bank_complete`, `rt.rs`, `state.py`); all other MODE1 bits take effect at once, so a missing `NOP` would not show |
| registered | `LCNTR = 32, DO _dv_copy UNTIL LCE` (two-instruction body), then `R3 = 6`, `CALL _dv_crumb` | loop and PC stacks are used together; a `CALL` in the last five instructions of a loop needs care (4-41, p0144); no change of flow in the body keeps it an F1-active loop (p0140) | no: the `CALL` is after the loop end, not inside; loop depth measured 2 | yes (stack depths measured) |
| registered | `JUMP (M13, I12)`, `JUMP (M13, I13)`, `JUMP (M14, I12) (DB)` with `R3 = DM(-6, I6)` and `RFRAME` in the slots | a delayed branch's two slots must not hold a branch or a PC-stack operation and run uninterrupted (4-18, 4-19, p0121, p0122) | no: the slots are a load and `RFRAME`, copied from the stock epilogue; the indirect jump through M13 exists 67 times in the stock image (finding 45) | the slots run; a forbidden slot is not diagnosed **[O]** |
| registered | `MODE1 = R0` after `R0 = DM(-38, I6)` | as the MODE1 writes above; R0 is read from the secondary file before and after (the bank bits are unchanged) | no | yes |

An observation outside the PRM rules **[D]** (`sharcdb`, dt2-1.17, table `edges`): the stock DSP image has 1,714 delayed and only 4 non-delayed `CALL`s (two in the
context-switch code at 0xb8822a and 0xb88531, two in the startup routine at 0x1c0f91 and 0x1c1417), and **1,001 returns, all `RTS (DB)`: not one non-delayed `RTS`**.
The stock renderer behind the hook has 3 delayed `CALL`s and 2 delayed returns. The wrapper's `CALL` and `RTS` are the non-delayed forms; the firmware never
runs a non-delayed `RTS`. They are valid instructions and no PRM rule is broken, but it is the one thing about the path that no stock code in this context does.

### 8.3 What the emulator models and what it does not **[D]**

Modelled: the one-instruction latency of the MODE1 register-bank selects and of PCSTKP writes; the PC, loop and status stacks (reference core: 30 entries, raising
on overflow; runtime core: 256, no SOVFI); delayed-branch slots as executed instructions; software interrupts as functional scheduling. Not modelled: pipeline timing
and stalls, the branch predictor, the L1 instruction and data caches (the stock firmware drives them: 0x1c0035 and 0x1c0923 walk SHL1C_CFG 0x3e000/0x3e002 and the
range registers), posted writes and `SYNC` (PRM 9-3), preemption of the wrapper by a nested interrupt at an arbitrary instruction, MODE1 effect latency of the
non-bank bits, and the checks on delay-slot contents. Every rule of 8.2 that is open is in that second list.

### 8.4 What the audit says about the cause

1. The **stage 3 path has no MODE1 write, no stores below the frame, no loop, no indirect jump**, and Em reported the DSP dead in that state (a reboot with the track
   still on type 18 and no module loaded; this report is not in a saved log, finding 47 does not carry it **[O]**). If that report holds, the registered-only rules
   (MODE1, I7, loop, delayed jump) cannot be the whole cause, and what is left of the wrapper is the non-delayed `CALL`/`RTS` pair and the stores into the reply
   records. If it does not hold, the registered path is in play again and so is every row of 8.2.
2. No row of 8.2 breaks a rule of the manual on the measured context. The two things the manual leaves to the program, the predictor distance (twelve valid
   instructions after a branch, inside one memory block) and the double MODE1 read-modify-write, are met or harmless on what was measured.
3. The bisect below removes the `CALL`/`RTS` pair, the reply-record stores through a pointer and every registered step from the path at once; the
   record stores can be put back later separately.

### 8.5 The bisect build **[D]**

- **Option, not fork.** `core/dsp-voice/src/dsp/dsp-voice.s` has two `#ifdef DV_BISECT_C2` blocks; `tools/patchkit/pk_build.py` gained `#ifdef`/`#ifndef`/`#else`/`#endif`
  (`apply_conditionals`; `--define NAME --out DIR` writes the objects of a variant to DIR and never over the committed ones). With the option off the text the
  assembler sees is the old text, so the committed objects are byte-identical: `pk_build build modules/core/adapters/dsp-voice --check` says current for both devices
  (the DN2 object is unchanged and does not use the option).
- **What the variant does.** For a registered type: `JUMP _dv_stock` right after the scan (no registered path at all). For a type of 7 or more: the stage 3 breadcrumb is
  written inline (the body of `_dv_crumb` without its `RTS`, two conditional jumps, no `CALL`), and the dispatch counter (`_dv_diag + 52`) counts the stage 3 passes so
  that the device log can show them (the last voice of a block overwrites the breadcrumb word with its stage 1, the counter does not get overwritten). The stock-type path
  (types below 7) is untouched; it keeps its L4 write and `MODIFY(I7, -26)`, which rung B ran on the device. The variant executes no `CALL`, no `RTS`, no `MODE1`
  or other system-register write, no store below the frame and no loop.
- **Tooling.** `tools/patchkit/pk_variant.py make MODULE OPTION LABEL` builds the variant into `out/variants/LABEL/` and prints that directory, a repository root (devices and
  firmware linked, modules linked but for the variant); `DIGI_PK_ROOT=<it>` makes `mise run patch|preflight|pack` and the proof rigs (`pk_dkrt.Tools`) use it. The
  scenario `rung-c-bisect` (`tests/digikit/scenarios/rung-c-bisect.toml`, `[rungc] variant` and `defines`) runs the rung C flow on it.
- **Proof 1, stock PCM unchanged.** `pk_proof_voice --patched out/patch/c2-voice-dt2.syx --only type0,sine60 --quick` (the variant with `core/dsp-voice,test/sine-voice`): part
  `type0` PASS (patched-type0 bit-identical to stock). Part `sine60` FAILS as it must (6.9 Hz instead of 261 Hz: the forced type 8 is registered by `test/sine-voice` and the
  variant never dispatches), which shows that the measurement saw the variant. Unit rig: `tests/patchkit/test_dsp_voice.py::test_the_bisect_build_takes_the_stock_path_for_added_types_with_no_call`
  (a breakpoint on `_dv_crumb` is never hit, the stage 3 words equal the stock build's, a registered type is not dispatched); `123 passed` for the file on the edited tree.
- **Proof 2, the rung C flow.** Scenario `rung-c-bisect` on the device image (the report carries sha256 `3a34dcf6...`, the image of 8.6): PASS in 228 s (full tier): the upload,
  the menu and the restart as for rung C; the run-time pluck chosen on track 1 (type 18); then **39,148 stage 3 passes in 19,877 blocks (1.97 a block, 2 voices; the 1.5 % shortfall is
  the 300 blocks between the sample and the TRIG press), 0 returns, the heartbeat rises (37,823 to 57,700), no `DSP_STOPPED`**. Two earlier runs of the same scenario
  helped to fix the check: the first read the last breadcrumb (always voice 31, stage 1) and so could not show stage 3; the second compared against exactly 2 passes a block.
  **A difference to the device [D]:** in the emulator the passes begin at the TRIG press, not at the selection (`dispatches` is 0 at 500 ms after the selection and the count
  matches a start at the trig to within 6 passes). The ColdFire puts the added type into the frame the DSP reads only for a track that plays. On the device the halt
  of rung C came with the selection and the held TRIG together, and the reboot case needs no TRIG at all, so in some state the device's frame carries the type earlier
  than the emulator's: that is candidate 1 of section 6 seen from the other side **[O]** (a boot with a project that has track 1 on type 18 and a look at the frame word before any trig).

### 8.6 The device image and how to read the result

- Image `out/patch/rungC2-dt2-1.17.syx` (not committed): 1,885,472 bytes, sha256 `3a34dcf60697a73fce28c67d476be3b6e421e1d4ea08ddc8f9496e1aa3350174`.
  `mise run preflight` ends with `PREFLIGHT OK` (sections 3 and 7 differ from stock, 2, 4, 5, 8 identical, checksum, HMAC, limits, base sha256 `26c22f66...`).
  The rung C image of the same tree: `ac72f220f3c2f39b22566ec9b23e5884214f4ebebdabc6ddd676d84e1c556f1a` (1,885,344 bytes). The module file is the same `dyn-pluck-dt2.dkm`.
- Procedure and reading: docs/PATCHING.md "Rung C2 (bisect)". No halt: the wrapper's call path was the cause and the stock pipeline accepts type 18. Halt (with
  `dispatches` frozen at N, or 0): the stock pipeline stops on the type with the wrapper's path out of the way; candidates 1 and 3 of section 6 are next.

### 8.7 Open items

1. Em's report of the dead DSP at a reboot with the track on type 18 and no module has no saved log; if the C2 image does not halt, repeat that reboot case with the
   C image and keep the log: it is the cleanest pair (same state, `CALL` path against inline path).
2. A C3 variant with delayed `CALL`/`RTS (DB)` (two `NOP` slots) in `_dv_crumb` would test the "firmware never runs a non-delayed `RTS`" observation if C2 does not halt and C does.
3. The reply-record stores through a pointer (`DM(192, I4)`) were never run by this core on a device outside the block tail; the C2 image keeps them, so a halt in C2 does not clear
   them. A C2b without them (breadcrumb in `_dv_diag` only) is a one-line change of the option.
4. The twelve-instruction predictor distance and its memory block were read, not measured (code at 0xbcd7f4 to 0xbcda60 in L1).
5. In the emulator the type reaches the DSP at the TRIG; on the device earlier. Find the state that makes the difference (8.5) before trusting any emulator pass for this halt.
6. Caches (L1 D-cache on the L2 variables, write-back by default, PRM Table 8-2, p0262) are not modelled; the reply records are L1 (not cached), the wrapper's variables at 0x2009b4d4 and up are L2.


## 9. Follow-up: the 69 instructions before stage 2, an interrupt model, and three variants for rung C3 (Lane b, 2026-10-18, third pass)

Question (after the device run of rung C2, finding 47): with the wrapper's call path out of the way the Digitakt II 1.17 keeps running with a track on type 18, so
the halt of rung C is inside `core/dsp-voice`'s path for a **registered** added type, before its stage-2 breadcrumb write. The emulator runs that path fine.
Emulator and static work only; nothing was sent to a device. Marks: **[D]** read or measured once here, **[O]** open; nothing is **[V]**.
**No cause was found.** This section lists the instructions, adds an emulator setting that delivers interrupts between any two of them (rung C still passes),
and gives three device images, each without one suspect, that also name the place of a halt that remains.

### 9.1 What the device fixes **[D]**

- Rung C2 (finding 47): type 18 reached the hook every block for 40 s, the stock pipeline accepted it, no halt. So frame handling, the change detector, the
  parameter converter and the voices 0 and 1 are cleared on silicon, and so are the scan of `_dv_table` and the inline stage 3 breadcrumb (they ran in C2).
- Rung C: the last breadcrumb stays at stage 1 and `dispatches` is 0 (finding 49 section 1). Stage 2 writes its word into `_dv_diag` and then into both reply
  records at its **first store**, so the core stopped **before the first store of the first stage 2 of voice 2**.
- That store is not the `RTS`. The `RTS` of `_dv_crumb` comes after all its stores, so a non-delayed `RTS` cannot explain a missing stage 2 word. **[C]** Finding 49
  section 8 named "the non-delayed `RTS`" as the one thing stock never runs; for the halt before stage 2 the suspect in that family is the **`CALL`** (and the
  crumb's own first instructions), the `RTS` is the next place a halt would show.

### 9.2 The instructions from the hook entry to the stage-2 store (DT2 1.17 build, object offsets in short words) **[D]**

100 instructions on the path of voice 2 (the scan counted once; +26 for voice 0, which also records the context registers). 31 of them (entry, stage 1, type read, scan: offsets 0 to 124 without
the voice-0 block) run for every voice on every rung, including A, B and C2. **The other 69 never ran on silicon before rung C**: offsets 125 to 245 (59 instructions) and the
first 10 of `_dv_crumb`. They are (an offset is the first short word, then the size in short words):

```text
 125  2  I12 = R2;
 127  3  MODIFY(I7, -40) (NW);
 130  2  DM(-6, I6) = R3;
 132  2  DM(-7, I6) = R5;
 134  2  DM(-8, I6) = R6;
 136  2  DM(-9, I6) = R7;
 138  2  DM(-10, I6) = R9;
 140  2  DM(-11, I6) = R10;
 142  2  DM(-12, I6) = R11;
 144  2  DM(-13, I6) = R13;
 146  2  DM(-14, I6) = R14;
 148  2  DM(-15, I6) = R15;
 150  2  DM(-16, I6) = I0;
 152  2  DM(-17, I6) = I1;
 154  2  DM(-18, I6) = I2;
 156  2  DM(-19, I6) = I3;
 158  2  DM(-20, I6) = I5;
 160  2  DM(-21, I6) = M1;
 162  2  DM(-22, I6) = M2;
 164  2  DM(-23, I6) = M3;
 166  2  DM(-24, I6) = B3;
 168  2  DM(-25, I6) = I13;
 170  2  DM(-26, I6) = M5;
 172  2  DM(-27, I6) = M6;
 174  2  DM(-28, I6) = M7;
 176  2  DM(-29, I6) = M13;
 178  2  DM(-30, I6) = M14;
 180  2  DM(-31, I6) = M15;
 182  2  DM(-32, I6) = L0;
 184  2  DM(-33, I6) = L1;
 186  2  DM(-34, I6) = L2;
 188  2  DM(-35, I6) = L3;
 190  3  R3 = DM(_dv_sv_l4);
 193  2  DM(-36, I6) = R3;
 195  2  DM(-37, I6) = L5;
 197  2  R3 = MODE1;
 199  2  DM(-38, I6) = R3;
 201  2  M5 = 0;
 203  2  M6 = 1;
 205  2  M7 = -1;
 207  2  M13 = 0;
 209  2  M14 = 1;
 211  2  M15 = -1;
 213  2  L0 = 0;
 215  2  L1 = 0;
 217  2  L2 = 0;
 219  2  L3 = 0;
 221  2  L5 = 0;
 223  3  BIT CLR MODE1 0x20A000;
 226  3  BIT SET MODE1 0x10000;
 229  1  NOP;
 230  2  R3 = 32;
 232  1  R3 = R3 - R14;
 233  3  R3 = LSHIFT R3 BY 8;
 236  3  R5 = LSHIFT R1 BY 16;
 239  1  R3 = R3 OR R5;
 240  3  DM(_dv_bcw) = R3;
 243  2  R3 = 2;
 245  3  CALL _dv_crumb;
      ...  CALL _dv_crumb enters _dv_crumb (short word 443)
 443  3  _dv_crumb      R5 = DM(_dv_bcw);
 446  1  R3 = R3 OR R5;
 447  3  I4 = _dv_diag;
 450  2  DM(12, I4) = R3;
 452  3  I4 = _dv_rec;
 455  2  R5 = DM(0, I4);
 457  1  R5 = PASS R5;
 458  3  IF EQ JUMP _dv_cb;
 461  2  I4 = R5;
 463  3  DM(192, I4) = R3;
```

What could differ on silicon, group by group (PRM = `out/refs/sharc-plus-prm`, page = printed page, file `pages/pNNNN.txt`):

| instructions | rule or hardware fact | the emulator | verdict |
| --- | --- | --- | --- |
| 125 `I12 = R2` | none | functional | no |
| 127 `MODIFY(I7, -40) (NW)` (stock: -26) | I7 is a circular register: a modify wraps by L7 and L7 = 0 with CBUFEN would raise CB7I at every change (6-31, p0215). Measured L7 = 0x3e2 words, B7 = 0x2ca5a0, I7 = 0x2cb2c0: I7 is 0xd20 bytes above B7, the modify is 160 bytes | wraps are modelled | no |
| 130-199 35 stores `DM(-6 .. -38, I6)`, `R3 = DM(_dv_sv_l4)`, `R3 = MODE1` | the same form (Type 15b, 8-bit signed offset) as the stock stores `DM(I6 - 26 .. -4)` (`9d0802e7` for -25 in the stock image, `9d0801fa` for -6 here). **No store at or below SP before SP moves**: the `MODIFY` is the second instruction of the path, the stores follow it, and the lowest word written is I6 - 38 words against the new I7 = I6 - 42 words. A nested interrupt cannot overwrite the frame: the kernel's context save (`blk69@0xb88200`) does `I7 = modify(I7, -0x81)` first and then writes `DM(I7 + n)`, all below the interrupted I7 | interrupts are not delivered at arbitrary points (9.3) | no |
| 201-221 11 register loads: M5, M6, M7, M13, M14, M15 = constants, L0-L3, L5 = 0 | a load of a register that the next instruction uses stalls, interlocked (4-64, p0167). All six register-bank selects are set (MODE1 0x39011cf8), so the writes go to the secondary set (6-29, p0213: a one cycle latency applies to a change of the selects, not to these writes). CBUFEN is on: a zero L register makes I0-I5 linear; only L7 = 0 and L15 = 0 are special (p0215). The values equal the measured context except L0-L5 (not measured) | bank model: yes | no, but see below |
| 223 `BIT CLR MODE1 0x20A000`, 226 `BIT SET MODE1 0x10000`, 229 `NOP` | identity on the measured MODE1 (finding 49 8.1). Two read-modify-writes of MODE1 in a row; the stock image follows each of its MODE1 writes with a `NOP` (`0x1c4f3e`, `0x1c4f48`). The latency of IRPTEN and NESTM is up to one cycle (4-75, p0178); an interrupt entry pushes MODE1 and RTI restores it | bank-select latency yes; the other bits take effect at once | not a rule broken; the only system-register write on the path and new on the device |
| 230-243 `R3 = 32 ... DM(_dv_bcw) = R3`, `R3 = 2` | an absolute store to L2 (write-back D-cache, 8-2, p0262, not modelled); the stock-type path and rung A/B store `_dv_diag + 48` the same way | no cache | no |
| 245 `CALL _dv_crumb` (48-bit, non-delayed, absolute) | non-delayed branch flush (4-17, 4-18); PC stack 30 deep, SOVFI at 29 (4-8, p0111): measured depth 2 to 3 in the emulator (section 4); the predictor wants at least twelve valid 48-bit instructions after a branch inside one memory block and, in VISA, no two branches in a 48-bit window (4-13, 4-14, p0116, p0117): the 69 instructions have **no** such pair (the scan has one pair, 118/122, and stage 5 has one, 290/293: both outside the 69 or run by every voice already). The stock image has 4 non-delayed `CALL`s in 1,718 (all in the context switch and the startup routine) | stack push yes, flush and predictor no | **the one control transfer of a form new to the device on this path** |
| 443-463 `_dv_crumb`: `R5 = DM(_dv_bcw)` (a load of the word stored 8 instructions earlier), `OR`, `I4 = _dv_diag`, `DM(12, I4) = R3`, `I4 = _dv_rec`, `R5 = DM(0, I4)`, `PASS`, `IF EQ JUMP`, `I4 = R5`, `DM(192, I4) = R3` | the same instructions as the inline body of C2 (which ran on the device) but for the load of `_dv_bcw` and the `CALL` that reaches them. The store at 463 is the stage-2 store | functional | the `CALL`, and the load of `_dv_bcw` |

Reading of the table. **Nothing in the 69 instructions breaks a rule of the manual on the measured context**, and no store goes below SP. What is left is not a rule but a
list of things that are new to silicon here and that the emulator cannot show: the non-delayed `CALL`, the back-to-back `MODE1` read-modify-writes, nested interrupts
inside a 160-byte-deep frame, the L0-L5 writes in a context whose L values were never measured, and plain code placement (the region 0xbcd871 to 0xbcd980 has never executed).
The three variants in 9.5 take the first three away one by one; the fine breadcrumbs (9.5) place a halt that remains within a few instructions.

**An unrelated defect found on the way, after stage 7 [D].** The epilogue restores `I3` (offset 156 in the saves) and **then** `B3`. A load of a B register also loads its I
register (PRM 6-24, p0208, "Circular Buffer Programming Model" step 2), so after the first registered voice returns, `I3` holds `B3`, not the caller's I3. The stock epilogue loads
`B3` first (`0x1c4f58`) and `I3` later (`0x1c4f7e`). The emulator does not model the I load (`tools/sharc_dev/sharc_core/state.py`, `_write_ureg`, writes only the named register), so no
emulator proof can see it. It cannot be the halt (it comes after stage 7), but it would corrupt the caller after a first successful dispatch. All C3 variants restore `B3` first
(`DV_C3_BFIRST`); the default build is unchanged until a device run says the order is the only change needed. **[O]** model the I load in the Python core and the runtime
(regenerates every core: `CORE_FINGERPRINT`), then the pins show whether stock code relies on the other order; test `test_the_c3_variants_restore_b3_before_i3` pins the source order meanwhile.

### 9.3 The emulator with interrupts between any two instructions **[D]**

What the coupled model delivers today: the core timer and the software interrupts are taken at instruction boundaries wherever the interpreter runs, the SEC line (peripheral
events) latches SECI at a boundary too, but the **events themselves** (the SPI2 frame exchange, the SPORT block, link port 0) are produced by the host between runs of the core, at a
fixed phase of the audio period (`sharc_peer.rs`, `exchange`, `render`). Where a render is running at such a point depends on one phase constant; a short routine is hit rarely, and
the same instruction is hit every block. On silicon the SPORT and SPI events come while the voices render.

`SHARC_IRQ_PROBE=LO-HI[,LO-HI...]:MASK[:EVERY]` (hex; `packages/emulator/sharc/src/irq_probe.rs`, `docs/INSTRUMENTATION.md`) latches the IRPTL bits MASK just before the interpreter executes an
instruction whose PC is in a window, once per visit of that PC (after the interrupt returns to the same PC it moves on), for every EVERY-th visit. The core's own entry rules decide
(IMASK, IRPTEN, NESTM, delayed-branch slots, loop ends) and the firmware's handler runs. Default off; with it off the engine does what it did (one `Option` test per step). The
patched code runs in the interpreter, so every boundary is visited. Scenario key `[rungc] irq_probe = "tick" | "sft3" | "both" | HEXMASK` (+ `irq_every`), scenario `rung-c-irq`:

- Windows: the registered path (from `_dv_scan + 15`, the `I12 = R2`, to `_dv_stock`) and `_dv_crumb` to `_dv_exp2`. The stock path runs for all 32 voices and would swamp the core
  (a first run with the whole wrapper never became ready). Mask `0x80400000`: TMZHI (the kernel tick) and SFT3I (the kernel's context switch, `0x1c0a70` and `blk69@0xb88200`). EVERY = 7, which rotates
  over all positions across blocks.
- Result, rung C image (`ac72f220...`), DT2 1.17, full tier, 287 s: **PASS**. In the emulator that held the note: **1,450,094 latches, 2,879,241 interrupts taken** (SECI 167,373, TMZHI
  1,318,740, SFT3I 1,393,128), the pluck's dispatches 19,574 and returns 19,574, heartbeats rise, no `DSP_STOPPED`, no stray write. The scenario fails if fewer than 10,000 interrupts were latched, so a pass cannot be a run in which the probe did nothing.
- **A kernel tick or a context switch between any two instructions of the registered path does not reproduce the halt.** **[O]** What the probe does not give: DMA or SEC interrupt handlers
  (a SECI needs a pending source in the SEC; the handler reads the SID), the 11-stage pipeline (flushes, the 12-instruction rule), the branch predictor, write buffers and caches, the
  parity checks of MODE1 bits 27 to 29 (IPERREN, DPERREN, SPERREN) on memory that was never written. The model that reproduces the halt does not exist yet.

### 9.4 Build options of `core/dsp-voice` for rung C3 **[D]**

All in the one source file (`#ifdef`, as `DV_BISECT_C2`); with no option the preprocessed text is byte-identical to the old (`pk_build build modules/core/adapters/dsp-voice --check` says current for both devices).

| option | what it does |
| --- | --- |
| `DV_C3_FINE` | three more breadcrumbs written **inline** (no `CALL`, same body as C2's) on the registered path: stage 8 `C3_TYPE_MATCHED` right after the scan matched, 9 `C3_FRAME_SAVED` after the 35 stores, 10 `C3_CONTEXT_PINNED` after the pins, before the `CALL` of stage 2. They go into `_dv_diag` and both reply records like stages 2 to 7, so a halt names its zone: last stage 1 = before offset 125 or in the first breadcrumb; 8 = `MODIFY`/stores; 9 = the pins; 10 = the `CALL` and the crumb's first instructions; 2 = past the `CALL`. `tools/device/dklog.py` names them; the ABOUT page shows them as `?` |
| `DV_C3_BFIRST` | restores `B3` before `I3` in the epilogue (9.2, last paragraph) |
| `DV_C3_DB` | every `CALL _dv_crumb` is `CALL (DB)` with two `NOP` slots and every `RTS` (the crumb's and the three helpers) is `RTS (DB)` with two `NOP` slots: the delayed forms the stock image uses (1,714 of its 1,718 calls, all 1,001 returns) |
| `DV_C3_NOIRQ` | clears MODE1.IRPTEN (0x1000) right after the first breadcrumb and puts the entry MODE1 (read before) back in the epilogue: no interrupt between the stores and the end of the voice |
| `DV_C3_NOSYS` | drops the `BIT CLR`/`BIT SET MODE1`/`NOP` and the epilogue's `MODE1 = R0`: no MODE1 write on the path (right on the measured context, where the three bits are already 0 and RND32 already 1; wrong in a context with other bits, so a measurement variant, not a fix) |

Variants (each with `DV_C3_FINE,DV_C3_BFIRST`): **a** + `DV_C3_DB`, **b** + `DV_C3_NOIRQ`, **c** + `DV_C3_NOSYS`.

### 9.5 The device images and their proofs **[D]**

Built with `tools/patchkit/pk_variant.py` and `DIGI_PK_ROOT` (docs/PATCHING.md "Rung C3"), the rung C module list, base `Digitakt_II_OS1.17.syx` (sha256 `26c22f66...`). All three: 1,885,472 bytes,
`PREFLIGHT OK`, sections 3 and 7 differ from stock and nothing else. The module is the same `dyn-pluck-dt2.dkm` (sha256 `e66c4660...`).

| variant | image (not committed) | sha256 |
| --- | --- | --- |
| a delayed CALL/RTS | `out/patch/rungC3a-dt2-1.17.syx` | `56d1f9945e09c2dab2980a0f80c1084efd86a941fc9e10d8b600fa137c353d55` |
| b interrupts masked | `out/patch/rungC3b-dt2-1.17.syx` | `5dd9e7a7c459f72e1550069bc66b55023c4e1a3ce39c12ab4f6ea59127834c4e` |
| c no MODE1 write | `out/patch/rungC3c-dt2-1.17.syx` | `aaa9f365b229a5a74c0eb5a605f30dd9246786211fa84d96a7ab6c6ffe00c906` |

Emulator proofs, each variant (DT2 1.17):

- Scenarios `rung-c3a`, `rung-c3b`, `rung-c3c` (full tier, 254 to 259 s, the images above): **PASS**. Upload, menu, restart, run-time pluck on track 1, 12 s note: 19,574 dispatches and as many returns,
  no `DSP_STOPPED`, and **the note equals the patched-in pluck of the same wrapper sample for sample** (96,000 samples, exact).
- `pk_proof_voice --only type0,sine60 --quick` on a `core/dsp-voice,test/sine-voice` image of each variant: PASS (patched type 0 bit-identical to stock; the sine at the right pitch).
- `tests/patchkit/test_dsp_voice.py`: 139 passed on the edited tree, with the new tests: the variants keep the registered path (even and odd voices, the callee-saved registers, the breadcrumbs 8, 9, 10 in order,
  then 7), b masks IRPTEN inside and restores MODE1 (hostile MODE1 included), c has no MODE1 write on the registered path, the `B3`/`I3` order.
- The ruff and mypy lists pass; `tests/devtools/test_render_inputs.py` still fails only on the file-read counts it failed on before (section 7).

### 9.6 Which to try first, and how to read the outcomes

Order **a, b, c**; probabilities are judgement, not measurement: a about 40 %, b about 25 %, c about 10 %, none of the three (the breadcrumb then names the zone) about 25 %. Why a first: the `CALL` is the
only control transfer of a new form on the path, the C2 run that passed differs from rung C by exactly the `CALL` (plus the registered prologue), and Em's report of a dead DSP after a reboot with the track on
type 18 and no module loaded (the stage 3 path, which has the `CALL` and none of the registered prologue) points at it too (not in a saved log **[O]**). It is also the only variant that needs no assumption about the context.
One device run per variant, hold TRIG 1 for 60 s, keep the log. All three carry the fine breadcrumbs, so a run that still halts is not wasted.

| run | outcome | reading | next |
| --- | --- | --- | --- |
| a | no halt, pluck sounds, `dispatches` = `returns` | the non-delayed `CALL`/`RTS` was the cause (or the layout it implies); keep the delayed forms. The pluck routine's own `CALL _dv_exp2` is still non-delayed and played | make `DV_C3_DB` the default after one more clean run |
| a | halts, last crumb stage 10 | the `CALL`, even delayed, or the first instructions of `_dv_crumb` (the load of `_dv_bcw`) | variant with the stage-2 breadcrumb inline (no `CALL`, no `_dv_bcw` load): the C2 body on the registered path |
| a | halts, last stage 9 | the pins: MODE1 or L/M writes | run c; if c halts at 9 too, the L0-L5 writes |
| a | halts, last stage 8 | the `MODIFY` or the 35 stores | run b (interrupts); then a variant with a smaller frame |
| a | halts, last stage 1 | before the first fine breadcrumb: the last scan branch, `I12 = R2`, or the breadcrumb 8 itself | layout or placement: move the wrapper (a padding option), not a suspect in this list |
| a halted, b | no halt | a nested interrupt landed in the registered path (9.3 found none in the model) | keep IRPTEN masked or find the handler that overwrites |
| a, b halted, c | no halt | the MODE1 writes | drop them, pin the three bits only when the entry MODE1 differs (`dl_diag` shows the device's: `0x39011cf8`) |
| a, b, c halted | same stage each time | the cause is in the zone the stage names; next the variants for that zone (inline crumb, smaller frame, no L writes) | |

A pass in any variant can be the removed suspect or the changed layout; if a variant passes, the cheap control is the same variant with the option replaced by `NOP`s of the same size.
Flash back with the stock file after a halt; **set track 1 to a stock machine before the next boot** (a project that holds type 18 on a track halts the DSP again when the module is loaded, finding 47).

### 9.7 Open items

1. The model that reproduces the halt does not exist. Next candidates for it: the SPORT/SPI events delivered at a configurable fine phase inside the render (`rx_phase` exists; a second knob for the SPORT block), parity errors on never-written memory
   (`SHARC_UNINIT` covers data reads; instruction fetch is not covered), the branch-predictor rules.
2. `B`-register loads do not load the I register in the emulator (9.2): model it, or keep the source-order test.
3. The default `core/dsp-voice` still has the `B3`/`I3` order of 9.2 and the non-delayed forms; decide after the device runs.
4. `dl_diag`'s L registers: `L0`-`L5` of the voice context were never measured (only L7); the C3 images do not add them. A one-line addition to `_dv_diag` would.
5. The same wrapper logic for the Digitone II (`dsp-voice-dn2.s`) has no `B3` save, no breadcrumbs and no C3 options; not looked at further.
6. ABOUT shows stages 8 to 10 as `?` (the menu's text table is in the runtime; the log names them).

## 10. The cause candidate in the frame save: 17 stores the ISA does not have (Lane b, 2026-10-19)

Question (after the device run of rung C3a, finding 47: the SHARC stops after fine stage 8 and before stage 9): which instruction of `MODIFY(I7, -40)`, the 35 frame stores,
`R3 = DM(_dv_sv_l4)`, `R3 = MODE1` stops it. Static and emulator work only; nothing was sent to a device. Marks: **[D]** read or measured once here, **[O]** open; 10.1-10.3 are **[V]** (second agent, 2026-10-19).
**Result: 17 of the 33 stores are not instructions of the ISA.** The assembler and the emulator accept them; the stock image has none. This is a strong candidate, not a proof (10.5).

### 10.1 The rule **[V]**

A store through an index register (Types 15a, 15b, 3a, 3b, 3d) cannot take a register of the same DAG as its source: through DM (a DAG1 index) not a DAG1
register, through PM not a DAG2 register. The direct-address form Type 14a (`DM(<addr32>) = Ureg`, file `p0382`) takes the full `UREG` class and is exempt; the stock image
has 304 such DM stores of DAG1 registers, which are legal. (Second-agent check, 2026-10-19: the encode tables and the quoted text, pages as below; the post-modify load of a
same-DAG register, Type 3a `UREGXDAG1 = dm(I1,M1)`, is also excluded, so "loads are allowed" holds for the pre-modify and immediate forms only.) The manual encodes the source of such a store as a register
class that leaves the same-DAG registers out, and says it in words:

- Type 15a (`DM(<data32>,Ia) = Ureg`), PRM p.15-7 (file `p0387`): the encode table lists the DM store as `dm(imm32, I1REG) = UREGXDAG1` and the PM store as `pm(imm32, I2REG) = UREGXDAG2`;
  the loads use the full `UREG` class. Type 15b (`<data7>`, the form of `DM(-16, I6) = ...`), p.15-10 (`p0390`): the same four lines with `imm7visa`.
- The text, p.15-8 (`p0388`) and p.15-11 (`p0391`): "The Ureg may not be from the same DAG (that is, DAG1 or DAG2) as Ia/Mb or Ic/Md." Types 3a and 3b say the same, p.13-13 (`p0313`), p.13-16 (`p0316`).
- `DM(I6 - n)` addresses through I6, a DAG1 register, so its source may be anything but I0-I7, M0-M7, L0-L7, B0-B7. Loads of those registers are allowed (the stall rule of Table 4-44, `p0172`, applies to them).
- What the silicon does with the excluded encodings is not in the public manual. The manual lists an illegal-opcode interrupt (ILOPI, level 4, Table 4-45, `p0172`; "Illegal Opcode Error Detection for Instruction Fetch", 9-2, `p0278`),
  which is what a register code outside the class would raise if the core checks it. **[O]** which of the three it is (ILOPI, a silent wrong store, a hang) is not decided here; all three stop a voice.

Build-time checks (`packages/patchkit/src/build/sharc_legal.rs`, 2026-10-30; `patchkit build` and `patchkit isa-scan` run them) **[D]**: the store rule above, and the post-modify load of a same-DAG register (Types 3a, 3b, 3d with `u` = 1, `d` = 0: the encode tables list the
destination as `UREGXDAG1` / `UREGXDAG2`, PRM p.13-15, 13-18, 13-24; the pre-modify load, `u` = 0, lists the full `UREG` class). Zero hits on the DT2 1.17 and DN2 1.12 DSP images and on every SHARC module source and core object of the repository. The DM/PM
index pairing (I0-I7 with DM, I8-I15 with PM) is not a rule: the I and M fields are 3 bits wide and `g` picks the DAG, so a mismatch has no encoding.

### 10.2 What the stock image does **[V]** (index-register forms)

Decoded stock instructions (`sharcdb`, `dt2-1.17`, forms 15a, 15b, 3b, confident, aligned), the register in the store's `ureg` field and the `g` (PM) / `d` (store) bits:

| instruction | count in the DT2 1.17 DSP image |
| --- | ---: |
| DM store of a DAG2 register (`DM(I6 - 3) = I12`, `DM(I5 + 13) = M13`, `DM(I6 - 23) = L8`) | 1,573 |
| **DM store of a DAG1 register through an index register** (15a, 15b, 3a, 3b, 3d) | **0** |
| PM store of a DAG1 register (`PM(I12 + 0x72) = L0`, the kernel's context save) | 32 |
| PM store of a DAG2 register | 1, at 0x16b8fe, inside a data table (offset `0xb8bea3`) |
| DM loads into DAG1 registers (`M3 = DM(I6 - 6)`) | 2,101 + 13 + 272 |

So the stock code never stores M0-M7, I0-I7, L0-L7 or B0-B7 with a DM store, and where it must save them it goes through a data register: the render function `FUN_1c4ecf` has
`R2 = M1; DM(I6 - 16) = R2`, `R2 = B3; ...`, `R2 = I0; ...`, and the kernel's context save (`FUN_1c02a4`, 0x1c03ed to 0x1c040f) has `R0 = B4`, `R0 = L0 ... R0 = L5` each followed by `DM(I6 - n) = R0`,
while its DAG2 registers go straight (`DM(I6 - 23) = L8` ... `DM(I6 - 16) = L15`). The DN2 1.12 image has 105 DM stores of DAG1 registers by the same decoder, all at 0xb8f6c9 to 0xb97752, outside every
function of the database (data decoded as code, offsets such as `-0x578f5835`); none in code. Both images follow the rule everywhere the decoder is confident.

### 10.3 The 17 stores in the registered path **[V]**

Offsets of finding 9.2 (short words) in the default object (built without `DV_C3_FINE`; the FINE build's offsets are shifted by its breadcrumb blocks). Everything else on the path is legal: the ten R-register stores (130-148), `I13` (168) and `M13`-`M15` (176-180), the loads and `R3 = MODE1`.

| offset | instruction | class |
| --- | --- | --- |
| 150-158 | `DM(-16 .. -20, I6) = I0, I1, I2, I3, I5` | DAG1 |
| 160-164 | `DM(-21 .. -23, I6) = M1, M2, M3` | DAG1 |
| 166 | `DM(-24, I6) = B3` | DAG1 |
| 170-174 | `DM(-26 .. -28, I6) = M5, M6, M7` | DAG1 |
| 182-188 | `DM(-32 .. -35, I6) = L0, L1, L2, L3` | DAG1 |
| 195 | `DM(-37, I6) = L5` | DAG1 |

The first is at offset 150, `DM(-16, I6) = I0`, the 12th instruction after the stage-8 breadcrumb in the FINE build's source order (the `MODIFY` and ten legal stores come first): **inside the zone between stage 8 and stage 9, where rung C3a stopped**. The build now
says so: `pk_build` prints `WARNING ... 17 store(s) the ISA does not allow` for the default `core/dsp-voice` DT2 object and for the DN2 object (`pk_sharc.ureg_class_problems`, called in `verify` for every instruction;
the committed objects are unchanged). The epilogue only loads these registers, which is legal.

### 10.4 The suspects, ranked **[D]**

1. **The 17 DAG1-register stores** (150-195; first `DM(-16, I6) = I0`). The ISA has no such instruction (10.1), the stock image has none (10.2), nothing else in the zone differs from what stock does, and the halt zone starts 11 instructions before the first and holds all of them. Not seen by the emulator, whose decoder and store path take any register.
2. **The head of the stage-9 breadcrumb** (11 instructions up to its first record store). Same code as stage 8's, which ran, with the same registers (R3, R5, I4) and a load of `_dv_bcw`; only the context differs (the 35
   stores are done). Low, but it is the other thing inside the measured zone (a halt before `DM(192, I4) = R3` of stage 9 shows as stage 8).
3. **`R3 = MODE1`** (197). A read of a system register; the entry code reads it into R1 at voice 0 and runs (the diagnostic word 11). No hazard in the manual. Very low.
4. **`MODIFY(I7, -40) (NW)`** (127). I7 = 0x2cb2c0 is *outside* the circular buffer: B7 + L7 = 0x2ca5a0 + 0x3e2 = 0x2ca982, so I7 is 0x93e words above its end (finding 9.2 and 8.2 called it "inside" by comparing with B7 only).
   The wraparound equations (PRM p.6-25, `p0209`) wrap a negative modify only when `I + M < B`; here the result 0x2cb298 is far above B7, so there is no wrap and no CB7I (p.6-30/6-31). The stock renderer does `MODIFY(I7, -26)` in the
   same context, with the same I7; only the constant differs. The positive stock modifies of I7 (+2, +4, +6: 219 in the image) would wrap by -L if I7 were above the end in their contexts, which says nothing about this one **[O]**.
   Very low.
5. **The 33 stores' addresses** (0x2cb2a2 to 0x2cb2c2: between the new I7 = 0x2cb298 and I6 = 0x2cb2c8): below the interrupted frame, the addresses we know of (the reply records 0x266220 and 0x267220, the type cache 0x255970, the frame copy 0x2558dc) are in block 0; a reference to this stack area from DMA was not looked for **[O]**. A store never reads, so the parity check of MODE1 bits 27 to 29 (p.9-1, `p0277`: it acts on reads) cannot fire on a first write. The `DM(-6 .. -15, I6)` stores run in
   stock's own range (`FUN_1c4ecf` stores at I6-4 to I6-26). Very low.
6. **`R3 = DM(_dv_sv_l4)`** (190): an absolute load of a word the same hook wrote at its entry, the same form the stock-type path runs (`_dv_stock_go`, which rung B and C2 ran on the device). No.
7. **System-register reads right after writes**: none in the zone. The first MODE1 write is at offset 223, after the zone; `R3 = MODE1` follows only loads and stores.

### 10.5 Two builds and a third, and what to expect **[D]**

Options of `core/dsp-voice` (docs/PATCHING.md "Rung C3d"), all in the one source, the preprocessed text with no option is byte-identical to the old (`pk_build build --check` says current for both devices; the C3 variants'
text is unchanged):

| option | what it does |
| --- | --- |
| `DV_C3_VIAR` | each of the 17 stores becomes `R2 = reg; DM(n, I6) = R2;` (R2 is free there: the routine pc has moved to I12), as the stock code does. The frame layout, the epilogue and everything else are unchanged. |
| `DV_C3_DEEP` | twelve more inline breadcrumbs (stages 11 to 22, `R0`, `R2` and `I4` only, so nothing the frame save has not stored yet is touched): after the MODIFY (11), after the R-register stores (12), after `I0` (13), `I1`-`I5` (14), `M1`-`M3` (15), `B3`+`I13` (16), `M5`-`M7` (17), `M13`-`M15` (18), `L0`-`L3` (19), after the `_dv_sv_l4` load (20), after `L5` (21), after `R3 = MODE1` (22). Needs `DV_C3_FINE`. Do not combine with `DV_C3_NOIRQ` (both use R0). `tools/device/dklog.py` names them. |

Images (not committed), base `Digitakt_II_OS1.17.syx` (`26c22f66...`), module list of rung C, all `PREFLIGHT OK`; options `DV_C3_FINE,DV_C3_BFIRST` plus:

| variant | options | image | bytes | sha256 |
| --- | --- | --- | ---: | --- |
| d1 | `DV_C3_DEEP` | `out/patch/rungC3d1-dt2-1.17.syx` | 1,885,728 | `3972e3929bbcde01a3719496b860ecc6c760334be94ee4023df412f6fa6515cf` |
| d2 | `DV_C3_VIAR` | `out/patch/rungC3d2-dt2-1.17.syx` | 1,885,600 | `18d7a6d56c537d3df664a5b5edf2931d74ac676b537ad1a318b78d30973cb94b` |
| d3 | `DV_C3_VIAR,DV_C3_DEEP` | `out/patch/rungC3d3-dt2-1.17.syx` | 1,885,728 | `0840b3bba8c32fbda7fd7fb98b923b76fb9b411d90efa8c3a9e7a789c860f0fa` |

Emulator proofs (DT2 1.17, full tier, scenarios `rung-c3d1`, `rung-c3d2`, `rung-c3d3`): see the table in the section "Rung C3d" of docs/PATCHING.md; each is the rung C flow (upload, menu, restart, run-time pluck on track 1, 12 s note) and must
show 19,574 dispatches and as many returns, no `DSP_STOPPED`, the note equal to the patched-in pluck. `tests/patchkit/test_dsp_voice.py`: the deep breadcrumbs come in order at their labels for d1 and d3, the VIAR builds have no
DAG1 store on the path and the default has 17, the object-level check agrees (17 / 0), and the existing C3 tests run on the three new variants too.

**Which to run first: d2.** It changes only the 17 stores. If the cause is the stores, d2 plays the pluck. d1 is the confirmation of the mechanism (the illegal stores kept, the halt placed to the instruction group: expected last
breadcrumb 12, the `I0` store) and is worth a run only if d2 halts or if a proof of the cause is wanted before the default is changed. d3 is the fallback that finds a second problem behind the first: it carries the fix and the
breadcrumbs. The default `core/dsp-voice` is **not** changed here (it still has the 17 stores and the DN2 wrapper has the same ones, section 10.6).

### 10.6 Open items

1. A device run decides it (d2; d1 if the mechanism is to be shown). Marks stay **[D]** until a second agent has checked 10.2's counts against the image bytes and a device run has agreed.
2. **If d2 plays:** change the default (`DV_C3_VIAR` always on, the option dropped), and fix `dsp-voice-dn2.s` (`DM(15 .. 25, I4) = I0 .. M4` at lines 69-79 and `DM(0 .. 10, I4) = M5 ... L5` at 157-167; I4 is DAG1, the sources I0-I3, I5, M1-M4, M5-M7, L0-L3, L5 are DAG1: 17 stores). The Digitone II has never run these on a device.
3. **The emulator should refuse the instruction.** A store whose source the class excludes is accepted by `tools/sharc_dev/sharc_core` and by the runtime; making it raise the illegal-opcode path (or at least stop with a log line) would have
   caught this in the rung C emulator run. It changes `CORE_FINGERPRINT` (regenerate the cores), so it is a separate piece of work; `pk_sharc.ureg_class_problems` already checks every assembled instruction.
4. The silicon's reaction to the excluded encodings (ILOPI, wrong store, hang) is not public; d1's last breadcrumb tells it only as "stopped in that group".
5. Other forms with a class: `Ureg` restrictions of Type 3a/3b (post-modify) and the `DM`/`PM` loads into the same DAG's I/M/B/L (allowed, with a stall) were not audited beyond the counts of 10.2; the checker covers 15a/15b/3b/3a by the `d`, `g` and `ureg` fields only.

### 10.7 The device result: d2 plays **[D]** (2026-10-19)

Rung C3d2 on the Digitakt II (finding 47, "Rung C3d2"): the run-time pluck played on TRIG 1 and its parameters responded; `dispatches` equal to `returns`
at every report (15,693 to 45,799, about 1,500 a second), no `DSP_STOPPED`, the context words unchanged. The 17 stores were the only difference from the
halting C3a build that touches the frame save (C3d2 is `DV_C3_FINE,DV_C3_BFIRST,DV_C3_VIAR`; C3a was `DV_C3_FINE,DV_C3_BFIRST,DV_C3_DB`, and `DV_C3_DB` acts after
stage 10, which C3a never reached), so **the excluded stores are the cause of the rung C halt** **[D]** (one device run; d1, the mechanism run, was not flashed).
How the silicon reacts to them (10.6 item 4) stays **[O]**.

Done since (commit 8c7c7c93): items 1 and 2 of 10.6. The R2 route and the B3-before-I3 epilogue are the default of `core/dsp-voice` (the options are gone; the
default object equals the old `DV_C3_BFIRST,DV_C3_VIAR` build byte for byte), `dsp-voice-dn2.s` saves its 17 through R0/R3, `pk_sharc.ureg_class_problems` checks the
index-register forms only (3a, 3b, 3d, 15a, 15b; Type 14a exempt) and fails the build, and preflight scans section 7 code blocks (`packages/patchkit/src/dspscan.rs`, the Rust port of the earlier Python scan, equal on the stock and patched images:
stock DT2 1.17 and DN2 1.12 have 0, the old rung C image 17). Item 3 (the emulator) is still open; a check at decode time in `packages/emulator/sharc-rt`
(`decoded_insn`, and the fast tier's `decode_one`) raising a native trap would not change `CORE_FINGERPRINT` (a scout's reading, not built).
