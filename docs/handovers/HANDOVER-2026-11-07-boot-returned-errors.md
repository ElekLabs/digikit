# Handover 2026-11-07: returned storage error proof, phase 1

Continues `work/loadable-bootscreens` at `1688d68f`. Private opt-in adapter
`core/storage-error-proof`, six pure-data boot-io configurations and production
decider host cases prepare returned-error experiments. Phase gating uses the
existing `BOOT_GUARD_ATTEMPTS`; ordinary pre-intro I/O passes without counters.
The base requires a mode export and intentionally fails linking alone.

Device-only owner `core/storage-error-tests` scopes acceptance prerequisites to
the explicit configurations/provider and c-demo, with pass and one-copy cases
on both current images. Each case creates one startEmpty initial baseline,
reuses raw CardSpec.recs/encodeRecord, and retains bounded fault JSON, startup,
lifetime and intro PNG evidence. A valid old empty A sequence 1 and newer B
sequence 10 selecting c-demo expose possible loss of the newer selection after
one returned B read error. The expected behavior suppresses early scene/provider
work and preserves c-demo for late loading.

Static validation passed: 6 host modes, regression typecheck, 6 scoped build
tasks, all six pure-data configurations, generated-source and layer checks.
Pass and one-copy images offline-link on both current firmware versions; the
unconfigured base fails with unresolved BOOT_IO_MODE as required. Adapter code
is 424 bytes plus 32 bytes BSS; each configuration is 4 bytes read-only data.
Only the six intended runtime/config/tail externs remain, with no libc helper.
Plan reports show 8-byte displaced prologues and continuation at entry+8 for
all four hook windows. Target inputs explicitly cover the config/provider
closure, c-demo and source asset; device result caching is disabled.
The initial runtime object refresh changed only its source hash, with no code
or data changes before the reproducer.

## Accepted before-fix device evidence

DN2 pass control passed 1/3 skipped in 50.33 s, one 45.977 s baseline. BootInfo
seq 10/copy B/count 1/loaded 1/counter 0; stored selection advanced to seq 11,
counter 1 with c-demo. All 2,166 owned bytes, scene hash, UI/input/DSP and
lifetime checks passed. Fault reads/writes were 30/1, rejects 0/0. Log:
`/private/tmp/bootscreen-io-dn2-control.log`; retained bounded evidence:
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-boot-io-evidence-dn2-pass-vupWYa`.

The single parent-authorized DN2 one-copy run failed as expected: 1 failed/3
skipped in 48.08 s, one 45.500 s baseline. Exactly one 512-byte B read at sector
48,340,991 was rejected. BootInfo used older A seq 1 with count/loaded 0; early
provider/scene still ran (4 callbacks), and B seq 10 selecting c-demo was
replaced by seq 2 empty with counter 1. The lifetime report passed with zero
write-watch/flush hits and no invalid writes. The failure evidence was saved
before assertions. No retry. Log:
`/private/tmp/bootscreen-io-dn2-one-copy-before.log`; retained bounded evidence:
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-boot-io-evidence-dn2-one-copy-NvhIJ9`.

## Bounded early-selection fix, host/static phase

`boot.rs` now separates returned read failure from a successfully read invalid
record. One private closure helper reads each copy once and retains any I/O
uncertainty. Early prepare resets PREP_STATE to untouched and returns before
card/settings/crash/counter side effects after either read fails; late prepare
therefore rereads fresh copies. Existing non-early current() behavior and tie
ordering remain unchanged. Both failed reads already deferred before this fix.
Late persistent read-error policy remains unchanged and open.

All 63 runtime host tests pass, including five new cases covering either/both
read errors, invalid CRC, ordering/ties, and a later successful seq 10 c-demo
lookup and counter arm. An initial host assertion had a path-length typo; it
was corrected before the passing run. Scoped runtime build passed 4 tasks.
Both current images offline-link; generated-source, layer and diff checks pass.
Runtime text grows 718 bytes (156,880 to 157,598), data stays 74,146 and BSS
670,015; no new undefined symbol. Placement grows 720 bytes on each image: DT2
CF expansion 959,148/1,396,736, DN2 965,484/25,976,832. DSP/cave use is unchanged.
Logs/summary: `/private/tmp/boot-io-runtime-links.log` and
`/private/tmp/boot-io-runtime-summary.json`. At that checkpoint no after-fix emulator run had been
authorized. No hardware, default image/SDK/test-fixture change, daemon build,
dependency change, commit, push, PR or merge. Physical blocked I/O remains open.

