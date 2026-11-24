# DSP development and profiling workflow

Connect the firmware catalog, address maps, profiler, rig and Workbench so a developer or an LLM can answer one DSP question end to end. Start from a stock firmware file or module build, replay a workload, identify the functions and loops that do the work, compare two builds and check the audio. Use the same workflow for machine development, effects, reusable SDK kernels and performance work. Apply it first to `digikit/freqshift` for hypnotic techno.

Target **Digitakt II OS 1.17** and **Digitone II OS 1.12**. The payoff is a repeatable path from a DSP question to evidence someone else can inspect. Confidence is high in connecting the existing catalog, symbol resolver and capture formats. Board timing gains and the preferred FREQ SHIFT sound are decisions for measurement and listening.

## Worktree and review gates

Work phase by phase on `work/freqshift-workflow` in the `freqshift-workflow` worktree. At the end of each phase, leave its changes ready for review and **stop before committing**. Summarize the outcome, changed paths, checks and their results, representative artifacts, and open decisions. Ask Em whether the phase is ready to commit. Continue to the next phase after approval and the commit.

Keep firmware, captures, generated catalogs and listening WAVs local under ignored output paths. Arrange hardware sessions with Em. Use the repository's owner checks during iteration and its affected merge gate before merging.

## Status on this branch

