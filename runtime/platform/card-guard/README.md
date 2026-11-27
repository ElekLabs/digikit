# core/card-guard

Refuses a request to the firmware's eMMC block write or erase that lies outside the rows of its writer class. Design and tests: [docs/plans/card-write-guard.md](../../../docs/plans/card-write-guard.md); hook sites: [finding 156](../../../docs/findings/156-card-write-guard-sites.md).

- `src/cf/guard.h`: the check, plain C with no firmware access (host test `tests/guard-host.c`, run by `tests/guard.test.ts`).
- `src/cf/guard.c`: `dkcg_write`, `dkcg_erase` (called by the shims), the counters and `dkcg_stats` (read by the runtime).
- `src/cf/hooks.s`: the two `before` hook shims. A refused request returns -10 to the caller before the stock code runs.
- `src/cf/<dev>/device.s`: the rows of `[card.guard]` of the device file, from `build/gen/card-guard-<dev>.inc`.

The class of a caller is its return address: the runtime's raw write calls the firmware from `dkrt_raw_write_ret` (core/runtime `tramp.s`). Every other caller is a stock caller. The module is required wherever the device file turns `[card.guard] enabled` on (`needed_when` in `module.toml`); a device adds a `device.s`, an object and a port.
