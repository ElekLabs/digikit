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
  so each PLAITS track that has played costs DSP time until its type changes; 16 tracks at once do not fit yet.
- A project saved with PLAITS loads on a stock 1.17 with the track as MANUAL SLICE.

The gallery shows `summary` from module.toml and `gallery/icon.svg`. Keep this file free of firmware bytes and vendor
tool names.
