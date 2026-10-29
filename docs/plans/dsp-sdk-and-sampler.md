# DSP SDK and first sampler

Status: discovery and the first implementation milestone approved by Em on 2026-10-29.
Approved implementation: exact reusable DSP extractions, numerical sampler primitives, and the bounded
loading/lifetime proof below (Em approved 2026-10-29). The product sampler remains a separate decision.

## Resume here

**Latest checkpoint: DN2 native bulk transfer with stock audio passes (2026-10-29).**
The private LP0 adapter uses the existing CF sender and SHARC driver. The
first-page proof passes, then the separately selected full-bank proof passes:
235 packets, 960,000 exact bytes, final 1,536-byte clamp, intact 4 KiB guards,
terminal DMA inactive, and subsequent ordinary DSP frames. Checkpoint `31d96245`
is pushed. A subsequent held-stock-note case also passes: finite non-silent PCM
before, during and after the full transfer, with no silent 100 ms window in the
measured transfer interval. This is emulator evidence, not production pool
ownership or hardware safety.
See [finding 92](../findings/92-dn2-native-bulk-seam.md) for artifacts and limits.

- [x] Implement private task-context setup, guarded callback and descriptor rearm.
- [x] Pass 21 focused tests, scoped static checks and independent review.
- [x] Run byte-exact first-page and full-bank native proofs.
- [x] Prove finite non-silent stock DN2 playback before/during/after transfer;
  record per-window metrics without claiming dropout-free hardware audio.
- [ ] Bound stock fixed DDR object access before choosing production reservation.
- [ ] Establish a device-side completion channel preserving stock reply data;
  header and trailer candidates both have stock accessors (finding 88).
- [ ] Integrate per-track loading/publication and prove both devices.

Ownership audit: fixed FX object rooted at `0x8005912c` has bounded ring indices
and a layout ending at `0x80459388`; workspace bounds remain conditional on the
live frame count. Continue with the next fixed roots and output-pointer provenance.
Existing small allocation entry is an L1 heap;
no general reservable DDR arena has been established. Keep the experimental bank
private and unpublished to voices until ownership is resolved.
The following smaller ring object is also locally bounded; a lookup index is
bounded by a signed-halfword load, correcting the initial unbounded-word reading
(finding 91). Completion research now evaluates an explicit reply extension:
verify actual CF/DSP buffer extents and all DMA/copy counts before any mutation.
No stock header/trailer bytes are approved for reuse.


**Confirmed (2026-10-29): independent per-track sampling.**
Changing a track's sample stops that track's old notes; unrelated tracks keep
playing. Em confirmed this interpretation and asked us to continue until a real
decision is needed. Parser checkpoint `db3514f7`, audit checkpoint `bc7ad3a6` and bounded source
checkpoint `3b402543` are pushed. Source-track checkpoint is `03b6310e`. The existing spare-frame channel has a theoretical 72,000-byte/second
ceiling on DN2; 960,000 bytes require at least 13.33 seconds before overhead.
This is not measured loading time. The diagnostic's global mute is unsuitable
for runtime integration.

Current parallel work (source cursor reviewed; 59 SDK tests, no_std and owner pytest pass):
- [x] Resolve loading policy and record independent per-track selection.
- [x] Map SDK input/version checks: append-only input needs a machine minor gate;
  DN2 physical voice slots must not be treated as track IDs.
- [x] Trace the current DN2 frame's source-track producer and same-handler assignment ordering ([finding 90](../findings/90-dn2-track-owner-and-worker.md)); dynamic reassignment proof remains open.
- [x] Trace DN2 background-worker callbacks against current image bytes ([finding 90](../findings/90-dn2-track-owner-and-worker.md)); no sample allocator follows from generic worker dispatch.
- [x] Add bounded DN2 store reads with generation checks and mandatory digest completion ([finding 87](../findings/87-pcm16-streaming.md)).
- [x] Audit DN2 DDR configuration: 512 MiB configured geometry, stock scratch beyond loaded extent ([finding 91](../findings/91-dn2-ddr-configuration.md)).
- [ ] Establish explicit DN2 pool reservation from runtime pointer origins/bounds; configuration is not ownership.
- [x] Implement machine contract 1.1 source-track input (runtime 0.5.3), with DN2
  delayed-note latching and invalid-owner isolation; 17 focused + 8 adapter tests,
  generation/object/header checks and independent review pass (finding 90).
- [ ] Integrate owned backing, per-track generation binding and live transport.
- [x] Audit DN2 generic 4 KiB FlexBus sender and generic LP0 driver ([finding 92](../findings/92-dn2-native-bulk-seam.md)); empty function matches did not mean absence.
- [x] Enable/test DN2 1.12 emulator LP0 delivery: six peer tests and one profile
  test pass, reviewed; runtime-only native build, no generated-core changes.
- [x] Establish generic driver setup/callback/submit ABI and preflight obligations (finding 92).
- [ ] Implement/prove private receive setup/completion/rearm and byte-exact bank
  transfer during normal audio frames. Refuse owned/active port; initialize from
  task context. Existing driver open and submit mutate state before failure.
  Payoff: bypass the 72 kB/s SPI ceiling and occupied reply rows for payload.
  Confidence: moderate until receive setup and concurrent audio run; reuse the
  experimental bank, with no new production reservation or hardware operations.

Checkpoint checks: changed-document pathcheck, diff whitespace and direct generated
JSON formatting pass. Routed `test:changed` passes scoped Python/layer checks but
retains ten baseline stale references and the pnpm linked-node_modules launcher
failure for host/format/types. Direct host tests pass 195 with 16 skipped across
the initial run and the six socket tests rerun with local-socket permission; the
initial six failures were EPERM. Logs: `out/track-changed.log`, `out/track-host.log`,
`out/track-host-sockets.log`. No full merge gate or hardware run is claimed.

