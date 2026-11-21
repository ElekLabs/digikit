# Device/emulator SHARC calibration on DN2 1.12

Marks: **[D]** measured once here; **[O]** open. A second check against image bytes does not apply: these are measurements.

## Setup [D]

- DN2 1.12, firmware id 7e5b8a733d4ab1fd, runtime 2565 in the run header.
- Profile `kernels` (`devices/calibration/profiles/kernels.toml`): five synthetic kernels of `tests/fixtures/modules/cal-kernels`, one never-ending voice on track 1 (one call per block), 3 captures of 5 s each per case.
- Device: the workbench Calibration tab over USB MIDI (debug module DSP stats). Emulator: `wb profile calibrate run --profile kernels --emu`.
- Calls per block are about 1.0 on both sides (0.994 to 1.000).
- Data files are not committed (firmware-derived run data). Local paths: `~/src/digi/calibration-kernels-dev.captures.json` and `~/src/digi/calibration-kernels-emu.captures.json`.

## Result [D]

| Kernel | Emulator instructions per call | Device cycles per call (mean) | Factor |
| --- | --- | --- | --- |
| alu | 20,064 | 20,263 | 1.010 |
| dm | 19,973 | 21,531 | 1.078 |
| mix | 19,970 | 29,145 | 1.460 |
| branch | 20,060 | 42,060 | 2.097 |
| mem | 19,258 | 51,824 | 2.691 |

The spread is 1.01 to 2.69. One factor per device cannot estimate module cost (worst error 2.7x). ALU and data moves cost about the emulator's instruction count on the device; memory access and branches cost 2 to 3 times that. The per-class model of docs/plans/profiling.md 13.2 is needed for emulator-only estimates [O].

Device stock load in this profile: mean 443,181 and max 443,837 cycles per block (5 stock cases).

## Earlier device results [D]

From docs/handovers/HANDOVER-2026-11-21-calibration-filters.md:

- Stock max block: light about 249k cycles per block, heavy about 420k.
- Analog module: 5,723 mean and 6,811 max cycles per call, against 4,642 emulator instructions per call.

Investigation results:

1. Emulator capture windows were wall time. They now wait in emulated time.
2. The emulator stock engine does the same instructions in the light and heavy projects (about 337k per block), while the device figure rises +68 %. Stock load comes from the device, not from the emulator.
3. Emulator DTIM0 counts one tick per ColdFire instruction (`packages/emulator/machine/src/board.rs`), so ColdFire factors are per owner and informational.
4. The module DSP factor is the key number: module code is exact on the emulator.

## Open [O]

- The memory factor may depend on where module state lives (internal or external memory). To check with the resonator bank.
- How the fit tool pairs captures: calls per block must agree within 1 % (a device capture runs a few blocks longer than an emulator capture).
