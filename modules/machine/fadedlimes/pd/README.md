# Phase Dist

**Category:** machines  **Devices:** Digitakt II 1.17; Digitone II 1.12  **Status:** experimental (emulator only)  **Kind:** run-time module (`.dkm`, loaded from the card; the runtime gives it its machine type)

Phase distortion, the Casio CZ way of making a filter sweep without a filter: the oscillator reads a sine *through a
warped phase*, so the wave leans from a sine into a saw, a pulse or a resonant formant. The wave stays periodic, so the
harmonics move along the series instead of spreading into the inharmonic sidebands FM produces — it sweeps rather than
rings.

| knob | range |
|---|---|
| **AMOUNT** | 0 = a plain sine · full = the full warp |
| **CURVE** | saw · square (a pulse) · **resonance** — a steep run then a crawl, which puts a moving formant in the sound |
| **DET** | 0 = unison · full = 3 % apart, two warped oscillators beating |

Put an **LFO on AMOUNT** with **CURVE** on resonance: that is the classic CZ sweep, sourced from the Digitakt's own
modulation. P-locks on AMOUNT give you stepped DCW moves.

Two oscillators, a curve, an amount — all before the track filter.
