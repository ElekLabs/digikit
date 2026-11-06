# Handover 2026-11-07: fresh UART gate and I/O deferral

Current user-authorized checkpoint on `work/loadable-bootscreens`, based on
`70c0875d`, in `/Users/em/.codex/worktrees/28d7/digitakt2`. All 25 focused current
cases passed on DN2 1.12 / DT2 1.17. No assets/player, provider/SDK phase,
hardware, default patch membership or input/CPU/IRQ driver semantics change.
The full merge gate was not run; no push, PR or merge.

## Current behavior

The existing RECORD safe-key check was already on main. The new opt-in adapter
observes only UART parsed-key dispatch at hash `8a1e573cfa9d7893e83dc64c6f6caeb3`,
offset `0xa8`, length 6 (DT2 `0x4011e6f0`, DN2 `0x40120064`). Independent audit
checked the callback bytes and JSR/BRA window. Aligned sp+0/sp+4 arguments are
generated normally; restored-SP tails are jumped to, preserving stock cleanup.

Channel-2 receipt and any RECORD-down are packed together; intro closes and
captures an immutable decision before skip/null checks. Missing receipt or
sticky RECORD-down denies the runtime request. Absent/skip paths only perform
RAM bookkeeping. STOP down/up supplies real fresh channel-2 packets without
RECORD-down; initial release alone would be suppressed by Emulator.button.
All four held/sticky fixtures observed late reason **NONE**, decision 3 and
zero runtime requests: optional early-work bypass is proved, global late safe
mode and hardware RECORD behavior are not.
Functional REC access to global safe mode remains an open issue: it did not
activate in these cold fixtures on either device, and was not fixed here.

The bounded `firmware.load.initial_buttons` seam validates at most 32 ordered
logical transitions and requires an explicit cold, patched coupled path,
including when the supplied list is empty. Omission preserves old paths.
PatchInstall replays the immutable list across boot-time resets through existing
Emulator.button. Nonempty input changes the real pair key; empty keeps the old
key. No input/IRQ semantics or snapshot format changes.

A pure volatile KEEP word-0 check now denies early preparation before selection
I/O when crash magic is pending, even with corrupt payload. Normal late
preparation retains settings-before-crash validation/persistence. Invalid
selection, safe-once or starting counter >=2 defers after selection reads but
before card classification/settings/crash side effects. Clean early preparation
skips unrelated settings loading; main ensures it once before module work.
Counter fallback uses preparation completeness on main entry, preventing a
deferred request from retrying a failed normal late write twice. No new core
persistent state or KEEP/settings format change.

## Actual validation and evidence

- Runtime host units: 58 passed. Focused fwgen: 19; input parser/profile: 2;
  production pair identity: 2; zero-instruction packet queue: 1. Types passed.
- Existing daemon prerequisite passed in 1 min 18 s. Final runtime/adapter build:
  five tasks passed in 5.9 s. Generated-module and layer checks passed.
- Separate DN2 pending-fault zero-counter: 1 passed, 101.53 s total / 88.789 s
  baseline. Raw `/private/tmp/bootscreen-dn2-fault-deferral-zero.log`.
- Remaining exact three-file matrix: 24 passed / 1 intentionally skipped (the
  preceding DN2 case), 1015.06 s, exit 0, two workers. Raw
  `/private/tmp/bootscreen-focused-matrix.log`. Thus all 25 current cases covered:
  13 early-boot, 8 panel and 4 exception cases. No retry or budget change.
- Panel evidence index: `/private/tmp/bootscreen-matrix-panel-summary.json`.
  Fresh cases show decision 1, one request/callback, settings loaded 0/0/1;
  missing cases show zero requests and selected module loaded late.
- Exception evidence index: `/private/tmp/bootscreen-matrix-exception-summary.json`.
  Each retained directory contains only the named fault/startup/lifetime JSON.
  Exact vector 4 / PC DN2 `0x46804824`, DT2 `0x47f73f64`, valid retained CRC and
  persisted fault passed. Counter-0 + selected module: one genuine fault, pending
  record deferral, late counter-1 normal boot/module loaded. Seeded counter-1:
  one genuine fault, late safe-counter 2. Seeded history is not a second fault.
  Narrow settings/selection writes, settings 0/0/1, DSP growth, settled UI,
  SETTINGS/NO, IRQ/input and lifetime checks passed. The separate reset family
  still proves two real resets/safe third boot, using the existing 180 s window.

Measured against `70c0875d`: adapter text 130 ->296 B, BSS 172 ->196 B,
rodata/data zero. Actual placement 492 B object +16 B intro tail +20 B UART tail
=528 B, versus 320 B (+208 B). Runtime text 156900 ->157072 B (+172 B),
placement 901280 ->901456 B (+176 B); data/BSS unchanged. Metadata:
`/private/tmp/bootscreen-deferral-section-costs.json` and
`/private/tmp/bootscreen-deferral-placement.json`. No stack, compressed-flash or
performance claim. Earlier first-packet-only 480 B adapter / unchanged-runtime
measurements were intermediate, before deferral, and are superseded here.

Final `mise run test:changed`: 13/13 tasks passed in 48.5 s (0 cache hits).
`mise run gen:modules -- --check` and final diff checks passed. This cheap
preflight does not replace the unrun full merge gate.

## Remaining boundaries

Working-card emulator proof is not blocked-I/O or hardware recovery. Selection
reads, card classification/possible 1 MiB blank scan and counter persistence can
still block; normal late settings/fault I/O can block too. A caller timeout
cannot cancel SDHC/DMA. Cached selection refresh after transient early read
errors, read-only recovery, independent physical bypass and initial-scan timing
remain open. Packet receipt is not physical scan completion. See
[finding 115](../findings/115-loadable-bootscreens.md) for the historical ledger
and NXP register evidence; no safe bounded cancellation implementation is approved.

## Future follow-up only

The user excludes snapshot-speedup implementation this session. Existing
withReusablePatchedSession/reloadRuntimeBaseline already restores compatible
CF+DSP ready pairs, advances epoch and clears scheduled input/UI state. A future
ordinary-test path could prefer a validated clean checkpoint in a kept daemon,
after proving immutable card identity and fresh overlay/input/host state. The
pair cache uses backing path/length/mtime plus overlay, not full backing-content
hash. Early-boot proof keeps true cold coverage, or a future pre-phase checkpoint
plus explicit cold coverage.

Current logs also show older DT2 probe tests doing two roughly 16 s reloads:
default fixture setup then h.boot(custom card). Lazy/test-specific initial card
setup might avoid the redundant first boot; this is an observed optimization
candidate requiring validation, not an established safe refactor.
