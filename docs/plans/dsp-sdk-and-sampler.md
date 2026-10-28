# DSP SDK and first sampler

Status: discovery and the first implementation milestone approved by Em on 2026-10-29.
Approved implementation: exact reusable DSP extractions and numerical sampler primitives. Sample transport,
resource/lifetime ABI changes and the product sampler remain proposals requiring agreement.

## Resume here

- Worktree: `/private/tmp/digikit-dsp-sampler`; branch: `refs/heads/sdk/dsp-sampler-discovery`.
  Keep Claude's original checkout on its cleanup branch untouched.
- Read this plan, then [finding 77](../findings/77-dsp-sdk-sampler-discovery.md). Check git status and the
  cleanup checkpoint before implementation; the module inventory is pinned to `219570ca`, not final cleanup.
- First approved implementation is now integrated: exact xorshift and BD/PD sine-polynomial SDK macros with
  consumer replacements; split cursor and bounded mono linear-reader kernel with numerical references/tests.
  Read the implementation checkpoint below for verification status. Sample transport, stereo ABI and hardware
  work are still unapproved; settle the next scope with Em after this milestone.
- Use workers for scoped implementation, in separate writer worktrees if parallel; keep design and integration
  in the parent. The user confirmed the personal spawn-policy hooks were reviewed and trusted in this session.
  A new session must satisfy its own applicable hook/trust requirements before delegation.
- Read-only sample delivery/lifetime discovery can continue independently. Do not launch competing gates/builds.
- Shared symlinks are input databases/manuals and `.venv`; local `out/cache/` and run outputs are isolated.
  Use `UV_NO_SYNC=1`, worktree-local `UV_CACHE_DIR`, and local `MISE_STATE_DIR` as needed. Never rebuild shared DBs
  implicitly with `sharc.load()`; use read-only queries.
- Documentation and the first approved SDK implementation are now present. Validation and known baseline failures
  are recorded below; do not mistake the numerical sample reader for a complete sampling machine.
  Em explicitly requested this documentation checkpoint commit; no push or merge was requested.

## Objective and scope

Extend the device-independent module SDK with reusable DSP operations, migrate existing machines and effects,
and build a first one-shot sampler for Digitakt II OS 1.17 and Digitone II OS 1.12. Working name: SPARK (not final).
Initial musical scope: sample selection, start/end, reverse, and normal note pitch. Granular playback, slicing,
time stretch, wavetable synthesis and multisampling are future consumers, not first-release requirements.

The payoff is a reusable sample-access contract and tested kernels that make later machines cheaper to build.
Confidence is high in the discovery approach; DN2 playback feasibility, memory capacity and delivery cost remain
open until traced. If sample delivery requires a large unrelated runtime redesign, stop and present alternatives
before extending scope. Do not survey every stock machine before delivering a sampler decision.

## Agreed direction

- Device independence is required: content modules use SDK contracts, never firmware addresses or layouts.
- Keep mathematical contracts and portable reference models; implement an optimized SHARC backend today.
  A universal processor instruction abstraction is deferred.
- Move reusable DSP operations and platform access into the SDK. Module orchestration, state transitions and
  distinctive algorithms can remain module code; wrapping every assembly instruction is not the goal.
- Offer small step operations and scheduled block kernels. Add fused kernels when a measured consumer warrants
  them; sequential macro expansion must not force a performance regression.
- Use both public SHARC manuals and targeted firmware analysis. Manuals establish semantics; firmware reveals
  integration, ownership and scheduling. Describe interfaces and idioms without copying firmware code.
- Reuse SQLite helpers and NetworkX graphs. Extend tooling only for a concrete unanswered question.
- Prefer bounded resident playback for the first sampler if the memory and transport evidence supports it.
  Card storage capacity is not a playable-RAM budget. Streaming is a separate decision.
- Em confirmed on 2026-10-29 that DN2 has no stock sampling. Its sampling service is new digikit functionality;
  general SHARC kernel research still covers both devices equally. Stock class names do not imply DN2 playback.

## Workspace and coordination

