# Workbench UI refactor

Make the desktop Workbench easier to navigate and safer to change. Each feature should have an identifiable owner for its state and asynchronous work, an API that callers can understand from its types, and a host test seam for its behavior. Preserve how the Workbench operates across navigation, target switches, and reopen. This work starts with Log and tool drawers on `work/workbench-log-drawer`, based on the local `origin/main` at `85e056bf9`.

## Progress

| Slice | State | Evidence |
| --- | --- | --- |
| Log and tool drawers | Committed as `056dd95ec` | Desktop host and type checks, simulated drawer and save checks, independent review. Review caught a live fixture behavior change that was corrected before the commit. |
| Emulator lifetime | Committed as `ae185d45c` | The window owner discards late startup results and clears its polls. A single controller stop waits for pending firmware work. The final desktop host and type checks, both simulated scenarios, and an independent re-review passed. |
| Firmware operations and shell | Committed as `c6b4bde53` | Firmware and default-module jobs have separate owners. `WorkbenchShell` composes Navigation, FirmwareToolbar, StatusBar, and the drawers. The desktop host suite, type checks, production UI build, and simulated preview checks passed. |
| Profile and Calibration | Committed as `646d164c1` | The target-bound owner governs reads, recording, and DSP release. History and saved comparison have separate views. The desktop suite, type checks, production UI build, and simulated Profile, Calibration, and Performance checks passed. |
| Samples | Committed as `9e6e6e1b7` | Window-owned source, target-bound destination, and separate panes. Desktop and Workbench Core host and type checks, production UI build, and rendered preview passed. |
| Tauri adapter | Host coverage complete | IPC ordering, rejection recovery, drawer remount, PCM responses, and Log save payloads are covered. Native webview verification remains open. |
| Inspector collapse | Fixed | The compact inspector reserves space for the collapse button on Memory and Metrics. Preview scenes exercise all four tabs in expanded and collapsed states. |
| Remote and Analyzer organization | Layout reviewed | Plan separate folder moves around the existing stream and target lifetimes. |

## Ownership and dependencies

| Lifetime | Owner | Examples |
| --- | --- | --- |
| Workbench window | Desktop composition at `createDesktopApps` and the shell | Log entries, Follow preference, active tool drawer, emulator session |
| Selected target binding | The feature that acquires the binding | Inspector reads, profiling counters, remote held keys |
| Mounted component | The component that handles the interaction | Resize drag, focus, dialog fields, temporary selection |
| Pure computation | A plain TypeScript module beside its caller | Log filtering and auto-open policy, boot decisions, drawer size calculations |

`main.tsx` and `mount-workbench.tsx` remain the composition entry points. Keep native IPC in `host/tauri-host.ts`. Pass services and feature capabilities explicitly at the composition boundary. Pass a prop directly to a component that uses it. If several intermediate components would have to forward the same window-scoped dependency, use a narrow Solid context at the shell boundary. Use the existing `Host`, `KeyValue`, `TargetBinding`, and worker factory contracts. Keep shared Transfer and Firmware behavior in `workbench-core` and desktop wiring in the app.

Use read accessors and named commands at feature boundaries. Keep signal setters private when an operation also maintains other state. Own polling and cleanup in the same feature. A Solid factory with effects needs a Solid owner or an explicit disposal contract. Keep JSX and its styles beside feature logic, and extract plain TypeScript when it describes behavior that can be tested independently.

## 1. Log and tool drawers

**Implemented and verified.** This slice covers the Log data and tool drawer selection, the drawer frame, and the wiring to current desktop callers.

### Work sequence

1. Write a Vitest file in `apps/desktop/ui/tests/` against a plain TypeScript factory. Use `memoryKeyValue` and create independent feature instances. The factory tests should run in the current `desktop-host` project without loading JSX.
2. Replace the signals at module scope in `log-state.ts` with a window-owned log and drawer. Compose them once with the injected storage in `desktop-app.ts`. The log exposes entries, unread count, and Follow controls. Producers take a log writer. Drawer selection has one active value for Log, Analyzer, or Performance.
3. Wire the shell and Log panel to the instance. Route Transfer and Firmware messages through their desktop environments, and pass the writer to Card, Samples, and the runtime log follower. Supply the same instance to the simulated and live preview entry points. Preserve the preview's ability to open Log from the fixture URL.
4. Extract the drawer frame and resize separator from `Console.tsx`. Keep `dock-core.ts` as the owner of sizing calculations. Preserve the existing class names, computed drawer height, resize bounds, preference key, ARIA attributes, and keyboard shortcuts. The drawer applies its height locally, so the unused shell variable `--dock-h` can be removed. Keep Analyzer and Performance mounted only when selected. Leave target selection and emulator commands at their current call sites.

### Acceptance cases

