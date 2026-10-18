# PLAITS on the unit: H1 (the hook), H2 (the machine) and H3 (a second model)

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
| `out/patch/plaits-2models-48k-1.17.syx` (H3) | `68fee193f40a611a58613409b3e9c4a81fa242fcf3b09355fa7f073d75d236e4` | `uv run python -m tools.patchkit.patchkit patch --firmware firmware/Digitakt_II_OS1.17.syx --modules machines/plaits-48k -o out/patch/plaits-2models-48k-1.17.syx` |
| `out/patch/diag-h1-i13return-48k-1.17.ungated.syx` | `401ccf2be3f2814e4bb79c6aed1872912da4ee73f2010e93c5b387fbf4279cf3` | a copy of the pre-Task-5b h1b; not rebuildable from this branch |
| `out/patch/diag-h1-i13return-96k-1.17.ungated.syx` | `02926b3f833ccdaa51172b98af3de83c4f65779999a3ea901d1a527c6796652c` | a copy of the pre-Task-5b h1a; not rebuildable from this branch |

`mise run patch -- ...` is the same command. The gate runs by default; never add `--no-encoding-gate` for an image
you flash. The H2 hashes above are those of the images after the final fix wave (the stack-switch order was proved
interrupt-safe without a code change, so they equal the images of Task 6).

## The decision tree

1. **Flash h1b** (`6171da82…`, 48 kHz straight into the voice output).
   - Select ITONE on track 1: FUNC+SRC, then 7 x DOWN (ITONE is the 8th entry of the machine list), YES.
   - Check that track 2 still plays its sample.
   - Trig track 1: note 60 should be a 261.6 Hz sine. PLAY, then reboot and check again.
   - P-lock TUNE on one step (hold the trig, turn TUNE): only that step's pitch changes. The routine renders both
     voices of the track in every block, so a releasing voice also follows a TUNE p-lock: the tail of the previous
     note bends to the p-locked step's tune while it decays. That is expected, not a fault.
   - **h1b passes:** go to step 3 (H2 with plaits-48k).
   - **h1b stalls or is silent:** recover, then step 2.
2. **Flash h1a** (`bc6a3314…`, 96 kHz through the stock decimator), the same checks as in step 1.
   - **h1a passes:** go to step 4 (H2 with plaits-96k, one track only).
   - **h1a stalls too:** no H2. Stop and report both H1 results (the hook itself does not run on the unit).
3. **H2 with plaits-48k** (`50743416…`; only after h1b passed).
   1. FUNC+SRC, 7 x DOWN (PLAITS is the 8th entry), YES. Check that track 2 still plays its sample. Trig track 1:
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
   - **plaits-48k passes:** go on to H3 (section "H3: model 1, ASYM-TRI" below), then step 6.
4. **H2 with plaits-96k** (`f37e87a0…`; after h1a passed and h1b did not, or as the optional fallback of step 3), steps
   3.1 to 3.5 and 3.7 on track 1. **Never more than 1 track triggered as PLAITS since boot on this image** (the same
   rule: a track stops costing DSP only after a trig under another machine). Do not run step 3.6: two tracks on this
   image measure 85.6 % of the block at the high bound, over the 85 % budget.
5. **Diagnostics come last,** and only if both h1b and h1a passed. Optional: they tell whether the earlier stalls came
   from the `JUMP (M13, I13)` return.
   - `diag-h1-i13return-48k-1.17.ungated.syx` and `diag-h1-i13return-96k-1.17.ungated.syx`: the same tone, returning
     through `JUMP (M13, I13)`. **FAILS THE ENCODING GATE; may stall; recovery: hold FUNC at power-on, TRIG 4, send the
     stock .syx with Transfer's legacy mode over MIDI DIN.**
   - If they stall while h1a/h1b passed, the stall is pinned on `083f683f`. If one passes, `word = "083f683f"` may be
     added to encoding_proven.toml with that evidence.
6. Record every result (H1, H2 and H3) in the design's hardware table, and the shapes or `word =` entries of each image that passed in
   tools/patchkit/encoding_proven.toml. Decision for PLAITS: h1b passing keeps mode 1 (plaits-48k) as the default;
   otherwise mode 0 (plaits-96k).

## H3: model 1, ASYM-TRI (only after H2 with plaits-48k passed)

H3 (`out/patch/plaits-2models-48k-1.17.syx`, `68fee193…`) is H2 plus model 1, ASYM-TRI (Plaits' waveshaping model).
Model 0 (VA) renders bit for bit as in H2 (tests/patchkit/test_plaits_model0_h2.py). The model numbers now follow
Plaits' own order: 0 VA, 1 ASYM-TRI, 2 2FM, ... 23 SQUARE; 2 to 23 still play silence. Flash it only after step 3
(H2 with plaits-48k) passed; there is no 96k H3 image.

1. Flash H3. FUNC+SRC, 7 x DOWN (PLAITS, the 8th entry), YES on track 1. Check that track 2 still plays its sample.
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

## Known, not faults

- A triggered PLAITS voice keeps rendering until its track is triggered under another machine; the amp envelope
  silences it. That is why the track limits above count tracks triggered as PLAITS since boot.
- A project saved with PLAITS loads on stock 1.17 without a fault, with that track as ONESHOT (type 0): the stock
  loader keeps only the stock machine types.
- The track limits come from the DSP load measured in the emulator with the manual's stall rules (finding 06,
  "PLAITS: measured DSP load per block"), kept at or under 85 % of the block at the worst-case parameters.
