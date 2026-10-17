# Inline tone, 48k (test)

The sine routine of test/inline-tone (ITONE) for core/dsp-inline, in the build that renders 32 samples at 48 kHz
straight into the voice output and skips the stock decimator (it returns through core/dsp-inline's `_di_return_out`). Hardware image h1b.

- Devices and firmware: Digitakt II 1.17.
- Requires test/inline-tone (the machine) and core/dsp-inline; conflicts with the other build.
- Source: modules/test/inline-tone/src/dsp/tone.s, with `TONE_MODE` defined by src/dsp/mode.s.

The gallery shows this text. Keep it free of firmware bytes and vendor tool names.
