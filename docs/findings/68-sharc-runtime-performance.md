# SHARC runtime performance diagnosis, 2026-10-26

**[D] [C]** DT2 OS 1.17, native host, base `d0a4d76d`. No emulator source,
generator, defaults or firmware patches were changed during this investigation.
This updates the October 23 external performance plan and the historical
patched-image section of [finding 18](18-dsp-performance.md). The narrower
invalidation in `60aa71ba` already exists; do not repeat that implementation.

## Decision

Investigate compiled execution of the patched parameter-conversion region
`0x1c2607..0x1c28b5` first. Current runtime patches still cause a large amount
of stock code to execute through the interpreter. Stock firmware itself has a
different distribution: most sampled DSP work is already compiled.

No optimization is accepted. The recoverable **host time** of this region has
not been measured; its instruction share is not a speedup ceiling. The small
pending-transfer dispatch optimization below is lower priority.

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

**[O]** Next obtain dynamic exits/useful-work attribution for the patched
converter and its continuation, plus a bounded runtime host profile. Choose
between a narrowly supported run-time compilation improvement and splitting
generated dependencies/entries around the patch. State the measured host-time
ceiling before implementation; avoid broad regeneration until that decision.
Then use current PGO policy consistently, exact PCM and state boundaries,
and one alternating `coupled_ab.py` comparison. Its current acceptance gaps
(failed exits/gates/hashes are recorded but not rejected before ratios) must
be addressed before relying on automated speed claims. Include audible
transitions and the dynamically loaded/selected PLUCK case before acceptance.
