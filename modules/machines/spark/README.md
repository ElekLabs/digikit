# SPARK

digikit/spark: a stereo sample-playback machine for the Digitakt II and the Digitone II, loaded at run time. It plays the sample bound to its
track once per note (a one-shot) through the per-track sample table of API 1.5 (`<dk/sample_track.h>`). It never selects a file: the runtime or a
picker binds a PCM16 file (mono or stereo) to a track, SPARK reads it.

| Word | Parameter | Range and default | Meaning |
| --- | --- | --- | --- |
| 0 | START | 0 to 127, default 0 | first frame, spread over the whole sample: frame = round(frames * v / 127) |
| 1 | END | 0 to 127, default 127 | end frame (exclusive) the same way; END at or below START plays nothing |
| 2 | REVERSE | 0 to 127, default 0 | 64 and above plays backwards from END - 1 to START |
| 3 | PITCH | 0 to 127, default 64 | 64 is the played note unchanged; each unit is 3/8 semitone, so -24 to +23.6 semitones |

- Pitch: MIDI note 60 plays the sample at its own speed; the cursor rate is `srcrate / 48000 * 2^(semitones / 12)` in Q16.16, limited to 1/65536 to 64
  frames per sample. START, END and REVERSE are read at note-on; PITCH and the note follow every block.
- Stereo: a stereo sample gives distinct L and R through machine contract 1.2 (`out_r`). A mono sample plays centred (`base_r = base_l`). On the
  Digitakt II R goes to the odd voice of the stock voice pair. On the Digitone II only L is heard until the shadow right-channel stages are
  production (adapter TODO).
- Lifecycle: `note_on` copies the track's `dk_strack` record under the admission and epoch macros; every `render` checks the epoch and acquires and
  releases the track. A stale token (select, unbind, stock reset) or a track that is not ready gives a silent block and `DK_DONE`.
- Packing: `patchkit pack digikit/spark --device dt2|dn2` carries the track table in the DSP section (kind 2 record) and declares API 1.5 and machine
  contract 1.2. The runtime reserves it when the module loads and writes the records over the DSP link once the DSP image is in.
- Selecting a sample (test route, no picker yet): the fixture `test/track-select` (`tests/fixtures/modules/track-select`) calls `sample_select` for
  the first table in the image (handle 1). Request 0x3e with body `[1, track, ...path bytes]` selects (status 0 = queued); request 0x3f with body
  `[1, track]` returns the BE32 status (3 = ready) and, when ready, frames, rate, channels, token.

- `src/module.c`: the machine, parameters, page and the track table declaration; `dkm_init` resolves the table handle.
- `src/dsp/spark.s`: `render`, `note_on`, the table storage and the descriptor.
- `tests/test_spark_playback.py`: the DSP on the concrete core through the real adapter, against a float32 model.

```sh
mise run module:check -- digikit/spark
```
