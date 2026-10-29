# DSP SDK and first sampler

Status: discovery and the first implementation milestone approved by Em on 2026-10-29.
Approved implementation: exact reusable DSP extractions, numerical sampler primitives, and the bounded
loading/lifetime proof below (Em approved 2026-10-29). The product sampler remains a separate decision.

## Resume here

- Worktree: `/private/tmp/digikit-dsp-sampler`; branch: `refs/heads/sdk/dsp-sampler-discovery`.
  Keep Claude's original checkout on its cleanup branch untouched.
- Read this plan, then [finding 77](../findings/77-dsp-sdk-sampler-discovery.md). Check git status and the
  cleanup checkpoint before implementation; the module inventory is pinned to `219570ca`, not final cleanup.
- First approved implementation is now integrated: exact xorshift and BD/PD sine-polynomial SDK macros with
  consumer replacements; split cursor and bounded mono linear-reader kernel with numerical references/tests.
  Read the implementation checkpoint below for verification status. Sample transport, stereo ABI and hardware
  work remain separate decisions. Em has now approved the bounded sample transport/lifetime proof below.
- SDK checkpoint: `4cd29a3c`. The resumed loading/lifetime investigation is in
  [finding 78](../findings/78-sample-loading-lifetime.md). Read the next proof proposal below before changing runtime
  code. It preserves Transfer destinations and uses a module-owned, session-pinned DSP bank. The proof is approved;
  do not ask for its approval again. Source, packaging and runtime patches are integrated; the transferred-file
  selected DN2 boot passes after the ColdFire byte-swap helper fix. DN2 now confirms actual type 16 dispatch
  and unready PCM silence, but its first note halts the native DSP before transfer; publication subsequently
  times out (-35, no chunks acknowledged). Diagnose that note's control flow with existing PCDUMP before
  changing transport. DN2 and DT2 transferred-file proofs remain incomplete; no hardware run is authorized.
  [Finding 79](../findings/79-sample-bank-proof.md) records the evidence and current blocker.
- Use workers for scoped implementation, in separate writer worktrees if parallel; keep design and integration
  in the parent. The user confirmed the personal spawn-policy hooks were reviewed and trusted in this session.
  A new session must satisfy its own applicable hook/trust requirements before delegation.
- Read-only sample delivery/lifetime discovery can continue independently. Do not launch competing gates/builds.
- Shared symlinks are input databases/manuals and `.venv`; local `out/cache/` and run outputs are isolated.
  Use `UV_NO_SYNC=1`, worktree-local `UV_CACHE_DIR`, and local `MISE_STATE_DIR` as needed. Never rebuild shared DBs
  implicitly with `sharc.load()`; use read-only queries.
  The resumed source investigation also links the existing `out/ghidra/` dump directory read-only.
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
- [ ] At Em's request, trace stock DT2 OS 1.17 OneShot dispatch → note/render → sample access and ownership.
  Finding 77 covers metadata/pages only; finding 15 is historical DT2 1.16 evidence, not this complete path.
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

- [x] Propose a bounded sample-bank resource API and explicit readiness/failure/lifetime policy below; general eviction remains deferred.
- [x] Specify ColdFire source/transfer jobs separately from DSP consumption; no file I/O or allocation in render.
- [ ] Specify playback cursor precision, signed rate, frame/channel units, start/end convention and completion.
- [ ] Specify interpolation guard reads, out-of-range behavior, gain/fades, PCM conversion and channel policy.
- [ ] Decide how selection and precise start/end editing fit the current four-word, single-page machine contract.
- [ ] Define voice stealing, retrigger, sample replacement and live parameter semantics.
- [ ] Specify state sizes, register/clobber rules, aliasing, rounding/error bounds and block contracts per kernel.
- [ ] Give a residency limit, polyphony target, memory budget and worst-case block-cost budget for both devices.
- [x] Present an initial phased proposal and API sketch below; detailed ABI and measured budgets are still open.
- [x] Em approves exact DSP extractions and numerical sampler primitives (2026-10-29).
- [x] Em approves the bounded sample service/transport and lifetime proof (2026-10-29).
- [ ] Em approves product-machine changes beyond the proof.

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

