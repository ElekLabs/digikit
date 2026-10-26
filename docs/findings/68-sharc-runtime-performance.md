# SHARC runtime performance diagnosis, 2026-10-26

**[D] [C]** DT2 OS 1.17, native host, base `d0a4d76d`. The initial diagnosis
changed no emulator source. The October 26 follow-up tested a small fast-tier
load guard change. October 27 default-activation experiments found stock
regressions; those experiments and the shared load change were withdrawn
from the active source (see the final section). No generator or firmware
patches changed.
This updates the October 23 external performance plan and the historical
patched-image section of [finding 18](18-dsp-performance.md). The narrower
invalidation in `60aa71ba` already exists; do not repeat that implementation.

## Decision

Investigate compiled execution of the patched parameter-conversion region
`0x1c2607..0x1c28b5` first. Current runtime patches still cause a large amount
of stock code to execute through the interpreter. Stock firmware itself has a
different distribution: most sampled DSP work is already compiled.

No default activation is accepted. The follow-up demonstrates a useful load
guard improvement with an explicitly enabled region, but stock regressions
prevent accepting it for normal workbench launches. The recoverable
**host time** of the whole region remains unmeasured; its instruction share
is not a speedup ceiling. Pending-transfer dispatch remains lower priority.

Any accepted follow-up must enable its improvement in normal workbench
launches, without experimental environment variables. Opt-in measurements
alone do not satisfy that requirement.

## Inputs and build cost

Artifacts are local under `out/native/runs/20261026-sharc-diagnosis/`:
`manifest.json`, `stock-clean.json`, `stock-profile.json`, `stock-host.trace`,
`stock-host.xml`, `host-attribution.json`, `runtime-probe.json`,
`runtime-diagnostic.json`, `runtime-fast-probe.json`, their logs and diagnostic
scripts. Firmware, snapshots, card and binary hashes for the coupled stock
probe are in the manifest. Do not commit these derived artifacts.

The coupled binary is the immutable copy `coupled-run-1a3ab96cb008`, copied
from the nominal plain target used by the October 26 verification run. There
is no immutable original build-flags manifest, so non-PGO provenance is not
fully established. The separate daemon comparison uses the same immutable
`digi-emud-355279ba4e0b` for both configurations, from the PGO daemon target.
Do not combine those binaries' timings as an A/B.

`native_pgo check --device dt2` reports stale sources, workspace, generated
core and workload. Refresh applicability/training before a tuned baseline or
optimization claim. Its input preparation unexpectedly ran a cached +Drive
build check (0.20 seconds, no compilation reported). There were no SHARC Rust
builds or core regenerations in this investigation.

The stock SYX hash is `26c22f6652625ac2cfd47f7ee970d388ed8b6427dae3563c0a6a2f2d334350d5`,
matching DT2 1.17 and the ready-state provenance. The generated core reports
`dt2-1.16`: both versions' section-7 blobs hash to
`0f514a12a2255f5c081e292c47f1f29462003177658da4bbae0a22fd737fffa2`, matching
the generated image hash. The label does not mean these runs loaded OS 1.16.
An independent evidence audit checked those hashes and the stock results.

Keep the registered generated-core path and existing target directory when
iterating. `tools/emulator/native_target.py` keys Cargo reuse by generated
path and variant; a new worktree, generated path or RUSTFLAGS variant can pay
for another large generated-crate compilation. An engine-only change can
reuse that crate. Keep experiments out of the launcher's reserved `pgo-use`.

## Stock observations

Two three-audio-second scenarios used the same one-sample card and ready
state: dismiss the startup modal with no note, and the existing
`dt2-sample-trig`. These short observations include startup after restoration;
they are not a steady-state or representative workload baseline.

| Scenario | DSP thread CPU / audio second | Interpreted / non-skipped instructions | PCM |
| --- | ---: | ---: | --- |
| No note | 1.07909 | 0.62072% | Three seconds of zeros |
| Sample trigger | 1.08257 | 0.62279% | Finite, nonzero audio |

Both passed runner health gates with no missing blocks. Neither supplied an
expected PCM pin. A separate execution-profile run reproduced both full PCM
hashes and reported zero code mismatches. Its timing was perturbed and is not
a performance comparison.

A separate 12-audio-second no-note recording used CPU Profiler. In the
trace-relative `[2,10)` window, 28,403 running DSP-thread samples partitioned
by stack ancestry as follows (not exact CPU-time shares or removable costs):

| Stack ancestry, in classification precedence | Samples | Share |
| --- | ---: | ---: |
| `exec_insn` | 2,354 | 8.29% |
| Generated compiled block, without `exec_insn` | 22,850 | 80.45% |
| Other `sharc_native::Engine` | 2,928 | 10.31% |
| Other or unresolved | 271 | 0.95% |

