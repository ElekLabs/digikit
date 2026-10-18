# Plaits

PLAITS brings Mutable Instruments Plaits to the Digitakt II as a machine. This version has seven models, numbered in
Plaits' own order (0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM, 9 NOISE,
10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18-20 DX7 1-3, 21 TERRAIN,
22 STRING MACHINE, 23 SQUARE; owner, 2026-10-17):
- 0 VA (virtual analog): a variable square plus a variable saw on OUT, a "monster sync" pair on AUX.
- 1 ASYM-TRI (waveshaping): an asymmetric triangle through a waveshaper and a wavefolder on OUT; on AUX a sine that
  TIMBRE turns into a second wavefolder curve.
- 2 2FM (two-operator FM): a carrier and a modulator with feedback on OUT; on AUX a sub-oscillator an octave down.
- 3 FORMANT (granular formant oscillator): two formants made of windowed sine grains on OUT; on AUX windowed sines
  that imitate filtered waveforms (Braids' Z models).
- 4 ADDITIVE (harmonic oscillator): 24 harmonics shaped by a spectral bump on OUT; on AUX only the harmonics of a
  Hammond organ's drawbars.
- 5 WAVETABLE: 4 banks of 8 x 8 waves scanned in three dimensions on OUT; on AUX the same quantised to 5 bits.
- 6 CHORD: a four-note chord from Plaits' chord bank on five voices, divide-down organ/string registrations turning
  into wavetable voices as MORPH rises; on AUX the chord's root.
It plays at the track's note and TUNE, and goes through the track's amp envelope, filter, sends and FX like a sample.
Pick it with FUNC+SRC (the 8th entry).

- Devices and firmware: Digitakt II 1.17.
- SRC page: A TUNE, B MODEL (0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD; 7 to 23 are the
  later models and play
  silence), C HARMONICS, E TIMBRE, F MORPH (per model as in the Plaits manual:
  VA detune / square width and sync / saw shape; ASYM-TRI waveshaper curve / wavefolder amount / asymmetry;
  2FM frequency ratio / modulation index / feedback; FORMANT formant ratio / formant frequency / formant width and
  shape; ADDITIVE number of spectral bumps / most prominent harmonic / bump shape; WAVETABLE bank / row / column;
  CHORD chord / inversion and transposition / waveform), G MIX (OUT to AUX), H LEV. Knob D
  is unused. Every knob is a normal parameter row: p-lockable and an LFO destination.
- MODEL shows the model's name, not its number, in the header while the knob is touched or turned (`Model=ASYM-TRI`).
  The names, by value: 0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM,
  9 NOISE, 10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18 DX7 1, 19 DX7 2,
  20 DX7 3, 21 TERRAIN, 22 STRING MACH, 23 SQUARE. They come from a small ColdFire function of this module
  (src/cf/model_fmt.s, `plaits_model_fmt`), which core/params' `formatter` field puts in MODEL's parameter row. The knob
  slot still draws LEV's fader (MODEL's template). Tests: tests/patchkit/test_plaits_model_names.py and the harness
  test tests/model_names.toml.
- A note-on restarts the engine and latches MODEL: a MODEL change (knob or p-lock) takes effect at the next note-on,
  which initialises that model's engine (the engines share one state per voice, 344 bytes, the size of CHORD's).
  Velocity does nothing in these models. Model 0 renders bit for bit as in the H2 image (VA only), model 1 as in H3,
  models 2 to 4 as recorded for H3b, models 5 and 6 as recorded for H3c (tests/plaits/vectors).
- A note-on restarts the engine from Plaits' own power-up state, so WAVETABLE (and CHORD at a high MORPH and a low
  note) starts each note with Plaits' cold-start transient: about 25 times the steady level for a few samples on
  WAVETABLE (tests/plaits/test_wavetable_chord_host.py `test_note_on_transients`). The engines are bit exact against
  upstream; whether to soften this is the owner's call.
