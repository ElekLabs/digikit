# DN2 cache configuration and LP0 staging

2026-10-30, DN2 OS 1.12. **[D]** static inspection and existing artifacts;
**[O]** production ownership/coherence unresolved; **[C]** corrected inference.
Original-byte checks were independently audited; no new hardware, emulator run,
core change or regeneration is part of this checkpoint. Section-7 SHA-256:
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
See the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Cache contract and current RX placement

**[D]** The public [SHARC sources](../sharc/SOURCES.md), Programming Reference
Rev 1.5 printed pp. 8-5, 8-8–12 and 35-7–8, and ADSP-2156x Hardware Reference
Rev 1.0 p. 9-9 describe 64-byte DM/PM cache lines. Cache cross-checking does not
establish DMA coherence. Dirty CPU output needs writeback before a DMA read;
DMA-written RX needs invalidation before a CPU read. Maintenance requires
exclusive ownership of the whole affected cache lines, including edge padding.

**[C]** Private 0.2 RX is module BSS in `dsp.l2`, not the L1 allocator:
[`lp0-proof.s`](../../modules/core/adapters/dsp-lp0-proof/src/dsp/lp0-proof.s)
and [`module.toml`](../../modules/core/adapters/dsp-lp0-proof/module.toml).
`_lp_rx_storage` reserves 4,112 bytes with 16-byte alignment. Existing full run
`out/runs/lp0-proof/5e0b6f2a-84ac-4814-bc6e-57794f493b38` places storage at
`0x20088dc8`, aligned RX at `0x20088dd0`, active payload of 4,100 bytes at
`[0x20088dd0,0x20089dd4)`, and storage end at `0x20089dd8`.
Rounded maintenance covers 65 lines, `[0x20088dc0,0x20089e00)`: 16 bytes before
and 44 after payload, eight before and 40 after storage. The historical 0.2
alignment does not provide exclusive whole lines. This artifact is not a
hardware cache proof.

## Conditional startup configuration

**[D]** Startup `0x1c0e6e` calls `0xb8b93a` at `0x1c0ef6`. M5=0, M6=1,
M7=-1 are installed at `0x1c0e86..8e`; their preservation through preceding
calls has **not** been audited. Under those constants, `0xb8b93a` first calls
`0x1c0272` to clear CFG at `0x3e000`. Range helper `0x1c00da`, selectors
`0x602`/`0x603` and stack argument 1, programs noncacheable ranges:

| Range | Half-open byte interval |
| --- | --- |
| RR2 | `[0x200fa000,0x200fe000)` |
| RR3 | `[0x28240000,0x283a0000)` |

Helper `0x1c008c` validates selectors 2/3 and uses descriptor rows `0x240408`
/ `0x240414`, respectively `{0x3e014,0x3e015,4}` /
`{0x3e016,0x3e017,6}`, selecting CFG2 pairs `0x30` / `0xc0`; other pairs
are retained. Final CFG OR `0x00414141` enables/invalidates I, DM and PM
caches, 16 KiB each. RX lies outside both ranges and is potentially cached.
This conditional trace does not prove the live configuration or residency.
Public semantics: Programming Reference printed pp. 8-9–10, 26-1, 33-2–7 and
35-7–8; helpers were checked against original bytes.

## Stock maintenance helper and limits

**[D]** The inspected interface of helper `0x1c07e3` maps R4 to first address,
R8 to inclusive end, and R12=zero to writeback only or nonzero to DM/PM
writeback plus invalidate. Its effective range/line behavior remains conditional
on correct preceding address normalization; this is not a validated full contract.
Raw instruction `0x1c08a7 = 170078000000` requests DM and PM
writeback/invalidation for the selected maintenance range;
`0x1c08ad = 170050000000` requests writeback only for that range. Neither
invalidates I cache.
The printer's status-stack/no-op interpretation is inaccurate for hardware.