Em approved exact DSP extractions, numerical sampler primitives and the bounded loading/lifetime proof on
2026-10-29. This covers the DSP work in milestone 1 and the bounded proof in milestone 2 below. The read-only
graph-constructor tooling suggestion and product sampler remain proposals, not implementation approval.

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

### Earlier general sample API sketch (future direction)

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

## Approved proof — loading and lifetime

Em approved this bounded bank API on 2026-10-29, narrower than the earlier general prepare/bind/release sketch.
Evidence and exact current-image mappings are in finding 78. Runtime implementation is integrated below;
the transferred-file device proof remains incomplete. The first SDK milestone is checkpointed at `4cd29a3c`.

**Scope:** prove an actual transferred mono sample can reach the existing numerical reader on both devices,
with correct ownership and failure behavior. Preserve the current Transfer destinations. No stock-pointer
borrowing, new transport, stereo ABI, eviction, or new sample browser in this proof.

**Resource declaration:** a versioned SDK sample-bank declaration associates a module-local bank id with
its DSP BSS capacity and initialized descriptor symbol. The packer resolves and validates offsets within the
owning module's DSP data/BSS; runtime accounts the capacity in that module's existing allocation. Modules do
not submit arbitrary DSP addresses. Exact record encoding/version compatibility is an implementation design
task within this scope, to be reviewed before the packer and runtime workers diverge.

**Control interface:** conceptual `dk_sample_load(bank, path)` requests one job and `dk_sample_status(bank)`
reports queued/loading/ready/error. The bank declaration identifies the owner, which must already be fully
loaded. Calls marshal into the UI task; they do not mutate the DSP queue from a MIDI callback. The path uses
the device source adapter (DT2 +Drive, DN2 digikit store). Missing support reports a capability/error result.
Only an unused bank accepts a request; repeated or concurrent requests report BUSY/ALREADY_LOADED instead of
overwriting an active view. Once a transfer starts, no destination reuse in this proof, even after failure.

**Lifetime:** one job, one module-owned bank, pinned until restart. Retriggers/voice stealing reset only voice
cursors. A failing sample job must not invoke module-load rollback. READY starts zero in initialized DSP data;
bytes transfer first, immutable descriptor fields next, READY last, each phase acknowledged. A lost final
acknowledgement may mean publication already occurred, so pin the bank regardless of the control-side error.
Never reclaim the retained ColdFire transfer buffer until the link is demonstrably quiescent. After timeout or
cancel, report failure but keep that source quarantined if necessary. Shared transport cleanup changes are in
scope only where the new lifecycle tests demonstrate they are needed.

**Proof limit:** one 32 KiB float32 bank = 8,192 mono frames at 48 kHz (~171 ms); code, descriptor and voice state
are additional. Input native file at that frame limit is 16,464 bytes including header/trailer. This keeps the
proof below the existing whole-file cap and uses the already-tested reader. The declaration is capacity-based;
this is a proof configuration, not a universal SDK or product length limit. Fit must include other modules.
Reject stereo and oversize input explicitly. Final packed PCM format and longer clips remain separate decisions.

**Source copy:** lookup/read/validate the complete native file into private ColdFire storage before asynchronous
delivery; do not retain shared staging. On DN2 require stable metadata plus matching content hash; missing
fingerprints or exhausted retries fail closed. On DT2 open only in read mode, use size(entry id), verify counts
and format, and close every successfully opened handle. First emulator proofs exclude simultaneous Transfer
writes; do not claim concurrent DT2 overwrite/delete support until its handle behavior is tested or an enforceable
exclusion exists. Bound work per UI pass; synchronous read latency must still be measured.

**Work after approval:**

- [ ] Agree declaration/record and status contracts here, then assign disjoint SDK/packer and DT2 source-adapter
  tasks to workers in separate worktrees. One worker owns shared runtime queue/lifetime integration.
- [ ] Add host tests for declaration ownership/bounds, private snapshot consistency and fake-link state transitions.
- [ ] Add current-image DT2 read-only filesystem bindings/wrapper and DN2 snapshot reader.
- [ ] Add module-owned bank job and immutable descriptor publication, serialized with existing module/registry work.
- [ ] Add a minimal proof consumer on both devices using actual transferred data; bound emulator runs and test PCM,
  silence-before-ready, failure, full-memory and retrigger behavior. No user sample audio committed.
