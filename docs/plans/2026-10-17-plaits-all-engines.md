# PLAITS phase 5: every Plaits engine except DX7 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PLAITS models 2–17 and 21–23 (every Plaits engine except the three DX7 slots) run on the Digitakt II at
exactly 48 kHz, each proven bit-exact against upstream on the host and equivalent on the compiled DSP core, delivered
as an owner image (H4).

**Architecture:** Extends `machines/plaits` (Tasks 6–7 of `docs/plans/2026-10-17-plaits-hook-and-va.md`). Each engine is
a C99 port behind the wrapper's model switch. All engines share one per-voice state union (Plaits' shared arena
pattern). Shared upstream helpers are ported once into `src/dsp/lib/`. The DX7 slots (18–20) stay silent until their
UI is designed (owner, 2026-10-17).

**Tech Stack:** C99 compiled with selcc behind the encoding gate and operand cross-check; host clang plus ctypes for
the upstream oracle; pytest; patchkit; the digikit emulator.

**Spec:** `docs/design/2026-10-17-plaits-machine-design.md`. Owner decisions:
- all engines except DX7 (2026-10-17)
- model numbers in Plaits' order (Task 7 brief)
- note-on strikes self-enveloped engines
- 48 kHz exactly
- reference quality first

## Global Constraints

- Prerequisite: Task 7 of the previous plan (model renumbering, ASYM-TRI, the per-voice state union, model switch at
  note-on) is complete and reviewed. Every task here builds on its wrapper and tests.
- Model numbers (permanent):
  - 0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH
  - 8 SWARM, 9 NOISE, 10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT
  - 16 VCF-VA, 17 PHASE-DIST, 18–20 DX7 (silent, not in scope), 21 TERRAIN, 22 STRING MACHINE, 23 SQUARE
- `a0` from exactly 48,000 Hz. No 47,872.34 anywhere in the runtime path. Tables that bake in the corrected rate are
  regenerated.
- Upstream source: `../eurorack/plaits` (f4cfa76) with `../eurorack/stmlib` (e3bd7c9). Reuse dt2-machine-lab engine
  translations only where they carry the MIT notice (`machines/plaits-bank`, `plaits-trio`). Never copy its
  unlicensed tooling.
- Strike engines (STRING 11, RINGS 12, KICK 13, SNARE 14, HIHAT 15) get Plaits' trigger from the DT2 note-on (trigger
  latch), never `TRIGGER_UNPATCHED`. Their decay stays on the engine's own parameter mapping. Velocity goes to
  `accent`.
- Owner images pass all four checks: checksums ok, `sharc_bootcheck --stock` 0 errors, encoding gate, and sections 2,
  4, 5 and 8 equal to stock. The gate and cross-check are never weakened, and no owner image uses
  `--no-encoding-gate`.
- Known selcc/Selache hazards (finding 20): negative float initialisers, dropped addends, 9a under `.NOCOMPRESS`, PM→DM
  stores. Every initialised table is verified bit for bit against host float32.
- **Safety rule for CPU:** an engine whose worst-case 1-track load (the high bound, finding 06 method) exceeds 85 % of
  the block ships **silent**. Its MODEL value renders zeros and it is listed as "ported, disabled: over budget". The
  owner decides on approximations later. The owner track rule is recomputed from the heaviest *enabled* model.
- **Engine recipe** (every engine task does all of these):
  - **(a)** C99 port in `modules/machines/plaits/src/dsp/engines/<name>.c/.h`, plus shared helpers in `src/dsp/lib/`.
  - **(b)** Host oracle test against upstream at 48 kHz: bit-exact over parameter vectors and corners (for strike
    engines, over trigger sequences), plus finite output at extremes.
  - **(c)** Wrapper dispatch for its model number, with state in the per-voice union.
  - **(d)** Compiled-core equivalence (max abs error ≤ 2e-3, the established tolerance), an extreme-parameter compiled
    test with a measured per-block instruction bound, and model-switch-at-note-on to and from VA.
  - **(e)** CPU worst case per block and 1- and 2-track load recorded in finding 06 [D], plus the safety rule above.
  - **(f)** Encoding gate and cross-check pass on a built image.
  - **(g)** Model 0 and every earlier enabled model stay bit-identical (regression vectors).
