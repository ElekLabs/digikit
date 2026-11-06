# Handover 2026-11-07: genuine early CPU exception proof

Em authorized the next emulator-only safety phase on `work/loadable-bootscreens`
in `/Users/em/.codex/worktrees/28d7/digitakt2`, from checkpoint `0202672b`.
One writer; no hardware, player/assets, SDK/provider phase, default module
membership or engine exception semantics changes. No push, PR or merge.
The previous two-device reset proof remains accepted in
[the prior handover](HANDOVER-2026-11-06-bootscreens.md).

Current result: genuine early guest CPU-exception cases passed on DN2 1.12
and DT2 1.17 under working-card emulator conditions. Storage, physical bypass,
read-only recovery and hardware timing remain open; no player work follows.

## Current bounded implementation

The linked `test/boot-early-exception` callback begins with illegal opcode
`0x4afc`, as in `test/dyn-crash`; its entry symbol is the expected vector-4 PC.
It does not write the reset register. The acceptance must show the real guest
handler writes a valid retained CPU record and resets twice, then the third
boot safely bypasses the callback with zero selected modules. It checks the
exact PC/vector, valid retained record, CPU fault persisted as `dk/fault`,
empty selection, DSP/UI/input response and lifetime safety. The existing
180 s three-boot window and DT2 known-main gate are reused unchanged.

Static inspection found a necessary policy bridge: `cpu.exceptions` reached
the installed runtime but not the cold worker, whose emulator default is
fatal-error handling. The patch forwards that existing policy through
`ColdInputs` before guest execution. Other producers remain false. True mode
is included in the production patched-pair key; false preserves the old key.
CardRig gets an initial-baseline opt-in using the existing RuntimeBaseline
field. Both exact cases passed after host/build checks and root review.
Preserve full output and named evidence before successful cleanup.

Pre-proof checks: the targeted fixture builder re-decoded both instructions;
the object is four bytes `4afc60fe`, with callback symbol at offset zero and no
relocations. The named coupled-feature production pair-key tests passed 2/2,
including true/false delivery separation; final regression typecheck passed.
Rust formatting and TypeScript/Markdown format checks passed. The existing
Nx daemon prerequisite passed in 16.1 s.
The case permits only profile-derived first settings-copy sectors (`setA`,
`setB`) beyond selection sectors, rejects any other observed store write and
requires an actual settings-copy write. Existing `onClose` copies only the
early-exception, startup-empty and lifetime-safety JSON to a unique temporary
evidence directory; no firmware/card or screenshot retention.

## Two-device proof results

The single exact DN2 1.12 case passed with exit 0 (1 passed / 1 skipped):
135.58 s total, 132.802 s case, 123.623 s baseline. Actual retained record:
PC `0x468046a4` equals the plan's callback entry, vector 4, `found:1`, `bad:0`,
`module:0xffffffff`, `stable:0`, `ramKept:1`, `saved:1`, no DSP/panic kind.
Persisted `dk/fault` is CPU at that PC with no module name. Safe third boot has
counter 2 and zero modules; selection stays empty and its counter clears at UI.
DSP/UI/input and final scenario lifetime assertions all passed.

- Raw output: `/private/tmp/bootscreen-dn2-early-exception.log`
- Named evidence: `/private/tmp/digi-early-exception-evidence-dn2-Mtm7qy/`
- Original cleaned case directory: `digi-early-CPU-exception-recovery-dn2-gMbx02`

The raw `lifetime-safety.json` uses the generic default policy, so its
`pass:false` flags the two intentional first settings-copy sectors. CardRig
subsequently filters only our predeclared `setA`/`setB` extras and passed its
final assertions. The observed store sectors were exactly those plus `selA`
and `selB`; write-watch/flush hits were zero and no other invalid writes were
reported. Preserve that distinction rather than rewriting the raw artifact.
That historical raw DN2 artifact remains unchanged.

