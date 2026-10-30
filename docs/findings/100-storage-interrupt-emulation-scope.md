# Storage interrupt emulation: bounded discovery and proposed scope

**[D/O]** Discovery checkpoint, 2026-10-31. Targets: DT2 OS 1.17 and DN2 OS
1.12. This records a proposal, not implementation approval. The controlled actual
DT2 ISR fixture passes; full default native fault-history acceptance remains
**failed/open**. No new runtime behavior or emulator experiment was performed for
this report. Evidence below is read once/bounded, not **[V]**. Independent audit agrees with
the primary claims at high confidence against cached dumps; it did not recheck
original firmware bytes, so the evidence mark remains **[D]**.

The useful next experiment is an isolated Device-policy status/register and
signal model, followed by integrated controller-source delivery into the real
firmware handler and waiter. Keep normal Oracle completion unchanged. The payoff
is testing early sticky fault capture through the controller path instead of
synthetic ISR entry. Confidence is medium for register/component work and low for
runtime integration until that experiment. Earlier informal 1–3-day/week calendar
estimates are withdrawn; scope and acceptance gates below replace them.

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
| DT2 `0x4012d8fc..0x4012d908`; DN2 `0x4012bfc0..0x4012bfcc` | IRQSIGEN `0xfc0cc038` is ORed with `0x00310003` | No IRQSTATEN write found in this bounded setup; not an exhaustive write search. Reset `0x117f013f` enables those bits |
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
| [eSDHC](../../packages/emulator/periph/src/esdhc.rs): reset near 250, writes near 321/350 | Device policy has W1C, but commands set CC and TC unconditionally; enable/signal gating and modeled errors are missing | Add explicit status events under Device policy; preserve Oracle behavior |
| [Board](../../packages/emulator/machine/src/board.rs): 1075, 1637–1698 | Oracle completion directly posts semaphores; DMA service is synchronous | Integrated proof must not use host semaphore posting as ISR/waiter evidence |
| [Runner](../../packages/emulator/machine/src/runner.rs): 226–267 | Inspected Device route delivers configured DMA59 IRQ only | Add the separate eSDHC source; this is a bounded path observation, not exhaustive absence proof |
| [INTC](../../packages/emulator/periph/src/intc.rs): 144, 167, 209 | Level/mask, hardware CIMR and IPR helpers exist | Reuse after checking actual mask/priority/CPU-IPL arbitration; `forced_candidate` near 235–258 omits source 0 and uses max(level, vector) across banks, so is not a ready universal arbiter |
| [Boot runtime](../../packages/emulator/boot/src/runtime.rs): 483, 2253 | Board is fixed to Oracle; drain counts completion events | Opt-in runtime selection needs separate integration work |
| Board snapshot near 410; boot runtime near 3357 | Save rejects undrained events and retains registers/card; restore checks firmware identity | Complete policy binding and pending-interrupt safety were not inspected/proved |

## Proposed stages and acceptance gates

These stages are a scope proposal only. Gate each before proceeding; do not promise
that cold boot, IRQ delivery and fault history will all work immediately.

### A — Device-policy status and signal component

Model command/data/error events, IRQSTATEN latching and enable-clear effects,
IRQSIGEN line gating, W1C and reset. Add a controlled one-shot fault-injection test
seam under Device policy. Preserve normal Oracle behavior and existing transfer
effects. This is a bounded component task with medium confidence.

Acceptance must distinguish status-disabled events (not latched) from
signal-disabled events (latched, then delivered when signalling is enabled).
Also cover clearing a status enable clearing its status, W1C preserving unrelated
bits, reset, and simultaneous error and completion as raw status bits. Make modeled event precedence explicit against the
manual; do not infer fault severity from the sticky union. Existing
[eSDHC manual tests](../../packages/emulator/periph/tests/esdhc_manual.rs) cover
reset/W1C, but their current Device expectations of CC and TC on every command
need a deliberately scoped update. Preserve relevant
[DMA transfer-effect tests](../../packages/emulator/periph/tests/esdhc_dma.rs).

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
