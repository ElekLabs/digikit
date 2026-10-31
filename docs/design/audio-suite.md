# Audio suite: one set of audio readers

Three tools read rendered audio today. This note says what each one does, and how they become one set in
`tools/emulator/`, in Python.

## The three tools

| Measure | dn2_firmware `dnfw wav` | moveforge `palette.ts` | digikit `audiocheck.py` |
|---|---|---|---|
| Pitch, and the power between its harmonics | yes | no | harmonic levels of a decaying tone |
| Power above 20 kHz, clipping, DC | yes | no | DC, level |
| Clicks (isolated steps) | yes | no | onsets |
| Two renders, sample for sample | yes | no | no |
| Spectrogram | PNG sheet | no | PNG |
| Knob sweeps and dead controls | no | yes | no |
| Presets that are the same sound | no | yes | no |
| T60, attack, flatness, stereo width | crest, stereo correlation | yes | RT60, L-R difference |
| A page with audio on every row | no | yes | no |

## Why Python

- digikit's audio readers are Python on numpy (`audiocheck.py`, `live_audio.py`, the SHARC harness).
- The renders come from Python and Rust tools: the coupled runner, the SHARC harness, and recordings from a unit.
- numpy gives the FFT. `palette.ts` carries its own FFT.

The palette page stays HTML. Python writes it.

## What this change adds

- `tools/emulator/audiotone.py`: a held note. Pitch, `between_db`, `above_db`, `steps`, `compare` and
  `note_report`. These come from dn2_firmware's `dnfw.audio.metrics`.
- `tools/emulator/palette.py`: the palette. `describe`, `fingerprint`, `travel`, `is_dead`, `same_voice`, and
  `palette.md` and `palette.html` from a manifest of WAVs. The measures, the thresholds and the report follow
  `palette.ts` and `lib/descriptors.ts` (moveforge 3edfa17).
- `tests/dsp/test_audiotone.py` and `tests/dsp/test_palette.py`, on synthetic signals.

## The polyBLEP table as a regression test

dn2_firmware chose a polyBLEP for the Waverider sub on one measure. That measure is the power between the sub's
harmonics, note by note, with and without the polyBLEP. `between_db` is that measure. The test rebuilds both columns of that
table to 1 dB, from a naive square and a textbook polyBLEP square:

| Note | Sub Hz | Naive | polyBLEP |
|---|---|---|---|
| A3 | 110 | -27 dB | -39 dB |
| C5 | 262 | -24 dB | -40 dB |
| A5 | 440 | -26 dB | -63 dB |
| C7 | 1,047 | -17 dB | -32 dB |

A change to the measure that reads the table differently fails the test. As a control, a harmonic band of
±0.05 × f0 in place of ±0.12 × f0 moves 6 of the 14 notes by more than 1 dB.

`palette.ts` cannot make this reading. It does not detect pitch, and its spectrum bars and flatness show a bright
square with or without aliasing. It also reads the first 250 ms of a hit, which suits drums but not a held note.

## Next steps

1. dn2_firmware: `dnfw wav` calls these modules in place of its own copies. dn2_firmware has two definitions of
   "between the harmonics" (`metrics.inharmonic_db` and the page script's `_between_db`). The one in
   `audiotone.between_db` is the one that the polyBLEP decision used, and the other one goes.
2. A renderer writes a palette manifest: the coupled runner for a machine, the SHARC harness for a DSP module.
   The sweep needs each parameter's range and default from the module's own description.
3. dn2_firmware's plot sheet (waveform, close-up, spectrum, spectrogram) joins `audiocheck.plots`.

## Open question

moveforge has no licence file. `palette.py` is new Python code, but it follows `palette.ts` closely. Its author
must agree to this before the merge, or give moveforge a licence that is compatible with AGPL-3.0.