## Shared lookup simplification, before device verification

The private `current()` now returns the selection helper's triple directly,
with one production closure instantiation. Early prepare uses it; four existing
late callers ignore the uncertainty result, preserving their policy. Pure-helper
tests are unchanged. All 63 host units and the scoped 4-task object build pass.
Runtime code is now 157,072 bytes: +192 versus before-fix, 526 bytes below the
initial fix. Data/BSS remain 74,146/670,015, with no new undefined symbols. Both
current-image offline links pass. CF expansion placement is +192 bytes versus
before-fix, reduced from +720: DT2 958,620/1,396,736; DN2
964,956/25,976,832. DSP/cave placement is unchanged. Diff check passes.
Summary: `/private/tmp/boot-io-runtime-sharing-summary.json`; offline links:
`/private/tmp/boot-io-runtime-sharing-links.log`. At that host checkpoint no after-fix emulator run or
commit had occurred; the authorized results follow.

## Updated-runtime regression verification

The unchanged DN2 one-copy regression passed once: 1 passed/3 skipped in
51.05 s, one 47.485 s baseline. Exactly one B 512-byte read returned error
(reads/writes 25/1, rejects 1/0). Early provider, publication and scene counters
were all zero, guard attempts/requests 1 with entered/returned 0; fallback intro
matched the stock last-drawing hash. Late boot reread B seq 10/copy B and loaded
one c-demo with result 0. A became seq 11/counter 1 with the c-demo path; original
B seq 10/counter 0 with that path stayed intact. The unchanged test's UI/input/
DSP and lifetime checks passed; no write-watch/flush hits or invalid writes.
Log: `/private/tmp/bootscreen-io-dn2-one-copy-after.log`; retained evidence:
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-boot-io-evidence-dn2-one-copy-mTC5si`.

The separately authorized updated DT2 pass control also passed once: 1 passed/3
skipped in 26.63 s, one 20.333 s baseline. It loaded c-demo from B seq 10/copy B,
armed A seq 11/counter 1, preserved all 2,166 owned bytes, played 4 callbacks/3
records with the exact scene hash, and rejected no I/O (reads/writes 30/1). The
DT2 main-screen hash, UI/input/DSP and lifetime checks passed with no write-watch/
flush hits or invalid writes. Log: `/private/tmp/bootscreen-io-dt2-control-after.log`;
retained evidence:
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-boot-io-evidence-dt2-pass-srECRa`.

Both outcomes are indexed in `/private/tmp/boot-io-after-summary.json`. No retry
or case/shim/configuration change. Updated DN2 pass and DT2 one-copy remain
unrun; no other modes were executed. No hardware/native build/commit.

## Expanded recovery table, prepared only

The existing table now registers eight scenarios on each device (16 total):
pass, one-copy, both-copies, payload, write-error, probe, held-write-error and
safe-once-write-error. The final two reuse the existing write-error config;
metadata includes all six existing configs explicitly. No runtime/shim/config
changes or rebuilds. Cases use the same old empty A/new c-demo B seed, with the
safe-once B flag where specified. Failed-read recovery suppresses early work
except payload, which enters once and discards its failed-read allocation.
Write-error cases retain B/counter 0 and assert actual storeWrites is empty;
held RECORD bypasses requests and safe-once retains its flag after failed clear.
Probe uses shared cardInfo to assert ours/readErrors 1. All evidence is saved
before assertions. Existing UI/input/DSP/lifetime checks and budgets remain.

Prepared table passed regression typecheck and owned formatting; generation,
layers and diff checks pass. At that preparation checkpoint no expanded case had run. Parent selected 11 new
cases: DN2 both-copies/payload/write-error/probe/held-write-error/safe-once-write-error
and DT2 one-copy/payload/write-error/held-write-error/safe-once-write-error.
Already passed updated DN2 one-copy and DT2 pass must not be rerun in that set.
This remains a returned-status fixture, distinct from hardware write protection
or DMA stalls. Blocked I/O and persistent late-read policy remain open.