| Case | Expected behavior | Proof |
| --- | --- | --- |
| A regular error arrives while Log is closed | The unread count increments and the active drawer stays selected | Vitest |
| A qualifying app error or runtime fault arrives, including in a batch | Log replaces the active tool drawer and the entries are marked seen | Vitest |
| Log is opened and receives more entries | Existing entries and arrivals while open are seen. Closing Log leaves a zero badge until a new unread error arrives | Vitest |
| A different tool is selected or the active tool is toggled | The new tool replaces the active drawer. Toggling the active tool closes it | Vitest |
| Two instances use two injected stores | Entries, unread count, drawer selection, and live Follow signals are independent. Each instance uses the existing `digikit.logFollow` key | Vitest |
| A user opens Log or Analyzer, uses the resize control, or switches tools | The shell selects and displays the right drawer. Pointer and keyboard resizing and the existing layout still work | Simulated preview |
| A user opens a Log fixture and saves | The fixture follows its simulated sources and the save action reaches the mock adapter | `log-save` preview scene |

The desktop host suite and both desktop type checks passed. The simulated preview passed `analyzer-scope`, `log-save`, and an explicit check of keyboard and pointer resizing, saved height, and reopen. The live preview entry was type-checked.

## 2. Emulator session and shell

### 2a. Own the emulator lifetime

Keep one session owner mounted for the Workbench window. It starts the native runtime, hands off launch arguments, schedules the source, audio, and health polls, and releases the runtime at disposal. Keep the existing `native-runtime.ts` protocol and its generation protections. The shell can supply the existing launch decisions and poll policies while this boundary settles.

- First test disposal during startup. Hold `Host.emu.startup` pending under a Solid root. Dispose the root, resolve startup, and verify that the adapter is released, it publishes no late snapshot, and the Workbench schedules no polling.
- Test cleanup after a successful start and during an in-flight launch or poll. Reads or launch results that finish after cleanup must leave the shell untouched.
- Preserve the one-second polling cadence, native launch arguments, saved DSP choice, and the user-visible boot state. Keep the faceplate and emulator alive across page navigation.
- Run the desktop host suite and both type checks. In the simulated preview, run `run-target-switch` and `performance-emulator` to cover switching targets during a boot, navigating away and back, and load readings while running, paused, and resumed.

### 2b. Move firmware operations and narrow the shell

1. Put boot, firmware reload, module reload, reset, and file-watch policy under the emulator feature. Give the operations access to the narrow target, job, firmware default, inventory, and log capabilities they use. Keep boot state and preferences private. Preserve job grouping, held-key release, success and failure logging, and card refresh.
2. Bind Reload with runtime to the Workbench window. Transfer and Memory use the existing desktop capability. Dispose the binding with the shell so closed windows cannot receive the action.
3. Once firmware operations are independent, rename `Console.tsx` to `WorkbenchShell.tsx`. Pull navigation and status into focused components with direct inputs. Use Solid `<Switch>` and `<Match>` for the three drawer choices. Preserve the upstream module pages and keep visible selectors stable.

| Behavior | Lowest test seam |
| --- | --- |
| Launch file and DSP flag precedence | Compose the firmware owner with a fake runtime and memory preferences. Assert the launch file and options reach the runtime and the launcher choice stays session-local. |
| Last-file load deferred on a hardware target | Change page and target through their accessors. Assert a single boot on the first eligible visit to Emulator. |
| File watching and shutdown | Feed the firmware owner source readings with and without a changed timestamp. Resolve a pending operation after disposal and check for stale notices, preference writes, or refreshes. |
| Reload with runtime from Transfer | The Vitest integration case uses the production emulator composition, the real Transfer target and job owners, and a fake Host. It starts with stock firmware, runs the shared recovery action, verifies Transfer is available again, and checks that disposal disables the action. |
| Rendered shell and firmware controls | Simulated preview smoke checks cover target switching, performance, booting and failed boot, the Analyzer drawer, a Transfer-to-Files round trip, and stock reload followed by recovery from Memory. |

Firmware behavior is covered in `apps/desktop/ui/tests/emulator-firmware.test.ts`. The composition workflow in `apps/desktop/ui/tests/emulator.integration.test.ts` runs under the existing desktop host target without an emulator or browser. The final independent review checked lifecycle, shell wiring, and test assertions. Use the real emulator or Tauri checks when verifying behavior that depends on those boundaries.

## 3. Inspector, Samples, and adapter

These edit surfaces can proceed in separate worktrees once shell-facing interfaces are stable. Keep one coder per worktree and review each slice against its base.

