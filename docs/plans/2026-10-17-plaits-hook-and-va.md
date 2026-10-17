# PLAITS phases 1–3: toolchain, in-renderer hook, VA end to end — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A PLAITS machine on Digitakt II 1.17 that plays the Plaits VA model at exactly 48 kHz through the stock amp/filter/FX, built only from instruction encodings proven on silicon, ready for the owner to flash.

**Architecture:** Fix Selache's negative-shift encoding and gate every added DSP instruction against stock/hardware-proven shapes. A new patchkit framework module `core/dsp-inline` hooks inside the stock voice renderer (SW `0x1c4f15`, the shape proven on the owner's unit) and dispatches registered machine routines. `machines/plaits` is a MANUAL SLICE clone (core/machines + core/params) whose DSP routine runs a C99 port of Plaits VA compiled with selcc.

**Tech Stack:** Python 3.11+ (uv, pytest), Rust patchkit (unchanged except where noted), Selache (`selas`, `seld`, `selcc`) at the pinned revision plus a local patch, SHARC+ assembly, C99, upstream Plaits C++ for the host oracle.

**Spec:** `docs/design/2026-10-17-plaits-machine-design.md` (approved 2026-10-17, commit c238362). Phases 4–6 of the spec (FUNC+LEV list, randomize research, remaining engines, 16-voice budget) get their own plans after this one ships.

## Global Constraints

- Target firmware: Digitakt II OS 1.17, `firmware/Digitakt_II_OS1.17.syx`, SHA-256 `26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5`.
- Plaits renders with `a0` from exactly **48,000 Hz**, never 47,872.34 Hz. Pitch within 1 cent of 440·2^((n−69)/12).
- DT2 block: 32 samples at 48 kHz (1500 blocks/s); stock work buffer: 64 floats at 96 kHz at voice record `+4`.
- PLAITS is a **MANUAL SLICE clone** (`clone_of = 6`, DSP selector 5).
- `core/dsp-voice` is not used and conflicts with `core/dsp-inline`.
- Every image for the owner: `elektron-firmware-tool -i` checksums ok; `sharc_bootcheck --stock` 0 errors; encoding gate passed; sections 2, 4, 5, 8 byte-identical to stock.
- No firmware bytes, firmware-derived data or vendor DSP tool names in commits (`AGENTS.md`). `out/`, `firmware/`, `docs/refs/` stay untracked.
- dt2-machine-lab tooling has no licence: do not copy its scripts, wrappers or harness. Its engine translations carry Plaits' MIT notice and may be reused with that notice kept.
- No agent flashes a device. Hardware checks are an owner checklist at the end of a task.
- Model numbers: VA 0, PHASE-DIST 1, TERRAIN 2, STRING MACHINE 3, 2FM 4, ADDITIVE 5, WAVETABLE 6, CHORD 7, SPEECH 8, STRING 9, RINGS 10, KICK 11, then 12–23 as in the spec.

## Review Focus

1. Switching a ringing PLAITS track to another machine (and back) mid-note: no stall, no stale sound, the stock machine plays normally → test in Task 6.
2. PLAITS on several tracks at once (different notes, overlapping): each voice has its own state and pitch → test in Task 6.
3. Fast retriggers and repeated identical notes: every note-on restarts the engine (trigger-latch bit per voice, not just track 1) → test in Task 4.
4. Extreme knob values (TUNE 0 and 127, HARMONICS/TIMBRE/MORPH/MIX at 0 and 127): no NaN, no runaway DC, no silence where Plaits makes sound → host test and emulator check in Task 6.
5. A project saved with PLAITS loaded on stock 1.17: loads as MANUAL SLICE without fault → test in Task 6.

---

### Task 1: Selache encodes negative shift immediates like Elektron's toolchain

**Files:**
- Create: `tools/patchkit/selache_patches/0001-negative-shift-immediate.patch`
- Modify: `tools/patchkit/pk_toolchains.py` (`build_selache`, `status`)
- Test: `tests/patchkit/test_selache_shift.py`

**Interfaces:**
- Produces: `pk_toolchains.SELACHE_PATCHES: list[pathlib.Path]`; `status()["selache_patches"]` = list of patch SHA-256s; patched `selas`/`selcc` binaries in `out/tools/selache/target/release/`.

- [ ] **Step 1: Write the failing tests** (`tests/patchkit/test_selache_shift.py`), using `tools.patchkit.pk_sharc.assemble` and comparing the encoded 48-bit words (as the stock listings print them):

