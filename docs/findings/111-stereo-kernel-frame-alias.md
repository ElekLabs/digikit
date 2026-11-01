# Stereo PCM16 kernel and the adapter's byte-alias frame

**[D]** Found while building `digikit/spark` on the concrete SHARC+ core through `core/dsp-voice` (model tests, no emulator).

- `_dk_sample_pcm16_stereo_linear` (`sdk/dsp/sample_pcm16_stereo.inc`) keeps the right channel's copy of the cursor on the stack. It computes the
  address as `I6 - 11` in word units. Its own test (`tests/patchkit/test_dsp_sample_stereo_kernels.py`) uses a normal-word frame, so it passes.
- The adapters enter a module with a byte-alias `I6` (byte addresses, `0x2009...`; see `tests/fixtures/modules/sample-proof`, "actual adapters
  enter through JUMP with byte-alias I6"). `I6 - 11` is then 11 bytes below the frame: the right call reads a misplaced cursor, finishes at
  once, and R is silent. The left channel is correct.
- Fix used in SPARK: no stereo kernel. It copies the cursor to state words 16 to 21 and calls `_dk_sample_pcm16_linear` twice (L with the cursor, R with
  the copy). The result equals the stereo kernel's contract (same cursor, separate outputs); the test compares both channels with the float32 model.
- Open: either change the stereo kernel to take its cursor copy from a caller buffer (the golden hashes and the kernel test change), or keep it
  documented as normal-word only. Pointer arithmetic on the state pointer or `I6` in a module must count bytes (a word is 4); `DM(n, I)` offsets
  count words.
- Related adapter facts used by SPARK: the state pointer (`R4`) and `in->out` are byte addresses; a `dk_strack` record and the PCM planes are
  normal-word addresses (`LSHIFT BY -2` of a data symbol gives one).
