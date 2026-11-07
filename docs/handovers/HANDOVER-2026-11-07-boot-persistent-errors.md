# Handover 2026-11-07: persistent selection read errors

Continues the returned-error work at `80c1181f` in lane `bootscreen-safety`.
This checkpoint implements the parent-approved late read-error policy and
prepares six device cases. The later parent results below supersede that preparation checkpoint.

Every selection lookup consumer now honors the uncertainty result. The getter
returns E_READ without touching its output; both setters return E_READ before
writes. Early preparation still defers. Late preparation selects additive
SAFE_READ (6) before other safe reasons, retains any readable record's diagnostic
sequence/count, loads nothing and reports E_SAFE_MODE. Stable UI processing
never rereads or updates selection for a SAFE_READ boot, and any other stable
lookup that fails skips its update without per-frame retries.

The menu displays READ FAILED and selection errors, omits stale ABOUT names,
and refuses edits/APPLY after a failed selection refresh until a successful
refresh rebuilds ticks. Existing UI host tests compile the actual menu C source
and exercise failure, recovery, toggle refusal and save refusal. Runtime-log
labels now include panic (5) and read (6).

Private modes 6/7 persistently reject B or both selection reads, gated by
BOOT_GUARD_ATTEMPTS. Mode 8 resets at the 2,560-byte provider payload read and
never returns, using the existing SOFTRST proof pattern. All three configurations
remain explicit opt-ins. Existing modes 0–5 retain their behavior.

Prepared cases `persistent-one`, `persistent-both` and `payload-reset` run on
both current devices. Persistent cases assert safe reason 6, zero early asset/
scene/module work, original record bytes, no writes through stable UI, and
E_READ from SELECT and SELECT_ONE for both copies. There is no existing direct
getter-call test seam, so untouched-output remains source-reviewed here.
Reset cases assert counter-safe recovery, no third-boot provider work and the
bounded reset trace marker array `[1, 2]`; they model reset during a nonreturning
read boundary, not controller timeout, real DMA stalls or automatic recovery.

Scoped checks passed: runtime 63 host units; dkui-host 86; dk-session 35;
storage-error-proof 8; regression-host 175 passed/2 skipped; session/regression
typechecks; generated-source, layer, manifest catalog, owned TS lint/format,
UI host Rust format and diff checks. The first runtime compile preceded ABI
generation and failed for the then-missing SAFE_READ; the two new menu tests
initially had driver navigation mistakes, corrected before their passing run.
A standalone dkrt format check reports pre-existing differences throughout the
crate; no unrelated formatting was applied. The scoped object build passed
8 tasks. All three new configs offline-link on DT2 1.17 and DN2 1.12.

Object footprint versus lane base: runtime text 157,176 (+104), data including
rodata 74,146 (unchanged), BSS 670,015 (unchanged). Menu text +88, rodata +37,
BSS +4. Shim text 458 (+34), BSS 32 unchanged. Configs each contribute 4 bytes
read-only data. Existing boot-guard/UI objects refresh source hashes only due
to the generated runtime header. For persistent-one, expansion placement is
958,776/1,396,736 on DT2 and 965,112/25,976,832 on DN2; cave/DSP use unchanged.

Next: parent runs exactly the six prepared cases, then the changed gate. One
file-filtered invocation selects them without the older matrix:

```sh
mise exec -- pnpm --filter @digi/regression test:runtime \
  ../../modules/core/adapters/storage-error-proof/tests/device/error.device.test.ts \
  -t 'boot returned I/O (persistent-one|persistent-both|payload-reset)'
```

No device run, hardware action, dependency change, default opt-in, push or merge
occurred in this lane. The full merge gate remains for merge preparation.


## Parent integration and completed device checks

Implementation cherry-picked as `aafa618f` on `work/loadable-bootscreens` in
`/Users/em/.codex/worktrees/28d7/digitakt2`; lane commit `2cf94d96`. Earlier
closeout commit `399182c6` completes the original card-provider 16/16 and
returned-error 16/16 tables on `80c1181f` across the recorded runs.

All six new device cases and two updated-runtime controls passed on `aafa618f`.
Two frozen groups, max two Vitest processes/maxWorkers 1, no retries or edits:
A 3 passed/19 skipped in 214.04 s; B 5 passed/17 skipped in 221.87 s. Eight
baseline requests and four guest resets; original default budgets except the
existing three-boot 180 s pattern for reset cases. No daemon/native rebuild.

Both payload-reset cases retained exact reset markers `[1, 2]`, safe reason 1,
count 1/loaded 0, boot snapshot seq 12/counter 2. The third boot performed zero
provider/scene work; stable UI left A seq 13/counter 0 and B seq 12/counter 2,
both preserving c-demo. This is reset recovery at the nonreturning provider-read
boundary, not automatic escape from permanent DMA/controller blockage.

All four persistent cases reached stable UI with safe reason 6, count/loaded 0,
zero provider work and no store writes. Original A seq 1 empty and B seq 10
c-demo were preserved. Startup rejected reads were 3 for B-only, 5 for both;
after SELECT and SELECT_ONE for A/B the asserted totals were 6/11. Each setter
returned E_READ (-3). Tests compared all 512 bytes of both records against the
seed before and after API calls and checked no writes through subsequent UI
activity. These assertions passed; post-API raw sector copies and return values
were not separately retained by the frozen fixture. Getter output preservation
is source-reviewed, not a direct device-call measurement.

DN2 pass and DT2 one-copy controls loaded c-demo and armed counter 1. All eight
cases passed existing UI/input/DSP checks and strict lifetime reports, with zero
write-watch/flush hits and no invalid writes. Logs:
`/private/tmp/bootscreen-safety-device-a.log` and
`/private/tmp/bootscreen-safety-device-b.log`; bounded evidence index (including
retained directories, actual reset markers and per-case baseline durations):
`/private/tmp/bootscreen-safety-device-summary.json`.

Persistent late-selection policy is now closed for returned errors. Hardware
initial REC timing, display presentation, physical write protection and escape
from permanently blocked I/O remain unproved. The initial static logo and a
public loadable service/module contract remain outside this private prototype.
No hardware/default-membership change. A full merge gate remains required
before merging; the final branch preflight and draft PR are recorded below.


## Final branch preflight

`DIGI_GATE_BASE=d140042f mise run test:changed` passed once across the whole
feature branch: `NX 19 tasks: 19 succeeded, 4 cached`, 52.8 s. Log:
`/private/tmp/bootscreen-final-branch-preflight.log`.

This is the cheap host/static preflight, not `mise run test`. Full affected merge
verification and physical-device observations remain outstanding; the requested
PR will be a draft. No default opt-in or hardware action was performed.
