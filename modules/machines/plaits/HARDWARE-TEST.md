# PLAITS on the unit: H1 (the hook), H2 (the machine), H3 (a second model), H3b (models 2 to 4), H3c (models 5 and 6), H3d (engines run on between notes), H3e (model 7, SPEECH) and H3f (output stage, velocity, RINGS and TERRAIN)

One decision tree for the owner's hardware round on a Digitakt II with OS 1.17. H1 tests core/dsp-inline with a
plain sine (test/inline-tone); H2 tests machines/plaits (the VA model) on the same hook. The images are firmware
derivatives: they live under out/patch/ and are never committed. Results go in the design's hardware table
(docs/design/2026-10-17-plaits-machine-design.md) and, for an image that passed, its executed shapes or words in
tools/patchkit/encoding_proven.toml.

Recovery from a stall (any image): hold FUNC at power-on, press TRIG 4, then send the stock
`firmware/Digitakt_II_OS1.17.syx` with Elektron Transfer's legacy mode over MIDI DIN.

## The images

Every image below except the diagnostics passes the four owner checks: `elektron-firmware-tool -i` reports
`checksums : ok`; `uv run python -m tools.sharc_dev.sharc_bootcheck --syx X --stock firmware/Digitakt_II_OS1.17.syx`
reports 0 errors; `uv run python -m tools.patchkit.pk_encoding_gate X X.plan.json` reports 0 violations; sections 2, 4,
5 and 8 are byte-identical to stock.

| image | sha256 | built with |
|---|---|---|
| `out/patch/h1b-tone-48k-1.17.syx` | `6171da820712e3b1c66c014dd102c2cc7ee3c038d9c50fa81e540d63c283e8e9` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules test/inline-tone-48k -o out/patch/h1b-tone-48k-1.17.syx` |
| `out/patch/h1a-tone-96k-1.17.syx` | `bc6a3314170f013b5974cf35b8f001e76adcb000f6c843ae961648af2b8fb139` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules test/inline-tone-96k -o out/patch/h1a-tone-96k-1.17.syx` |
| `out/patch/plaits-va-48k-1.17.syx` | `507434164c90e42f26c3111d9b5a366b8b2b36f6d7d100c4e519b80d5c3f7492` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-48k -o out/patch/plaits-va-48k-1.17.syx` |
| `out/patch/plaits-va-96k-1.17.syx` | `f37e87a0025ab07895c17275c5e0ee531e04125de34feafa0866781cc761c83f` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-96k -o out/patch/plaits-va-96k-1.17.syx` |
| `out/patch/plaits-2models-48k-1.17.syx` (H3) | `68fee193f40a611a58613409b3e9c4a81fa242fcf3b09355fa7f073d75d236e4` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-48k -o out/patch/plaits-2models-48k-1.17.syx` (at 3608a92) |
| `out/patch/plaits-5models-48k-1.17.syx` (H3b) | `e768eb4823f708c976242805f90615586ca0ec7e9c247e7f203f8b1506fe70ec` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-48k -o out/patch/plaits-5models-48k-1.17.syx` |
| `out/patch/plaits-5models-names-48k-1.17.syx` (H3b with the MODEL names; verified by the owner, PLAY included, with a new project; built by the coordinator, its owner checks are not repeated here) | `c3b7596bc379113124554105c0aec89d47c96a4596bbeea75087469171aec397` | the same command at 119d75d |
| `out/patch/plaits-7models-48k-1.17.syx` (H3c) | `09ac59c0c1fb47460e39cd4639886f2fd0a8bbd733e71d318520f0637627b8a3` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-48k -o out/patch/plaits-7models-48k-1.17.syx` (at 6bf06f1) |
| `out/patch/plaits-7models-names-splash-48k-1.17.syx` (H3d: engines run on between notes, MODEL names, boot splash) | `8032d08653293d7fa322c4e7f68b20752342aee3542e3e182a50038324c2754d` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules ui/boot-splash,machines/plaits-48k -o out/patch/plaits-7models-names-splash-48k-1.17.syx` (at ed36e25) |
| `out/patch/plaits-8models-names-splash-48k-1.17.syx` (H3e: H3d plus model 7, SPEECH) | `8701a9bf5c73df271a20f4b0540230044903abdc09fa538ccec202e4a61d0e24` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules ui/boot-splash,machines/plaits-48k -o out/patch/plaits-8models-names-splash-48k-1.17.syx` (at caba134) |
| `out/patch/plaits-10models-names-splash-48k-1.17.syx` (H3f: Plaits' output stage, velocity, models 12 RINGS and 21 TERRAIN) | `b3797740adf014a5f189a4a19279bab22b07079b1e1d846ed59347268436a04e` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules ui/boot-splash,machines/plaits-48k -o out/patch/plaits-10models-names-splash-48k-1.17.syx` (at 711a7f3) |
| `out/patch/diag-h1-i13return-48k-1.17.ungated.syx` | `401ccf2be3f2814e4bb79c6aed1872912da4ee73f2010e93c5b387fbf4279cf3` | a copy of the pre-Task-5b h1b; not rebuildable from this branch |
| `out/patch/diag-h1-i13return-96k-1.17.ungated.syx` | `02926b3f833ccdaa51172b98af3de83c4f65779999a3ea901d1a527c6796652c` | a copy of the pre-Task-5b h1a; not rebuildable from this branch |