| Slice | Candidate files and boundary | Proof |
| --- | --- | --- |
| Profile and Calibration | Move target-bound polling and action coordination out of `ProfilePanel.tsx`. Retain `createReadGate` and the `Recorder` lifecycle API. Move the recorder to `profile/recorder.ts` and extract the history and reference views. Keep calibration's counter ownership and replace broad state setters only where the callers expose a useful command. | Host cases for target change, in-flight reads, or calibration ownership as affected. Check Profile and Calibration in the preview. |
| Samples | Compose one Samples app per desktop window using injected storage and Host capabilities. Keep the source state for the window and the destination state for the selected Target lifetime. Use the shared Transfer runner for uploads, other target actions, and readback. Keep pane gestures and dialogs in the views. | Host regressions for state isolation, target changes, and bulk claim handoff. Run a fake-I/O upload through the production Samples and Transfer owners, then smoke test the rendered interactions. |
| Tauri adapter | Prove analyzer open/read/close ordering across drawer lifetimes and recovery after a rejected call. Test the desktop adapter by intercepting IPC at the Tauri boundary. Keep representative byte and header assertions tied to the Rust command contract. | Focused host adapter tests. Use native checks when the behavior depends on Rust or the webview. |

### Profile and Calibration acceptance

- A Profile owner belongs to one target binding. It drops results from an invalidated read, stops its polling at disposal, and releases the original recorder's DSP capture after any pending action settles. An export that has already started can finish saving before release.
- Profile and Performance share one recording. Full reads follow the visible Profile tab, and the Performance drawer can collect spans in the background.
- Calibration keeps its run owner across tab changes. A host integration case uses the Profile owner, Calibration state, and their shared hub over fake I/O. It checks that a pending profile result is discarded, no new reads or recording actions start during calibration, and reads resume afterward.
- Move the recorder to `profile/recorder.ts` without changing its API. Update the preview mock alias with its import. Split the history and reference views while keeping the saved comparison as target-bound state.
- The desktop host suite covers target leave, recording release, export completion, refresh, and Calibration handing reads back. Reference tests cover stale file and symbol replies and disposal.
- The simulated preview exercised `performance-spans`, `profile-nodebug`, `profile-calibration-run`, `profile-calibration-fit`, and `profile-calibration-estimate`. A rendered smoke check loaded a saved reference and verified that the comparison and hotspots survived a tab change. The Profile owner and handoff tests use fake I/O.

### Samples acceptance

- [`samples/source.ts`](../../apps/desktop/ui/src/samples/source.ts) owns the remembered `digikit.samplesFolder` preference, the open local path, added files, and their selection. A second window gets a separate source owner. Local listing and labeling can finish while the target read is waiting. Upload planning captures the original local root and files.
- [`samples/destination.ts`](../../apps/desktop/ui/src/samples/destination.ts) binds to the Workbench Target selection. A successful switch starts with a fresh destination folder, listing, selection, and upload history. A failed switch keeps the selected target's presentation and replaces the released session on the next operation. If the failed switch interrupted the first listing, Transfer requests another read of the retained target either when the switch settles or when the page remounts behind the old read. Old reads and writes lose their authority even after returning to the same port.
- [`samples/device-session.ts`](../../apps/desktop/ui/src/samples/device-session.ts) owns the device session, USB claim, and SysEx fallback. Leaving waits for claims and their cleanup on the shared native USB slot. A pending MIDI nonce before a claim does not hold up a switch.
- [`samples/app.ts`](../../apps/desktop/ui/src/samples/app.ts) owns the Sample commands. The plain TypeScript [Transfer runner](../../packages/workbench-core/src/transfer/run-transfer.ts) keeps busy state, cancellation, logging, and readback shared with Modules. If the selection has changed by the time a stale Samples job ends, the runner schedules a current-target read unless the page already queued one. When the old read finishes before the switch settles and Transfer is closed, Transfer refreshes on its next mount.
- [`LocalSamplesPane`](../../apps/desktop/ui/src/samples/LocalSamplesPane.tsx) and [`TargetSamplesPane`](../../apps/desktop/ui/src/samples/TargetSamplesPane.tsx) reuse Transfer's selection and list primitives. `SamplesMode` composes the panes and routes cross-pane drops. The local pane stays visible and browsable while the target is loading or unavailable. The target pane hides its rows and actions and shows the appropriate status, and Upload requires an available target. The target pane clears its folder editor and delete confirmation when the target changes or becomes unavailable and releases its confirmation timer on unmount.
- The [Samples host tests](../../apps/desktop/ui/tests/samples-backend.test.ts) check window isolation, delayed listing success and failure after A → B → A, stale upload planning, failed-switch deletion, new-target readback with and without a queued read, and late USB claim cleanup. The upload workflow uses the real Samples and Transfer owners over fake folder and session I/O and checks progress, verification, and the refreshed listing.
- The browser preview covers rendered keyboard and marquee selection, local and target navigation, folder upload and cancellation, drag and drop, delete confirmation and progress, SysEx fallback, local polling, and narrow scrolling. Rendered checks also verify that switching targets preserves local selection, reopening Transfer behind a delayed outgoing read refreshes the replacement target, and the local pane remains browsable while the target waits, fails, or disconnects. The `samples-failed-target-first-read` and `samples-failed-target-remount` scenes each verify exactly one recovery read and a visible local listing. The `samples-timeout-first-read-after-remount` scene verifies that an ordinary read makes one request and preserves its timeout after remount. Host tests cover the owner timing races deterministically. Real device and emulator transports remain outside this proof.