The helper saves/restores CFG2 per line but programs range 0 without restoring
RR0 start/end: range-0 scratch ownership is required. Entry IRPTEN is preserved;
critical sections cover individual lines, not the whole buffer. Prefetch is
invalidated when enabled. There is no invalidate-only operation here: post-DMA
writeback could overwrite DMA data if a dirty CPU line remains.
**[O]** Helper ABI, DAG/SIMD/stack preservation and callback context are unproved;
it is not yet a production callable maintenance API.

**[D]** Public Programming Reference Rev 1.5 printed p. 8-9 requires cache
MMR/configuration/range maintenance to execute from internal/L1 memory using
unconditional DM accesses. CFG/CFG2 changes require 12 uncompressed 48-bit
unrelated instructions before cache-MMR access, non-L1 access or a cache operation;
one instruction of separation suffices between range start/end writes. Printed
pp. 8-12–14 describe byte-addressed, 64-byte-aligned half-open ranges, joint
DM/PM operation and per-line maintenance walking controller index/count. PFB
invalidation explicitly sets then resets its control after range writeback/
invalidation. These constraints rule out casually inlining maintenance into the
current `dsp.l2` module; no implementation route is selected, and the driver's
existing cache handling is not established by this manual reading.

**[D]** Existing [`forms_system.py`](../../tools/sharc_dev/sharc_core/forms_system.py)
records maintenance events over coherent backing rather than modeling dirty
cache lines; the transpiler strips the event. Emulator execution cannot prove
physical coherence under this model.

## Correction: address-conversion encoding remains unresolved

**[C]** Raw `0x1c0807 = 04ef08800000` and
`0x1c081b = 04ef88800000` currently decode and execute as Type 7a conditional
MODIFY under NOT SV. The initial audit called these conditional W2B, but that
interpretation is plausible, not unambiguous. Programming Reference printed
pp. 14-46, 14-48 and 14-50–52 cover the relevant encodings; Tables
14-20/14-22 both allow conditional/compute forms; the existing
[`build_table.py`](../../tools/sharc_dev/sharcspec/build_table.py) rationale
selecting 7d only for condition 31 / compute zero is unsupported. Figures
14-19/14-21 overlap; the 7a figure labels `breg`/`toby`. The
[spec notes](../../tools/sharc_dev/sharcspec/README.md) already acknowledge the
contradiction. Both instructions have w/l=11, absent from the documented MODIFY
table; the current core guesses short-word behavior.

