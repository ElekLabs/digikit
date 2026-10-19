# Plaits

PLAITS brings Mutable Instruments Plaits to the Digitakt II as a machine. This version has eight models, numbered in
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
- 7 SPEECH: speech synthesis. HARMONICS crossfades a naive formant synth into a SAM-style synth into LPC10 vowels,
  then selects one of five LPC10 word banks (colours, numbers, letters, the NATO alphabet, synthesizer words); each
  note-on speaks the word MORPH selects. On AUX the excitation (the "vocal cords").
It plays at the track's note and TUNE, and goes through the track's amp envelope, filter, sends and FX like a sample.
Pick it with FUNC+SRC (the 8th entry).

- Devices and firmware: Digitakt II 1.17.
- SRC page: A TUNE, B MODEL (0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH; 8 to 23
  are the later models and play
  silence), C HARMONICS, E TIMBRE, F MORPH (per model as in the Plaits manual:
  VA detune / square width and sync / saw shape; ASYM-TRI waveshaper curve / wavefolder amount / asymmetry;
  2FM frequency ratio / modulation index / feedback; FORMANT formant ratio / formant frequency / formant width and
  shape; ADDITIVE number of spectral bumps / most prominent harmonic / bump shape; WAVETABLE bank / row / column;
  CHORD chord / inversion and transposition / waveform; SPEECH synthesis method and word bank / formant shift
  (species) / vowel or word), G MIX (OUT to AUX), H LEV. Knob D
  is unused. Every knob is a normal parameter row: p-lockable and an LFO destination.
- MODEL shows the model's name, not its number, in the header while the knob is touched or turned (`Model=ASYM-TRI`).
  The names, by value: 0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM,
  9 NOISE, 10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18 DX7 1, 19 DX7 2,
  20 DX7 3, 21 TERRAIN, 22 STRING MACH, 23 SQUARE. They come from a small ColdFire function of this module
  (src/cf/model_fmt.s, `plaits_model_fmt`), which core/params' `formatter` field puts in MODEL's parameter row. The knob
  slot still draws LEV's fader (MODEL's template). Tests: tests/patchkit/test_plaits_model_names.py and the harness
  test tests/model_names.toml.
- The engine runs on from note to note, as in Plaits (Task 9b): a note-on latches MODEL, and initialises the engine
  only at the voice's first note-on since boot and when MODEL changed (a knob or p-lock change takes effect at the
  next note-on). Every other note-on leaves the engine, the DC blocker and the interpolator running; the Digitakt's
  amp envelope shapes the note. The engines share one state per voice (372 bytes, the size of SPEECH's), so a model
  change starts the new engine from Plaits' Init + Reset rather than resuming its old state as Plaits would. The DSP
  does not see a track's machine type change: a track switched away and back resumes its engine where it stopped
  (the engine is paused while the voice is not rendered). Proof: tests/plaits/test_continuous_host.py (bit exact
  against upstream rendered continuously, models 0 to 7).
- A note-on reaches the engine as Plaits' trigger, as Plaits' Voice builds it with its TRIG input patched:
  RISING_EDGE | HIGH in the note-on's first sub-block, then HIGH until the next note-on (the voice routine sees no
  note end; no ported engine reads the gate's length). SPEECH reads it: a note-on speaks the selected word from its
  start, or puts a consonant before an LPC vowel. Velocity does nothing yet: Plaits' accent is fixed at 0.8 (its
  value with LEVEL unpatched; SPEECH's word gain). SPEECH's intonation and speed (Plaits' FM and MORPH
  attenuverters with TRIG patched) are fixed at 0, the attenuverters' centre.
- The first note of a voice (and the first note after a model change) still starts from Plaits' power-up state, so
  WAVETABLE (and CHORD at a high MORPH and a low note) plays Plaits' cold-start transient once: about 25 times the
  steady level for a few samples on WAVETABLE (tests/plaits/test_wavetable_chord_host.py `test_note_on_transients`).
  Later notes of the same model have none. The digests (tests/plaits/vectors) were re-recorded at Task 9b; the first
  note after init renders as on H2 (model 0), H3 (1), H3b (2 to 4) and H3c (5, 6); SPEECH's were recorded at H3e.
- The engines are the Plaits C++ source translated to C (MIT, Emilie Gillet), rendered at exactly 48 kHz (Plaits'
  own pitch reference of 47,872.34 Hz is replaced by 48,000 Hz, so notes are in tune), in two sub-blocks of 16
  samples per Digitakt block, with a 5 Hz DC blocker. Plaits' internal envelope, low-pass gate and modulation
  amounts are left out on purpose: the Digitakt's envelopes and LFOs do that work.
- The DSP code comes from one of two modules, which pull this one in: machines/plaits-48k (the default: 48 kHz
  straight into the voice output) or machines/plaits-96k (interpolated to 96 kHz, then the stock decimator).
  Selected alone, this module gives the track the PLAITS name and rows but plays it as MANUAL SLICE.
- Known limits: eight models; a triggered PLAITS voice keeps rendering after its note (the amp envelope silences it),
  so each PLAITS track that has played costs DSP time every block. The rule: **never more than 1 track triggered as
  PLAITS since boot** with the current build (models 0 to 7; it was 2 on the H2 and H3 images, which had VA and
  ASYM-TRI only), and 1 with the 96 kHz build. A track stops costing DSP only after it has been triggered under
  another machine (the type reaches the DSP only on a trig). Worst case per track per block on the concrete core at
  48 kHz (with the 96 kHz build): VA 76,311 (99,848), ASYM-TRI 42,313 (65,850), 2FM 57,442 (80,979), FORMANT 80,917
  (104,454), ADDITIVE 88,288 (111,825), WAVETABLE 41,168 (64,705), CHORD about 55,000 (about 79,000), SPEECH 34,083
  (57,620) instructions;
  the block is 666,667 cycles at 1 GHz. WAVETABLE's reads from its 101 KB wave table make it the heaviest single
  track at the high bound (67.4 %, ADDITIVE 67.1 %), but pairs of it (76.7 %) and of CHORD (70.5 %) fit; ADDITIVE sets
  the limit: two ADDITIVE tracks measure 86.6 %, over the 85 % budget of
  tools/patchkit/pk_dsp_budget.py (docs/findings/06-sharc-engine-and-startup.md, "PLAITS: measured DSP load per
  block" and the model sections after it). 16 tracks do not fit yet.
- SPEECH's output is not limited: Plaits passes its OUT through a limiter (and every model through its own output
  gain), which the port leaves out for all models; at its loudest corners (HARMONICS 0 or about 42, TIMBRE and MORPH
  at the left, a high note) SPEECH's OUT peaks at about 2.2 at the voice output, where default VA peaks at 0.52
  (its typical level is VA's: RMS 0.1 to 0.4 against 0.29).
- Hardware test (the owner's H1/H2 decision tree, the image hashes and patch commands): HARDWARE-TEST.md.
- A project saved with PLAITS loads on a stock 1.17 without a fault, but that track comes back as ONESHOT (type 0):
  the stock loader keeps only the stock machine types.
- Tests: tests/plaits/test_va_host.py, test_ws_host.py, test_fm_grain_additive_host.py,
  test_wavetable_chord_host.py and test_speech_host.py (host, bit exact against
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
