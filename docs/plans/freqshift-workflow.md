# FREQ SHIFT for hypnotic techno

Build a version of `digikit/freqshift` that gives a repeating hat, stab or texture slow stereo movement while the kick and bass stay dry. Use it to prove that a developer or an LLM can generate a firmware catalog, replay a DSP workload, identify a hot function and compare two builds through the CLI. Workbench opens the same saved evidence.

Target **Digitakt II OS 1.17** and **Digitone II OS 1.12**. The catalog, symbol resolver and capture formats already exist, so confidence is high in connecting them. The main unknowns are which modulation range Em will want to keep and what separate stereo oscillators cost on hardware. The payoff is an effect for a real project and a workflow the next machine or FX can use.

## Worktree and review gates

Work phase by phase on `work/freqshift-workflow` in the `freqshift-workflow` worktree. At the end of each phase, leave its changes ready for review and **stop before committing**. Summarize the outcome, changed paths, checks and their results, representative artifacts, and any open decisions. Ask Em whether the phase is ready to commit. Continue to the next phase after approval and the commit.

The plan commit is the starting point. Keep firmware, captures, generated catalogs and listening WAVs local under ignored output paths. Arrange hardware sessions with Em. Use the repository's owner checks during iteration and its affected merge gate before merging.

## 1. Generate a catalog from one firmware file

