# Stereo sampler: numerical routing and memory diagnostics

Primary worktree: `/private/tmp/digikit-dsp-sampler`, sampler discovery branch.
Latest bounded native checkpoint: the private first-stage proof described below
(source integration/publication is coordinator-owned). The ongoing source of truth is
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
- Optional `core/dsp-stereo-proof`, DN2 1.12 only: 39 model cases and both native
  identity/polarity modes pass. Each native mode records 2,400 render/shadow calls,
  one mirrored reset and zero mode errors/nonfinite observations/mismatches/fallbacks.
  Mode 1 observes 76,799 nonzero opposite-sign audio and 4,800 opposite-sign live
  history relations, with numeric MODE1 `0x39011cf8`. Both stock/patched 100 ms,
  4,800-frame stereo WAVs retain identical hashes and zero shift. This is finite
  metamorphic independence evidence, not an independent numerical oracle.
  Original left state/output remain authoritative; private L2 data occupies
  4,020 bytes, with no DDR reservation or default activation.
  U/Y caller/context audit is complete: both early/late calls share the same
  per-slot record/history and require both hooks plus the local reset. A bounded
  mode-0 interpreter profile positively observes early (2,400) and reset (one),
  with 2,850 helper entries across all callers, zero AOT/fast/traps and unchanged
  stock WAV. Late is unexercised here; gate stability is unproved. Implementation
  approach is now approved and implemented with persistent right histories across
  placements. Both owner model files pass 75 cases (39 first-stage +36 U/Y);
  combined two-mode native acceptance passes. Each stage records 2,400 calls and
  one reset mirror per mode, all errors zero; mode 1 observes 76,799 opposite audio
  and 4,800 opposite history relations at each stage. All stereo WAVs retain the
  stock hash, 4,800 frames/100 ms and zero shift. The appended source preserves the
  first-stage source/instruction prefix. Combined private data is 6,760 bytes,
  code 5,830 bytes/1,375 decoded instructions; the exercised U/Y model depth is
  176 bytes. Minimum native entry minus that model depth leaves 3,128 bytes in
  this scenario, not a native store watermark or universal stack bound. Both-stage
  native gates require zero errors/nonfinite/mismatches/fallbacks, U/Y reset and
  mode-1 opposite audio/history. Late-site native execution remains open.
  Full-route, production backing ownership and hardware acceptance remain open.
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
`out/runs/ddr-monitor-native/`, `out/runs/ddr-monitor-wasm/` and
`out/runs/dsp-stereo-proof/commands.json`. The last records the exact exclusive
benchlock/native invocation and immutable daemon/patchkit hashes. Current results
are `out/runs/dsp-stereo-proof/native/4c8488cb-3275-4c6e-a6f6-0a9dccef49d2/result.json`
and `out/runs/dsp-stereo-proof/native/cefabd29-f00f-413c-a368-f8db89f1b179/result.json`;
`out/runs/dsp-stereo-proof/native-polarity-acceptance.log` records both passes.
The earlier identity-only run remains separately recorded in provenance.
U/Y coverage has its own `uy_coverage` provenance entry, profile pair
`out/runs/dsp-stereo-proof/uy-interpreter-profile.e45298-1` and
`out/runs/dsp-stereo-proof/uy-interpreter-profile.e45298-2`, log
`out/runs/dsp-stereo-proof/uy-interpreter-profile.log` and result
`out/runs/dsp-stereo-proof/native/fd1c110f-a444-4e5f-ad50-42d2b4a4d11d/result.json`.
That run intentionally filters mode 1; no absence claim follows from fast-profile
missing call rows or this scenario's missing late row.
Combined observer acceptance is in
`out/runs/dsp-stereo-proof/native/5e1a8014-6b17-47b1-b3c6-c1d26d167f99/result.json`
and `out/runs/dsp-stereo-proof/native/a49fdec1-2e03-42a1-a0d0-58989c6c9ede/result.json`,
with `out/runs/dsp-stereo-proof/native-uy-acceptance.log`. Late-site positive
coverage remains open despite both callsites being hooked.

The private per-stereo-frame fetch experiment now passes 59 model tests, with no
runtime/core policy changes: both channels' packed taps/scalars are copied in one
admitted masked scope, count is released, and private compute/store/one cursor
advance follows exact mask restore. Denial zeros the remaining block without epoch
relatch. Actual DDR aliases and stock page geometry replace represented overlap
spans for this proof; the original DT2 guard helper supplies exact ten-byte stores.
All representative arrivals reduce to checked fetched-frame classes; a split-lock
control tears a real frame. Reader masked spans 80/81 and totals 5,172/5,204 are
model instructions only. Cursor/scratch must remain callback-owned after backing
count zero; production lifetime, IRQ/cache/coexistence and ownership remain open.
See [finding 99](../findings/99-stereo-frame-fetch-exclusion.md) and ignored
`out/runs/stereo-frame-fetch/commands.json` for scope, commands and evidence.

**[C][D]** AudioTask requests 1,000 four-byte units, not `0x1000` bytes:
`0x1ca009` loads `0x3e8`; creator shift `0xb88840` precedes allocation at
`0xb88853`. For `T=DM32(0x268a84)`, `S=DM32(T+0x30)`, allocation is
`[S,S+4000)`. Observed native entry minimum minus the model-exercised 172-byte
store depth leaves 3,132 bytes in this scenario. This is not a native store
watermark or worst-case interrupt/scheduler headroom. See
[finding 83](../findings/83-sample-retirement-design.md).

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


The raw containing-word fetch is now reusable as the eight-instruction
`DK_SAMPLE_PCM16_FETCH_WORDS` macro in `sdk/dsp/sample_pcm16_fetch.inc`; only the
private frame fixture is refactored to use it. The SDK documents scalar/linear,
nonwrapping full-word and caller-protection preconditions. Existing 58 proof
cases retain their metrics, plus a focused shared-index-register/final-DDR-word
edge case. Old PCM kernels, public resource/output ABI and production exclusion
policy are unchanged. See [the SDK contract](../../sdk/dsp/README.md) and
[finding 99](../findings/99-stereo-frame-fetch-exclusion.md).
