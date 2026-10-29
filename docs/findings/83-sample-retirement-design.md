# Sample consumer retirement: design and proof boundary

2026-10-29. DT2 OS 1.17 and DN2 OS 1.12. **[D]** denotes a source read or prior observation;
**[O]** denotes a proposal or unresolved proof. No new [V], runtime implementation, ABI approval,
hardware or performance claim. This follows [the executed replacement observation](82-stock-sampler-lifetime.md)
and [the stock dependency map](81-stock-sampler-dependencies.md).

## Evidence and interception candidates

**[D]** Finding 82 establishes a bounded original-image DT2 replacement observation: a retained active voice
address and progressing cursor coexist with changed backing bytes. Clearing slot metadata is not consumer
retirement. The immutable, session-pinned owned-bank proof remains valid on both devices.

**[D]** The current DT2 static survey identifies these candidate pre-mutation boundaries. These are discovery
addresses, not approved hooks or exhaustive indirect-call coverage:

| Candidate | Observed direct callers / scope | Remaining proof |
| --- | --- | --- |
| Release `0x401543a2` | `0x4004b794`, general loader `0x4015481c` | All entry paths, locks, IRQ state and whether waiting can safely progress. |
| Reset `0x40154634` | `ProjectSettings::ReloadAllSamples` at `0x4004e8be` | Worker ordering, reset/reload ownership and safe caller context. |
| Preview `0x40154112` | Separate fixed-slot preview path | Excluded from the initial project-sample scope; no preview safety claim. |

**[O]** Do not block inside a stock release/reset hook until its caller context and progress dependencies are
proven safe. An early return from a void release is not a deferral protocol: callers may proceed assuming
release completed. External DT2 replace, unload and project reset/reload require their own interception proof.

**[D]** The delivery contract in
[`dsp-link.c`](../../modules/core/adapters/dsp-link/src/cf/dsp-link.c) says `DL_DONE` means every chunk is in DSP
L2 and instruction caches have been invalidated. It does not assert that a consumer has stopped reading.
Transport completion and consumer retirement are separate obligations.

## Proposed logical protocol

**[O]** The next implementation candidate is an asynchronous runtime-owned-bank proof on both devices,
after its scheduling, admission/drain and writer-order obligations are specified. It tests pointer retirement
independently of larger backing for approximately ten-second one-shots and of strict UI-time event-age policy.
The sequence below describes the broader logical protocol; a narrowed proof must state which event-age
obligations it does not establish. The product sampler and borrowed-view API remain gated.

1. Close admission for the old resource/generation before any backing mutation.
2. Drain already admitted `note_on` and `render` callbacks; prevent new old-generation callbacks from entering.
3. Invalidate active readers, armed starts and pending old-generation events, including events upstream of DSP dispatch.
4. Produce logical `RETIRED` for the exact retirement token only when no old callback or future admitted start can read the backing.
5. Mutate or load backing only after that retirement; complete delivery and validation before publishing the new generation last and reopening admission.

`RETIRED` and its token describe required semantics, not a chosen wire format. No ABI, mailbox layout or new
transfer window is approved; window 3 is merely a possible implementation location. Stale or duplicate
acknowledgments must not satisfy another request. Rapid A → B → C changes must reject obsolete completions.
On failure or timeout retain backing and keep admission closed; no timeout-based reuse or partial publication.
Serialize backing writes as well as acknowledgments: an obsolete B transfer must finish or be cancelled and
quiescent before C can publish or reuse its destination. Exact-token ACK rejection alone cannot stop a stale
writer from corrupting the current bank.

**[O]** Per-machine blanket stopping may be acceptable only as an explicitly bounded proof scope. Product
per-resource or per-track stopping, shared consumers and resource ownership still require decisions and proof.
The user's product direction remains “Stop old notes on selection”; this document does not choose its final scope.

## Scheduling and event-order blockers

**[D]** DT2's adapter uses active/mark state and a per-track block counter before calling `note_on`, then
`render`: [`dsp-voice.s`](../../modules/core/adapters/dsp-voice/src/dsp/dsp-voice.s), `_dv_contract`, `_dv_nback`
and `_dv_rback`. DN2's [`dsp-voice-dn2.s`](../../modules/core/adapters/dsp-voice/src/dsp/dsp-voice-dn2.s)
stores phase, previous-trigger bit and block count per slot. `_dv_cedge` sets phase 1 and silences that block;
the next block calls `note_on` and `render`. Thus retirement must cover the armed gap as well as active render.

