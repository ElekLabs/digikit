# digikit/probe

A ColdFire service module that collects the words of a DSP probe module (`test/sharc-probe`) and gives them to a computer. It needs `core/dsp-link` and the
per-block callback `dl_set_tick_cb` of the runtime. In phase 3 of `docs/plans/debug-mode-and-probes.md` it becomes `digikit/debug`.

`dkm_init` sets the callback, which runs in the block interrupt every block with the copy of the previous DSP reply. When a module ran in that record,
the callback keeps diagnostic word 25 (the module step) in a ring of 24,576 words. The ring stops taking words when it is full (overruns are counted). RESET empties it.

| Command | Number | Body | Reply |
| --- | --- | --- | --- |
| STATUS | 0x32 | none | 16 u32 (big endian): format, callback calls, new records, records missed, repeats, records with no module call, words in the ring, overruns, ring size, words checked, frames ok, sequence errors, checksum errors, last frame number, last heartbeat, synced |
| READ | 0x33 | u32 first word, u16 count (at most 200) | u32 words in the ring, u32 first, the words |
| RESET | 0x34 | none | status 0 |

The commands lie in the `probe` range of `sdk/sdk.toml`. STATUS checks the frames (header, size, frame number, checksum) a bounded number of words per request.
The host tool is `tools/device/dkprobe.py`. Unloading the module clears the callback.
