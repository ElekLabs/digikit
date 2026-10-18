# PLAITS machine for Digitakt II 1.17: design

Status: draft for the owner's approval (revised 2026-10-17 after hardware tests). No product code built; only
throwaway diagnostic images (`out/patch/diag-*.syx`).

## Intent

Add one machine, **PLAITS**, to the Digitakt II, based on the Mutable Instruments Plaits source in
`../eurorack/plaits` (relative to the `elektron-re` directory that holds digikit). Plaits works best with modulation,
and the Digitakt's sequencer is the reason to want it on the box. Every control it exposes should be p-lockable,
modulatable from the MOD page, and randomizable with SRC+YES. The sound goes through the same filter, amp, FX and
mod path as a sample machine.

The owner thinks of the Plaits models as the machine's samples. Picking a model should feel like picking a sample in
Oneshot: FUNC + turn the LEV knob opens a list, and the list holds the model names instead of samples. Model is also
one of the main knobs on the SRC page.

## Goals

1. A PLAITS machine type, selected with FUNC+SRC like any other machine, that persists in projects.
2. v1 models (owner's choice, in this order):
   VA, PHASE-DIST, TERRAIN, STRING MACHINE, 2FM, ADDITIVE, WAVETABLE, CHORD, SPEECH, STRING, RINGS, KICK.
   This stays the owner's v1 set. Implementation now follows the model table's order (Plaits' own order, below; owner,
   2026-10-17), so ASYM-TRI (model 1) comes second although it is not in this list.
3. Model selection through the FUNC+LEV list (the Oneshot sample-select gesture) and through a knob.
4. One SRC page: TUNE, MODEL, HARMONICS, TIMBRE, MORPH, MIX, LEV. All p-lockable, available as LFO/MOD
   destinations, randomized by SRC+YES.
5. The voice passes through the stock amplitude envelope, filter, track FX, sends and master. The DT2's envelopes
   and LFOs replace Plaits' internal envelope, low-pass gate and CV attenuverters (owner).
6. All 16 tracks can run PLAITS at once at full Plaits quality (owner). Optimize with efficient implementations that
   give the same sound. Approximations that trade quality for CPU come later, and only if the owner chooses them.
7. Emulator for development and equivalence tests; the owner flashes builds and tests on the unit.

## Non-goals (v1)

- The other 12 slots (VCF-VA, ASYM-TRI/waveshaping, FORMANT/grain, SWARM, NOISE, DUST/particle, SNARE, HIHAT,
  SQUARE/chiptune, three DX7 banks). They come later, in the same machine; model numbers are reserved now.
  (Since the owner's 2026-10-17 instruction, implementation follows the table order, so ASYM-TRI is model 1 and the
  second one ported.)
- Plaits' AUX output as a separate track output (see MIX).
- MIDI CC/NRPN control of PLAITS parameters (added types ignore CC today, finding 02).
- Plaits' user-data upload.
- Plaits' internal decay envelope, low-pass gate, LPG colour, DECAY, FM amount, TIMBRE/MORPH modulation amounts and
  fine tune (owner). A second SRC page is therefore not needed.
- Digitone II support.

## Anti-goals (failure even if it technically works)

- p-locks that don't audibly change the sound. P-locking is the core reason for the project.
- Pitch that doesn't track the Digitakt's notes and TUNE exactly (within a cent).
- A build that can't be recovered with the stock OS file, or one that changes anything other than the MAIN OS and
  DSP blob sections. The bootloader and updater stay byte-identical to stock.
- Any image handed to the owner that contains an instruction encoding not proven on silicon (see "Toolchain rule").
- Shipping firmware bytes or vendor DSP tool names in the repository (digikit `AGENTS.md`).

## Hardware evidence (owner's unit, OS 1.17, 2026-10-17)

The emulator accepted everything below; only flashes on the unit told the cases apart. Images are in
`out/patch/`; DSP sources of the lab-based ones in `/tmp/labsine/` (throwaway).

| Image | What it adds | Result on the unit |
|---|---|---|
| `pluck-1.17` | digikit PLUCK (core/machines, core/params, core/dsp-voice) | **stalls** once the PLUCK type reaches the DSP: all audio dead, OS update from the running OS fails |
| `diag-xslice` | an added MANUAL SLICE clone, no DSP code | works |
| `diag-D` | PLUCK's machine and rows, no DSP routine | works (track plays the stock sample engine) |
| `diag-E/F/H` | PLUCK with DECAY 0 / a routine that only writes zeros / stage A skipped | **stall** |
| `diag-J` | XSLICE + dt2-machine-lab SINE hook | **stalls** |
| `diag-K` | lab hook, custom branch continues through the stock sample reader | works |
| `diag-L/M/N` | lab hook, custom branch writes zeros / a register saw / L2 table reads, then the stock decimator | work, M and N audible |
| `diag-O/P` | + per-voice phase kept in L2 between blocks / + trigger-latch read | work, audible |
| `diag-Q/R` | J without envelope / J without interpolation | Q **stalls**, R works |
| `diag-Q1` | Q with its one negative immediate shift replaced by a register-amount shift | **works**: clean 440 Hz |

Conclusions:

1. **Root cause of the lab-hook stall: the Selache assembler's encoding of a negative immediate shift.** Elektron's
   toolchain encodes `R2 = LSHIFT R15 BY -1` as `023e7800ff2f` (the four bits above the 8-bit amount set, labelled
   `dataex` by digikit's decoder); Selache encodes `R1 = LSHIFT R6 BY -24` as `023e0000e816`. Silicon chokes on
   Selache's form; digikit's emulator reads it Selache's way, so no emulator test can catch it.
2. **The in-renderer hook works on silicon:** a branch inside the stock renderer at SW `0x1c4f15` (after its
   prologue) that renders the voice's 64-float, 96 kHz work buffer (record `+4`) and rejoins the stock decimator at
   `0x1c524a`. Per-voice state in L2, the trigger latch (DM `0x24f0cc`), L2 table reads, float maths and custom
   audio through the stock amp, filter and FX are all proven.
3. **digikit's `core/dsp-voice` (whole-renderer replacement at `0x1c4ecf`) stalls on silicon** for an unknown reason
   (F has no negative immediate shift). PLAITS does not use it.
4. A machine type reaches the DSP only on the next trig (or at boot from a saved kit). Tests must trig.
5. `core/machines` (type list, persistence, the DSP selector table) and `core/params` (own rows) do not break the unit.
   p-lock on added rows is still unproven on hardware.
6. DT2 1.16 and 1.17 DSP blobs are byte-identical (SHA-256 `0f514a12…`), so the lab's 1.16 DSP addresses hold.

## Architecture

- **ColdFire:** digikit patchkit `core/machines` adds PLAITS as a **MANUAL SLICE clone** (type → selector 5, as
  XSLICE, the configuration proven on the unit); `core/params` supplies its own rows. Type number from
  `modules/core/machines/types.toml`.
- **DSP hook:** a new patchkit framework module (working name `core/dsp-inline`) that installs the in-renderer hook
  at `0x1c4f15`: it checks the track's type, runs the registered machine's routine for its voices and rejoins the
  stock decimator, and otherwise replays the six displaced bytes and continues at `0x1c4f18` (the lab's proven
  shape). Not `core/dsp-voice`.
- **Output path for the 48 kHz port (owner's priority).** The proven path delivers 96 kHz into the work buffer, then
  the stock decimator halves it. Two options:
  - **(a, proven hook point)** render Plaits at 48 kHz, upsample 2× with a linear-phase half-band interpolator (flat
    within 0.1 dB to 20 kHz, images below −90 dB), then the stock decimator. No sample duplication (the lab's
    "experimental rate adapter" is rejected).
  - **(b, needs one hardware test)** rejoin after the decimator's call instead and write the 32 output samples at
    48 kHz directly: no resampling at all.
  Phase 2 tests (b) on the unit. If it works, PLAITS uses (b); otherwise (a). *(Designer's call.)*

## Toolchain rule (from conclusion 1)

- Fix Selache's encoder so negative shift immediates are encoded like Elektron's (carried as a local patch on the
  pinned revision, with a test against the stock examples above).
- **Mandatory encoding gate** before any image reaches the owner: every instruction in every added code region is
  decoded; each instruction class and encoding shape must either occur in the stock 1.17 DSP code (from
  `out/sharcdb/dt2-1.17.sqlite`) or be on a list of shapes proven on the unit (the executed blocks of the passing
  diagnostic images). Anything else fails the build. This covers selcc output too.
- Known unknowns for the gate to watch: other Selache short/compressed forms the lab had to avoid with `.NOCOMPRESS`,
  and selcc's byte/word pointer handling (finding 20).

## Plaits source facts the port depends on

- `dsp/dsp.h`: `kSampleRate = 48000`, but pitch uses **`kCorrectedSampleRate = 47872.34`** through
  `a0 = (440/8) / kCorrectedSampleRate`. `kBlockSize = 12`, `kMaxBlockSize = 24`.
- `dsp/voice.cc` registers 24 engine slots from 22 classes (`six_op_engine` three times, one per DX7 bank), with
  per-engine "already enveloped" flags and output/AUX gains.
- Engines share one 16 KB arena per voice (`plaits.cc`). `voice.cc` calls every engine's `Init` once at power-up and,
  at an engine change, only `LoadUserData` and `Reset`; a trigger never re-initialises the engine (it reaches it as
  `EngineParameters.trigger`). The port follows this since Task 9b: init at a voice's first note-on and at a model
  change, otherwise the engine runs on (`plaits.c` `note_on`).
- Read-only data: `resources/waves.bin` (33 KB) and the tables in `resources.cc` (sizes to be measured).

## How the port adapts Plaits to the DT2

1. **Rate.** Plaits renders at 48 kHz with `a0` from exactly 48,000 Hz (not 47,872.34, which would play about
   4.6 cents sharp). Any table that bakes in the corrected rate is regenerated. Delivery to the DT2 per the output path
   above.
2. **Block size.** The DT2 needs 32 samples at 48 kHz per block: two 16-sample Plaits sub-blocks (builder may choose
   24 + 8). Control rate becomes 3 kHz instead of 4 kHz; stated in the verification tolerances.
3. **Sample format.** Float throughout; Plaits' 16-bit DAC step is dropped. Scale chosen so a default PLAITS voice
   matches the RMS of a full-scale stock sample at the same level.
4. **DC.** A one-pole DC blocker (about 5 Hz) at the voice output. *(Designer's call.)*
5. **Gate and envelope (owner).** No internal decay envelope, LPG or attenuverters; DT2 envelopes and LFOs do that
   work. **Strike engines (STRING, RINGS, KICK) (owner):** every DT2 note-on (trigger latch, proven in image P) is
   sent as Plaits' trigger, so they keep their attack; ring length stays on MORPH. Plaits' untriggered sustained mode
   is not used. A model change lands at the next note-on.
6. **Pitch and velocity.** `note` from the frame's pitch and TUNE words; velocity to `accent`. *(Designer's call.)*
7. **Per-voice state.** Engine objects plus a 16 KB arena per voice; 16 voices ≈ 16 × (16 KB + state). L2 first,
   external memory if it doesn't fit. The target is 16 voices (owner).
8. **CPU.** Target: 16 voices of the heaviest v1 model at reference quality. Output-identical optimizations from the
   start (no work for removed features, skip AUX when MIX is at an end, nothing for idle voices, shared read-only
   tables, SIMD and zero-overhead loops where the encoding gate allows). If a model doesn't fit, measure and report;
   the owner decides on approximations.

## Parameters and pages

One page; the controls fit the eight existing per-track machine fields (mirrors 25–28, 31–34), so no storage
extension.

| Knob | Slot (mirror) | Parameter | Notes |
|---|---|---|---|
| A | 0 (25) | TUNE | stock row |
| B | 1 (26) | MODEL | stable model numbers 0..23; also set through FUNC+LEV |
| C | 2 (27) | HARMONICS | |
| D | 3 (28) | — | MANUAL SLICE's sample slot; kept unless the FUNC+LEV research puts MODEL here |
| E | 4 (31) | TIMBRE | |
| F | 5 (32) | MORPH | |
| G | 6 (33) | MIX | OUT/AUX blend (designer's call; drop it and the knob is empty) |
| H | 7 (34) | LEV | stock level row |

The DSP reads them from the frame at `0xda + 0x60t + 2·(mirror − 25)` (finding 06). Image D showed the stock sample
engine tolerates foreign values in these slots.

**Model numbers are permanent and follow Plaits' own order** (owner, 2026-10-17: "add more of the models, basically
matching the order of engines as they appear in plaits, add the second one"; read against the Plaits manual's model
order). They replace the earlier table (VA 0, PHASE-DIST 1, TERRAIN 2, ...), under which no model other than VA had
been implemented; VA keeps 0:

| # | model | # | model | # | model |
|---|---|---|---|---|---|
| 0 | VA | 8 | SWARM | 16 | VCF-VA |
| 1 | ASYM-TRI (waveshaping) | 9 | NOISE | 17 | PHASE-DIST |
| 2 | 2FM | 10 | DUST (particle) | 18 | DX7 1 |
| 3 | FORMANT (grain) | 11 | STRING | 19 | DX7 2 |
| 4 | ADDITIVE | 12 | RINGS (modal) | 20 | DX7 3 |
| 5 | WAVETABLE | 13 | KICK (bass drum) | 21 | TERRAIN |
| 6 | CHORD | 14 | SNARE | 22 | STRING MACHINE |
| 7 | SPEECH | 15 | HIHAT | 23 | SQUARE (chiptune) |

Implemented: 0 VA (H2) and 1 ASYM-TRI (H3). An unported number plays silence; the plan to hide unported numbers from
the list and skip them with the knob stands. *(The order is the owner's; the rest the designer's call.)*

## Engine port approach

- Start from the dt2-machine-lab C99 translations, which match upstream exactly on the host: VA, 2FM (FM), KICK
  (analog bass drum) and ADDITIVE cover 4 of the v1 models; waveshaping and granular cover two later ones. They carry
  Plaits' MIT notice. The lab's own tooling (translation scripts, wrappers, harness) has **no licence file**: use it
  as reference and write our own, unless its author grants terms.
- Port the remaining v1 engines the same way (C99 translation with a host oracle against upstream), compiled with
  selcc behind the encoding gate. Hand-written assembly only for measured hot spots.
- The lab's wrapper idea (save the stock context, own C stack, call the compiled engine) is reused in our own code.

## Verification

- **Host reference:** upstream Plaits built on the Mac with the rate fixed at 48,000 Hz; each C99 port matches it
  exactly on the host.
- **Emulator equivalence:** compiled SHARC output against the host port within tolerance (pitch 1 cent, RMS 0.5 dB,
  spectrum 1 dB per band below 8 kHz), measured at the voice output, not the work buffer. Needs the emulator set up
  (the public ADSP-2156x hardware reference PDF in `docs/refs/`). The emulator cannot catch encoding bugs; that's
  the encoding gate's job.
- **Hardware checks by the owner** for each milestone image, with the standard script: select on track 1, check
  track 2, trig, PLAY, reboot.
- **p-lock proof:** a p-locked parameter on one step changes that step's audio only (first on hardware, Phase 2).
- **Randomize proof:** SRC+YES changes PLAITS parameters; MODEL lands only on ported models.
- **Budget gate:** 16 voices of the heaviest v1 model against the measured block capacity.

## Phases

0. **Hook proof on silicon.** *Done* (table above).
1. **Toolchain.** Selache negative-shift fix and the encoding gate; the gate must flag the original Q and pass Q1.
2. **Hardware capability checks** (one image, owner-tested): output path (b) and a p-lock of TUNE changing the pitch
   of a test tone on one step.
3. **VA end to end:** the lab's C99 VA at 48,000 Hz, compiled, in the hook, through the chosen output path, on a
   PLAITS machine entry (type, rows, one page). Host, emulator, then the unit.
4. **ColdFire UI research (Ghidra):** the FUNC+LEV list with model names, SRC+YES randomize, MODEL p-lock behaviour.
5. **Remaining v1 engines,** simplest first: 2FM, ADDITIVE, KICK (lab ports), then WAVETABLE, PHASE-DIST, TERRAIN,
   STRING, RINGS, CHORD, STRING MACHINE, SPEECH.
6. **Integration and budget:** 16 voices, randomize, MOD destinations, save/load, the owner's full test.

Every image for the owner: `elektron-firmware-tool -i` checksums OK, digikit `sharc_bootcheck` 0 errors against
stock, encoding gate passed, sections other than 3 and 7 identical to stock. Recovery: hold FUNC at power-on, TRIG 4,
send the stock `.syx` with Transfer's legacy mode over MIDI DIN. No agent flashes a device.

## Owner decisions (2026-10-17)

1. One page, no storage extension. DECAY, FINE, FM, LPG colour and the modulation amounts are cut.
2. 16 simultaneous PLAITS tracks at reference quality; identical-output optimizations first.
3. The unit runs 1.17. Emulator for development; the owner flashes and tests builds.
4. v1 model list as in Goals; the full set is the engines in `dsp/engine` and `dsp/engine2`.
5. Note-on strikes STRING, RINGS and KICK; no sustained mode.

## Left to the builder

- The structure of `core/dsp-inline` and how it registers machine routines.
- Assembly versus selcc per hot spot, after measurement.
- Per-voice state layout in L2 or external memory.
- Sub-block split within the 32-sample block.
- On-screen knob order; the short machine name (suggest `PLT`) and shortened model names where needed.