Completed static audits:
- [x] [Finding 88](../findings/88-dn2-reply-ownership.md): current DN2 1.12 ACK/diagnostic row bytes reach outgoing records. Header +0x08 is only a candidate and has a stock DMA-buffer accessor of unresolved reachability. No safe eight-byte reply slot is established.
- [x] [Finding 89](../findings/89-dt2-waveform-workspace.md): the 32 MiB pool above 0x80a00008 is a recurring min/max summary workspace with a forward bump allocator. It cannot be borrowed as free sample memory.
- [x] Source consistency: DT2 read calls serialize individually; no cross-read immutable lease or digest is established. Concurrent same-size Transfer replacement remains unsupported by the current source path.
- [ ] Choose and validate transport and backing integration that preserve unrelated tracks. No hardware probe/upload has been authorized or performed.

**Runtime integration in progress (2026-10-29):** Em asks us to continue
through routine implementation/checkpoints and pause only for a real decision,
with occasional progress updates. Preserve unrelated audio during sample loading
as confirmed by Em; do not inherit the diagnostic's global mute.

The completed independent implementation is a bounded-memory native PCM16 stream
validator/packer in the shared runtime SDK. It accepts fragmented input and
bounded output, validates the canonical mono 48 kHz header/trailer, and emits
little-endian packed words with odd final-frame padding. A late validation error
invalidates all staged output; nothing becomes READY until complete validation
and source-consistency checks succeed. The existing pinned float32 ABI stays
unchanged. This removes the need to stage/decode the entire ten-second source in
the 512 KiB CF arena; it does not itself establish an immutable source lease.

- [x] Map transport geometry and existing source snapshot constraints.
- [x] Implement and test bounded streaming validation/packing with backpressure.
Streaming checkpoint: 53 shared runtime SDK tests, the named owner pytest and
host `no_std` compilation pass. The new parser emits only complete packed words,
handles odd final frames and output backpressure, and fails sticky on bad/truncated
or trailing input. It does not change the current decoder/runtime/public ABI.
Evidence and integration gates: [finding 87](../findings/87-pcm16-streaming.md).

- [ ] Establish owned DSP backing and source-consistency strategy for integration.
- [ ] Connect serialized runtime selection/retirement/loading/publication.
- [ ] Add module controls and user-facing selection, then prove on both devices.

**Current checkpoint (2026-10-29): shared lifecycle primitives extracted.**
Em approved implementation and parallel independent agent work. The SDK now
provides `sdk/dsp/sample_bank.inc`: caller-bound mask/restore, admission checks,
fresh/cached generation handling, acquire/release, fail-closed bookkeeping,
close/drain and publication. Both diagnostic producer and consumer use it.
Wire validation, exact successor policy, state storage and transport remain
caller-owned. The six-word numerical cursor and public pinned float32 bank ABI
are unchanged; no general allocator or public selection API was added.

Validation: 99 focused assembled tests pass (84 existing + 15 independently
bound SDK cases). Independent review finds no issues. The DSP object still has
912 instructions; only its source fingerprint changed. Code/data sections,
relocations and symbols are identical to `e5c274d6`. Parent rebuilt complete
patched DT2 1.17 and DN2 1.12 images with the existing CLI and verified exact
identity with the previously passing images. Evidence:
`out/runs/sample-bank-extraction/image-identity.json` and
[finding 86](../findings/86-sample-bank-sdk.md). Direct host checks pass 195 with 16 skipped; scoped path checks pass. Routed
checks retain baseline reference and pnpm launcher failures (finding 86). No
emulator/core regeneration or coupled rerun is needed for unchanged firmware bytes. Instruction identity
preserves the schedule; it is not a hardware performance measurement.

- [x] Map consumer/producer lifecycle and existing runtime upload ownership.
- [x] Choose parameterized, instruction-identical primitives and caller-owned policy.
- [x] Implement SDK include and refactor both diagnostic paths.
- [x] Test alternate state/register/label bindings and lifecycle failure behavior.
- [x] Independently review contracts and compare complete patched image identity.
- [x] Record the extraction evidence and runtime integration boundary.
- [ ] Next: integrate a runtime-owned replaceable packed-PCM16 bank and sample selection.

Runtime integration must reuse existing source snapshots and `sample_job::Job`
source-retention behavior instead of creating a parallel loader. Its current
pinned mono-float32 ABI has neither generations nor replacement and cannot be
silently changed. The larger backing still needs an explicit owner/reservation,
a packed-PCM16 source/transfer path, and serialized select/retire/load/open state.
The SDK now supplies the DSP building blocks; production ownership, device
transport and UI integration remain separate tasks. Preserve stop-old-notes and
late-note/current-generation policy; uncertain transfer completion never permits
reuse. Hardware reservation/cache/coexistence are still open.

**Current checkpoint (2026-10-29): DT2 lifecycle port is implemented and all six
cases per device pass across the initial and corrected runs.**
DT2 uses normal frame length `0x802`, DN2 `0xa80`; both retain the diagnostic
`0xa80` send and `0xabc` reply. The DSP object and SDK reader are unchanged.
Independent original-image audits establish the transport and a boot-stream gap
for the bounded emulator bank; they do not establish runtime/hardware ownership.
DT2 diagnostic type 11 preserves retired type 10 (historical XSLICe).

Validation: 84 focused assembled CF/DSP tests; 195 host passes/16 skips;
scoped lint/types/object/path checks. Initial suite: nine passes and three DT2
playback-setup failures due to treating append insertion value 255 as a menu row.
The corrected test verifies the live menu membership and selects the observed row;
it also reads the correct per-device dispatched-type diagnostic. All six affected
playback/lifecycle cases pass on both devices in
`out/runs/ddr-proof/dt2-port-playback-corrected.log` (443.42-second wall time,
not a performance measurement). The six rejection cases passed in
`out/runs/ddr-proof/dt2-port-suite.log` and were not unnecessarily repeated.

