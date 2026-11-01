# Nx pilot

Status: dependencies installed without lifecycle scripts; all three resolved targets inspected with
Nx 22.7.12 and the project graph exported to `.nx/pilot-graph.json`, with `Cargo.lock` unchanged.
Targeted Prettier checks passed for six configuration files; independent review reported no issues.
The bounded host proof is complete after cache/workspace metadata isolation: six tests passed fresh,
repeat and unrelated-README probes hit the cache, a temporary failing test caused a miss and failure,
and restoring the original files recovered the original successful cache entry. See the
[observations and limits](../findings/nx-pilot.md). The named Rust target also executed once: only
dkneeds compiled and all ten tests passed, with caching disabled. The named runtime file passed its
two original DT2 1.17/DN2 1.12 cases using existing daemons and isolated local inputs/outputs, also with
caching disabled. All three execution checks are complete; Rust/runtime cache input audits remain
open. The host preflight migration below passed focused validation; no gate speedup
is proven. Existing full-host commands and merge-gate routing remain the
owners of merge acceptance. The parked `--only` routing change is independent of this pilot.
Five host groups now enable local result caching: control-client, test-support, dk-session, desktop-host
and regression-host. Both extensions passed dependency failure/restoration probes; only the earlier
support/session extension has a bounded timing pair. The integration gate was attempted and aborted;
[PR #237](https://github.com/ElekLabs/digikit/pull/237) merged at `a150cac5` after focused verification
and the authorized one-time full-gate exception, without a full-gate pass. Follow-up
[PR #238](https://github.com/ElekLabs/digikit/pull/238) adds diff-preserving explicit selection;
the routing-consolidation batch below follows those independent changes.

## Boundary

Pin Nx and `@nx/devkit` to 22.7.12. The evaluated `@monodon/rust` dependency has been removed. Use the existing Node 24.15.0,
pnpm 12.8.2 and Rust 1.98.1 pins in [mise.toml](../../mise.toml). The root `nx:pilot` script disables
the Nx daemon and Cloud. Use that entrypoint rather than a bare Nx invocation. There is no remote
cache or new gate. The explicit dkneeds command retains its exact Rust pin and offline/locked flags.
The script sets `NX_CACHE_DIRECTORY="$PWD/.nx/cache"` and
`NX_WORKSPACE_DATA_DIRECTORY="$PWD/.nx/workspace-data"` to absolute paths in the current checkout.
Nx 22 otherwise shares its cache and task database with the main worktree, even when this worktree's
`nx.json` sets `cacheDirectory`. Run the script from the repository root so cache and metadata stay
within this checkout. The initial shared Nx artifacts were preserved intact under
`.nx/pilot-validation/shared-cache-before-isolation`; the main checkout was clean at that isolation checkpoint.
The local cache and isolated Cargo build files live under ignored `.nx/`; do not share or upload them.

The initial pilot evaluated Rust plugin graph discovery with an include filter for
`packages/dkneeds/Cargo.toml`. It nevertheless called root `cargo metadata --format-version=1` for
the workspace before filtering. The plugin is now unregistered so host preflight does not acquire
a Rust toolchain or Cargo metadata prerequisite. Its unused dependency and unreachable lockfile entries
have been removed; broader Rust graph migration remains deferred. The explicit dependency-free dkneeds target
does not require plugin discovery. Historical plugin observations in the finding remain unchanged.

The plugin test executor accepts only stable/beta/nightly as its toolchain option, so it cannot express
the repository's exact Rust pin. The pilot uses the standard `nx:run-commands` executor for the test
command; the initial pilot used the plugin only for Cargo graph discovery. There is no custom plugin, executor, hasher,
runner or wrapper.

## Host preflight migration

The next bounded change replaces only `test:changed`'s full-host versus `vitest related` selection
with standard Nx affected selection. The initial three-target execution evidence above remains
separate from the focused migration checks below.

| Nx project with `test-host` | Vitest project |
| --- | --- |
| `control-client` | `control-client` |
| `@digi/test-support` | `test-support` |
| `@digi/dk-session` | `dk-session` |
| `@digi/web` | `web-transfer` |
| `@digi/desktop` | `desktop-host` |
| `regression` | `regression-host` |
| `module-tests` | `module-tests` |

Control-client, test-support, dk-session, desktop-host and regression-host result caching is enabled.
Web-transfer and the empty module-tests group still use `cache: false`.
Each fixed command runs its whole Vitest project; preflight supplies all changed paths on standard
Nx `--stdin`, rejecting newlines rather than splitting a pathname. `--parallel=1` preserves the
previous one-worker host budget. There is no root project covering the repository. Existing package
names and the regression project's ownership stay intact. Lint, formatting, types and the full
verification gate keep their existing logic.

Shared host inputs include the registry/config/toolchain files and desktop `ui/src` sources/assets,
preserving the conservative all-host trigger for filesystem reads. Known package/relative-import
edges are explicit where needed; desktop also declares device TOMLs and module manifests, and
dk-session declares the SDK format source. Deleted paths remain inputs to affected selection.
The distributed module-tests group currently has no tests: it retains its modules root and uses
`--passWithNoTests`. Add real helper/dependency inputs when introducing its first TypeScript tests;
there are no speculative dependencies on all host projects.

The web host target clears `DIGI_FW_EMU_SOCKET` so fast preflight cannot opt into the firmware
emulator test through inherited environment. That test remains available through its direct owner
workflow. Ignored WASM/generated/live inputs are another reason web result caching stays disabled.

Rust plugin registration and the script's Cargo environment overrides are removed for this migration.
Host graph discovery needs Node/pnpm, without Rust metadata, fetches or toolchain installation. The
unused plugin dependency was subsequently removed with an offline, lockfile-only, scripts-disabled
update; `@nx/devkit` remains pinned. Installed dependencies were reconciled with a frozen,
scripts-disabled install (policy metadata fetched, 576 packages reused, none downloaded, no version updates).

Focused validation passed: all 57 changed-tests Python cases (56 in the initial file run and the one
corrected test in isolation), plus one uncached `@digi/test-support:test-host` run, four tests in one
file. Eleven offline affected-project probes covered shared config, dependencies, deleted paths,
desktop filesystem assets, module tests and unrelated documentation and Python changes. Selection stayed the
same with an unavailable Rust toolchain after unregistering the plugin. The explicit dkneeds target
still resolved to its pinned command. Static review reported no issues. No whole-host suite or merge
gate was run for this migration, and no performance claim follows from these checks.

The support/session extension passed all 34 original tests, repeat hits, intentional dependency/source
failure probes and exact restoration. The [extension evidence](../findings/nx-pilot.md#support-and-session-cache-extension)
records its single locked timing pair and input-audit limits. Local caching assumes a frozen install;
audit new helpers, environment reads or external artifacts before extending the cache boundary.

The desktop/regression extension passed 140 original tests (99 desktop across 13 files, 41 regression
across five), repeat hits, imported dependency failure probes and byte-exact restoration hits. Both
use `^hostTests` with their existing explicit data inputs; see the
[extension evidence](../findings/nx-pilot.md#desktop-and-regression-cache-extension). No new timing was measured.

## Initial pilot targets and graph

| Named target | Existing work invoked | Cache | Scheduling |
| --- | --- | --- | --- |
| `control-client:test-host` | Root host Vitest config, project `control-client` | Local | Normal host target |
| `dkneeds:test` | `cargo test -p dkneeds --lib --locked --offline`, Rust 1.98.1 | Disabled | `parallelism: false`; existing shared benchlock |
| `regression:service-commands` | Runtime Vitest project, only `runtime/service-commands.test.ts` | Disabled | `parallelism: false`; one Vitest worker |

Project discovery lists `control-client`, `@digi/test-support`, `@digi/dk-session`, `dkneeds`,
`regression`, `@digi/desktop` and `@digi/web`. The resolved dkneeds target has caching disabled and
the configured named Cargo command; the initial plugin registration also supplied Cargo release metadata.
The exported project graph has no dependency edges for control-client or dkneeds and shows regression
depending on control-client, `@digi/test-support` and `@digi/dk-session`. There is no generated-core
build edge for dkneeds. This pilot adds no `dependsOn` or prerequisite
build targets. Runtime inputs are prepared separately. Other existing package scripts may appear
as Nx targets through package discovery; do not use broad `run-many` commands for this pilot.
The three commands disable automatic argument forwarding to keep their suite and package scope fixed.

The host target has no persistent test outputs to restore (`outputs: []`); a cache hit replays the
successful test result and terminal output. Cargo's `.nx/cargo/dkneeds` is a separate incremental
build directory, not a declared task output and not a generated-core/PGO target directory. Nx's
`parallelism: false` serializes heavy targets within one Nx invocation, not across separate Nx processes
or other tools. The dkneeds command itself acquires the repository's existing shared benchlock before
Cargo; do not wrap it in another lock. Do not run any pilot validation alongside PGO work or a gate.

## Inputs and limits

The host hash includes all control-client files, the host Vitest config and project registry, root
package/lock/workspace files, mise and Nx config, the resolved Vitest dependency, Node version/platform/
architecture, pnpm version, and selected Node/locale environment variables. The current tests use mock
Unix sockets and their own sources; they do not start emulator daemons. New helper imports or filesystem
reads require an input review before relying on a cache hit.

The declared Rust inputs include all dkneeds files (including committed `src/gen_limits.rs`), SDK format source,
workspace manifests and lockfile, tracked root Cargo config, tool pins, the existing benchlock helpers
and Python dependency metadata, pinned rustc/Cargo identity, and selected Rust flags/wrapper/test
environment variables. All workspace manifests are intentionally conservative because Cargo resolves
the workspace even for a named package. The command compiles/tests only the dependency-free dkneeds
library and uses its own Cargo target directory. It does not regenerate SDK sources.

Rust caching stays disabled pending an input audit covering global Cargo configuration, linker/compiler
changes, arbitrary Cargo environment overrides and local dependency patches outside the checkout.
Its declared inputs are a starting point for that review, not a claim of complete closure. This pilot
tests Rust graph and execution compatibility independently of caching. Do not add inline global-config
hashing or a custom hasher to enable it.

The host pilot assumes installed JavaScript packages match the frozen lockfile. Keep installation fixed
during validation; use `--skipNxCache` after local package modifications and clear the local Nx cache
before trusting hits again. No cache correctness or speed claim is approved until the protocol below
passes. This is a local pilot, not a guarantee for remote or cross-machine cache reuse.

Runtime caching stays disabled. The named test reads ignored/external daemon binaries, firmware,
ready state, cards and prepared module artifacts. Tracking their filenames or a Git diff would not
prove their contents are unchanged. No attempt is made to hash that closure yet.

## Commands and prerequisites

After dependency installation and after PGO work has finished, from the repository root:

```sh
pnpm nx:pilot show project control-client
pnpm nx:pilot show project dkneeds
pnpm nx:pilot show project regression
pnpm nx:pilot graph --file=.nx/pilot-graph.html
pnpm nx:pilot run control-client:test-host
pnpm nx:pilot run dkneeds:test
pnpm nx:pilot run regression:service-commands
```

For a worktree reusing an existing Python environment, optionally set `UV_PROJECT_ENVIRONMENT` to
its absolute path and `UV_NO_SYNC=1` to avoid synchronization, with
`UV_CACHE_DIR="$PWD/.nx/uv-cache"` for local uv artifacts. If `out` is not linked to the shared
repository, supply an absolute `BENCHLOCK_PATH` to that repository's existing `out/.benchlock` so the
Rust target coordinates with other builds. `RUSTUP_AUTO_INSTALL=0` prevents implicit toolchain
installation; put the pinned Node/pnpm binaries on `PATH`. These are execution prerequisites, not a
claim that the disabled Rust result cache covers those environment inputs.

The runtime command requires prebuilt, preflighted `DIGI_EMUD_DT2` and `DIGI_EMUD_DN2`, complete ready
inputs for DT2 OS 1.17 and DN2 OS 1.12, prepared `svc-echo.dkm` artifacts for both devices under
`out/regression/modules/<device>/` (or `DIGI_RUNTIME_MODULES`), and the DT2 card sample directory at
`out/runs/patchkit/samples`. The shared live helper requires that directory even for service commands.
See [regression prerequisites](../../tests/regression/README.md#prerequisites) and
[testing inputs](../TESTING.md#inputs). Prepare inputs using the existing owner workflows separately;
the Nx target invokes Vitest directly and never builds daemons, module artifacts, plusdrive or PGO.
Do not substitute `mise run test:runtime`: it also builds plusdrive.

## Validation and remaining work

The coordinator owns installation and the dependency-only lockfile update. Offline project discovery,
all three resolved targets and the exported project graph have passed inspection. Preserve the named
commands without workspace-wide Cargo tests, generated-core builds, implicit runtime preparation,
Cloud connections or added dependency tasks.

The following bounded protocol completed for `control-client:test-host` with fixed toolchains and
environment; config/lock/toolchain coverage was reviewed statically rather than individually probed:

1. Run normally once to execute tests and populate the local cache, then repeat normally. Record
   the task hash, fresh result and explicit cache-hit evidence; they must agree for the same inputs.
2. Change an unrelated Markdown file and repeat. Require the same hash and a local cache hit, then
   restore the file.
3. Introduce a reversible failing assertion in a control-client test. Require a new hash and an
   executed failure rather than a replayed pass; restore it and require the original cached result.
   Review config/lock/toolchain input coverage. Add another miss check or a `--skipNxCache` run only
   if that review or the observed results leave an unresolved concern. Keep fresh and cached evidence
   explicit. A performance claim requires the repository's approved paired measurement.

The resolved Rust target's `cache: false` and one functional run have been checked: only dkneeds
compiled in the isolated target directory and all ten tests passed, with `Cargo.lock` unchanged.
No Nx result cache entry was present for that run. Enabling Rust result caching remains a separate follow-up requiring Cargo input
closure review and the full hit/miss/fresh-agreement protocol above, including Rust source, manifest,
configuration and toolchain changes.

The resolved runtime target's `cache: false` and one functional run have been checked: the two original
service-command device cases passed using prebuilt daemons and copied inputs, without daemon/module
build targets or plusdrive CLI invocation. Local patched-firmware and card generation occurred within
the owned pilot directories. No Nx result cache entry was present; `Cargo.lock` remained unchanged.
This is functional acceptance evidence, not proof of runtime cache safety or independently audited
ready-snapshot provenance; see the finding for the older ready metadata labels and actual image checks.

Host test selection in preflight is now migrated; the full merge gate remains unchanged. Further
repeated pilot runs are not required without a new change or unresolved concern. Rust graph adoption
and runtime input modeling remain separate follow-ups; Rust/runtime result caching stays disabled
pending its input audits.

Before integration, run the applicable formatting, path and routed merge checks once under the
coordinator's budget. Do not remove existing routing until the pilot is validated, a unit's full input
and acceptance contract has migrated, and that unit's old selection logic is explicitly retired.
Any failure of graph isolation, input invalidation or fresh/cached agreement blocks migration.

The warm integration lane attempted the broad gate in `out/runs/verify/20261101-043906`; it did not
pass. The parent stopped runtime execution after the user challenged the coverage breadth. Focused
host and web follow-up passed, as did the forced Rust compile with the final absolute pinned compiler
override. Runtime sample failures have not been rerun after providing the missing WASM prerequisite. See the
[integration checkpoint](../findings/nx-pilot.md#integration-gate-checkpoint) for the exact limits.

The next priority is tighter gate selection using static dependency ownership and existing phase
evidence, not more caches or custom tracing. Existing runtime evidence already identifies fixed waits;
do not repeat broad acceptance for observation or timing. Focused checks are complete and the user
authorized the one-time full-gate exception for PR #237, now merged at `a150cac5`. The exception is not
evidence that the aborted gate passed. The `--only` follow-up adds diff-preserving explicit selection;
automatic routing is implemented and verified in the separate batch below.

## Routing consolidation

The last-25-merge baseline was saved before this batch. The bounded changes remove the routing
files' automatic all-step/all-Python/all-Rust escalation, reuse the existing seven-file
`changed_tests.FAST_PYTHON_TESTS` audit to release those jobs from build barriers, and retain only
existing Rust/build ownership for crate-level emulator integration-test sources. Unknown device
modules keep importer tests and mapped importer scenario kinds, with `test_verify.py` explicitly
selected to enforce `DEVICE_KINDS` classification. Owner tests, import-graph consumers, `PY_GATE`,
static checks, explicit full mode and mixed production coverage remain in place.

The audit found production/preparation consumers of emulator examples: coupled imports boot common
helpers, ready preparation uses DSP/SHARC examples, and card checks use plusdrive. Examples therefore
retain production routes. No generic deleted-file exemption is introduced: absent modules can be
missing from the current import graph, leaving ownership unproven. Root manifests and Vitest
configuration retain conservative shared coverage. The existing `acceptance_python` closure remains
unchanged because its consumed preparation/client entry points have real production dependencies.

`docs/TESTING.md` now reflects the actual 16 smoke cells, four maintenance exemptions, existing
test-only/patchkit-group exemptions, and seven fast devtools files.

The historical comparison uses exactly the 25 first-parent landed commits recorded in
`out/runs/verify/20261101-062531/selection-audit/manifest.json`, from baseline `a6c29b4f` through `11ad79ee`.
For each manifest entry, run `.venv/bin/python -m tools.devtools.verify --base COMMIT^1 --head COMMIT --explain`
with the baseline router and the implemented router. The same audit directory retains `comparison.json`
and the exact 25 raw logs in each of `before/` and `after/`.
Eleven plans changed and fourteen stayed unchanged. Representative selections:

| Landed change | Steps before → after | Python files before → after |
| --- | --- | --- |
| #238 | 17 → 1 | 115 → 9 |
| #225 | 17 → 1 | 115 → 9 |
| #230 | 18 → 3 (`runtime-fixture`, `python-tests`, `smoke`) | 116 → 29 |
| #231 | 18 → 18 | 188 → 183 |

Production and root-manifest coverage stayed in place; examples remain conservative for the
consumers described above. This comparison measures selection, without executing historical suites.

The routed gate in `out/runs/verify/20261101-062531` selected nine Python files: 689 cases passed,
with three stale test expectations failing. After correcting those expectations without changing
production code, the targeted retry passed all three cases in 1.67 seconds. All 692 cases therefore
passed across the gate and retry. Ruff, format, mypy, path, layer and generated-source checks all
passed; final independent review reported no issues. The gate's 87-second wall time describes this
routed run; it is neither a benchmark nor evidence of a full-suite pass or gate speedup.


## Bounded runtime readiness checkpoint (2026-11-01)

The readiness batch changes only remote-input and input-adapter workflows and their shared helpers.
It replaces selected startup/post-install sleeps with bounded observations of the current runtime
UI hook and normal boot, a newer epoch after the single install restart, and remote command readiness.
Fresh pre-install boots must have zero selected modules and a zero boot-counter snapshot; this does
not claim that a stability flag proves a persisted card update. ABOUT now requires an empty buffer
before opening and a complete runtime/BOOT/final RAM row snapshot. Awaited readiness RPCs share
absolute observation deadlines, including stalled reads; input hold/release behavior is unchanged.

Focused device proof passed: DN2 OS 1.12 ran all three selected cases in 203.62 s; DT2 OS 1.17 retried
all three in 278.20 s. Each invocation passed two files, with the other device's three cases skipped.
Local logs are `/private/tmp/runtime-readiness-dn2.log` and
`/private/tmp/runtime-readiness-dt2-retry.log`. The initial DT2 remote-UI case failed because post-restart
status reported `cf.ready=false` and an unobserved filesystem check despite a newer epoch and current
UI hooks. The reviewed restart contract preserves the card and relaxes that filesystem-worker check:
initial readiness still requires `cf.ready`; post-restart readiness permits an unobserved check but
rejects an explicit filesystem failure, retaining all current boot/UI/epoch guards. No filesystem
verification is claimed by a null result.

Before that adjustment, 49 host cases and static/type checks passed. The final helper file passed
25 tests, regression types passed, and independent review reported no issues. The removed scheduled
waits total at most 269 s across eight sessions (171 s initial, 80 s post-install, 18 s ABOUT); this is
a static ceiling, not an A/B measurement or measured speedup. Protocol-only startup, navigation,
key holds, remote refill pacing and faulttrace waits remain. Global settle helpers are unchanged.

This partial readiness migration merged in [PR #241](https://github.com/ElekLabs/digikit/pull/241)
at `0feb02cb` under Em's explicit acceptance of the focused runtime checks. The broad routed gate
(host, runtime-fixture across all ten runtime files, and smoke) remains unrun. This specific merge
exception is separate from the earlier routing-only exception and is not a broad-gate pass. The [timing finding](../findings/69-runtime-test-timing.md)
records the same scope and a future completion-event direction; no event facility is implemented here.


## Static coverage and cache audit checkpoint (2026-11-01)

**[D]** Provisional static reading corrects the proposed safe/crash deletion premise; it does not
verify firmware semantics against image bytes. Independent static review confirmed all three scenario
gaps; synthetic TypeScript decoder tests do not prove guest capture or cross-boot persistence. The [safe runners](../../tools/patchkit/pk_digikit_scen.py)
(lines 774–953) retain unique init-reset/safe-once, counter/result/selection and three-boot early/late
stability assertions on both devices. The [crash runner](../../tools/patchkit/pk_digikit_debug.py)
(lines 1277–1443) adds DT2 vectors 4/5, 16 registers, 32 stack words, cold-RAM behavior, record replay
and selection across boots absent from the [TypeScript crash case](../../tests/regression/runtime/fault-trace.test.ts)
(lines 213–237). Inputs are [safe-fault](../../tests/digikit/scenarios/safe-fault.toml),
[safe-late-fault](../../tests/digikit/scenarios/safe-late-fault.toml) and
[crash-capture](../../tests/digikit/scenarios/crash-capture.toml).

There is no established deletion-only set. Retain the four safe-cut/debug device wrappers, which
also own cut/card/debug coverage. Migration requires a fact-by-fact coverage comparison and device
proof before removing assertions. The [router](../../tools/devtools/verify_route.py) currently sends
a generic deleted scenario TOML to the full digikit family; no routing change is justified without
an actual deletion. A contract-coverage guard enables safer thin tests, but neither proves duplicate
coverage nor blocks separately demonstrated equivalent consolidation.

Further readiness work should replace only waits with proven observable completion conditions;
deliberate input pacing and stability-boundary tests stay. PR #241 completes a bounded subset, not
the roadmap's whole readiness goal. Existing Cargo incremental builds and
[packed-module reuse](../../tools/device/dktransfer.py) already avoid work. Build-cache input closure
is broad, and incremental ROI has not been established: rebuild byte identity alone proves neither
saved work nor worthwhile caching. No new benchmark, plugin or implementation is authorized by this
checkpoint; broader build/pack caching remains conditional on a justified candidate and input audit.

The remaining cheap readiness candidate using an existing symbol plan is debug-module
`expectWithoutDebugModule` (57 s static scheduled-wait ceiling across its cases), deferred rather
than opened as a separate small PR. Em first chose FX consolidation design, then approved its first fixture/PCM implementation;
the [current fixture-first FX design](test-architecture.md#fx-consolidation-design-2026-11-01) records
the approved direction and the [first implementation checkpoint](test-architecture.md#first-implementation-checkpoint-2026-11-01).
The fixture/PCM pilot is implemented; focused host/type checks and both first-run device proofs passed
(DT2 89.66 s, DN2 62.06 s invocation), as correctness evidence rather than a speedup benchmark.
Final `mise run test:changed` passed 612 Python tests and 301 host cases (one skipped), with lint,
format, types, path, layer and generated checks; log `/private/tmp/fx-reuse-preflight.log`.
The gate plan was explained only; the broad gate remains unrun.
Native preboot-card seeding and the existing PCM comparator resolved the first slice's seed/analysis
choices without a new codec or RPC. Legacy coverage remains intact; no speedup or full migration is claimed.

The first checkpoint left inferred needs, relocated DSP bytes/row pointers, selected-track type
and launch-time write-watch/flush/card-sector policy as legacy facts. The current increment below
adds safety/oracle checks; the approved gain-leg replacement below records the subsequent coverage decision.


The [PR #242 safety increment](test-architecture.md#safety-increment-in-pr-242-2026-11-01) now adds
startup-lifetime write/flush summaries, exact overlay policy and independent inferred-needs/DSP
relocation/row oracles, including post-note adapter checks. Revised DT2 and DN2 cases both passed
first run (63.62 s / 50.16 s invocation), with eight clean emulator summaries and four policy-valid
overlays each. The 11 new Python synthetic cases, 57 affected host cases, types and scoped lint
passed. Required fast preflight also passed: 612 audited Python routing tests and 75 host cases
(four test-support, 71 regression), with lint/format/path/layers/generated and all workspace TS types.
Mypy was skipped (no changed typed files); log `/private/tmp/fx-safety-preflight.log`. The fresh-process reference above is historical
one-time proof, removed from default acceptance. Only the redundant SDK patched load was removed;
CLI stock startup remains. At that checkpoint legacy `one()` was intact and selected-type coverage
awaited a decision; the approved replacement below records the later decision.


The [approved gain-leg replacement](test-architecture.md#approved-gain-leg-replacement-2026-11-01)
removes only Python `one()` and its two PCM comparisons after Em accepted the audible-input FX
contract. TS does not directly observe selected-track type during gain or deselection; legacy
dry/full retain that DT2 proof. All gain boot/metadata/needs/relocation/row/state/PCM/restart/safety
assertions map to the already verified runtime case; its execution is unchanged. Plain/dry/full,
fit/overflow and other families remain. The owner is `test:runtime -- fx-reuse`, not the stock-FX tasks.
Remaining legacy quick counts subtract one rig plus one restart per device: 35/38 rig entries and
three restarts each (38/41 static operations), excluding TS and preserving the historical counts
above. No measured-speedup claim follows. Follow-up fast preflight passed; acceptance was still pending at that pre-merge checkpoint.

The existing runtime-source map/import closure now includes the consumed machine/arena CLI helpers
and needs wrapper, so replacement coverage follows its real dependencies. Three focused routing
cases passed; final deletion fast preflight passed 345 Python and 71 regression-host cases, with
Ruff/mypy/path/layers/generated/oxlint/Prettier/all types (`/private/tmp/fx-gain-deletion-preflight.log`).

The actual-PR-base gate explanation (`origin/main` at `0feb02cb`) selects host/web, three stock,
11 runtime, three module-load and 49 Python files, smoke and an implicit daemon build. Its log
`/private/tmp/fx-gain-merge-explain-origin-main.txt` supersedes the stale-local-main (`365b45b`)
explanation. At that pre-merge checkpoint the broad gate was not run and focused acceptance was
still pending; the merge checkpoint below supersedes that status. No device rerun
was needed for this TS wording/comment-only follow-up to the prior both-device safety proof.


PR #242 merged at `a41699e5` (2026-11-01T09:46:15Z) under Em's explicit one-time focused exception
using the prior both-device, helper/host/routing and static/type proofs. The broad routed gate was
not run; no measured speedup follows. Next-batch bookkeeping maps the remaining basic-FX/fxpage and
other families for independent workers, as already endorsed after the first proof/deletion. The
shared fixture and assertion mapping carry forward; this is not a broad bulk-deletion plan.

FX SEND is the chosen next migration: five DT2/eight DN2 same-image legs. The mechanical shared
`fx-live.ts` extraction is the prerequisite for independent common-SEND/DN2 work, without changing
gain execution or basic plain/dry/full/fxpage coverage. No new device result or speedup is claimed.
