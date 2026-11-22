# Observability: capture attribution

**Draft for review.** The first deliverable is a reliable module name beside a measured DSP row on the workbench and in an exported profile. This extends P8e in the [profiling plan](profiling.md#131-item-8-remaining-steps-p8c-p8d-p8e). Keep the existing measurement units and recording opt-in.

## What a name means

A Digitone II track can play Pluck on track 12 while the profiler measures machine row 0. The track identifies where the musician played. The row identifies where the DSP dispatched work. A label such as `digikit/pluck · Machine slot 0` means the machine registry assigned that row to Pluck during the capture. Its cost includes all tracks and voices using that row. A per-track breakdown would need another measurement.

Keep the raw source and identifier with each measurement. Attach ownership separately, once per capture. Treat an unresolved owner, a single owner and multiple owners as distinct cases. An aggregated row has one measured cost, even if several modules contributed. ColdFire poll owners provide a second example for reviewing the design. Program-counter symbolization already has its own address maps and firmware identity checks.

## Proposed phases

1. **Agree on the capture contract.** Trace a machine and an effect from the actual DSP stats index to the module record on DT2 1.17 and DN2 1.12. Account for reserved and free rows, any offset between service and DSP tables, and ordinals folded into the last counter. Sketch optional ownership metadata alongside the existing numeric rows in `dk.profile/1`. Use an old capture and a shared-row example to review decoding, display and round-trip behavior. Check the same design against a ColdFire poll before generalizing it.
2. **Expose ownership on the selected target.** Read ownership through the services that allocate the rows and return stable module IDs through a read-only debug query in DkSession. Before allocating an opcode or changing a service interface, check the current branch and coordinate with the sessions owning protocol generation and service contracts. Start with the existing [machine rows](../../services/machines/src/rows.rs), [effect rows](../../services/fx/src/rows.rs) and [DSP stats decoder](../../packages/dk-session/src/dkupload-core.ts). Keep name lookup off the audio path. Content modules need no instrumentation for this feature.
3. **Use the snapshot in the workbench and exports.** Fetch ownership when an opted-in recording starts. Bind it to that target and recording. Decide how to detect registry changes before trusting labels across a reload: split the capture at a known change or keep affected rows numeric if their ownership is uncertain. Use the same result for the live chart, stopped view and offline export. Unsupported queries on older targets should leave recording and numeric rows available.

## Validation and coordination

Host tests with fakes should cover identical row numbers from different sources, shared rows, missing metadata, target switches and changes of ownership. Exercise old and new files through the TypeScript writer and Rust reader. Cover the service/query boundary with an assembly test. Use a focused device check for the association between a rendered machine or effect and its measured row on each supported target.

Review one phase at a time. The first coordination decision is who owns the wire reply and its compatibility behavior. The second is how a recorder knows that the mapping stayed valid. Leave the wire layout, field names and opcode open until those owners review them.

## Later candidates

- **Capture quality.** Present existing sweep gaps, dropped trace events and partial windows alongside the measurements they qualify. A zero cost and a missing measurement should look different.
- **Comparing captures.** Use stable module IDs to line up captures even when registry order changes. Keep device cycles, emulator instructions and calibrated estimates explicit.