DT2 full playback artifact: `out/runs/ddr-proof/dt2/a3273fdd-d212-4cb8-98ce-025ed8d6c427`.
Terminal retirement: `out/runs/ddr-proof/dt2/4f054e94-6ff7-4483-91b3-22164872ea6f`.
A → B sealed skip → C: `out/runs/ddr-proof/dt2/efe92f48-b20f-4b3c-9526-fa462c245b34`.
[Finding 85](../findings/85-dt2-sample-lifecycle.md) records raw-image provenance,
independent audits, hashes/audio and limits. The existing prebuilt DT2 emulator
reports generated DSP core `dt2-1.16` while executing DT2 1.17 firmware; no core
regeneration was performed. Ten baseline stale references and the pnpm launcher
failure remain in routed changed checks; no clean integration gate is claimed.

- [x] Independently audit DT2 transport and candidate bank against original bytes.
- [x] Port private diagnostic and per-device tests; preserve retired type numbers.
- [x] Review and correct live-menu lookup and device-specific dispatch observation.
- [x] Complete all six coupled cases per device; audit retained DT2 playback/lifecycle evidence.
- [x] Record validation and independent audits in this plan and finding 85.
- [x] Extract shared DSP lifecycle primitives and document runtime ownership boundaries (finding 86).

The next extraction should separate numerical reader/cursor operations from bank
publication, reader admission/retirement and device transport. Keep the proven
stop-old-notes policy and exact successor generations. Public sample selection,
Transfer integration and hardware reservation/cache/coexistence remain separate
work; do not expose this hardcoded diagnostic bank as a general allocator.

**Prior DN2 checkpoint:**
**Current checkpoint (2026-10-29): private generation reopening is implemented and all six coupled cases pass.**
The user selected: stop currently playing notes on selection; queued notes starting
after new READY play the new bank. Exact successor tokens, sealed COMMIT, explicit
OPEN, operation-specific ACKs and ACK-derived held-note loading grants are implemented.
The private host writer owns whole transactions and stops permanently after uncertain
completion, even when callers catch the error. There is no cancellation: seal B,
retire B, then load C.

Validation: 68 focused assembled diagnostic/CF tests; 195 host passes/16 skips
(`out/runs/ddr-proof/reopen-host-checks.log`); six coupled passes
(`out/runs/ddr-proof/generation-reopen-suite.log`, 202.04-second wall time, not a
performance measurement). A → B sealed skip → C artifact:
`out/runs/ddr-proof/5103437f-373f-4f40-b9f9-c2ef10a73698`. C's full readback matches
its source, guards remain intact, a fresh note uses C, and stale-B RETIRE times out
without changing C or preventing resumed normal frames. An independent audit confirms
all 72,544 C frames finite and window RMS 0.0094734175. The tightened sixth-case
finite/RMS assertions also pass in `out/runs/ddr-proof/generation-reopen-audio-check.log`;
artifact `out/runs/ddr-proof/9a8eff19-98bf-4d2d-b643-8461f07fd6a2` retains 72,448
finite frames and the same measured window RMS. Metrics save before audio verdicts.
Objects re-decode 370 CF / 912 DSP instructions.

The ten known reference failures and pnpm launcher failure remain in changed routing
(`out/runs/ddr-proof/reopen-changed-checks.log`); no clean integration gate is claimed.
[Finding 84](../findings/84-sample-generation-reopening.md) retains exact hashes,
audio evidence and scope. Next: port the owned-bank lifecycle proof to DT2, then
extract the reusable SDK/runtime contract for both devices. No public API or new
hardware pool ownership is approved; hardware cache/coexistence remain separate work.
The queued-note policy is settled.

**Prior independent guard checkpoint (2026-10-29): the independent cached-voice generation guard is complete.**
The private seventh state word preserves the six SDK cursor words; state/clear size is
28 bytes. Forty-two focused assembled diagnostic/CF checks and all five existing coupled
cases pass. Host checks pass 193 with 16 skipped. Coupled log:
`out/runs/ddr-proof/voice-generation-suite.log` (119.59-second wall time, not a performance
measurement); normal artifact `out/runs/ddr-proof/55f9f2f3-ff27-47d5-aa3b-1d80f4054be6`,
terminal artifact `out/runs/ddr-proof/d1974ff3-48fa-46f1-8bec-6feb6b6f9314`.
[Finding 84](../findings/84-sample-generation-reopening.md) records the guard and proposed
next protocol. Those coupled results predate the explicit-OPEN wire changes. The selected policy stops
currently playing notes on selection; queued notes that start after new READY play
the new bank. The ten known path-reference
failures and pnpm launcher failure remain in routed changed checks; no clean gate is claimed.

**Prior terminal-retirement checkpoint:**
All five coupled cases pass: full 960,000-byte transport and ten-second playback,
three fresh-boot failure cases, and terminal retirement during progressing playback.
Log: `out/runs/ddr-proof/terminal-retirement-suite.log`; its 121.38-second wall time is
not a performance measurement. Normal playback artifacts:
`out/runs/ddr-proof/6f65ad4c-3cf6-4553-8500-15eadd79bc51`. Terminal artifacts:
`out/runs/ddr-proof/ff8f8057-0a9f-4030-95ff-83143b7ac2a5`.

The terminal request observed cursor 49664/DONE=0. Exact RETIRED then reported
READY/open=0, closed=1, admitted=0, fault=0 and 1,572 source-kernel calls, below natural
completion at 15,000. Only after that ACK did the test poison all 960,000 backing bytes.
Both full canary bands retained the expected pattern. Source-kernel calls remained 1,572
while the original trigger stayed held, after an actual fresh note-on, and after duplicate
RETIRE. Normal frames progressed after poison; callbacks returned DONE and no fault.
[Finding 83](../findings/83-sample-retirement-design.md) retains hashes and observation limits.

The private terminal contract opens admission only at the first complete COMMIT.
RETIRE (op 4, seq 377, exact bank token, zero offset/count) permanently closes admission
and READY. Status 6 means PENDING while an admitted callback remains; only exact status
5 means RETIRED. CF retains one immutable staged request and retries boundedly; timeout
never permits backing reuse. Admission, final-read release and closure use short
IRPTEN-masked sections, two conservative NOPs after clear/restore and exact incoming
MODE1 restoration. The numerical reader runs with incoming interrupt state restored.
Assumptions remain one core, serialized adapter/hook paths and maskable accesses;
no universal firmware/NMI exclusion is claimed. Corrupt bookkeeping fails closed.