- [ ] Report latency, memory and instructions; review product sample length, stereo and editing scope separately.

Independent evidence audit supports the source constraints, including UI task placement, conditional early-free
risk, private-snapshot requirement and per-operation filesystem locking. It does not establish hardware cache
coherence, file read latency, concurrent DT2 replacement safety or the adverse timeout schedule on hardware.

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

### 2026-10-29 — resumed source/lifetime discovery

- [x] Resume from `4cd29a3c`; preserve clean implementation checkpoint and isolated worktree.
- [x] Run disjoint read-only scouts for DT2 source acquisition and runtime/store lifecycle.
- [x] Port DT2 filesystem candidates by strict function hashes, then read current Ghidra dossiers. Correct the
  initial mistaken nearby-address inference and worktree input-link gap; record only the actual mapped entries.
- [x] Inspect UI job context, source snapshots, allocator ownership, transport source retention and READY ordering.
- [x] Obtain independent evidence audit; record caveats and the bounded next implementation proposal in finding 78.
- [x] Em agrees to the loading/lifetime proof scope above before core runtime/adapter or packaging edits (2026-10-29).

### 2026-10-29 — approved loading/lifetime implementation in progress

- Source worker: `/private/tmp/digikit-sample-source`, branch `refs/heads/sdk/sample-source-proof`.
- SDK/packer worker: `/private/tmp/digikit-sample-bank`, branch `refs/heads/sdk/sample-bank-proof`.
- Runtime worker: `/private/tmp/digikit-sample-runtime`, branch `refs/heads/sdk/sample-runtime-proof`.
- All lanes start at `a3a95073`; parent integrates into this plan's worktree. No edits in the cleanup checkout.
- Contract settled: optional one-bank DSP section v5, existing 64-byte big-endian header with bank-record offset
  at byte 60; 32-byte little-endian record (version 1, local numeric id, BSS byte offset, capacity in frames,
  initialized descriptor byte offset, three zero reserved words). Capacity 1–8,192 frames for this proof.
- DSP descriptor is four initialized zero words: READY, normal-word source pointer, frame count, sample rate.
  Runtime publishes READY last. Both packer and runtime validate ownership, bounds and initialization.
- API minor 1.3 appends `sample_bank`, `sample_load`, `sample_status`. Lookup during init yields an opaque handle;
  load queues a copied path, and execution waits for the owner's full DSP load. Status 0 unused, 1 queued,
  2 loading, 3 ready, negative existing SDK error codes on failure. No destination reuse after transfer starts.
- [x] Integrate source, packaging and lifecycle implementations; review the combined boundary behavior.
- [ ] Add the proof consumer and transferred-file emulator scenarios; execute owner checks on both devices.

Implementation resume notes (not completion):

- Source, SDK/packer and runtime patches are integrated in the parent worktree, uncommitted. Decoder/snapshot
  tests pass (five); lifecycle tests pass (seven), including delayed pointer reads and lost final READY acknowledgement.
  Worker validation: patchkit 127, dkneeds 11, SDK generation 8 tests passed; host and actual ColdFire builds passed.
- New `test/sample-proof` fixture and `sample-playback.test.ts` in `tests/regression/runtime/` are integrated. Six assembled
  consumer tests pass. The fixture packs for both devices. Device playback remains unproven until the TS scenario passes.
- Parent SDK tests pass (47), and fit tests pass (nine). `module:check test/sample-proof` reports 29 passed and five
  skipped in total; package/fit covers both devices. These checks do not establish transferred-file device playback.
- The proof bridge preserves I12/I13 and adjusts I7. Harness tests now use frame zero at the actual DN2 adapter
  entry; the stock helper's frame value 18 applies only after its return.
- Generation initially omitted all cached content declarations. A scoped fix now reads existing cache objects in
  lenient generation without compiling, with a passing regression. Fixture gallery was generated after packing.