```python
CASES = {
    "R1 = LSHIFT R6 BY -24;": "023e7800e816",
    "R2 = LSHIFT R15 BY -1;": "023e7800ff2f",       # stock 0xb8088e
    "R7 = ASHIFT R6 BY -4;": "023e7801fc76",        # stock 0x1c0e57
    "R1 = R1 OR LSHIFT R0 BY -8;": "023e7808f810",  # stock 0xb883b3
    "R1 = LSHIFT R0 BY 4;": "023e00000410",         # positive: unchanged
}
def test_shift_immediates_match_stock_encoding(): ...  # each CASES item

@pytest.mark.skipif(not STOCK_DB.exists(), reason="needs out/sharcdb/dt2-1.17.sqlite")
def test_stock_rule_negative_shift_sets_data_hi():
    # every stock 6b_shiftimm whose op is a shift (lshift/ashift and their OR forms) has
    # data_hi == 0xF exactly when the 8-bit amount is negative, else 0
```

- [ ] **Step 2: Run** `uv run pytest tests/patchkit/test_selache_shift.py -v` — expected: the negative cases FAIL (`023e0000…`), the positive case and the stock-rule test PASS.

- [ ] **Step 3: Write the patch.** In `selas/src/asmparse.rs`, both `Instruction::ImmShift` constructions from a parsed immediate (the shift-family parser and the `OR LSHIFT/ASHIFT` parser, currently `data_hi: 0`) set `data_hi: if imm_val < 0 { 0xF } else { 0 }`. Check whether `selcc` builds `ImmShift` directly (`selcc/src/emit_asm.rs`, `isel.rs`); if it does, apply the same rule there. Include a Selache unit test for the stock example. Save as a `git format-patch` file at the path above.

- [ ] **Step 4: Apply patches in `build_selache()`** after `git checkout -q SELACHE_REV`: `git checkout -- .` then `git apply` each file in sorted `SELACHE_PATCHES` order, then `cargo build --release`. `status()` reports the patch hashes. Rebuild: `uv run python -m tools.patchkit.pk_toolchains selache`.

- [ ] **Step 5: Run** the Task 1 tests (all PASS), then `uv run pytest tests/patchkit/test_pk_sharc.py -v` (PASS), then `mise run patch:build -- modules/machines/pluck --check`. That last command is expected to report only toolchain-hash staleness, because PLUCK has no negative immediate shifts. Confirm by diffing object sections as in the session notes; do not regenerate committed objects for that alone.

- [ ] **Step 6: Commit** `git add tools/patchkit/selache_patches tools/patchkit/pk_toolchains.py tests/patchkit/test_selache_shift.py && git commit -m "patchkit: Selache patch encodes negative shift immediates like the stock toolchain"`

### Task 2: Encoding gate on every patched image

**Files:**
- Create: `tools/patchkit/pk_encoding_gate.py`, `tools/patchkit/encoding_proven.toml`
- Modify: `tools/patchkit/patchkit.py` (`cmd_patch`)
- Test: `tests/patchkit/test_encoding_gate.py`

**Interfaces:**
- Consumes: `out/sharcdb/dt2-1.17.sqlite` (`insn` table), `tools.sharc_dev.sharc_disasm.decode_confident_loaded`, `tools.sharc_dev.sharcldr.LoadedMemory`, the plan JSON `patchkit patch` writes (`sections`, `symbols`, inserted DSP blocks).
- Produces: `shape(ins: Instruction) -> str`; `stock_shapes(db: Path) -> set[str]`; `check_code(mem, ranges: list[tuple[int,int]], allowed: set[str]) -> list[Violation]` (`Violation(pc: int, raw: str, shape: str)`); `check_image(syx: Path, plan: Path) -> list[Violation]`; CLI `uv run python -m tools.patchkit.pk_encoding_gate OUT.syx PLAN.json` (exit 1 on violations).

- [ ] **Step 1: Failing tests.**

```python
def test_selache_old_negative_shift_is_rejected():   # raw 023e0000e816 → one Violation
def test_stock_negative_shift_is_allowed():          # raw 023e7800e816 → none
def test_short_and_fsub_forms_are_allowed():          # 01840262, 01882710 → none (stock uses these shapes)
def test_unknown_shape_reports_pc_and_raw():          # a synthetic unmatched form → Violation with pc/raw
def test_check_image_on_pluck_image_runs():           # @slow: out/patch/pluck-1.17.syx + plan → returns a list
```

