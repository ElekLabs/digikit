# Storage interrupt emulation: controlled delivery and remaining scope

**[D/O]** Controlled Stage B checkpoint, 2026-10-31. Targets: DT2 OS 1.17 and
DN2 OS 1.12. The user approved delivery from explicitly initialized registers,
deferring full RSTA fidelity. The isolated controller-delivery and actual-waiter
component now passes on both firmware images. Stage A remains incomplete on full
reset; DMA/source-0 and global arbitration remain open within B. Stage C is
unstarted. Full default native fault-history acceptance remains **failed/open**.
The evidence remains **[D]**, not **[V]**.

Normal Oracle completion, ordinary `Machine::step` and daemons retain their
existing behavior. The payoff of the bounded component is observing sticky fault
capture through real eSDHC/INTC registers and the firmware ISR instead of synthetic
exception entry. It establishes waiter ready-queue insertion and a scheduling
request, not resumed task execution, cold boot or full fault history. Runtime
integration confidence remains low pending arbitration and policy/snapshot work.
Earlier informal 1–3-day/week calendar estimates remain withdrawn.

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
| [Runner](../../packages/emulator/machine/src/runner.rs) | Ordinary `step` retains configured DMA59 delivery; explicit `step_esdhc` delivers isolated eSDHC source 31 | Configured, pending and queued DMA major completions are rejected by the isolated path; arbitration is separate work |
| [INTC](../../packages/emulator/periph/src/intc.rs): 144, 167, 209 | Level/mask, hardware CIMR and IPR helpers exist | Reuse after checking actual mask/priority/CPU-IPL arbitration; `forced_candidate` near 235–258 omits source 0 and uses max(level, vector) across banks, so is not a ready universal arbiter |
| [Boot runtime](../../packages/emulator/boot/src/runtime.rs): 483, 2253 | Board is fixed to Oracle; drain counts completion events | Opt-in runtime selection needs separate integration work |
| Board snapshot near 410; boot runtime near 3357 | Save rejects undrained events and retains registers/card; restore checks firmware identity | Complete policy binding and pending-interrupt safety were not inspected/proved |

## Stages and acceptance gates

Stage A is partially implemented. The user-approved controlled B delivery/waiter
component passes from explicitly initialized registers despite unresolved RSTA.
The remaining B arbitration work and Stage C are unstarted. Component success
does not establish cold boot or complete fault history.

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

### Reset investigation and controlled checkpoint decision

**[D/O] Bounded evidence, 2026-10-31:** the full setup paths at DT2
`0x4012d78e` / DN2 `0x4012be52` and immediate command helpers at DT2
`0x4012d6c8` / DN2 `0x4012bd8c` show absolute IRQSIGEN
(`0xfc0cc038`) OR with `0x00310003`. No explicit IRQSTATEN initialization
was found in these audited paths. Offset `0x38` is IRQSIGEN, not IRQSTATEN;
these writes must not be described as IRQSTATEN ORs.

The earlier direct-reference scan covered only DT2 `0x4012d500..0x4012e600`
(4,352 bytes, 1,190 instructions, 100% coverage). The expanded raw DT2 1.17
MAIN scan covered `0x40000400..0x4031ff60`: 1,007,317 decoded instructions,
96.81% coverage and zero absolute references to IRQSTATEN `0xfc0cc034`.
Computed/indirect accesses and undecoded bytes remain outside that result;
it does not prove global absence of IRQSTATEN writes.

The manual's broad RSTA clearing rule explicitly includes RW, while the generic
IRQSTATEN reset value is `0x117f013f`. The investigation cannot distinguish
preservation, reset-default restoration or zeroing of enables after RSTA.
The locally retained public errata (`out/refs/MCF54418DE/`) p0012,
SECF215, describes an RSTA clock glitch and a disable-clock workaround. Page
p0013, SECF218, concerns SDIO and does not clarify these reset values. This was
a bounded errata search, not an exhaustive exclusion of further documentation.

Two public driver paths provide context without resolving hardware state:

