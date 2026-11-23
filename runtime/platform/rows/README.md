# Parameter rows (core/rows)

The image makes room for parameter rows, the services fill them. This module owns the parameter table of the Digitakt II (1.17) and the
Digitone II (1.12) MAIN OS and the rows reserved in it:

- **The copy.** patchkit copies the firmware's parameter table (60-byte rows, finding 02) to `pf_table`, appends one row for each entry of the
  `params` registry, points every reader (56 references) at the copy and raises every clamp bound (54 per device) by the number of added rows. The
  routine `pf_t2` (`src/cf/dt2/params.s`) answers the value-class lookup for an added code with its template's code. Content modules do not add
  `params` entries to the image: no module other than `core/rows` does (`packages/patchkit/tests/rows.rs`).
- **The reservation.** Three blocks, in this order, each one `[[entry]]` line with a `count` over the device file's `[limits]` (`modules/README.md`
  "Reservations"): `dp`, `[limits] param_rows` rows for the parameters of run-time machines; `fp`, `8 * fx_slots` rows (7 knobs and the level of a page for
  each effect slot); `fs`, `send_rows` rows for the cells of the send page (at least `send_slots`). A user fills its block at load and clears it at unload.
- **`rows_info`.** The table in the image's info table (`src/cf/rows-info-<dev>.s`, words big endian): `'ROWS'`, version 1, device id; the table, its row size;
  `params.rows`, its row size and the offset of `template`; the first code and the count of added rows; three blocks of four words (user, first code, rows,
  template of a free row; users 1 machines, 2 effect pages, 3 send page); the number of reserved rows; the 15-word recipe of a row (0 to 0x3c copies that word
  of the template's row, 0x100 min, 0x101 max, 0x102 default, 0x103 name, 0x104 short, 0x105 to 0x108 computed words); the four cap words
  (`cap_machine_rows`, `cap_param_rows`, `cap_page_rows`, `cap_send_slots`, generated from `[limits]`) that the admission reads before a service loads.

`dynreg_info` and `fxpages_info` keep their exports until their users move (RC4-2, RC4-4); the services still read their blocks from those tables, and
the numbers in them come from the same device constants.

Checks: `pnpm nx:pilot run patchkit:rust-tests` (`tests/rows.rs`: the reserved counts, the row expressions and write ops against an independent row builder,
`pf_t2` on the ColdFire core, `rows_info` against the device file). Evidence: `docs/findings/02-machines-and-parameters.md`, `docs/findings/158-service-entry-survey.md`.