- The engines are the Plaits C++ source translated to C (MIT, Emilie Gillet), rendered at exactly 48 kHz (Plaits'
  own pitch reference of 47,872.34 Hz is replaced by 48,000 Hz, so notes are in tune), in two sub-blocks of 16
  samples per Digitakt block, with a 5 Hz DC blocker. Plaits' internal envelope, low-pass gate and modulation
  amounts are left out on purpose: the Digitakt's envelopes and LFOs do that work.
- The DSP code comes from one of two modules, which pull this one in: machines/plaits-48k (the default: 48 kHz
  straight into the voice output) or machines/plaits-96k (interpolated to 96 kHz, then the stock decimator).
  Selected alone, this module gives the track the PLAITS name and rows but plays it as MANUAL SLICE.
- Known limits: seven models; a triggered PLAITS voice keeps rendering after its note (the amp envelope silences it),
  so each PLAITS track that has played costs DSP time every block. The rule: **never more than 1 track triggered as
  PLAITS since boot** with the current build (models 0 to 6; it was 2 on the H2 and H3 images, which had VA and
  ASYM-TRI only), and 1 with the 96 kHz build. A track stops costing DSP only after it has been triggered under
  another machine (the type reaches the DSP only on a trig). Worst case per track per block on the concrete core at
  48 kHz (with the 96 kHz build): VA 76,311 (99,848), ASYM-TRI 42,313 (65,850), 2FM 57,442 (80,979), FORMANT 80,917
  (104,454), ADDITIVE 88,288 (111,825), WAVETABLE 41,168 (64,705), CHORD about 55,000 (about 79,000) instructions;
  the block is 666,667 cycles at 1 GHz. WAVETABLE's reads from its 101 KB wave table make it the heaviest single
  track at the high bound (67.4 %, ADDITIVE 67.1 %), but pairs of it (76.7 %) and of CHORD (70.5 %) fit; ADDITIVE sets
  the limit: two ADDITIVE tracks measure 86.6 %, over the 85 % budget of
  tools/patchkit/pk_dsp_budget.py (docs/findings/06-sharc-engine-and-startup.md, "PLAITS: measured DSP load per
  block" and the model 1, models 2 to 4 and models 5 and 6 sections). 16 tracks do not fit yet.
- Hardware test (the owner's H1/H2 decision tree, the image hashes and patch commands): HARDWARE-TEST.md.
- A project saved with PLAITS loads on a stock 1.17 without a fault, but that track comes back as ONESHOT (type 0):
  the stock loader keeps only the stock machine types.
- Tests: tests/plaits/test_va_host.py, test_ws_host.py, test_fm_grain_additive_host.py and
  test_wavetable_chord_host.py (host, bit exact against
  upstream Plaits), tests/patchkit/test_plaits_module.py (compiled code on the concrete core, the images),
  test_plaits_model0_h2.py and test_plaits_model_digests.py (each model unchanged since it was first enabled),
  tools/patchkit/pk_proof_plaits.py (emulator). `mise run verify`
  runs tests/plaits (suite "plaits") whenever this module, tests/plaits or tests/support/plaits_* change.
- The bit-exact oracle builds upstream Plaits from ../eurorack (the Mutable Instruments repository next to this one)
  and its stmlib submodule at commit e3bd7c9. Provide stmlib in one of two ways:
  `git -C ../eurorack submodule update --init stmlib` (offline from a local clone: `git -C ../eurorack submodule init
  stmlib`, `git -C ../eurorack config submodule.stmlib.url <clone>/stmlib`,
  `git -C ../eurorack -c protocol.file.allow=always submodule update stmlib`, then set the URL back), or
  `PLAITS_STMLIB=<directory holding a stmlib/ checkout at e3bd7c9>`. Without stmlib the oracle tests skip;
  `PLAITS_ORACLE_REQUIRED=1` turns that skip into a failure, and verify's python-tests step sets it (override with
  `PLAITS_ORACLE_REQUIRED=0`).

The gallery shows `summary` from module.toml and `gallery/icon.svg`. Keep this file free of firmware bytes and vendor
tool names.
