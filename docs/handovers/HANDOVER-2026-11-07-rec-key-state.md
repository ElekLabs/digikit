# Handover 2026-11-07: current-state REC safe-mode check

User-authorized bounded follow-up on `work/loadable-bootscreens`, based on
`04f5ec86`, in `/Users/em/.codex/worktrees/28d7/digitakt2`. All 18 unique affected
cases passed. The prior [fresh-panel checkpoint](HANDOVER-2026-11-07-fresh-panel.md)
observed late NONE for held/sticky input; that historical emulator limitation is
corrected here. Hardware initial-scan timing remains open. No new hook, driver
semantics, SDK/provider/assets, hardware, deadline or reset infrastructure change.
Full merge gate not run; no push, PR or merge.

## Behavior and scope

Both existing safe-key decision points now query the existing firmware key_down
binding for RECORD, replacing the queued-event scan entirely. Independent raw-byte
audit verified the binding/current-state query on both devices. Intro consuming
the old queue is not an established cause of the earlier result.

Direct acceptance uses RT without boot-guard, selected c-demo and real cold UART
input. On both devices held REC reaches KEY (safe 1, count 1, loaded 0), leaves
the selection/counter 0 unchanged, then is released after observation. Down/up
reaches NONE, loads the module once and persists counter 1. Decisions are checked
immediately after uiReached, not polled until a desired safe result appears.
DSP/UI and lifetime checks passed. Selected-module panel cases prove the same
late behavior while both held/down/up retain sticky decision 3 and zero early
runtime requests.

Absent-callback and four probe scenarios now each start on their exact initial
card in isolated startEmpty rigs. Assertions/case count are preserved; reset and
control families are unchanged. Existing fixture options remove the redundant
default baseline followed by h.boot(custom card); no shared-harness API or warm
snapshot machinery was added.

## Actual validation

- Runtime host units: 58 passed (1.3 s); regression types passed (2.1 s).
- Final scoped runtime/adapter object build: five tasks passed in 5.8 s.
  Generated-module, layer, owned TS/JSON format and diff checks passed.
- First exact DN2 direct held: 1 passed/3 skipped, exit 0, 62.95 s total /
  59.599 s baseline. One reload. Raw `/private/tmp/bootscreen-direct-rec-dn2-held.log`.
- Group A exact early-boot file, filter
  `^(DN2 1\.12|DT2 1\.17): early boot (without callback|probe)`:
  10 passed/3 unchanged reset/control skips, exit 0, 426.42 s.
  `/private/tmp/bootscreen-rec-refactored-group-a.log` has exactly ten
  session.baseline-reload events across ten distinct device/scopes: one each.
- Group B exact early-panel + late-rec files, filter
  `^(?!DN2 1\.12: direct REC held)(DN2 1\.12|DT2 1\.17): (early panel (held|record down then up)|direct REC)`:
  7 passed/5 intentional skips, exit 0, 300.99 s.
  Raw `/private/tmp/bootscreen-rec-panel-group-b.log`.
- Both groups reused `mise exec -- pnpm --filter @digi/regression test:runtime`
  with named files, maxWorkers=1/bail=1/verbose, at most two concurrent workers;
  source frozen and existing deadlines unchanged. No duplicate first held proof,
  full 25/29 sweep or unchanged exception/reset repeat.

Each REC/panel onClose retained only direct-rec or early-panel JSON,
startup-empty and lifetime-safety JSON. Paths are printed in the raw logs.
First DN2 directory:
`/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-direct-rec-evidence-dn2-oLSg4J/`.
Remaining direct directories: DT2 held `digi-direct-rec-evidence-dt2-UITomJ`,
DT2 released `digi-direct-rec-evidence-dt2-Wnk8FZ`, DN2 released
`digi-direct-rec-evidence-dn2-m50Oub`, under the same temporary parent.

The traces verify ten redundant baseline boots removed, one per refactored case.
The root audit's old first-span sum 281.769 s is only a potential setup ceiling;
host load varies, so no wall-time percentage or paired speed improvement is claimed.
Future general snapshot optimization remains separate.

Final `mise run test:changed` passed 13/13 tasks in 42.4 s (7 cached);
final generated-module and diff checks passed. Blocked SDHC/DMA, read-only recovery,
cached selection refresh after transient early reads and hardware scan timing
remain open. Current-state key correctness does not establish fresh physical
scan arrival or independent escape from blocked I/O.