- [Linux ColdFire eSDHC reset](https://raw.githubusercontent.com/torvalds/linux/master/drivers/mmc/host/sdhci-esdhc-mcf.c),
  `esdhc_mcf_reset` (audited lines 180–190), calls `sdhci_reset`, then writes
  `host->ier` to both `SDHCI_INT_ENABLE` and `SDHCI_SIGNAL_ENABLE`. Those explicit
  writes mask the hardware-default question.
- [U-Boot eSDHC driver](https://android.googlesource.com/platform/external/u-boot/+/825936815a752ce15f5a883c23bc54be76403268/drivers/mmc/fsl_esdhc_imx.c)
  includes MCF5441x branches. `fsl_esdhc_init` resets then writes IRQEN;
  `esdhc_init_common` later issues another RSTA and only clears BRR/BWR enables.
  Its command path clears signal enables and polls IRQSTAT. The inference is
  reliance on nonzero status enables, not a measurement or a distinction between
  preservation and reset-default restoration.

No trusted hardware readback was found in this bounded search. RSTA, including
combined reset writes, remains unchanged legacy behavior with unsupported
full-reset fidelity. The existing 21 passing component tests remain the current
result; no tests, builds or emulator runs were repeated for this investigation.

**Approved checkpoint decision:** proceed with controlled Stage B from explicitly
initialized registers while deferring full RSTA fidelity. This relaxes the earlier
full-Stage-A prerequisite only for this bounded component. No hardware readback,
full reset, cold boot or native fault-history release acceptance is implied.
Trusted enable readback remains a possible separate scope; SECF215 requires a
quiescent, isolated controller and erratum handling before a procedure is defined.

### B — Controller source delivery and actual waiter

**Implemented, bounded [D]:** explicit
[`Machine::step_esdhc`](../../packages/emulator/machine/src/runner.rs) requires
`CompletionPolicy::Device` and an attached `Time` with `TimerPolicy::Device`.
It samples the derived eSDHC line at an instruction boundary, reads source-31
eligibility from the existing Time INTC bank (vector 223, actual IMR/ICR), checks
the live CPU IPL and enters through `Cpu::take_interrupt`. Device Time adds only
byte SIMR/CIMR writes selecting INTC2 source 31; other INTC accesses retain their
existing passthrough behavior. Fixtures initialize IMR and all required registers
explicitly. CALL/SALL, source 0 and full INTC hardware semantics are not added.

Invalid policies, missing Time, configured legacy DMA delivery and pending,
queued or held Device DMA major completions are rejected before mutation. An
exception-entry failure has a separate poisoned-state error requiring the machine
to be discarded. The line remains derived from register state, so masked status
survives until unmasking and an unacknowledged line is eligible again after RTE.
Actual IRQSTAT W1C deasserts it. There is no serialized new mode or pending-line
boolean, and no daemon or default Oracle/ordinary-step change.

The [machine tests](../../packages/emulator/machine/tests/esdhc_irq.rs) pass all
nine cases for enable/signal/mask/ICR/IPL behavior, W1C versus repeat delivery,
atomic prerequisite/DMA rejection, poisoned entry, Device no-host-posting and
ordinary-step/Oracle preservation. Four existing runner and four timer-storage
tests also pass. Log: `out/runs/storage-fault-proof/stage-b-delivery-tests.log`.

The [firmware fixture](../../packages/patchkit/tests/storage_fault_isr.rs) loads
exact stock DT2 1.17 and DN2 1.12 inputs, checks each original six-byte site and
links the shared compiled observer directly into an in-memory copy. The packaged
module remains DT2-only; this is not a DN2 production port. An ordinary guest
CMD13 creates CC, then the fixture verifies that the blocked waiter has not been
released or host-posted before delivery. It optionally adds an immediate controlled
Device event between that command boundary and interrupt entry.

Four cases (CC, CTOE+CC, TC+DTOE+CC and TC+DCE+CC), on stock and patched copies of
both devices, pass: 16 actual ISR/RTE invocations and waiter insertions, with 36
entries each at the real signal wrapper `0x4000155c` and helper `0x400011ee`.
Each replay is bounded by 2,048 instructions. Assertions cover exact final status,
semaphore consumption and pointers, ready-head insertion, waiter self-links,
high-ready selection, scheduling-request bit 13 at `0xfc050014`, W1C line
acknowledgement and interrupted registers/SR/PC/A7. Stock and patched observables
match. CTOE+CC leaves command status zero after CC, while the observer retains the
raw CTOE bit. TC followed by DTOE or DCE leaves data status `-1` and retains the
raw error. The original DT2 no-waiter fixture still passes 96 ISR invocations.
Log: `out/runs/storage-fault-proof/stage-b-firmware-tests.log`.

The route and fixture received independent static review with no issues reported.
The only added dependency is the local `machine` dev-dependency for patchkit; no
external packages were added. Exact firmware hashes, both opt-in environment
variables and reproduction commands are in the
[adapter README](../../modules/core/adapters/storage-fault-proof/README.md).

**Remaining [O]:** queue insertion and a forced scheduling request do not prove
resumed task execution or the full scheduler. No complete read transfer, DMA
ordering/quiescence, cold boot or native fault-history capture is proved. The next
proposed slice is deliberate simultaneous DMA59/eSDHC ordering and source-0
arbitration from initialized state before daemon integration. Manual p0354,
section 17.3.2, gives same-level intercontroller priority INTC0 above INTC1 above
INTC2; source 0 and cross-bank priority remain unproved and unimplemented here.
The existing arbiter must not be treated as a validated global scheduler.

### C — Explicit opt-in daemon and bounded fault-history proof

Only after the remaining B arbitration proof, expose explicit opt-in daemon mode with snapshot identity and
pending-state safety. Bound an actual read followed by a controlled fault on both
current devices, including late/error completion and failure cleanup. Establish
which boot/cache history the selected mode actually covers; an old Oracle
snapshot does not establish prior controller fault history. Run default completion
regressions to demonstrate preserved normal behavior.

The native observer assertions remain intact. A successful isolated read/IRQ
experiment is not full early/reset coverage; full native fault-history acceptance
remains a release blocker until its own assertions pass. Stage C is unstarted and has low
confidence pending the remaining B work and the snapshot/policy audit.

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

For this checkpoint, the coordinator's scoped `mise run test:changed` across the
11 changed files passed pathcheck and layer checks. The full merge gate remains
deferred and has not run. Documentation pathcheck and diff checks cover this
update; no additional build or emulator run was performed for the prose change.
