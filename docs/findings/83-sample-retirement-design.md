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
after scheduling and event ordering are resolved. It tests retirement independently of the larger memory
backend needed for approximately ten-second one-shots. The product sampler and borrowed-view API remain gated.

1. Close admission for the old resource/generation before any backing mutation.
2. Drain already admitted `note_on` and `render` callbacks; prevent new old-generation callbacks from entering.
3. Invalidate active readers, armed starts and pending old-generation events, including events upstream of DSP dispatch.
4. Produce logical `RETIRED` for the exact retirement token only when no old callback or future admitted start can read the backing.
5. Mutate or load backing only after that retirement; complete delivery and validation before publishing the new generation last and reopening admission.

`RETIRED` and its token describe required semantics, not a chosen wire format. No ABI, mailbox layout or new
transfer window is approved; window 3 is merely a possible implementation location. Stale or duplicate
acknowledgments must not satisfy another request. Rapid A → B → C changes must reject obsolete completions.
On failure or timeout retain backing and keep admission closed; no timeout-based reuse or partial publication.

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

**[O]** Capture generation before an event is queued, or establish an explicit upstream ordering fence that
prevents delayed old events crossing publication. Tagging an event with the current generation only when it
arrives at the DSP mislabels delayed old notes as new. Identify every queue/trigger path and its ordering boundary.

**[D]** Current-image decompiler reads identify these producer/frame candidates; they do not establish a
generation binding or a stale-event fence. Parameters and triggers sharing a frame is insufficient.

| Image | Producer/frame observations | Remaining ordering proof |
| --- | --- | --- |
| DT2 1.17 | Builder `0x4002dd0c` processes an event queue, populates SPI frame `0x80005348`, then sends through `0x400cd420`. | Capture generation before queue admission or prove an explicit producer/frame fence. |
| DN2 1.12 | Builder `0x40025e36` populates frame `0x80005e60`, masks at `0x80005e82/84/86/88` and per-voice `0x92`-byte blocks; helper `0x400cf922` copies/configures DMA. | Full queue-to-parameter ordering and generation binding remain unresolved. |

Reproduce focused reads with `uv run python -m tools.coldfire.fw dossier ADDRESS -i IMAGE --decomp 80`
using the image and function addresses above. These are [D] decompiler observations, not original-byte audits.

**[D]** [Prior voice-context observations](45-dsp-voice-context.md) record MODE1 `0x39011cf8` on both target
images, including IRPTEN/NESTM in a software-interrupt voice context. **[O]** This does not exclude nested
callbacks. An in-flight counter needs a proven atomic admission/drain protocol and interrupt behavior. A common
block-tail hook alone guarantees neither callback quiescence nor event fencing; ordinary render-before-tail
ordering must not be inferred solely from the anchor description.

## Proof checklist and decision gate

- [x] Record the stock replacement counterexample and preserve the immutable-bank proof (finding 82).
- [x] Survey candidate DT2 release/reset boundaries and both device adapters; review the held-trigger and delayed-event hazards.
- [x] Separate transport delivery from logical consumer retirement and persist the proposed sequencing.
- [ ] Resolve callback scheduling, interrupt nesting/atomicity and safe admission/drain points before coding the protocol.
- [ ] Resolve generation capture or an upstream event-order fence before coding stale-note rejection.
- [ ] Agree a bounded owned-bank asynchronous proof contract and explicit stop scope; no product ABI follows automatically.
- [ ] On both devices, prove retirement during `note_on` and `render`, the armed gap, held triggers and delayed old events.
- [ ] Prove exact-token handling for stale/duplicate acknowledgments, rapid A → B → C, stale load completion,
  load failure and timeout with backing retained and admission closed.
- [ ] Separately prove external DT2 replace/unload/project reset coverage before borrowing stock project backing.
- [ ] Separately establish DN2 larger backing/delivery and both-device capacity for the ten-second target.

Next perform a bounded diagnostic scheduling trace plus current-image IRQ-path analysis, and trace the CF
producer/frame fence. Current static reads do not establish tail/render caller ordering or nested-interrupt
exclusion. Only then propose the concrete owned-bank
proof implementation for approval; preserve existing proof behavior until that decision. This document records
design progress, not execution of the checklist's open proofs or product implementation approval.

## Checkpoint validation

Scoped pathcheck for this finding, the findings index and the sampler plan passes; `git diff --check` passes.
The parent's `mise run test:changed -- --files docs/findings/83-sample-retirement-design.md docs/FINDINGS.md
docs/plans/dsp-sdk-and-sampler.md` passes layercheck and fails only on the ten baseline stale references in
five unchanged files. An independent design reviewer found no issues in this document; that review is not
image-byte verification. No device proof, build or full merge gate ran for this documentation checkpoint.
