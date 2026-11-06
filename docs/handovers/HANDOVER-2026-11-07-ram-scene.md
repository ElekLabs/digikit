# Handover 2026-11-07: RAM-only intro scene proof

User-authorized opt-in proof on `work/loadable-bootscreens`, based on `cd094d69`.
One core/boot-scene-proof adapter contains the linked early callback, diagnostic
scene table and stock-setup argument hook. No card/asset decoder/player,
provider/SDK phase, default patch membership, hardware, driver or deadline change.

The existing boot guard accounts before publishing the aligned table pointer.
The scene_setup entry hook preserves D0 and original SP, substitutes SP+4 only
when that pointer is nonzero, then jumps to the displaced stock setup. Raw audit
verified the six-byte LEA/CLR window and callback ABI. Three normal callbacks draw
an asymmetric diagnostic using only existing fill_rect; original stock task,
bitmap ownership, presentation and completion stay intact. Missing panel receipt
or held REC leaves the diagnostic inactive; held also takes late KEY safe mode.

The native observer now captures the validated callback bitmap at the uniquely
resolved stock callback return, before LEA12SP cleanup and stock presentation.
It does not infer coverage from individual setPixel calls. Legacy bitmap/seen/
seen_pixels/pending snapshot slots retain exact defaults and serialized values as
opaque metadata; no format bump, input regeneration or new stored revision.
framebuffer.get source:intro exposes only retained latest drawing; current remains
default and revision is the existing global display revision. This does not prove
physical OLED flush/display or create capture history.

Focused host checks passed: callback/bitmap tests 2; constructor/resolver 1 across
available DT2 1.17, DN2 1.12 and DN2 1.11; legacy snapshot roundtrip 1 with nondefault
opaque metadata on both current images; RPC source validation 1. Regression and
control-client types, generated-module, layer and diff checks passed. The attempted
Nx package-name target for control-client did not exist; its existing package
script passed. Scoped object build passed six tasks in 26.3 s. Existing daemon
prerequisite passed in 1 min 13 s. Runtime own-code DKO changed only its source hash
following normal generated binding documentation changes, not executable sections.

Scene object: text 252 B, const table 16 B, BSS 20 B, mutable data 0. Actual placed
size is 288 B object plus 12 B displaced tail =300 B expansion; the six-byte site
is patched in place. Small metadata: `/private/tmp/bootscreen-ram-scene-placement.json`.
No stack, compressed-flash or
performance claim. Six device cases are registered (active fresh STOP, missing
receipt, held REC, both devices); first approved run is only DN2 active after all
checks. The exact DN2 active case passed: 1 passed/5 skipped, exit 0, 79.29 s total,
74.363 s case, 60.550 s baseline. CALLS3/LAST2/SEEN7/ERRORS0, one guard request/
entered/returned, nonzero published pointer; exact 1024-byte index-2 drawing hash
`63053142ed4218204f369f72b315a6294777a0b9180d5a158de4810b28814a66` matched.
Safe0, selected c-demo loaded once, selection counter1; DSP150, main UI,
SETTINGS/NO roundtrip, IRQ/input and lifetime checks passed (pass:true, zero
writewatch/flush hits and no invalid writes).

All six registered cases now pass. Remaining DN2 missing/held group: 2 passed,
276.82 s, exit 0. DT2 active/missing/held group: 3 passed, 261.16 s, exit 0.
Both active cases match the exact diagnostic bitmap/hash and callback order.
Both missing cases leave pointer/calls/requests zero, retain a stock black
callback bitmap (hash `5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef`),
and load the selected module normally/counter1. Held cases leave the diagnostic
inactive, enter KEY2, skip the selected module/counter0; they do not require an
intro image. All UI/DSP/input/lifetime checks pass. Six distinct scopes have
one baseline each; no retries, budget changes or unrelated device suites.

Raw: `/private/tmp/bootscreen-ram-scene-dn2-active.log`. Retained three JSON and
intro PNG: `/var/folders/v4/dxpyscrd41n7h6v9bc672p0r0000gn/T/digi-boot-scene-evidence-dn2-qMosgn/`.
Remaining raw logs: `/private/tmp/bootscreen-ram-scene-dn2-remaining.log` and
`/private/tmp/bootscreen-ram-scene-dt2.log`. Small index with every retained
directory/outcome: `/private/tmp/bootscreen-ram-scene-summary.json`.
DT2 placement metadata: `/private/tmp/bootscreen-ram-scene-placement-dt2.json`,
also 300 B plus the six-byte in-place hook. Final `mise run test:changed` passed
all 13 tasks in 1 min 41 s (0 cached); final generated/layer/diff checks passed;
full merge gate not run, no push/PR/merge.

Future test-only deletion candidate, not implemented here: current panel taps
copy legacy 3,000 ms emulated settling while the helper defaults to 80 ms.
Before a later required cold verification, try default taps plus existing
state/frame checks and measure. No new generic UI helper or wall-time claim.

Physical initial-scan timing, blocked SDHC/DMA, read-only recovery and cached
selection refresh remain open. Drawing-callback capture is not hardware timing or
independent storage escape. Host DKBS format is not loaded by this adapter.
