# T1 test architecture: tests follow layers (plan)

Roadmap item T1 in [sdk-runtime-roadmap.md](sdk-runtime-roadmap.md). It builds on [test-speed.md](test-speed.md) (items 7, 16 to 18) and is separate from the older level plan in [test-architecture.md](test-architecture.md).

Layers and fakes now exist (runtime layers in [ARCHITECTURE.md](../ARCHITECTURE.md); `FakeCore` and `FakeService` from R2c; `FakePlatform` from R3). Test selection can follow them.

## Goal

- A change runs only the tests that its code can affect.
- Emulator runs are reserved for what needs them.
- Confidence stays at least as high as today. `test:full` per batch is the safety net.

## Principles

- (a) mise tasks stay the entry points. Nx does selection and caching.
- (b) Every Nx project has build targets and test targets. Build targets are cached and declare their outputs. Test targets depend on the build and key on its outputs, not on its sources (early cutoff). Test-speed item 16 did this for device tests. PR 461 started the general split with module-build, device-tests and device-test-keys.
- (c) Layer-scoped host tests run against fakes: `FakeCore` and `FakeService` (R2c), `FakePlatform` (R3).
- (d) One contract suite per interface. It runs against the fake and against the real implementation. The real side runs in the emulator once per device.
- (e) Device smoke per service: boot, load, play, edit.
- (f) Module DSP tests run on a SHARC-only rig without a ColdFire boot (`tests/support/machine_rig.py` exists).
- (g) `test:full` once per batch catches interaction effects.

## Levels

| Level | What | When it runs |
| --- | --- | --- |
| Host unit tests per layer | Logic of one layer against fakes | Every change that reaches the layer |
| Contract tests per interface | Same suite on the fake and the real implementation | Fake: with the layer. Real: in the emulator, once per device, when the interface or its implementation changes |
| Device smoke per service | Boot, load, play, edit for one service | When the service or a layer below it changes |
| Module DSP tests | PCM and DSP behaviour on the SHARC-only rig | When the module or its DSP code changes |
| `test:full` | Everything | Once per batch of merges |

## Work packages

### T1a Baseline

- Scope: static, right after R2c merges. For each change class, record what `test:plan` selects and the gate wall time. Classes: workbench TS edit, web TS edit, services/fx edit, core edit, SDK header edit, module C edit, module DSP asm edit, patchkit refactor with no byte change, Python tool edit, docs-only.
- Find inputs that over-select.
- Output: a table in this plan, plus target numbers per class.
- Depends on: R2c.

### T1b Uniform build/test split in Nx

- Scope: all projects get build and test targets as in principle (b). Continues PR 461 and test-speed item 16.
- Proof: an `nx graph --file` dump diff, plus `test:plan` for the T1a classes.
- Depends on: nothing.

### T1c Host contract tests

- Scope: contract suites for `CoreApi` and `Service` now, and `Platform` with R3. Fake and real pass the same suite.
- Proof: both implementations run the suite.
- Depends on: R2c (`CoreApi`, `Service`), R3 (`Platform`).

### T1d Device-test audit

- Scope: test-speed item 7. Move logic checks to host tests. Tag each device test by the layer or service it exercises.
- Proof: deletions go through an independent review (rule: review test deletions).
- Depends on: T1c.

### T1e Service-scoped selection

- Scope: map changed functions (patchkit `objdiff`) or changed files to a layer or service. Run that service's tagged device tests plus a fixed smoke set.
- Proof: measure against T1a before it becomes the default.
- Depends on: T1c, T1d.

### T1f SDK-surface fixtures and coverage

- Scope: fixtures cover every `sdk.toml` import and builder call. Measure it: imports linked per fixture against `sdk.toml`, and ColdFire PC coverage with `cf.profile`.
- A conforming module then needs the static checks (packer, `module:check`, needs and fit) plus one load-and-smoke test. Module-specific emulator tests remain for DSP behaviour.
- Proof: the coverage numbers.
- Depends on: nothing (own lane).

### T1g SHARC-only rig

- Scope: module DSP and PCM tests run on a SHARC-only rig.
- Proof: the same PCM results as the full-device tests, in less time.
- Depends on: nothing (own lane).

### T1h Workbench and web composability

- Scope: TypeScript only. Clearer boundaries, mocked integration points.
- Proof: `test:plan` shows that a TS-only change selects only Vitest targets.
- Depends on: nothing (own lane, any time).

## Order

- T1a after R2c merges.
- T1b any time.
- T1c alongside R3.
- T1d after T1c.
- T1e after T1c and T1d.
- T1f, T1g and T1h in parallel lanes.

## Rules

- No kept tests for tooling.
- No new Python test infrastructure.
- Deletions are reviewed.
- The per-batch `test:full` stays.