### Tauri Analyzer adapter acceptance

The drawer builds an Analyzer stream owner for each mount. The Tauri Host instance survives those mounts and queues Analyzer IPC in issue order. Rust owns the current session ID and the emulator and remote PCM sources. Opening a session replaces the previous one. Reading or closing a stale ID leaves the current capture alone.

- Add `apps/desktop/ui/tests/tauri-analyzer.test.ts` in the desktop host suite. Mock Tauri `invoke` at the import boundary and exercise the production `createTauriHost().analyzer`. Hold an IPC promise pending, queue a close and reopen, and check that each command starts only when the preceding call settles. Assert the source and session ID sent to Rust.
- Reject one queued call and check that its caller sees the rejection and the next command still runs. Check both a representative Analyzer PCM response (`generation`, `rate`, `left`, `right`) and the empty response. A separate raw-body adapter case can use Log save to check bytes and the encoded filename header against the Rust contract.
- Exercise one close-and-reopen workflow with two production stream owners using the same Host over mocked IPC. Hold the old read pending, dispose the old stream, and start the new one. Check that old PCM is discarded, the old ID is released, and the new stream receives PCM. A close submitted after the new open can reach Rust in that order. The existing native test covers stale close preserving the current capture.
- The host suite covers the TypeScript boundary without opening a webview. All four cases are in `apps/desktop/ui/tests/tauri-analyzer.test.ts`. The simulated Scope and Spectrum scenes passed. A native drawer smoke check remains open for real IPC and PCM serialization.

## Navigation and naming

Organize folders by feature as each slice changes. Start with shell and log, then emulator, inspector, samples, remote, and analyzer where the feature has enough files to justify a directory. Colocate a feature's components, styles, and supporting logic. Prefer names such as `drawer-sizing.ts`, `inspector-tabs.ts`, and `recorder.ts`. Keep production and preview imports direct. If a later move changes a path named by an Oxlint exception, a Vite alias, or a Vitest glob, update and verify that reference in the same slice.

After the adapter slice, inspect `src/remote/` and `src/analyzer/` as separate cleanup steps. Remote's page, audio controls, held-key behavior, and audio owner belong near each other. Give the INFO-port device lookup a shared home so Memory, Profile, Calibration, and log naming can use it directly. Its inputs are firmware INFO values such as a device ID with a version suffix. Keep that contract distinct from MIDI port display-name matching. Preserve held-key release on target leave and the audio owner's lifetime. Analyzer's panel, plot, stream, measurement, frame types, and worker belong near each other. Retain the stream cleanup and stale-frame checks across drawer lifetimes. Keep the shared Host adapter and worker factory at their existing boundaries, and update the worker entry URL when moving the Analyzer worker. Use the existing host suites and Remote and Analyzer preview scenes as the proof for each move.

## Review and commit

A coder implements one slice. Run its focused checks, then have an independent reviewer examine the complete diff, including new tests, before committing code. Resolve findings and rerun the checks affected by the fixes. Ask a fresh reviewer to check any behavior changes made in response to review.

Update the progress table after verification or when a review changes the direction. Record what passed, any open issue, and the commit hash when referencing an earlier commit.

## Verification

- Iterate with `pnpm exec vitest run --config vitest.host.config.ts --project desktop-host <test-file>`. Run the test that owns the behavior being changed, with existing tests for nearby boundaries.
- Verify the slice with `pnpm nx:pilot run @digi/desktop:test-host`, `pnpm nx:pilot run @digi/desktop:check:ui`, and `pnpm nx:pilot run @digi/desktop:typecheck`. Run Oxlint and Prettier on the changed files. Triage Solid reactivity warnings where the slice changes reactive reads.
- Use `mise run wb:preview` for the rendered UI while iterating. Run `node tools/devtools/ui_shots.mjs analyzer-scope log-save` for the first slice, plus the drawer interactions above. The standard preview substitutes simulated services at the Host boundary. Vitest owns exact state assertions and the native boundary has its own tests.
- Test feature owners through named commands and read accessors. Add a small integration case for the important workflow when separately owned pieces are wired together. Use the production composition and fake platform I/O. Keep the preview for checks that need the rendered controls or layout.
- Use the affected merge gate once for changes headed for merge. Select device coverage only when the behavior under test needs firmware or hardware.
