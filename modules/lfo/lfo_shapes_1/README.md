# LFO Shapes 1

digikit/lfo_shapes_1: LFO shapes for the Digitone II and the Digitakt II, loaded at run time. Each shape adds one value to the WAVE parameter of the three LFOs of every
track: the MOD page shows its name, the graph draws it, and the LFO tick reads its 32 samples with linear interpolation (the last sample leads back to the first).

- `module.toml`: the module and one `[[record]]` per shape. The key of a shape derives from the module id and the record id (`DK_KEY_<ID>` in `build/gen/module_ids.h`).
- `src/module.c`: the table `SHAPES` (a 4-character name and 32 `int16` samples, Q15, one period) and `KEYS` (the key of each shape, same order). `init` adds one record per shape with `dk_record_add` and stops at the first error.

## Shapes

The samples in `src/module.c` are literal numbers. This table gives each shape's name, the three characters the Digitone II shows, and its formula for `i` = 0 to 31
(`round` to the nearest integer; amplitude 30000):

| Name | 3 characters | Formula |
| --- | --- | --- |
| STP4 | STP | `round(30000 * (2 * floor(i / 8) / 3 - 1))`: a staircase up in 4 levels (-30000, -10000, 10000, 30000) |
| PULS | PUL | `30000` for i < 8, else `-30000`: a 25% pulse |
| HSIN | HSI | `round(30000 * sin(2 * pi * i / 32))` for i < 16, else `0`: a half sine |
| ABSN | ABS | `round(30000 * abs(sin(2 * pi * i / 32)))`: a rectified sine |
| SOFT | SOF | `round(30000 * clamp(1.6 * sin(2 * pi * i / 32), -1, 1))`: a soft square |
| DUCK | DUC | `round(30000 * (1 - 2 * exp(-5 * i / 32)))`: a sidechain duck |
| WOBL | WOB | `round(30000 * (sin(2 * pi * t) + 0.4 * sin(6 * pi * t)) / 1.4)`, t = i / 32 |

To add a shape: add a `[[record]]` with a new id to `module.toml`, its `DK_KEY_<ID>` to `KEYS` and an entry to `SHAPES`, and a row to this table. Run `mise run gen:modules`.

## Rules

`mise run module:check -- digikit/lfo_shapes_1` runs `init` in the emulator and checks the records with the rule that the `lfo` service uses:

- 1 to 8 shapes in a module. The service keeps 8 shapes in total over all modules; a ninth is refused at load.
- A name has 4 characters, space to tilde, and is used once. No two names share their first 3 characters (the Digitone II shows 3).
- A shape is not flat (all samples equal) and not all zero.

A shape that has no negative sample is unipolar (like the stock EXPO and RAMP).

The shape needs `lfo.dks` on the card (and `core/lfo` in the firmware image): the `lfo` service gives each shape a wave number from 7 up (kept across boots) and
keeps it while the module is loaded. A sound that names the number shows ERR and runs like RAND on a device without the service, and as the triangle (TRI) when the module is not loaded.
Never rename the module id or a record id: sounds keep a shape by its number, and the number follows the key.

```sh
mise run module:check -- digikit/lfo_shapes_1                  # build, run init in the emulator, check the shapes
mise run module:pack -- digikit/lfo_shapes_1 --device dt2      # make the .dkm (or --device dn2)
```