- Independent combined review found no new issues. Existing blocking module-row cleanup can still ignore failed
  writes; the new quarantine protects the accepted module-image source but is not a universal rollback safety proof.
- DN2 emulator first failed on sandbox socket permission; escalated local emulator execution is authorized. The
  subsequent optional `cf.*` import/mandatory-anchor failure is resolved: per-device function tables contain the
  four DT2 path bindings and DN2 zeros, preserving generated symbol anchors. Runtime artifacts are rebuilt.
- DN2 boots with an empty module selection; selecting the proof module exposed `UnsupportedGuestPc(0)` before
  sample loading. Diagnostics identified context restoration by RTE at `0x40000458` after stack corruption.
  The confirmed cause is an emitted weak compiler-builtin `__bswapsi2` that calls itself: its return address
  `0x4673d290` and argument `0x00200000` match the corrupted ColdFire registers. The new `valid_bank` call to
  `from_le_bytes` triggers this before module init. The initial static init audit had found no error.
- `rust.rs` and the new `cf_rust_helpers.s` in `packages/patchkit/src/build/` now provide a strong
  private ISA_C BYTEREV cdecl helper before the builtin archive; the helper source text is included in the build
  hash. Runtime rebuilt with 33,384 instructions; the helper is eight bytes (`202f000402c04e75`) with no outgoing
  relocations. Build log: `out/runs/dsp-sdk-checks/cf-helper-runtime-build.log`.
- Worker regression `coldfire_rust_endian_conversions_return_without_recursive_helpers` in
  `packages/patchkit/tests/build.rs` passes: actual cross-compiled endian reads/writes on five inputs run on
  `ColdFireCpu`, bounded at 100 instructions, checking cdecl stack/kept registers and private helpers with no
  outgoing relocations. It ran release/offline in `out/native/target-wt/digikit-dsp-sampler/nogen`.
  Independent review found no issues. MCF5441xRM printed page 97, Table 3-4 confirms BYTEREV's operation,
  matching emulator behavior; this is not hardware proof.
- `packages/emulator/coupled/src/cold.rs` now writes bounded `boot-fault.json` diagnostics on guest faults,
  preserving the original error and capturing registers, at most 128 stack bytes, the last recorded bus PC and
  at most 32 code bytes. The rebuilt daemon uses mandatory `--coupled`; build log:
  `out/runs/dsp-sdk-checks/diagnostic-daemon-build.log`. Selected-fixture DN2 boot now passes; log:
  `out/runs/dsp-sdk-checks/sample-dev-dn2.log`. This is boot evidence, not a performance or sample-playback claim.
- First DN2 TypeScript run (session 6653) passed module upload/reboot, oversize rejection (-13), zero descriptor,
  no-fault and fresh-boot checks, then failed unready audio after selecting MIDI row 4. This navigation failure
  does not establish a DSP playback result. Live panel inspection confirmed four audio rows, MIDI, then SAMPLE
  PROOF at row 5; the corrected selection shows the fixture's START/END/REV controls.
- The scenario now selects row 5, closes the modal with NO, saves framebuffers, and asserts the adapter's actual
  `_dv_c` type from the patch plan equals the runtime-assigned type before testing unready silence. Session 39860
  failed that assertion (expected type 16, actual 0), despite unready peak zero. The saved panel was still in the
  machine menu on MIDI: a fixed 500 ms wall wait raced queued keys, so silence did not establish fixture dispatch.
- The selector now waits on `inputTap.release_at` and `status.elapsed`, polling every 50 ms with a 20 s bound;
  missing clock fields fail clearly. It mirrors the existing harness: 20 ms hold, 5 ms pre-scan/release,
  200 ms open, 80 ms navigation and 300 ms YES. Parent's known-panel NO → menu → row 5 → YES sequence confirmed
  SAMPLE PROOF START/END/REV. Completed DN2 reruns now confirm actual type 16 dispatch and exact-zero unready
  PCM, followed by valid-load error -35 instead of READY. Artifact `run-15D8JZ/publication-state.json` records
  `dl_link` state 3, error 100 (timeout), acked 0, and an unpublished descriptor. The failed timing log remains
  `out/runs/dsp-sdk-checks/sample-playback-dn2-input-timing.log`, artifact run `run-F9Pyr7`.
