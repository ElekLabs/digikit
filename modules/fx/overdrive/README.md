# Overdrive flavours

Seven more curves for the tracks' overdrive. The FX page's OVR ROUTING knob becomes OVR TYPE: pre or post filter, as before,
times eight curves. OVERDRIVE still sets the amount, with more at the top: the drive is the stock drive d times 1 + d^2 / 2,
1.5x at full (d is about 0.99 there), 1.03x at half. Every machine gets it, samples included; the master overdrive is
unchanged.

| OVR TYPE | curve, on z = 8 x drive x (the stock curve's small-signal gain) |
|---|---|
| PRE STK, PST STK | the stock curve: clamp to +-1, then y (2 - \|y\|) three times. Old projects (0 / 0x100) load as these |
| TNH4 | tanh(4 z) |
| HARD | clamp(z, -1, 1) |
| SOFT | z / (1 + \|z\|) |
| TUBE | tanh(z + 0.4) - tanh(0.4): asymmetric, even harmonics |
| FOLD | a triangle wavefolder |
| FUZZ | clamp(4 z + 0.3, -0.6, 1) - 0.3: asymmetric, high gain |
| SQR | clamp(64 z, -1, 1): close to a square |

Every curve keeps the stock dry/wet blend (out = x + mix (y - x)) and the stock drive and mix ramps.

How (firmware 1.17, docs in `src/dsp/overdrive.s`):

- **ColdFire:** OVR Routing (parameter 0x103, sound mirror 68) gets the range 0 to 0xf00, the name OVR Type / OD.TY and a
  formatter (`src/cf/fmt.s`), through core/params' `param_edits`. The value is flavour << 9 | post << 8. The knob cell
  still draws the stock PRE / POST FLTR graphic; the header shows the type.
- **DSP, routing:** the DSP copies the frame twice per block in `FUN_1c2b24`; a hook after each copy keeps each track's
  flavour (bits 9-15) and leaves only bit 8 in the frame, so the stock code still sees pre / post.
- **DSP, curves:** the two per-track calls of the stock overdrive (`FUN_1cb3d8`) are retargeted to `_od_sat`. It finds the
  voice from the chain driver's countdown (R14 = 32 - voice) and runs the stock record handling with the boosted drive and
  the flavour's curve (flavour 0 is a copy of the stock curve). Only a block that is not 32 samples goes to the stock routine.

Tests: `tests/test_overdrive.py` runs `_od_sat` and the split on the concrete SHARC core (every curve against a float reference,
the record updates and the boost, the registers it must keep, the stock fallback).