The XML analysis resolves IDs/references and nested tagged backtraces. An
independent auditor reproduced the counts. The largest compiled leaf samples
were `r_1C6EA9` and `r_1CB407`; do not infer their full function costs from leaf
samples. The historical generic-browser 5.8%-instructions/72%-time result
does not describe this native stock configuration.

One stock entry, `0x1c06fe`, made 463,500 zero-progress compiled calls in the
three-second no-note probe. Every recorded bail had a pending delayed
transfer. The generated entry immediately returns when `s.pending` is set.
Checking earlier could avoid the attempt while preserving interpretation,
but would not remove interpreted instructions. Its host-time ceiling is
unknown; do not relax the delayed-transfer safety condition.

## Current runtime observations

The disposable `runtime-probe.ts` uses the existing `Session` helpers and
pinned daemon. Stock starts from its ready snapshot; runtime uses the current
`emulator` patch set and a genuine patched cold boot. Both run freely, settle
for 12 wall seconds, then measure status-counter deltas over at least three
audio seconds. Each owns its card overlay. No module is uploaded by this probe.

| Configuration | Audio window | DSP CPU / audio second | Interpreter instructions / audio second | Appended instructions / audio second |
| --- | ---: | ---: | ---: | ---: |
| Stock | 3.2 s | 0.94084 | 1.948M | 0 |
| Runtime | 3.0 s | 2.33669 | 23.239M | 4.715M |

The runtime has 249,750 code-mismatch rejections per audio second. Both
windows have zero additional code verifications and zero verification time:
the old repeated-hashing bug is not the observed cause. Boot and selected
firmware identities are saved in `runtime-probe.json`.

This is one exploratory configuration comparison, not an alternating paired
optimization benchmark. It has no PCM/state equivalence gate; stock and
runtime boot paths differ. It establishes a target for attribution, not an
approved speedup or real-time claim.

The diagnostic runtime pass logs 10 differing decoded PCs:
`1c207a, 1c207b, 1c2731, 1c2732, 1c2733, 1c4ecf, 1c75c0, 1c75c1, 1c75c2, 1c80f2`.
No range is barred whole; 44 entries are barred by the narrower verifier.
The final engine-lifetime histogram contains 182,999,253 interpreted
instructions. Of these, 105,457,536 (57.63%) are in
`0x1c2607..0x1c28b5`, within DT2 1.17's parameter converter `FUN_1c24e9`.
The histogram includes settling and shutdown work; it is not the measured
three-second window and has no host-time attribution.

There is also conservative over-barring from inferred block extents: the
previous entry `0x1c78cc` is treated as extending to the next entry plus six
words, which includes changed `0x1c80f2` across a large gap. Static review
found no such instruction in that body, but complete dependency coverage is
still required before narrowing it. The sampled instruction counts in this
affected group are much smaller than in the genuinely patched converter;
do not prioritize it merely because it appears easier to change.

## Bounded fast-tier probe and next experiment

One diagnostic run set only `SHARC_FAST_REGIONS=0x1c2607`, preserving the
existing models and guards. It built a kernel and executed 3.696M fast-tier
instructions per audio second, at 48,000 calls per audio second: **77 useful
instructions per call**. The remaining work still fell back. Compilation
reported a stop at `0x1c2844`, unsupported form `14d`; that static stop is not
proof of the dominant dynamic exit.

The instrumented window observed 2.35579 DSP CPU/audio second. It is not
comparable to the clean runtime number as a speedup measurement: logging,
different ordering and no paired correctness gate remain. Do not ship the
region list on this evidence. This probe required no Rust rebuild.

The initial next step was to obtain dynamic exits/useful-work attribution for the patched
converter and its continuation, plus a bounded runtime host profile. Choose
between a narrowly supported run-time compilation improvement and splitting
generated dependencies/entries around the patch. State the measured host-time
ceiling before implementation; avoid broad regeneration until that decision.
Then use current PGO policy consistently, exact PCM and state boundaries,
and one alternating `coupled_ab.py` comparison. Its current acceptance gaps
(failed exits/gates/hashes are recorded but not rejected before ratios) must
be addressed before relying on automated speed claims. Include audible
transitions and the dynamically loaded/selected PLUCK case before acceptance.

## Same-page load experiment, October 26