`mise run patch -- ...` is the same command. The gate runs by default; never add `--no-encoding-gate` for an image
you flash. The H2 hashes above are those of the images after the final fix wave (the stack-switch order was proved
interrupt-safe without a code change, so they equal the images of Task 6).

## The decision tree

**After every flash, create a new project before any check:** an older project may not play. The PLAY failures
reported on H3b went away with a new project (owner, 2026-10-18: `plaits-5models-names-48k` plays, PLAY included);
a project saved under an earlier image is the likely cause. Every step below that flashes an image starts with this.

1. **Flash h1b** (`6171da82…`, 48 kHz straight into the voice output).
   - Create a new project after flashing (an older project may not play).
   - Select ITONE on track 1: FUNC+SRC, then 7 x DOWN (ITONE is the 8th entry of the machine list), YES.
   - Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent, load any sample onto track 2 first), then check that it plays.
   - Trig track 1: note 60 should be a 261.6 Hz sine. PLAY, then reboot and check again.
   - P-lock TUNE on one step (hold the trig, turn TUNE): only that step's pitch changes. The routine renders both
     voices of the track in every block, so a releasing voice also follows a TUNE p-lock: the tail of the previous
     note bends to the p-locked step's tune while it decays. That is expected, not a fault.
   - **h1b passes:** go to step 3 (H2 with plaits-48k).
   - **h1b stalls or is silent:** recover, then step 2.
2. **Flash h1a** (`bc6a3314…`, 96 kHz through the stock decimator). Create a new project after flashing (an older project may not play). Then the same checks as in step 1.
   - **h1a passes:** go to step 4 (H2 with plaits-96k, one track only).
   - **h1a stalls too:** no H2. Stop and report both H1 results (the hook itself does not run on the unit).
