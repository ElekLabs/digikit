# pd — state, and what the click hunt taught

A phase-distortion voice: the oscillator reads a sine through a warped phase, so the wave leans from a sine to a saw,
a pulse or a resonant formant without ever leaving the harmonic series. Two detuned oscillators, before the track
filter. The runtime assigns the machine type when the module is loaded.

## Panel

| slot | knob | range |
|---|---|---|
| 1 | AMOUNT | 0 = plain sine, 1 = the full warp |
| 4 | CURVE | saw · square (a pulse) · **resonance** (a steep run then a crawl: a moving formant) |
| 6 | DET | 0 = unison, 1 = 3 % apart |

The warp is one shape whose three numbers the CURVE knob picks, all constants because the assembler has no division:

    p' = p + AMOUNT * ( min(s1 p, 0.5) + s2 max(p - b2, 0) - p )

    saw        s1 2   s2 2/3   b2 1/4
    square     s1 2   s2 2     b2 3/4
    resonance  s1 6   s2 6/11  b2 1/12      h6 twelve times the saw's, where 1/(2 b2) predicts

The sine is the SDK fixture's odd polynomial: fold the phase to +-0.25 with CLIP, then a Horner in the squared fold.

## The click, in full, because it cost four builds

**Symptom:** a faint broadband tick at every note-on, audible on a sine-like patch, masked on a saw, present with
portamento, and visible in a spectrogram as a dim vertical spike with the tone unbroken through it.

**Cause:** the oscillators were left free-running (`state_clear_bytes = 0`) on purpose, so a note started wherever the
previous one left the phase. Measured: the first sample of a retriggered pure sine was **-0.2164**, against
**-0.0000** for the first note. Under a fast amp attack that is a step from silence to full amplitude. A saw masks it
because a saw is already at its edge every cycle — which is why it looked like a PD problem and was not.

**Fix:** `state_clear_bytes = 8` — the two phase words are zeroed at note-on, so a note starts at `sin(0) = 0`, the
same starting condition a sample has. The curve numbers sit past the cleared words and survive. `note_on` is a no-op.

**What went wrong on the way, in the order it happened:**

1. A level ramp was added to mask the supposed phase jump. It started at zero, so it dropped a *sounding* note to
   silence — measured 0.191 step against 0.007 of natural motion. The ramp *was* a click, for three builds.
2. Confusingly, the rig could never reproduce the original click: it honours `state_clear_bytes` and the phase really
   is continuous there. The bug only existed on hardware, where a machine starts a note from silence.
3. Every theory about wiped state, curve resets and stale builds was wrong. The one measurement that mattered —
   "what is the first sample of a retriggered note?" — would have found it in the first hour.

**Lesson:** when a machine's output should be silent-then-sounding, measure its *first sample* against zero. A sample
player starts at zero; an oscillator does not, unless you make it.