**[D]** DN2 compares the current trigger bit against stored history and arms when current > previous.
Setting previous-trigger to zero while a trigger remains high therefore rearms it. **[O]** While closed,
consume/synchronize trigger history, clear armed phase and require a fresh low/high transition where needed;
merely zeroing state does not establish stale-start rejection.

**[O]** Separate backing-pointer safety from strict rejection of notes by their UI-time age. A proposed
owned-bank proof can establish pointer safety without an upstream CF event-age fence if every admitted
consumer resolves the current descriptor and all cached old voice pointers are drained and invalidated before
reuse. This is a proof proposal, not established behavior. Held-trigger handling and the armed gap still need
explicit coverage. Whether a delayed old note event may start the newly selected sample remains a product
decision; do not silently relax “Stop old notes on selection.” Strict UI-time stale-event rejection requires
capturing generation before queueing or establishing an upstream event-order fence. Tagging an arriving event
with the current generation cannot establish its original selection age.

**[D]** Current-image decompiler reads identify these producer/frame candidates; they do not establish a
generation binding or a stale-event fence. Parameters and triggers sharing a frame is insufficient.

| Image | Producer/frame observations | Remaining ordering proof |
| --- | --- | --- |
| DT2 1.17 | Builder `0x4002dd0c` conditionally submits existing SPI frame `0x80005348` through `0x400cd420` at entry, guarded by `0x402a19c8`; queue traversal starts at decompiler line 166 and next-frame construction at line 638. | Fence both the pending prior frame and the queue before publication; establish guard and DMA completion semantics. |
| DN2 1.12 | Builder `0x40025e36` conditionally submits existing frame `0x80005e60` through `0x400cf922` at entry, guarded by `0x40287c38`; queue traversal starts at line 181 and next-frame construction at line 953. The frame contains masks at `0x80005e82/84/86/88` and per-voice `0x92`-byte blocks. | Fence both the pending prior frame and the queue; resolve queue-to-parameter ordering and generation binding. |

**[C][D]** The earlier DT2 shorthand described queue processing, frame population and submission as if they
occurred in that order in one invocation. Both current decompilations instead submit an existing frame before
traversing the queue and reconstructing the next frame. This does not prove a fixed one-block latency:
the submit guards and DMA timing remain unresolved. A selection fence must account for the pending prior
frame as well as queued events.

**[D]** A current-section-3 absolute-reference scan with base `0x40000400` over each full section found
only the recorded builder read/write pair for each submit guard:

| Image | Scanned bytes / decoded instructions | Decode coverage | Guard read / write |
| --- | ---: | ---: | --- |
| DN2 1.12 | 3,196,288 / 977,590 | 96.61% | `0x40287c38`: `0x40025e82` / `0x40025eac` |
| DT2 1.17 | 3,275,616 / 1,007,317 | 96.81% | `0x402a19c8`: `0x4002dd58` / `0x4002dd82` |

Each scan returned two hits. This bounds the scanned absolute references only; it cannot exclude computed
initialization or unrecognized references and does not establish a safe producer fence.

**[D]** DN2 send routine `0x400cf922` copies the caller frame into DMA staging at `0x80002510`.
TCDs `0xfc045380` and `0xfc0453a0` belong to RX channel 28 and TX channel 29 respectively. The register
interpretation follows the repository's [DSPI model](../../packages/emulator/periph/src/dspi.rs) and
[eDMA model](../../packages/emulator/periph/src/edma.rs): status `0xec03802c` bit `0x10000000` is the
write-one-to-clear EOQF end-of-queue flag, not a generic transfer-start bit; a SERQ write of `0x1c` at
`0xfc044018` enables channel 28. Neither staging-copy completion nor these register operations proves DSP
reader retirement. **[O]** Guard initialization, pending-frame completion and every producer's ordering
remain unresolved; no safe fence is established.

