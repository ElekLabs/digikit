# DSP development and profiling workflow

Connect the firmware catalog, address maps, profiler, rig and Workbench so a developer or an LLM can answer one DSP question end to end. Start from a stock firmware file or module build, replay a workload, identify the functions and loops that do the work, compare two builds and check the audio. Use the same workflow for machine development, effects, reusable SDK kernels and performance work. Apply it first to `digikit/freqshift` for hypnotic techno.

Target **Digitakt II OS 1.17** and **Digitone II OS 1.12**. The payoff is a repeatable path from a DSP question to evidence someone else can inspect. Confidence is high in connecting the existing catalog, symbol resolver and capture formats. Board timing gains and the preferred FREQ SHIFT sound are decisions for measurement and listening.

## Worktree and review gates

Work phase by phase on `work/freqshift-workflow` in the `freqshift-workflow` worktree. At the end of each phase, leave its changes ready for review and **stop before committing**. Summarize the outcome, changed paths, checks and their results, representative artifacts, and open decisions. Ask Em whether the phase is ready to commit. Continue to the next phase after approval and the commit.

Keep firmware, captures, generated catalogs and listening WAVs local under ignored output paths. Arrange hardware sessions with Em. Use the repository's owner checks during iteration and its affected merge gate before merging.

## 1. Generate a catalog from one firmware file