**[D]** Opportunity ceiling first: the restored runtime's separate eight-audio-
second host profile has 20,402 interpreter-ancestry samples out of 42,818
running DSP-thread samples (47.65%) in trace-relative `[2,14)`. This is the
broad interpreter opportunity, not an exact removable-time bound for this
load or region. Compiled ancestry accounts for 35.46%, other engine ancestry
16.39%, and unresolved samples 0.50%.

The actual dynamic exit was `0x1c26b7`, at the flat-memory bitmap guard,
before the static `14d` stop. Register captures show a four-byte SISD load
at byte address `0x2559de`: offset 30 within a 32-bit presence-bitmap word.
Subsequent addresses advance by `0x60`, alternating offsets 30 and 62 within
a 64-bit bitmap word. The old guard rejected these even when all four bytes
were present on the same page.

The public [SHARC+ Core Programming Reference](../sharc/SOURCES.md), printed
page 7–6 (PDF page 224, local extraction `out/refs/sc58x-2158x-prm/pages/p0224.txt`),
allows all alignments for normal-word SISD accesses in byte space. It separately
requires normal-word alignment for SIMD. The manual establishes that this
captured load is legal; the bitmap restriction is an emulator implementation
detail, not a SHARC alignment requirement.

The experiment checks both affected 32-bit presence words for multibyte
loads and requires the complete access to remain inside one page. Stores,
page absence, MMIO, aliases and code-watch handling retain their guards.
No instruction semantics, generated cores or default region lists changed.
Focused tests cover 1/2/4-byte accesses, both bitmap boundaries, each missing
byte, page ends/crossings, absent/MMIO pages, aliases and store guards through
both the interpreter backend and native Cranelift. Both tests and
`mise run test:changed` passed; an independent source review found no issue.

Artifacts are under `out/native/runs/20261026-sharc-flat-crossing/`:
`runtime-inputs.json`, `runtime-host-attribution.json`, `crossing-registers.csv`,
`build.json`, `before-correctness.json`, `after-correctness.json`, `paired.json`
and `paired-validated.json`. The replay restores a genuine current patched
runtime boot, including its matching ColdFire and DSP snapshots and card.
It contains no dynamically uploaded module. Each scenario runs three audio
seconds, including restoration startup rather than only steady state.

One alternating `coupled_ab.py` experiment used three pairs per scenario,
the same frozen inputs and `SHARC_FAST_REGIONS=0x1c2607` on both binaries:

| Scenario | Before median CPU/audio s | After median CPU/audio s | Median paired reduction |
| --- | ---: | ---: | ---: |
| No note | 2.304 | 1.734 | 24.76% |
| Sample trigger | 2.353 | 1.745 | 25.59% |

Paired ratios ranged 0.752–0.761 and 0.742–0.746 respectively. Every run had
exit status zero, health gates true, and the same full 64-character PCM hash
within its scenario. These were independently asserted after the tool returned,
before interpreting its ratios; the tool's acceptance behavior itself is
unchanged. Ignore its misleading total-CPU/interpreted-instruction statistic.
The separate correctness reports also confirm finite audio, three-second
duration, silence for no note, and nonzero sample-trigger output.

The trigger moves from 77 to 229 fast instructions per call at 48,000 calls
per audio second. Interpreted work falls from 58,619,406 to 36,731,406
instructions per three-second run. This explains the measured direction
without equating instruction counts with host time.
The separate `after-coverage.log` records all 143,993 successful no-note
kernel calls exiting at the static `0x1c2844` stop, rather than the old load
guard. An independent evidence audit reproduced the profile counts, PCM
equality, instruction movement and paired ratios.

The candidate `coupled-run-7545ca71a8c7` built in 16.04 seconds (Cargo 15.95),
reusing all four recorded generated/runtime rlibs unchanged in size and mtime.
Focused test rebuilds took 17–19 seconds. The current plain Cargo fingerprints
record empty Rust flags; the old frozen binary still lacks its original
immutable build manifest. This is a local non-PGO experiment, not a claim
about refreshed PGO, browser performance or stock firmware gains.

**[O]** Before enabling the region by default, establish safe runtime-specific
selection, check stock behavior and the uploaded/selected PLUCK workload,
compare architectural state at matched boundaries, and measure under a
consistent current PGO policy. Full PCM equality in two short probes is
useful evidence, not general architectural equivalence. The helper fix alone
does not activate this DT2 region in normal launches.
Before merging the shared helper, measure DN2's existing default fast regions
for overhead from the additional presence-bitmap check; correctness gates do
not establish performance neutrality there.
Static dispatch review confirms that a configured fast region runs before
an available AOT entry. Adding this root to the default table would therefore
also change healthy stock dispatch. There is no existing invalidation-only
activation hook; the eligibility decision after AOT invalidation in
`packages/emulator/sharc/src/lib.rs` is the narrow boundary to investigate.

