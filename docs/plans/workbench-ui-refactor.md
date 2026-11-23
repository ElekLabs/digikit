# Workbench UI refactor

Make the desktop Workbench easier to navigate and safer to change. Each feature should have an identifiable owner for its state and asynchronous work, an API that callers can understand from its types, and a host test seam for its behavior. Preserve how the Workbench operates across navigation, target switches, and reopen. This work starts with Log and tool drawers on `work/workbench-log-drawer`, based on the local `origin/main` at `85e056bf9`.

## Progress

| Slice | State | Evidence |
| --- | --- | --- |
| Log and tool drawers | Committed as `056dd95ec` | Desktop host and type checks, simulated drawer and save checks, independent review. Review caught a live fixture behavior change that was corrected before the commit. |
| Emulator lifetime | Verified and reviewed | The window owner discards late startup results and clears its polls. A single controller stop waits for pending firmware work. The final desktop host and type checks, both simulated scenarios, and an independent re-review passed. |
| Firmware operations and shell | Planned | Starts after the lifetime boundary is verified and reviewed. |
| Inspector, Samples, and adapter | Planned | Separate edit surfaces once the shell-facing interfaces are stable. |

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

1. Put boot, firmware reload, module reload, and file-watch policy under the emulator feature. Give the operations access to the narrow target, job, firmware default, inventory, and log capabilities they use. Preserve job grouping, held-key release, success and failure logging, and card refresh.
2. Preserve the deferred last-file boot when a hardware target starts selected, refused-load recovery, and upstream module-page and inventory behavior. Prove boot decisions and runtime behavior in their existing host tests.
3. Rename `Console.tsx` to `WorkbenchShell.tsx` once its remaining responsibility is composing navigation, page content, and drawers. Pull the navigation and status bar into components with direct inputs. Use Solid `<Switch>` and `<Match>` for the three drawer choices. Keep selectors stable through the preview checks.

Use the real emulator or Tauri checks when verifying behavior that depends on those boundaries.

## 3. Inspector, Samples, and adapter

These edit surfaces can proceed in separate worktrees once shell-facing interfaces are stable. Keep one coder per worktree and review each slice against its base.

| Slice | Candidate files and boundary | Proof |
| --- | --- | --- |
| Profile and Calibration | Move target-bound polling and action coordination out of `ProfilePanel.tsx`. Retain `createReadGate` and the `Recorder` lifecycle API. Move the recorder to `profile/recorder.ts` and extract the history and reference views. Keep calibration's counter ownership and replace broad state setters only where the callers expose a useful command. | Host cases for target change, in-flight reads, or calibration ownership as affected. Check Profile and Calibration in the preview. |
| Samples | Move the Samples slot wiring out of `desktop-env.ts` into the Samples feature. Create window-owned local-folder, target-folder, and upload state. Preserve the shared Transfer slot, independent local and device reads, target reset behavior, cancellation, and bulk fallback. Keep selection gestures in the pane and reuse Transfer's existing primitives. | Existing Samples host tests plus a focused stale-result or isolation regression if the seam changes. Exercise cancel, folder navigation, and target switching against preview fixtures. |
| Tauri adapter | Prove analyzer open/read/close ordering across drawer lifetimes and recovery after a rejected call. Test the desktop adapter by intercepting IPC at the Tauri boundary. Keep representative byte and header assertions tied to the Rust command contract. | Focused host adapter tests. Use native checks when the behavior depends on Rust or the webview. |

## Navigation and naming

Organize folders by feature as each slice changes. Start with shell and log, then emulator, inspector, samples, remote, and analyzer where the feature has enough files to justify a directory. Colocate a feature's components, styles, and supporting logic. Prefer names such as `drawer-sizing.ts`, `inspector-tabs.ts`, and `recorder.ts`. Keep production and preview imports direct. If a later move changes a path named by an Oxlint exception, a Vite alias, or a Vitest glob, update and verify that reference in the same slice.

## Review and commit

A coder implements one slice. Run its focused checks, then have an independent reviewer examine the complete diff, including new tests, before committing code. Resolve findings and rerun the checks affected by the fixes. Ask a fresh reviewer to check any behavior changes made in response to review.

Update the progress table after verification or when a review changes the direction. Record what passed, any open issue, and the commit hash when referencing an earlier commit.

## Verification

- Iterate with `pnpm exec vitest run --config vitest.host.config.ts --project desktop-host <test-file>`. Run the test that owns the behavior being changed, with existing tests for nearby boundaries.
- Verify the slice with `pnpm nx:pilot run @digi/desktop:test-host`, `pnpm nx:pilot run @digi/desktop:check:ui`, and `pnpm nx:pilot run @digi/desktop:typecheck`. Run Oxlint and Prettier on the changed files. Triage Solid reactivity warnings where the slice changes reactive reads.
- Use `mise run wb:preview` for the rendered UI while iterating. Run `node tools/devtools/ui_shots.mjs analyzer-scope log-save` for the first slice, plus the drawer interactions above. The standard preview substitutes simulated services at the Host boundary. Vitest owns exact state assertions and the native boundary has its own tests.
- Use the affected merge gate once for changes headed for merge. Select device coverage only when the behavior under test needs firmware or hardware.