- Put the SHARC quick start in [Firmware analysis](../FIRMWARE.md) and link it directly from the root README. Keep Ghidra prerequisites for ColdFire in that guide. Link to it from [Development](../DEVELOPMENT.md), [Tools](../TOOLS.md) and the [firmware-query skill](../../.agents/skills/firmware-query/SKILL.md). Keep review labels and the query reference in their owning docs.
- Add `fw review --syx PATH --core dsp` to identify a registered stock release by hash, extract its DSP sections, build the database and standalone catalog, and generate HTML. An optional peer file adds cross-release matching. Keep image-key review available for re-export from an existing catalog.
- Return the release, firmware identity, core and generated artifact paths in machine-readable form. Use stable links by function entry within the matching release and core. Document how to register the resulting SQLite file under [Workbench Firmware symbols](../ADDRMAP.md#firmware-symbols-in-desktop-settings).

**Exit check.** Run the command separately on each target from empty analysis output. Verify the HTML, SQLite and TSV paths, a link to a function row, and a known counterpart in paired mode. Check the existing database-only review path and an unrecognized input. Run `tests/firmware/test_sharc_catalog.py` and `tests/firmware/test_catalog_page.py`.

## 2. Connect profiles to firmware and module evidence

- Use the same `dkprof` and addrmap resolution for `wb profile report` and Workbench. Keep the sampled PC, resolved entry, owner, offset and units in structured output. Use the capture's stock firmware hash and CPU to link firmware functions to the catalog. Use the capture's module identity, load table and module map for loaded-module symbols and analysis. Preserve the distinction between ColdFire byte addresses and SHARC word PCs.
- Surface loop body size, trip-count evidence and the existing `fw cost` estimate in the SHARC catalog and structured query output. Show unresolved counts and calls. Use a paired stock biquad to check the output. Add scheduling or memory-access evidence when it answers a specific kernel or hotspot question.
- Keep symbol registration and identity rules in [Addrmap](../ADDRMAP.md), and link the CLI recipe from the firmware-query skill. Structure CLI results so a caller can inspect the evidence without scraping the HTML.

**Exit check.** Resolve a saved SHARC hotspot per release in `wb profile report` and Workbench. Both report the same entry and owner, with a usable link for a stock or module match. Check a module relocation using its recorded load table and preserve numeric addresses when firmware identity is missing or mismatched. Check that existing captures still load.

## 3. Replay and compare machine and effect workloads

- Extend the existing calibration runner where needed to replay a user-selected machine or effect case. Use the existing remote and debug paths for setup and observation. The current effect case is skipped because debug `SETUP` handles machines and filters. Build a general effect setup path around the existing routing and input interfaces. Verify the selected route and dispatch before accepting an effect capture. Use event or target-state waits and the runner's teardown.
- Save the case and input identity, firmware and build hashes, route and settings, clocks, relevant markers and loss counts with the capture. Reuse `dk.profile/1` and its existing reporting path. Keep emulator SHARC instructions, hardware dispatch or block cycles, and ColdFire spans in their recorded units. Reopen saved traces in the existing PixiJS Timeline with their recorded owners and case markers where available.
- Add a named state capture when a mismatch requires it. Use debug `PEEK` for ColdFire, emulator DSP reads for SHARC state in the model, and the existing probe path for selected hardware DSP values. Record the address space and capture boundary for each value.

**Exit check.** From the CLI, run the same workflow with an existing machine case such as [PLUCK](../../modules/machine/digikit/pluck/calibration.toml) and an effect case such as [FREQ SHIFT](../../modules/fx/digikit/freqshift/calibration.toml) on both emulator targets. The caller selects a case through its definition. Each capture identifies the module dispatch, and Workbench and CLI agree on its reported evidence. Where hardware is available, replay the same stimulus and record board cycles separately.

## 4. Establish the SDK and optimization workflow

- Document a worked example in [the DSP SDK](../../sdk/dsp/README.md) from a reference signal and a rig case to static cost, function or loop profile, and paired A/B. Link to [Instrumentation](../INSTRUMENTATION.md) for capture commands and to the generated register contract and device profiles for portability rules. Cover the per-device DSP area and the gate, stereo and sample-backing differences that affect module design.
- Use an existing kernel or module for the worked example. State the instruction ceiling first. Verify the intended sound against the reference or capture and record the workload and units with the comparison. Use matched board captures for board-cycle claims. Favor measured changes in scheduling, loop work or memory traffic that apply to the observed bottleneck.
- Give shared kernels an explicit numerical, state, buffer and register contract and a reference case. Extract a primitive into `sdk/dsp/` when shared use and the checks justify it.

**Exit check.** Starting from the docs, a second caller can use the CLI to run a different existing case, identify its DSP cost and compare two builds. The method covers a machine or effect with the same tooling and produces artifacts Workbench can inspect. The worked example separates static instruction estimates, observed instructions and measured hardware cycles.

## 5. Apply the workflow to FREQ SHIFT

- Use a send on a repeating hat, stab or texture, with the kick and bass dry. Keep the effect identity, current signed `SHIFT` range and framework `MIX`. Add **SPREAD** first, with opposite offsets around `SHIFT` for left and right. Add **DRIFT** for shallow modulation and **RATE** for its period. Start with 8–32 second periods for listening. Zero spread and drift must preserve the existing sound and settings loaded from an existing kit.
- Profile the current shared oscillator and all-pass loops before choosing an implementation. Measure the cost with independent shifts active and check it against each target's effect budget. Update the README's cost statement from the verified result. For an optimization, compare the same algorithm and workload first so the cost of a new feature is visible.
- Save dry/current/new renders from the same techno loop and listen across a full drift cycle. Keep phase continuous through turns and block boundaries. Use the general capture workflow for comparison and reserve a focused state probe for a specific mismatch.

**Exit check.** `module:check` and the affected rig and device checks pass on both targets. The zero-feature path matches the existing float32 reference within the established tolerance. The new checks cover channel shift separation, parameter continuity and bounded output at control limits. Save the CLI before/after report and linked Workbench capture. Em accepts the musical result after listening to the comparison.

## End result

A reusable CLI and Workbench workflow for firmware investigation, module development, SDK kernels and performance comparisons. A developer or an LLM can reproduce a run and point to the function, loop and audio evidence behind a change. FREQ SHIFT is the first shipped application of that workflow, with a stereo send Em can use and a saved comparison that demonstrates it.
