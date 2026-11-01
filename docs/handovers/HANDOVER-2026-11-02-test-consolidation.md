# Handover 2026-11-02: FX persistence/shifter consolidation and next Nx work

## Start here

The active integration checkout is `/Users/em/src/digi/lanes/nx-integration`, branch
`test/fx-persistence-shift-consolidation`, based on `4bd50f5b`.
The Claude checkout was left untouched. Do not switch it or reuse its worktree for integration.
Read [AGENTS.md](../../AGENTS.md), [testing](../TESTING.md), the
[FX assertion ledger](../plans/test-architecture.md#fx-kit-persistence-and-frequency-shifter-design-2026-11-02)
and the [Nx audit checkpoint](../plans/nx-pilot.md#family-project-audit-checkpoint-2026-11-02).

This is a draft captured before the final retirement/dispatch review, preflight and PR.
The parent must append the final integration HEAD, exact artifact/input paths, PR and check outcomes.
Do not interpret the four completed device proofs below as a completed merge gate.

## Decisions and committed checkpoints

Base `4bd50f5b` is merged PR #247 (BASIC FX/pages). Its PR-only exception applied to that batch;
it does not authorize skipping the gate for this batch. No current-batch exception is recorded.
These checkpoints were pushed on the active branch:

| Commit | Change |
| --- | --- |
| `9b23b0ff` | Fixed `fx-kit` write/flush safety policy |
| `6f12384f` | Shared KIT UI and load-observation helper |
| `73711a01` | Consolidated persistence scenarios |
| `01594832` | Consolidated frequency-shifter scenarios |
| `8b7c248e` | Explicit acceptance routing ownership |
| `4b7cbf8d` | Proof and Nx family-audit documentation |

At this snapshot the integration HEAD is `4b7cbf8d`; retirement and module dispatch are uncommitted.
The earlier retirement-only snapshot was +74/-1473 with ten deletions. The working diff now also
contains the module dispatch fix, so that count is not the final PR diff. Inspect the actual diff.

## Acceptance now implemented and proved

[Persistence](../../tests/regression/runtime/fx-kit-persist.test.ts) owns one fixture and test per device,
using one image containing `core/runtime`, `core/dynfx`, `core/fxpages` and an inventory of sine,
gain A and gain B with distinct manifests. DT2 runs the real sine-only Save/Load Kit, SELECT A/B
and restart, then SELECT B/A and restart. This full DT2 case subsumes the old quick DT2 setup;
there is no separate quick DT2 replacement. DN2 preserves its quick zero-ID setup and table swap.

Common persistence assertions retain A01/A02 kit identity, effect defaults, serialized values/IDs,
stock kit values, DSP words, PCM/page round trips and project save/load. Project save must call the
saver at least 100 times. Kit loads observe DSP changes every 2 ms for 600 ms; project load every
10 ms for 7 s, rejecting transient zero p0/mix. Original tolerances remain, with no added generic
peak threshold (`requireAudible: false`; legacy PCM comparisons own that oracle).

[Frequency shifter](../../modules/fx/freqshift/tests/freqshift.device.test.ts) owns four pristine card baselines
per device: dry, master, selected FX with no send amount, and send. Both devices execute all four;
DN2 must not accidentally omit the no-send case. It retains panel/kit/DSP mapping, stock/page
pixels, exact dry/mix-zero/other-track PCM, spectral rejection, send mix and independent float32
model assertions. Settings and route lifetime checks use `fx-routes`; persistence uses `fx-kit`.

Shared [KIT helper](../../tests/regression/runtime/fx-kit-ui.ts):
`createFxKitUi(runtime, plan, profile, artifacts)` exposes `fxPage(page)`, `saveKit(n, tag)` and
`loadKit(n, tag) -> { seen, file }`; `seen` contains `(ms, row, p0, mix)` transitions.
`watchFxKitRows` serves project load polling; `pageKit` accepts optional row zero by default.
The existing tap/release-in-finally and settling primitives are reused. Restart barriers retain the
original `afterEpoch`; silence or an old UI frame is not proof of a new boot.

## Completed device evidence

All four family/device proofs passed using prepared inputs. Durations are wall seconds;
artifact suffixes identify the temporary artifact directories, not stable archive paths.
The parent must append exact paths and retained logs before publication.

| Proof | Invocation / file / case | Artifact suffix | Safety evidence |
| --- | --- | --- | --- |
| DN2 persistence | 129.50 invocation / 123.760 case | `zDOVuw` | 3 watch windows, 2 overlays, `fx-kit` |
| DN2 shifter | 215.72 invocation / 209.578 case | `WDkalo9` | 4 baselines, `fx-routes` |
| DT2 shifter | 398.171 file / 375.478 case | `acXXAd9` | 4 baselines, `fx-routes` |
| DT2 persistence | 328.322 file / 320.201 case | `86nKeR7` | 4 watch windows, `fx-kit` |

The combined DT2 invocation took 726.77 s. All lifetime reports recorded zero illegal writes,
flushes and invalid sectors. These are acceptance and safety observations, not a paired performance
benchmark. Do not claim a speedup from them.

Prepared module artifacts are under `out/runs/fx-persistence-shift-consolidation/modules` in the
shared local artifact area. The original binary environment and store inputs were retained; the
parent must append their exact values rather than reconstructing or guessing them. Do not build
new daemons or replace those inputs merely to resume review. Temporary system artifact locations
can disappear; retain the evidence needed by the PR before cleanup.

## Retirement and module ownership: inspect the working diff

The ten planned deletions are five scenario TOMLs (three persistence, two shifter), four legacy
Python device wrappers, and the persistence runner. They are being retired only after the proofs.
The analysis functions remain: `pk_digikit_fxpage` supplies the PCM comparison CLI;
`pk_digikit_fxshift` retains spectral/model analysis and its JSON adapter. Shared FX/profile/card,
OLED and aligned-PCM analysis remain for their TS consumers. Do not delete independent analysis.
Routing, documentation, the safety policy's source of kit areas and discovery lists are updated
alongside retirement. The parent owns final deletion review and integration.

A final discovery review caught that deleting the module's scenario TOMLs would remove shifter
acceptance from `module:check`. The scoped fix is in progress, not yet accepted:

- One uncached `module-freqshift:test-device` target in the module's `project.json` owns acceptance.
- Its command uses existing `verify --tests modules/fx/freqshift/tests/freqshift.device.test.ts`, with no duplicate
  module-to-file ownership table.
- `pk_modcheck` dispatches the owned target after Python conformance/own tests succeed, before
  size/fit. Failure must stop later stages. Legacy daemon/input resolution is preserved.
- Device acceptance still runs both devices; `--device` retains its original conformance and
  size/fit scope. It must not silently narrow the new both-device proof.
- Nested Nx ownership preserves the existing `module-tests` host dependency on the module project.

Target resolution, ownership/closure checks and contract review are pending. Pure dispatch changes
do not justify rerunning the completed emulator proofs; test the routing boundary with minimal
host/static checks. Do not claim `module:check` itself passed unless it was actually run.

## Validation and next actions

The parent owns final scoped validation, changed-file preflight, `verify --explain`, gate decision
and PR. The known baseline pathcheck failures are in `docs/plans/module-architecture.md` at
lines 3, 4 and 284. Keep these distinct from new failures; no exception is yet granted for this batch.
The earlier worker preflight did not retain a final exit result: Ruff and layercheck passed,
pathcheck failed on those baseline references, and no process remained. It is not a gate pass.

1. Finish module target/dispatch contract review and inspect the final retirement diff.
2. Run the smallest relevant checks, then the parent-owned preflight and gate routing explanation;
   record actual outcomes and resolve the PR/gate decision with the user.
3. Commit reviewed checkpoints, update this draft with final HEAD/evidence/PR, and publish only
   within the user's authorization. Never reuse the PR #247 exception automatically.
4. Next, undertake the static Nx family-graph spike described in the Nx plan. The broader graph is
   **unimplemented**. The scout's 18-project candidate is not approval to widen scope.
   Map every consumer before deleting the old related selector. No extra cache or plugins.

For a fresh context, preserve one writer per lane. A scout may read bounded code; a worker may
make explicitly scoped changes; children do not delegate. The parent owns decisions, integration,
gates and publication. Use push checkpoints as authorized; do not commit firmware, generated
artifacts, cards or sample audio. Run minimal checks for the change, with no duplicate device sweeps.
Lane writes outside the sandbox workspace require targeted escalation; do not bypass a denial.
