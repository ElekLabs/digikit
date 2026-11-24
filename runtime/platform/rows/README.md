# Parameter rows (core/rows)

The image makes room for parameter rows, their owners fill them. This module owns the parameter table of the MAIN OS of the devices with a
parameter table and the rows reserved in it:

- **The copy.** patchkit copies the firmware's parameter table (60-byte rows, finding 02) to `pf_table`, appends one row for each entry of the
  `params` registry, points every reader (56 references) at the copy and raises every clamp bound (54 per device) by the number of added rows. The
  routine `pf_t2` (`src/cf/*/params.s`) answers the value-class lookup for an added code with its template's code. Content modules do not add
  `params` entries to the image: no module other than `core/rows` does (`packages/patchkit/tests/rows.rs`).
- **The reservation.** Three numbered blocks, in this order (entry ids `dp`, `fp`, `fs`, the symbols their users link by), each one `[[entry]]` line with a
  `count` of `rows_b1`, `rows_b2` or `rows_b3` (the block sizes of the device file's `[[rows.block]]`, one source) (`modules/README.md` "Reservations"). The platform knows the blocks by number: the device file's `[rows]` and
  `[[rows.block]]` give each block's owner number, its size (a `[limits]` key times a factor) and its template, and say what the block is for.
  An owner fills its block at load and clears it at unload.
- **`rows_info`.** The table in the image's info table (`src/cf/rows-info.inc`, included by `rows-info-<dev>.s` after the device constants; words big
  endian): `'ROWS'`, version 1, device id; the table, its row size; `params.rows`, its row size and the offset of `template`; the first code and the count
  of added rows; three blocks of four words (owner number, first code, rows, template of a free row); the number of reserved rows; the 15-word recipe of a
  row (`[rows] recipe`: 0 to 0x3c copies that word of the template's row, 0x100 min, 0x101 max, 0x102 default, 0x103 name, 0x104 short, 0x105 to 0x108
  computed words); the four capacity words (`[rows] caps`, keys of `[limits]`). The `[[info]]` names `cap_machine_rows`, `cap_param_rows`, `cap_page_rows` and
  `cap_send_slots` are the admission's: it reads `cap_<kind>` for each kind of `[capacity.kinds]` in `sdk/sdk.toml`, so the names stay.

`dynreg_info` and `fxpages_info` keep their exports until their users move (RC4-2, RC4-4); their users still read their blocks from those tables, and
the numbers in them come from the same device constants.

Checks: `pnpm nx:pilot run patchkit:rust-tests` (`tests/rows.rs`: the reserved counts, the row expressions and write ops against an independent row builder,
`pf_t2` on the ColdFire core, `rows_info` against the device file). Evidence: `docs/findings/02-machines-and-parameters.md` (the stock parameter table), `docs/findings/158-service-entry-survey.md`.