- Tests live in `tests/plaits/test_<name>_host.py` and `tests/patchkit/test_plaits_module.py` (or a sibling per family).
  Test basenames are unique.

## Review Focus

1. A model change while a note rings (VA → SPEECH → STRING on one voice): no stale state, no stall, and the new model
   starts clean at the next note-on → Task 8 adds the generic test; each engine task extends it.
2. Memory: 16 voices × the largest engine state plus all tables must fit the free L2 (or the documented external
   memory) with no overlap → Task 13 measures and enforces it; each task reports its state and table bytes.
3. Strike engines retriggered quickly (repeated notes, 1/32 steps): every note-on strikes, with no runaway energy →
   Task 11.
4. Speech and other table-driven engines at every MODEL/HARMONICS corner index the tables in bounds (no read past a
   table) → Tasks 9 and 10 assert index ranges.
5. The heaviest model at worst-case parameters on the maximum allowed tracks stays ≤ 85 % → Task 13.

---

### Task 8: Lab-ported family — 2FM (2), FORMANT (3), ADDITIVE (4)

**Files:**
- Create: `src/dsp/engines/fm.c/.h`, `grain.c/.h`, `additive.c/.h`, `src/dsp/lib/` helpers as needed
  (sine_oscillator, downsampler, harmonic_oscillator, grainlet/vosim/z oscillators)
- Test: `tests/plaits/test_fm_grain_additive_host.py`, `test_plaits_module.py`

**Interfaces:**
- Consumes: Task 7's state union, model switch and test helpers.
- Produces: the generic multi-model test helper `assert_model_switch(rig, a, b)` in `tests/support/plaits_rig.py`.

- [ ] **Step 1:** Write the failing host oracle tests (bit-exact, 4 settings × 4000 blocks, plus extremes) for the three engines.
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port, from the lab MIT translations where present (FM, grain, additive exist in `plaits-bank`/`plaits-trio`).
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire models 2–4 into the wrapper. Add the compiled tests (equivalence, extremes, switch). Run them, record CPU, build the image, run the gate.
- [ ] **Step 6:** Commit `machines/plaits: 2FM, FORMANT, ADDITIVE models`.

### Task 9: Table-driven family — WAVETABLE (5), CHORD (6)

**Files:**
- Create: `src/dsp/engines/wavetable.c/.h`, `chord.c/.h`
- Create: `src/dsp/lib/wavetable_oscillator.*`, `string_synth_oscillator.*`, `chord_bank.*`, `waves.h`
  (generated from `resources/waves.bin`/`resources.cc` by a committed generator)
- Test: `tests/plaits/test_wavetable_chord_host.py`

**Interfaces:** Produces `chord_bank` and `wavetable_oscillator` for Task 12 (string machine, chiptune, terrain).

- [ ] **Step 1:** Write the failing oracle tests. Include the index-range assertions for every bank, row and column corner.
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port the engines and generate the tables. Verify the tables bit for bit against host float32/int16.
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire into the wrapper and run recipe (d)–(g).
- [ ] **Step 6:** Commit.

### Task 10: SPEECH (7)

**Files:**
- Create: `src/dsp/engines/speech.c/.h`
- Create: `src/dsp/lib/` naive formant, SAM, LPC synth and controller, words, phonemes
- Create: generated `speech_tables.h`
- Test: `tests/plaits/test_speech_host.py`