- Fast `dk:dev` reproduction isolates `native-trap: no decoded instruction at 0x254aa1` on the first TRIG 1,
  before sample transfer: `out/runs/dsp-sdk-checks/sample-note-dn2.log`. The later link timeout is likely
  downstream of that DSP halt; this is an inference, not a transport root cause. Whether the note fault is in
  the module integration or emulator remains open. Numerical and conformance passes do not resolve it.
- Bounded live PCDUMP traces pin the bad return to the second `DK_CALL` delay-slot store: Type 3b `u=0`
  pre-modify leaves I7 unchanged and writes zero at I6-8; the expected I6-4 return slot retains `0x254aa0`.
  The kernel epilogue reads that stale value into I12, then jumps to `0x254aa1`. Evidence:
  `out/runs/dsp-sdk-checks/unready-module-trace-window/pcdump.csv` and
  `out/runs/dsp-sdk-checks/unready-stack-alias-trace/stack-rows.json`. Public PRM pages 13-18/13-19 confirm
  `u=1` denotes post-modify. No root-cause fix yet: investigate assembler encoding/native PC handling and why
  Python call-stack tests passed despite the missing saved return.
- Focused stock OneShot investigation is now requested. Current DT2 1.17 database candidates are render
  dispatch `0x1c6b00` → `0x1c4ecf` and stretch branch `0x1c6af1` → `0x1c5576`; stock Type 16a immediate
  post-modify return stores provide a comparator. These are [D] database observations, not [V] byte audits
  or a completed sample-access/ownership trace.
- The TypeScript scenario now records `unready-state.json` (daemon status, faults and PCM summary) immediately
  after capture and calls `expectNoFault('unready proof-machine note')` before valid loading. Publication
  diagnostics also retain daemon status/faults. These latest assertions pass formatting and TypeScript checks
  only; their full emulator rerun has not run yet.
- Partial `test:changed` checks pass Ruff, layercheck and the two selected Python files
  `tests/devtools/test_changed_tests.py` and `tests/devtools/test_verify.py`. The known baseline ten stale
  references still block repository pathcheck. Host checks were blocked by pnpm 12's automatic install attempt
  and unavailable offline cache; no dependency changes were made. The later owner host related-test command
  exits zero with no selected host tests (`out/runs/dsp-sdk-checks/sample-host-related.log`); oxlint passes
  using existing Node binaries. Neither is a full gate pass. Changed-check log:
  `out/runs/dsp-sdk-checks/sample-test-changed.log`.
- Next: resolve the captured call-frame mismatch and examine the focused OneShot path before transport changes,
  then finish DN2 and DT2
  upload/publication/audio proofs. Both full proofs remain incomplete. No hardware run or timing claim.
- A standalone native-boot diagnostic halted after six instructions, before reaching the target. It is unusable
  for this investigation and does not establish proof behavior.
- Runtime artifact builds require rustup Cargo first in PATH (`/Users/em/.cargo/bin`), not Homebrew Cargo.
- Parent shares only input symlinks; Node workspace package links resolve to this worktree, dependency cache is shared.
  Ready inputs' firmware hashes match current DT2 1.17 / DN2 1.12, despite an old DN2 image label in provenance.
- Local check logs: `out/runs/dsp-sdk-checks/`. Parent completes the single `runtime/sample-playback.test.ts`
  scenario, DN2 first and DT2 last. Use bounded sessions and shared benchlock.

This approved continuation implements the bounded sample API and proof fixture. It does not establish device
playback or resolve the general conditional transfer cleanup risk; no completed device or hardware proof is claimed.

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

Validation (2026-10-29, historical resumed discovery documentation, before proof approval):

- Targeted pathcheck for the plan, finding 78 and findings index passes; `git diff --check HEAD` passes.
- `mise run test:changed` passes layercheck and fails only on the same 10 stale references in five
  unchanged baseline files recorded below. No code, firmware or hardware checks were needed for this
  documentation-only checkpoint. The runtime proof was awaiting agreement then; Em subsequently approved it.

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
