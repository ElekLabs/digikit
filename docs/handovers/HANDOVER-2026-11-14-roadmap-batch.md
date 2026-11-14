# Handover 2026-11-14: roadmap batch (R2-R7, P2-P6, T1)

Start from `docs/plans/sdk-runtime-roadmap.md`, then this file. Results are in the plans and findings; this file holds state and next steps.

## Merged to main today

| PR | Item |
| --- | --- |
| 450, 454 | P3 module DSP analysis: dksharc (Rust) and DSP names in the address map |
| 453 | test-speed 16: device tests keyed on the bytes they load (images.sha256, daemon-test.key, pack-verdicts.json, live markers) |
| 457 | P2 dkprof, dkprof.wasm, wb profile and addrmap commands; cfprof.py deleted |
| 458 | flake fixes: remote-helpers deadline, emu-slot retry |
| 459 | R2a deletions; D1-D12 service anatomy in runtime-and-services 5.12; ARCHITECTURE.md layers and boundaries; finding 129 |
| 460 | R2b runtime file split and services layout (moves only, objdiff-proven); patchkit objdiff |
| 462 | progress-based hang guard for device-test waits |
| 463 | T1 test architecture plan (docs/plans/test-architecture.md) |
| 464-467, 469 | T1h steps 1-6: TS-only selection, Host/EmuRpc/FakeHost, app objects, WorkerFactory/WasmSource, one test runner, packages/workbench-core plus NodeHost |
| 470 | R2c interfaces and crate split, R2d sink slots, R3a Platform trait (profile-driven), R3b core/platform adapter, emulated-time probes |
| 471 | T1a test-selection baseline; over-selection items 20-25 in test-speed.md |

## Built, waiting for a gate

`gate/batch4` (lane `~/src/digi/lanes/sdk-gen`, not pushed) is main + these branches, with all host checks green:
- `test/noise-timeouts-cache`: device-test timeouts are outer nets only, `expectPollEmulated`, atomic cache entries.
- `rt/r6a-link`: R6a link API (desired-state table, owner tickets, drain), UI-task pump over today's protocol.
- `prof/p4-cost`: P4 sharc.profile (feature `sharc-profile`), `dkprof::budget` and `check_selection`, pk_conform on Rust.
- `ui/p6-names`: P6 Log-pane names over dk-session plus debug PEEK (emulator `runtime.maps`, device ship map from firmware prepare), Memory per-module rows.

Next step: merge `rt/r7-registry` into `gate/batch4` once its follow-ups land, then run one gate (`DIGI_GATE_BASE=origin/main mise run test`, about 100 min). Its failures name tests; a fix agent gets only the failure list. Then push and merge each branch as its own PR, in this order: noise fixes, R6a, P4, P6, R7.

## In progress

- `rt/r7-registry` (lane `sdk-design`): R7 generic records, service registry, boot planner with Verdict, declare-check-commit, `dk_record_add`, tagged .dkm header table (runtime 0.8.0), the `lfo` service proof. Follow-ups running:
  - confirm the fit and trim the +8% .text;
  - replace `tools/patchkit/dkm_identity.py` with a Rust patchkit subcommand;
  - keep the LFO fixture under tests/fixtures/modules/.

  The gate list is in runtime-and-services 4.1.
- S5 design scout (read-only).

## Decisions taken today (in the plans unless noted)

- **R6:**
  - R6a is the API (desired state, owner results, drain) over today's protocol, pumped from the UI task.
  - R6b is a batched state-sync transport (stock-like: each block carries a batch of values, one ack per batch), triggered by modulation rate (R7 LFO, P4). It needs SHARC dsp-link changes and an ISR A/B first.
  - Stock firmware research: a per-block frame ISR copies dirty values (dirty bit, mirror); there is no queue.
- **P6:** Log names, plus Memory per-module rows. The dedicated Profile pane moves to P8. One dk-session/debug-module path serves the emulator and the device.
- **P8 split:**
  - P8a is the ColdFire half: DK_SPAN in the service dispatchers, the debug module owning the span sink, PROFILE_STATE/PROFILE_READ in the debug module (range from 0x23), and an additive `spans` field in dk.profile/1.
  - The Profile pane is its own package.
  - P8b is the DSP stats (the SHARC stub accumulates; dsp-link reads it into a `dsp_stats` sink), built with R6b.
  - Hard rule: no measurable cost without the debug module (paired A/B, golden hashes and PCM pins unchanged).
- **P4:**
  - P4 measures; S5 writes `dsp_cost` into the header and the broker applies the rule max per-voice machine cost × voices + FX costs + stock <= headroom.
  - Units are instructions until P9.
  - Real headroom is 194,244 (DT2) and 92,796 (DN2) instructions per block (from `patchkit budget`).
  - Fast-tier refusals moved to P5.
- **R7:**
  - The LFO shape is a test kind and the base for real LFOs.
  - The proof is in-crate.
  - `dk_record_add`.
  - One .dkm format change carrying the R12 entries, gated by a named list.
  - The storage interface and files memory split into their own item.
  - R7 defines the Verdict type; R11 builds the states on it.
- **R3:**
  - Device facts come from devices/*.toml (`[capabilities]` from the device-registry branch, plus `[digikit] sample_path_max`), generated into the device objects.
  - No device-id comparisons above L1 (layer rule 7).
- **Tests:**
  - Fix gate noise at the root: waits in emulated time, wall clocks only as outer nets.
  - T1d audit (scratchpad notes, below): about 29 device tests can split logic to host tests; tags for T1e.
  - Test-speed item 19: share compiled emulator crates between lanes.

## Next on the roadmap

- S5 design (scout running), then build after P4 merges.
- P8a, then the Profile pane (after R7 merges).
- R8 whole-path spike.
- Storage interface and files memory.
- T1: T1e service-scoped selection (needs the T1d tags), T1c contract tests, T1b uniform build/test split, T1f SDK-surface fixtures, T1g SHARC-only rig, and the T1a over-selection fixes (items 20-25).
- Review stop with Em after R8.

## Notes

- Design inputs from today's scouts (not in the repo) are under the session scratchpad: r2c, r6, p4, p6, p8, r7 and t1h inputs, and the T1d audit. The decisions above are the parts that matter.
- Lanes: `sdk-design` (R7), `sdk-gen` (gate/batch4), `sdk-fixtures` (prof/p4-cost), `sdk-ui` (new; TS work and this handover).
- Em removes merged branches and lanes.