## Expanded recovery results

Both frozen groups passed without edits or retries. Group A: 5 passed/11 skipped
in 219.31 s (DT2 one-copy/write-error; DN2 both-copies/payload/probe). Group B:
6 passed/10 skipped in 252.90 s (DT2 payload/held-write-error/safe-once-write-error;
DN2 write-error/held-write-error/safe-once-write-error). Exactly 11 selected
cases and 11 cold baselines. Logs:
`/private/tmp/bootscreen-io-expanded-a.log` and
`/private/tmp/bootscreen-io-expanded-b.log`. Bounded retained evidence directories,
individual baseline durations and actual boot/provider/guard/scene/fault/card/
selection/write/lifetime observations are indexed in
`/private/tmp/boot-io-expanded-summary.json`.

DT2 one-copy rejected one B read and DN2 both-copies rejected both reads; both
loaded c-demo from the late seq 10 B selection and armed A seq 11/counter 1,
without early asset work. Both payload cases rejected one 2,560-byte read,
allocated/discarded once, published no pointer, reported scene status 2, loaded
c-demo normally and retained stock fallback drawing. Both persistent write-error
cases rejected exactly two counter-write attempts, loaded c-demo with counter
snapshot 0 and kept A seq 1 empty/B seq 10 c-demo counter 0; actual store-write
sector lists were empty. DN2 probe rejected one header read, retained card state
ours/readErrors 1, suppressed early work, and loaded c-demo with the late
counter retry succeeding. Held RECORD on both devices produced key safe reason
2, count 1/loaded 0, no early requests or write attempts. Safe-once on both
produced flag safe reason 3, count 1/loaded 0, one failed flag-clear write and
kept B's flag 1; actual store-write lists were empty.

All 11 existing UI/input/DSP and lifetime checks passed. Lifetime reports had
pass true, zero write-watch/flush hits, and no outside-store/invalid-store/invalid-
low writes. Each case saved its fault evidence before assertions. Across the
updated runtime, 13 of 16 registered cases now pass (the earlier DN2 one-copy
and DT2 pass plus these 11). Updated DN2 pass, DT2 both-copies and DT2 probe
remain unrun. Including the original normal control and single before-fix red
reproducer, the returned-error work has 15 attempted baselines: 14 successful
tests plus the expected failing before-fix test; no retries.

No additional code/shim/config/case edit or build during these runs. No full
gate, commit, hardware, push, PR or merge. This verifies local emulator
caller-return recovery, not actual hardware write protection, DMA failure or
escape from a blocked read. Persistent late-read-error policy remains open.

## Finalization boundary

The test-only entry hooks and configurations remain opt-in and outside every
default image. Final runtime footprint is +192 bytes code/placement per image,
with data/BSS unchanged. Evidence totals are 13 selected after-fix passes, one
before-fix control success and one expected before-fix failure: 15 attempted
baselines without retries. Blocked controller I/O, physical write protection,
initial RECORD scan and display timing, and persistent late selection-read-error
policy remain open. The changed-test gate passed as recorded below; the full merge gate is
not part of this local finalization.

## Local commit and final changed gate

Implementation commit: `2ad071ef4a976cdc430a64c033db8f9f70cd072a`
(`fix: defer early boot after uncertain selection reads`). The authorized
`DIGI_GATE_BASE=1688d68f mise run test:changed` passed once: 8/8 tasks in
43.8 s, zero cache hits. Selected targets were storage-error-proof:test-host,
workspace fmt-check/lint/rust-fmt, tools:python-checks, desktop test-host/
typecheck and nx-tools:test-host. No failed owner or rerun. Log:
`/private/tmp/bootscreen-io-final-changed-gate.log`. Final generated-source,
layer and diff checks passed. Existing local artifacts were reused; no native
build, dependency change, additional emulator run or full merge gate.
The earlier card-DKBS handover links this continuation at its top.

No push, PR, merge or hardware action. Three registered updated-image cases
remain unrun as listed above. Physical/controller and late-error limitations
remain open.
