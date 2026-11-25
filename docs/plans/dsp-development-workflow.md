# DSP development and profiling workflow

Connect the firmware catalog, address maps, profiler, rig and Workbench so a developer or an LLM can answer one DSP question end to end. Start from a stock firmware file or module build, replay a workload, identify the functions and loops that do the work, compare two builds and check the audio. Use the same workflow for machine development, effects, reusable SDK kernels and performance work. Apply it first to `digikit/freqshift` for hypnotic techno.

Target **Digitakt II OS 1.17** and **Digitone II OS 1.12**. The payoff is a repeatable path from a DSP question to evidence someone else can inspect. Confidence is high in connecting the existing catalog, symbol resolver and capture formats. Board timing gains and the preferred FREQ SHIFT sound are decisions for measurement and listening.

## Worktree and review gates

Work phase by phase on `work/freqshift-workflow` in the `freqshift-workflow` worktree. At the end of each phase, leave its changes ready for review and **stop before committing**. Summarize the outcome, changed paths, checks and their results, representative artifacts, and open decisions. Ask Em whether the phase is ready to commit. Continue to the next phase after approval and the commit.

Keep firmware, captures, generated catalogs and listening WAVs local under ignored output paths. Arrange hardware sessions with Em. Use the repository's owner checks during iteration and its affected merge gate before merging.

