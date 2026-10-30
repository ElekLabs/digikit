# Storage interrupt emulation: bounded Stage A and remaining scope

**[D/O]** Stage A checkpoint, 2026-10-31. Targets: DT2 OS 1.17 and DN2 OS
1.12. The user approved Stage A; its bounded Device-policy interrupt component is
implemented and tested, but Stage A remains incomplete on full-controller reset.
Stages B and C are unstarted. The controlled actual DT2 ISR fixture passes; full
default native fault-history acceptance remains **failed/open**. No controller
model boot or runtime interrupt-delivery proof was performed for this checkpoint.
The evidence remains **[D]**, not **[V]**.

The next action is narrow reset-resolution evidence before integrated
controller-source delivery into the real firmware handler and waiter. Keep normal
Oracle completion unchanged. The payoff is testing early sticky fault capture
through the controller path instead of synthetic ISR entry. Confidence is medium
for bounded register/component work and low for runtime integration until that
experiment. Earlier informal 1–3-day/week calendar estimates remain withdrawn;
scope and acceptance gates below replace them.

## Evidence and boundary

The public MCF54418 reference manual is retained locally in ignored
`out/refs/MCF54418RM/`; its recorded SHA-256 is
`83f0c1e0b1c16ff25724383b36ab5d71c1e18bc2eb97443813bc71f98ba2c680`.
Page-image identifiers below name the local extracted pages; printed page
numbers identify the manual sections independently.

| Evidence | Bounded conclusion | Limit |
| --- | --- | --- |
| Manual p0642, printed 25-26, IRQSTATEN | Enables status latching; clearing an enable clears its corresponding status bit | Events must enter through this rule, not unconditional status OR |
| Manual p0645, printed 25-29, IRQSIGEN | Gates the shared interrupt line from latched status | Masked status can exist without a CPU interrupt |
| Manual table 25-16, p0642 | Transfer complete (TC) has precedence over data timeout (DTOE) | Raw sticky error/completion bits are not a severity classifier |
| Manual p0350, printed 17-18 | eSDHC is INTC2 source 31, vector 223 | Separate from DMA completion |
| Manual p0349, printed 17-17 | DMA56–63 group is INTC2 source 0, vector 192 | Source arbitration matters when both are pending |
| Manual p0341/0344/0333, printed 17-9 and surrounding INTC/CPU sections | CIMR, configured interrupt level and CPU instruction-boundary sampling constrain delivery | No complete INTC implementation audit or exact timing proof |
| DT2 setup `0x4012d78e`; DN2 setup `0x4012be52` | Vector slot `0x4000037c` holds ISR `0x4012d5ba` / `0x4012bc7e`; ICR `0xfc05005f` gets 3 at DT2 `0x4012d86c` / DN2 `0x4012bf30`; CIMR command `0xfc05001d` gets 31 at `0x4012d872` / `0x4012bf36` | Current-image bounded setup only |
| DT2 `0x4012d8fc..0x4012d908`; DN2 `0x4012bfc0..0x4012bfcc` | IRQSIGEN `0xfc0cc038` is ORed with `0x00310003` | No IRQSTATEN write found in this bounded setup; not an exhaustive write search. Generic reset value `0x117f013f` enables those bits; post-RSTA state is unresolved |
| DMA59 setup DT2 `0x4012dd74`, DN2 `0x4012c438` | Separate DMA vector 192 path | Cannot substitute for controller command/data/error delivery |

Runtime sample-store I/O on both devices reaches firmware eMMC block operations:
[card reader](../../modules/core/runtime/src/rs/src/card.rs), runtime
[block wrappers](../../modules/core/runtime/src/rs/src/lib.rs) and
[firmware declarations](../../modules/core/fw/rs/src/lib.rs). Current read/write
anchors are DT2 `0x4012e1b6` / `0x4012e39c`, DN2 `0x4012c87a` /
`0x4012ca60`. This is separate from the auxiliary SHARC SPI DMA investigation.
DN2 lacks the DT2 stock path-filesystem interface, but its runtime store still
uses this block-I/O transport.

| Existing seam | Observed behavior | Reuse or remaining work |
| --- | --- | --- |
| [eSDHC](../../packages/emulator/periph/src/esdhc.rs) | Bounded Device status events, enable/signal gating and W1C are implemented; commands emit CC and readiness, with TC explicit | Full-controller reset remains unresolved; Oracle command behavior is preserved |
| [Board](../../packages/emulator/machine/src/board.rs): 1075, 1637–1698 | Oracle completion directly posts semaphores; DMA service is synchronous | Integrated proof must not use host semaphore posting as ISR/waiter evidence |
| [Runner](../../packages/emulator/machine/src/runner.rs): 226–267 | Inspected Device route delivers configured DMA59 IRQ only | Add the separate eSDHC source; this is a bounded path observation, not exhaustive absence proof |
| [INTC](../../packages/emulator/periph/src/intc.rs): 144, 167, 209 | Level/mask, hardware CIMR and IPR helpers exist | Reuse after checking actual mask/priority/CPU-IPL arbitration; `forced_candidate` near 235–258 omits source 0 and uses max(level, vector) across banks, so is not a ready universal arbiter |
| [Boot runtime](../../packages/emulator/boot/src/runtime.rs): 483, 2253 | Board is fixed to Oracle; drain counts completion events | Opt-in runtime selection needs separate integration work |
| Board snapshot near 410; boot runtime near 3357 | Save rejects undrained events and retains registers/card; restore checks firmware identity | Complete policy binding and pending-interrupt safety were not inspected/proved |

## Stages and acceptance gates