- Add a direct SHARC recipe to [Firmware analysis](../FIRMWARE.md) and link to it from the root README. Use that guide for the generation procedure, including Ghidra prerequisites for ColdFire. Link to it from [Development](../DEVELOPMENT.md), [Tools](../TOOLS.md) and the [firmware-query skill](../../.agents/skills/firmware-query/SKILL.md). Keep review labels and the query reference in their owning docs.
- Add `fw review --syx PATH --core dsp` to identify a registered stock release by hash, extract its DSP sections, build the database and standalone catalog, and generate HTML. An optional peer file adds cross-release matching. Keep image-key review available for re-export from an existing catalog. Describe how to register the resulting SQLite file under [Workbench Firmware symbols](../ADDRMAP.md#firmware-symbols-in-desktop-settings).
- Return the device and release, firmware identity, core and generated artifact paths in a machine-readable form. Keep stable links by function entry within the matching release and core. Preserve distinct evidence for standalone and paired catalogs.

**Exit check.** Run the command separately on DT2 1.17 and DN2 1.12 from empty analysis output. Verify the reported HTML, SQLite and TSV paths, that a function link opens its row, and that the paired mode resolves a known counterpart. Check the existing database-only review path and an unrecognized input. Run `tests/firmware/test_sharc_catalog.py` and `tests/firmware/test_catalog_page.py`. Keep check output separate from shared analysis directories.

## 2. Connect saved hotspots to firmware evidence

- Reuse `dkprof` and addrmap. Keep the sampled PC and resolved entry, owner, offset and units available in structured `wb profile report` output and in Workbench. Use the recorded stock firmware hash and CPU for catalog navigation. Use the capture's module identity, load table and module map for loaded-module symbols and analysis. ColdFire byte addresses and SHARC word PCs stay distinct.
- Add loop body size, trip-count evidence and the existing `fw cost` estimate to the SHARC catalog and structured query output. Show unresolved counts and calls alongside estimates. Add instruction scheduling or memory evidence when it answers an observed hotspot question. Use a paired stock biquad as a check for the cost display.
- Link the catalog and CLI workflow from [the DSP SDK](../../sdk/dsp/README.md). Put symbol registration and identity rules in [Addrmap](../ADDRMAP.md), and capture and replay instructions in [Instrumentation](../INSTRUMENTATION.md).

**Exit check.** Open one saved SHARC hotspot per release in both `wb profile report` and Workbench. Both resolve the same recorded PC to the same entry and owner. A stock match opens its catalog row. A module address uses its captured load address. Unknown or mismatched firmware remains visibly unresolved. Check that existing captures still load and that the CLI can be used without opening Workbench.

## 3. Capture one repeatable FREQ SHIFT workload

- Make the existing [FREQ SHIFT calibration case](../../modules/fx/digikit/freqshift/calibration.toml) run via the shared calibration runner. Today the effect setup step is skipped because debug `SETUP` handles machines and filters. Reuse the existing routing and remote input paths. Confirm the effect route, send and resulting DSP dispatch before collecting the measurement. Reuse the runner's teardown and waits on target events or state.
- Use a deterministic test signal for the timing baseline and a short musical loop with kick, bass and one sent hat or stab for listening. Keep the input and setup repeatable and record the case identity, firmware and module hashes, routes, settings and relevant start/stop markers with the capture. Keep source audio and renders local.
- Save emulator SHARC instruction profiles and hardware dispatch or block cycles separately. Use the same stimulus and compare like measurements. Keep optional ColdFire spans and trace with their clocks and loss counts. Reopen a saved trace in the existing PixiJS Timeline and show case markers if it helps locate the event being investigated. Associate PC hotspots with functions and trace events with their recorded owners.
- If a mismatch persists, capture the smallest relevant named state at a known audio block. Start from debug `PEEK` for ColdFire and the existing SHARC probe path for hardware DSP. Record address space and capture timing. Use emulator memory reads for their matching side. Keep the capture scoped to that diagnostic record.

**Exit check.** From the terminal, replay a bounded case on both emulator targets and save a capture that identifies a nonzero FREQ SHIFT dispatch and resolves its hottest loop. Run the same workload on authorized hardware for the cycle baseline. Record emulator results separately wherever hardware is pending. Open the saved artifact in Workbench and check that it agrees with the CLI report. Refresh the module README's instruction-cost statement from measured evidence. It currently differs from the manifest.

## 4. Make FREQ SHIFT useful in a project

- Keep the effect identity, the existing signed `SHIFT` range and the framework's `MIX` control. Add **SPREAD** first, with left and right shifts offset in opposite directions around `SHIFT`. Add **DRIFT** for shallow modulation around that setting and **RATE** for its period. Start listening with periods of roughly 8–32 seconds. Zero spread and drift must preserve the original behavior, including settings loaded from an existing kit.
- Keep phase continuous across turns and block boundaries. Measure the existing shared oscillator and all-pass loops before choosing a schedule. When features are active, measure the cost of independent channel shifts. Reuse SDK pieces where their numerical and register contracts fit. Extract a shared primitive only when the implementation demonstrates a useful, testable contract for another effect.
- Compare the current and new builds using the workload and capture recipe from phase 3. Start with a low send on the hat or stab, retain the dry kick and bass, and audition the whole drift cycle. Retain dry/current/new renders for Em. Feedback and tempo sync can follow a separate decision informed by that listening pass.

**Exit check.** `module:check` and the affected rig and device checks pass on both targets. The zero-feature path matches the existing float32 reference within the established tolerance. New checks cover channel shift separation, continuity during turns, and bounded output at the control limits. Record the instruction ceiling first, then a paired A/B for any optimization claim. Report board cycles only from matched board captures. Check the effect against each target's budget with the new controls active. Em accepts the musical result after listening to the saved comparison.

## End result

A two-device FREQ SHIFT build Em can use as a slow stereo send, a saved listening comparison, and one documented CLI recipe that reproduces the run, resolves hotspots and compares builds. The same capture opens in Workbench. The recipe and evidence are the starting point for the next machine or FX.

## Related references

- [Existing frequency shifter and its reference tests](../../modules/fx/digikit/freqshift/README.md)
- [Firmware analysis and the catalog](../FIRMWARE.md)
- [Address maps and Workbench firmware symbols](../ADDRMAP.md)
- [Profiles, calibration and trace captures](../INSTRUMENTATION.md#profiles)
- [DSP SDK and register contract](../../sdk/dsp/README.md)