- [x] Inspect the current checkout without modifying it; Claude owns `cleanup/self-contained-modules`.
- [x] Create branch `refs/heads/sdk/dsp-sampler-discovery` in `/private/tmp/digikit-dsp-sampler`.
  Baseline: `219570cafe73da2beb605cdebe2f04950968b960` (cleanup groundwork checkpoint, not final cleanup).
- [x] Keep generated outputs local to the new worktree; do not run the shared-output worktree setup script.
- [ ] Record the final cleanup checkpoint and reconcile the inventory before module migration.
- [ ] Before builds, identify shared artifacts, use unique run outputs, and coordinate the CPU/build budget.

No checkout, reset, stash, cleanup, or edits in Claude's tree. Read-only database/manual inputs may be reused;
do not mutate them. No dependency changes in a shared environment. No hardware action without Em's decision.
The parent owns decisions, plan updates, integration and verification; scouts read, workers implement only after
the implementation decision, reviewers audit. One writer per worktree; children do not delegate.

## Phase 1 — contract and module inventory

- [x] Read SDK, module and firmware-query guidance and the latest handover.
- [x] Confirm the distinction between transferred/stored samples and DSP playback access.
- [x] Locate existing SQLite/NetworkX graph helpers and DSP reference tests.
- [x] Inventory pluck, bd, analog, pd and freqshift: initial source inventory below; measurements and final cleanup reconciliation remain open.
- [x] Audit the machine/FX ABI: source-level gaps and device adapter behavior recorded in finding 77; firmware-owned buffer lifetime remains open below.
- [x] Record current checks, timing-model limits and memory constraints from authoritative code/configuration; measurements remain pending.

Acceptance: an evidence-backed gap matrix and migration order, with implemented behavior separated from proposals.

## Phase 2 — sample path on both target images

- [ ] Inventory available current-image databases, dumps and provenance; do not rebuild them implicitly.
- [ ] Trace DT2 1.17 selection/load/slot registration through ColdFire to SHARC-readable PCM and voice access.
- [ ] Trace DN2 1.12 storage and available cross-core delivery/memory mechanisms; identify what must be added.
- [x] Establish transferred-file PCM format, channel layout and rate from the host converter; DSP conversion/representation proposal below, implementation pending.
- [ ] Establish placement, capacity, alignment, address representation, ownership, retention and eviction.
- [ ] Establish ready/failure/unavailable behavior and synchronization for sample replacement and active voices.
- [ ] Check retriggering, voice stealing, reverse boundary access and parameter changes during playback.
- [ ] Decide resident versus streaming feasibility, with explicit bounds and missing evidence.

Initial bounded investigation is complete with precise blockers; the remaining boxes require deeper traces or an
approved proof. Findings: [77](../findings/77-dsp-sdk-sampler-discovery.md). DT2 slot metadata and page-copy arithmetic
were re-read on 1.17; stock allocation/retention is not yet safe to reuse. DN2's existing link is suitable to investigate
for resident data, but its 72,000 B/s theoretical payload ceiling is below mono 48 kHz int16 streaming's 96,000 B/s.
This rules out that representation through that link, not every possible streaming design. No general DSP work
needs to wait for a new sample transport or a full stock-machine census.

Use `tools/coldfire/fw.py` atlas/catalog/dossier/batch first; use `tools/sharc_dev/sharc.py` CFG/callgraph/defuse
and `tools/coldfire/cf.py` for focused graphs. SQL is read-only and follows existing helpers. Graph absence is not
proof of absence: inspect indirect dispatch and data references. Resolve DT2 1.16 findings on 1.17 before use.
All new firmware claims stay [D]/[O] until an independent agent checks image bytes. Findings belong under
`docs/findings/` and are indexed in `docs/FINDINGS.md`; this plan tracks work and decisions.

Acceptance: a traceable sample lifecycle on each device, or precise unresolved blockers that bound the proposal.

## Phase 3 — SDK proposal and decision gate