| Phase | State | Evidence and remaining work |
| --- | --- | --- |
| 1. Catalog from stock firmware | Committed in `2f3cae241` | The [firmware guide](../FIRMWARE.md) has the one-file command and generated output paths. |
| 2. Profile-to-catalog links | Committed in `1de16dc5c` | Workbench and the CLI share symbol resolution. Firmware symbol settings and catalog navigation are documented in [Addrmap](../ADDRMAP.md). |
| 3. Workload replay | Committed in `7716df71d` | Machine and effect cases replay on the two emulator targets. Saved Timeline shows retained events, markers and loss. Hardware timing and focused state probes are follow-ups when needed. |
| 4. SDK workflow and unified capture | Committed in `8e8f8d372` | The [SDK example](../../sdk/dsp/README.md) compares PLUCK on both rigs with byte-identical audio. Rigs export function and loop evidence for effects too. Workbench shares recording, opening and run selection across views. This is rig and emulator evidence, not board timing. |
| 4a. Saved spans and processor boundaries | Complete | Offline Spans, per-run CLI reports and target-aware recording options are implemented. See the verification below. |
| 5a. Render exports and audio comparison | Complete | The [CLI workflow](../INSTRUMENTATION.md#render-and-compare-audio-from-the-cli) exports original WAV and structured evidence and compares them offline. The two-device FREQ SHIFT rig baseline precedes the listening fixture. |
| 5b. Musical send baseline | Complete | Both generated scenes have dry/current WAVs, matching source hashes and linked rig profiles on both targets. No module changes. |
| 5c. FREQ SHIFT listening and performance proof | Pending | Add Spread first, then Drift and Rate. Use scripted sound checks, rig cost and the musical A/B to decide what ships. |
| 6. Review the workflow | Deferred | Consider saved audio evidence in the existing Analyzer only after the CLI workflow has seen real use. No new Workbench panel. |

## 1. Generate a catalog from one firmware file

- Put the SHARC quick start in [Firmware analysis](../FIRMWARE.md) and link it directly from the root README. Keep Ghidra prerequisites for ColdFire in that guide. Link to it from [Development](../DEVELOPMENT.md), [Tools](../TOOLS.md) and the [firmware-query skill](../../.agents/skills/firmware-query/SKILL.md). Keep review labels and the query reference in their owning docs.
- Add `fw review --syx PATH --core dsp` to identify a registered stock release by hash, extract its DSP sections, build the database and standalone catalog, and generate HTML. An optional peer file adds cross-release matching. Keep image-key review available for re-export from an existing catalog.
- Return the release, firmware identity, core and generated artifact paths in machine-readable form. Use stable links by function entry within the matching release and core. Document how to register the resulting SQLite file under [Workbench Firmware symbols](../ADDRMAP.md#firmware-symbols-in-desktop-settings).

**Exit check.** Run the command separately on each target from empty analysis output. Verify the HTML, SQLite and TSV paths, a link to a function row, and a known counterpart in paired mode. Check the existing database-only review path and an unrecognized input. Run `tests/firmware/test_sharc_catalog.py` and `tests/firmware/test_catalog_page.py`.

## 2. Connect profiles to firmware and module evidence

- Use the same `dkprof` and addrmap resolution for `wb profile report` and Workbench. Keep the sampled PC, resolved entry, owner, offset and units in structured output. Use the capture's stock firmware hash and CPU to link firmware functions to the catalog. Use the capture's module identity, load table and module map for loaded-module symbols and analysis. Preserve the distinction between ColdFire byte addresses and SHARC word PCs.
- Surface loop body size, trip-count evidence and the existing `fw cost` estimate in the SHARC catalog and structured query output. Show unresolved counts and calls. Use a paired stock biquad to check the output. Add scheduling or memory-access evidence when it answers a specific kernel or hotspot question.
- In Workbench, group source defaults and persisted overrides by stock release under Settings. Open the catalog under Firmware and route stock hotspots to the matching function. Keep the standalone review available in a browser.
- Keep symbol registration and identity rules in [Addrmap](../ADDRMAP.md), and link the CLI recipe from the firmware-query skill. Structure CLI results so a caller can inspect the evidence without scraping the HTML.

**Exit check.** Resolve a saved SHARC hotspot per release in `wb profile report` and Workbench. Both report the same entry and owner, with a usable link for a stock or module match. Check a module relocation using its recorded load table and preserve numeric addresses when firmware identity is missing or mismatched. Check that existing captures still load.

## 3. Replay and compare machine and effect workloads

- Replay a user-selected machine or effect case through the calibration runner. Use the existing remote and debug paths for setup and observation. Effect cases use the routing and input interfaces because debug `SETUP` handles machines and filters. Verify the selected route and dispatch before accepting an effect capture. Use event or target-state waits and the runner's teardown.
- Save the case and input identity, firmware and build hashes, route and settings, clocks, relevant markers and loss counts with the capture. Reuse `dk.profile/1` and its existing reporting path. Keep emulator SHARC instructions, hardware dispatch or block cycles, and ColdFire spans in their recorded units. Reopen saved traces in the existing PixiJS Timeline with their recorded owners and case markers where available.
- Add a named state capture when a mismatch requires it. Use debug `PEEK` for ColdFire, emulator DSP reads for SHARC state in the model, and the existing probe path for selected hardware DSP values. Record the address space and capture boundary for each value.

**Exit check.** From the CLI, run the same workflow with an existing machine case such as [PLUCK](../../modules/machine/digikit/pluck/calibration.toml) and an effect case such as [FREQ SHIFT](../../modules/fx/digikit/freqshift/calibration.toml) on both emulator targets. The caller selects a case through its definition. Each capture identifies the module dispatch, and Workbench and CLI agree on its reported evidence. Where hardware is available, replay the same stimulus and record board cycles separately.

## 4. Establish the SDK and optimization workflow

- Document a worked example in [the DSP SDK](../../sdk/dsp/README.md) from a reference signal and a rig case to static cost, function or loop profile, and paired A/B. Link to [Instrumentation](../INSTRUMENTATION.md) for capture commands and to the generated register contract and device profiles for portability rules. Cover the per-device DSP area and the gate, stereo and sample-backing differences that affect module design.
- Use an existing kernel or module for the worked example. State the instruction ceiling first. Verify the intended sound against the reference or capture and record the workload and units with the comparison. Use matched board captures for board-cycle claims. Favor measured changes in scheduling, loop work or memory traffic that apply to the observed bottleneck.
- Give shared kernels an explicit numerical, state, buffer and register contract and a reference case. Extract a primitive into `sdk/dsp/` when shared use and the checks justify it.

**Exit check.** Starting from the docs, a second caller can use the CLI to run a different existing case, identify its DSP cost and compare two builds. The method covers a machine or effect with the same tooling and produces artifacts Workbench can inspect. The worked example separates static instruction estimates, observed instructions and measured hardware cycles.

## 4a. Make saved profiling and processor roles consistent

1. **Fix saved Spans first.** Use the selected run's span totals in both Profile and Performance → Spans. Show owner, calls, total and max time, and CPU share in a clearly labeled saved summary. Only draw an over-time chart when the run actually has interval history. Timeline shows recorded events and any loss. Do not infer missing intervals from totals or a partial trace. Saved views work offline and do not surface errors from the live target.
2. **Separate processor identity from measurement support.** Use the device registry for roles and architecture. DT2 and DN2 have a ColdFire main CPU and SHARC+ DSP. Syntakt has a ColdFire main CPU and a separate ColdFire audio peer. Use **Main CPU**, **DSP**, and **Audio peer** in general views, with architecture and address-space details in symbol settings and analysis. Let each backend advertise what it can record for the active target, runtime and emulator build. Keep Syntakt main-CPU debug spans independent of peer profiling, which is not currently available. Distinguish unavailable, available but not recorded, and recorded zero.
3. **Keep state and reporting shared by responsibility.** The capture session owns private live and saved state and exposes actions such as `start`, `stop`, `save`, `open`, `selectRun`, and `returnToLive`. Collectors own transport and architecture-specific commands. Views consume read-only data grouped by measurement (spans, trace, DSP blocks and hotspots). Share report data with `wb profile` through the existing core and `dkprof` rather than computing it again in UI components. Verify that the same export produces matching span summaries and hotspot reports without a device connected. CLI collectors may keep their existing entry points. Store clocks, units and address spaces with their evidence. Keep `dk.profile/1` CPU keys and existing files readable. Add processor instance IDs to the format when a producer needs them to distinguish, for example, two ColdFire cores.

Use one **Open capture** and one **Record / Stop** flow in Workbench. A file can contain multiple independent runs, but switching views must not silently combine their measurements. For example, the local `out/runs/dsp-workflow/phase4/freqshift-dt2-review.captures.json` has a send replay with span totals, trace and DSP dispatch costs, and a separate rig run with function and loop hotspots. Keep profiling off by default. Automatic boundary measurements should not require module edits. Module authors may add optional internal probes for questions the existing measurements cannot answer, and builds that omit those probes must not pay for them.

**Exit check.** With no device connected, open the DT2 example (or a checked-in synthetic equivalent). Profile and the drawer agree on the span totals. Timeline shows trace loss, and the send run does not borrow the rig run's hotspots. The CLI reports the same recorded evidence. Check capabilities for DT2, DN2 and Syntakt, including a Syntakt debug-span target and an emulator build without DSP PC sampling. Existing captures still load. Leave audio-peer instrumentation and a new capture schema out of this phase.

**Verified.** The offline Workbench preview opens a two-run fixture, keeps span totals and trace loss on the send run, and moves to the rig's PCs without carrying over the send data. The CLI reports 15 span rows and 392 dropped events on the DT2 send replay, and function counts on its separate rig run. Host tests cover DT2, DN2, the Syntakt audio-peer topology and legacy debug runtimes. A native RPC test passed on the build without the DSP sampler. No physical device was used.

## 5. Apply the workflow to FREQ SHIFT

- Start with the shared CLI. `wb module render` runs a named rig case and emits WAV and/or JSON. `wb audio compare` checks two exports, uses the original PCM where present and reports summary-only comparisons as such. The technical baseline uses `params/all-mid/noise` on both rigs. It is a master-insert check, not the musical send comparison.
- Generate two repeatable musical inputs without needing user samples. Send the hat or rim from a tight percussion loop while keeping kick and bass dry. Use a sustained stab or texture as a second check and try the same settings there before retuning. Save dry and current stereo WAVs, input/build identity and matching cost evidence before editing the effect. Keep rig send evidence separate from full emulator or hardware captures.
- Keep the effect identity, current signed `SHIFT` range and framework `MIX`. Add **SPREAD** first, with opposite offsets around `SHIFT` for left and right. Add **DRIFT** for shallow modulation and **RATE** for its period. Start with 8–32 second periods for listening. Zero spread and drift must preserve the existing sound and settings loaded from an existing kit.
- Reuse the [scripted TOML sound checks](../TESTING.md#sound-checks-testsrigtoml). Add the needed signals as the design calls for them: stepped sine across the band for shift and image rejection, two tones or seeded noise for broadband behavior, impulse then silence for settling, and a steady tone with a scripted knob move for continuity. Share the stimuli and measurement code while keeping expectations in each module. Extend the CLI to export the same named TOML cases that `test-dsp` runs when we need that export. Do not make a general tone UI a prerequisite.
- Profile the current shared oscillator and all-pass loops before choosing an implementation. Measure the cost with independent shifts active and check it against each target's effect budget. Update the README's cost statement from the verified result. For an optimization, compare the same algorithm and workload first so the cost of a new feature is visible.
- Save dry/current/new renders from the same techno loop and listen across a full drift cycle. Keep phase continuous through turns and block boundaries. Use the general capture workflow for comparison and reserve a focused state probe for a specific mismatch.

**5a verification.** The local `out/runs/dsp-workflow/phase5/baseline` contains the unchanged FREQ SHIFT master-insert render from each target. Independent DT2 repeats matched all 768 stereo frames and the recorded cost. A JSON-only repeat reports summary-only as intended. A PLUCK case exercised the machine export.

**5b verification.** The local `out/runs/dsp-workflow/phase5/send-baseline` contains both dry/current pairs per device. Each comparison verified matching source audio, a changed output when the send was raised, and no full-scale samples. The [render manifest](../INSTRUMENTATION.md#listen-to-a-send-return) links each take to a profile of block 375, with matching input and linked-build identities. The module is unchanged. Em can audition the baseline before Spread work starts.

**Exit check.** `module:check` and the affected rig and device checks pass on both targets. The zero-feature path matches the existing float32 reference within the established tolerance. The new checks cover channel shift separation, parameter continuity and bounded output at control limits. Save the CLI before/after report and linked Workbench capture. Em accepts the musical result after listening to the comparison.

## 6. Review whether the UI needs more

Review one complete CLI-driven experiment before changing Workbench. If auditioning saved captures there would save repeated work, add a small saved A/B view to the existing Analyzer with synchronized playback and the relevant measurements. Keep Performance focused on timing and counters. Reuse saved artifacts. Leave the UI as it is if the CLI and WAVs are enough.

## End result

A reusable CLI and Workbench workflow for firmware investigation, module development, SDK kernels and performance comparisons. A developer or an LLM can reproduce a run and point to the function, loop and audio evidence behind a change. FREQ SHIFT is the first shipped application of that workflow, with a stereo send Em can use and a saved comparison that demonstrates it.