- [ ] **Step 2: Run** `uv run pytest tests/patchkit/test_encoding_gate.py -v` — FAIL (module missing).

- [ ] **Step 3: Implement.** `shape()` keeps the instruction type and opcode bits and masks register numbers and immediate values. Exceptions that stay in the key:
  - for `6b_shiftimm`: the shift op, the sign of the 8-bit amount and `data_hi`
  - for `2c`: the op nibble
  - for `2a_short`: `compute[22:16]` and the top nibble of `compute[15:0]`

  Allowed set = `stock_shapes(db)` ∪ shapes in `encoding_proven.toml`. Each TOML entry is `shape`, `evidence` (image name and date), and `note`. Start the TOML empty. Add an entry only for a shape executed in an image that passed on the owner's unit. `check_image` walks the code ranges of inserted DSP blocks and of the hook site from the plan.

- [ ] **Step 4: Wire into `cmd_patch`.** After a successful patch, run `check_image`. On violations, print them, delete the output `.syx`, and exit 1. `--no-encoding-gate` exists only for diagnostic images and prints a warning naming every violation.

- [ ] **Step 5: Run** the Task 2 tests (PASS), then `mise run patch -- --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/xslice -o out/patch/gate-xslice.syx` (passes: XSLICE adds no DSP code).

- [ ] **Step 6: Commit** `git commit -m "patchkit: encoding gate refuses DSP instruction shapes not proven on silicon"`

### Task 3: Emulator ready for 1.17 with DSP audio

**Files:** none tracked. Local only: `docs/refs/adsp-2156x-hwr.pdf`, `out/refs/`, `out/ready-pk-dt2-117/`.

**Interfaces:**
- Produces: a working `DIGI_EMU_READY_DIR_DT2=out/ready-pk-dt2-117` for `mise run inject`, `tools/patchkit/pk_harness`, and headless renders with `DIGI_EMU_SINK_WAV`.

- [ ] **Step 1:** Fetch the public *ADSP-2156x SHARC+ Processor Hardware Reference, Rev 1.0* (the file `docs/sharc/SOURCES.md` lists) from analog.com into `docs/refs/adsp-2156x-hwr.pdf`. Build text with `uv run --with pymupdf python -m tools.devtools.refstext`. Expected: `out/refs/adsp-2156x-hwr/all.txt` exists. If the ready step later asks for other refs from `SOURCES.md`, fetch those the same way.
- [ ] **Step 2: Run** `uv run python -m tools.emulator.ready dt2 --syx firmware/Digitakt_II_OS1.17.syx --out out/ready-pk-dt2-117 --force` — expected: completes, no traceback.
- [ ] **Step 3: Run** `mise run patch:test -- --cell "dt2-1.17:machines/xslice"` (record screen goldens with `--update` once if asked) — expected: smoke tour passes.

### Task 4: `core/dsp-inline` — the in-renderer hook framework

**Files:**
- Create: `modules/core/dsp-inline/module.toml`, `modules/core/dsp-inline/src/dsp/dsp-inline.s`, `modules/core/dsp-inline/README.md`, `modules/core/dsp-inline/tests/smoke.toml`
- Modify: `docs/findings/06-sharc-engine-and-startup.md` (append a section "In-renderer hook: core/dsp-inline", with the hardware table from the spec)
- Test: `tests/patchkit/test_dsp_inline.py`

**Interfaces:**
- Consumes: anchors `dsp.voice.render_entry` (renderer entry `0x1c4ecf`) and `dsp.voice.type_cache` (`0x255970`) from `core/dsp-voice`'s anchor rules (copy the rules; the modules conflict), plus new anchors:
  - `dsp.inline.decimate`: SW `0x1c524a`
  - `dsp.inline.after_decimate`: the first instruction after the decimator `CALL 0xb80000` at `0x1c525d` and its delay slots; establish the exact SW address by disassembly in Step 1
  - `dsp.inline.trig_latch`: DM `0x24f0cc`
  - `dsp.inline.frame_base`: `0x2558dc`