- [ ] Propose a sample resource API: stable identity, metadata, prepare/readiness, retain/release and failure.
- [ ] Separate resource management on ColdFire from bounded DSP consumption; no file I/O or allocation in render.
- [ ] Specify playback cursor precision, signed rate, frame/channel units, start/end convention and completion.
- [ ] Specify interpolation guard reads, out-of-range behavior, gain/fades, PCM conversion and channel policy.
- [ ] Decide how selection and precise start/end editing fit the current four-word, single-page machine contract.
- [ ] Define voice stealing, retrigger, sample replacement and live parameter semantics.
- [ ] Specify state sizes, register/clobber rules, aliasing, rounding/error bounds and block contracts per kernel.
- [ ] Give a residency limit, polyphony target, memory budget and worst-case block-cost budget for both devices.
- [x] Present an initial phased proposal and API sketch below; detailed ABI and measured budgets are still open.
- [x] Em approves exact DSP extractions and numerical sampler primitives (2026-10-29).
- [ ] Em approves sample service/transport, lifetime and product-machine changes. **Those changes remain gated.**

Proposed priority:

| Stage | Operations/services | Consumers |
| --- | --- | --- |
| First sampler | sample access/lifetime; bounded fractional cursor; signed rate; safe linear interpolation; PCM/channel handling; gain ramp and short fades | one-shot playback |
| Quality follow-up | cubic interpolation; explicit anti-alias/resampling policy | pitched playback; granular and wavetable |
| Later consumers | phase accumulator; table lookup; envelopes/windows; overlap mixing | oscillators, grains, wavetables |
| Driven by migration | noise; all-pass/comb; fractional delay; filter coefficient generation | existing machines and FX |

Interpolation quality and anti-alias filtering are distinct. Do not promise that linear or cubic interpolation
provides band-limited playback at high rates. Do not duplicate stock downstream filters/envelopes without first
checking what the module adapter already provides.

## Phase 4 — implementation (first milestone approved)

- [x] Implement exact reusable DSP extractions with reference/equivalence checks (approved).
- [x] Implement numerical sampler cursor/interpolation primitives with boundary tests (approved).

- [ ] Implement the smallest sample-service/adaptor changes and test ownership/failure behavior.
- [ ] Add minimum sampler primitives with independent numerical references and meaningful boundary tests.
- [ ] Build the one-shot sampler on both devices, with agreed selection and playback controls.
- [ ] Migrate one existing consumer at a time; preserve behavior and resource usage unless a change is approved.
- [ ] Update templates and authoring guidance to use the SDK operations.
- [ ] Add an enforceable dependency check for the agreed content-module boundary where existing checks lack it.

Do not expand core generated instruction semantics or regenerate emulator cores as incidental SDK work.
Any such requirement is a separate scoped decision with its full regeneration costs.

## Phased implementation scope

Em approved exact DSP extractions and numerical sampler primitives on 2026-10-29. This covers the DSP work in
milestone 1 and numerical work in milestone 2 below. The read-only graph-constructor tooling suggestion, runtime
sample transport, full resident playback proof and product sampler are still proposals, not implementation approval.

Three milestones allow useful SDK work before the harder memory questions are solved:

1. **Exact DSP extraction.** Compare BD/PD float-phase and sine sequences and the BD/pluck xorshift paths;
   extract only operations whose semantics really match. Preserve arithmetic order, seeds, scaling and PCM.
   Keep freqshift's integer-turn oscillator separate. First integrate one consumer against its existing reference,
   then migrate the others after the cleanup checkpoint. Also add the small read-only graph-constructor option
   needed to prevent accidental `sharc.load()` rebuilds during analysis, with a focused read-only test.
2. **Bounded mono resident-sampler proof on both devices.** Add fractional cursor and boundary-safe linear read
   references, then a runtime-owned sample allocation/upload/publication path using existing DSP-link machinery.
   Start with a transferred native file below the existing 64 KiB whole-file limit; this is a proof limitation, not
   the final product sample-length promise. A packaged synthetic fixture tests the kernel but is not completion
   of the transferred-sample requirement. No continuous streaming, new transport or external DSP memory region.
3. **Product sampler decision.** Based on proof, choose the actual pool/clip limit, chunked card reads, DT2 stock
   pool adapter or owned pool, replacement behavior, stereo scope and exact endpoint UI. Then implement the user
   machine and migrate remaining modules with measured costs. Do not label the mono proof a finished sampler.

