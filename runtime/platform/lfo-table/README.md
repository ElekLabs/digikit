# core/lfo-table

The LFO waves of the devices that have the stock LFO tick, beyond the seven stock ones (WAVE 7 to 6 + N). N is `[limits] lfo_waves` of the device file (8: waves 7 to 14).

**What it changes in the image** (declared in `ports/fw/lfo.toml`, written by `module.toml`; design `out/runs/hot-reload/lfo-redesign.md`):

- The LFO tick (vector 191) and its second reader (engine task) clamp the wave byte to 0..6 and index four 7-entry tables: the wave function table and three edge-value tables (modes 3 and 4). The module copies each table into the expansion with N cells appended and points the eight `lea` operands of each device at the copies.
- The compare of the clamp (`moveq 6,Dn`) becomes 6 + N. The store of the clamp stays 6, so a wave byte above 6 + N plays RAND as stock.
- An added wave is *empty* (its function entry is the stock sine, its edge value 0: it plays sine) or *set* (its entry is the stub of the wave, which reads the 32 cells of the wave).

**Files**

| file | what |
| --- | --- |
| `src/cf/lfo-wave.inc` | the wave stubs (18 instructions per call, bit-equal to `interp_cells` of `services/lfo`; D0, D1, A0 and the flags only), the cells (`.bss`, N x 32 x 8 bytes), the interrupt mask, `lfo_table_info` |
| `src/cf/lfo-table.c`, `lfo-table.h` | `lfo_table_set` and `lfo_table_clear` (UI task; the cells and the function entry change under the interrupt mask) |
| `src/cf/lfo-wave-*.s` | the device's `LFO_WAVES` (`build/gen/device-<dev>.inc`, from `patchkit gen`), then `lfo-wave.inc` |

**Interface** (`lfo_table_info`, the image's info table, found with `CoreApi::info`): `'LFOT'`, version 1, first wave 7, N, samples 32, `lfo_table_set`, `lfo_table_clear` (`src/cf/lfo-table.h`). The cell format is private to this module.

**Tests**

- `tests/table-host.c` (run by `tests/table.test.ts`): set and clear over fake mask functions.
- `packages/patchkit/tests/lfo_table.rs`: each stub on the ColdFire core against the formula (registers kept, no memory written, 18 instructions), set and clear on the core, and the patched images (clamps, eight operands, stock and added cells, info words).