Raw first/latest MODE1, ASTATX, I6/I7/I13 and active markers are bounded, asynchronous
observations. MODE1 was `0x39011cf8`; ASTATX is not a definitive task/IRQ identity.
Tail entry/exit reads (35355/35358) straddle execution and are not a coherent counter
snapshot. Zero observed tail overlap does not establish global exclusion.

Validation: 36 focused assembled diagnostic/CF checks pass, including preserved-reader
interleavings before first read, mid-reader and after kernel return. These test the
protocol against shared DM, not physical IRQ dispatch. Rebuilt objects re-decode 264 CF /
813 DSP instructions. Host checks pass 193 with 16 skipped
(`out/runs/ddr-proof/terminal-host-checks.log`); direct lint/format, regression TypeScript
and scoped path/diff checks pass. `test:changed` retains ten known path-reference failures
and the pnpm launcher failure (`out/runs/ddr-proof/terminal-changed-checks.log`); this is
not a passed routed integration gate.

**Next:** port the owned-bank lifecycle proof to DT2, then extract the reusable
SDK/runtime contract for both devices after the focused tighter-audio rerun and final review. Stop currently
playing notes on selection; queued notes that start after new READY play the new bank. The terminal bank never
reopens. Product sample APIs, Transfer UI integration, external DT2 mutation interception,
DT2 ten-second backing and hardware ownership/cache/coexistence remain separate work.

**Completed independent guard (2026-10-29): cached-voice generation fencing.** The diagnostic now
reserves a private seventh state word (28-byte state/clear size), leaving the six-word
SDK cursor unchanged. A fresh accepted note latches the nonzero bank token inside the
masked admission section; render rejects a zero or mismatched cached token in that same
section before incrementing the admitted count. Denied callbacks invalidate the token.
Forty-two focused assembled diagnostic/CF checks pass, including six new cases for a
changed global token, fresh-note capture, denied notes and armed-like cached cursors.
The DSP object rebuilds/re-decodes 832 instructions and passes its freshness check.
This checkpoint does not implement reopening, queued-event fencing or serialized writers.
Queued-note policy is selected as above; the coordinator retains final integration
verification. [Finding 84](../findings/84-sample-generation-reopening.md)
records the selected policy, implemented exact-successor/sealed-COMMIT/explicit-OPEN
protocol and whole-transaction writer requirements. All five existing coupled cases
pass with the changed consumer; no actual bank reopening is claimed.

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
  do not ask for its approval again. Source, packaging and runtime patches are integrated.
  Both DN2 1.12 and DT2 1.17 transferred-file acceptance now pass after the ColdFire byte-swap helper and explicit-label
  DK_CALL corrections: oversize rejection, clean restart, actual type 16 dispatch, unready silence/no fault,
  exact published bytes, READY, replacement refusal, audible playback and retriggering. Log:
  `out/runs/dsp-sdk-checks/sample-playback-fixed-dn2.log` and
  `out/runs/dsp-sdk-checks/sample-playback-dt2-retry.log` (each one pass, the other device filtered out).
  The bounded emulator proof is complete; production capacity, replacement and sample selection remain proposed decisions.
  Use git log for the final implementation checkpoint commit.
  No transport or core-semantics change was made for the call-frame fault. No hardware run is authorized.
  [Finding 79](../findings/79-sample-bank-proof.md) records the evidence and remaining limits.
- Capacity discovery is recorded in [finding 80](../findings/80-sampler-capacity-and-replacement.md): four snapshots
  establish baseline and proof-only module pools, not stock free RAM or general coexistence. The CF arena is
  512 KiB on both devices; shared DSP module areas are 512 KiB DT2 and 320 KiB DN2. Decoded CF source memory
  is freed after terminal transport; only the DSP bank stays pinned until restart.
  Em confirmed approximately ten-second one-shots and “Stop old notes on selection.” Ten seconds of 48 kHz
  mono PCM16 is 960,000 bytes (stereo twice), beyond either current reserved DSP module pool. The official DT2
  manual documents 400 MB project sample RAM; our module reservations are not hardware memory limits.
  [Finding 81](../findings/81-stock-sampler-dependencies.md) now records the stock-machine-led dependency map
  and corrects the earlier preview-loader lead. The executed OneShot slot/binding and active replacement
  follow-up is in finding 82; start/end/reverse and project reload remain open. Derive independently written SDK
  operations, rather than copy a complete machine.
  Proposed shared packed-PCM16 reader/cursor/region semantics use device-specific ready immutable views;
  runtime owns selection/loading/lifetime and stock downstream envelope/filter remains. Mono first is a proposal.
  DN2 already plays samples through the working voice seam/source acquisition; investigate its missing larger
  backing/delivery against DT2's dependencies, not playback feasibility from scratch. No new backend is implemented.
  Finding 83's independent DN2 1.12 raw-byte audit withdraws finding 80's external-read leads at `0x1c8ef7`
  and `0x1c8f00`: the former is not an instruction boundary and the latter is a register move. Larger backing
  remains unresolved; next trace initialization, allocation/ownership and cross-port delivery. Missing exact
  address literals do not establish absence, and module reservations do not establish hardware memory limits.
- [Finding 82](../findings/82-stock-sampler-lifetime.md) records the executed slot/lifetime discovery and exact
  image provenance. Stock OneShot selection and ten-second acquisition are established. Both the identified
  patched-image trial and the passing unpatched DT2 1.17 critical reproduction retain a progressing active voice
  address while exact backing bytes change; cleared slot metadata is not retirement. Next design requires explicit consumer retirement
  before overwrite/release, stale-note generation guards, and coverage of external slot replacement/project reset.
  No borrowed-view API is implemented or safe to expose yet; preserve the current immutable-bank proof.