- Produces:
  - Hook: `op = "hook"`, `cpu = "sharc"`, anchor `dsp.voice.render_entry`, `offset = 0x8c` (SW `0x1c4f15`), `displaced = "run"`, `expect_sha256` = SHA-256 of bytes `5499a801408c`. The fallback jumps to patchkit's tail symbol, which replays the six bytes and returns to `0x1c4f18`.
  - Registry `inline_voices`, rows `{type: u32, pc: u32}`, max 16, terminator.
  - **Routine ABI**, entered by `JUMP (M13, I12)` with `I13` = return pc:
    - in: `R4` = voice record base, `R0` = voice index v (0..31, from `(record − 0x2412cc) / 0x1d8`), `R2` = track `v >> 1`, `R1` = type, `I1` = record + 4
    - returns with `R0` = 0: "wrote 64 floats at 96 kHz to the work buffer" → continue at `dsp.inline.decimate`
    - returns with `R0` = 1: "wrote 32 floats at 48 kHz to the voice's final output" → continue at `dsp.inline.after_decimate`
    - may clobber R0–R15, I0–I5, M1–M4; must keep I6, I7, M5–M7, M13–M15, L registers, MODE1
  - Helper symbols `_di_frame_hw(k) -> R0` (halfword k of the frame copy) and `_di_trig(v) -> R0` (the voice's byte of the trigger latch).

- [ ] **Step 1: Pin the post-decimator continuation.** Disassemble 1.17 from `0x1c524a` to the renderer epilogue (`tools.sharc_dev.sharc_disasm.decode_loaded_at`). Record in findings 06:
  - the SW address control reaches after `CALL 0xb80000` returns
  - which frame slot holds the voice's final 32-float output pointer, given the prologue stores at `DM(-4..-25, I6)`
  - what the decimator writes besides that output
  - the trigger-latch layout for all 32 voices: the lab only used one 32-bit word at DM `0x24f0cc` (voice v's byte
    = `(word >> 8v) & 0xff` for v = 0, 1). Find the stock writer and reader of the latch and record where voices 2–31
    live.

  The `R0 = 1` path must leave everything the epilogue reads exactly as the decimator would.
- [ ] **Step 2: Failing tests** in `tests/patchkit/test_dsp_inline.py`, on the concrete core with `pk_sharc.run_call` (as `test_dsp_voice.py` does):

```python
def test_unregistered_type_runs_displaced_and_returns_to_0x1c4f18()
def test_registered_type_mode0_continues_at_decimate_with_work_buffer_written()
def test_registered_type_mode1_continues_after_decimate_with_output_written()
def test_voice_and_track_derived_from_record_for_v0_v1_v30_v31()
def test_trig_latch_byte_per_voice()          # each voice 0..31 reads its own latch byte at the location Step 1 pinned;
                                              # setting only voice 5's byte → _di_trig(5) != 0, all others == 0
def test_image_passes_encoding_gate()         # patch core/dsp-inline + test/inline-tone; check_image → []
```

- [ ] **Step 3: Run** them — FAIL.
- [ ] **Step 4: Implement** `dsp-inline.s` and `module.toml`, with `conflicts = ["core/dsp-voice"]`, `requires = ["core/machines"]` and `ports = ["dt2-1.17"]`. Use only stock-proven instruction shapes. Do right shifts with register amounts if Task 1's patch isn't present.
- [ ] **Step 5: Run** the tests — PASS. Then `mise run patch:build -- modules/core/dsp-inline --check`.
- [ ] **Step 6: Commit** `git commit -m "core/dsp-inline: in-renderer voice hook with work-buffer and direct-48k return modes"`

### Task 5: `test/inline-tone` and hardware capability image H1

**Files:**
- Create: `modules/test/inline-tone/module.toml`, `modules/test/inline-tone/src/dsp/tone.s`, `modules/test/inline-tone/tests/select.toml`, `modules/core/machines/types.toml` entry `"test/inline-tone:itone"` (via `mise run patch:new`)
- Test: `tests/patchkit/test_inline_tone.py`

**Interfaces:**
- Consumes: Task 4 registry `inline_voices`, ABI, `_di_frame_hw`, `_di_trig`.
- Produces: images `out/patch/h1a-tone-96k-1.17.syx` (mode 0) and `out/patch/h1b-tone-48k-1.17.syx` (mode 1), built from one source with an assembler define `TONE_MODE`.

The machine is `name = "ITone"`, `short = "ITN"`, `clone_of = 6`, with MANUAL SLICE's TUNE (`f0 = 0xf8`) and LEV (`f7 = 0xfe`) rows and `f8 = 0x0a`. The routine plays a sine at f = 440·2^((n − 69)/12), with n = pitch/256 + (tune/256 − 64) from the frame words, using a phase accumulator in per-voice state. The phase restarts on the voice's trigger-latch byte. Mode 0 renders at 96 kHz (64 samples); mode 1 renders at 48 kHz (32 samples). Gain is 0.125.

- [ ] **Step 1: Failing tests:**

```python
def test_tone_pitch_note60_tune64_is_261_626_hz()      # concrete core, both modes, within 1 cent
def test_tone_pitch_follows_tune_word()                 # tune 0x4100 → +1 semitone
def test_tone_restarts_on_trigger_byte()
def test_h1_images_pass_all_owner_image_checks()        # checksums, bootcheck, gate, sections 2/4/5/8 == stock
```

- [ ] **Step 2: Run** — FAIL.  **Step 3: Implement.**  **Step 4: Run** — PASS.
- [ ] **Step 5: Emulator check** (Task 3). Inject each H1 image, select ITONE on track 1, play trig 1 (note 60), and record `DIGI_EMU_SINK_WAV`. Expected: 261.6 Hz ± 1 cent. A track-2 ONESHOT still plays.
- [ ] **Step 6: Commit** `git commit -m "test/inline-tone: hardware capability probe for core/dsp-inline (96k and direct 48k)"`
- [ ] **Step 7: Owner checklist (H1):**
  - Flash `h1b` first: select ITONE on track 1, check track 2, trig, PLAY, reboot.
  - Then p-lock TUNE on one step (hold the trig, turn TUNE) and confirm only that step's pitch changes.
  - If `h1b` stalls or is silent, repeat with `h1a`.
  - Record the result in the spec's hardware table and in `encoding_proven.toml` (shapes executed by the passing image). **Decision:** if `h1b` passes, PLAITS uses mode 1. Otherwise mode 0, and Task 5a applies.

### Task 5a (only if H1 picks mode 0): half-band 2× interpolator

**Files:** Create `modules/machines/plaits/src/dsp/halfband.c`, `.h`; Test: `tests/plaits/test_halfband.py`

**Interfaces:**
- Produces: `void hb_up2(HalfBand *s, const float *in48, float *out96, int n48)`, with `HalfBand` state per voice.

- [ ] **Step 1:** Failing tests on the host build (ctypes):
  - passband flat within 0.1 dB to 20 kHz
  - image rejection ≥ 90 dB at 48 kHz − f for f ≤ 20 kHz
  - linear phase (constant group delay)
  - no sample duplication: `out[2k+1]` differs from `out[2k]` for a 5 kHz sine
- [ ] **Step 2:** Run — FAIL. **Step 3:** Implement a symmetric FIR half-band design (taps generated by a committed script, coefficients in the header). **Step 4:** Run — PASS. **Step 5:** Commit.

### Task 6: `machines/plaits` with the VA model, end to end

**Files:**
- Create:
  - `modules/machines/plaits/module.toml`
  - `src/dsp/plaits.c` (voice wrapper)
  - `src/dsp/plaits_entry.s` (ABI shim: context save, own C stack, calls `plaits_render`)
  - `src/dsp/engines/va.c`, `va.h` (from the lab's MIT VA translation: keep its notice, set `a0` from 48000.0f)
  - `src/dsp/dsp_util.h`
  - `tests/plaits/oracle/va_oracle.cc` (own code: links upstream `eurorack/plaits` VA with an overriding `dsp.h` copy where 47872.34f → 48000.0f)
  - `tests/plaits/test_va_host.py`
  - `tests/patchkit/test_plaits_module.py`
  - `modules/machines/plaits/tests/select.toml`
  - `README.md`
- Modify:
  - `tools/patchkit/pk_build.py` (`build_module`: a DSP object may list `.c` sources; compile each with `pk_sharc.compile_c` and merge with assembled `.s` into one object)
  - `modules/core/machines/types.toml` (entry `"machines/plaits:plaits"` via `mise run patch:new`)

**Interfaces:**
- Consumes: Task 4 ABI and registry; Task 5's mode decision; Task 5a `hb_up2` if mode 0; core/params rows; Task 2 gate.
- Produces:
  - Machine: `name = "Plaits"`, `short = "PLT"`, `clone_of = 6`. Descriptor fields:
    - `f0 = 0xf8` (TUNE)
    - `f1` = MODEL, `f2` = HARMONICS, `f4` = TIMBRE, `f5` = MORPH, `f6` = MIX (core/params rows `plaits_model`, `plaits_harm`, `plaits_timbre`, `plaits_morph`, `plaits_mix`)
    - `f3 = 0`, `f7 = 0xfe` (LEV), `f8 = 0x0a`
  - Row ranges: MODEL `0..0x1700` (UI 0..23), default 0; the others `0..0x7f00`, default `0x4000`, except MIX default 0 (all OUT).
  - C API: `int plaits_render(PlaitsVoice *v, const PlaitsFrame *f, int trig, float *out, int n, int rate)`. `PlaitsVoice` is per-voice state: VA engine, two sub-block buffers, DC-blocker state, last model, and half-band state if mode 0. The shim reads the frame and calls it for even voices; odd voices copy the even voice's output, as the stock stereo pair expects.
  - Note n = pitch/256 + (tune/256 − 64). harmonics/timbre/morph = word/0x7f00 clamped to [0, 1]. MIX blends OUT/AUX linearly. Velocity is not used in VA.
  - Two 16-sample `va_render` calls per DT2 block. DC blocker: one pole at 5 Hz. Gain scaled so default VA at note 60 matches the RMS of a full-scale stock sample at equal LEV (measure in the emulator; record the constant in `plaits.c` with the measurement).
  - MODEL ≠ 0 renders silence in this plan; model changes take effect at the next note-on.

- [ ] **Step 1: Failing test** `tests/patchkit/test_pk_build_c.py::test_dsp_object_with_c_and_s_sources_builds`: a fixture module with one `.c` function and one `.s` caller builds a `.dko.json` exporting both symbols. **Step 2:** Implement in `pk_build.py`, run — PASS, commit (`patchkit: DSP objects may include C sources (selcc)`).
- [ ] **Step 3: Failing host tests** (`tests/plaits/test_va_host.py`, host clang via ctypes):

```python
def test_va_c99_matches_upstream_48k_bit_exact()      # 4 parameter sets × 4000 blocks of 12, max abs diff == 0
def test_va_pitch_note69_is_440_hz_at_48k()            # within 1 cent
def test_va_extreme_params_finite()                    # all of {0,1}^3 params, notes 0 and 127: no NaN/Inf, |x| < 4
def test_wrapper_dc_blocker_removes_offset()           # constant input → output decays below 1e-4 within 0.5 s
```

- [ ] **Step 4:** Run — FAIL. **Step 5:** Implement `va.c`/`va.h`, `plaits.c` and the oracle. **Step 6:** Run — PASS.
- [ ] **Step 7: Failing module tests** (`tests/patchkit/test_plaits_module.py`):

```python
def test_plaits_image_passes_all_owner_image_checks()
def test_compiled_va_matches_host_on_concrete_core()   # 120 blocks, 4 settings: max abs err ≤ 2e-3, RMS ≤ 1e-4
def test_two_voices_independent_state()                # v0 note 60, v2 note 67 → two pitches, no crosstalk
def test_retrigger_restarts_engine()
def test_switch_type_away_and_back_is_silent_until_trig()
```

- [ ] **Step 8:** Run — FAIL. **Step 9:** Implement the module and shim. **Step 10:** Run — PASS.
- [ ] **Step 11: Emulator checks** (Task 3), recording `DIGI_EMU_SINK_WAV` at the voice output:
  - note 60 pitch within 1 cent
  - HARMONICS/TIMBRE/MORPH knob turns change the spectrum
  - LEV, filter and reverb send act as on a stock track
  - PLAITS on tracks 1 and 2 at once
  - save, set track 1 to ONESHOT, load → PLAITS and its values return
  - the same project on stock 1.17 loads with track 1 as MANUAL SLICE and no fault
- [ ] **Step 12: Commit** `git commit -m "machines/plaits: VA model at 48 kHz on core/dsp-inline"`
- [ ] **Step 13: Owner checklist (H2)** with `out/patch/plaits-va-1.17.syx`:
  - the standard script (select on track 1, check track 2, trig, PLAY, reboot)
  - play notes across the keyboard: in tune
  - p-lock HARMONICS, TIMBRE, MORPH and TUNE on single steps
  - LFO on TIMBRE
  - SRC+YES (record what it does; Phase 4 owns it)
  - PLAITS on three tracks at once
  - switch a ringing PLAITS track to ONESHOT and back

  Record the results in the spec's hardware table and `encoding_proven.toml`.