The approved DSP work in milestone 1 and the numerical part of 2 can run independently in separate writer worktrees.
Runtime sample transport and voice binding share contracts, so settle those contracts here before assigning
implementation. Run builds/gates serially under the repository's CPU budget; parallel static work is already cheap.

### Sample API sketch for review

Names below are conceptual, not new exported ABI commitments:

- Control side: `sample_prepare(asset_id)` starts a bounded job; `sample_status(handle)` reports pending/ready/error;
  `sample_bind(machine, slot, handle)` publishes only a ready immutable descriptor at a block boundary.
- The handle includes a generation so old identities cannot silently refer to newly uploaded data. It is not a raw
  firmware pointer. Runtime/adapters own physical placement, accounting, transfer serialization and failures.
- DSP side: a resolved read-only view with frame count, channels, sample rate, encoding, channel stride and
  adapter-normalized address(es). Address representation must be explicit for SHARC's memory spaces. Resolve
  selection at note-on, outside the sample loop; no per-sample virtual dispatch, file I/O, allocation or lookup.
- First proof: resources remain pinned until restart, no eviction or live replacement. This avoids pretending a
  `DK_DONE` callback covers all voice termination paths. A reclaiming product API must cover stealing, retriggers,
  faults, idle and type changes in the adapter before memory can be reused.
- Keep the sample reader independent of storage encoding; compare packed int16 with float32 on the target rather
  than committing all samples to float32 and halving capacity without measurement.

### Proposed initial playback semantics

- Frame interval is half-open `[start, end)`; forward starts at `start`, reverse at the last included frame.
  Empty intervals produce a fully written silent block and completion. Never read an interpolation neighbor beyond
  the interval; a proposed first policy clamps the neighbor at the edge. Tail samples after completion are zero.
- Cursor uses a separate integer frame index and fractional phase (precision selected with boundary/rate tests),
  avoiding an ever-growing float32 absolute position. Signed increment controls direction and note pitch; source
  rate and root-note metadata determine the ratio. Pitch range and anti-alias quality remain explicit decisions.
- Latch sample identity, bounds and direction at note-on for the first proof. Define live automation separately;
  changing a pointer mid-block is not an acceptable implementation of sample selection.
- Reuse stock downstream amp/filter behavior; any tiny boundary fade is separately specified and tested, since
  even a click-prevention fade changes the waveform. Test hard endpoints and faded endpoints independently.
- Proposed four-word layout: sample slot, start, end, reverse, with pitch from the existing voice input. This fits
  the current parameter transport but cannot provide precise frame editing of long clips with 128 knob values.
  Exact trim controls, sample browsing and true stereo remain product decisions; no silent downmix is approved.

### Acceptance before expanding the proof

- Same mathematical cursor/reader contract on both devices; synthetic exact vectors for one/two-frame samples,
  fractional rates, reverse, bounds, completion and block continuation. Numerical quality is separate from timing.
- A transferred sample actually becomes audible through the machine contract on each emulator. Invalid/truncated
  files, full allocation, transfer failure and selecting an unready asset produce a defined failure or silence.
- Account for all code/state/sample bytes. Plan for 16 concurrent voices and simultaneous triggers; do not inherit
  a conformance assumption that only one note-on consumes the block headroom.
- Report instruction ceilings and one paired A/B for any speed claim. Hardware performance/cache coherence remains
  unproved until an explicitly authorized device test. No new absolute CPU budget or sample-length promise yet.

## Validation and performance

The current audio contract is 32 frames at 48 kHz: approximately 667 microseconds for the entire audio block.
This is a deadline, not measured free time for a module. Establish stock load and reserve headroom on both devices.
Emulator instruction counts do not establish hardware cycle timing or model all pipeline/cache/DMA stalls.

- [ ] Numerical and boundary tests: empty/short samples, endpoint reads, forward/reverse, fractional rates,
  stereo, retrigger, replacement, missing samples, state across blocks and non-finite outputs.