- [Finding 83](../findings/83-sample-retirement-design.md) records the retirement discovery,
  corrected AudioTask roots, explicit IRQ/admission assumptions and passing terminal DN2 result.
  Both CF builders submit the prior frame before queue traversal/reconstruction; strict UI-time
  generation binding remains unresolved. The actual AudioTask entries lie inside merged database
  function extents; earlier same-parent voice/tail shorthand is corrected in finding 83.
  The private terminal bank proof closes/drains before reuse but never opens a new generation.
  Current DN2 960,000-byte backing/delivery and ten-second playback are established only for the
  isolated emulator diagnostic. Earlier source-survey drafts are historical; external DT2 mutation,
  hardware ownership/cache/coexistence, strict stale-event fencing and serialized next-generation
  writers remain separate design/proof obligations.
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
  Em authorized committing and pushing this branch; checkpoints have been pushed. Consult git log for current
  commit identity. No merge is claimed here.

## Objective and scope

Extend the device-independent module SDK with reusable DSP operations, migrate existing machines and effects,
and build a first one-shot sampler for Digitakt II OS 1.17 and Digitone II OS 1.12. Working name: SPARK (not final).
Initial musical scope: sample selection, start/end, reverse, and normal note pitch. Granular playback, slicing,
time stretch, wavetable synthesis and multisampling are future consumers, not first-release requirements.

The payoff is a reusable sample-access contract and tested kernels that make later machines cheaper to build.
Confidence is high in the discovery approach; DN2 playback is proven for the bounded bank, while larger backing
capacity, ownership and delivery remain open. If sample delivery requires a large unrelated runtime redesign, stop and present alternatives
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
- [x] Record current DT2 project loader/release/reset, slot/setup/render dependencies and DN2 hash-comparison limits
  in finding 81, including independently audited rebase/store/tap bytes. Finding 82 now captures the current
  OneShot UI; broader CF/lifecycle audit and controlled parameter/boundary semantics remain open.
- [x] Capture current stock OneShot slot binding and the unique ten-second a → b active replacement transition
  on the original DT2 1.17 image; retain exact source bytes, active/cursor state and PCM (finding 82).
- [x] Persist retirement discovery and adapter/trigger review in finding 83; the private terminal DN2 diagnostic is now implemented and passes, without a public ABI.
- [x] Read both current CF builders' prior-frame submission order, DN2 timestamp queue/reuse helpers and
  both-device same-parent static voice/tail chains, submit-guard absolute references and DN2 staging/register
  semantics; record existing diagnostic capabilities and limits in finding 83.
- [x] Prove terminal DN2 owned-bank retirement under explicit one-core, serialized adapter/hook and maskable-access assumptions, with assembled interleavings and coupled held/retriggered coverage.
- [x] Prove private DN2 reopen/new-generation publication and serialized writers under the selected policy: queued notes starting after READY use the new bank; cached old voices cannot adopt it.
- [ ] Prove DT2 retirement separately and cover external replace/unload/project reset before borrowed-view exposure. Start/end/reverse captures remain open.
- [ ] Map DT2 dependencies against DN2 1.12's working voice seam and source acquisition; identify missing larger
  backing ownership/delivery without assuming DT2's pool or LP0, or restarting playback feasibility from scratch.
- [x] Establish transferred-file format/rate and the implemented bounded float32 delivery path; the proposed
  packed-PCM16 reader is separately implemented and tested in the fixed DN2 diagnostic.
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
  unproved until an explicitly authorized device test. No new absolute CPU budget; the confirmed ten-second
  sample-length target is not yet implemented.

## Approved proof — loading and lifetime

Em approved this bounded bank API on 2026-10-29, narrower than the earlier general prepare/bind/release sketch.
Evidence and exact current-image mappings are in finding 78. Runtime implementation is integrated below;
the transferred-file bounded emulator proof passes on both current OS images. The first SDK milestone is checkpointed at `4cd29a3c`.

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

**Capacity and replacement follow-up:**

- [x] Measure no-dynamic-module and proof-only runtime capacity on both current images; persist provenance and limits.
- [x] Record the user's stop-old-notes-on-selection choice and distinguish temporary CF source from pinned DSP storage.
- [x] Record the confirmed target: approximately ten-second one-shots; stop old notes on selection.
- [ ] Prove DT2 1.17 stock DSP-pool slot ownership, external mutation interception, lifetime/reclaim and cache rules
  before borrowing stock backing. The terminal DN2 owned-bank proof does not establish stock backing ownership.
- [x] Prove bounded DN2 emulator backing/delivery for 960,000 packed PCM16 bytes and full ten-second playback,
  with exact payload, complete canaries, failure cases, actual dispatch, end cursor and early/late PCM evidence.
- [ ] Establish hardware ownership/cache/coexistence and general capacity beyond that isolated emulator range.
- [x] Record proposed retirement sequencing and source-survey/review results in finding 83; transport ACK is not a reader barrier.
- [x] Implement and prove the scoped terminal DN2 retirement contract: guarded note_on/render admission/drain, closed cached/delayed callbacks, held/retriggered starts, exact/stale ACKs, pending retries and timeout.
- [x] Prove DN2 generation reopening, sealed B skip, C playback, stale controls and serialized writer failure-stop. Concurrent queued loads are covered by host tests; the coupled case executes A → sealed B → C. Strict event-age rejection is not the selected policy.
- [ ] Establish product stop granularity and a shared public ABI after the DT2 lifecycle proof.
- [x] Prove the packed PCM16 reader and fixed resident diagnostic consumer numerically and on coupled DN2.
- [ ] Measure paired cost before a performance/adoption claim; product sample-picker scope remains open.

**Approved proof work:**

- [x] Agree declaration/record and status contracts here, then assign disjoint SDK/packer and DT2 source-adapter
  tasks to workers in separate worktrees. One worker owns shared runtime queue/lifetime integration.
