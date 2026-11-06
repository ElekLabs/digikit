# Handover 2026-11-07: opt-in card DKBS provider

Work on `work/loadable-bootscreens`, based on `38cb06a8`. The static/host phase and first
DN2 valid-card proof are accepted by the parent. No UI edit, commit, hardware,
push, PR or merge has happened in this seam. The provider is opt-in only.

## Implemented contract

`core/boot-card-proof` requires the scene proof and reads only
`bootscreens/active.dkbs` through runtime stat and the existing API table. It
allocates exact stat size only for 16..65,536 bytes, copies with SHA-verified
store_read and checks positive return <= capacity. Failed reads and invalid DKBS
free before return/publication. The optional discard bridge clears rejected
decoder pointers. Valid bytes are immutable for the entire boot/session; no free
from the intro task. Upload replacement/delete applies next boot, no SELECT.
Private aligned counters expose attempts/stat/read/owned bytes/pointer/discards.
The provider has no staging alias, large buffer, new SDK/runtime API or hook.

## Checks and pending proof

Provider host 9/9, unchanged codec host 4/4, scoped object build 7 tasks, both
current-image offline links, regression typecheck, generated metadata and layers
passed. SDK owner Rust tests passed 192/192 with one same-path replacement commit-cut
case using the existing RAM card/CUT/commit/snapshot seams. It proves metadata
publication over already written separate payload extents, not interrupted DATA
or hardware flush. Memory and limits are recorded in finding 115 and the design.

Sixteen device cases are registered in the provider's project: each device has
valid/missing/corrupt/oversized/held/empty/counter/transfer. Ten selected boot cases passed:
all seven DN2 plus DT2 valid/corrupt/held, one startEmpty initial baseline per
case (ten total). First DN2 valid passed 1/13 skipped in 54.61 s; remaining DN2
passed 6/8 skipped in 300.03 s; DT2 passed 3/11 skipped in 90.60 s. Four unselected
DT2 cases remain registered. Both valid cases retained all 2,166 bytes after late
c-demo loading, outside staging; empty selection also played the asset with
count/loaded 0 and counter 1. Valid scene calls/records were 4/3 with the exact
resident proof hash and discard 0. Missing/oversized made no allocation/read;
CRC-corrupt discarded once before publication. Fallback matched stock's black
last drawing, not full-animation equivalence. Held/counter skipped the getter
and selected loading. Seeded counter 2 stayed in BootInfo while the stored
counter cleared to 0 at UI. All ten passed existing 80 ms UI/input/DSP/lifetime
checks with zero write-watch/flush hits or invalid writes. Logs:
`/private/tmp/bootscreen-card-dn2-valid.log`,
`/private/tmp/bootscreen-card-dn2-remaining.log`,
`/private/tmp/bootscreen-card-dt2.log`; actual observations and bounded evidence
index: `/private/tmp/bootscreen-card-summary.json`.
No old RAM
matrix or broad reset sweep is needed. Workbench transfer is owned separately and uses the existing generic store on
both devices: Transfer → Bootscreens → Add .dkbs → Upload (PUT then fresh
VERIFY), with two-click Remove active. Activation waits for the next normal
restart; no SELECT/restart/capability detection. Basic header preflight is distinct
from the pack tool's and device boot's full validation. Opt-in provider firmware
is required. UI validation/integration results are the parent's responsibility.

Blocked I/O, physical scan/display timing, read-only recovery and hardware
compatibility remain open. No new firmware-byte claim is marked [V].

## USB transfer proof and UI lane

DN2 transfer passed 1/15 skipped in 50.55 s (one 45.898 s baseline). Existing
h.usb/DkSession PUT/verifyFile/LIST replaced the path with the CRC-repaired 40 B
black fixture; DELETE/LIST then showed absence. Three snapshots retained the same
pointer, all 2,166 bytes, source SHA, display hash, counters and selection paths/
flags; one c-demo remained loaded with zero faults. The existing 80 ms UI/input/
DSP and uploads:true safety assertions passed, with zero write-watch/flush hits,
no outside-policy writes and no invalid low writes. The raw default boot-only
lifetime report has pass:false for five expected USB store writes; the upload
policy explicitly permits them. Do not report that raw flag as true.

The first setup attempt failed before USB mutation: h.usb requires paused mode,
but the initial baseline was running. One explicit pause fixed it; only this
case was retried. Total: 11 successful unique cases/11 successful baselines,
12 attempted baselines. Five DT2 scopes remain registered, not run. Failure log:
`/private/tmp/bootscreen-card-dn2-transfer.log`; successful retry:
`/private/tmp/bootscreen-card-dn2-transfer-retry.log`. The summary JSON includes
both attempts and the bounded fourth transfer-proof.json.

Parent-accepted UI commit `75a24fb8b282580d0512fed3be28d72570162217` awaits
integration. Reported checks from that lane: initial desktop host/check:ui/types
3 tasks passed in 4.0 s; focused bootscreen/transfer UI 23/23, target-race backend
3/3, named web transfer 14/14; final desktop check:ui/types 2 tasks passed in
1.5 s. Initial web owner had 64 passed/17 skipped with transfer blocked by missing
lane Wasm; the named transfer suite passed after reusing existing ignored main
artifacts. Root viewed/accepted 1440×900 and 700×600 screenshots at
`/private/tmp/bootscreen-ui-shots/`. No dependency or artifact build was added.
Integrated cheap gate and final commit results are pending; full merge gate was
not requested and remains required before a future merge.
