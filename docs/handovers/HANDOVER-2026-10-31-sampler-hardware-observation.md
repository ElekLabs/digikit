# Sampler: resume with a bounded hardware observation

## Checkpoint and direction

Source worktree: `/private/tmp/digikit-dsp-sampler`, branch
`sdk/dsp-sampler-discovery`, clean and fully pushed at `63a6ba13` before this
handover. Earlier checkpoints: `6623d5a8` (SSI), `0f38f288` (mask commands).
The DTIM tick checkpoint passed 60 named tests, independent static review and
scoped path/layer checks. The full merge gate remains deferred; full native
fault-history acceptance remains failed/open. No hardware proof follows from
these component results.

The user agreed to hardware observation in a separate worktree/session as the
next direction. Remaining native/storage/memory-ownership scope has low
confidence without targeted evidence, so pause further emulator expansion.
This documentation task authorizes no hardware actions or flashing. The separate
worktree/branch has not been created. One owner must coordinate device access and
preserve the other Claude checkout, edits and PIDs.

Read the [master plan](../plans/dsp-sdk-and-sampler.md),
[previous handover](HANDOVER-2026-10-30-stereo-sampler.md) and
[finding 100](../findings/100-storage-interrupt-emulation-scope.md) for prior
commands, evidence and limits. Firmware inputs and generated `out/` artifacts
are ignored and absent from the remote; tracked plans/findings preserve the
commands and evidence references, not the artifacts themselves.

## Product requirements and remaining work

Keep full stereo on DT2 OS 1.17 and DN2 OS 1.12, sixteen tracks sharing one
deduplicated project pool, and long samples bounded by real capacity/format
limits. There is no ten-second or 16 MiB product cap. Selecting a sample stops
only that track's old notes. Keep one shared SDK/sampler engine with
runtime-shipped device adapters. No production reservation is selected.

Open work includes backing ownership/coexistence and reader lifetime, native
fault-history proof, full stereo integration, per-track binding, and the
on-device picker/end-to-end flow. Reusable listing passes on both devices;
the picker is unfinished. DT2 observations cannot establish DN2 hardware parity.

## Concrete gap map for the hardware session

These are not all instruction-emulation bugs. Separate missing model behavior
from firmware ownership/lifecycle questions and unmeasured hardware behavior.
Remote control reproduces the scenario; dedicated telemetry supplies evidence.

| Priority / area | Gap and source | Useful observation |
| --- | --- | --- |
| First: ColdFire storage reset | Post-RSTA IRQSTATEN is answered statically: the model sets `0x117f013f` (finding 100); only the exact full value is open. [Finding 100](../findings/100-storage-interrupt-emulation-scope.md) and [eSDHC model](../../packages/emulator/periph/src/esdhc.rs). | Optional: read IRQSTATEN/IRQSIGEN after the firmware's normal reset completion and initialization, tagged with event order, to confirm the full value. Do not manually reset an active storage controller. |
| ColdFire delivery/order | Default Oracle storage completion bypasses real handlers; isolated Device handlers/waiters pass, but native composition of storage, timers, SSI and host inputs is unfinished. [Board](../../packages/emulator/machine/src/board.rs), [Time](../../packages/emulator/machine/src/time.rs), [native runtime](../../packages/emulator/boot/src/runtime.rs), finding 100. | Small records of command issue, DMA/controller ISR entry, captured status, acknowledgement and waiter continuation during a known sample load. This provides an ordering reference, not complete interrupt coverage. |
| SHARC cache/DMA visibility | Emulator proofs do not establish physical cache coherence or maintenance correctness for DMA buffers. [Cache/link finding](../findings/91-hardware-loop-ends-and-dsp-link-cache.md), [LP0/cache finding](../findings/95-dn2-cache-and-lp0-staging.md). | Producer sequence, DMA completion, consumer-visible sequence/checksum and relevant cache configuration at a deliberately owned buffer. DN2-specific helper addresses are not DT2 probe addresses. |
| SHARC reader/interrupt lifetime | Frame-fetch tests supply controlled interleavings; physical IRQ/DMA exclusion, callback retirement and complete writer coverage remain open. [Finding 99](../findings/99-stereo-frame-fetch-exclusion.md), [retirement design](../findings/83-sample-retirement-design.md). | Correlate publication, reader admission/exit, stop acknowledgement and reclaim events under a repeatable remote-controlled selection/playback scenario. A quiet trace alone is not an ownership proof. |
| SHARC arithmetic, separately scoped | Ordinary RECIPS/RSQRTS ROM seed values are not numerically exact in the analysis model; RECIPS has an explicit approximation. [compute_alu.py](../../tools/sharc_dev/sharc_core/compute_alu.py), [floats.py](../../tools/sharc_dev/sharc_core/floats.py), [stereo finding](../findings/96-stereo-machine-processing.md). | A small instruction probe with known input bit patterns, output bits, flags and mode, covering table indices and boundaries. Live memory dumps alone cannot identify instruction input/output reliably. Core changes require separate regeneration/validation planning. |
| Product memory ownership, not just emulator fidelity | Safe shared project backing, allocator/reclaim boundaries and coexistence are not established. [Stock sampler dependencies](../findings/81-stock-sampler-dependencies.md), [DN2 DDR finding](../findings/91-dn2-ddr-configuration.md). | Observe stock DT2 allocation/publication/retirement across repeatable load, replace and project-change scenarios, then reconcile with static firmware evidence. DT2 cannot establish DN2's available pool or ownership. |

