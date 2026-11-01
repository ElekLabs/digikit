# Test architecture: four levels, native module tests, TypeScript (plan, 2026-10-21)

One PR per item, branch `<area>/<topic>`. Related plans (outside the repo, in ~/src/digi):
TEST-SELECTION-PLAN-2026-10-20.md (routing; this plan replaces its Phases 3 and 4 and Phase 2's representative set and
`module:test`), WORKBENCH-AND-MODULES-PLAN-2026-10-19.md (P3d FX contract carries T2).

## Status at end of session
- 2026-10-21: plan written. No code changed. Order: TEST-SELECTION Phase 2 PR 1 (gate routing) first, then the modules
  plan; T1 when module work needs it, T2 with P3d.

## Goal

Each test runs at the cheapest level that answers its question. Module tests, including sound, run in seconds on the
native SHARC engine. Tests that do not listen to audio do not run the DSP. Long term, tests and non-RE tools are
TypeScript (vitest); Python stays for reverse engineering (tools/sharc_dev, tools/coldfire, Ghidra, firmware analysis).

## Decisions (Em, 2026-10-21)

1. Local only. No CI for now.
2. Four test levels (below). Split unit and integration tests.
3. TypeScript and vitest for tests and non-RE tools, long term. Migrate by replacement, not by a bulk port: new levels
   are written in TS; a Python test is deleted when its TS replacement checks the same oracle. No new Python test code
   outside the RE tooling.
4. Little change for the sake of tests: do this work where module or plan work needs it.
5. The Python SHARC core stays the reference for the instruction semantics and generates the Rust core.
6. PCM pins (`tests/scenarios/*.json`, module pins) change only with Em's approval.

## The four levels

| Level | Question | Runs on | Speed |
|---|---|---|---|
| unit | Is the logic right (packing, manifests, routing, tables)? | Rust and TS (Python until moved), no emulator | ms to s |
| module | Is the machine or FX right, and does it sound right? | the module routine alone, through its contract, on the native SHARC engine | s |
| device-cf | Does the firmware handle it (menus, parameters, save and load, upload, type numbers, memory audit)? | ColdFire emulator, DSP stubbed | about 1.1x real time |
| device-coupled | Does it sound right inside the device (stock filter, envelope, FX, master; the hook glue)? | both CPUs | 0.21-0.24x real time |

Each module type keeps one coupled test per device. Everything else moves down a level where it can.

## Evidence (2026-10-21)

