# Native INTC mask commands

**[D] 2026-10-27.** The native ColdFire board used plain register writes for
SIMR/CIMR, so an ISR's request to mask itself left IMR unchanged. The existing
hardware helper also ignored sources 32–63. This is an interrupt-controller
defect; it does not require changes to MIDI messages or firmware.

## Hardware and scope

[MCF5441x Reference Manual, Rev. 5](https://www.nxp.com/docs/en/reference-manual/MCF54418RM.pdf),
sections 17.2.2, 17.2.5 and 17.2.6: each command's low six bits select an IMR
bit; indices 0–31 address IMRL and 32–63 address IMRH. Bit 6 selects all masks.
SIMR sets masks, CIMR clears them, and both command registers read as zero.

`machine::Time` now applies these commands for native byte accesses, using
`IntcBank::hw_apply_simr_cimr`. That helper addresses both halves of each
controller. The standalone `IntcBank::read/write` and `periph::machine::Timers`
remain the frozen RAM-style trace-replay contract. There is no new timer policy,
checkpoint field, public interface or firmware change.

## Checks and remaining qualification

**[D]** Synthetic regressions failed before the fix and pass afterward:

- `periph --test intc_manual`: all 64 source indices across all three
  controllers; individual and all-mask commands, neighboring-bank/controller
  isolation, and preserved RAM-style replay writes. Fourteen tests pass.
- `machine --test intc_masks`: actual ColdFire `MOVE.B D1,absolute-long`
  instructions through the native Board/Time bus for source 28 (MIDI receive),
  32 and 63; IMR visibility, zero command reads, unaffected ICRs and all-mask
  precedence. Two tests pass. No firmware fixture is required.

The focused command is `cargo test --locked --offline -p periph --test
intc_manual -p machine --test intc_masks`, under the repository bench lock.
The neighboring SSI, timer-storage and timer-state checks also pass (18 tests).
Rust formatting and diff checks pass. `test:changed` reaches the existing full
repository pathcheck failure; new files introduce no stale paths, and all eight
reported references in touched files are unchanged lines already in the base.

**[D]** Installed DN2 OS 1.12 emulation also passes an unchanged downstream
firmware regression: 31,912 audio frames, native controls, parameter locks,
LFO delivery, project reload, mixed voices and PCM, with no reported halt or
missing blocks. A separate immediate post-STOP MIDI control test retains its
original input ordering and post-message settling; the fresh note uses the
new selection within six DSP frames. Earlier voices retain their selections,
and native output is nonzero. No firmware change, message retry or pre-message idle pause
was used. Generated code, firmware, captures and private fixture sources are
not part of this patch.

**[O]** DT2 boot/audio, complete MIDI ordering under other workloads and
physical-device behavior remain unqualified. The coordinator still owns the
routed merge gate; the downstream runs qualify this candidate, not an already
adopted or released upstream revision.

The routed gate ran once against base `359a229`: its native build lacked the
local generated DN2 core, and desktop frontend dependencies/output were not
available. Dependent lanes were skipped. This is an incomplete gate, separate
from the passing focused and downstream native checks above.
