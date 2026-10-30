# Stereo sampler: numerical routing and memory diagnostics

Primary worktree: `/private/tmp/digikit-dsp-sampler`, sampler discovery branch.
Current implemented checkpoint: `1df2325c`. The ongoing source of truth is
[the plan](../plans/dsp-sdk-and-sampler.md); this handover is a short resume guide.
Other sessions may own the original checkout. Do not reset it or stop their runs.

## Constraints and next decisions

DT2 1.17 and DN2 1.12 remain the analysis/acceptance targets. The user requires
full stereo, sixteen tracks sharing deduplicated project backing, long samples,
and no arbitrary ten-second or 16 MiB product limit. Selecting a sample stops
old notes on that track. Modules use shared SDK contracts; device adapters ship
with the runtime. Existing module cleanup in other sessions must be preserved.

No production sample reservation, public stereo machine ABI, hardware flashing,
PCM pin change, merge or Python instruction-core regeneration is approved by
these diagnostic checkpoints. DN2 configured 512 MiB geometry is not a free-pool
proof. Bounded quiet observations do not establish ownership.

The bounded stock DT2 paired-input pan audit agrees with diagonal left-A/right-B
scaling; it does not prove UI or whole-pipeline equivalence. The local
DN2 reset/helper map and bounded reset replay now pass. Next resolve production
capture/commit stability, cleanup and route integration. The immediate AudioTask
caller chain has no local exclusion proof; do not assume it serializes all writers. Memory work must establish pre-reclaim reader retirement and
coexistence, not just search for an interval that appears quiet. DT2 release/reset
can reclaim before header clear; existing findings 81/82 already demonstrate why
header invalidation alone is inadequate.

## Completed and independently reviewed

- [Finding 96](../findings/96-stereo-machine-processing.md): all ten identified
  post-source families have bounded numerical examples, plus the source
  continuation. The selected composed route adds original scratch blending and
  output splitting. All 44 named replay tests pass without skips, including actual reset helpers. The candidate
  output mapping remains a bounded synthetic route, not full stock integration.
- [Finding 97](../findings/97-ddr-access-diagnostic.md): opt-in first-attempt DDR
  monitor, compiled native guard fallback, ordered diagnostic RPC, both native
  device acceptance cases, and 49 focused Wasm/V8 cases from real lowering.
  Wasm uses a checked simulated wasm32 layout; full browser-engine fallback/RPC
  and hardware behavior are not proved.
- Three flat-access tests became four with the Wasm export fixture. Native
  Cranelift guard-block ordering was fixed separately at `aed78721`.
- The RPC daemon is immutable at
  `out/runs/ddr-monitor-rpc/native-build/stable/digi-emud-6850e27b98d9`.
  Both current firmware inputs are hash-bound in the acceptance artifacts;
  legacy generated profile labels do not identify firmware versions.

The default reciprocal seed remains unknown for some Python-model routines.
Specific replay cases explicitly use the existing approximation; the new
composed route does not. No instruction semantics were changed. Exact reciprocal
behavior remains a separately scoped investigation, with regeneration costs.

## Validation and resume commands

Use the repository's shared benchlock at the original checkout's `out/.benchlock`
for native builds/runs; never overlap a gate with another build. Owner commands
and exact environments/logs are retained under `out/runs/ddr-monitor-rpc/`,
`out/runs/ddr-monitor-native/` and `out/runs/ddr-monitor-wasm/`.

The normal changed-files wrapper encounters the existing worktree pnpm
hoist/bootstrap layout for JS steps. Scoped installed-tool lint, formatting,
related-host checks and regression TypeScript checks passed. Python/docs scoped
changed-file checks, pathcheck and layercheck pass. Full workspace types and the
pre-merge routed gate are not claimed. No dependencies were changed to bypass it.

Numerical tests use immutable patchkit
`out/native/target-wt/digikit-dsp-sampler/nogen/stable/patchkit-344b899674ac`
through `tools.patchkit.patchkit._BUILT`, then run the named file
`tests/patchkit/test_dsp_stereo_shadow_replay.py`. This avoids rebuilding patchkit.
The original firmware bytes load at runtime from ignored section artifacts;
no firmware or sample audio is committed.

The secondary `/private/tmp/digikit-sampler-gain-proof` worktree retains uncommitted
proof files developed from an older base. All current additions were integrated
and checked in primary. Do not apply its whole diff again or discard it blindly.
Use only configured agents with fresh context, read-only reviewers, one writer
per worktree, and root-owned decisions/integration/publication.