- [ ] Meaningful reference vectors and memory guard checks for new kernels; preserve old PCM pins unless Em approves.
- [ ] Cost ceilings first: per voice/block, worst-case transitions, aggregate voices/FX, state and buffers.
- [ ] One paired A/B with `tools/devtools/coupled_ab.py` for performance claims, with identical workload and provenance.
- [ ] Assess ColdFire loading/conversion, cross-core transfer, bandwidth and underrun risks separately from arithmetic.
- [ ] Hardware timing proof only after explicit authorization; distinguish emulator evidence from hardware evidence.
- [ ] Use owner's iteration checks, then routed integration gate once before merging; no parallel gate/build.

SHARC review covers scheduling, multifunction instructions, register pressure, DAG hazards, loops, SIMD suitability,
alignment, bank/bus access and state placement. Cache/prefetch/DMA changes require an established memory path and
ownership/coherency protocol. Hoist conversions/coefficient work out of sample loops where semantics allow it.

## Evidence and progress log

### 2026-10-29 — first implementation checkpoint

- Two workers used isolated writer worktrees based on `177bec02`; the parent integrated their scoped changes.
  Original cleanup checkout remained at `219570ca` when checked; final cleanup reconciliation remains pending.
- `sdk/dsp/noise.inc`: 6-instruction preloaded-shift and 7-instruction inline-load xorshift schedules.
  BD and pluck now use these macros without changing seeds, conversion or surrounding instructions.
- `sdk/dsp/oscillator.inc`: exact 16-instruction folded-phase polynomial, used by BD and PD. The preliminary
  spoken estimate of 17 instructions was corrected by counting the actual sequence. Phase folding/warping stays
  in the consumer. All register aliases and clobbers are explicit.
- Complete assembly comparison against `177bec02` found identical executable bytes, relocations, symbols and DM
  for BD (1,056 PM bytes), PD (1,446) and pluck (1,328). Local proof: `out/runs/dsp-extract-proof/`.
- `sdk/dsp/sample.inc`: 5-instruction integer cursor update plus bounded 32-sample mono float32 reader. State is
  start/end/index/fraction/signed Q16.16 rate/done. It clamps the interpolation neighbor, never reads an exhausted
  interval, zeros the output tail, and carries exact split position across calls. This is not a storage/resource ABI.
- Numerical limitations: quantized Q16.16 rates, mono immutable float32 view, mapped source interval, no overlap,
  no playback looping/fades/anti-alias filter. Large indices beyond float32 integer precision are tested using
  sparse valid mapped addresses; a maximum-range cursor case stays reference-only because SHARC address windows
  are segmented. A numeric base+end bound alone does not establish a valid memory mapping.
- Workers ran 13 oscillator/noise tests and 45 sampler tests successfully. Sampler coverage includes independent
  combined-position arithmetic, reverse/zero/extreme rates, two-block continuation, done/tail behavior, source-read
  guards and callee-saved registers. Pure reference tests run even without an assembler.
- Sample-reader test ceiling: 1,380 instructions including caller argument loads; the two-call harness measured
  557–2,231 against its 2,761 ceiling. No hardware cycle, throughput or speedup claim.
- Independent read-only review found no concrete issues in either patch. Parent integration checks and module
  owner checks are recorded in the validation section below; a reviewer inspection does not replace them.

### 2026-10-29 — parallel discovery and independent audit

- [x] Run three disjoint read-only investigations: DT2 stock sample path, DN2 delivery/memory, and general DSP on both.
- [x] Parent audits voice lifecycle/format and uses existing SQLite-backed NetworkX call graphs for both target images.
- [x] Independent evidence auditor checks DN2 payload/capacity arithmetic, mono ABI and DT2 page-copy listing.
- [x] Record source/listing results and unresolved lifetime/coherency questions in finding 77; index it in `docs/FINDINGS.md`.
- [ ] Independently decode relevant original instruction bytes before using [V]; matching the DT2 blob hash alone is insufficient.

