# Inline tone (test)

A test machine, ITONE, for the first hardware run of core/dsp-inline (image H1). It is a clone of MANUAL SLICE whose
SRC page shows only TUNE and LEV. With one of its two routine modules a trig plays a sine at the track's note: note
60 with TUNE at rest is 261.6 Hz, and TUNE moves it in semitones (p-locks included). The amp envelope, filter and FX
apply as for a sample.

- Devices and firmware: Digitakt II 1.17.
- The routine (src/dsp/tone.s) comes in two builds that test the hook's two output modes:
  test/inline-tone-48k (renders 48 kHz straight into the voice output, image h1b) and test/inline-tone-96k (renders
  96 kHz into the work buffer, then the stock decimator, image h1a). Select one of them; it pulls this module in.
- Selected alone, ITONE plays like MANUAL SLICE.
- Known limits: a test tone only, no parameters besides TUNE and LEV.

The gallery shows this text. Keep it free of firmware bytes and vendor tool names.