Start with the first row. Treat SHARC arithmetic as a separate experiment, and
prioritize cache/lifetime observations for sampler integrity. Do not advertise a
full emulator gap list: this map covers the unresolved sampler-related seams.
Use firmware-query before choosing current-image probe sites; do not transplant
DN2 addresses into DT2 simply because the devices share a DSP family.

## First hardware session checklist

1. Create a separate worktree/branch from the latest pushed
   `sdk/dsp-sampler-discovery` (including this handover); `63a6ba13` is the code
   baseline. Appoint one hardware owner
   and establish session authority before touching the device. Leave the sampler
   checkpoint and other sessions' work/processes intact.
2. Inventory the actual DT2 firmware, installed runtime/probes, available host
   transport and local artifacts. Do not assume the current device matches the
   emulator's patch set. Record identity before relying on results.
3. Establish identity, log retrieval, a screenshot and one benign remote-control
   roundtrip through the existing transport before extending it. See the
   [remote-control plan](../plans/remote-control.md),
   [debug finding](../findings/46-digikit-debug.md) and
   [upload protocol](../design/digikit-upload.md). Remote commands send keys and
   encoder turns; the documented physical remote-control path remains unvalidated.
4. Choose one named blocker: observe **normal ColdFire RSTA initialization** and
   the associated IRQSTATEN (`0xfc0cc034`)/IRQSIGEN (`0xfc0cc038`) state. Plan a
   passive capture around the normal path, rather than an active controller-reset
   experiment. The audited firmware ORs IRQSIGEN and never writes IRQSTATEN; the
   static result (`0x117f013f`, finding 100) answers the blocker, so this read is
   optional and only confirms the exact value. An isolated late read cannot reconstruct initialization history.
5. Design the smallest triggered CF/SHARC records needed for that blocker and
   retrieve them after capture. Existing PEEK reads allowed ColdFire RAM only,
   1–512 bytes per request, and excludes MMIO; it is not a direct register probe
   or general SHARC readout. Any new probe/transport needs its own scoped design
   and authorization. Do not assume continuous bulk dumping is feasible.
6. Record sequence/completeness, overflow/loss and consistency checks, bounded
   capture overhead, firmware identity and probe hash, scenario and stock
   comparison. Distinguish actual measurements from interpretation. Convert the
   answered blocker into a named regression; DT2 evidence remains DT2-only.

Initial exit criterion: one reproducible capture that answers the named blocker,
with enough identity and completeness evidence to interpret it. Reassess the
next seam from that result before expanding probes or emulator scope. This does
not replace production backing/lifetime proof or full native acceptance.

## Timer boundary to preserve

Ordinary Device DTIM completed post-prescaler ticks are implemented and supported
by the manual's DTRR+1 interval and immediate FRR reset passages; no hardware cycle
accuracy or native clock integration is established. PIT counter/phase advancement
is **not implemented**. Manual p1158 wording suggests PIF at the 1→0 transition;
a PMR+1 period formula alone does not settle initial phase. Inspect the waveform
visually before implementing PIT phase. Do not copy the DTIM phase by analogy.
See [finding 100](../findings/100-storage-interrupt-emulation-scope.md).

## Copyable resume instructions

```text
Resume sampler hardware-observation planning from:
  source cwd: /private/tmp/digikit-dsp-sampler
  source branch: sdk/dsp-sampler-discovery
  code checkpoint: 63a6ba13 (fully pushed)
Read:
  docs/handovers/HANDOVER-2026-10-31-sampler-hardware-observation.md
  docs/plans/dsp-sdk-and-sampler.md
  docs/findings/100-storage-interrupt-emulation-scope.md
  docs/plans/remote-control.md
  docs/findings/46-digikit-debug.md
  docs/design/digikit-upload.md
Create a separate hardware worktree/branch from the latest pushed source branch
including this handover; 63a6ba13 is the code baseline. None exists yet.
Preserve the other Claude checkout and PIDs; use one hardware owner.
First inventory identity/transport and define a benign roundtrip, then scope one
optional normal-RSTA IRQSTATEN/IRQSIGEN capture. Obtain hardware-session authority before
hardware actions; this handover itself authorizes no device operation or flash.
Pause emulator expansion until the targeted evidence supports the next decision.
```