- [ ] **Step 1:** Write the failing oracle tests:
  - bit-exact across the HARMONICS range (formant → SAM → LPC vowels → word banks)
  - every word-bank corner, with an index-range assertion
  - prosody and speed as Plaits sets them in `voice.cc` (patch values fixed to Plaits' defaults; document them)
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port the engine and generate the tables. Report the table bytes; this is the largest table set.
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire into the wrapper and run recipe (d)–(g), plus the CPU safety rule.
- [ ] **Step 6:** Commit.

### Task 11: Noise and physical family — SWARM (8), NOISE (9), DUST (10), STRING (11), RINGS (12)

**Files:**
- Create: `src/dsp/engines/swarm.c`, `noise.c`, `particle.c`, `string.c`, `modal.c` and headers
- Create: `src/dsp/lib/` clocked_noise, particle, diffuser, string_voice, string, modal_voice, resonator,
  delay_line, random (stmlib Random, bit-exact seeding)
- Test: `tests/plaits/test_noise_physical_host.py`

- [ ] **Step 1:** Write the failing oracle tests:
  - bit-exact with identical RNG seeding per voice (document the seed rule: the same per-voice seed on every
    engine init, as upstream)
  - STRING and RINGS driven by trigger sequences: single, repeated at 1/32-step spacing, and accent extremes
  - assertions that energy stays bounded on repeated strikes
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port. Report the per-voice state bytes: DUST's diffuser is 8192 × uint16 = 16 KB, and STRING's delay lines are `kDelayLineSize` floats.
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire into the wrapper, including the strike trigger from the latch for 11 and 12. Run recipe (d)–(g).
- [ ] **Step 6:** Commit.

### Task 12: Drums — KICK (13), SNARE (14), HIHAT (15)

**Files:**
- Create: `src/dsp/engines/bass_drum.c`, `snare_drum.c`, `hi_hat.c`
- Create: `src/dsp/lib/` analog/synthetic bass and snare drums, hi_hat, overdrive, sample_rate_reducer
- Test: `tests/plaits/test_drums_host.py`

- [ ] **Step 1:** Write the failing oracle tests over trigger sequences and accent, bit-exact. Include fast-retrigger tests. (The lab has an MIT analog bass drum translation.)
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port.
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire into the wrapper with the note-on strike, and run recipe (d)–(g).
- [ ] **Step 6:** Commit.

### Task 13: Newer engines — VCF-VA (16), PHASE-DIST (17), TERRAIN (21), STRING MACHINE (22), SQUARE (23)

**Files:**
- Create: `src/dsp/engines/vcf_va.c`, `phase_dist.c`, `terrain.c`, `string_machine.c`, `chiptune.c`
- Create: `src/dsp/lib/` ensemble, arpeggiator, nes_triangle, super_square
- Test: `tests/plaits/test_engine2_host.py`

- [ ] **Step 1:** Write the failing oracle tests:
  - bit-exact for all five
  - SQUARE's envelope shape and arpeggiator modes as `voice.cc` drives them (no timbre-modulation CV on the DT2:
    use `NO_ENVELOPE` unless the owner decides otherwise; document it)
- [ ] **Step 2:** Run them — FAIL.
- [ ] **Step 3:** Port.
- [ ] **Step 4:** Run them — PASS.
- [ ] **Step 5:** Wire into the wrapper and run recipe (d)–(g).
- [ ] **Step 6:** Commit.

### Task 14: Integration — memory, CPU policy, image H4

**Files:**
- Modify: `modules/machines/plaits/README.md`, `HARDWARE-TEST.md`, `docs/findings/06-...md`, the design doc's model table
- Test: `tests/patchkit/test_plaits_module.py` (memory and budget tests)

- [ ] **Step 1:** Failing tests:
  - `test_state_union_fits_l2_budget`: 16 voices × the union size, plus all tables, fits the free L2 range patchkit
    reports, or the documented external-memory placement with its own bounds
  - `test_disabled_models_render_silence`: every model listed over budget renders zeros
  - `test_every_enabled_model_switches_cleanly`: a pairwise model switch at note-on across all enabled models on one
    voice
- [ ] **Step 2:** Implement the placement and the enabled-model table. Recompute the owner track rule from the heaviest enabled model at worst case (≤ 85 %).
- [ ] **Step 3:** Build `out/patch/plaits-all-48k-1.17.syx`. Run the four owner checks, and the emulator proof for every enabled model at note 60 (pitch within 1 cent where the model is pitched; strike engines sound on trig).
- [ ] **Step 4:** Write the H4 checklist in `HARDWARE-TEST.md`:
  - after H3 passes
  - walk the MODEL knob 0→23 (18–20 and disabled models silent)
  - MODEL p-locks across families
  - strike engines on fast trigs
  - the track rule
  - recovery
- [ ] **Step 5:** Commit and push to the fork branch (owner-authorised remote `spacejam`, branch `plaits-machine-dsp-inline-va`).