- [x] Add host tests for declaration ownership/bounds, private snapshot consistency and fake-link state transitions.
- [x] Add current-image DT2 read-only filesystem bindings/wrapper and DN2 snapshot reader.
- [x] Add module-owned bank job and immutable descriptor publication, serialized with existing module/registry work.
- [x] Add a minimal proof consumer on both devices using actual transferred data; bound emulator runs and test PCM,
  silence-before-ready, oversize failure and retrigger behavior. Host tests cover allocation capacity/failure;
  emulator acceptance does not establish arbitrary module coexistence. No user sample audio committed.
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
- [x] Add the proof consumer and transferred-file emulator scenarios; execute owner checks on both devices.
- [x] DN2 1.12: run the transferred-file scenario through oversize failure, clean restart, unready dispatch/silence,
  exact READY publication, replacement refusal, audible playback/retrigger and no-fault checks.
- [x] DT2 1.17: run the same transferred-file scenario with the current-worktree daemon.

Implementation and bounded-proof checkpoint (product scope remains open):

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
  `u=1` denotes post-modify. The Python test path also used an abstract call stack: an unknown saved return
  bypassed target checking and returned through recorded call metadata. This explains the earlier false pass;
  no interpreter/native core semantics were changed.
- Approved bounded correction: `DK_CALL(fn, return_label)` now requires a caller-supplied unique label and
  emits an immediate Type 16a post-modify store of `return_label - 1`. The one-argument form is rejected. All
  repository SDK callers were migrated. Repeated/nested-call experiments proved distinct relocations with
  addend -1; neither numeric local labels nor a builtin `__COUNTER__` was usable, so no preprocessor feature
  or dependency edit was added. Artifacts: `out/runs/dsp-sdk-checks/call-label-experiment/`.
- New `test_dsp_call_frames.py` in `tests/patchkit/` executes the actual assembled bytes in Python architectural-stack
  mode and the fingerprint-checked native core, with runtime decoding, poisoned saved-return slots, repeated
  and nested calls, low-alias and L2-byte-alias stacks. It asserts actual continuation PCs, saved frame/return
  slots and exact I6/I7 restoration; old one-argument assembly must fail. The four named suites (call frames,
  generic kernels, sample kernels, fixture consumer) passed **85 tests, no skips** with an explicit current native
  library. Log: `out/runs/dsp-sdk-checks/sdk-call-frame-tests.log`.
- Actual fixture `module:check` passes **29 tests, five skipped**, and both device packs were refreshed under
  `out/regression/modules/{dn2,dt2}/sample-proof.dkm`. Each needs 33,984 DSP bytes and fits an empty device;
  this does not establish coexistence or device playback. Logs: `out/runs/dsp-sdk-checks/sdk-call-module-check.log`,
  `sdk-call-module-build.log`, `sdk-call-pack-dn2.log` and `sdk-call-pack-dt2.log` in the same directory.
  The cached fixture now decodes its return store at byte 396 as Type 16a, with an ABS32/addend -1 relocation
  to `_sp_after_linear` (parcel 201).
- Parent's corrected fast DN2 `dk:dev` first-note reproduction now passes without a DSP halt (12.38 s total):
  `out/runs/dsp-sdk-checks/sample-note-fixed-dn2.log`. This is a bounded first-note check, not full transferred
  playback acceptance by itself. The subsequent full DN2 scenario **passes** (session 7984), with one test
  passed and one DT2-filtered skip, 235.36 s total wall time and 228.197 s scenario time:
  `out/runs/dsp-sdk-checks/sample-playback-fixed-dn2.log`. It verifies uploaded 9,000-frame oversize rejection
  (-13), zero descriptor/no fault, clean restart, actual type 16 dispatch, exact-zero unready PCM/no fault,
  4,096-frame exact decoded publication with READY=3, replacement refusal (-10), audible playback/retrigger
  and no faults. These suite durations are not loading-latency or performance measurements. Passing temporary
  artifacts at `run-fMhyJt` were removed by normal test cleanup; exact ready/retrigger peaks are not retained here.
- Independent review found no concrete issues in the explicit-label macro or native return-frame regression.
  DT2 current-worktree daemon build completed in about 90 s (session 47443), logged in
  `out/runs/dsp-sdk-checks/sample-dt2-daemon-build.log`. The prepared binary is
  `out/native/target-wt/digikit-dsp-sampler/g-e7aa50f9/release/digi-emud`, used for the passing DT2 retry below.
- The first DT2 scenario retry failed its diagnostic assertion (expected type 16, read 1), while screenshots
  showed SAMPLE PROOF with START/END/REV and the DSP remained healthy. The test had read DN2's diagnostic
  layout: DT2 stores type at `_dv_g + 4` and `_dv_c` word 0 is note-on; DN2 stores type at `_dv_c` word 0.
  The scenario now resolves the device-specific symbol from its exact patch plan and records the address.
  The exact assigned-type assertion is retained; this test-only correction resolves the failure.
- DT2 1.17 retry **passes**: one test passed, one DN2-filtered skip, 94.25 s wall time and about 67 s scenario
  time (`out/runs/dsp-sdk-checks/sample-playback-dt2-retry.log`). It verifies the same assertions as DN2:
  oversize -13/zero descriptor/no fault, clean restart, actual type 16, exact-zero unready PCM and healthy DSP,
  exact READY=3 bank bytes, replacement -10, audible finite playback/retrigger and no faults. Both-device
  bounded proof acceptance is complete; these durations are not loading-latency or performance measurements.
- Focused stock OneShot investigation is now requested. Current DT2 1.17 database candidates are render
  dispatch `0x1c6b00` → `0x1c4ecf` and stretch branch `0x1c6af1` → `0x1c5576`; stock Type 16a immediate
  post-modify return stores provide a comparator. These are [D] database observations, not [V] byte audits
  or a completed sample-access/ownership trace.
- The TypeScript scenario now records `unready-state.json` (daemon status, faults and PCM summary) immediately
  after capture and calls `expectNoFault('unready proof-machine note')` before valid loading. Publication
  diagnostics also retain daemon status/faults. Formatting and TypeScript checks pass, and the full DN2 run
  above and the DT2 retry now exercise these assertions successfully.
