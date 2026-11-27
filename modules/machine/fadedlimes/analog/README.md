# Analog

**Category:** machines  **Devices:** Digitakt II 1.17; Digitone II 1.12  **Status:** experimental (emulator only)  **Kind:** run-time module (`.dkm`, loaded from the card; the runtime gives it its machine type)

A subtractive synth voice for the Digitakt II's tracks: two sawtooth oscillators through a wave-shaping slider, a
detune that goes from unison all the way to a reese, a wave folder and a sub. It runs before the track's filter, so the
FLTR and AMP pages do the shaping exactly as they would on a sample — no single-cycle waveforms to load.

| knob | range |
|---|---|
| **DET** | 0 = two saws in unison · full = 3 % apart, the rolling reese beat |
| **WAVE** | one slider: saw → triangle → pulse (width 5–50 %) → square |
| **SUB** | sub level over 0 → ¾ · wave-folder drive over ¾ → 1 |

The folder is a reflection, not a clipper: the overshoot folds back through the wave, so it adds harmonics and grit
rather than the rounded peaks a saturator gives. Push **SUB** past ¾ for it.

The oscillator level is set by **LEV** and the pitch by **TUNE**, the stock knobs at the ends of the row.

Two oscillators, one wave slider, a folder and a sub — all before the filter. Try **DET** at ¾ with **WAVE** on square
and a short filter envelope for a reese bass, or **WAVE** on triangle with a long attack for pads that will not sit
still.