Keep the [event-driven device test audit](event-driven-device-test-audit.md)
in a separate warm worktree. Start it from the observer checkpoint, which
passed the SETTINGS open-and-close regression on DT2 1.17 and DN2 1.12.

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
| 5c. FREQ SHIFT listening and performance proof | Implemented and rig-verified | Spread was committed in `12c8ae781`. Drift and Rate were approved after the full-cycle A/B, though the difference was subtle on laptop speakers. No kit migration for the experimental prototype. |
| 6. Reuse a real listening session | Host and DT2 emulator checked | [Workbench sessions](../WORKBENCH-SESSIONS.md) package the card and attachments and keep capture inspection independent. Underpass reopened with the same A01 grid and stab-only send. Automated project selection and saved audio views remain follow-ups. No new Workbench panel. |
| 7. Stock audio reference pilot | Verified, ready for review | Underpass gives a conservative Feedback candidate. DN2 confirms the stock decay anchor and the DT2 plain-versus-PGO sweep is byte-identical. |
| 7a. Run Feedback inside FREQ SHIFT | Verified, listening review open | The fifth knob controls a filtered 55 ms stereo loop. Zero Feedback is byte-identical to the previous build on both send rigs. Both emulator record checks and packs passed. |
| 8. Replay a reference case on emulator and hardware | Emulator checked, hardware open | Use one TypeScript Delay case with the shared session and input helpers. Prepare the project separately from capture. |

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
- Keep the effect identity, current signed `SHIFT` range and framework `MIX`. Add **SPREAD** first, with opposite offsets around `SHIFT` for left and right. Add **DRIFT** for shallow modulation and **RATE** for its period. Start with 8–32 second periods for listening. Zero spread and drift must preserve the existing sound. Both depths start at zero in fresh kits.
- Reuse the [scripted TOML sound checks](../TESTING.md#sound-checks-testsrigtoml). Add the needed signals as the design calls for them: stepped sine across the band for shift and image rejection, two tones or seeded noise for broadband behavior, impulse then silence for settling, and a steady tone with a scripted knob move for continuity. Share the stimuli and measurement code while keeping expectations in each module. Extend the CLI to export the same named TOML cases that `test-dsp` runs when we need that export. Do not make a general tone UI a prerequisite.
- Profile the current shared oscillator and all-pass loops before choosing an implementation. Measure the cost with independent shifts active and check it against each target's effect budget. Update the README's cost statement from the verified result. For an optimization, compare the same algorithm and workload first so the cost of a new feature is visible.
- Save dry/current/new renders from the same techno loop and listen across a full drift cycle. Keep phase continuous through turns and block boundaries. Use the general capture workflow for comparison and reserve a focused state probe for a specific mismatch.

**5a verification.** The local `out/runs/dsp-workflow/phase5/baseline` contains the unchanged FREQ SHIFT master-insert render from each target. Independent DT2 repeats matched all 768 stereo frames and the recorded cost. A JSON-only repeat reports summary-only as intended. A PLUCK case exercised the machine export.

**5b verification.** The local `out/runs/dsp-workflow/phase5/send-baseline` contains both dry/current pairs per device. Each comparison verified matching source audio, a changed output when the send was raised, and no full-scale samples. The [render manifest](../INSTRUMENTATION.md#listen-to-a-send-return) links each take to a profile of block 375, with matching input and linked-build identities. The module is unchanged. Em can audition the baseline before Spread work starts.

**5c Spread prototype.** The FSH page keeps SHIFT and MIX and adds SPREAD, from 0 to 8 Hz per side. At zero from rest it uses the legacy oscillator. When turned back to zero, the side phase slews back into the centre before returning to that path. The local `out/runs/dsp-workflow/phase5/spread` has a zero and a 4 Hz per side render for each of the two scenes and both rigs. The zero renders match the original PCM bit for bit. At 4 Hz per side, the input still matches and none of the renders clips. The active setting adds 1,102 steady-block rig instructions over the original on both targets in these two scenes. Each render links to its block 375 profile.

The `spread/audition` subfolder has a stronger stereo check using the same texture and maximum send, with centre SHIFT at zero and SPREAD at either zero or 8 Hz per side. The zero file also has a level-matched listening copy. Use that pair to hear the control, not to judge the subtler ±4 Hz musical setting.

**Experimental kit compatibility.** A kit or project saved with the one-knob prototype can carry a stored value for a knob that did not exist then. The saver copied all eight FX row words without recording the knob count. On reload, the first unused word can become SPREAD. Set SPREAD back to 0 if an old save opens with a nonzero value. No migration is required for this experimental proof of concept. Revisit saved parameter compatibility when defining the stable module and package contracts for 1.0.0. Drift, Rate and Workbench changes are out of the Spread prototype.

The float32 model and named DSP sound checks pass, including the previous PCM pin, the left and right test tones and the return to the shared oscillator. The full `module:check` now puts the current system package onto the temporary emulator card before selecting modules. This fixed record capture on both targets and regenerated the gallery and resource metadata. Conformance measured 6,470 instructions per steady effect block on the shared rig, and both packs fit. The optional minute-long oscillator soak was skipped. This is emulator and rig evidence, not a physical hardware run.

**5c Drift and Rate prototype.** Drift moves the centre of the stereo pair by up to ±4 Hz with a triangle. RATE spans 32 to 8 seconds per cycle and defaults to 16. Updating the rate changes the phase increment, not the phase. The audio shift still ramps inside the existing 32-sample block. Zero depth parks the modulation and changes neither the saved Spread sound nor the legacy path when held at zero. The rig checks a rate change through the modulation crest, the parked phase and the settled output with Drift off.

The local `out/runs/dsp-workflow/phase5/drift` contains dry, Spread-only and Drift-on takes on both rigs. `wb module render --repeat 3` runs the same texture for 21.82 seconds with no DSP reset at the scene boundaries. The A/B uses the same input, full send, SHIFT at zero and ±4 Hz Spread. The active take uses ±4 Hz Drift over 16 seconds. Neither take reached full scale. Separate zero-Drift exports matched the accepted Spread PCM bit for bit on both rigs.

`module:check` regenerated the four-knob gallery, measured a 6,511-instruction steady effect-block cost and confirmed both emulator packs fit. The paired texture renders added 39 steady-block rig instructions on each target when Drift was active. They include the target-specific send adapter, so they are a separate measurement from conformance. The 30 sound checks and 26 model tests passed. No physical hardware was tested.

**Exit check.** `module:check` and the affected rig and device checks pass on both targets. The zero-feature path matches the existing float32 reference within the established tolerance. The new checks cover channel shift separation, parameter continuity and bounded output at control limits. Save the CLI before/after report and linked Workbench capture. Em accepts the musical result after listening to the comparison.

## 6. Save and reopen a listening session

Use Underpass at 138 BPM to prove the reusable workflow. Save the card and
pinned firmware identity with the separate profiling and audio artifacts in a
[Workbench session](../WORKBENCH-SESSIONS.md). Open the capture offline, then
boot the working card and load the saved project. Verify A01 and the stab
track's FREQ SHIFT send before making another recording. Save As must leave
the original session and the default +Drive untouched.

Keep reusable lifecycle logic under mock-backed unit tests. Use one emulator
round trip for the card-dependent boot that a fake cannot prove. Expose
session and capture metadata offline through `wb session inspect` and
`wb profile report`.

The DT2 1.17 emulator round trip booted the imported card, saved its overlay,
then mounted that checkpoint in a fresh emulator. Both boots recovered the
same A01 trigs and the send values `[0, 0, 111, 0]` after loading Project 001.
The CLI inspected the saved bundle and reported its attached capture without
an emulator connection. The host tests cover Save As isolation separately.

The listening A/B restarted both takes from that checkpoint and kept SHIFT,
Spread, Rate, MIX, the pattern and sends fixed. The 34.8-second takes changed
only Drift depth, from zero to ±3.02 Hz on a 16-second cycle. Worst FREQ SHIFT
send cost rose from 5,592 to 5,631 emulated SHARC instructions per block.
Neither capture recorded DSP overruns. Raw WAVs, level-matched copies, two
Workbench captures and the JSON comparison are kept together under ignored
`out/runs/`. A separate DN2 PLUCK rig run verified the render and hotspot
reporting path for a machine. It was not a DN2 saved-session round trip.

Defer automated project selection and in-app A/B auditioning until the saved
session workflow sees use. If an A/B view would save repeated work, add it to
the existing Analyzer rather than adding a panel. Keep Performance focused
on timing and counters.

## 7. Build a small stock audio reference

- Reuse the [stock FX scenario fixtures](../../tests/regression/scenarios/README.md)
  and [saved-session workflow](../WORKBENCH-SESSIONS.md). Start with stock Delay
  on DT2 1.17 and cross-check on DN2 1.12. Use the closest delay time to the
  55 ms Feedback prototype and record actual echo spacing, feedback readback,
  first-repeat level, fall per repeat, tail length and peak. Use a short burst
  to identify repeats, then the same musical source to check mix behavior.
- Keep two comparisons separate. Use stock Delay to anchor echo spacing,
  decay and return level. Use FREQ SHIFT with itself to verify the frequency
  offset in Hz, Spread as the left/right offset, and Drift depth and period.
  Stock Chorus can be a listening reference for width or motion, not an
  equivalent control. Keep the raw gain evidence and make separate
  level-matched listening copies.
- Store each case with device, firmware, input hash, route, settings, tap,
  WAV and JSON metrics. Keep audio reference metrics separate from measured
  DSP cost. Expand only when the first case informs a real range decision.
  A DT2 playback reference would use a known WAV. A DN2 machine reference
  would use a saved patch and a fixed note and velocity sequence.

**DT2 pilot.** A 25 ms, 1 kHz burst through stock Delay on the 1.17 emulator
gave about 63.5 ms between returns. At the feedback settings near 19, 32,
45 and 58, the first repeats fell by 10.7, 6.1, 3.2 and 0.9 dB per repeat,
respectively. Zero feedback gave one return. The same recorded dry burst
through the actual FREQ SHIFT kernel on the SHARC rig, with the feedback
loop in the host, gave about 55.4 ms between returns and losses of 6.5 and
2.3 dB per repeat at loop gains 0.48 and 0.78. Use 0.48 as the first musical
audition candidate and 0.78 as the long-tail stress case, not as a proposed
default or upper bound. The stock recording includes the dry signal and the
rig output is wet-only, so this compares separated returns, not overall
loudness or headroom. Both pilot sweeps captured exactly 120,480 frames per
take at 48 kHz, with byte-identical dry PCM. At stock Feedback near 32,
decay was about 6.1 dB per repeat at both delay times. Host wall-clock speed
does not enter this measurement. The headless emulator ran in emulated time
and wrote PCM before the host playback buffer. The capture recorded firmware
hashes but not the daemon or DSP library build IDs, so PGO is unverified.

**DT2 musical audition.** Retuned stock Delay's TIME control on the 138 BPM
Underpass session and measured 53.65 ms from dry stab to return, close to the
prototype's 55 ms loop. Kept stock Delay near Feedback 32 and compared the
same stab played solo and in the first verified bar of the groove. The
listening reel matches the first return and preserves the dry signal. The
untouched emulator captures and raw SHARC rig returns are kept alongside the
comparison JSON. Over the 100 ms before the next programmed stab, the
level-matched isolated return was 1.75 dB quieter than stock at 0.48 and
1.36 dB louder at 0.78. Keep 0.48 as the conservative audition candidate and
0.78 as the stronger comparison, not as a chosen default or limit. The
feedback loop is still outside the module.

**DN2 cross-check.** Stock DN2 1.12 used a short, factory audition-sine
note, not the DT2 sample. At about 62.4 ms between returns, the first
repeats fell by 10.85, 6.26, 3.28 and 1.03 dB per repeat near Feedback
19, 32, 45 and 58 respectively. Zero feedback gave one return. At each
setting the decay was within 0.16 dB per repeat of DT2. This makes about
6 dB per repeat a useful stock anchor on both emulated devices. It does
not equate their gain staging or establish a module ceiling.

**DT2 build parity.** Replayed the same probe card and six-take stock
Delay sweep with matching firmware, runtime assets and effect readbacks.
The plain and PGO daemon and SHARC library pairs were pinned by hash and
matched to the same emulator source revision. All six takes were
byte-identical at 120,480 stereo frames each, with zero sample difference.
The PGO build used the available DT2 profile without retraining. This
verifies PCM for this stimulus and build pair, not host throughput, the
PGO mode of the original pilot, or parity with a physical unit.

**Implementation choice.** Keep 0.48 as the first Feedback candidate and
0.78 as the stronger audition option. A hardware check is a separate
validation, not a gate on the musical audition.

**Exit check.** Reproduce the chosen stock Delay case from its inputs. Show
which Feedback settings give a useful short tail and which settings should
stay out of the default range. Document any emulator or capture limitations.
No new Workbench UI or module control is required for the pilot.

### 7a. Run Feedback inside FREQ SHIFT

The fifth knob, FDBK, is off by default and capped provisionally at 0.8.
The module feeds its filtered wet signal into the shifter after exactly
2,640 frames, without changing the adapter's dry signal or MIX. The
per-effect state is 22,356 bytes, including the stereo ring. Turning
the knob off ramps the gain down over one block and invalidates the ring.
The rig allocates and guards larger effect states from their descriptors.

**Checks.** The Underpass stem and first-bar mix preserve the previous
DT2 and DN2 send output byte-for-byte at zero. The two rigs match a
separate float32 reference loop, including the half-block wrap. At 0.48
and 0.78 the worst steady Underpass send blocks took 7,517 instructions
on DT2 and 7,782 on DN2, compared with 6,505 and 6,770 at zero. The
adapter's first-block state clear raised those active counts to 12,931
and 13,196. These are rig counts, not board timing. The raw groove peak
was at most −20.7 dBFS in these takes. A deliberately hot source can go
above unity, so do not treat this comparison as a general headroom limit.
The standard module check accepted both sets of registered records and
measured 24,768 bytes of DSP area per pack, including code and alignment.
Its packing check shows that either build fits on an empty target.

**Review.** Listen to the in-module Underpass takes at zero, 0.48 and 0.78,
with a shared audition gain. Keep the default off while the module is
experimental. Review the upper limit by ear and with hot sources before
promoting it. Hardware timing and loading are separate from this rig proof.

## 8. Replay a reference case on emulator and hardware

- Express one DT2 stock Delay tone case in a [TypeScript case file](../../packages/workbench-core/src/reference/dt2-stock-delay.ts).
  Reuse the `DkSession` and harness MIDI, clocks and panel input. The case defines the
  firmware, source WAV and its hash, project setup, effect settings, stimulus
  and capture window. The target supplies the clock, panel control transport,
  and a way to verify the effective settings.
- Add opt-in firmware state observers to `digikit/debug`, with a shared
  `DkSession` interface for snapshots and changes. Prove SETTINGS open and
  close on DT2 1.17 and DN2 1.12, then use structured sample assignment and
  stock Delay readback for the DT2 case. `INPUT_TAKEN` remains a delivery
  diagnostic, and screenshots remain failure evidence, not assertions.
  Before sending any input, fail if the target lacks a required observer.
  Defer Syntakt's view and value observers until its firmware readbacks are
  mapped and tested. Its existing debug functions remain available.
- Keep card images and snapshots emulator-only. The explicit
  `prepare-project` command builds a private, reopenable Workbench session
  on the emulator or stages the same verified WAV using the shared sample
  backend on hardware. Report the remaining panel setup separately. Never
  rewrite a saved hardware project or upload anything during capture.
- Use the emulator's internal PCM tap or the device's USB audio input. Share
  Workbench's native input setup with the CLI capture bridge. Reject apparent
  capture loss and record the source, channel route and rate.
  Export the same WAV and JSON fields from both targets through the CLI.

**Exit check.** Run the same case on DT2 1.17 in the emulator and on a
physical DT2, with Em's hardware session arranged separately. Verify the
fixture and settings, then compare echo spacing and decay against documented
tolerances. Do not require matching PCM hashes across targets. Add more cases
or a Workbench control only once this first case proves useful.

**Emulator checkpoint.** [The reference CLI](../INSTRUMENTATION.md#replay-the-dt2-stock-delay-reference)
generates the pinned WAV and records dry and wet takes using the same
TypeScript case for both targets. Two consecutive DT2 1.17 emulator captures
measured 64.901 ms echo spacing and 6.034 dB loss per repeat, and the Delay
send returned to zero after each run. They also passed against the earlier
64.047 ms baseline, with a 0.854 ms spacing difference from the prepared
stock Delay setting and no measurable difference in decay. The USB audio
bridge builds and lists inputs. The physical run and cross-target comparison
remain open. The CLI validation is acoustic and does not read back the stock
Delay knob values or the assigned sample hash.

**Project preparation checkpoint.** The CLI-built session reopened in the
session inspector. A disposable mount booted with its preselected debug and
remote modules and played the seeded probe without a sample upload or panel
assignment. After applying the listed stock Delay settings, two takes
measured 64.755 ms and 6.034 dB per repeat. The earlier TypeScript baseline
comparison passed. Hardware staging has host tests for new uploads, reuse,
and collision refusal. Running it on the physical DT2 remains open.

**View observer checkpoint.** With the 0.13.1 runtime and the debug module,
the same event-driven regression opened and closed SETTINGS on the DT2
1.17 and DN2 1.12 emulators, without a DSP or any screenshot matching.
It checked the reported view against an independent read of the firmware's
view stack. Host tests cover default-off, debug-level events, the initial
snapshot, and refusal before input when the observer is unsupported.
The observer samples the top of the stack. It cannot yet identify a
particular dialog or a page change within an unchanged view.

**Parameter observer checkpoint.** DT2 1.17 resolves `sampleSlot` in Track
1's Sound and `delay.feedback` in the live FX array from the device file.
In one disposable emulator, the sample pick, a panel edit, and two MIDI
CCs emitted `VALUE_CHANGED` through the existing event listener. The
snapshots matched independent reads of the live memory. The CLI can inspect
both raw values and includes them in the capture report. DN2 and Syntakt
do not yet advertise mappings for this observer.

**Loaded sample checkpoint.** The DT2 1.17 reader in Workbench Core uses
the resolved `cf.samples.site.slot.keys` anchor and the existing read-only
`DkSession.peek`. The session and capture scaffold do not know the DT2
table layout. `wb audio reference inspect` reports the live and expected
native keys. `run` checks them before sending panel input or creating the
output directory and records both in the capture report. On two disposable
emulator cards, slot 7 matched the seeded probe (`b9694553`, 19,280 stored
bytes) and a different sample in the same slot was refused before recording.
The saved hardware upload receipt agrees on the expected hash and counts
19,264 payload bytes before the firmware's 16-byte trailer. See
[the DT2 pool tables](../findings/109-dt2-stock-pool-borrowing.md).

**Delay TIME checkpoint.** The fresh project had TIME `0x1700` and echoes
441.3 ms apart even though Track 1 received the requested Delay send.
Changing only TIME to `0x0288` gave 64.90 ms spacing and 6.03 dB decay per
repeat. The corrected emulator capture passed against the saved baseline
within 0.15 ms. The reference CLI now reads TIME through the existing kit
observer and refuses values outside `0x0240–0x0340` before recording.
That range catches setup mistakes, but does not verify tempo or replace
the acoustic check. The CLI refused the original `0x1700` without creating
output. A second take at `0x027C` passed at 64.05 ms and 6.03 dB per repeat,
within 0.86 ms of the `0x0288` take.

Verify the remaining Delay parameters as needed for automated setup. The
physical comparison still needs matched runtime artifacts and a working
USB audio input.

## End result

A reusable CLI and Workbench workflow for firmware investigation, module development, SDK kernels and performance comparisons. A developer or an LLM can reproduce a run and point to the function, loop and audio evidence behind a change. FREQ SHIFT is the first shipped application of that workflow, with a stereo send Em can use and a saved comparison that demonstrates it.