- Partial `test:changed` checks pass Ruff, layercheck and the two selected Python files
  `tests/devtools/test_changed_tests.py` and `tests/devtools/test_verify.py`. The known baseline ten stale
  references still block repository pathcheck. Host checks were blocked by pnpm 12's automatic install attempt
  and unavailable offline cache; no dependency changes were made. The later owner host related-test command
  exits zero with no selected host tests (`out/runs/dsp-sdk-checks/sample-host-related.log`); oxlint passes
  using existing Node binaries. Neither is a full gate pass. Changed-check log:
  `out/runs/dsp-sdk-checks/sample-test-changed.log`.
- The subsequent SDK-call changed check passed Ruff and layercheck but reported eleven path references.
  The added temporary-test path in `packages/patchkit/src/module.rs` is now constructed with separate `join`
  calls (same resulting path, no production change). Repository pathcheck confirms the remaining ten known
  baseline references in five unchanged files; log: `out/runs/dsp-sdk-checks/sdk-call-pathcheck-after-test-path.log`.
  Scoped checks of that source file and these documents pass. The full routed gate has not run.
- Next: use the executed replacement observation in finding 82 to design and prove explicit consumer retirement
  before overwrite/release, including external slot replacement/reset and stale queued notes. Preserve the existing
  immutable-bank proof and DN2 playback seam; larger DN2 backing/delivery remains a separate missing contract. The confirmed target is approximately ten-second
  one-shots, stopping old notes on selection. Backend capacity/lifetime and sample-picker work remain unimplemented. The current limit is 8,192 mono frames at 48 kHz
  (about 171 ms), with session-pinned immutable source storage and fixed proof paths. Continue stock-reader
  investigation separately; no stock-quality equivalence, coexistence, performance or hardware claim follows.
- A standalone native-boot diagnostic halted after six instructions, before reaching the target. It is unusable
  for this investigation and does not establish proof behavior.
- Runtime artifact builds require rustup Cargo first in PATH (`/Users/em/.cargo/bin`), not Homebrew Cargo.
- Parent shares only input symlinks; Node workspace package links resolve to this worktree, dependency cache is shared.
  Ready inputs' firmware hashes match current DT2 1.17 / DN2 1.12, despite an old DN2 image label in provenance.
- Local check logs: `out/runs/dsp-sdk-checks/`. The parent completed the single `runtime/sample-playback.test.ts`
  scenario on DN2 first and DT2 last, with bounded sessions and shared benchlock.

This approved continuation completes the bounded sample API, proof fixture and both-device emulator playback
acceptance. It does not resolve general conditional transfer cleanup or establish hardware behavior, production
capacity, arbitrary module coexistence, stock quality or performance. Use git log for the final checkpoint commit.

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


### 2026-10-29 — packed PCM16 checkpoint and isolated transport preflight

Em's “Go for it” approves the bounded ten-second diagnostic and numerical prerequisites;
a product sample/resource API remains outside that approval. Packed PCM16 reader checkpoint
`6ece2ae8` has 96 passing numerical/assembled tests and completed parent review. The float
reader remains unchanged; the new reader uses packed signed16 containing words and the
existing exact split cursor.

The next isolated transport stage is `core/dsp-ddr-proof`, selected only by explicit
`apply --modules core/dsp-ddr-proof`, conflicting with `core/dsp-link`. Its full-frame mailbox protocol is
[documented with the module](../../modules/core/adapters/dsp-ddr-proof/README.md). It is a
quiet host-driven CF→SPI→DSP diagnostic, without a runtime sample ABI, event-drain claim,
live-audio loading or production machine. The candidate emulator range holds exactly
960,000 packed PCM16 bytes, with complete 4 KiB canary bands. Hardware cache/coexistence
remains open. [Finding 83](../findings/83-sample-retirement-design.md) records the current-image
command-4 rejection audit, corrected DAG2 register display interpretation and exact eight-byte
ACK owner region. These observations require no core semantic change or regeneration.

The CF/DSP objects cross-build and instruction re-decoding passes. The initial focused unit
run passed 12 checks using actual assembled DSP and a prebuilt CF CPU harness. The new
TypeScript coupled acceptance test covers the complete payload, canary bands, duplicate and
stale-token handling, out-of-range rejection and partial checksum failure, with fresh boots
for sticky errors. It is prepared for parent review; no coupled transport or ten-second
playback completion is claimed here. The external-source report remains excluded pending
citation clarification.


Preflight launch blocker: patch sets are embedded by `packages/patchkit/src/devdata.rs`.
The existing prebuilt patchkit reports only `device`/`emulator` and refuses `ddr-proof`;
the prebuilt daemon likewise cannot discover the new set from the edited TOML. No daemon
build or coupled run was started. Parent decision is required between rebuilding binaries
with this set or using the existing explicit patched-image loading path. Focused unit
checks still pass (12 tests); lint/format checks and document pathcheck pass. Default
runtime test collection safely skips the four diagnostic scenarios without both explicitly
supplied preflight binary environment variables. Artifacts are retained under `out/runs/ddr-proof/`.


The parent resolved the embedded-set blocker by choosing the existing explicit-image path:
remove the proposed device patch-set addition, use the prebuilt CLI's
`apply --modules core/dsp-ddr-proof`, then load the patched image and stock base hash through
`firmware.load` on a stock `withSession` session. The test checks the returned load hash and
live injected/base identity against the exact plan, and retains stock identity, patched load,
plan and status artifacts. No daemon/native rebuild is needed. The explicit-module image
preflight succeeds (DN2 1.12, proof plus `core/expansion` only); the coupled run remains the
parent's next step.


### 2026-10-29 — transport passed; fixed resident playback preflight

