# Track flags

Sixteen on/off bits per track that a machine toggles with **FUNC + a knob press** on its SRC page. The page shows `<LABEL> ON` or
`<LABEL> OFF`; the bits are saved with the sound and reach the DSP every block. A bit that is clear is the machine's default, so
old projects and other machines are unaffected.

A machine adds a row per toggle (`[[entry]] registry = "flag_toggles"`: machine type, knob 0..7, bit, label) and reads the word on
the DSP at the frame copy's byte `0xf8 + 0x60 * track` (low half). ParaWT uses bit 0 on WAVE: frame smoothing off.

How (DT2 1.17):

- **Storage:** `Sound[0]` (`Sound + 0x14`), a runtime index no parameter uses. The stock sound saver already writes it to record
  slot 0; stock firmware loads that slot into nothing, so saved projects stay compatible both ways.
- **Load:** a hook after the sound loader's table loop (which fills index 0 with padding) puts record slot 0 back.
- **DSP:** a hook at the end of `vector_191_handler`'s per-track loop copies the word to frame `+0xf8` for tracks whose machine has
  a row. That word is a slice boundary only for MANUAL SLICE (type 6) and its clones, which never have a row.
- **Key:** a hook at the entry of the SRC page's key handler takes a knob press with FUNC held when the track's machine has a row
  for that knob; every other key goes to the stock handler.
