# DN2 native bulk sample-delivery seam

2026-10-29, DN2 OS 1.12. **[D]** inspected current-image evidence; **[O]** remaining
integration. Tracked in the [sampler plan](../plans/dsp-sdk-and-sampler.md).

**[C][D]** Missing cross-device function matches did not mean DN2 lacked a sender.
A direct endpoint-reference query found `0x400cfb0e`; a separate evidence agent
checked original bytes. It locks mutex `0x42441964`, sends a tag, sends 1,024
source words, waits through `0x4013492c(100)`, and unlocks. Boot initializes that
mutex at `0x400cf69e..6a4`. Helper `0x400cf2dc` emits each word least-significant
byte first; `0x400cf368` emits source bytes 1,0,3,2 for PCM16-pair swapping.
The decompiler's single-byte presentation is incomplete.

**[D]** Each helper writes 0x80 to control endpoint `0x8c00000a`, waits for bit 0
at `0x8c000002`, then writes four clock-high/low byte pairs. The readiness wait
has no timeout: an adapter must establish an armed receiver before invoking it,
or provide its own bounded failure path. No natural sample-loader caller was
established; empty caller lists do not prove the sender dead.

**[D]** The SHARC LP0 record at `0x268ca8` contains LP0 base `0x30ffe000`, DMA30
base `0x30fff000` and SEC sources 81, 82, 168. Generic opener `0x1caffb` selects
the port record, resets LP/DMA state and registers handlers `0x1cae6b`,
`0x1cac4c` and `0x1caf73`. Callback setter `0x1cab60` takes a record handle in
R4, callback in R8 and context in R12; stores them through record +0x14 under
a critical section. Public ADSP-2156x HWR Tables A-40/A-64 and 14-2 identify
the MMIO and interrupt sources; see [SHARC sources](../sharc/SOURCES.md).

**[O]** A natural application setup of a 4,100-byte receive descriptor was not
found. Existing generic driver code is a setup seam, not an armed receiver proof.
Exact submit/callback ABI, bounded failure behavior, completion/rearm and concurrent
audio must be established before runtime use. No throughput claim follows.

## Emulator path

**[D]** The CF peripheral path already forwards FlexBus bytes generically. The
coupled peer groups 0x401-word units; SHARC LP0 transfer writes the armed DMA
channel's destination/count and signals completion. However, current DN2 profiles
have no `sample_link`, so coupled startup does not enable delivery. DT2 enables
LP0. Relevant sources are `packages/emulator/periph/src/spilink.rs`,
`packages/emulator/boot/src/sharc_peer.rs` and the device profiles.

**[O]** Next bounded milestone: enable the audited DN2 1.12 route, test generic
ready/queue/rearm behavior, then prove byte-exact delivery to the existing private
experimental bank while normal audio frames continue. Private memory inspection
can observe completion in this emulator proof; it is not a production ACK channel.
Do not infer available project memory or hardware safety from this experiment.

**[D]** Raw provenance: CF section-3 SHA-256
`a098f620019b2ad38fd7e0397df39501fb1cf6bc26d0283915f34c49446fdf1a`;
DSP section-7 SHA-256
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.


## Explicit receiver setup ABI

**[D]** Follow-up original-byte inspection, cross-checked against current DT2 1.17
application setup, supplies these DN2 1.12 calls:

| Entry | Arguments and result |
| --- | --- |
| Open `0x1caffb` | R4=0, R8=1, R12=private aligned state; stack arguments 0x80 state bytes and pointer to output handle. Success returns R0=0 and handle 0x268ca8. |
| Callback `0x1cab60` | R4=handle, R8=callback, R12=context. |
| Async submit `0x1caaa6` | R4=handle, R8=descriptor, R12=1; first stack argument extra configuration 0. Success returns zero. |

The seven-word descriptor is next=0, ADDRSTART=private buffer, CFG=0x00100000,
XCNT=0x401, XMOD=4, YCNT=0, YMOD=0. The buffer must hold 4,100 bytes. The driver
modifies configuration, so reconstruct the descriptor before rearming. Completion
calls the registered function with R4=context, R8=4 and R12=normalized buffer
address; its return value is ignored. Current DT2 callback `0x1c7e43..63`
reconstructs and resubmits, corroborating this pattern. Preserve normal SHARC call
frames; the extra arguments are read at callee I6+4 and I6+8 in byte-space terms.

**[D]** Open writes owner pointer at record +0x14 before error checks; submit
mutates queue state before its busy test (which may return 4). Neither call is a
safe existing-owner probe. Preflight owner `0x268cbc`, LP0 control and DMA30
configuration before mutation, and refuse an owned or active port. Open constructs
an RTOS synchronization object and must run from task/init context. The blocking
receive path `0x1caa05` can wait indefinitely and is excluded from this experiment.

## Emulator route checkpoint

**[D]** Only the DN2 1.12 firmware profile now enables `sample_link="lp0"`.
This routes physical endpoint traffic; it does not arm a receiver or claim stock
sampling support. DN2 1.11 remains unchanged. Six firmware-free peer tests plus
the existing profile test pass. They use the real embedded profile selection and
exercise 4099+1-byte assembly, unarmed retention, exact delivery, rearm, four-unit
per-frame limit, the 100,000-instruction rearm bound, and continued SPI/PCM frames.
Independent review found no issue.

The focused native build used the existing worktree target, Rust 1.98.1, and no
SHARC_GEN_DIR: runtime-only test engine, no firmware or generated-core rebuild.
Commands/results: `out/dn2-lp0-final-checks.log`; binary provenance:
`out/dn2-lp0-test-provenance.json`. Real descriptor/ISR execution, receiver teardown,
cache behavior and audio coexistence remain unproved by these mocks.


**[D]** Existing cold-idle and ready DSP snapshots both show LP0_CTL and DMA30_CFG
zero with complete known masks. Owner 0x268cbc inherits known zero from the saved
loader image, which exactly matches packing current DN2 1.12 section 7. The
SOURCE.json label says 1.11, but its source hash and exact image comparison resolve
the provenance as 1.12. Cold snapshot SHA-256:
`f6df8e82dab21e78bc808f19416b58ecdaf1151ac545d57c65e24acd7d2bace4`;
ready snapshot SHA-256:
`779afd8e47b79a4ceb48ea2825d33fedcd0102d6754ef3eb4bac544cb59ebfb4`.
This supports preflight for those saved emulator states only; the new receiver
must still check live state before mutation.

Checkpoint validation: generation check, scoped document paths and diff whitespace
pass. Routed changed checks pass layercheck and fail only on the known ten stale
references in five unchanged files (`out/dn2-lp0-changed.log`). No full gate or
native firmware receiver proof has run for this checkpoint.