The routed `mise run verify -- --base main` gate passed in 1,531 seconds;
logs are in `out/runs/verify/20261026-122921/`. It included 243 passing Rust
tests (three skipped), all 17 tier-comparison scenarios equal, audio pins,
stock, runtime workflows, module loads and smoke checks. This broader gate
used normal region configuration, not the experimental DT2 root override.
Its runtime workflow suite dominated elapsed time (1,244 seconds); this
should not be confused with the 16-second candidate compilation.

## Default activation rejected, October 27

**[D] [C]** The opportunity ceiling remains the broad 47.65% interpreter
sample share above, not an exact removable-time bound. Runtime gains alone
were insufficient to accept a shared-engine change as a workbench default.
The October 26 shared load change has been reverted to `d0a4d76d` in the
active source, alongside removal of the unaccepted activation experiments.
The historical experiment results above remain valid for their binaries.

The proposed policy registered DT2 1.17 root `0x1c2607` as fallback-only:
code verification had to bar its actual generated entry before it could
execute. Healthy stock AOT chains were retained. Launcher, coupled runner,
PGO workload configuration and TypeScript test inputs supplied firmware-keyed
defaults; explicit settings retained precedence. DN2 region lists were unchanged.
No generator changes or regenerated cores were needed.

Initial activation showed approximately 29% lower runtime DSP CPU, but also
stock regressions. Subsequent experiments cached policy, skipped inactive
tier work, reduced bitmap checks, specialized dispatch and finally restricted
widened loads to fallback roots. None established acceptable performance on
both stock devices. Correctness tests and source review did not predict these
performance regressions. The final V6 candidate passed 67 focused Rust tests.

One final alternating `coupled_ab.py` experiment used three pairs per
scenario, three audio seconds per run, frozen inputs and fresh plain baseline
builds from `d0a4d76d`:

| Fixture / scenario | Baseline median DSP CPU/audio s | V6 median | Median paired change |
| --- | ---: | ---: | ---: |
| Runtime DT2 idle | 2.408 | 1.781 | -25.95% |
| Runtime DT2 sample | 2.403 | 1.781 | -25.90% |
| Stock DT2 idle | 1.040 | 1.106 | +6.32% |
| Stock DT2 sample | 1.051 | 1.113 | +5.94% |
| Stock DN2 idle | 1.083 | 1.107 | +2.60% |
| Stock DN2 chord | 1.074 | 1.102 | +2.72% |

All 36 runs exited successfully, passed health gates and matched their full
baseline PCM hashes. Every stock pair was slower. Separate correctness runs
matched all six complete SHRD state files byte for byte and matched DSP
instruction boundaries: 3,000,001,500, except DN2 chord at 3,001,334,834.
An independent evidence auditor reproduced these checks and all ratios.
Raw PCM was not retained for independent rehashing; the PCM comparisons use
full hashes emitted by the runner. Three short pairs do not characterize all
workloads, and these measurements do not establish a cause for the slowdown.

The new baseline DT2 build reproduced `coupled-run-1a3ab96cb008` exactly,
resolving the earlier baseline build-provenance concern for these comparisons.
Both device builds reused cached generated crates; V6 builds took about
15 seconds each. This is not a refreshed-PGO or GUI end-to-end performance
claim. The loaded/selected PLUCK proof and workbench candidate build were not
run after the stock performance rejection. The helper must not be described
as a currently enabled workbench speedup, and real-time DSP is not achieved.

Artifacts are retained in `out/native/runs/20261027-sharc-default/`:
`baseline-build.json`, `candidate-v6-build.json`, `v6-paired-summary.json`,
`*-v6-paired.json`, `v6-correctness-summary.json`, and the original reports and
state dumps. `unaccepted-v6-performance.patch` preserves the complete proposed
performance diff against main; `rollback-manifest.json` records the source
restoration. Earlier variants are retained there for comparison, not accepted
as production alternatives.

Rollback validation passed all 55 baseline no-gen Rust library tests. Fresh
DT2 and DN2 coupled builds reproduced both main-baseline executable hashes
exactly (`rollback-validation.json`); documentation path checks passed.
No full routed gate was repeated, and the retained timing instrumentation
still requires that gate before merge.

**[O]** Keep workbench defaults unchanged. A follow-up should first isolate
and profile the stock overhead with a bounded experiment, then establish
stock neutrality, loaded/selected-module audio correctness and normal-launch
activation together. More speculative dispatch variants are not justified
by today's evidence. The separate test-suite timing result is recorded in
[finding 69](69-runtime-test-timing.md); it changes no waits or boot isolation.