**[D]** DN2 `0x40138e3c` inserts into a timestamp-sorted timeline rooted at `0x44645228`, linked through
`node[4]`. `node[1]` holds the timestamp; an exact timestamp match replaces `node[3]`. Observed callers include
both builder `0x40025e36` and `0x400d7d76`, so the builder is not established as the exclusive producer.
`0x40138cb6` pops a reusable event from `0x44645234`, and `0x40138cec` returns one. Resource identity and
generation semantics of these nodes remain unproved.

Reproduce full builder reads with `UV_NO_SYNC=1 UV_CACHE_DIR=out/cache/uv uv run python -m tools.coldfire.fw
decomp ADDRESS -i IMAGE --max 0`; use `dossier ADDRESS -i IMAGE --decomp 80 --max 0` for focused context or
`batch FILE` for the image/address pairs above. Line numbers refer to the full decompiler body. These are [D]
decompiler observations, not original-byte audits.

**[D]** [Prior voice-context observations](45-dsp-voice-context.md) record MODE1 `0x39011cf8` on both target
images, including IRPTEN/NESTM in a software-interrupt voice context. **[O]** This does not exclude nested
callbacks. An in-flight counter needs a proven atomic admission/drain protocol and interrupt behavior. A common
block-tail hook alone guarantees neither callback quiescence nor event fencing; ordinary render-before-tail
ordering must not be inferred solely from the anchor description.

**[D]** The current DT2 static call chain reaches the voice: callsite `0x1c771e` in function `0x1c75d8`
calls `0x1c2b24`, whose callsite `0x1c3083` reaches `0x1c642a`; callsite `0x1c6b00` there reaches voice
`0x1c4ecf`. The link-tail anchor `0x1c75c1` belongs to a different function, `0x1c74cd`, but parent
`0x1c75d8` directly calls that function at `0x1c7756`, after its voice-dispatch call at `0x1c771e`.
The conditional branch at `0x1c7753` skips to `0x1c775d`, and a loop runs from `0x1c775d` to `0x1c7749`.
These ordered static callsites do not establish every-path execution or a global interrupt-order guarantee.

**[C][D]** DN2 has an established same-parent static chain: `0x1c9e76` calls voice dispatcher `0x1c2712`
at `0x1c9fbc`; its call at `0x1c3044` reaches `0x1c8ef1`, whose call at `0x1c9459` reaches voice
`0xb8251b`. The same parent calls `0x1c9d6b` at `0x1c9ff4`, reaching link-tail `0x1c9e5f`.
The conditional at `0x1c9ff1` can skip that call, and `0x1c9ffb` loops to `0x1c9fe7`. The parent's external
caller remains unresolved, but is not required to establish these shared-parent callsites. As on DT2,
this static order does not prove every-path execution, callback drain or global interrupt ordering.
A candidate dynamic jump at `0x1c29aa` follows loading I12 from DM at base `0x25d968` plus M4, but the
jump is `JUMP PM(I4,M5)` after I4 = M9. I12 must not be equated with that branch target; the table is only
a candidate. An empty static caller result does not establish runtime absence. Reproduce these static
chains with `fw disasm ADDRESS -i IMAGE --max 0` and function/caller queries on the current images;
these are [D] reads, not a second-agent image-byte audit.

**[D]** Existing diagnostics can support part of the bounded trace without new instrumentation:
[`SHARC_IRQ_PROBE`](../INSTRUMENTATION.md) can inject the kernel tick (`0x400000`) or SFT3 context-switch
request (`0x80000000`) at interpreter instruction boundaries; `SHARC_PCDUMP` records selected PCs.
[Finding 49, section 9.3](49-dt2-type-sites-and-uninit-model.md)
records the existing probe and its limits. Injection does not create a pending DMA/SEC source or variable
peripheral event timing; interpreter-only PCDUMP may miss stock PCs executed through AOT. No diagnostic run
was executed for this discovery update.

## DN2 backing-lead correction and provenance

