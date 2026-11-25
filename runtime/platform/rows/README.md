# Rows (core/rows)

The image makes room for the rows that run-time modules fill, their owners fill them: the parameter rows, the machine types and the page rows of a device that has them. This module owns the parameter table of the MAIN OS of the devices with a
parameter table and the rows reserved in it:

- **The copy.** patchkit copies the firmware's parameter table (60-byte rows, finding 02) to `pf_table`, appends one row for each entry of the
  `params` registry, points every reader (56 references) at the copy and raises every clamp bound (54 per device) by the number of added rows. The
  routine `pf_t2` (`src/cf/*/params.s`) answers the value-class lookup for an added code with its template's code. Content modules do not add
  `params` entries to the image: no module other than `core/rows` does (`packages/patchkit/tests/rows.rs`).
- **The reservation.** Three numbered blocks, in this order (entry ids `dp`, `fp`, `fs`, the symbols their users link by), each one `[[entry]]` line with a
  `count` of `rows_b1`, `rows_b2` or `rows_b3` (the block sizes of the device file's `[[rows.block]]`, one source) (`modules/README.md` "Reservations"). The platform knows the blocks by number: the device file's `[rows]` and
  `[[rows.block]]` give each block's owner number, its size (a `[limits]` key times a factor) and its template, and say what the block is for.
  An owner fills its block at load and clears it at unload.
- **The machine types.** The registry `machines` (`types.toml` gives each type its permanent number; the stock types are gap rows of the registry from `machines.first`, the last eight are the reserved run-time
  types), the copies of the firmware's name, permission and per-type tables with one row per type (`mf_names`, `mf_permit`, `mf_pertype`; the DSP's selector table is `core/dyndsp`), the bounds of their readers
  and of the sound loader, and the dispatch of a type (`mf_dispatch`, `src/cf/machines-<dev>.s`). A project that names a reserved type boots without the machines service: the row is a gap row, the project
  keeps the number and the track plays as type 0. A row without descriptor strings gets the empty ones at its first dispatch, so nothing needs the service to make them. On a device with page rows the registry `pages` has
  the reserved page rows. Everything else a machine needs at run time (the list, the rank pairs, the hooks, the strings of a row, the native pages) is `services/machines`; no other module adds a row to the registry
  in the image (a machine is a run-time module).
- **`rows_info`.** The table in the image's info table (`src/cf/rows-info.inc`, included by `rows-info-<dev>.s` after the device constants; words big endian; the layout is in the file's header and in
  `services/rows/src/rows_info.rs`): `'ROWS'`, version 2, device id; the machine rows, their size, `machines.first` and `.count`, the first reserved type and how many there are, the name, permission and per-type tables; the
  offsets of a machine row's fields; the parameter table and registry rows with the first code and the first block; the 15-word recipe of a parameter row (`[rows] recipe`: 0 to 0x3c copies that word of the template's
  row, 0x100 min, 0x101 max, 0x102 default, 0x103 name, 0x104 short, 0x105 to 0x108 computed words); the knobs of a machine and the slot of knob 0; the page rows with their layout; the builder profile (the machine facts of the
  device file's `[modules]`, `PROFILE_*`); the slot map, the copy of the type reader and the stock list order; the three parameter blocks (owner number, first code, rows, template of a free row) and the four capacity
  words (`[rows] caps`, keys of `[limits]`). The `[[info]]` names `cap_machine_rows`, `cap_param_rows`, `cap_page_rows` and `cap_send_slots` are the admission's: it reads `cap_<kind>` for each kind of
  `[capacity.kinds]` in `sdk/sdk.toml`, so the names stay.

The fx service finds its two blocks (the effect knobs and the send knobs) by owner number with `dkrt_svc_rows::block::block`; no image point holds a table for it. The machine selection view is `core/ui`'s (`machine_view`).

Checks: `pnpm nx:pilot run patchkit:rust-tests` (`tests/rows.rs`: the reserved counts, the row expressions and write ops against an independent row builder,
`pf_t2` on the ColdFire core, `rows_info` against the device file; `tests/machines_fw.rs`: the dispatch and the machines hooks on the ColdFire core and the copies of the stock tables). Evidence: `docs/findings/02-machines-and-parameters.md` (the stock parameter table), `docs/findings/158-service-entry-survey.md`.
