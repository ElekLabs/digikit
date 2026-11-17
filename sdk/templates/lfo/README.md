# __TITLE__

__ID__: an LFO waveform for the Digitakt II and the Digitone II, loaded at run time. The module adds one value to the WAVE parameter of the three LFOs of every
track: the MOD page shows its name, the graph draws it, and the LFO tick reads its 32 samples with linear interpolation. Replace the sine in `src/module.c` with your own samples.

- `module.toml`: the module and its record `__NAME__` (the key of the shape derives from the module id and this id).
- `src/module.c`: `init` adds the record with `dk_record_add`: the key, a 4-character name and 32 samples of `int16` (Q15).

The shape needs `lfo.dks` on the card (and `core/lfo` in the firmware image): the `lfo` service gives the shape a wave number from 7 up (kept across boots) and
keeps it while the module is loaded. A sound that names the number shows ERR and runs like RAND on a device without the service, and as the triangle (TRI) when the module is not loaded.
Never rename the module id or the record id: sounds keep the shape by its number, and the number follows the key.

```sh
mise run module:check -- __ID__    # build and check on both devices
mise run module:pack -- __ID__ --device dt2     # make the .dkm (or --device dn2)
```
