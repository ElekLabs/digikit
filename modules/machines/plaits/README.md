# Plaits

PLAITS brings Mutable Instruments Plaits to the Digitakt II as a machine. This version has two models, numbered in
Plaits' own order (0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM, 9 NOISE,
10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18-20 DX7 1-3, 21 TERRAIN,
22 STRING MACHINE, 23 SQUARE; owner, 2026-10-17):
- 0 VA (virtual analog): a variable square plus a variable saw on OUT, a "monster sync" pair on AUX.
- 1 ASYM-TRI (waveshaping): an asymmetric triangle through a waveshaper and a wavefolder on OUT; on AUX a sine that
  TIMBRE turns into a second wavefolder curve.
It plays at the track's note and TUNE, and goes through the track's amp envelope, filter, sends and FX like a sample.
Pick it with FUNC+SRC (the 8th entry).

- Devices and firmware: Digitakt II 1.17.
- SRC page: A TUNE, B MODEL (0 VA, 1 ASYM-TRI; 2 to 23 are the later models and play silence), C HARMONICS (VA: detune
  between the two oscillators; ASYM-TRI: the waveshaper's curve), E TIMBRE (VA: square width and sync; ASYM-TRI: the
  wavefolder amount), F MORPH (VA: saw shape; ASYM-TRI: the triangle's asymmetry), G MIX (OUT to AUX), H LEV. Knob D
  is unused. Every knob is a normal parameter row: p-lockable and an LFO destination.
- MODEL shows the model's name, not its number, in the header while the knob is touched or turned (`Model=ASYM-TRI`).
  The names, by value: 0 VA, 1 ASYM-TRI, 2 2FM, 3 FORMANT, 4 ADDITIVE, 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM,
  9 NOISE, 10 DUST, 11 STRING, 12 RINGS, 13 KICK, 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18 DX7 1, 19 DX7 2,
  20 DX7 3, 21 TERRAIN, 22 STRING MACH, 23 SQUARE. They come from a small ColdFire function of this module
  (src/cf/model_fmt.s, `plaits_model_fmt`), which core/params' `formatter` field puts in MODEL's parameter row. The knob
  slot still draws LEV's fader (MODEL's template). Tests: tests/patchkit/test_plaits_model_names.py and the harness
  test tests/model_names.toml.
- A note-on restarts the engine and latches MODEL: a MODEL change (knob or p-lock) takes effect at the next note-on,
  which initialises that model's engine (the engines share one state per voice). Velocity does nothing in either
  model. Model 0 renders bit for bit as in the H2 image (VA only).
- The engines are the Plaits C++ source translated to C (MIT, Emilie Gillet), rendered at exactly 48 kHz (Plaits'
  own pitch reference of 47,872.34 Hz is replaced by 48,000 Hz, so notes are in tune), in two sub-blocks of 16
  samples per Digitakt block, with a 5 Hz DC blocker. Plaits' internal envelope, low-pass gate and modulation
  amounts are left out on purpose: the Digitakt's envelopes and LFOs do that work.
- The DSP code comes from one of two modules, which pull this one in: machines/plaits-48k (the default: 48 kHz
  straight into the voice output) or machines/plaits-96k (interpolated to 96 kHz, then the stock decimator).
  Selected alone, this module gives the track the PLAITS name and rows but plays it as MANUAL SLICE.
- Known limits: two models; a triggered PLAITS voice keeps rendering after its note (the amp envelope silences it),
  so each PLAITS track that has played costs DSP time every block. The rule: **never more than N tracks triggered as
  PLAITS since boot**, N = 2 with the 48 kHz build and N = 1 with the 96 kHz build. A track stops costing DSP only
  after it has been triggered under another machine (the type reaches the DSP only on a trig). Worst case on the
  concrete core: VA 76,249 instructions per track per block at 48 kHz and 99,784 with the 96 kHz build, ASYM-TRI
  42,307 and 65,842 (the block is 666,667 cycles at 1 GHz); VA is the heaviest model and sets the limits; the DSP load measured in the emulator with the manual's stall rules is in
  docs/findings/06-sharc-engine-and-startup.md ("PLAITS: measured DSP load per block" and "PLAITS model 1
  (ASYM-TRI): ..."): two ASYM-TRI tracks measure 71.5 % of the block at the high bound (two VA tracks 73.4 %); the
  limits hold for any mix of the two models. Two tracks with the 96 kHz
  build go over the 85 % budget of tools/patchkit/pk_dsp_budget.py; 16 tracks do not fit yet.
- Hardware test (the owner's H1/H2 decision tree, the image hashes and patch commands): HARDWARE-TEST.md.
- A project saved with PLAITS loads on a stock 1.17 without a fault, but that track comes back as ONESHOT (type 0):
  the stock loader keeps only the stock machine types.
- Tests: tests/plaits/test_va_host.py and test_ws_host.py (host, bit exact against upstream Plaits),
  tests/patchkit/test_plaits_module.py (compiled code on the concrete core, the images), test_plaits_model0_h2.py
  (model 0 unchanged since H2), tools/patchkit/pk_proof_plaits.py (emulator). `mise run verify`
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