Corrections caught during integration: filling the same DSP byte area takes the same transfer time for mono and
stereo (duration differs); DT2's page copy is scalar head/tail plus SIMD pairs, not 512 scalar words; the slot-reader
valid branch is a separate database function fragment. DN2's 64 KiB public read limit includes the native file's
header and trailer. No implementation or performance measurement was performed.

Parent source comparison confirms a first exact extraction candidate: BD/pluck share xorshift shifts 13, -17, 5;
their float conversion paths differ and should initially remain with the caller. BD/PD share sine coefficient words
and polynomial operation order after folding, but their phase/modulation code remains distinct. Preserve scheduling
and rounding first; optimization is a later measured change. Finding 77 records this narrower scope.

### Initial module inventory (pinned baseline; source inspection, not execution)

All five include the SDK machine/FX ABI. None currently includes the DSP kernel library. The four machines import
the adapter's `_dv_exp2`; review its public SDK exposure before deciding whether to wrap or replace it. A runtime
import is not itself a hard-coded firmware address. The existing SDK's similar-sounding operations are not
automatically equivalent to the algorithms below.

| Module | Extraction candidates | State and behavior to preserve | Evidence/checks |
| --- | --- | --- | --- |
| pluck | noise/excitation, one-pole, fractional all-pass, gain | 16,512 bytes per voice including 4,096-sample line; Karplus–Strong tuning, pick comb, damping and quiet-note termination | `modules/machines/pluck/src/dsp/pluck.s`; `module:check digikit/pluck`; module-load acceptance |
| bd | phase stepping, sine approximation, noise and envelopes | 32 bytes per voice; carrier/modulator phase and counter reset, falling FM index | `modules/machines/bd/src/dsp/bd.s`; `modules/machines/bd/tests/scenarios/equiv-dyn-bd.toml` pins per-device PCM |
| analog | oscillator phase/waveshapes, noise, gain | 24 bytes per voice; free-running phases; reflective wave folder is not the SDK soft clip | `modules/machines/analog/src/dsp/analog.s`; owner module check after cleanup reconciliation |
| pd | phase stepping, polynomial sine, phase warp, slew | 36 bytes per voice; reset/bootstrap/slew and folding semantics | `modules/machines/pd/src/dsp/pd.s`; owner module check after cleanup reconciliation |
| freqshift | oscillator and polyphase all-pass stages | 664 bytes of state; preserve phase/quadrature and all-pass operation order | `modules/fx/freqshift/src/dsp/freqshift.s`; float32 reference and device scenarios described in `modules/fx/freqshift/README.md` |

BD, analog and PD declare a block instruction ceiling of 1,200 and note-on field of 200 in their descriptors.
These are source declarations, not measurements; the generated ABI describes note-on cost as note-on plus render,
so verify the checker/adapter interpretation before using those declarations as performance evidence.
PLUCK's generic delay cannot simply be substituted for its tuned feedback loop; freqshift's all-pass network cannot
be substituted with the existing biquad merely because both are filters.

Tentative migration order: extract exact shared phase/sine/noise operations where their semantics match, then one
small oscillator consumer, then the remaining oscillators, then pluck and freqshift. Start with the consumer having
the strongest existing reference proof after cleanup lands. The new sampler remains the priority for new APIs;
its linear interpolation and channel behavior belong in the initial contract, not an unspecified later extension.

### ABI and capacity observations

- Machine v1 passes pitch, velocity, block index and a mono output buffer; it exposes note-on and render, but no
  sample view, explicit channel identity, release callback or sample-resource lifetime hook. An adapter or ABI
  extension must account for those requirements; no extension has been chosen.
- FX v1 uses 64 planar floats (32 left then 32 right). Machine stereo behavior therefore needs explicit investigation;
  do not infer it from the FX contract. Definitions: `sdk/sdk.toml`, `sdk/include/dk/gen/machine.h`.
- Device profiles reserve 524,288 DSP bytes on DT2 and 327,680 on DN2, shared by loaded module code/state. These
  are not established sample-pool limits. The profiles explicitly say their 1 GHz clock is an emulator assumption,
  not a known hardware clock: `devices/digitakt-ii.toml`, `devices/digitone-ii.toml`.