The exact DT2 1.17 case then passed with exit 0 (1 passed / 1 skipped):
61.13 s total, 57.886 s case, 43.957 s baseline. Callback PC `0x47f73de4`
matches both the vector-4 retained record and persisted CPU fault. The same
valid-record, safe-counter2/zero-module, four-sector-only write, DSP/UI/input
and final lifetime assertions passed.

- Raw output: `/private/tmp/bootscreen-dt2-early-exception.log`
- Named evidence: `/private/tmp/digi-early-exception-evidence-dt2-ldET0h/`
- Original cleaned case directory: `digi-early-CPU-exception-recovery-dt2-ccMIen`

Before DT2, the reporting boundary was corrected to include CardRig's existing
explicit store-sector allowances in reported/applied facts. This changes no
permission: default extras remain empty, and existing final assertions remain.
The existing write-safety host file passed 21 tests, covering default rejection,
exact acceptance, adjacent-store rejection and unchanged firmware-area rules.
DT2's retained report has `pass:true`, exactly `setA`/`setB` extras, zero
write-watch/flush hits and no invalid writes. DN2 was not repeated for this
reporting-only change.

After DT2, the retention destination was changed from environment-specific
`/private/tmp` to `path.join(os.tmpdir(), ...)` for portability. This affects
only output location; format/type checks suffice and no emulator repeat is
required. Current retained paths above remain valid. This is a bounded
experimental checkpoint; future work follows the user's scope. The full merge
gate has not run.

Final checks: `mise run gen:modules -- --check` passed without regeneration;
`mise run test:plan` explained 65 affected merge tasks plus prerequisites.
The first `mise run test:changed` attempt completed 10/11 tasks in 42.9 s,
with the unrelated ABOUT deadline host test reporting `expired:false` where
`true` was expected (176 other host tests passed). No code or timeout was
changed: its one isolated rerun passed, then only the failed
`regression:test-host` owner was rerun and passed all 177 tests in 6.2 s.
The other ten successful preflight tasks were not repeated. Thus all selected
cheap checks completed successfully, while preserving the initial timing
failure in this ledger. Final Markdown/whitespace checks passed; no broad
device suite or full merge gate was run.

## Independent blockers and next decisions

Pre-arm work includes selection reads, area probes and a possible 1 MiB blank
scan, settings/store loads, and immediate `at_boot` fault persistence/flush.
A caller timeout cannot cancel a blocked SDHC/DMA request. This exception
patch does not change those paths or claim production recovery.

Root is considering conservative deferral as a later design option: pending
KEEP crash evidence could deny optional early work and leave normal late
handling intact, while clean early boots could defer unrelated settings load.
This is **not approved code** in this phase. A bypass candidate needs separate
received and held state; absent freshness must deny optional work, and receipt
alone would not prove the hardware's initial scan. No independent early
physical bypass is established. Read-only recovery and cached-selection refresh
after early read failure also remain open.

**[D/O]** A candidate UART dispatch call-site rule has hash
`8a1e573cfa9d7893e83dc64c6f6caeb3`, offset `0xa8`, length 4, resolving to DT2
`0x4011e6f0` / DN2 `0x40120064`. Decoder entry also sees `core/input` injection,
so a physical receipt candidate must observe the UART dispatch call site.
Optional work would require fresh channel-2 receipt **and** RECORD released;
held or missing receipt denies it. Neither packet receipt nor request
`0x60,0x00` proves an initial physical scan. Low-level `Emulator.button` can
queue pre-step packets, but `ColdInputs` has no initial-input entry point.
No hook or input bridge is approved in this exception patch.

Parallel NXP manual research is recorded in finding 115: host reset bits and
eDMA request suppression/cancellation semantics do not establish bounded
abort latency or firmware semaphore/IRQ ownership. The `EDMA_CR` inactive-write
restriction versus current-sequence cancellation remains unresolved. No
low-level storage cancellation implementation is approved.

Resume from [finding 115](../findings/115-loadable-bootscreens.md), preserving
the distinction between synthetic-reset acceptance and genuine CPU-fault
acceptance. Stop if the handler needs unavailable pre-main state or a broader
storage/runtime redesign; do not silently expand this proof.