Stage A is user-approved and partially implemented below. Stages B and C remain
proposed and unstarted. Gate each before proceeding; component tests do not prove
cold boot, IRQ delivery or fault history.

### A — Device-policy status and signal component

**Implemented, bounded [D]:** the host-only `device_event(bits)` input applies one
immediate event under Device policy, accepting only `0x00310033` (CC, TC, BRR,
BWR, CTOE, DTOE and DCE). Oracle use and unsupported bits are rejected before
mutation. Bits latch independently through IRQSTATEN; clearing a status enable
clears its latched status. IRQSIGEN independently gates a read-only derived shared
line. W1C preserves unrelated status. Combined completion/error bits remain raw
sticky bits; the API does not classify severity or imply physical DMA quiescence.
The manual's TC-over-DTOE interpretation precedence does not erase either bit.

Device command response and data readiness remain synchronous. Commands emit CC
and BRR/BWR as applicable, and no longer emit TC for every command; transfer/busy
completion requires an explicit event. Contained big-endian 1-, 2- and 4-byte
writes receive the interrupt-register semantics; crossing writes are rejected
before mutation. This is a model boundary, not a hardware alignment claim.
Oracle host-write ordering and transfer effects are preserved by the code and
component tests. Snapshot byte layout is unchanged, with no queued event state
or cached interrupt line. This is not a full default native regression proof.

Validation: 21 targeted tests passed across
[manual registers](../../packages/emulator/periph/tests/esdhc_manual.rs),
[Device events](../../packages/emulator/periph/tests/esdhc_device_events.rs) and
[DMA effects](../../packages/emulator/periph/tests/esdhc_dma.rs). The constructor
reset-value case alone was rerun after extending it to both policies, and passed.
An independent static reviewer reported no issues. The coordinator's scoped
`mise run test:changed` for the three Rust and three documentation files passed
pathcheck/layers. The full merge gate was deferred and has not run. No controller
model boot was run.

**Reset limit [O]:** manual p0636, printed 25-20, supports the implemented IRQSTAT
effects of individual RSTC (clear CC) and RSTD (clear BRR, BWR, DINT, BGE and TC).
Other command/data-path reset effects remain unchanged. Manual p0637, printed
25-21, says RSTA clears types ROC, RW, RW1C and RWAC: RW is explicitly included.
That broad wording and the generic IRQSTATEN reset value `0x117f013f` leave the
post-RSTA enable state unresolved. No safe post-enable state is assumed. RSTA,
including combined reset writes, retains the legacy self-clear behavior unchanged;
it is not a hardware-faithful full-controller reset implementation. Stage A
therefore remains incomplete.

A bounded DT2 1.17 MAIN refscan used section base `0x40000400`, scan range
`0x4012d500..0x4012e600` and target range `0xfc0cc034..0xfc0cc038`: zero references,
100% coverage of 4,352 bytes and 1,190 instructions. This covers that exact range
and direct references only; it does not exclude computed aliases or establish a
global absence of IRQSTATEN writes. The next action is narrow reset-resolution
evidence, not a blanket Device-mode switch.

### B — Controller source delivery and actual waiter

Expose the Board's eSDHC line to INTC2 source 31/vector 223. Delivery must obey
actual CIMR/ICR and CPU IPL, and remain eligible until acknowledged. Keep DMA59's
source-0/vector-192 path separate; check arbitration with the DMA group pending.
Do not treat a counted Board event as interrupt delivery. Manual extracted page
p0354, section 17.3.2, gives same-level intercontroller priority INTC0 above INTC1
above INTC2. Explicitly test source 0 and cross-controller priority against the
specification before relying on the existing arbiter. Boot runtime near line 2000
services multiple IRQ sources sequentially; adding eSDHC at an arbitrary point
does not prove global scheduling fidelity. Isolate the first test to the relevant
storage sources, then scope full runtime scheduling separately.

First use a controlled command boundary to execute the real firmware handler and
wake an actual guest waiter, with no host semaphore post. Include pending masked
sources, unmasking, acknowledgment and repeated eligibility. The existing
[runner test](../../packages/emulator/machine/tests/runner.rs) near line 94 is a
synthetic DMA interrupt test, not this proof. The opt-in
[actual ISR fixture](../../packages/patchkit/tests/storage_fault_isr.rs) passes
96 invocations across stock/patched DT2; its controlled IRQSTAT double and
no-waiter helpers do not prove controller delivery or scheduler wakeup. Runtime
confidence stays low until this first integrated experiment passes.

### C — Explicit opt-in daemon and bounded fault-history proof

Only after B, expose explicit opt-in daemon mode with snapshot identity and
pending-state safety. Bound an actual read followed by a controlled fault on both
current devices, including late/error completion and failure cleanup. Establish
which boot/cache history the selected mode actually covers; an old Oracle
snapshot does not establish prior controller fault history. Run default completion
regressions to demonstrate preserved normal behavior.

The native observer assertions remain intact. A successful isolated read/IRQ
experiment is not full early/reset coverage; full native fault-history acceptance
remains a release blocker until its own assertions pass. Stage C has low
confidence pending B and the snapshot/policy audit.

## Unknowns and exclusions

Exact command/data timing, physical DMA quiescence, hardware recovery and a
complete INTC audit are not established. Software must not interpret emulator
fault injection as proof that physical DMA stopped. Existing first-release
fail-closed recovery policy remains in force; automatic recovery is out of scope.

No normal Oracle switch, production sample API, pool reservation, SHARC/core work,
source-generation implementation or hardware flashing is included. Source
lifecycle/content-change witnesses and complete relevant fault history remain
independent production gates. See [the sampler plan](../plans/dsp-sdk-and-sampler.md)
and [stock storage audit](81-stock-sampler-dependencies.md).