**[C]** The independent-source audit also retracts the claim that already-byte
pointers necessarily remain unchanged. Public
[Selache](https://github.com/js216/selache), pinned revision
`2b26d3b75c53063575bc5c820fa0d38879335187`, `selinstr/src/disasm.rs:1076–1105`
tests bit 39 only, ignoring bit 23; no W2B/B2W/ACONV coverage was found.
That coverage gap is not independent proof of MODIFY. The cache-opcode fields
are supported, but the helper's actual coverage depends on the unresolved
address normalization. The RX 65-line arithmetic above is independent of the
helper interpretation.

**[O]** This is a suspected model gap, not a confirmed fix. A universal shift is
unsafe: Table 6-4 conversions depend on the memory map. Hold core edits and
regeneration pending independent encoding evidence. Do not seek approval for a
core correction on the premise that W2B has been proved.

Production pool reservation, cache-safe RX ownership and CF/DSP close/drain
acknowledgement remain open. Completion-dispatch and ownership investigation
continues read-only; no production protocol or sampler readiness is established.

## Private 0.2.1 RX layout change

**[D]** Private fixture 0.2.1 reserves 4,224 bytes and computes RX with
`(storage + 63) & -64`, independently of the unchanged 16-byte-aligned,
128-byte driver state. The complete 4,160-byte rounded footprint for 4,100 DMA
bytes now fits inside RX storage, disjoint from descriptor/context/metadata.
Payload remains 4,096 bytes;
no cache calls, address conversions, descriptor protocol or public ABI changed.
The addresses above remain historical 0.2 artifacts. **[O]** Layout alone does
not provide coherence; a new native fixture bound to the exact 0.2.1 image hash
is required before claiming native evidence for this layout.

**[D]** Focused assembled checks pass all 47 cases, including each four-byte
placement phase within a 64-byte line, full RX footprint containment/disjointness,
preserved padding and payload/rearm behavior. Scoped Python/TypeScript static
checks, generation check, document paths and diff whitespace checks pass.
No new native execution, boot fixture preparation or hardware check was run.

**[C]** Ownership checks use order-independent interval disjointness, not the
Python JSON section iteration order. Production preserves declaration order;
Python's sorted section keys place the descriptor before RX and saved context
after it. The assembled phase checks derive reservation size from the actual
RX section length and check each metadata word separately. Parent object
validation built the tracked DSP object (528 instructions), redecoded it
successfully and passed `build --check`; the CF object is unchanged. This does
not supply new native or physical-cache evidence.

## Bounded STOP receive callback ownership

**[D]** An independent agent checked original DN2 1.12 section-7 bytes for the
private single-descriptor STOP RX path. Under private port ownership and no
other submitter, successful IRQ DONE callback entry grants the adapter RX
ownership until its explicit rearm. Async submit initializes completed count 0,
total 1, and descriptor head/tail. Source 81 dispatch reaches
`0x1cae6b → 0x1caf5d → 0x1cad20`.

The STOP path clears the busy byte at +2 (`0x1cadc5`), LP enable
(`0x1cade2`), and all DMA_CFG (`0x1cade8`), then reset `0xb8e210` clears
DMA address/count/config before callback `0x1cae16`. Callback arguments are
R4=context, R8=event 4, R12=address-helper result. No arm precedes the callback
on this path; the adapter copies before explicitly arming again. The inactive
descriptor is **not dequeued**: head/tail retain it and submit replaces it.
This narrows the earlier open ownership question for STOP only.

**[D]** AUTO FLOW1 is a counterexample to generalizing this result:
`0x1cad87 → 0x1cae56 → 0x1cadfc` bypasses STOP cleanup and can receive again
before callback; chained paths advance head at `0x1cae67/68` or `0x1cae52/54`.
The public [Hardware Reference](../sharc/SOURCES.md), Rev 1.0 printed p. 27-31,
places work-unit completion IRQ after acknowledgement of the last memory
transaction, while allowing next-descriptor prefetch. That is not cache visibility.

**[D]** Dispatch is synchronous SEC interrupt context, not a deferred task:
SID 81 selects an index, trampoline `0x1c02a4` uses handler table
`0x2404b0[index]`, reaching `0x1cae6b`. Registration is
`0xb8ce5c → 0x1c066c`, installation at `0x1c069d`, IVT slot 15 jumps to
`0x1c0acd`, and trampoline calls the handler at `0x1c04b1`. Do not assume
interrupts disabled: saved IRPTEN is set at `0x1c02bb`, status restored at
`0x1c03ea`, and IRPTEN cleared only after handler return at `0x1c04b7`.
**[O]** Exact nesting constraints and a callable cache-maintenance contract
remain open. STOP ownership does not establish physical cache coherence,
production pool ownership or a production close/drain protocol.


## L1 staging option and allocator failure

**[D]** Independent original-byte inspection of DN2 1.12 allocator `0xb89c9c`
shows R4 is a **byte count**: add an eight-byte header, then round to eight bytes.
Success returns R0=`block + 8`, an eight-byte-aligned private L1 byte pointer;
contents are uninitialized. The known arena is `[0x2d08b8,0x2dd0b0)`,
51,192 bytes, not measured free capacity. Calls `0xb87b3b` before and
`0xb88089` after allocation suspend/resume the scheduler and may process work:
this is a task/init-context seam, not an IRQ allocator.

**[D]** Failure with R11=0 follows
`0xb89d59 → 0xb89d96 → 0x1cd95c` (disables IRQs) → `0xb8b538 → 0x1c1430`.
The last path has unresolved parcel `0x0081` at `0x1c1431`, followed by raw
unconditional, non-delayed JUMP `0x063e001c1431` at `0x1c1432`, looping back to
`0x1c1431`. This does not provide a recoverable NULL result. Do not expose this
as an SDK allocator or use it as the proposed L1 staging shortcut. The bounded
adjacent check found `0xb89c86` empty, `0xb89c8c` minimum-free getter and
`0xb89c94` free getter, with no adjacent nonfatal allocator identified; this
is not a global absence claim.

**[D]** Helper `0x1c0776` maps private byte pointer p to DMA/completer alias
`p + 0x28000000` using plain arithmetic. Helper `0x1c0790` reverses that mapping
with arithmetic and bounds checks; neither uses ACONV. The public
[ADSP-2156x datasheet](../sharc/SOURCES.md), Tables 2/4, supports the L1 block-1
private SRAM/completer alias, subject to live protection configuration.
An owned L1 RX could avoid the L2 RX cache issue without limiting PCM length:
only roughly 4.3 KiB staging would move, while large PCM remains in DDR. However,
the fatal OOM behavior rules out this allocator-based route **as proposed**.
No allocation, free-capacity measurement or hardware check was performed.

**[C]** Moving RX to L1 would not eliminate maintenance-helper execution.
Nested submit invokes existing maintenance entry `0x1c07e4` through `0xb8e259`
at call `0xb8e334`, with descriptor flush at `0xb8e5c1`. The bounded successful
STOP callback inspection at `0x1cad20` establishes reset and normalization
before callback; it does not establish universal maintenance absence or full
RX coherence. The cache contract remains unresolved.

**[D]** Existing SDK/port inspection found no exposed SHARC L1 allocator or owned
L1 code region. `sdk.alloc` is a CF arena and `fw_alloc` a CF heap; current
ports expose `dsp.l2` placement only. **[O]** An L1 maintenance helper needs an
owned placement proof; the existing heap is not implicitly free. No export,
new API or production route is selected. Retain the aligned L2 private proof
while investigating checked production reservation, cache maintenance and real
CF/DSP transport integration. The completed logical close/ACK proof is
independent of this cache question.


## Private LP0 0.3 transport/control integration

**[D]** The 0.3 diagnostic adds exact CLOSE/ACK transport and terminal receive
rearm while retaining the 64-byte-isolated L2 RX layout. All 73 focused owner
cases pass; module-owned CF/DSP objects re-decode (450/727 instructions).
The callback masks local interrupts across stock rearm and restores incoming
MODE1; this does not settle physical interrupt behavior, cache coherence or
production backing ownership. See [finding 94](94-sample-binding-lifetime.md#private-lp0-03-transportcontrol-integration)
for the protocol, review and native evidence. The conditional address-conversion
question and rejected fatal-OOM allocator route above remain unchanged.


## Rebase: main DSP-link cache maintenance is a distinct path

**[D]** Main commit `463248e3` adds full-cache maintenance after CPU-owned DSP
code writes: combined DM/PM write-back plus invalidate, followed separately by
instruction-cache invalidation, with uncompressed instruction spacing. See
[the hardware-loop and DSP-link findings](91-hardware-loop-ends-and-dsp-link-cache.md).
That report itself leaves external/L2 execution and cacheability questions open;
its device result cannot separate the loop-encoding fix from cache maintenance.

This code-load sequence does not establish LP0 DMA receive visibility, exclusive
receive-buffer ownership or a production sample pool. In particular, post-DMA
write-back of stale dirty CPU lines remains unsafe. The sampler's cache and
reservation obligations above are unchanged. The rebased private LP0 and DDR
proof objects pass main's new hardware-loop legality check; LP0's exact patched
firmware hash also remains identical to its audited native proof.
