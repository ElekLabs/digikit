# Plaits

PLAITS brings Mutable Instruments Plaits to the Digitakt II as a machine. This first version has one model, VA
(virtual analog): a variable square plus a variable saw on OUT, a "monster sync" pair on AUX. It plays at the track's
note and TUNE, and goes through the track's amp envelope, filter, sends and FX like a sample. Pick it with FUNC+SRC
(the 8th entry).

- Devices and firmware: Digitakt II 1.17.
- SRC page: A TUNE, B MODEL (0 = VA; other numbers are reserved for later models and play silence), C HARMONICS
  (detune between the two VA oscillators), E TIMBRE (square width and sync), F MORPH (saw shape), G MIX (OUT to AUX),
  H LEV. Knob D is unused. Every knob is a normal parameter row: p-lockable and an LFO destination.
- A note-on restarts the engine and latches MODEL. Velocity does nothing in VA.
- The engine is the Plaits C++ source translated to C (MIT, Emilie Gillet), rendered at exactly 48 kHz (Plaits'
  own pitch reference of 47,872.34 Hz is replaced by 48,000 Hz, so notes are in tune), in two sub-blocks of 16
  samples per Digitakt block, with a 5 Hz DC blocker. Plaits' internal envelope, low-pass gate and modulation
  amounts are left out on purpose: the Digitakt's envelopes and LFOs do that work.
- The DSP code comes from one of two modules, which pull this one in: machines/plaits-48k (the default: 48 kHz
  straight into the voice output) or machines/plaits-96k (interpolated to 96 kHz, then the stock decimator).
  Selected alone, this module gives the track the PLAITS name and rows but plays it as MANUAL SLICE.
- Known limits: one model; a triggered PLAITS voice keeps rendering after its note (the amp envelope silences it),
  so each PLAITS track that has played costs DSP time every block until its type changes. Worst case measured on the
  concrete core: about 76,000 instructions per track per block at 48 kHz and about 100,000 with the 96 kHz build
  (the block is 666,667; stock uses about 44 % with stalls). That allows about 3 PLAITS tracks at once with the
  48 kHz build and 2 with the 96 kHz build; 16 tracks do not fit yet.
- A project saved with PLAITS loads on a stock 1.17 without a fault, but that track comes back as ONESHOT (type 0):
  the stock loader keeps only the stock machine types.
- Tests: tests/plaits/test_va_host.py (host, bit exact against upstream Plaits; the oracle needs
  `PLAITS_STMLIB=<stmlib checkout at e3bd7c9>` when ../eurorack/stmlib is empty), tests/patchkit/test_plaits_module.py
  (compiled code on the concrete core, the images), tools/patchkit/pk_proof_plaits.py (emulator).

The gallery shows `summary` from module.toml and `gallery/icon.svg`. Keep this file free of firmware bytes and vendor
tool names.