- Replay of the last 25 merged PRs (static, verify's routing against the import graph): 12 touched verify.py or
  verify_route.py and ran everything (about 1,350 file-seconds); 5 Rust or `core/*` PRs expand to tests/patchkit and
  tests/digikit (860-1,335 file-seconds even with the import graph); 8 Python-only PRs drop from 100-1,270 to 3-92.
- The Python SHARC core runs every isolated routine test (MachineRig, dsp_voice_rig, pk_sharc): test_pluck 322 s,
  test_dsp_voice 99, test_patchkit (part) 73, test_machine_conformance 65, test_sine 60, the pluck tune and range files 67,
  others about 15 (medians). About 700 of the 1,350 s of a full python-tests run, and every `module:check`.
- The smoke step costs about 380 s on 4 cores on every non-docs change (17 cells).
- Coupled runs 0.21-0.24 emulated s per wall s; ColdFire-only 1.1 (finding 39). `digi-emud --dsp zero` exists; its
  cost and whether the ColdFire side waits on DSP replies are not measured.

## Items

### T1. Native call harness for module routines (0.5-1 day; payoff high, confidence medium)
- [ ] Scout: can the Rust SHARC engine run one routine outside the coupled loop? Does it have a store hook (needed by
      the P4a write-set check)?
- [ ] A Rust CLI or daemon with a plain request format: load the module objects, set the registers, call `note_on` and
      `render` for N blocks, return the output, cycle counts and recorded stores. Language neutral: Python calls it now,
      TS later. No Python dependency added.
- [ ] MachineRig and dsp_voice_rig use it; the tests stay unchanged. A cross-check test runs a few cases on both engines
      and compares the outputs.
- [ ] Before/after: the files listed in Evidence.

### T2. Module test kit (with P3d; 1-2 days; confidence medium-high)
- [ ] TS/vitest harness over T1, for machines (`render`, `note_on`, `desc`) and FX (`dk_fx_process`).
- [ ] Sound checks a module declares in its `tests/*.toml`: pitch against note, decay time, RMS, DC, parameter response
      (direction per parameter), a pinned PCM hash.
- [ ] `module:test NAME`: build check, conformance, sound checks, the smoke cells and quick scenarios that name it.
- [ ] Retire the Python conformance suite and the pluck sweeps when the TS kit checks the same pins.

### T3. DSP stub for tests that do not listen (1 hour to measure, then about 1 day)
- [ ] Measure `--dsp zero`: cost, and whether the ColdFire side needs DSP replies that a stub must answer. Stop if it does
      not pay.
- [ ] Move to ColdFire-only: menu, registry, upload, round trip, safe mode, cut, the table half of equiv. Only PCM compares
      stay coupled.

### T4. Device levels in TS (several days; by replacement)
- [ ] vitest projects `unit`, `module`, `device-cf`, `device-coupled`; device tests drive `digi-emud` through
      packages/control-client.
- [ ] Each landed TS level deletes its Python counterpart (pk_harness cells, digikit scenario runners).

### T5. Smoke and scenario selection by module (0.5-1 day)
- [ ] Filter the smoke matrix and the quick scenarios to the cells that contain a changed module.
- [ ] `core/*` and runtime changes run a representative set (one cell and one quick scenario per module type per
      device); the full sets run on request before a release.

### T6. verify.py (last; about 1 day)
- [ ] Python routing reduced to what vitest's related-test selection does not cover; keep the CPU budget.

## Rules

- One PR per item; design agreed with Em in chat before code.
- Two test systems run side by side only per level, and only until that level's replacement passes: one Python file
  deleted per TS file that replaces it.
- Each item reports before/after (files, file-seconds, gate wall time) against the recorded runs.


## FX consolidation design (2026-11-01)

Status: Em selected design work and proposed reusing a daemon through the control socket with
setup/teardown fixtures. The first lifecycle/PCM pilot is now implemented under subsequent approval;
its earlier fixture proofs passed. The safety increment below records the current PR #242 state. This design supersedes the historical one-PR-per-item rule: keep the reusable fixture
and compatible FX coverage in one coherent change, with explicit checkpoints rather than a bulk
seven-family port. No legacy deletion, new RPC or benchmark is authorized by this checkpoint.

### Fixture lifecycle and isolation

Extend the existing Session lifecycle with Vitest `beforeAll`, `beforeEach`, `afterEach` and
`afterAll` hooks; do not introduce a parallel test framework. `beforeAll` owns one daemon per device
per test file/worker, reused by serial cases. Never attach to another agent's or the user's daemon.
A Session baseline-reload method used by `beforeEach` updates its private load result, card and event
state; a raw-client load with stale Session metadata is insufficient. Reload through the existing
control-socket `firmware.load` operation (also used by the workbench): `replace: true`, `force: true`, explicit
pristine card/image/runtime patch set and `start: auto`. This rebuilds firmware state in the same
process; firmware boot still occurs. The stock-only
[Session restart wrapper](../../packages/test-support/src/session.ts) is not a limitation of the
underlying load operation. In implementation, extend the existing
[FirmwareLoadParams type](../../packages/control-client/src/types.ts) with its missing `replace`
field rather than inventing a reset RPC.

Use a fixture-owned pristine card directory, not just a pristine base file:
[controller card loading](../../packages/emulator/control/src/controller.rs) applies a saved sibling
overlay. There must be no inherited `overlay.bin`; keep mirror/card synchronization off. Reuse the
fixture-owned pristine base without copying a huge card per case only when read-only backing and
disabled synchronization are guaranteed. Specify `auto_no` consistently on every load. Recreate DkSession and EmuUsbTransport per case, fence observations
by the new epoch, and clear collected events. Actor-level options survive runtime replacement:
explicitly set CPU exception delivery (normally false), watchdog stall policy and mirror settings;
reapply guards/watchdog configuration rather than assuming load resets them.

A load is rejected while a controller job or patch job is active. Settle or cancel owned work within
a deadline before loading; replacement does not promise to cancel every job. Failed or dirty reset
fails setup, preserves diagnostics and discards the owned daemon; never silently continue as a clean
case. `afterEach` collects the case's guard/fault/card/PCM closing evidence before discarding state,
then stops capture, pauses the runtime and cancels pending protocol work. Input helpers own release
in `finally`; no release-all RPC is assumed. `afterAll` closes the owned daemon.

Within a persistence scenario, use protocol restart to preserve guest writes and RAM according to
that scenario's contract. An independent-case clean load must not replace that restart. The first
batch introduces no generic reset RPC, global daemon pool, result cache or snapshot restoration.
CF-only snapshots do not prove matched DSP state, and the existing state-load operation requires
instruction count zero; neither establishes the desired coupled reset contract.

### Static scope and coverage groups

**[D]** Historical counts before the approved gain-leg deletion: these describe rig entries and explicit restarts, not
actual uncached boots or elapsed time. Quick tier has 14 scenario/device pairs; full tier has 15.
The earlier estimate of 80 boots per device reduced to nine is unsupported and is withdrawn.

| Family / source anchor | Quick DT2 rig entries | Quick DN2 rig entries | Explicit restarts per device |
| --- | ---: | ---: | ---: |
| [fx](../../tools/patchkit/pk_digikit_fx.py) line 312 | 4 | 4 | 1 |
| [fxsend](../../tools/patchkit/pk_digikit_fxsend.py) line 192 | 5 | 8 | 0 |
| [fxroute](../../tools/patchkit/pk_digikit_fxroute.py) line 558 | 11 | 11 | 0 |
| [fxroutes](../../tools/patchkit/pk_digikit_fxroutes.py) line 637 | 8 | 8 | 2 |
| [fxpage](../../tools/patchkit/pk_digikit_fxpage.py) line 513 | 3 | 3 | 1 |
| [fxpersist](../../tools/patchkit/pk_digikit_fxpersist.py) line 275 | 1 | 1 | 0 |
| [fxshift](../../tools/patchkit/pk_digikit_fxshift.py) line 499 | 4 | 4 | 0 |
| Total | 36 | 39 | 4 |

Quick totals are therefore 40 DT2 and 43 DN2 static operations. Full tier adds one DT2 persistence
rig entry and two explicit restarts, three additional operations. These counts make no savings
promise. Use three coverage groups for the design: chain plus page baseline; send plus routes;
persistence plus frequency shifter. These are coverage boundaries, not a proven nine-boot schedule.

Start with the reusable fixture, one existing representative gain-FX lifecycle/PCM flow and its
cleanup assertion on DT2 OS 1.17 and DN2 OS 1.12; no large new test matrix is needed for this pilot.
Only then fold compatible fx/fxpage checks into the same coherent PR, using an
assertion ledger that maps each old assertion, device, image/seed state and PCM/PIX oracle to its
replacement. Retain special no-hook/fxpages-off images, capacity/overflow and no-module cases,
deselect/restart checks and each case's closing checks. Do not delete a family until every unique
assertion is mapped and its port passes. Contract-feature coverage is a safety aid, not proof that
these scenario facts are interchangeable.

### Proof checkpoint and unresolved implementation choices

The focused isolation proof dirties a case, clean-loads in the same daemon, and checks expected
registry, card/settings, CF/DSP baseline, input state and a new epoch. Compare that reset baseline
with a fresh-process reference once per device using focused cases. A separate persistence case
must prove that writes survive protocol restart. Keep the closing guard/fault/card/PCM checks;
there is no broad sweep for this experiment. Stop the first slice if useful process reuse or exact
isolation cannot be established. Process-reuse payoff is unmeasured; use the existing bounded
measurement workflow only if a later decision needs it, without making a performance claim now.

Preboot seeding must reuse the existing Rust store/settings codecs. Inspect the
[native card builder](../../packages/emulator/control/src/cards.rs) and available APIs before proposing
a narrow operation; do not duplicate the codec in TypeScript. Postboot upload cannot substitute for
assertions about a preboot-seeded state. Exact seed transport and fixture implementation remain open.

PCM capture is already available through generic client calls to `pcm.tap`; `--no-audio` disables
playback only, not capture. The existing
[FX support](../../tests/regression/scenarios/fx-support.ts) uses a Python renderer/NumPy path.
Choose minimal analyzer reuse for live captures or a small TypeScript comparison only after inspecting
the needed oracles; do not port DSP analysis wholesale or invent protocol APIs. Seed/analyzer choices
and the complete actor-state reset checklist are bounded checks during the first implementation
slice; resolve them before relying on the affected assertion, rather than blocking every fixture step.


### First implementation checkpoint (2026-11-01)

**[D]** Historical first fixture proof, before the safety increment: both device proofs and fast preflight passed. the [gain-FX pilot](../../tests/regression/runtime/fx-reuse.test.ts)
uses [Session baseline reload](../../packages/test-support/src/session.ts) and the existing Vitest
[regression fixture](../../tests/regression/support.ts). No legacy family is deleted or declared equivalent.
One serial scenario per device covers dry → gain → protocol deselection/restart → pristine dry,
plus one sequential fresh-process dry reference while the kept daemon is paused.

Clean reload uses the existing firmware-load replacement path in the same owned process, updates
Session metadata, verifies a newer epoch and creates fresh protocol/USB receive state. Firmware
boot still occurs. The deselection stage instead uses protocol restart, retaining guest writes;
it must not be substituted with pristine reload. Reset/cleanup failures discard only the owned
daemon and retain its diagnostics. Native plusdrive builds the preboot selections from prepared
DKMs in owned overlay-free card directories, with mirroring/synchronization disabled. No TypeScript
store codec, new reset/seed RPC, snapshot restore or daemon pool was added.

At that first checkpoint, the following assertions passed the focused DT2 and DN2 proofs:

| Area | Pilot asserts | Retained legacy facts / limits |
| --- | --- | --- |
| Boot and registry | Normal boot, count/loaded/results/UI; assignment identity; reported module types/contracts, FX slots and free/used rows | No-hook/plain, full-capacity/overflow, no-module and fxpage variants remain |
| Gain chain | Enabled/default parameters, descriptor/state within the module DSP range, adapter fault fields and one-call-per-block state progress | Inferred resource needs, exact relocated DSP bytes and exact row pointers remain |
| Reset isolation | New epoch, old protocol invalidation, restored normalized module/FX capacity/input/settings/DSP semantics and fresh-process comparison | Not a whole-RAM/tick digest or deliberate dirty-settings isolation proof |
| PCM | Dry × 0.5 within 2 ULP, RMS within 0.001 dB and onset shift at most 128 samples; deselected/restored/reference dry bit-identical from onset | Existing PCM/PIX oracles and all other FX family scenarios remain |
| Closing evidence | No runtime fault/DSP halt/nonfinite PCM, audible sine, targeted post-load boot-identity guard and exported card overlay | Launch-time expansion write-watch, flush, card-sector allow policy and DT2 selected-track type/retry proof remain |

Captures pause execution, choose current DSP frame + 300 after panel setup (the legacy restarted-note
strategy), step 44,000 CF instructions beyond the boundary and hold TRIG 1 for 1,500 DSP frames.
Key-up runs in `finally`. Input idle is observed while free before pausing. The existing
[PCM comparator](../../tools/patchkit/hook_ab.py) now exposes onset alignment and JSON metrics; the
legacy scaled-comparison alias uses the same function. No WAV codec or DSP analyzer was duplicated.
Absent pristine settings are null only for the structured `E_SET_NOKEY` status; other errors fail.

Prerequisites are prepared separately, not built by the test. For the default module directory:

```sh
uv run python -m tools.firmware.plusdrive_check --build-only
mkdir -p out/regression/modules/dt2 out/regression/modules/dn2
for device in dt2 dn2; do
  uv run python -m tools.patchkit.patchkit pack test/sine --device "$device" -o "out/regression/modules/$device/sine.dkm"
  uv run python -m tools.patchkit.patchkit pack test/dyn-fx-gain --device "$device" -o "out/regression/modules/$device/dyn-fx-gain.dkm"
done
```

The test uses the existing fixed native plusdrive binary and preflighted daemon/ready inputs.
`DIGI_RUNTIME_MODULES` may name an already prepared module directory. Gate ownership maps both
fixtures to the pilot, and both explain/execution select plusdrive preparation for an FX-only run.

Focused checks passed: 50 host cases plus the one canonical-path assertion correction retry (51
across the runs); 10 PCM synthetic cases; seven fault/settings cases; 41 routing cases plus four
new prerequisite/explain cases. Affected package/regression types passed. Independent foundation
and pilot reviews completed with their reported blockers corrected. Both device proofs passed
on their first runs, each with one selected case and the other device's case skipped:

| Device | Invocation | Case | Local log |
| --- | ---: | ---: | --- |
| DT2 OS 1.17 | 89.66 s | 73.23 s | `/private/tmp/fx-reuse-dt2.log` |
| DN2 OS 1.12 | 62.06 s | 53.506 s | `/private/tmp/fx-reuse-dn2.log` |

These runs include the fresh reference, gain PCM, protocol deselection and clean restore. They are
functional evidence, not a speedup benchmark, complete migration or broad-gate acceptance. Legacy
coverage remains retained. Final `mise run test:changed` passed: 612 Python tests, 301 host passed
and one skipped, plus lint, format, types, path, layers and generated checks. Log:
`/private/tmp/fx-reuse-preflight.log`. Final independent static review found no issues after the
reported blockers were corrected. The gate plan was explained only; the broad gate was not run.

At that checkpoint, exact inferred needs, relocated DSP bytes/row pointers, selected-track type
and launch-time write-watch/flush/card-sector policy were not established as equivalent. No legacy
deletion is authorized by this evidence. Next, close the assertion ledger before parallel family
migrations. The coordinator retains integration, remaining acceptance and publication decisions.


### Safety increment in PR #242 (2026-11-01)

**[D]** Both revised device cases and required final fast preflight passed. This is the
same PR #242, with no legacy `one()` deletion and no merge-acceptance claim. Eleven new Python
synthetic cases, 57 affected mocked host cases, affected types and scoped lint passed.

The existing Python runtime rig and the TypeScript test now share the offline watch-range calculation,
including its DMA-buffer holes and allowed writer-PC range. The patch plan is prepared before spawn,
then validated against the loaded image after pristine baseline load. Existing write-watch/flush
instrumentation is configured in the daemon's environment from startup, with CACR activation and
per-emulator sequence output. After the owned daemon closes, the check requires contiguous watch
logs matched by complete summaries, at least the observed epochs, zero write events and zero flush
hits. Every exported overlay is parsed by the existing Python parser: store writes are limited to
the two selection sectors, and writes below sector 0x50000 are limited to those same sectors.
Missing/truncated summaries or malformed overlays fail rather than becoming missing evidence.

The gain oracle reuses the independent Python DSP parser/relocator and the existing needs wrapper.
It now compares the exact relocated DSP image, exact first 12 chain-row words and MODULES/FX-slot
resource figures against the inferred expectations. The row defaults, enabled/was/disabled/fault
fields are checked again after capture, preserving the post-note assertion rather than setup alone.
No new emulator instrumentation, RPC, runner, store codec or relocation implementation was added.

The earlier fresh-process reference passed on both devices as a one-time fixture-equivalence proof;
it is historical evidence, removed from default acceptance without an optional hidden test. The
permanent case remains initial dry → gain → protocol deselection/restart → pristine dry restore.
Reuse now skips Session's redundant SDK patched-image load; the existing CLI stock startup still
occurs. Clean baseline replacement and guest-write-preserving protocol restart remain distinct.

| Revised proof | Invocation | Case | Watch/overlay result | Local log |
| --- | ---: | ---: | --- | --- |
| DT2 OS 1.17 | 63.62 s | 60.64 s | 8 emulator summaries, zero write/flush hits, 4 valid overlays | `/private/tmp/fx-safety-dt2.log` |
| DN2 OS 1.12 | 50.16 s | 47.441 s | 8 emulator summaries, zero write/flush hits, 4 valid overlays | `/private/tmp/fx-safety-dn2.log` |

Both passed on the first revised run. These durations describe correctness runs, not a paired
benchmark or measured speedup. At this safety checkpoint, the selected-track-type decision was
still pending and legacy `one()` remained. The approved replacement below resolves that boundary;
plain/full/fxpage and other family coverage remain. The coordinator owns
acceptance and publication; the broad merge gate remains unrun.

Required fast preflight passed (`/private/tmp/fx-safety-preflight.log`): 612 audited Python routing
tests and 75 host cases (four test-support, 71 regression), plus lint, format, path, layers, generated
checks and all workspace TypeScript types. Mypy was skipped because no changed files were in its
typed set. The separate 11 new Python synthetic and 57 focused mocked-host passes above remain
iteration evidence, not additional unique cases to add to these totals. This is a verified safety
checkpoint in draft PR #242, before the approved gain-leg replacement below.


### Approved gain-leg replacement (2026-11-01)

Em explicitly accepted the FX-behavior contract: the TypeScript gain/deselect sequence proves behavior
on audible input, without a direct selected-track-type assertion. The removed `one()` leg previously
made DT2 type observations for both its wet note and its post-restart dry note. Those observations
are intentionally not reproduced; the independent legacy dry/full legs retain their DT2 selected-sine
checks. This is a bounded coverage decision, not a claim that PCM identifies the selected machine.

Only the nested Python `one()` leg, its parallel entry and its gain/back PCM comparisons are removed.
The remaining plain/dry/full functions, plain-versus-dry and full-chain PCM, overflow/fit rules and
common launch-time checks are unchanged. No scenario TOML, wrapper file or other FX family is deleted.
The replacement owner is `mise run test:runtime -- fx-reuse`; `test:fx` / `test:fx:dt2` own stock-FX
suites and are not the replacement owner.

Runtime ownership also follows the consumed CLI helpers: direct changes to the existing machine
oracle or arena-safety module select the gain case; their import closure, including the function-local
needs wrapper, retains runtime coverage through the existing dependency graph. No synthetic
legacy-runner-to-TypeScript edge was added. The three focused routing cases passed; coordinator
fast preflight of this deletion follow-up passed (final checkpoint below).

| Removed `one()` assertion | Replacement / accepted boundary |
| --- | --- |
| Boot results, safe/loaded state, module names/contracts/rows and inferred DSP range/FX slot | Verified TypeScript `inspect` plus the existing independent needs wrapper |
| Relocated DSP image, exact first 12 chain-row words/defaults and free rows | Independent Python parser/relocator JSON oracle, compared with live DSP bytes/rows |
| Post-note enabled/was/disabled/fault/parameter fields and state call counter | Post-capture row checks and the unchanged >1000 / 90–110 call-count assertions |
| Gain ×0.5 PCM | Existing comparator, at most 2 ULP, RMS tolerance 0.001 dB, onset shift at most 128 samples |
| SELECT/restart, dry-only registry/boot/free rows/capacity and restored dry PCM | Protocol deselection/restart in the TypeScript sequence; bit-identical dry PCM from onset |
| Lifetime write/flush and card-sector policy | Sequenced startup-to-close summaries and every exported overlay, proven on both devices in the safety increment |
| Direct DT2 selected-type observations inside this leg | Explicitly accepted narrower audible-input FX contract; legacy dry/full retain direct type coverage |

Historical static counts above remain the pre-deletion baseline. This deletion subtracts one legacy
rig entry and one explicit restart per device: the `fx` family is now three rig entries and zero
restarts per device. Across the seven legacy quick families, remaining counts are 35 DT2 / 38 DN2
rig entries plus three restarts each (38 / 41 static operations). Full tier still adds one DT2 rig
entry and two restarts; scenario/device pair counts remain 14 quick and 15 full. These are remaining
legacy source counts, excluding the TypeScript case, not uncached boots or measured speedup.

The verified TypeScript execution is unchanged by this deletion-only follow-up; its audible-input
assertion text and selection comment now state the accepted contract. Earlier safety/device results
remain evidence for that unchanged execution. The follow-up fast preflight passed; the coordinator
owns the remaining acceptance decision and the broad gate remains unrun. Other FX families remain unmodified; subsequent migrations will use the completed assertion mapping and fixture.


Deletion follow-up fast preflight passed (`/private/tmp/fx-gain-deletion-preflight.log`): 345 Python
cases (57 changed-tests and 288 verify), 71 regression-host cases, Ruff, mypy, path, layers,
generated checks, oxlint, Prettier and all workspace types. No device rerun was needed: both-device
safety proof at `b963e257` covers unchanged TS execution; only its wording/comment changed afterward.

The gate was explained against the actual PR base `origin/main` at `0feb02cb`: host/web, three stock
files, 11 runtime files, three module-load files, 49 Python files, smoke and an implicit daemon build.
`/private/tmp/fx-gain-merge-explain-origin-main.txt` supersedes the local-main explanation
(`/private/tmp/fx-gain-merge-explain.txt`): local main at `365b45b` was stale and overselected already
merged changes. The broad gate has not run, and no focused-acceptance exception for PR #242 exists.
The next decision is focused acceptance versus the full routed gate before merge; this checkpoint
is not merge acceptance. Historical proof/count tables and the bounded type-coverage decision stand.
