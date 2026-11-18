# __TITLE__

__ID__: LFO shapes for the Digitone II and the Digitakt II, loaded at run time. Each shape adds one value to the WAVE parameter of the three LFOs of every
track: the MOD page shows its name, the graph draws it, and the LFO tick reads its 32 samples with linear interpolation (the last sample leads back to the first).

- `module.toml`: the module and one `[[record]]` per shape. The key of a shape derives from the module id and the record id (`DK_KEY_<ID>` in `build/gen/module_ids.h`).
- `src/module.c`: the table `SHAPES` (a 4-character name and 32 `int16` samples, Q15, one period) and `KEYS` (the key of each shape, same order). `init` adds one record per shape with `dk_record_add` and stops at the first error.

## Shapes

The samples in `src/module.c` are literal numbers. This table gives each shape's name, the three characters the Digitone II shows, and its formula for `i` = 0 to 31
(`round` to the nearest integer):

| Name | 3 characters | Formula |
| --- | --- | --- |
| SINE | SIN | `round(32767 * sin(2 * pi * i / 32))` |

To add a shape: add a `[[record]]` with a new id to `module.toml`, its `DK_KEY_<ID>` to `KEYS` and an entry to `SHAPES`, and a row to this table. Run `mise run gen:modules`.

## Rules

`mise run module:check -- __ID__` runs `init` in the emulator and checks the records with the rule that the `lfo` service uses:

- 1 to 8 shapes in a module. The service keeps 8 shapes in total over all modules; a ninth is refused at load.
- A name has 4 characters, space to tilde, and is used once. No two names share their first 3 characters (the Digitone II shows 3).
- A shape is not flat (all samples equal) and not all zero.

A shape that has no negative sample is unipolar (like the stock EXPO and RAMP).

The shape needs `lfo.dks` on the card (and `core/lfo` in the firmware image): the `lfo` service gives each shape a wave number from 7 up (kept across boots) and
keeps it while the module is loaded. A sound that names the number shows ERR and runs like RAND on a device without the service, and as the triangle (TRI) when the module is not loaded.
Never rename the module id or a record id: sounds keep a shape by its number, and the number follows the key.

```sh
mise run module:check -- __ID__                  # build, run init in the emulator, check the shapes
mise run module:pack -- __ID__ --device dt2      # make the .dkm (or --device dn2)
```

## Output report

`mise run lfo:report -- __ID__ [--device dt2|dn2]` (default device dn2; about 5 minutes) shows that each shape number plays the table the module declares, from the LFO output on the emulator instead of by ear. It builds the module, the `lfo` service and the patched image when they are stale, loads the module with the bound service, and for each shape sets WAVE to its number with MIDI CC, lets LFO 1 of track 1 run, reads the output over several periods in emulated time and fits it (no phase or rate needed) against the shape's own table, every other table of the module and the stock waves TRI, SINE, SQR, SAW and RAMP. The tables are read from `SHAPES` in `src/module.c`, the wave numbers from the service's sticky map (`lfo.shapes`) matched with the keys in `build/gen/module_ids.h`. The fit error is the root mean square difference of the normalised curves: OK is below 0.06, and every other table must be worse by more than 0.02 (`lfo-fit.ts`). Swing is the measured peak-to-peak output over the full swing of a shape from -1 to +1; "table" is the same for the declared samples, so a shape of amplitude 30000 reads 92%. The fit is shape-only: it rescales both curves to 0..1, so it cannot see a wrong amplitude. The scale check covers that: it compares swing with table and flags `SCALE` when they differ by more than 5 percentage points (measured runs differ by at most 2 points from read spacing). Verdicts: `OK`; `FAIL` when the output does not follow the table; `CLOSE to X` when another table is within the margin, which means the two tables are too similar to tell apart; `SCALE` when the swing check fails. `FAIL` and `CLOSE` take precedence over `SCALE`. Stock tables are printed as `NAME (stock)`.

Output in `out/runs/lfo-shape-report/<device>/<module>/`: `summary.txt` (the table printed on stdout), `report.json` (all fit errors, min, max and the raw samples), and one `<number>-<NAME>.svg` per shape (the declared table as a line, the measured reads as dots at the phase of the best fit; characters outside A-Z, a-z, 0-9, `_` and `-` in the name become `_`). The command is `modules/core/runtime/tests/lfo/lfo-report.manual.device.test.ts` run through `tools/devtools/lfo_report.sh`; it is skipped unless `DIGI_LFO_MODULE` is set and is no part of a gate. It writes all outputs first, then fails (exit code non-zero) when a shape has the verdict `FAIL` or `SCALE`; `CLOSE` is a warning only. The full module id `AUTHOR/NAME` is required.