The parent completed the isolated DN2 1.12 coupled transport: all 960,000 payload
bytes matched, both complete canary bands (8,192 bytes) matched, and duplicate DATA
was accepted without advancing the word count. Separate fresh sessions passed
out-of-range rejection, partial checksum failure and stale-token/watchdog cases.
Valid-run artifacts: `out/runs/ddr-proof/84d1a9b9-4bca-463e-8572-1ac1414c257d`;
logs: `out/runs/ddr-proof/full-transport.log` and `out/runs/ddr-proof/failure-cases.log`.
The valid source SHA-256 was `296a543415ca5c7e6e32a40f0692d18f6d8a46fe8841ba6c245e4720c4073162`,
patched image `bea2909e36322ad082809fdcd46e45d79b809e295b0baf26f48734bab9008e75`.
Reported 24.2-second valid-test wall time is not a transport throughput measurement.

The approved next stage places the SDK PCM16 consumer directly in the same core
adapter. A scoped diagnostic-only DN2 type 11 reservation
`core/dsp-ddr-proof:ddr_proof` leaves all existing allocations and dynamic range
16..23 unchanged. No default patch-set inclusion or public sample ABI is added.
A declaration in a framework module is supported by existing typed lowering.
The actual preflight closure is `core/dsp-voice`, `core/expansion`, `core/machines`,
`core/params`, `core/dsp-ddr-proof`; `core/dsp-link` remains excluded.

The six-word consumer admits READY at note-on, plays the fixed immutable 480,000-frame
view forward or reverse, preserves the machine-adapter return/stack contract, and
returns DONE at the exact end. Conservative instruction budgets are 2400/2600;
these are not measured hardware cycles. Seventeen focused transport/consumer tests
pass at preflight. The coupled valid test now repeats full transport with a windowed
200 Hz PCM16 triangle, resolves its actual registry binding and UI list position,
then captures at least 11.5 seconds with one held note. It checks early/late/end
cursor, exact completion, continued normal frames, finite PCM and early/late periodic
audible content. WAV, state snapshots, UI and binding/image/descriptor evidence are
retained. Stock output workspace reuse or FX tails are not confused with the unit
check that an extra reader call after DONE emits zeros and reads no source.

The playback coupled run remains pending parent execution. Hardware cache/coexistence,
resource retirement/replacement, sample Transfer UI and user selection APIs remain open.


### 2026-10-29 — first playback evidence and observer correction

The first coupled playback attempt retained artifacts at
`out/runs/ddr-proof/5982a581-d387-491f-9d5b-5be402f1b872` and log
`out/runs/ddr-proof/transport-playback.log`. All assertions through exact cursor
`[0,480000,480000,0,65536,1]`, DONE and 15,000 renders passed. The normal-frame observer
then failed: it remained 21,478 across 257 snapshots while rendering advanced. Independent
WAV extraction (`playback-audio-audit.json`) found 552,736 stereo frames, all finite,
peak 0.02065205, early RMS 0.009347532 and late RMS 0.009347522; both windows have
lag-240 correlation approximately 1 and lag-120 approximately -1. Thus the captured
known 200 Hz content satisfies the planned early/late audio checks, although the test
verdict failed before reaching those assertions.

The parent raw-byte audit resolves the observer fault: current DN2 instruction `0x1c9da8`
is Type3b unsigned SHORT16 (`l=1,x=0,w=0`); its display omitted the load width. The stock
bounds check therefore applies to the low command halfword. Current CF builder
`0x400274b4` stores command 3 at `0x80005e60`; the next halfword at `0x80005e62` is pitch.
The diagnostic's 32-bit observer incorrectly compared both halves with 3. Only the observer
now extracts bits 0..15. The exact complete-word-4 proof admission gate is unchanged.
A focused assembled regression covers nonzero-pitch command 3 and rejects a high-half
forged command 4. All 18 focused tests pass; the rebuilt DSP object re-decodes 501
instructions. No core, decoder or formatter edit/regeneration is included.

The TypeScript test now writes state/audio/result evidence before state/counter verdicts.
The strict normal-frame progression assertion remains for the measured workload; a static
one-hook-call-per-render guarantee is not asserted. Parent rerun remains pending at this
checkpoint. Direct TS lint/format and Python checks pass; the parent's broader routed
check retains the known baseline path references and pnpm launcher constraint, while its
direct host coverage passed after rerunning socket tests outside the sandbox restriction.


### Final emulator milestone validation (2026-10-29)

The parent rerun **passed** the single full transport-plus-playback scenario after the
observer-only correction. Artifacts are retained at
`out/runs/ddr-proof/11723539-7f66-4fce-b663-45cd9ea3d3da`; log
`out/runs/ddr-proof/transport-playback-fixed.log`. Patched-image SHA-256:
`e079d4fadaa8d26512b714d290c23bd9421c33f6c858c8cac6b60294d0f9e933`.
The exact 960,000-byte payload and both complete canary bands pass before triggering.
Actual machine type 11 dispatches one note; cursor ends at 480000, DONE=1 and render
count=15000. The corrected normal-command count advances 21798→39082. The tap reports
11.514 seconds and the WAV contains 552,992 stereo frames, all finite. Early/late RMS
is 0.009347532/0.009347522; both expected 200 Hz windows retain lag-240 correlation
approximately +1 and lag-120 approximately -1. All strict playback assertions pass.

Eighteen focused tests pass, including adapter ABI, unready/after-DONE zero-source-read
checks, forward/reverse end/retrigger behavior and the nonzero-pitch observer regression.
The parent also passed the three transport failure scenarios before playback integration;
the wire/protocol semantics are unchanged by the observer correction. Generation and
cross-build/re-decoding pass; scoped Python/TS lint/format, pathcheck and diff checks pass.
The routed changed check passes Python lint/format and layercheck but retains ten baseline
path-reference failures; its host/type phases were blocked by the existing pnpm 12 shared
node_modules launcher constraint. Direct host Vitest passed 187 tests with 16 skips;
the six socket tests blocked by sandbox `listen` EPERM all passed on the parent's permitted
retry. Logs: `out/runs/ddr-proof/changed-checks.log`, `host-checks.log`, `host-sockets-check.log`.
This is an emulator capacity/playback milestone, without hardware timing, coexistence,
resource replacement/retirement or Transfer UI/user-selection API claims. Parent owns
checkpoint, push and subsequent integration; no further coupled run is needed here.