3. **H2 with plaits-48k** (`50743416…`; only after h1b passed).
   0. Create a new project after flashing (an older project may not play).
   1. FUNC+SRC, 7 x DOWN (PLAITS is the 8th entry), YES. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent, load any sample onto track 2 first), then check that it plays. Trig track 1:
      note 60 is 261.6 Hz, a bright VA tone. PLAY; reboot and check again.
   2. Play notes across the keyboard (KEYBOARD mode): in tune.
   3. P-lock HARMONICS (C), TIMBRE (E), MORPH (F) and TUNE (A) on single steps: only those steps change.
   4. LFO on TIMBRE: audible movement.
   5. SRC+YES: record what it does.
   6. PLAITS on track 1, then also on track 2 (two tracks at once). **Never more than 2 tracks triggered as PLAITS
      since boot on this image.** A track stops costing DSP only after it has been triggered under another machine
      (the type reaches the DSP only on a trig): switching a track away from PLAITS without a trig does not free it.
      Two tracks measure 73 % of the DSP block at the high bound with default settings; extreme HARMONICS, TIMBRE
      and MORPH on both tracks approach the 85 % budget (finding 06).
   7. Switch a ringing PLAITS track to ONESHOT and back (each change takes effect at the next trig). A small click
      on the first stock note after a track leaves PLAITS is expected: this image skips the stock decimator, whose
      state is then stale for that voice's first stock block.
   - **plaits-48k stalls or is silent** (with h1b passed, the hook works and the fault is in PLAITS): recover and
     report; optionally repeat 3.1 to 3.5 with plaits-96k, one track only (step 4).
   - **plaits-48k passes:** go on to H3 (section "H3: model 1, ASYM-TRI" below), then H3b (section "H3b: models 2 to
     4"), then H3c (section "H3c: models 5 and 6"), then H3d (section "H3d"), then H3e (section "H3e"), then step 6.
4. **H2 with plaits-96k** (`f37e87a0…`; after h1a passed and h1b did not, or as the optional fallback of step 3). Create a new project after flashing (an older project may not play).
   Then steps 3.1 to 3.5 and 3.7 on track 1. **Never more than 1 track triggered as PLAITS since boot on this image** (the same
   rule: a track stops costing DSP only after a trig under another machine). Do not run step 3.6: two tracks on this
   image measure 85.6 % of the block at the high bound, over the 85 % budget.
5. **Diagnostics come last,** and only if both h1b and h1a passed. Optional: they tell whether the earlier stalls came
   from the `JUMP (M13, I13)` return.
   - Create a new project after flashing (an older project may not play).
   - `diag-h1-i13return-48k-1.17.ungated.syx` and `diag-h1-i13return-96k-1.17.ungated.syx`: the same tone, returning
     through `JUMP (M13, I13)`. **FAILS THE ENCODING GATE; may stall; recovery: hold FUNC at power-on, TRIG 4, send the
     stock .syx with Transfer's legacy mode over MIDI DIN.**
   - If they stall while h1a/h1b passed, the stall is pinned on `083f683f`. If one passes, `word = "083f683f"` may be
     added to encoding_proven.toml with that evidence.
6. Record every result (H1, H2, H3, H3b, H3c, H3d and H3e) in the design's hardware table, and the shapes or `word =` entries of each image that passed in
   tools/patchkit/encoding_proven.toml. Decision for PLAITS: h1b passing keeps mode 1 (plaits-48k) as the default;
   otherwise mode 0 (plaits-96k).

## H3: model 1, ASYM-TRI (only after H2 with plaits-48k passed)

H3 (`out/patch/plaits-2models-48k-1.17.syx`, `68fee193…`) is H2 plus model 1, ASYM-TRI (Plaits' waveshaping model).
Model 0 (VA) renders bit for bit as in H2 (tests/patchkit/test_plaits_model0_h2.py). The model numbers now follow
Plaits' own order: 0 VA, 1 ASYM-TRI, 2 2FM, ... 23 SQUARE; 2 to 23 still play silence. Flash it only after step 3
(H2 with plaits-48k) passed; there is no 96k H3 image.

1. Flash H3. Create a new project after flashing (an older project may not play). FUNC+SRC, 7 x DOWN (PLAITS, the 8th entry), YES on track 1. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent, load any sample onto track 2 first), then check that it plays.
   Trig track 1 with MODEL 0: the H2 VA tone at 261.6 Hz, unchanged.
2. Turn MODEL (knob B) to 1. A MODEL change takes effect at the next note-on: trig again. Note 60 is 261.6 Hz, a
   folded-triangle tone that sounds clearly different from VA.
3. On model 1, per the Plaits manual (the waveshaping model: an asymmetric triangle through a waveshaper and a
   wavefolder; AUX a variant with another wavefolder curve):
   - HARMONICS (C): the waveshaper's waveform (the middle is the linear shaper; either end gives other curves).
   - TIMBRE (E): the wavefolder amount (more folds, brighter and buzzier as it rises).
   - MORPH (F): the waveform's asymmetry (triangle to saw-like).
   - MIX (G) to the right: the AUX variant (a sine at TIMBRE 0, the second wavefolder as TIMBRE rises).
4. MODEL p-lock:
   - set MODEL (knob B) back to 0 (the track's own value: the steps without a lock play it);
   - place trigs on steps 1, 3, 5, 7, 9, 11, 13, 15;
   - p-lock MODEL = 1 on steps 1, 5, 9, 13 (hold the step's trig, turn knob B to 1);
   - PLAY: the notes alternate VA / ASYM-TRI (steps 1, 5, 9, 13 ASYM-TRI; 3, 7, 11, 15 VA).
   Also p-lock TUNE, HARMONICS, TIMBRE, MORPH on single model-1 steps: only those steps change.
5. Save the project, reboot, load it: MODEL and the knob values are back and model 1 plays.
6. **Track rule (unchanged from H2): never more than 2 tracks triggered as PLAITS since boot on this image,** with
   any mix of models. Two ASYM-TRI tracks measure 71.5 % of the block at the high bound and 73.5 to 75.7 % at
   worst-case settings; two VA tracks at worst-case settings (81 to 85 %) still set the limit (finding 06). A track stops costing DSP only after it has been triggered under another machine.
- **H3 stalls or is silent** while H2 passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to the H2 image
  (`plaits-va-48k-1.17.syx`, `50743416…`) and report which step failed.

## H3b: models 2 to 4, 2FM, FORMANT and ADDITIVE (only after H3 passed)

H3b (`out/patch/plaits-5models-48k-1.17.syx`, `e768eb48…`) is H3 plus models 2 (2FM, Plaits' two-operator FM), 3
(FORMANT, Plaits' granular formant oscillator) and 4 (ADDITIVE, Plaits' harmonic oscillator). Models 0 and 1 render bit
for bit as in H3 (tests/patchkit/test_plaits_model0_h2.py, test_plaits_model_digests.py); 5 to 23 still play silence.
Flash it only after H3 passed; there is no 96k H3b image.

1. Flash H3b. Create a new project after flashing (an older project may not play). FUNC+SRC, 7 x DOWN (PLAITS, the 8th entry), YES on track 1. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent, load any sample onto track 2 first), then check that it plays.
   Trig track 1 with MODEL 0, then MODEL 1 (a trig after each change): the H3 tones, unchanged.
2. **2FM.** MODEL (knob B) to 2, trig. With HARMONICS (C) at 60 (a 2:1 ratio) note 60 is 261.6 Hz, an FM tone. Per the Plaits manual:
   - HARMONICS (C): the frequency ratio between the two operators (quantised to musical ratios; between the steps it
     glides through inharmonic ones).
   - TIMBRE (E): the modulation index (0: a pure sine; up: brighter, then metallic).
   - MORPH (F): feedback from the output into the modulator, none at the centre: to the right as phase modulation,
     to the left as frequency modulation (both roughen the tone towards noise at the ends, each differently).
   - MIX (G) to the right: AUX, the sub-oscillator (an octave below, phase-modulated by the carrier).
3. **FORMANT.** MODEL to 3, trig. Note 60 at 261.6 Hz, a vocal or resonant buzz. Per the manual:
   - HARMONICS (C): the frequency ratio between the two formants (the second formant above or below the first);
     towards the left also more of the carrier itself.
   - TIMBRE (E): the formant frequency (sweeps like a resonant filter or a vowel).
   - MORPH (F): the formant width and shape (narrow and buzzy to wide and soft; its effect fades on high notes).
   - MIX (G) to the right: AUX, windowed sines that imitate filtered waveforms (Braids' Z models): HARMONICS selects
     the filter character (peaking, low-pass, band-pass, high-pass), MORPH the carrier shape, TIMBRE the cutoff.
4. **ADDITIVE.** MODEL to 4, trig. Note 60 at 261.6 Hz, an organ-like tone. Per the manual:
   - HARMONICS (C): the number of bumps in the spectrum (one smooth peak to several).
   - TIMBRE (E): the index of the most prominent harmonic (the brightness: low harmonics to high ones).
   - MORPH (F): the bump shape, from flat and wide to peaked and narrow.
   - MIX (G) to the right: AUX, only the harmonics of a Hammond organ's drawbars (1, 2, 3, 4, 6, 8, 10, 12).
   - A change of HARMONICS, TIMBRE or MORPH on this model glides into the new spectrum over about a third of a second
     (Plaits smooths the harmonic amplitudes); that is expected.
5. MODEL p-lock across the new models:
   - set MODEL (knob B) back to 0;
   - place trigs on steps 1, 3, 5, 7, 9, 11, 13, 15;
   - p-lock MODEL = 2 on steps 3 and 11, 3 on steps 5 and 13, 4 on steps 7 and 15 (hold the step's trig, turn knob
     B);
   - PLAY: VA, 2FM, FORMANT, ADDITIVE, VA, 2FM, FORMANT, ADDITIVE; each note starts clean, with no click or leftover
     of the previous model.
   Also p-lock TUNE, HARMONICS, TIMBRE, MORPH on single steps of each new model: only those steps change.
6. Save the project with MODEL 4 on track 1, reboot, load it: MODEL is back at 4 and model 4 plays.
7. **Track rule (lower than H2 and H3): never more than 1 track triggered as PLAITS since boot on this image,** any
   model. One ADDITIVE track at its worst settings measures 67.1 % of the DSP block at the high bound; two measure
   86.6 %, over the 85 % budget (finding 06, "PLAITS models 2 to 4"). A track stops costing DSP only after it has
   been triggered under another machine. Do not test two PLAITS tracks on this image.
- **H3b stalls or is silent** while H3 passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to the H3 image
  (`plaits-2models-48k-1.17.syx`, `68fee193…`) and report which step and which model failed.

## H3c: models 5 and 6, WAVETABLE and CHORD (only after H3b passed)

H3c (`out/patch/plaits-7models-48k-1.17.syx`, `09ac59c0…`) builds on the owner-verified `plaits-5models-names-48k`
(`c3b7596b…`: MODEL names, models 0 to 4) and adds models 5 (WAVETABLE, Plaits' wavetable oscillator) and 6 (CHORD,
Plaits' chord engine). Models 0 to 4 render bit for bit as on H3b (tests/plaits/vectors); 7 to 23 still play silence.
There is no 96k H3c image.

1. Flash H3c. Create a new project after flashing (an older project may not play). FUNC+SRC, 7 x DOWN (PLAITS, the
   8th entry), YES on track 1. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent, load any sample onto track 2 first), then check that it plays. Trig track 1 with MODEL 0, 1, 2, 3, 4 (a
   trig after each change; MODEL shows the names): the H3b tones, unchanged.
2. **WAVETABLE.** MODEL (knob B) to 5 (`WAVETABLE`), trig. Note 60 is 261.6 Hz. Per the Plaits manual:
   - HARMONICS (C): the bank (8 x 8 waves each: additive, formant-like, Braids/Shruthi waves, then a shuffled mix of
     the other banks' waves); in the right half the banks come back in reverse order and the scanning becomes stepped
     (no interpolation between the waves).
   - TIMBRE (E): the column of the 8 x 8 grid; MORPH (F): the row. Both scan smoothly through the waves.
   - MIX (G) to the right: AUX, the same wave at 5-bit resolution (a lo-fi, gritty version).
   - **Expected on H3c: a click at the start of every note** on this model (owner, 2026-10-18: WAVETABLE and CHORD
     "sound very percussive and almost click at the beginning of a note"). On H3c a note-on restarts the engine from
     Plaits' own power-up state, and Plaits' wavetable starts with a jump of about 25 times the steady level for a few
     samples (tests/plaits/test_wavetable_chord_host.py `test_note_on_transients`). H3d fixes it (section "H3d").
3. **CHORD.** MODEL to 6 (`CHORD`), trig. Per the manual:
   - HARMONICS (C): the chord (OCT, 5, sus4, m, m7, m9, m11, 69, M9, M7, M; HARMONICS at 0 is the octave).
   - TIMBRE (E): the inversion and transposition (the chord climbs as the notes rotate).
   - MORPH (F): the waveform, from organ and string-machine registrations (left) to wavetable voices (right).
   - MIX (G) to the right: AUX, the chord's root alone.
   - With HARMONICS, TIMBRE and MORPH at 0, note 60 is the octave chord at 261.1 Hz (Plaits detunes its chord 0.998).
   - On H3c, at a high MORPH and a low note the note start can click as on WAVETABLE (smaller; about 7.5 times at
     note 0), and at a high TIMBRE each note rotates through the chord's inversions for about 10 ms. H3d fixes both.
4. MODEL p-lock across all models: set MODEL back to 0; trigs on steps 1, 3, 5, 7, 9, 11, 13, 15; p-lock MODEL = 5 on
   steps 3 and 11, 6 on steps 5 and 13, 4 on step 7, 2 on step 15; PLAY: each note plays its model. Also p-lock TUNE,
   HARMONICS, TIMBRE, MORPH on single WAVETABLE and CHORD steps: only those steps change.
5. Save the project with MODEL 6 on track 1, reboot, load it: MODEL is back at 6 and CHORD plays.
6. **Track rule (unchanged from H3b): never more than 1 track triggered as PLAITS since boot on this image,** any
   model. WAVETABLE and CHORD fit two to a pair (76.7 % and 70.5 % of the DSP block at the high bound; CHORD's
   figure was measured at MORPH 0x3c00 with TIMBRE 0, not at its worst, which Task 9b measured: one CHORD track at TIMBRE 0x7f00, MORPH 0x3c00 and a note just under the point where its top voice mutes measures 60.4 % and two 70.9 %, finding 06, "PLAITS CHORD (model 6) at its worst knobs"), but
   two ADDITIVE tracks still measure 86.6 % (finding 06, "PLAITS models 5 and 6"). A track stops costing DSP only after
   it has been triggered under another machine.
- **H3c stalls or is silent** while H3b passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to `plaits-5models-names-48k`
  (`c3b7596b…`) and report which step and which model failed.

## H3d: engines run on between notes, and the boot splash (only after H3c passed)

H3d (`out/patch/plaits-7models-names-splash-48k-1.17.syx`, `8032d086…`) is H3c with two changes:

- **Engines run on between notes, as in Plaits.** A note-on initialises the engine only at the voice's first note
  since boot and when MODEL changed; every other note-on leaves the engine running and the amp envelope shapes the
  note. This removes the click and the sweep at every note start that the owner heard on H3c (WAVETABLE about 25
  times the steady level, CHORD up to 19 times at note 0; after: at the steady level). The first note after boot, and
  the first note after a MODEL change, still start from Plaits' power-up state and may still tick once on WAVETABLE
  and CHORD; a pattern that changes MODEL on every step (MODEL p-locks) re-initialises at each of those steps.
- **The boot splash** (ui/boot-splash, merged at 4b15d2c and owner-verified on the unit) is part of the build:
  `--modules ui/boot-splash,machines/plaits-48k`.

Models 0 to 6 sound as on H3c at the first note; only what happens at later note-ons changed (tests/plaits/vectors,
re-recorded at Task 9b; tests/plaits/test_continuous_host.py). There is no 96k H3d image.

1. **Boot.** Flash H3d and power on: the splash shows for about 1.5 s instead of the stock intro, then the unit boots
   as usual.
2. Create a new project after flashing (an older project may not play). FUNC+SRC, 7 x DOWN (PLAITS, the 8th entry),
   YES on track 1. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent,
   load any sample onto track 2 first), then check that it plays.
3. **Models 0 to 6.** Trig track 1 with MODEL 0, 1, 2, 3, 4, 5, 6 (a trig after each change; MODEL shows the names):
   each sounds as on H3c.
4. **WAVETABLE retriggers (the acceptance check).** MODEL 5, trig once and let it ring. Then:
   - set a slow amp attack (AMP page, ATTACK around the middle) and trig repeatedly, fast (also hold the trig and
     play KEYBOARD notes): every note must swell in smoothly, **no click, no percussive start**;
   - set the attack back to 0 and trig rapidly at one pitch, then at different pitches: no click beyond the note's
     own start, and no sweep of the timbre at each note (on H3c each note swept in from another wave over about
     20 ms);
   - turn TIMBRE and MORPH between notes: the next note starts at the new position, with no sweep from the defaults.
5. **CHORD retriggers.** MODEL 6, HARMONICS at a chord (e.g. m7), TIMBRE and MORPH to the right, a low note (e.g.
   C1). The same checks as in step 4: with a slow attack the notes swell in smoothly, with no click; at attack 0 and
   rapid trigs no click and no rotation through the inversions at each note start.
6. **First note after a MODEL change.** MODEL 0, trig; MODEL 5, trig: this first WAVETABLE note may tick once (Plaits'
   power-up state); trig again: smooth. Report how audible that first tick is.
7. MODEL p-lock across all models as in H3c step 4 (each note plays its model), and p-lock TUNE, HARMONICS, TIMBRE,
   MORPH on single WAVETABLE and CHORD steps: only those steps change. PLAY with trigs on every step of one model:
   smooth.
8. **Track rule (unchanged): never more than 1 track triggered as PLAITS since boot on this image,** any model.
   Two ADDITIVE tracks measure 86.6 % of the DSP block at the high bound (finding 06); one CHORD track at its worst
   knobs (TIMBRE 0x7f00, MORPH 0x3c00, note 64) measures 60.4 % and two 70.9 % (finding 06, "PLAITS CHORD (model 6) at its worst knobs").
- **H3d stalls or is silent** while H3c passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to H3c
  (`plaits-7models-48k-1.17.syx`, `09ac59c0…`) and report which step failed.

## H3e: model 7, SPEECH (only after H3d passed)

H3e (`out/patch/plaits-8models-names-splash-48k-1.17.syx`, `8701a9bf…`) is H3d plus model 7, SPEECH (Plaits' speech
engine: a naive formant synth, a SAM-style synth and an LPC10 synth with five banks of words). Models 0 to 6 render bit
for bit as on H3d (tests/plaits/vectors); 8 to 23 still play silence. The boot splash is the same. There is no 96k H3e
image.

1. **Boot.** Flash H3e and power on: the splash, then the unit boots as usual.
2. Create a new project after flashing (an older project may not play). FUNC+SRC, 7 x DOWN (PLAITS, the 8th entry),
   YES on track 1. Make sure track 2 has a sample (a new project's track 2 may have none: if trigging it is silent,
   load any sample onto track 2 first), then check that it plays.
3. **Models 0 to 6.** Trig track 1 with MODEL 0 to 6 (a trig after each change): each sounds as on H3d.
4. **SPEECH: the HARMONICS walk.** MODEL (knob B) to 7 (`SPEECH`). Give the amp envelope a long decay (or hold) so
   that a word is not cut short; the amp envelope shapes every PLAITS note. Per the Plaits manual, HARMONICS (C)
   crossfades between the three synthesis methods and then selects a word bank (knob C's values, 0 to 127; the
   edges are approximate, and the bank selector has hysteresis, so a bank changes a little later in the direction
   you turn):
   - **0: the naive formant synth**, a pulse through five formant filters: a sustained vowel at the note (note 60 is
     261.6 Hz). MORPH (F) picks the vowel, TIMBRE (E) the vocal register (the formants shift).
   - **0 to about 21: towards SAM** (the Shruthi/Braids-style synth of three sine formants); about 21 is SAM alone.
   - **about 21 to 42: from SAM to the LPC10 vowels;** about 42 to 55 the LPC vowels alone. MORPH scans the vowels;
     TIMBRE shifts the formants (left a deep voice, right a small, high one).
   - **about 56 to 67: words, bank 1, colours** (red, orange, yellow, green, blue, indigo, violet).
   - **about 68 to 80: bank 2, numbers** (zero to ten).
   - **about 81 to 93: bank 3, letters** (a to z).
   - **about 94 to 106: bank 4, the NATO alphabet** (alpha to zulu).
   - **about 107 to 127: bank 5, synthesizer words** (analog, circuit, clock, control, digital, electronic, filter,
     frequency, generator, instrument, knob, machine, modular, modulator, operator, oscillator, patch, sequencer,
     synthesizer, VCA, voltage, waveform).
   - In a word bank MORPH selects the word (left the first, right the last) and TIMBRE shifts the formants; below
     about 51 and above about 76 TIMBRE also stretches the word (slower towards the ends of its travel).
   - MIX (G) to the right: AUX, the excitation alone (the "vocal cords" before the formant filters).
5. **Trigger behaviour.**
   - In a word bank every trig speaks the selected word once, from its start; a trig during a word restarts it.
     After the word the voice is silent until the next trig. Turn MORPH, trig: the new word. Turning MORPH between
     trigs does not change the word being spoken.
   - Holding a trig (or a long note length) changes nothing: the Digitakt gives PLAITS the note-on only, and
     Plaits' speech engine ignores how long its gate stays high too. The word plays at the note's pitch, in a
     monotone (Plaits' intonation and speed attenuverters are at noon here; there is no knob for them).
   - Where the LPC vowels sound (about 21 to 55, crossfaded with SAM below about 42) each trig puts a short random
     consonant (about 25 ms) before the vowel.
   - In the naive and SAM range (0 to about 21) a trig only starts the amp envelope: the vowel runs on (as on Plaits
     with its TRIG input patched).
   - Turning HARMONICS from a word bank down into the LPC vowels without a trig goes straight back to the vowels
     (Plaits reads past the end of a table there; the port stops the word instead).
6. **Sequenced words.** HARMONICS at a bank (e.g. about 74, numbers), trigs on steps 1, 5, 9, 13 with MORPH p-locked
   to different values on each: PLAY speaks a different word on each step. Then MODEL p-locks across models (e.g.
   MODEL 7 on steps 1 and 9, MODEL 0 and 5 on the others): each note plays its model.
7. **First note after a MODEL change.** The first note after a MODEL change starts the engine from Plaits' power-up
   state and can tick once (WAVETABLE and CHORD, as on H3d; on SPEECH in the naive range at high notes, about 3.5
   times the steady step). This is expected: do not report it unless it is more than a single tick.
8. Save the project with MODEL 7 on track 1, reboot, load it: MODEL is back at 7 and SPEECH plays.
9. **Track rule (unchanged): never more than 1 track triggered as PLAITS since boot on this image,** any model.
   One SPEECH track at its worst knobs (HARMONICS near 0, TIMBRE at the right, the top of the pitch range) measures
   54.3 % of the DSP block at the high bound and two 61.0 % (finding 06, "PLAITS SPEECH (model 7)"); two ADDITIVE
   tracks still measure 86.6 %, which sets the rule.
- **H3e stalls or is silent** while H3d passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to H3d
  (`plaits-7models-names-splash-48k-1.17.syx`, `8032d086…`) and report which step failed.

## H3f: output stage, velocity, RINGS (12) and TERRAIN (21) (only after H3e passed)

H3f (`out/patch/plaits-10models-names-splash-48k-1.17.syx`, `b3797740…`) is H3e plus Plaits' output stage (each
model's own output level and SPEECH's limiter, as in Plaits), velocity, and models 12 RINGS and 21 TERRAIN. The other
values from 8 to 23 still play silence.

1. **Boot and a new project.** Flash H3f and power on: the splash, then the unit boots. Create a new project (an
   older project may not play). FUNC+SRC, 7 x DOWN (PLAITS), YES on track 1. Make sure track 2 has a sample (if
   trigging it is silent, load any sample onto track 2 first), then check that it plays.
2. **Loudness across models.** At default knobs trig track 1 with MODEL 0 to 7: VA is as loud as on H3e; the others
   are equal or up to about 3 dB quieter (Plaits' own balance: ASYM-TRI, FORMANT and TERRAIN -1.2 dB, 2FM and
   WAVETABLE -2.5 dB, SPEECH -3.2 dB). **SPEECH is no longer much louder:** MODEL 7, HARMONICS 0, TIMBRE and MORPH
   at the left, a high note (e.g. TUNE up): on H3e this peaked about 12 dB over VA; now it stays at about VA's
   level (Plaits' limiter; a short peak at the first note after a model change is the limiter catching up).
3. **RINGS: velocity.** MODEL to 12 (`RINGS`). Give the amp envelope a long decay. Trig with step velocities 127,
   then 30 (or a velocity p-lock): 127 strikes louder and brighter and rings longer; 30 is soft and dull. The
   default velocity 100 sounds like Plaits with nothing in its LEVEL input. HARMONICS (C) bends the partials
   (around 38 harmonic, left and right inharmonic/bell-like), TIMBRE (E) the brightness, MORPH (F) the ring length.
4. **RINGS: fast retriggers.** MORPH to the right (long ring), velocity 127, trigs on every step at a fast tempo
   (or retrig 1/32): each trig strikes again on top of the ring, with no runaway loudness, no crackle and no
   silence; the level settles. Then MORPH to the left: short plucks.
5. **TERRAIN's knobs.** MODEL to 21 (`TERRAIN`). HARMONICS walks through eight terrains (smooth crossfades; the
   last third are the wavetable data read as terrains, brighter and buzzier); TIMBRE widens the path (left a near
   sine, right rich); MORPH moves the path's centre. MIX (G) to the right: AUX, a phase-modulated sine. After big
   jumps (a high note with TIMBRE right, then a low note) TERRAIN can turn rougher and louder until the next big
   change: Plaits does the same (its path leaves its terrain), clipped to full scale. Report it only if it
   crackles or drops out.
6. **First note after a MODEL change.** As on H3e it can tick once (WAVETABLE's is now clipped to full scale, as in
   Plaits). Do not report a single tick.
7. **Track rule (unchanged): never more than 1 track triggered as PLAITS since boot,** any model. One RINGS track
   while it rings measures 73.1 % of the DSP block at the high bound (the heaviest model now), one TERRAIN track at
   its worst knobs 65.0 % (finding 06, "PLAITS RINGS and TERRAIN"); two of either would exceed 85 %.
8. Save the project with MODEL 12 on track 1, reboot, load it: MODEL is back at 12 and RINGS plays.
- **H3f stalls or is silent** while H3e passed: recover (hold FUNC at power-on, TRIG 4, send the stock
  `firmware/Digitakt_II_OS1.17.syx` with Transfer's legacy mode over MIDI DIN), go back to H3e
  (`plaits-8models-names-splash-48k-1.17.syx`, `8701a9bf…`) and report which step failed.

## Known, not faults

- A triggered PLAITS voice keeps rendering until its track is triggered under another machine; the amp envelope
  silences it. That is why the track limits above count tracks triggered as PLAITS since boot.
- A project saved with PLAITS loads on stock 1.17 without a fault, with that track as ONESHOT (type 0): the stock
  loader keeps only the stock machine types.
- The track limits come from the DSP load measured in the emulator with the manual's stall rules (finding 06,
  "PLAITS: measured DSP load per block" and the later model sections), kept at or under 85 % of the block at the
  worst-case parameters: 2 tracks on H2 and H3, 1 track on H3b, H3c, H3d, H3e (ADDITIVE sets it) and H3f (RINGS, TERRAIN and ADDITIVE).