- Whole-file `store_read` is bounded both by caller capacity and the runtime staging-buffer limit; a future sample
  service must not assume it can load arbitrary transferred files through that API.

### Query setup and next trace

The worktree reuses only the existing `out/cfdb/`, `out/sharcdb/`, `out/atlas/`, `out/refs/` inputs and `.venv` by
symlink. Firmware query databases are opened read-only; the worktree has its own `out/cache/` and run outputs.
Use `UV_NO_SYNC=1` and a worktree-local `UV_CACHE_DIR` when invoking uv, so dependencies are not synchronized.
Both current-image ColdFire, SHARC and atlas databases are present; image-byte provenance checks remain open.

First `fw batch` queried sample atlas entries on both images and the SHARC sample-name catalogs. The name search
does not identify a DSP sampler; this is not evidence that no relevant code exists. Next inspect the sample-loader
class and its callers/dispatch on both images, then follow data references and the historical DT2 LP0 route.
Keep symbol-name matches as candidates until their actual behavior is established. No DB rebuild was requested.

### 2026-10-29 — initial inspection

- SDK kernels and their known scheduling limits: `sdk/dsp/README.md`.
- Numerical references and kernel checks: `tools/patchkit/dsp_ref.py`, `tests/patchkit/test_dsp_kernels.py`.
- Machine inputs/entry points currently lack a sample view: `sdk/include/dk/gen/machine.h`.
- Storage distinction: `docs/design/digikit-store.md`; whole-file ColdFire read:
  `modules/core/runtime/src/rs/src/store.rs`.
- Historical stock sample path: `docs/findings/15-sharc-sample-path.md` (DT2 1.16, not current-image proof).
- Current-image kernel study: `docs/findings/72-sharc-kernels-1-17.md` (static [D]/[O], not cycle measurements).
- Existing graph layer: `tools/sharc_dev/sharc.py`; graph communities: `tools/firmware/catalog.py`.
- Public manuals and provenance: `docs/sharc/SOURCES.md`; extracted reference text lives in `out/refs/`.
- Initial read-only scout confirmed the missing public sample-access contract. No firmware address has been
  newly verified, and no build/emulator/hardware run has been performed for this discovery.

## Plan maintenance

Validation (2026-10-29, first SDK implementation):

- Parent integrated suite: `uv run python -m pytest -q tests/patchkit/test_dsp_osc_kernels.py
  tests/patchkit/test_dsp_sample_kernels.py tests/patchkit/test_dsp_kernels.py`: **81 passed**.
- `mise run module:check -- fadedlimes/bd`: **15 passed, 7 skipped**; package/fit passes DT2 and DN2.
- `mise run module:check -- fadedlimes/pd`: **22 passed, 5 skipped**; package/fit passes DT2 and DN2.
- `mise run module:check -- digikit/pluck`: **42 passed, 5 skipped**; package/fit passes DT2 and DN2.
  Five skipped cases are the suite's unselected negative fixtures; BD also has two skipped equivalence scenarios.
  These results are not a new all-module load or full firmware-device proof.
- `mise run lint:py` passes (lint and formatting). Targeted changed-document/include pathcheck and
  `git diff --check HEAD` pass.
- `mise run test:changed` passes changed Python lint/format and layercheck, but the repository-wide pathcheck
  retains 10 pre-existing references in five unchanged files (DSP recipes and web transfer/test files). No unrelated
  fixes were made; reconcile these with the cleanup session. Local logs: `out/runs/dsp-sdk-checks/`.
- Independent reviewer found no concrete issues. Full routed merge gate is still required before merging;
  it was not run for this implementation checkpoint. No hardware run or cycle/speedup measurement.

Documentation checkpoint: `177bec02`. The first SDK implementation is checkpointed separately on this branch;
use git log for its commit. Module source instruction/data identity and numerical tests support this scoped
checkpoint; sample transport/lifetime, product controls and stereo remain open.

Check an item only when its acceptance criterion is met; record evidence and residual uncertainty alongside it.
Do not mark a proposal implemented or a historical finding current. Update this plan after each meaningful
discovery or implementation checkpoint. Keep new questions visible and record accepted decisions with dates.