**[C]** An independent raw-byte audit withdraws the external-read leads previously recorded in
[finding 80](80-sampler-capacity-and-replacement.md). Direct unpack of section 7 from the current DN2 1.12
SysEx matches `out/sections/dn2-1.12/section_7_BLOB.bin`: 836,956 bytes, SHA-256
`336e340aa0cdcd34e314cfa44849f709a3134f6bd4cd57dfc7e15702c83115e2`.
`LoadedMemory` plus `decode_confident_loaded`, starting at `0x1c8ef1`, decodes `0x1c8ef6` as form 15b
`DM(I6-57) = R2`; `0x1c8ef7` is its second shortword, not an instruction boundary.
At `0x1c8f00`, the instruction decodes as form `5b_move`, `R2 = M3`. These are loader block 83 offsets
`0x6d698` and `0x6d6ac`. Neither site establishes the previously claimed external memory read.
This audit used the repository decoder; it is not an independent manual audit of general ISA semantics or
a hardware memory-access proof. No firmware or binary artifact is added to the repository.

**[O]** No larger DN2 sample backing or delivery path follows from those withdrawn leads. Next trace memory
initialization, allocation/ownership and cross-port delivery from the working voice/source seam. Failure to
find an exact address literal is not proof that a range is absent or unreachable, and the module reservation
is not a hardware memory limit.

## Proof checklist and decision gate

- [x] Record the stock replacement counterexample and preserve the immutable-bank proof (finding 82).
- [x] Survey candidate DT2 release/reset boundaries and both device adapters; review the held-trigger and delayed-event hazards.
- [x] Separate transport delivery from logical consumer retirement and persist the proposed sequencing.
- [x] Trace both CF builders' prior-frame submission order, DN2 timestamp insertion/reuse helpers and both devices' same-parent static voice/tail chains.
- [ ] Resolve callback scheduling, interrupt nesting/atomicity and safe admission/drain points before coding the protocol.
- [ ] Specify pointer-safety admission/drain separately from UI-time event-age semantics; resolve generation
  capture or an upstream event-order fence before claiming strict stale-note rejection.
- [ ] Agree a bounded owned-bank asynchronous proof contract and explicit stop scope; no product ABI follows automatically.
- [ ] On both devices, prove retirement during `note_on` and `render`, the armed gap, held triggers and delayed old events.
- [ ] Prove exact-token handling for stale/duplicate acknowledgments, rapid A → B → C, stale load completion,
  load failure and timeout with backing retained and admission closed.
- [ ] Separately prove external DT2 replace/unload/project reset coverage before borrowing stock project backing.
- [ ] Separately establish DN2 larger backing/delivery and both-device capacity for the ten-second target.

Next perform a bounded diagnostic scheduling trace plus current-image IRQ-path analysis to test the
remaining dynamic ordering and admission/drain questions on both devices. Trace submit-guard initialization,
DMA completion and every queue producer to define the pending-frame/queue fence. Current static reads do not
establish global dynamic ordering, callback atomicity or nested-interrupt exclusion. Specify the bounded owned-bank proof's pointer-safety, held-trigger, drain, exact-token and serialized-writer
obligations independently of the still-open UI-time event policy, then propose its implementation for approval;
preserve existing proof behavior until that decision. This document records
design progress, not execution of the checklist's open proofs or product implementation approval.

## Checkpoint validation

For this update, scoped pathcheck for this finding and the sampler plan passes, and `git diff --check` passes.
The parent ran `mise run test:changed -- --files` for these two documents: layercheck passes; the aggregate
fails only on the known ten stale references in five unchanged files. No device test, build or full merge gate ran.

At prior checkpoint `506b00da`, scoped pathcheck for findings 80 and 83 and the sampler plan passed;
`git diff --check` passed. The parent ran `mise run test:changed -- --files` for those three documents:
layercheck passed; the aggregate failed only on the same ten stale references in five unchanged files.
Finding 80 is unchanged in this update. No diagnostic run was executed for these static observations.

At prior checkpoint `f4d22575`, scoped pathcheck for this finding, the findings index and the sampler plan passed;
`git diff --check` passed.
The parent's `mise run test:changed -- --files docs/findings/83-sample-retirement-design.md docs/FINDINGS.md
docs/plans/dsp-sdk-and-sampler.md` passed layercheck and failed only on the ten baseline stale references in
five unchanged files. An independent design reviewer found no issues at that checkpoint; that review does
not cover today's expanded text and is not image-byte verification. No device proof, build or full merge
gate ran for that documentation checkpoint.
