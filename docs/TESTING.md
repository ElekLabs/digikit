# Tests

Reference for the test levels, commands, inputs and gate. Use `mise run test` for affected verification
and the merge gate. Use `mise run test:plan` first when selection is unclear;
it explains the same task selection without executing tests or builds. `test:changed` is cheap preflight,
not complete verification. Other docs
link here. Results of runs belong in `docs/findings/`, not here. Planned changes:
[docs/plans/test-architecture.md](plans/test-architecture.md); test speed:
[docs/plans/test-speed.md](plans/test-speed.md). Current implementation sequence and evidence:
[test/build workflow](plans/test-build-workflow.md). The current module and SDK iteration follow-up is
[fast module and SDK iteration](plans/module-sdk-iteration.md).

## Test levels

A level says what a test proves. Firmware inputs, device version, cost and exclusive resources are
execution requirements, not levels. A successful boot proves only what the run asserts.

| Level | What | Command |
| --- | --- | --- |
| Static (JS/TS) | oxlint, Prettier check, host Vitest, `pnpm -r run typecheck` | `pnpm verify:host` |
| Static (Python) | ruff check and format over pyproject `[tool.ruff] include`; mypy over `[tool.mypy] files` | `mise run lint:py`; `mise run check:layers` (module layering, docs/ARCHITECTURE.md); `layercheck --boundary-report` lists the resident/package crossings |
| Fast preflight | Affected projects only; no builds, no devices | `mise run test:changed` |
| Unit (TS host) | Packages, desktop UI logic, regression helpers; no device | `pnpm test:host` |
| Unit (web) | Web app suites in `apps/web/tests`: Vitest `*.test.ts` (`test-host`, no wasm) and the real-wasm suites `*.wasm.test.ts` (`test-wasm`) | `pnpm nx:pilot run @digi/web:test-host`, `@digi/web:test-wasm` |
| Unit (Python) | RE, firmware and dev tools | pytest on named files under `tests/<owner>/`; `mise run test:pypy` for the PyPy subset |
| Unit (Rust) | Workspace and standalone dkrt | Explicit Nx project target `rust-tests`; runtime-only: `pnpm nx:pilot run core/runtime:rust-tests` |
| Contract (Rust, host) | The reload property of each service (`lfo`, `fx`, `machines`, `samples`): one generic suite over `FakeCore`, six cases, both device profiles | `pnpm nx:pilot run dkrt-svc-<service>:rust-tests` (the file `services/<service>/tests/contract.rs`) |
| Assembly (Rust, host) | The seams between the real packed artifacts of the runtime: fixtures, service files, the port file of the test image, the runtime object and the info table of the image, through the boot's package graph and admission, role checks and installs over `FakePlatform` | `pnpm nx:pilot run core/runtime:test-assembly` (seconds; every `test-device` and `test-conformance` target depends on it) |
| Component | Module DSP conformance on the SHARC core | `mise run module:check ID` |
| Device: stock UI | DT2 stock pages and transport goldens (zero DSP) | `mise run test:stock` |
| Device: FX | Stock FX oracles | `mise run test:fx`, `mise run test:fx:dt2` |
| Device: runtime | Emulator oracles (`tests/regression/emulator`) and device tests in the owning project | `pnpm nx:pilot run <module-id>:test-device` |
| Device: module loads | One shared conformance suite (`tests/conformance`) loads every shipped upload | `pnpm nx:pilot run-many -t test-conformance` |
| Device: Python | Module equivalence, pluck menu and pages scenarios, `upload-rust`, patchkit smoke matrix, patched-boot pytest files | owner pytest files; `mise run patch:test` |
| App E2E | None yet | none |

A mocked UI test, a preview or a daemon scenario is not app E2E coverage.

## Which check for which change

Use an explicit owner target or named test while iterating. Run `mise run test` once before merging;
iteration checks do not replace this gate. `mise run test:plan` explains task selection without running it.

| Change | While iterating |
| --- | --- |
| JS/TS host tests and helpers | `pnpm nx:pilot run <project>:test-host`, `<project>:typecheck` |
| Web tests | `pnpm nx:pilot run @digi/web:test-host` (no wasm); `@digi/web:test-wasm` (real `public/*.wasm` and the patchkit command line: `tests/*.wasm.test.ts`, which builds the wasm first) |
| Desktop UI or app | `pnpm nx:pilot run @digi/desktop:test-host`, `@digi/desktop:check:ui` (source types); `@digi/desktop:typecheck` for test types. `wb:preview` and `ui_shots.mjs` for pages (below). |
| Stock DT2 UI goldens | `test:stock` |
| DN2 or DT2 FX sources and oracles | `test:fx`, `test:fx:dt2` |
| Python under `tools/` or `tests/` | `test:changed`; then the named pytest files |
| Patchkit, ports | `test:changed`; `patch:test --cell ...` for one smoke cell |
| Leaf module | `module:check ID` |
| Device tests of one module | `pnpm nx:pilot run <module-id>:test-device` |
| One device test file | `pnpm nx:pilot run <module-id>:pack`, then `pnpm --filter @digi/regression test:runtime <path filter>` |
| Loading one module | `pnpm nx:pilot run <module-id>:test-conformance` |
| Runtime loader fixtures | `pnpm nx:pilot run core/runtime-fixture-tests:test-device` checks a machine and FX on both coupled devices; `core/runtime-service-tests:test-device` covers a zero-DSP service. |
| Empty-store menu fixture | `pnpm nx:pilot run core/digikit-menu-zero-tests:test-device` checks common menu navigation on both devices with zero DSP; the existing `core/digikit-menu:test-device` keeps coupled capacity checks. |
| SDK UI fixture | `pnpm nx:pilot run sdk-ui-zero-tests:test-device` exercises the `test/ui-demo` SDK UI controls on both devices with zero DSP. |
| Public SDK headers | `pnpm nx:pilot run sdk:test-host` and `pnpm nx:pilot run sdk-ui-zero-tests:test-device`; add the runtime loader fixtures above when load or ABI behavior is affected. |
| Framework module, runtime | `core/runtime:rust-tests` for runtime units, `core/runtime:test-assembly` for the seams to the service files and the image; an acceptance owner's `test-device` for iteration (runtime owners are listed below). Use `test:plan` for affected consumers. The default gate runs the device tests of the changed areas and the smoke set (`test:plan` prints them), and `test-conformance` of every affected module; `-- --all-affected` runs every affected `test-device`. |
| SHARC core or transpiler | regenerate the cores, then `test:changed` |
| Emulator crates | `test:changed`; the crates' Rust tests |
| PCM pins (`tests/scenarios/*.json`, device-test `<case>.pcm.json` pins, rig pins and fingerprints) | from the signal report (section "PCM pins"); a change in how a module sounds goes to Em |

## Commands

Nx owns task selection and prerequisites; mise supplies thin entry points. Nx 24 selects individual
affected tasks using their declared inputs and dependencies. Both local plugins (`tools/nx/modules.mjs`
and `cargo.mjs`) discover projects without running Cargo. Explicit Rust targets use their pinned commands.
`nx.json` owns shared inputs, target defaults and plugin registration. `tools/nx/modules.mjs` infers
module targets and patch-set inputs; `tools/nx/cargo.mjs` infers Rust targets and path dependencies.
Each owner's `project.json` supplies its explicit targets and overrides. The real plan regressions
in `tools/nx/tests/selection.test.ts` verify native Nx input resolution and affected selection.
Device targets may build their declared packs, CLIs and daemons on demand; direct Vitest invocations
require those artifacts to exist already (see "Inputs").

`test:changed` runs only affected lint, format, types, host tests and Python checks. `test` runs the
complete affected target list, with `test:plan` adding Nx `--explain` to that same invocation.
The default gate requests affected module `pack` targets directly, even when a module has no device-test
target. A `module.toml` edit also selects the host-only `patchkit:catalog-check` target once to validate
repository-wide manifest IDs and requirements; each module's pack and conformance read its own manifest
and declared requirement closure.
`DIGI_GATE_BASE` defaults to `main` and is exported so Nx and the Python selector compare the same base.
Set `DIGI_GATE_BASE=HEAD mise run test:plan` to inspect only uncommitted work, or use
`mise run test -- --explain=/tmp/test-plan.json` to save native Nx selection reasons.
Use `DIGI_GATE_BASE` rather than forwarding `--base`, so Python sees the same comparison.
For a proposed file, invoke `pnpm nx:pilot affected --files=path/to/file -t TARGET --explain` directly;
Nx rejects `--files` together with the main command's `--base`.

TypeScript edits select TypeScript targets. An edit in `packages/workbench-core/src` or `ui` selects its own `test-host` and `typecheck`, `@digi/web:test-host`, `check`, `test-wasm` and `build`, and the desktop `test-host`, `typecheck`, `check:ui` and `build:ui` (both apps import it); an edit in `packages/workbench-core/tests` selects only its own two targets; an edit in `apps/web/src` selects the web targets and an edit in `apps/desktop/ui/src` only the desktop targets (the apps do not import each other). Neither selects a cargo, wasm-build, device or conformance target: `patchkit:wasm` reads the Rust crates, `digiemu-desktop:rust-tests` reads the crate and its Rust dependencies, and the desktop `test-host` reads its own `ui/`, the workbench-core sources, `dk-session`, `dkbs`, `devices/*.toml`, `modules/*/*/module.toml` and `config/emulator.json`. `@digi/web:test-host` runs the Vitest project `web-transfer` and does not load wasm; `@digi/workbench-core:test-host` runs the project `workbench-core` (the tests of the shared code and of the NodeHost, with temp folders and stand-in bridge programs). Tests that load the real `public/*.wasm` or compare it with the patchkit command line (`tests/*.wasm.test.ts`, Vitest project `web-wasm`, the only web runner besides `web-transfer`) are the target `@digi/web:test-wasm`, which depends on `patchkit:wasm` (replayed from the cache unless a Rust input changed). A new test that needs the wasm or the command line is named `*.wasm.test.ts`. The fake devices of the web tests (`FakeDevice`, `FakeDrive`, `FakeUpgrade`), the fake modules and the command-line helpers are in `apps/web/tests/support/`; the site's screenshot tools import them from there. The logic of the desktop Samples mode is tested in `packages/workbench-core/tests/samples-core.test.ts`; the upload engine is tested in `packages/workbench-core/tests/sample-upload.test.ts`.

Control-client, test-support, dk-session, desktop-host and regression-host targets use local result
caching; web-transfer and the per-module `test-host` targets remain uncached. tools/nx/modules.mjs infers, for each shipped module, a `test-host` target (host tests under `tests/`), a `test-device` target (its `tests/**/*.device.test.ts`, run with Vitest directly after its packs) and, for each shipped dynamic, service or `host` module, a `test-conformance` target (the shared conformance suite for that module). A project with several device test files has one cached target per file, `test-device--<file>`, and `test-device` is their no-op parent; a project with one file has only `test-device`. Tasks and Vitest workers follow the CPU budget below. Other Nx targets: `pack` (patchkit pack for each device, building stale module objects on demand, into `dist/modules/<device>/<leaf>.dkm`; only for the modules the gate packs, not for patched-in core modules or static fixtures), `pack-aliases` (renamed copies named by `runtimeAcceptance` aliases, same directory), `patchkit:cli` (`dist/patchkit/patchkit` and `dist/bytes/patchkit-test-<device>.key`) and `plusdrive-format:cli` (`dist/plusdrive/plusdrive`). Outputs go to `dist/`, never under `out/`. Shipped content modules (not `core/*`) depend on `core/runtime`; machines also on `core/rows` and `core/dsp-voice`, effects on `core/dynfx`. Web tests retain ignored
generated WASM and optional inputs outside the cache boundary. Cache use assumes a frozen installation. New test helpers, environment
reads or external inputs require input/output review and scoped invalidation/restoration evidence before relying on cached results. The
[bounded cache observations](findings/nx-pilot.md) include dependency failure/restoration probes;
only the support/session pair was timed, not whole preflight. The standalone runtime unit target
remains uncached; all test-only device targets, including `core/runtime-tests`, are cached like other module device targets.
Plugin sources are not inputs of module device targets. A `runtimeAcceptance` record may set `dksampleWasm: true`
(depends on `dksample:wasm`) or `dkprofWasm: true` (depends on `dkprof:wasm`). Python tools need no record flag: list the `.py` tools a test runs in the record's `sources`. `tools/nx/py-imports.mjs` follows their `tools.*` imports (including function-local imports and `-m tools.x` strings) and hashes those files. Relative imports are not followed.

A `runtimeAcceptance` record of a device test of the runtime loading and lifecycle sets `firmwareOnly`: one line that names what only the firmware or hardware shows in its files (the rest is a unit or assembly test). Nothing reads it: it is the reason the test exists, kept next to the record (RC2 split, [ledger](plans/ledgers/device-test-inventory.md)).

| Command | What runs |
| --- | --- |
| `mise run test:changed` | `nx affected` against `DIGI_GATE_BASE` (default `main`) over the targets `lint`, `fmt-check`, `typecheck`, `test-host`, `python-checks` and `objects-check` (about 25 s uncached; `patchkit` is affected by edits of its own files, of core module sources, manifests or toolchain inputs, so a stale committed `.dko.json` fails here). Does not run Rust, module builds or devices. |
| `pnpm test:host` | The Vitest projects of `vitest.host.projects.json` except `web-wasm` (the Nx `test-host` set): control-client, test-support, dk-session, web-transfer, desktop-host, regression-host, module-tests, nx-host, dkbs. One worker by default; `-- --maxWorkers N` changes it. |
| `pnpm verify:host` | oxlint, Prettier check, host Vitest, typecheck of control-client, dk-session, test-support, regression and desktop. Stops at the first failure. |
| `pnpm --filter @digi/web test-wasm` | The Vitest project `web-wasm` (`tests/*.wasm.test.ts`, 120 s limit per test). Needs `public/*.wasm` (`@digi/web:test-wasm` builds it). Cases that need firmware skip when absent. |
| `mise run lint:py` | `ruff check` and `ruff format --check` over the pyproject include list. |
| `mise run lint` | `lint:py`, `lint:rust`, `pnpm lint`, `pnpm fmt:check`. `lint:rust` runs `workspace:rust-fmt` (`cargo fmt --check` on every workspace crate except `dkfw`) and `workspace:rust-clippy`. `rust-clippy` runs `cargo clippy --no-deps --all-targets -- -D warnings` on the crates that have no clippy warnings. Add a crate to its list when you fix the crate's warnings. `rust-clippy` is in the merge gate. It needs clippy in the pinned toolchain: `rustup component add --toolchain 1.98.1 clippy`. `dkfw` is left out because its crate root is generated. Other generated files carry `#[rustfmt::skip]` on their `mod` line. A format change of `dkrt-sdk`, `dkrt-fit` or `dkneeds` changes `source_sha256` in the committed `build/*.dko.json` objects, so rebuild them with `core/runtime:build-objects`. |
| `mise run test:pypy` | The SHARC core tests under PyPy 3.11. Slow. |
| `mise run test:stock` | Vitest project `stock` (`tests/regression/dt2`): DT2 1.17 zero-DSP goldens. Filters pass through. |
| `mise run test:fx` | Project `fx-dn2`: 61 quick assertions; `DIGI_FX_SWEEP=1` runs 191 across 134 scenarios. `DIGI_JOBS` bounds rendering. |
| `mise run test:fx:dt2` | Project `fx-dt2`: 135 quick assertions; `DIGI_FX_SWEEP=1` runs 146 across 60 scenarios. `DIGI_DT2_CARD` selects a prepared card image. |
| `pnpm nx:pilot run <module-id>:test-device` | The module's `*.device.test.ts` files (Vitest project `runtime`) with merge keys (a file outside `mergeTests` replays its last result after a change that reaches it only through shared inputs), after its packs, aliases and the plusdrive CLI are built into `dist/`. `<module-id>:test-device-all` and `test-device-all--<file>` run them with full inputs. |
| `pnpm nx:pilot run-many -t test-conformance` | Project `conformance`: `tests/conformance/load-{dt2,dn2}.device.test.ts` on DT2 1.17 and DN2 1.12, once per eligible module (`DIGI_CONFORMANCE_MODULES` is set per module). |
| Other Nx gate targets | `patchkit:objects-check` fails when a committed core object (`{runtime,services}/**/build/*.dko.json`) is older than its sources (fix: `patchkit build PATH` for the stale module path); it ignores the content-module cache. `digi-control:daemons` builds the daemons and writes `dist/daemons.json`; device helpers read it when `DIGI_EMUD_*` is unset. `digi-coupled:build` and `build-dt2` build the coupled emulator. `patchkit:wasm` builds the patchkit, dkupload and dksample Wasm into `apps/web/public/`; its inputs are the Rust crates of patchkit (and the build files), not TypeScript. `@digi/web:patchkit-wasm` is its no-op alias, and the web `test-wasm` and the workbench depend on it. `mise run workbench` builds the desktop UI, `patchkit.wasm` and the port files (`patchkit:ports`) through `@digi/desktop:workbench-inputs`. `workbench-inputs` also runs the `pack` targets of `core/services/*` (the services the Transfer page offers, `dist/modules/<device>/*.dks`); `mise run transfer` and `wb:cli` run them through the hidden task `services:pack`. `workspace:lint` and `workspace:fmt-check` run `pnpm lint` and `pnpm fmt:check`. Project `regression`: `test-device` runs `tests/regression/emulator/` after the packs of its runtime-acceptance records; `test:stock` depends on `daemons`, `test:fx` and `test:fx:dt2` on the coupled builds; `test:fx-no-aot` runs `test:fx` in the no-AOT configuration. Project `digiemu-desktop`: `rust-tests` runs the app crate tests (inputs are the crate and its Rust dependencies). Owner projects `tools`, `devices`, `sdk` and `tests` own the paths no other project owns; `tools` carries `python-checks` and `python-tests`. The emulator gate targets live in the gate projects under `tools/gate/` (see "Nx gate"). |
| `mise run module:check ID` | Machine/effect conformance and own Python tests, then any module-owned Nx `test-device-all` target. May build or emulate; size/fit follows both. Not an all-module load proof. |
| `mise run module:audit NAME` | ColdFire memory audit of one run-time module. |
| `mise run lfo:report -- <author/name> [--device dn2\|dt2]` | Output report of an LFO shape module: fit of the measured LFO output against each declared table, swing, verdict, SVG plots. Not a test, no gate (sdk/templates/lfo-shape/README.md "Output report"). |
| `mise run patch:test` | Smoke tour over `tests/patchkit/smoke/matrix.toml` (`-- --cell dt2-1.17:core/fx`, `-- --update`). |
| `mise run patch:proofs` | Every emulator proof on its quick tier. |
| `mise run dev:plusdrive-check` | The firmware's own view of an image built from a folder (about 1 min). |
| `mise run scenario NAME` | Replays one headless scenario. |
| `mise run scenario:all` | Every DN2 scenario; fails when a pinned PCM hash differs. |
| `mise run test:slow` | `test_outcomes run --check` over `tests` with `--slow` and a one-hour per-file timeout, then `uv run python -m tools.emulator.coupled gates`. |
| `mise run test:quick` | Launcher, PGO, target and coupled-tool tests, then `coupled test-quick`. |
| `mise run test` | Complete affected verification and merge gate: `nx affected` against `DIGI_GATE_BASE` (default `main`). |
| `mise run test:plan` | The `affected` selection of all gate targets, device and conformance included, with native Nx `--explain`; no tasks execute. |
| `mise run test:full` | Every gate target on every project, plus `tier-sweep` and `dt2-tier-sweep`; device tests run as `test-device-all` (full inputs for every file). It runs only when Em asks for it. A merge needs the merge gate in AGENTS.md "Checks": `mise run test` plus the static checks, or, when a change reaches most device tests through a shared input, a named device subset on DT2 and DN2 (one gate for a stack of PRs). |
| `mise run clean:runs` | Lists old `digi-*` run directories under the temp dir (older than 24 h, not used by a running `digi-emud` or node process) and orphan `digi-emud` processes; `-- --yes` deletes the directories, `-- --hours N` sets the age. It never kills a process. A Session refuses to start with less than 50 GB free. |

### Nx gate

- `mise run test` runs the gate targets against `DIGI_GATE_BASE` (default `main`) through `node tools/nx/gate.mjs --tier merge`; the Nx launcher holds the shared benchlock and runs at nice 10. `nx affected` selects every target but `test-device` (lint, types, host and Rust tests, packs, conformance, `smoke`, `pins`, ...); the device tests are selected by ownership (below). `mise run test -- --all-affected` puts `test-device` back into `affected`: every device test whose key changed.
- Device test selection by ownership (`test-subset.mjs --gate`, the second command of the merge tier; code: `tools/nx/device-owners.mjs`). A device test is a mechanism test on fixtures (owned by the project of the mechanism) or a behaviour test (owned by its service or module). The gate takes the Nx project that owns each changed file (the deepest root containing it, so a nested project wins over its parent) and selects: (1) the device tests of that project; (2) the device tests of every project whose authored `implicitDependencies` name it (a test project names its subject there); (3) the device tests that load a changed fixture (`tests/fixtures/modules/**`, listed in a record's `modules`); (4) the device tests whose import closure holds a changed `.ts` file (a harness or test-support helper; the driver packages only for `DRIVER_SUBJECT_FILES`), a changed test file, or a file in a record's `sources`; (5) the projects in the `users` of the entries of a changed `ports/fw/<point>.toml` that differ from the base (new, changed or removed declarations); (6) the affected halves of a device whose file changed (`devices/<stem>.toml` and companions). A service or module that a test only loads is no subject: the test names its subject. `gate-device-smoke:device` is added whenever `affected` selects any device half; `gate-smoke` and `gate-pins` follow `affected`. Conformance runs for the modules whose own source changed. A `project.json` edit is compared as data: `tags`, `incidental` and `firmwareOnly` of a record and the project's `implicitDependencies` are dropped, and an edit equal after that selects nothing (printed as `metadata only`); a changed record selects its own tests, any other difference (targets, unknown fields, a parse error) selects the project. A changed path no project owns, or owned by a project no test names, selects no device test and is printed. This holds by design for patchkit sources, `devices/*.json`, `config/emulator.json`, `ports/bindings`, `image-roots.json`, the emulator and `Cargo.lock`: the only device cover is the smoke set `gate-device-smoke`, plus `gate-smoke`, `gate-pins` and `objects-check` where `affected` selects them. The SDK (`sdk`) is named by the loader, package and machines tests, so it is not in this list. The gate cannot see a test that is missing an `implicitDependencies` entry, so the project graph build checks it (`checkSubjectCoverage`, `tools/nx/device-tests.mjs`; it runs for every project with device tests when Nx reads the graph, so lint and every Nx call fail on it): every real service or module (not a fixture under `tests/fixtures/modules`) in the `modules` of a runtimeAcceptance record must be named in the project's `implicitDependencies` or listed in that record as `incidental: [{ "module": ID, "reason": TEXT }]` (loaded, not exercised). A service must be named by both its dks project (the module id) and its crate (`dkrt-svc-<service>`): they select on different changes, and naming one is an error. Name the subject when the test asserts the module's behaviour; use `incidental` for a host or harness the test only needs loaded (`digikit/debug`, the machines service under a fixture). Nothing widens the selection. The gate prints what `affected` selected and it left out: `skipped, selected by affected and left to the full tier: N device halves and conformance tasks of M projects`. The full tier (`mise run test -- --all-affected`, `mise run test:full`) runs them on request. A project that a test should follow names the subject in `implicitDependencies`; fix a missed test there, not by a list in the gate. `mise run test:plan` prints the `affected` plan and then the device and conformance selection with its skipped list.
- `mise run test:full` runs every gate target on every project, plus `tier-sweep` and `dt2-tier-sweep`. Its device tests run as `test-device-all`. It runs only when Em asks for it. A merge needs the merge gate in AGENTS.md "Checks": `mise run test` plus the static checks, or, when a change reaches most device tests through a shared input, a named device subset on DT2 and DN2 (one gate for a stack of PRs).
- Core-change policy: a change runs all device tests of the projects it edits. A device test file not listed in `metadata.mergeTests` of the owning project.json (paths relative to the project root; policy in `tools/nx/device-test-keys.mjs`) has a narrow `test-device` cache key (the rule under "Three keys exist"). A change that reaches it only through shared inputs (other core modules, SDK, device profiles, patchkit, the daemon) leaves the key unchanged, and Nx replays the last result. Loaded packs include the alias packs of the file's records. Files in `metadata.mergeTests` use merge keys that include the device image. The shipped-module emulator loads and `test-device-all` retain shared runtime and pack inputs in their cache keys. `test-device-all` (per-file `test-device-all--<file>` for unlisted files) is the full-inputs twin; the `full` tier selects it instead of `test-device`. The twin also sets `DIGIKIT_TIER=full` in its task env, so `mise run test:full` runs the `full: true` tests of the card rig and the other harnesses.
- Test sources have one definition, `config/test-source-patterns.json`: a directory named `tests` at any depth (any language) and the Rust file names `tests.rs`, `tests_*.rs`, `*_tests.rs`, `fake_*.rs`. They hold unit tests, not product: an edit selects the crate's `rust-tests`, format and clippy, not the device and gate suites, and changes no committed `.dko.json` object. Readers: `tools/nx/test-source.mjs` (`isTestSource` for `test:subset --base`, `testSourceGlobs` for the negative globs of `digi-control:daemons` in `tools/nx/cargo.mjs` and of `subjectInputs`, and the `patchkit-test-<device>.key` file list in `tools/nx/patchkit-key.mjs`), `tools/devtools/emud_key.py` (daemon test key) and `packages/patchkit/src/build/test_source.rs` (`source_sha256`; compiled in, so a change rebuilds patchkit, and an input of `patchkit:cli` and `objects-check`). The `production` input of `nx.json` is static JSON: `tools/nx/tests/test-source.test.ts` checks that it holds exactly the derived exclusions. The release build still lists `#[cfg(test)]` module files in its dependency file (rustc reads them), so the keys leave them out by name. One guard keeps that honest: `unguardedTestSources` in `tools/nx/test-source.mjs`, run over the Rust files git tracks (the set the target's `**/*.rs` input sees) by the `workspace:test-source` target (a dependency of `workspace:lint`; inputs: every `.rs` file, the rule file and the script; its negative cases are host tests in `tools/nx/tests/test-source.test.ts`), fails when a Rust file that matches the rule is not declared as `mod x;` behind `#[cfg(test)]` (or `cfg(all(test, ...))`, or has an inner `#![cfg(test)]`). Known over-selection: `#[cfg(test)]` modules with other names (`test_lock.rs`, `host_io.rs`) still count as product.
- For a shipped dynamic machine or effect, the merge gate selects its individual emulator load when its manifest, production files or explicitly borrowed build inputs change. For shared loader changes, `core/runtime-fixture-tests:test-device` loads Sine and `test/dyn-fx-gain` in both device halves, each keyed on that device's patched image. The fixtures share the shipped load assertions for USB upload, restart, registration, DSP allocation and faults, including machine row publication. FX route behavior is covered by the `test/dyn-fx-gain-both` track route tests and the FX send tests. Capacity and SHOT sample-pool coverage have their own device tests. Service loads keep their existing merge selection. Run `test-conformance` to load any eligible module directly. The full gate uses `test-conformance-all`.
- `mergeTests` holds the smoke set: module-load, tb303, fx-send, samples-live (DT2), samples-store-live (DN2), upload-depth, safe-mode, menu and shot (`shot-dt2`, `shot-dn2-audio`). Every other device test file has the narrow key.
- Gate projects in `tools/gate/` carry their triggers as `{workspaceRoot}` inputs: `gate-smoke` (target `smoke`), `gate-card` (`card`), `gate-stock` (`stock`, cached), `gate-fx-dn2` (`fx`, `fx-no-aot`, `tier-quick`, `tier-sweep`), `gate-fx-dt2` (`fx-dt2`, `dt2-tier-quick`, `dt2-tier-sweep`), `gate-pins` (`pins`), `gate-dt2-tone` (`dt2-tone`) and `gate-rust-modules` (`rust-tests`). The Vitest suites behind `stock`, `fx` and `fx-dt2` are the `test:stock`, `test:fx` and `test:fx:dt2` scripts of `tests/regression`. `smoke` and `card` run through `tools/nx/daemon-env.mjs`, which sets `DIGI_EMUD_<DEV>` from `dist/daemons.json`, so they use the same content-keyed daemon as the Vitest device tests. Module projects with device tests depend on `regression` (harness), `digi-control` (daemons), `patchkit` (packs) and `devices`; shared SDK contract/compiler surfaces are explicit target inputs. Only modules with `test-conformance` also depend on `conformance`. `python-checks` and `python-tests` run on any change outside `docs/`.
- The number of concurrent Nx tasks and Vitest workers comes from the machine budget; see "CPU budget of the gate".
- A device or conformance file that fails only with a load symptom (boot timeout, lifecycle deadline, closed connection, remote release timeout) is rerun once by `tools/devtools/emu-slot.mjs`; the retry is printed in the task output and counted as `retried (n)` in the gate summary. Assertion failures and second failures fail the task.
- Compact output: every `pnpm nx:pilot` task run (and so `mise run test`, `test:changed`, `test:subset --run` and the gate) prints one line per finished task, `✔ project:target  at 72 s` or `✖ project:target`, where `at N s` is the time since the run started (Nx prints no task duration there). A failed task adds the path of the run log with the line where its section starts and the first failure block of its output (the first failing vitest test, cargo test panic, rustc or tsc error or pytest case, at most 30 lines). The run ends with `nx: N ok, M failed, T s; full output PATH`; the gate trace summary follows as before. The whole output of the run goes to `out/runs/nx/<stamp>-<pid>.log`. Exit codes and caching do not change. `--verbose` or `DIGI_NX_VERBOSE=1` keeps Nx's own streaming output; so does any other `--output-style` or `NX_DEFAULT_OUTPUT_STYLE`.
- Reading a log: `node tools/nx/failures.mjs <file | project:target> [--lines N]` prints the same failure block of a run log, an Nx terminal output (`.nx/cache/terminalOutputs/<hash>`) or any tool output; with `project:target` it uses the newest run log in which that task failed. Do not `cat` or page whole logs: the agent hook (`tools/devtools/agent_guard.py`) blocks `cat`, `less`, `more` and `head`/`tail` of over 200 lines of those files; use `failures.mjs`, `grep` or a bounded `tail -n`.
- Nx's `summary` output reports counts and paths to full failure logs; `--output-style=static-failures-only` prints failing task output inline. No custom status router is involved.

Wait without polling: run a long command with a long tool timeout (up to 2 hours in Claude Code with `BASH_MAX_TIMEOUT_MS`), not with `sleep` or `tail`. Claude Code moves it to the background after about 3 minutes and notifies you when it finishes: wait for that notification. If you stop early, stop what you started. Nx detects agents (`isAiAgent`, which reads `CLAUDECODE` among others; `AI_AGENT` alone does not trigger it) and then prints the `summary` style, so task output appears only in the log paths it names. A person who wants a live log sets `NX_DEFAULT_OUTPUT_STYLE=stream` (or passes `--output-style=stream`). The gate (`tools/nx/gate.mjs`, tiers `merge` and `full`) therefore passes `--output-style=static-failures-only` unless the style is already chosen: one line per task as it finishes (a tick, `nx run project:target`, the cache status) and the full output of failed tasks. Nx prints no duration there. When the run ends, the gate prints the slowest tasks, failures and slowdowns of that run. `mise run test:history` (`tools/nx/gate-history.mjs`, `-- --days N --top N`) prints them for the last 14 days of this worktree; it reads Nx's own task history database (the newest `-v<N>.db` in `.nx/workspace-data/`), which Nx writes when a command ends, so a killed gate leaves no record and a new worktree starts empty. A gate tier run also writes `out/runs/gate/<stamp>/` (`nx-profile.json` from Nx, `slots-trace.json`, and `trace.json`, the two merged for ui.perfetto.dev) and prints wall time, task-minutes, slot wait against run minutes and the ten longest tasks by run time; `node tools/nx/gate-history.mjs --trace <dir>` redoes this for a finished run. Nx runs twice as many tasks as the emulator ceiling (`DIGI_NX_PARALLEL` overrides), because device tasks hold an Nx slot while they wait for an emulator slot.

`uv run python -m tools.devtools.test_outcomes BEFORE.xml AFTER.xml --renames-from REF` compares
outcomes and skip reasons test by test after moving tests.

### Device test timings

`node tools/nx/device-timings.mjs [LOG] [--md] [--top N] [--audit audit.md]` prints the measured run time of each device half of one gate run: half, minutes, coupled or zero-DSP, project, file, and whether it ran or was cached; then the summed run time (coupled and zero-DSP) next to the wall of the run. LOG is a launcher run log (`out/runs/nx/*.log`; default: the newest); run it from the checkout whose `.nx/workspace-data` holds the project graph. The seconds are the ones `tools/devtools/emu-slot.mjs` records when a passed task leaves its emulator slot (`<git common dir>/emu-slots/durations/`, slot wait excluded): Nx's own task spans include the slot wait and cannot rank tests. A file is overwritten by the next passed run, so a row marked `*` was written outside the log's run window; a cached or failed half has no seconds of this run. `--md` prints every half; `--audit` adds the files whose minutes differ from a suite audit table by more than 2x. Host test: `tools/nx/tests/device-timings.test.ts`.

### Service contract suite

`dkrt_test_support::provider_contract::<Recipe>(&Fixture)` (`runtime/core/test-support/src/contract.rs`) proves for a service (an owner object behind the part tables of its row) that a module can be loaded, unloaded and loaded again (docs/plans/extension-model.md 3.5). Each service crate runs it in `tests/contract.rs` for the DT2 and the DN2 profile; it is part of the crate's `rust-tests` (about a second): `pnpm nx:pilot run dkrt-svc-lfo:rust-tests` (and `-fx`, `-machines`, `-samples`).

After editing the shared contract suite or rebasing such an edit, run the affected Rust targets against the gate base. `mise run test:changed` omits `rust-tests`. The merge gate includes them.

```sh
pnpm nx:pilot affected -t rust-tests --base "${DIGI_GATE_BASE:-main}"
```

`Lifecycle` runs a module as the loader does over a `FakeCore`: `settle` of the selection, the DSP reservation (`dsp_admit`, `dsp_claim`), `module_begin`, the module's `init`, the `PREPARE` and `WRITE` passes of `module_commit`, the registry walker's check (the layout by `dkrt_abi::regcheck::check_layout`, which the runtime's walker calls too, then the service's check steps), `DECLARE`, `apply` and `apply_record`, and the publication of the DSP rows. A refusal at any step gives the service `remove`, `dsp_clear` and `dsp_release`; an unload runs `dsp_clear`, `remove`, `dsp_release`. The state it compares is the non-zero words of the fake firmware memory, every byte of the DSP state, the last non-zero word on each link slot, the link queue (live values and queued requests), the last word at each row's enable word (`Fixture::enable`: the pc of an effect row, the type of a voice row), the settings, the `cap` counts and the service's own view (`Fixture::state`).

The cases: (1) `declare` gives the kinds of `[records.kinds]`; (2) `check` writes nothing and makes no core call, in the state where the loader checks (it accepts the section there); (3) the state after `A` is the state after `A`, unload `A`, `A`, and `B`'s own words are the same after `A`, unload `A`, `B`; (4) unloading `A` leaves `B`'s words, DSP state and ids as they were; (5) a refused commit and the loader's `remove` leave the state as it was; (6) ten reloads do not grow a table or move a sticky value; (7) an unload leaves nothing, by two comparisons. First, the whole state after "load `A`, unload `A`" equals the state after "load `a2`, unload `a2`" (`a2` has the keys of `A` and other content in every table the service fills, and DSP rows with other field values: `Dsp::salt`); loaded `A` and loaded `a2` must differ in each of memory, DSP state, settings and the service's view, or the fixture lists the area in `salt_same` with its reason. Second, the state after the unload equals the state before the load, word by word: every memory word, DSP state byte, byte of the service's row tables and the settings that differ fail unless `Fixture::kept` (address, value), `kept_dsp` (byte offset in the loader's `DspState`) or `rows.kept` (byte offset in the service's row tables: `DKRT_VROWS`, `DKRT_FROWS`) names the word with the value it may hold (a free value of the service, never the value the module wrote). So an unload that writes the module's own value back, or leaves an owner or active byte, fails. Besides: nothing is queued on the link after the unload (`Fixture::live` queues live values for the loaded module's rows, as a knob turn does), and every row's enable word is 0 or its off value; (8) a commit refused during apply (`bad_late`, after the rows of earlier records were written) and the loader's undo leave what the unload of a module with the same rows (`same_but_ok`) leaves. The last words sent for a row's content stay in the DSP memory until the next row overwrites them; they are not compared. (13) `Fixture::parked_knobs` (`ParkedKnobs`: the live kit, the stored value of the first row, a kit load): the value the user turned survives a reload of the same module, and is the default when another kit is live at the load, when a kit load ran while the module was gone, when another module takes the row, and when a module of another name has the same keys. A service that parks no values sets `None`.

A service gives the suite a `Fixture`: the profile, `[records.kinds]`, the image tables of the profile, its modules (`a`, `b` with other keys, `a2`, `bad` that the commit refuses, `bad_late` or none when its `check` covers every limit of its `apply`), the enable words, the kept words (memory and DSP), the areas where `a2` may equal `A`, an optional live-value hook, an optional knob-value hook and its state view. A module is a call list: the same builder calls (`build::*`) as the `module.c` of the shared fixture it is named after (`tests/fixtures/modules`), or `Init::record` for `dk_record_add`; no hand-built record bytes where a builder call exists. The call lists are in the service's `tests/contract.rs`, because they name the service's `build::*` and `dkrt-test-support` sits below the services; a call list mirrors its fixture's `module.c` exactly. The fixtures' `module.toml`, `module_ids.h` and sources are inputs of the crate's `rust-tests` (`FIXTURE_SOURCE` in `tools/nx/cargo.mjs`): when a fixture changes, read its call list against it. The samples service has no kind and no builder call; its modules are a track-table record in the DSP section (`Dsp::bank`) and no shared fixture has one.

A service passes by writing its rows through `FwMem` (`core.rd`, `core.wr`, `rd8`, `wr8`, `rd16`, `wr16`, `wr_half`), not at raw firmware addresses, so the fake memory sees them. A failure names the case, the profile and the words that differ.

To prove the suite sees a leftover, change the service's `remove`, `dsp_clear` or `dsp_release` to leave one item behind (not committed) and run the crate's `cargo test -p dkrt-svc-<name> --test contract`: it fails.

### Assembly tests

The assembly layer sits between the unit tests of a component and the device tests (docs/plans/test-architecture.md T2 step 4). A unit test owns the logic and the edge cases of one component against fakes. An assembly test owns a seam between separately built real artifacts: per seam one case that fits and one typical refusal, never a repeat of a unit edge case. A device test owns only what the firmware or hardware shows. An assertion belongs at the lowest layer that can see it.

`pnpm nx:pilot run core/runtime:test-assembly` runs `runtime/core/src/rs/src/package/loader/tests/assembly_tests.rs` (`cargo test --lib assembly_tests`, host build, seconds, cached). Its `dependsOn` build what it reads: `<id>:pack` of the fixtures (`test/sine`, `test/dyn-dn2`, `test/dyn-fx-gain`, `test/lfo-shape`, `test/svc-test`, `test/svc-test-user`) and of the services (`core/services/fx`, `machines`, `lfo`), `patchkit:ports-test` (`dist/ports-test/<device>-<version>.dkport`, the port file of the test image) and `patchkit:cli` (the test runs `patchkit port stock` to put the stock bytes of every hook site in the fake firmware), and `patchkit:device-images`, which also writes `dist/bytes/images/<device>/set-<set>.info`, the info table of the patched image: `name hash kind address words...` per entry, taken from the `info` list of the plan that `patchkit apply` writes. The committed `runtime.dko.json` and the service objects are read from the repository. A device without its firmware file is skipped with a `SKIP` line, as its device tests are.

The Workbench settings case uses `test/settings-a:pack` on both profiles. It replays descriptor registration during init over `FakePlatform` and checks discovery for the loaded instance and refusal after unload. It can run without the stock firmware images.

Every `test-device` and `test-conformance` target `dependsOn` `core/runtime:test-assembly`: no device run starts while it fails. Its inputs are the runtime sources and the packed artifacts above (`pnpm nx:pilot show target inputs core/runtime:test-assembly`). `rust-tests` skips `assembly_tests`.

A new service or runtime mechanism adds an assembly case before its first device test. A failure of this target is a wiring defect (a stale object or ABI word, an unmet need, a limit that is not in the image), not a flaky test.

### Device smoke set

`gate-device-smoke:device` runs a fixed set of 8 device tests, each on DT2 and DN2 (16 halves), plus the `card-guard` boot on ST (17 halves: the one device test that runs on ST). It proves: boot and selection (record, restart, boot loads the module), load, unload and reload while running with the entries back to stock, a ColdFire service hook installed and called, the SHARC audio path through a run-time machine, persistence on the card (settings), safe mode and its boot counter, transfer and store, and the DIGIKIT menu.

| Test | Project |
|---|---|
| `boot-restart` | `core/runtime-boot-tests` |
| `live` | `core/runtime-tests` |
| `services/providers` | `core/runtime-provider-tests` |
| machines `equiv` | `core/runtime-machines-tests` |
| `settings/settings-api` | `core/runtime-settings-tests` |
| `safe-mode` | `core/runtime-boot-tests` |
| `transfer-live` | `core/runtime-transfer-tests` |
| digikit-menu `zero/menu` | `core/digikit-menu-zero-tests` |

```text
pnpm nx:pilot run gate-device-smoke:device
```

The target (`tools/gate/device-smoke/project.json`) only depends on the existing `test-device--<file>--dt2` and `--dn2` targets, so it reuses their cached results and adds no test code. Best runs take about 7 min on DT2 and 12 min on DN2. When to run it: a change to one owner's firmware-facing code runs only that owner's device tests; a host-only change runs no device tests; a change to an input every device test reads (the runtime image, the SDK, patchkit, the platform) runs the device tests of the changed behaviour plus this smoke set, instead of every device test the change selects. `mise run test` runs it whenever `affected` selects a device test (see "Nx gate"); run it by hand with the command above. Nx skips a `dependsOn` target that does not exist, so a renamed smoke test leaves the set silently smaller: check `pnpm nx:pilot show project gate-device-smoke --json` lists 17 halves after renaming one. [device-test-inventory.md](plans/ledgers/device-test-inventory.md) maps the remaining device tests to the plan items that split them.

### Named device subset

`mise run test:subset -- <project...> [--base REF] [--why] [--gate]` runs the device tests of the named Nx projects: the project's own tests and the tests of every project that names it in `implicitDependencies`. The merge gate uses it with `--gate` (the ownership selection of "Nx gate"). By hand it is for a change that reaches device tests only through shared inputs (SDK, profiles, patchkit), which the gate leaves to smoke: name the projects whose tests should run. The command reads the Nx project graph, resolves each test file to its leaf targets `test-device--<file>--dt2` and `--dn2` (the devices the file runs on), and adds `gate-smoke:smoke` and `gate-pins:pins` only when Nx `affected` selects those tasks (against `--base`, `DIGI_GATE_BASE` or `origin/main`); otherwise it prints `gate-smoke: not affected`. Without `--run` it prints them. With `--run` it runs them through `pnpm nx:pilot run-many` (cached, with the remote cache); `DIGI_EMU_SLOTS` and the other emulator settings pass through the environment. `--base REF` adds the ownership selection to the names (a union): the tests the changed files select since REF, and `gate-device-smoke:device`; it prints the changed files no project owns, the touched projects no test names, and the skipped halves. `--gate` is `--base $DIGI_GATE_BASE` for the merge gate: it leaves out `gate-smoke` and `gate-pins`, which the gate's `affected` call runs. It sees the tracked diff against REF (commits and working tree) and the untracked files. It does not see files that tests read by path without importing them. `--why` adds a line to each selected file with the names that selected it and, with `--base`, the changed files. The command fails when nothing changed since the base (not with `--gate`); a change that selects no device test prints "nothing to run". An unknown name fails. It also prints how many tasks are coupled and zero-DSP and how many start at once.

```text
$ mise run test:subset -- core/runtime-lfo-tests --why   # gate-smoke and gate-pins are listed only when affected selects them
test:subset: core/runtime-lfo-tests
  core/runtime-lfo-tests lfo.device.test.ts: test-device--lfo--dt2 test-device--lfo--dn2
    why: core/runtime-lfo-tests
  core/runtime-lfo-tests lfo-shape-output.device.test.ts: test-device--lfo-shape-output--dt2 test-device--lfo-shape-output--dn2
    why: core/runtime-lfo-tests
  gate-smoke: smoke
  gate-pins: pins
test:subset: 0 coupled and 8 zero-DSP tasks; core budget 12, 12 slots: 0 coupled and 8 zero-DSP start at once
test:subset: pnpm nx:pilot run-many -t test-device--lfo--dt2,test-device--lfo--dn2,test-device--lfo-shape-output--dt2,test-device--lfo-shape-output--dn2,smoke,pins -p core/runtime-lfo-tests,gate-smoke,gate-pins
```

The subset does not replace the static checks (`lint,fmt-check,rust-fmt,typecheck,python-checks`). The selection has host tests (`tools/nx/tests/device-owners.test.ts`, `tools/nx/tests/test-subset.test.ts`).

## Fast iteration: dk:dev and Reload

For module code and the UI layer (`core/ui`, the menu of the system package, `test/ui-demo`) one edit and one look should take seconds, not a
boot. `mise run dk:dev -- emulator --store 'test/ui-demo*' --device dn2[,dt2] --keys 'tap:NO,tap:SETTINGS,wait:400,tap:DOWN*9,tap:YES,wait:600,shot:menu'`
(`tools/patchkit/pk_devreload.py`) builds the objects whose sources changed (`patchkit build`), reloads a daemon it keeps alive per device and
module set (`--stop` ends it), runs the key script from the main screen, and writes `out/runs/devreload/<device>/NAME.png` (4x) and
`sheet.png` (2x, four per row, labelled). The target is a patch set of `devices/*.toml` or module ids; `--keys` is a JSON list of harness
steps or inline text (`tap:KEY`, `wait:MS`, `turn:ENC A:BY`, `shot:NAME`, `*N` repeats). The workbench has the same call as the Reload
modules button (⇧⌘R), with `build: true`: it builds the stale objects of the session's modules first (ColdFire and SHARC+; docs/CONTROL.md). `dk:dev` builds in its own step
and reloads without `build`.
Each device test run target (`test-device`, `test-device-all`, `test-device-full`, per file or single-file) is split into two cached tasks, `<target>--dt2` and `<target>--dn2`. The harness runs a file for one device when `DIGI_DEVICE` is `dt2` or `dn2`; the halves set it and list it as an input, so they run in parallel and a file's floor is its DN2 half. The target without the suffix is a no-op parent with the same inputs that runs the halves that exist. A file has only the halves of the devices it runs on: they come from `devices` (`["dt2"]` or `["dn2"]`) on its `metadata.runtimeAcceptance` record, else from the `ports` of its module, else both (`fileDevices` in `tools/nx/device-tests.mjs`). A file that pins one device in its source (`devices: ['dt2']`, `describeOn('dn2', ...)`, `dn2Tests`) must declare it; `tools/nx/tests/device-halves.test.ts` checks this. `tests/regression:test-device` (stock oracles) is not split. While iterating, run one device with the half, e.g. `pnpm nx:pilot run <project>:test-device--<file>--dn2`, or with a Vitest name filter on a per-file or single-file target: `pnpm nx:pilot run <project>:test-device--<file> -- -t 'DN2 1.12'` (or `'DT2 1.17'`). The filter is part of the cache key, so a filtered result never stands in for a full one. It does not reach the files through the no-op `test-device` parent, and the ui-demo, ui-preview and conformance titles use lowercase device names.

A device that boots cold and has no ready snapshot (the Syntakt, `st`) is named by the file: `devices: ["st"]` in its `runtimeAcceptance` record gives `<target>--st` and nothing else (a file without `devices` still runs on `dt2` and `dn2` only). That half is not cached, because `dist/bytes` holds no daemon key, image hashes or patchkit key for it. The test starts the session with `dsp: 'cold'`; its daemon is the plain build of `tools.emulator.native_emu --daemon --build-only` on the stock image (`daemonBinary` in `tests/regression/runtime/devices.ts`; `DIGI_EMUD_ST` names a binary). The test runs its own `patchkit apply` for the plan (it imports `runtime/patchkit.ts`) because `dist/bytes/images.sha256` covers the acceptance devices only.
`--store ID,ID*,synth:NAME:BSS` boots the daemon on a card whose module store holds those modules (`*`: also in the saved selection; `synth:` needs BSS bytes of arena, so it never fits), which the MODULES list of `core/digikit-menu` needs; each list has its own kept daemon.

For a no-argument DN2 preview, `mise run dk:menu` opens the real DIGIKIT menu in a visible browser faceplate;
`mise run dk:ui-demo` opens the separate [`test/ui-demo` fixture](../tests/fixtures/modules/ui-demo/README.md), a content module with its own SETTINGS row (the tenth, after the system package's DIGIKIT row). See [visible previews](DEVELOPMENT.md#visible-preview) for the quick start. Both start or reload their kept daemon with the same automatic DSP selection as
`dk:dev`, navigate to the menu, then open `/panel.html` through the live Vite proxy on an available localhost port against that exact daemon.
The command prints the exact panel URL. `dk:dev` itself writes screenshots without opening a browser.
Keep the command running while using the panel. Ctrl-C stops its browser server and keeps the emulator for the next reload;
`mise run dk:dev -- emulator --stop` or `mise run dk:dev -- emulator --store 'test/ui-demo*' --stop` stops the respective emulator.
The commands also write `out/runs/devreload/dn2/digikit-menu.png` or `ui-demo.png`. Use `dk:dev` directly to choose DT2, load a particular module or card store, or provide another key script.

For a content module, `mise run dk:module ID [--device dn2|dt2]` packs its card `.dkm`, selects it for boot and opens the MODULES list in the live faceplate. It uses the `emulator` patch set. The module is packed in full; the DSP mode follows the module closure (ColdFire-only services run with zero DSP, machines and effects select SHARC automatically). `dk:module` delegates to generic `dk:dev --store`: the generic path stages content and compares the exact request and initial selection. Unchanged content keeps the daemon and overlay, including a user-edited live selection; changed content rebuilds the card and restarts that exact daemon. Stop and restart to restore the initial selected card after changing selection live. `mise run dk:module ID --stop` stops it. See [visible previews](DEVELOPMENT.md#visible-preview) for aliases and selection behavior. This interactive preview does not replace the module's acceptance checks.

`dk:dev` automatically reports and selects DSP `zero` for ColdFire-only module closures (such as `test/ui-demo`), skipping PCM
capture. `tools/patchkit/pk_devreload.toml` lists base patch sets whose menu/runtime supports a zero DSP
peer despite optional DSP adapters (`emulator` on both devices). Store module closures are always checked independently: adding
`--store digikit/pluck` selects coupled SHARC, as do content DSP objects, unknown object placements and arbitrary `file:` store content.

Services of the card come with the store. A service id in `--store` (a module with `role = "service"`, for example `core/services/machines`) is packed to a `.dks` (`patchkit services --pack-dir`) and put at `services/<name>.dks` with the port file of the device's firmware at `ports/<device>-<version>.dkport`. A stored module that needs a service gets that service without being listed: `patchkit services` ([PATCHING.md](PATCHING.md#which-services-a-module-needs)) reads the NEEDS items of the packed `.dkm` files and maps them to the service modules of the manifests; the device tests read its answer, which the pack target writes as `<leaf>.services.json` (`neededServices` in `tests/regression/runtime/service-files.ts`). So `mise run dk:dev -- emulator --device dn2 --store 'test/dyn-dn2*' --keys 'tap:SYN,shot:syn'` brings `machines.dks` along. The packed services and the port file are written to `out/cache/devreload-svc/<device>/` on each run (packed without firmware; nothing is bound). Not run live for this note (the machine was loaded).
Other patch sets, including `sample-tracks`, use the conservative object check. Each DSP mode has its own kept daemon directory;
the mode and audio status are checked after connecting and reloading. No additional flag is needed.

The daemon method is `dev.reload` (docs/CONTROL.md). It applies the modules again and compares the result with the running image:

| Path | When | What runs | Measured (PGO daemon, warm) |
| --- | --- | --- | --- |
| `swap` | only UI layer modules differ | restore the cached main-screen state, write the module bytes | DN2 1.4 s, DT2 1.7 s (apply 1.2 s, restore 0.3 and 0.4 s, write under 1 ms) |
| `boot` | the runtime, any other module, or a non-MAIN section differs | load the new image | cold boot DN2 5.3 s, DT2 10.2 s; image booted before: 0.3 s and 0.5 s |
| `none` | nothing differs | nothing | apply only, 1.2 s |

A `dk:dev` round for a UI change is about 3.6 s per device, with a two-key-step script of 1.8 s included. Build costs: Rust runtime 2.3 s,
C object about 0.3 to 1 s. The first run of a module set starts the daemon and boots the patched image (DN2 9 s, DT2 10 s with warm caches).
The harness stays the final proof: a swap shows what a change looks like and is not a substitute for `module:check`, the runtime suites or a
boot of the real image. `tools/emulator/boot_phases.py` measures where a boot of the workbench's patched image spends its time (cold and
warm `firmware.load`, `state.load`, first run of our runtime).

### RELOAD of a content module

A content module (a machine, an effect, an LFO shape, a code-only module) is reloaded without a boot: the new `.dkm` is uploaded to the path of the old one and the runtime's
`RELOAD` command (0x06, capability bit `RELOAD`) swaps it (docs/plans/extension-model.md E5). The reply only says the job is queued; `RELOAD_STATE` (0x07) gives `phase` (1 running, 2
finished), `result` (0 is ok), `step` and `unloaded` (1: the old module was torn down, so a result other than 0 means it is not loaded; 0: a refusal or an abort left it running). The command itself answers `E_NOT_FOUND` for a path no loaded content module came from and `E_FORMAT` for a service file. Clients: `DkSession.reloadAndWait(path)` (TypeScript, `{ result, unloaded }`), `Uploader.reload_and_wait(path, step)` (`tools/device/dkupload.py`, `(result, unloaded)`),
`wb transfer reload PATH...` and `wb transfer install ID... --reload` (the install, then a reload of each loaded module; a module that is not loaded or whose reload is refused goes
through the select and restart as before), and `dk:dev` with `--store` (a preview module that is loaded already is reloaded; a refusal or a failed reload falls back to the restart).

The runtime refuses a reload, with the old module untouched, in this order: `E_SAFE_MODE`, the transport playing or paused or unreadable (`E_RELOAD_PLAYING`), an open upload (`E_STATE`), `E_CARD_BUSY`, a module
that built menus of its own (`E_RELOAD_MENUS`), a module whose service refuses (`E_RELOAD_ROUTED`; no service refuses today: the fx service tears an effect down on any route),
then the file itself (any load check; another module name is `E_FORMAT`; a dropped key is `E_RELOAD_KEYS`; a plan failure over the live set is the plan's code, `E_MISSING` for a service
that is not loaded). While it runs the module's commands answer `E_BUSY`. A drain or teardown that does not end in about 4 s (or 1800 UI passes) gives the old module its tick and commands back (a teardown answers busy only before `fini`); a DSP loader that holds rows it could not clear is retained until the next boot, and every reload is then refused at once with `E_RELOAD_RETAINED`: restart the device. A DSP part that stalls (no image or row goes out for the budget) is cancelled and the job ends with `E_DSP_XFER`. A reload that fails after the old module was torn down leaves it not loaded, as a plain unload does. A service reload (`services/<name>.dks`) takes its consumers down with it: when it is torn (a busy restore that times out mid-closure, or a new file that fails after the take-down) the closure stays down until a restart, and the boot record says so (each consumer that was ACTIVE is FAILED with the reload's code, the service's code is in the log, `unloaded` is 1). Every job that got past the checks, whatever its result, rebuilds the machine list, writes gap rows for the machines that did not come back and frees the placeholder rows. A silent window (the machine's voice row is a
placeholder, the machine row reads MISSING) lasts from the teardown to the new row.

### LOAD and UNLOAD of a content module

The same job loads and unloads a content module without a boot (docs/plans/extension-model.md E5c; capability bit `LIVE`). `LOAD` (0x08) takes the store path of a `.dkm` that is on the card and that
no loaded module came from; `UNLOAD` (0x09) takes the path of a loaded module and leaves its file on the card. Both take the body and give the reply of `RELOAD`, and `RELOAD_STATE` (0x07) is the
progress of whichever job runs (its last byte `op`: 1 load, 2 unload, 3 reload) (steps: load check, load, DSP part, done; unload check, quiesce, unload, done). A load answers `E_LOADED` for a path that is loaded, a service file is `E_FORMAT`, and a
module needs a free selection index (`E_SEL_COUNT` at 8). A finished unload has `result` 0 and `unloaded` 1; after a load `unloaded` is 0. Clients: `DkSession.loadAndWait(path)` and `unloadAndWait(path)`,
`Uploader.load_and_wait(path, step)` and `unload_and_wait(path, step)` (`tools/device/dkupload.py`), `wb transfer load PATH...` and `wb transfer unload PATH...` (exit 1 when a path was refused or failed), and, from
the DIGIKIT menu, `dkrt_live(op, path)` with `dkrt_live_state(out)`.

Refusals, in the order of the reload list above, with the old state untouched: safe mode, the DSP retained, the transport, an open upload and card operations apply to all three; the module's menus
(`E_RELOAD_MENUS`) and the services' refusals (`E_RELOAD_ROUTED`; none today) to an unload and a reload; the file checks and the plan over the live set (`E_MISSING` for a service that is not loaded:
the service loads at the next boot) to a load and a reload. The `E_RELOAD_*` codes keep their names and also answer an unload and a load. An unload of a module that a track uses leaves a gap row, which plays the first
stock machine's sound, as after "untick and restart". The boot record then says `UNLOADED` (state 8) for that selection index; a later load of the same file takes the index its path had at the boot (else a free one that no boot path owns, else a new one) and
the numbers it had.

Host tests: the job over a scripted system and the loader's live load and unload (`pnpm nx:pilot run core/runtime:rust-tests`), the selection index rule (`boot::pick_index`), a real packed module loaded, unloaded and loaded
again (`core/runtime:test-assembly`), `packages/dk-session/tests/live.test.ts`, `packages/workbench-core/tests/reload-modules.test.ts`. Device tests: `runtime/core/tests/live.device.test.ts`
(the PR 1 proof of the plan: a code-only module loaded outside the boot selection, `test/sine` loaded and played with a pitch report, an unload with a note held; the refusals, counters and soak of the job are unit tests and the service contract suite; a sample-pool module loaded, bound, unloaded and loaded again: `modules/machine/digikit/shot/tests/shot-live-dt2.device.test.ts`, DT2).

Device tests: `runtime/core/tests/reload.device.test.ts` (reload of a code-only module, of a machine with a note held, the one real refusal while the transport plays, and the pitch soak (5 cycles, 20 with `DIGIKIT_TIER=full`): two builds of `test/sine` that differ in the DSP phase increment, made by `packModuleVariant` in `tests/regression/runtime/module-variant.ts` from a temporary copy of the module directory, put on the card alternately and reloaded with a note held). `reload-fx.device.test.ts` does the same for an effect on the master route and a send effect (level, knob value reaching the DSP row, swap windows, page open, send bus: the service's counts, residue and soak are the contract suite's cases 3, 6, 7, 11, 12 and 13), `reload-menus.device.test.ts` (the real core/ui reports the menu object of `test/entry-demo`, then the reload is refused; the counting itself is unit tests in `core/runtime`), `reload-project.device.test.ts` (DT2) saves a project with the machine, reloads it and loads the project. Host tests: `pnpm nx:pilot run core/runtime:rust-tests` (the job over a scripted system, the loader's reload steps, the placeholder rows)
and `core/runtime:test-assembly` (a real packed module reloaded twice), the service contract cases 9 to 12b (a reload takes over the parked row and a failed one leaves the gap row; `cargo test -p dkrt-svc-machines --test contract`; the fx service has its own reload case in `services/fx/tests/contract.rs`).

## Inputs

Tests that lack an input skip with a reason; `pytest -rs` lists them.

| Input | How to provide |
| --- | --- |
| Stock images in `firmware/` (DT2 1.17, DN2 1.12) | README "Firmware files". `DIGI_FIRMWARE_DIR` replaces the folder for every tool and test. `DIGI_EMU_SYX` and `DIGI_EMU_SYX_DT2` override the coupled tools' paths. |
| Sections `out/sections/<image>/` | `uv run python -m tools.firmware.extract firmware/SYX -o out/sections/<image>`. |
| SHARC database `out/sharcdb/<image>.sqlite` | `uv run python -m tools.sharc_dev.sharcdb build out/sections/*/section_7_BLOB.bin`. |
| ColdFire dump `out/ghidra/<image>-emac` and cfdb | Needs Ghidra: `tools.coldfire.ghidradump`, then `tools.coldfire.cfdb build`. |
| Native SHARC library, shared core | `mise run gen:native-lib` (firmware-free); `SHARC_NATIVE_LIB` overrides. `mise run gen:core` builds the generated core. |
| Module toolchains in `out/tools` | `mise run patch:toolchains` (about 15 min); `uv run python -m tools.patchkit.pk_toolchains status`. Selache builds `selas`, `seld` and `seldump` only: `selcc` does not compile at the pinned revision, and only two tests use it (they skip without it). On Apple silicon with an Intel Homebrew in `/usr/local`, the m68k build compiles for x86_64 (`CC`/`CXX` with `-arch x86_64`) so it links the Homebrew libraries; set `CC` and `CXX` yourself to override. |
| Objects of content modules and fixtures in `out/cache/dko` | Not committed. The Rust tests that scan the repository (`Library::scan_repo`), `module:pack`, the workbench app's library read (it packs the run-time modules of a device in-process), `dk:run`, `dk:dev` and the web gallery build the stale ones on demand (`patchkit build` does it by hand). Without the m68k-elf toolchain the Rust tests that need them print `SKIP` and return, except under `DIGI_REQUIRE_TOOLCHAINS=1` (set by the patchkit `rust-tests` Nx target), where they panic with the install command (`uv run python -m tools.patchkit.pk_toolchains m68k`). A checkout with an empty `out/` passes after the first build. |
| Ready inputs | `mise run emu:ready -- dt2` or `-- dn2` (about 2 min). Its helpers build against the shared core (`mise run gen:core` first; `SHARC_GEN_DIR` overrides). It writes to the `[acceptance] ready_dir` directories of the device files; the patchkit harness and the device tests read the same key. `DIGI_PK_READY_<DEV>_<VER>` overrides the directory for the harness and the tests. |
| Device generated cores `out/native/{dn2,dt2}-audio/gen` (AOT blocks) | `mise run perf:aot -- dn2` (about 4 min, then about 6 min for the first coupled build against it) or `-- dt2 --card-image IMG`; from the ready inputs, the SHARC database is built when missing. A DN2 core made otherwise (`sharc_dn2_aot`, from a capture) is kept: the tool refuses to overwrite it. Regenerate after a `CORE_FINGERPRINT` change. |
| Captures `out/captures/*.dt2cap` | Not reproducible from the repository; those tests stay skipped. |
| Opt-in switches | `LIVE_AUDIO_DEVICE_TESTS=1` opens a real audio device; `--slow` runs long firmware runs. |

On a fresh clone, prepare the inputs in this order (DN2 shown; DT2 is the same with `dt2` and its `.syx`):

```sh
mise run patch:toolchains            # module compilers (about 15 min)
mise run gen:core                    # shared generated core
mise run emu:ready -- dn2            # ready inputs, built against the shared core (about 2 min)
mise run perf:aot -- dn2             # device generated core (about 4 min)
pnpm nx:pilot run digi-control:daemons   # daemons for the device tests
```

A contributor with one device's firmware only can run `mise run module:check ID --device dn2` (or `dt2`): it resolves the
daemon of that device and runs only its halves of the device tests.

Device suites (`test:stock`, `test:fx*`, the Nx `test-device` and `test-conformance` targets) also need:

- Pinned daemons: `DIGI_EMUD_DT2` and `DIGI_EMUD_DN2` point to built `digi-emud` binaries
  (`mise run emu:serve -- --build-only`). `test:stock` takes `DIGI_EMUD`. The gate sets these from
  `build-daemons`.
- Complete ready inputs for each device, including the validated DSP files.
- Module artifacts for the device targets: `dist/modules/<device>/<leaf>.dkm`, built by the Nx `pack` and `pack-aliases` targets.
  `DIGI_RUNTIME_MODULES` (optional) overrides the directory. The gate packs the fixtures of the selected records and the modules named in `DIGI_CONFORMANCE_MODULES` with one `nx run-many`.
- `DIGI_PATCHKIT` (optional) names the patchkit binary. The Nx targets and the gate use `dist/patchkit/patchkit`; without it the tests build patchkit once through `patchkit.py bin`.
- DT2 card samples at `out/runs/patchkit/samples`.
- plusdrive: `test-device` builds it through `plusdrive-format:cli` (`dist/plusdrive/plusdrive`, also `DIGI_PLUSDRIVE`). The gate builds it before samples-live.

FX route and track SEND acceptance runs in the device tests of the owning modules
with `pnpm nx:pilot run <module-id>:test-device`. `tests/regression/runtime` keeps the shared helpers only. The legacy Python `fxroute` and `fxroutes` scenarios are retired;
their offline data and comparison helpers remain.

Details of the runtime and module fixtures: [tests/regression/README.md](../tests/regression/README.md).
FX coverage: [tests/regression/scenarios/README.md](../tests/regression/scenarios/README.md).
All device suites share `tests/regression/vitest.config.ts` with the projects `stock`,
`runtime`, `conformance`, `fx-dn2` and `fx-dt2`. A device project runs one file at a time. Moved tests
import helpers as `@digi/regression/<path under tests/regression>` and packages as `@digi/test-support`,
`@digi/dk-session` and `@digi/control-client`; the config and `tsconfig.json` resolve them for files
under `modules/` and `tests/conformance/`.

## Layout

| Directory | What it tests |
| --- | --- |
| `tests/sharc/{semantics,trace,generation,analysis,integration}` | SHARC+ semantics, tracer, assembler and generator, loader and sharcdb, firmware-backed harness and golden hashes |
| `tests/patchkit` | Patchkit's own tests and module primitives. `smoke/` holds the smoke matrix (`matrix.toml`, `tours/`, `goldens.json`). |
| `tests/test_*.py` under a module | Python tests whose subject is one module (DSP voice rig, FX, sample pool, proofs, PLUCK). An edit to the module selects them. |
| `tests/` under a module | A module's own tests, including `*.device.test.ts`. See [Module-owned tests](#module-owned-tests). |
| `tests/coldfire` | ColdFire analysis tools |
| `tests/emulator` | Launcher, PGO, coupled runner helpers, live audio, bootstrap |
| `tests/control` | The control-socket client and daemon |
| `tests/firmware` | `.syx` container, device profiles, snapshots, +Drive format |
| `tests/dsp` | DSP maps, FX scenario generators, audio checks |
| `tests/devtools` | Benchlock, agent guard, render-cache audit, test outcomes |
| `tests/digikit` | Digikit boot scenarios (below); data in `scenarios/*.toml`, plus `modules/*/*/tests/scenarios/` |
| `tests/regression` | Native Vitest: `dt2/` (stock), `emulator/` (emulator oracles), `runtime/` (helpers only), `scenarios/` (FX), `host/` (no device) |
| `tests/conformance` | The shared module-load suite (`load-dt2.device.test.ts`, `load-dn2.device.test.ts`, `load.ts`) |
| `tests/scenarios` | Stock scenario inputs and PCM pins |
| `tests/support` | Shared helpers, not collected; `paths.py` gives `REPO_ROOT`, `TESTS`, `TOOLS` |
| `tests/golden`, `tests/fixtures` | Reference hashes and small fixtures |

Other owners: `apps/web/tests` (web Vitest tests), `packages/control-client/tests`,
`packages/dk-session/tests`, `packages/test-support/tests`, `apps/desktop/ui/tests` (host Vitest), and
Rust tests in their crates.

## Writing a test

- Put the test in the directory of the code it tests.
- Import repo paths from `support.paths`. Never count parent directories.
- Import tools as `from tools.<owner> import <module>`. pytest puts the repository root and `tests/`
  on `sys.path` (`pythonpath` in `pyproject.toml`). Never change `sys.path`. A subprocess runs
  `[sys.executable, "-m", "tools.<owner>.<module>", ...]` with `env=python_env()` (`from tools import python_env`).
- Share helpers through `tests/support`. Never import another test module.
- Test file basenames are unique across `tests/`. There is no `__init__.py`.
- Mark long firmware runs `@pytest.mark.slow`. They are skipped unless `--slow` is given.
- A test that needs firmware or generated artifacts skips with a reason when they are absent.
- Lint and types come from `pyproject.toml`: new Python files are covered by `[tool.ruff] include`;
  add new typed files to `[tool.mypy] files`. Both run as commands in the gate target `python-checks`.

The patchkit harness shares boot snapshot pairs in `out/runs/.cache/patched`. The first harness call of a process
caps it at 30 GB (`DIGI_PATCHED_CACHE_MAX_GB`): it removes the oldest entries first by last write time and never an
entry written in the last 2 hours, so concurrent runs and lanes keep what they use. The harness cannot tell which
entry the daemon restored, so a reused but old entry can go and is rebuilt on its next use (about 13 s). `cards-store`
(a few entries) has no cap.

Device sessions run in a claimed `dist/session-cache/<key12>/<device>-<n>` (per worktree, gitignored).
`<key12>` is the first 12 characters of the daemon build key (the directory name of the binary in
`<git dir>/emud-cache/<dev>/<key>/`): a boot snapshot has no version, so a new daemon build must not read the
snapshots of an older one. A daemon from another place (a custom `DIGI_EMUD_<DEV>`) uses
`dist/session-cache/<device>-<n>`, or for the card and fx fixtures a fresh temp dir. A vitest worker process
claims the first free `<n>` with a pid lock file (`<device>-<n>.lock`) and keeps it until it exits; later
processes reuse the dir. The boot snapshots are not in the run dir: all run dirs of a daemon build and device, in all
worktrees, share `<git dir>/snap-cache/<daemon key>/<device>` (the daemon's `DIGI_SNAPSHOT_DIR`; `DIGI_SNAP_STORE` replaces
`<git dir>/snap-cache` for host tests). An entry is named by its content key (firmware, card identity, DSP state). The card identity
is the card's content id (`card.key` next to `card.img`, written by `cachedCard`), its length and the sha256 of its `overlay.bin`: no path
and no mtime, so the same card gives the same key in every worktree. A card without `card.key` falls back to path and mtime and can
only miss. A card boots cold once per daemon build and any later file, run or worktree restores it (about 0.2 s). A coupled cold boot
writes its pair into a `.tmp-*` dir of the store and publishes it with one directory rename; the first writer of a stem wins. A claim
prunes the whole `snap-cache` to 20 GiB (`SNAPSHOT_CAP_GIB_DEFAULT` in `packages/test-support/src/emulator-config.ts`; env `DIGI_SNAP_CAP_GIB` overrides it), oldest entries
first (a hit touches its entry), at most every 10 minutes. An entry used in the last 2 hours is never pruned. Entries are removed by
renaming them aside first. A file that fails removes the entries that its own cold boots wrote. Migration: `rm -rf dist/session-cache/*/snapshots`
frees the old per-worktree stores. The card
and fx fixtures claim a dir when their daemon starts (not at collection), empty everything but `cards/`, and release it when the session closes. A file with a failed test or a failed post-close safety check
renames its dir to `<dir>.failed-<time>` (the newest 2 per device stay), so the next claim boots cold: a snapshot
of a boot that failed a check would otherwise pass the next run. A `.dirty` marker, written at the claim and removed by a clean release, does the same for a killed or timed-out run. Only the newest 2 `<key12>` dirs of each device stay.
Cards that the fixtures build (`cardRig`, the fx presets) are content-named: `dist/session-cache/cards/<key>/`
holds `card.img` and `overlay.bin`, read-only. The key is a hash of the spec, the bytes of every input file
(`.dkm`, `.syx`, the settings tool), the plusdrive binary, the system package, `devices/*.toml` and a format version in
`tests/regression/runtime/card-cache.ts`. Regression fixtures and the Workbench reference CLI use the same [base-image builder](../packages/workbench-core/src/node/card-image.ts). It assembles the seed and invokes PlusDrive, which writes the verified filesystem superblock. The regression wrapper owns its selection overlays, injected sectors and alternate boot paths. Every card carries the system package `digikit/system.dksp` (built by the Nx target `core/runtime:package`, a dependency of every device target). `buildCard` installs it, including on a copied `folder` card. `systemPackage: false` leaves it off for a test of a card without it. The FX seed cards use the same installer. A changed package or builder changes the card key and boot snapshot identity. A test that lists the store expects the package in the listing (`SYSTEM_PACKAGE_STORE_PATH`, `systemPackageFiles()`). `card.key` holds this key. Equal inputs give the same content id, so the daemon's
snapshot key matches and a baseline reload restores the snapshot (0.2 to 1.5 s) instead of a cold boot
(14 s DT2, 38 s DN2). A card built on a `base` outside the cache (a card after the guest's writes) is built in a
fresh dir and always boots cold. Eviction runs when a fixture claims its run dir: only the newest 32 card dirs
stay (a load touches a dir; one used in the last 2 hours stays). Fx presets with a `seed-card` settings step are built in the run dir, not cached. Delete `dist/session-cache/cards` to rebuild.
`liveOptions(..., { fresh: true })` and mirror sessions use a fresh temp dir.
Live patched sessions (`liveOptions`, used by `runtimeTests`, `withPatchedSession` and the conformance loads) that boot start on a cached card of `live-card.ts`: the empty store, the sample folder `out/runs/patchkit/samples` with the boot project (a device whose `[capabilities] card` is `path-fs`; the folder goes to `plusdrive build` as it is, the scan `card.load` uses), and a selection record whose `stable_blocks` is 1500 (1 s of DSP blocks, `LIVE_STABLE_BLOCKS`). The runtime's default stable period is 30 s, so a test that boots three times quickly (transfer, lfo) would otherwise reach the safe-mode threshold. The period counts from the first UI-loop pass and the record keeps it through every later selection (`next_record`). Tests of the real 30 s rule build their own cards (`safe-mode`, `boot-restart`). `stableBlocks: 0` gives a test of the boot counter the runtime's 30 s (fault-trace-dt2). `blankCard: true` (the card-guard test of the stock initialisation) leaves the card to the daemon. Sessions with `runtime.reuse` return before the card step and restore a booted snapshot, so they keep the card of that snapshot. `SessionOptions.cardImage` hands the daemon a built image at `firmware.load` (instead of `card.load` of a folder).
A file that uses `withReusablePatchedSession` shares one session between its tests. A failure stays local: the failed test closes the session (diagnostics kept), and the next test starts a fresh session (one extra boot, only after a failure).

## Module-owned tests

A module keeps its tests in its own `tests/` folder. Content modules use `modules/<kind>/<author>/<name>/tests/`. Host test discovery scans the module roots.

- `test_*.py`, also in subfolders: pytest files. The basename is unique across the repository (prefix it with the module name). A helper
  next to the test is imported by name. Use `Path(__file__).parents[1]` for the module folder.
- `*.test.ts`: Vitest project `module-tests` of `vitest.host.projects.json` (`pnpm test:host`), found recursively under the module roots.
- `*.device.test.ts`: device tests, registered in the owner's `project.json` (`metadata.runtimeAcceptance`, files relative to that
  project root) and run by Vitest's `runtime` project. Runtime acceptance is split by concern: `core/runtime-tests` (the package: reload, live, missing; `runtime/core/tests/project.json`),
  `core/runtime-boot-tests`, `core/runtime-transfer-tests`, `core/runtime-diag-tests`, `core/runtime-platform-tests`,
  `core/runtime-storage-tests` and `core/services/samples-tests` (each in the folder of that name), with CF-only service commands and settings in nested
  `core/runtime-service-tests` and `core/runtime-settings-tests` (`service-commands/project.json` and
  `settings/project.json`). Each owner's `test-device` runs its files;
  `mise run module:check core/runtime` discovers and runs all owners serially.
  `pnpm nx:pilot run <module-id>:test-device` runs one module's files; `pnpm nx:pilot run <module-id>:test-conformance`
  loads the module through the shared conformance suite. Uploadable modules without a DSP image run
  with the zero SSI peer and no SHARC audio session; modules and services with a DSP part (a `.dks` with a DSP section, such as the DT2 `machines.dks`) retain coupled execution
  (`dsp: 'sharc'`) and the DSP registration wait; the zero peer never acknowledges a DSP transfer, so the card rig and `installModules` refuse a service with a DSP part in a session that is not coupled (`requireCoupledForServices`). Both paths retain upload/restart, persisted bytes/hash, SDK contract,
  registration, UI and fault acceptance, including the same forbidden runtime log kinds. Task and
  cache ownership remain per module.
- `scenarios/*.toml`: digikit scenarios. `pk_digikit_scen list|run NAME` and the tiers find them in `tests/digikit/scenarios/` and `modules/*/*/tests/scenarios/`.
- `module:check ID`, `gate` and `test:changed` select them. A change in the module selects its tests and those of the modules that depend
  on it; a change only in `tests/` runs python-tests (and `host` for `.ts`), no Rust or web step.
- A scenario that pins the PCM of a module lives with that module. A shared test needs "a module" only: it uses a fixture of
  `tests/fixtures/modules` (`test/sine`, a dynamic machine on both devices; `test/dyn-fx-gain`, an effect; the table is
  [Fixture table](#fixture-table) below), or a made-up id such as `acme/demo` when nothing is built. A list of all shipped modules is a scan of
  `modules/{machines,fx,tools}/*/module.toml`, never a literal list.
- `mise run check:layers` (rule `self-contained`): no tracked file outside a module's folder names its id or path. Docs, `*.md`, `.agents/`
  and the fixtures may. `host = true` in `[module]` exempts a module the workbench speaks to (the remote control service).

### Fixture table

`tests/fixtures/modules/<id>` holds the test modules (author `test`). Each is the one canonical subject of the cases in the second column; a new test
reuses a fixture before it adds one. Layers: **host** (Vitest or pytest, no device), **assembly** (runtime assembly tests and patchkit Rust tests), **rig**
(SHARC-only rig), **device** (emulator). A fixture marked *patch-time* is applied to the firmware by patchkit (`[[prim]]` or core patches), not loaded from the card.

| Fixture | Canonical subject for | Layers |
| --- | --- | --- |
| `sine` | The small dynamic machine of the SDK contract (pitched sine, one LEVEL parameter, both devices); the DSP source other fixtures share; pitch checks | host, assembly, rig, device |
| `dyn-dn2` | A machine with four parameters of different ranges on the DSP of `sine`, on both devices (Digitone II: its own SYN page of four knobs): registry rows, needs inference, capacity, project round trip | host, assembly, device |
| `dyn-fx-gain` | The effect contract: insert effect, GAIN knob, upload, persistence, kit save; the trace marks (`DK_TRACE_LOG`, `DK_STEP`) that exist only in a `--trace` build (zero-cost proof `trace.rs`); owns the gain DSP the wrappers share | host, assembly, device |
| `dyn-fx-gain-b` | A second effect with the same gain under another module id (another effect id) and other defaults (0.25, MIX 64): two effects in one kit; wrapper of the DSP of `dyn-fx-gain` | host, device |
| `dyn-fx-gain-bipolar` | A signed knob (BIAS, -64 to +63) and the routes master and send | device |
| `dyn-fx-gain-both` | Routes: master and send allowed, default master (`dk_fx_routes`); wrapper of the DSP of `dyn-fx-gain` | device |
| `dyn-fx-gain-send` | Routes: master and send allowed, default send; the FX ROUTING popup; wrapper of the DSP of `dyn-fx-gain` | assembly, device |
| `dyn-fx-gain-sendonly` | Routes: send only; wrapper of the DSP of `dyn-fx-gain` | assembly, device |
| `commit-demo` | A multi-feature commit: a machine with three parameters (latched, click action) and an effect with routes; the golden lowered records; refusals | device, rig |
| `local-ping` | The provider fixture: a `.dks` of the service `local-ping` (export `local_ping_value`, resident query `dk_service_query_v1`) over an owner object with the lifecycle, tick and query parts; it counts the consumers that attach and detach, the ticks, and answers the counters | host (`provider_contract`), assembly, device |
| `local-step`, `local-step-user` | The owner's two-phase call (`Owner::step`) on the device: the boot's first step makes one nested call inside the owner (refused, counted) and its act re-enters the service; the user is the client that imports its export so the provider loads; a thin wrapper of the source of `local-ping-user` with the export renamed | device |
| `local-ping-user`, `local-ping-user-b`, `local-ping-old-user` | Consumers of `local-ping` (the NEED path; the old user has a minimum version the service does not meet) | assembly, device |
| `local-patch` | A `.dks` of the service `local-patch` that takes one `[[patch]]` of the firmware: two bytes (the dead size bound `cf.filter.site.test_bound`, a code site that changes no behaviour), written at load with a value its start computed and restored at unload; the refusals of the patch entries (stock check, declared length, value that does not fit) | host (`svc-rt`), assembly, device |
| `local-patch-user` | The consumer of `local-patch` (a hard need that loads it); a thin wrapper of the source of `local-ping-user` with the export renamed, a module of its own because it imports another service | assembly, device |
| `svc-test` | The service fixture: a `.dks` of the test-only service `svc-test` with exports `dk_svctest_ping` and `dk_svctest_hits` and one hook | host, assembly, device |
| `svc-test-user` | A module with a hard NEED on `svc-test` (the provider and NEED path of the loader) | host, assembly, device |
| `lfo-shape` | A service-type module that emits one `lfo_shape` record (key from `DK_KEY_SHAPE`); a test makes a second file with the same key by renaming a copy | assembly, device |
| `lfo-shape-b` | A second shape (name STEP) under another module id, so another shape key: two shapes in one boot (numbers 7 and 8), a gap below the highest number; wrapper of the code of `lfo-shape` | device |
| `shortcut-demo` | A module with a hard NEED on the `shortcuts` service: one `dk_shortcut_add` call, hold [TRK] and press [YES] (the record, the callbacks as image offsets, the card without the service) | host, assembly |
| `entry-demo` | The module entry point: `dk_module` with `init`, `tick`, `fini`, `menu`, settings, log, time, an absent optional service; the module that owns a menu object (its `menu` function builds a submenu with `dk_ui_menu_new`, the case of `E_RELOAD_MENUS`) | host, device |
| `ui-views` | The module screens of core/ui: a content module that adds two SETTINGS rows (`dk_ui_settings_row`), opens a full-screen view (frame, text, key handling), the stock text input (`dk_ui_text_input`) and an overlay; an unload closes its view and rows | device (`tests/fixtures/modules/ui-views/tests/live/`) |
| `dyn-demo` | A raw module loaded from the card store with a counter in its `tick`: load, unload, store format, names | host, assembly, device |
| `dyn-demo-stray` | The ColdFire memory audit: `dyn-demo` that writes one word outside its block | device |
| `files-proof` | The public read-only listing boundary of the files API (deferred notifications, no tick) | host, device |
| `file-write` | Store file writes from an I/O job (create, write, close, abort, delete, hash) started by service commands; cut and PlusDrive cases | host, device |
| `file-write-b` | A second copy of `file-write` under another name and command range (two modules writing) | device |
| `settings-a` | The module settings API: put, get and remove through service commands, read in `init`. Also the public Workbench settings descriptor. | assembly, device |
| `settings-b` | The same code as `settings-a` under another module name and command range (two modules, one key) | device |
| `dyn-crash` | The one fault injector, set by the store file `cfg/crash-after.bin` (delay, mode, place): a CPU exception, a reset or a hang a number of blocks into the UI loop (crash capture, safe mode), a reset in `init`, a fault in the boot animation callback (`bootscreen`); no file, no fault, so each test asserts the fault happened | assembly, device |
| `dyn-dsp-hang` | A machine whose DSP routine never returns after a few blocks (fault trace) | device, rig |
| `dyn-fx-hang` | A send effect whose process routine never returns (fault trace, log follow) | host, device, rig |
| `dyn-fx-nan` | A send effect that outputs NaN: the fault count and the row switch-off after eight faults | device, rig |
| `dyn-guard-nan` | A machine that outputs NaN while a voice is active: the voice guard of `core/dsp-voice` | host, device, rig |
| `stock-pool` | Calls to the stock DT2 sample loader, reload and purge from the command task and the loader worker | host, device |
| `track-select` | Adds a sample to the pool of another module's table through SysEx commands | host, device |
| `cal-kernels` | Five machines of synthetic SHARC+ kernels with known instruction counts (alu, dm, mem, branch, mix) for the calibration profile `kernels`; its machine rig runs `module#id` specs and is not in the conformance suite (the kernels run about 20,000 instructions a block, over the voice budget) | host, rig |
| `dsp-facts` | Probe: cycle counts, instruction semantics and core register values on the SHARC+ core, one framed word per block | host, device |
| `sharc-probe` | Probe: RECIPS and RSQRTS over a fixed sequence, one framed word per block | host, device |
| `step-beacon` | Probe: stores a call counter in the module step word on every call | device |
| `ui-demo` | A content module: its SETTINGS row opens a menu with one row of each kind, text page, list, side panel, confirm, choice and toast (`dk_ui_settings_row`) | device |
| `native-page` | Native callback page cycling, parameter edits, local zoom and clipped drawing | host |
| `usb-echo` | The bulk interface of `core/usb-bulk`: echo of every buffer on EP2 IN from an I/O job | host, device |
| `input-probe` | The input adapter: SysEx commands press keys and turn encoders through `core/input` | device |
| `svc-echo` | Service commands: `dk_host_register` and an echo of the request body | host, device |
| `intro-ticks` | The boot animation takes: the tick hash (the faults in the frame function are `dyn-crash`, place 2) | device |
| `card-info` | Read-only eMMC and eSDHC register diagnostic of the card tool | host |
| `c-demo` | A C module that needs 64-bit division, remainder, multiplication and shifts (libgcc helpers); inferred needs | host, assembly, device |
| `synth-decl` | The code of the synthetic capacity modules (`packages/patchkit/src/dkmsynth.rs`) | assembly |
| `cf-demo` | *Patch-time.* Code, data and bss placed in `cf.expansion` | assembly, host |
| `rust-demo` | *Patch-time.* A Rust crate in `cf.expansion` | assembly |
| `registry-demo` | *Patch-time.* The generic primitives: a registry, a section copied from the image with appended rows, a counting hook | assembly, host |

## Byte manifests for repository moves

Install the development dependencies and module toolchains using the [README setup](../README.md#setup). Provide DT2 1.17 and DN2 1.12 stock firmware in `firmware/`, or set `DIGI_FIRMWARE_DIR`. From a clean checkout of the baseline revision, run:

```sh
node tools/nx/byte-manifest.mjs write
```

The command writes `out/e8/manifest-dt2.json` and `out/e8/manifest-dn2.json`. Keep both files. Use `write --baseline DIR` to save them somewhere shared with another worktree. Once the moved paths build, run the comparison in a clean checkout:

```sh
node tools/nx/byte-manifest.mjs compare --baseline DIR --move-map moves.json
```

Omit `--baseline DIR` to read `out/e8` in the current checkout. The move map is a JSON object from old to new repository-relative file or directory paths, for example `{"modules/core/runtime":"runtime/core"}`. The most specific source path wins. It applies to baseline keys for generated files, tracked objects and entries in the core object checksum list. It does not rewrite paths embedded in artifacts.

For E8, write the baseline at `54529ca54`, the parent of the rename-only commit `7fc7827b5`. Use the [E8 directory map](plans/e8-moves.json) from the comparison checkout.

```sh
node tools/nx/byte-manifest.mjs compare --baseline DIR --move-map docs/plans/e8-moves.json
```

Each manifest records the device's image, plan and address-map hashes for its patch sets and static acceptance root sets, and its packs (including aliases). Both record the pack verdicts, every output checked by `patchkit gen --check`, every tracked `build/*.dko.json` and the entries in `dist/bytes/core-objects.sha256`. The tool also rebuilds each tracked object in a temporary directory and records the fresh hash. For the tracked and fresh objects it masks only the value of `source_sha256`. The generated-file list comes directly from `patchkit gen --check --paths`.

If preparation refreshes a tracked object, the command reports its path and exits. Review the object diff before running the comparison again. A matching comparison prints both devices and exits zero. Missing, unexpected, stale or changed artifacts fail the command. Both commands prepare packs, images and checksum lists through their Nx targets, with the regular cache and build locks. They start no emulator. Store the reports under ignored `out/` or outside the repository.

## The gate

### Gate selection

Nx selects by project. `pnpm nx:pilot show projects --affected --base main` prints the selection
without running it.

- The gate projects `gate-smoke`, `gate-card`, `gate-stock`, `gate-fx-dn2`, `gate-fx-dt2`, `gate-pins`,
  `gate-fast-roots` (`fast-roots`), `gate-dt2-tone` and `gate-rust-modules` (in `tools/gate/`) carry their trigger paths as positive
  `{workspaceRoot}` globs in their target inputs. Nx affected ignores negated (`!`) inputs, so inputs
  that decide selection must be positive globs.
- `gate-fast-roots` replays fixtures built from the current runtime and fails when interpreted guest instructions per audio second (or, where a budget sets it, generated-block entries per audio second) exceed `tools/emulator/fast_budgets.json`, the sign that runtime patches bar hot generated regions or the generated core lacks links for hot blocks. DN2 has two cases: `first-boot` (the state right after the first boot) and `reset` (the state after a guest soft reset, which installing modules and Apply & Restart boot into another kit with effect sends active; about 40% more host work). DT2 keeps its first-boot state, so it has one case. `gate-fast-roots:fixtures` builds the fixtures first, with the daemon from `digi-control:daemons`; the fixture key covers the project's `fixtureKey` input (no tests or Markdown), and the store keeps the newest 4 keys per device.
- Module projects with device tests depend on `regression` (harness), `digi-control` (daemons),
  `patchkit` (packs) and `devices`; shared SDK contract/compiler surfaces are explicit target inputs.
  Only modules with `test-conformance` also depend on `conformance`. A change in a dependency selects the modules that use it. The module targets
  `test-device` and `test-conformance` hash `^production` of their dependencies. `production` is the
  named input in `nx.json` (default minus `{projectRoot}/tests/**` and Markdown); for `regression` it is
  the harness only (see `tests/regression/project.json`). So a change to a stock test, a golden, an FX
  scenario, a host test or another module's tests still selects the dependent modules, but their device
  results replay from the cache. Packs still run (they are not cached).
- Build and pack inputs share `sdk/sdk.toml`, `sdk/dsp` (including `sdk/dsp/contract`) and `runtime/core/vectors`. From `sdk/include` a pack or object build reads only the headers its sources include: `packSourceInputs` (`tools/nx/pack-inputs.mjs`) scans `#include` and `.include` over the build sources and the headers they reach, with the lookup order of the build (the including file's directory, the modules' `build/gen`, `src`, `include`, then the dirs of `tools/patchkit/toolchain/pins.toml`). A computed include is not seen. An object build (`build-objects`) reads the same source inputs plus the patchkit production, not the production of every graph dependency. Full-input device tasks do not
  hash them; they hash the built bytes (see "Cached targets"). `tools/devtools/emu-slot.mjs` is no device input: it only schedules.
  The SDK sources under `sdk/src` are linked into every content module with C sources (the packer builds them into one library and
  keeps the functions the module calls), so `sdk/src/**/*` is an input of the pack of each such module, and the stored object of
  each content module carries the library digest in its `source_sha256`. A module whose bytes do not change keeps its device
  results (full-input device tasks hash the built bytes). `dkui-host:rust-tests` names the picker sources and headers compiled by
  its build script. Module and Cargo source ownership remains project-wide. Public headers and the SDK contract retain broad selection.
  `sdk:check-generated` names the shared generator inputs and all generated output surfaces, including outputs
  outside `sdk`; changes to generator output paths must update this list. It excludes `sdk/src`, `sdk/templates` and the hand-written
  `sdk/dsp` files (only the generated includes `dsp/contract/dk_{machine,fx,sample_pool}.inc` are inputs). `sdk:test-host` does not hash `sdk/src` either:
  its tests compile public headers and fixtures only.
- `tools:python-checks` and `tools:python-tests` run on changes to tools, tests, modules, sdk, devices, Python and
  Cargo manifests, and the patchkit, dkneeds, control and plusdrive packages. `python-checks` runs ruff, ruff
  format, mypy, layercheck and `patchkit gen --check`. `python-tests` first runs
  `tests/patchkit/test_pk_current.py` (module and port staleness), then the Python test files that
  `tools/devtools/py_select.py` selects by the diff: the gate test (`PY_GATE`), the `PY_SUITES` the
  change triggers, the tests that reach a changed module in the import graph, and the changed tests.
  One process runs one file.
- Inputs name the toolchain by version (runtime inputs such as the pinned `rustup run <version> rustc -vV`, node versions), not by
  `mise.toml`, so task edits in `mise-tasks.toml` do not select every project.
- If the selection is wider or narrower than the change can reach, fix the inputs and edges in
  `tools/nx/*.mjs` or the `project.json` files.
- The merge gate uses `nx affected` to select conformance and the other targets from changed declared inputs, including dependency outputs; device tests are selected by ownership ("Nx gate"). A selected task with an unchanged key replays its cached result. Use `nx show target inputs PROJECT:TARGET --check PATH` to inspect direct file inputs and `mise run test:plan` to see task selection. After a change to untracked firmware, snapshots or override binaries, or to what `tools/nx/device-inputs.mjs` or `coupled-inputs.mjs` hash, run named targets or the full tier.
- Shared loader and public SDK edits select the common machine and effect load fixture through the patched image and retain the other affected device behavior checks. Changes to an individual machine's or effect's private pack inputs select that shipped artifact's own load, as described in “Nx gate.” Use the owner targets for iteration and inspect `mise run test:plan` before the merge gate.

### Remote cache

All worktrees of a clone share one Cacheiro server on `127.0.0.1:47311`. Its state is in
`<git common dir>/nx-remote-cache` (config with a read/write and a read-only token, mode 0600).
Plain `pnpm nx:pilot run ...` and every gate tier read and write the cache when the server already answers
(`, remote` on the stderr line); only the gate starts the server.
`--skip-nx-cache` or `DIGI_NX_REMOTE=0` turns it off. `tools/nx/remote-cache.mjs` has `status` and `stop` subcommands
and sets the Nx variables only when the server answers.
The store has a 14-day TTL swept daily and no size cap.

Cached targets: module `test-device` and `test-conformance`, `gate-stock:stock`, and the FX targets of `gate-fx-dn2`
(`fx`, `fx-no-aot`, `tier-quick`) and `gate-fx-dt2` (`fx-dt2`, `dt2-tier-quick`; the FX key is described below). A key covers what the run consumes: the
TypeScript import closure of the test files and of the Vitest setup file (`tools/nx/ts-imports.mjs`), the
built bytes (full set; the narrow set of a file outside `mergeTests` is described above): `patchkit:device-bytes` writes
`dist/bytes/core-objects.sha256`, one line per committed core object (`{runtime,services}/**/build/*.dko.json`) with its hash without
`source_sha256`, the only field that follows the source text (a comment edit changes no other field; checked for Rust, C and assembly).
The patched image is a function of these objects, the core manifests, the device files, the patchkit code and the firmware. A
merge key (the target of a file in `mergeTests`) hashes the result instead of the patchkit code (a narrow key holds none of these files: the image hashes and daemon key of its device, the packs the test loads with their `.services.json` and `.addrmap.json` sidecars, the device and port files of its own device (`devices/**/*.toml`, `ports/**/*.toml`, the other devices' files left out like in a merge half; a patchkit-live test also holds the core `module.toml` manifests patchkit reads), `pack-verdicts.json` when it imports `pack-needs.ts`, and the patchkit key of its device when it is live):
`patchkit:device-images` runs `patchkit apply` for each acceptance firmware (`CELLS` of `tools/nx/device-inputs.mjs`) with each patch set of
its device file and writes `dist/bytes/images.sha256`, one line `<device>-<version> <set> image <sha256> plan <sha256> addrmap <sha256>`
(the plan and the address map are what `runtime-plan.ts` reads), and the lines of each device again in `dist/bytes/images-<device>.sha256`:
the key of a half hashes the file of its own device only. `digi-control:daemons` also writes `dist/bytes/daemon-test-<device>.key`, the
daemon key of that device without the patchkit sources and without the files of the other devices: the repository files the daemon build read (its cargo dependency file, kept as `files.txt` in
the store entry), the crates of its dependency closure (`cargo tree`, not the whole `Cargo.lock`), the generated core, the profile, rustc and the
flags (`emud_key.test_key_of`, written by `verify_build daemons`). An unrelated workspace member leaves it unchanged; an emulator source edit
changes it. The daemon store key is not touched by it. `dist/bytes/daemon-test.key` lists the keys of all devices, one line each; no cache key reads it.
So a patchkit edit that changes no packed byte (image, plan, address map, `.dkm`, core object) selects no merge target. A test that runs
patchkit follows `dist/bytes/patchkit-test-<device>.key` in its merge or narrow key and runs with `DIGI_PATCHKIT_LIVE=1`. `patchkit:cli` writes that file
for each device after the build (`tools/nx/patchkit-key.mjs`): one sha256 line per repository file that the build read (the dependency file
`dist/cargo/release/patchkit.d`), without the files of the other devices (`tools/nx/device-files.mjs`) and without test-only Rust, plus the manifests,
`Cargo.lock`, `.cargo/`, the device's own files (device file, peer file, ports), `rustc -vV` and the profile and rustflags environment. Paths are
repository-relative, so lanes and the remote cache share keys. Any edit of a listed source changes the key, a comment too: the binary key ignored a
trailing comment but not a new first line, which moves panic locations (measured). The script fails when the dependency file is missing, and so does `patchkit:cli`. The rule for a live test is the import closure: it
holds `tests/regression/runtime/patchkit.ts` (the helper every module that runs patchkit imports: `dkm.ts` for `needs` and `fit`, `synth.ts`,
`addrmap.ts`, `runtime-plan-modules.ts` for `apply --modules`, the opt-in proofs through `optInPatchkit`), or the record patches fixture
modules (`patchModules`). Only `runtime-plan.ts` is exempt: its `apply --set` result is in `images-<device>.sha256`. All test code starts patchkit in
`runtime/patchkit-exec.ts`, which throws under Nx unless `DIGI_PATCHKIT_LIVE` is set (full keys, conformance and live closures set it);
`pnpm lint` runs `tools/nx/check-patchkit-spawn.mjs`, which rejects any other patchkit spawn or binary lookup in test code.
`fx-evidence.ts` reads the `needs` of the gain pack from `dist/bytes/pack-verdicts.json` (`patchkit:pack-verdicts`, packs listed in
`tests/regression/runtime/needs-packs.json`; `pack-needs.ts`, no patchkit; a missing entry fails): the key of a test that imports it hashes that file.
The static `--modules` root lists (`tests/regression/runtime/image-roots.json`: `rootsOf` in `image-roots.ts` gives `RT`, `COUPLED_ROOTS`, `IB` and the fx sets) are patched by `patchkit:device-images` too (lines `roots:<id>` in `images.sha256`; image, plan and address map kept in `dist/bytes/images/<device>/<id>.*`). `card-rig.ts` and `fx-session.ts` copy them through `prepareRuntimePlanListedOffline` (no patchkit; a list not in the file fails). A list built at test time uses `runtime-plan-modules.ts` and stays live.
`test-device-all` (a real run for every test, merge tests included), `test-conformance-all`, `stock-all` and the non-twin targets (`test-device-full`)
keep the full key (`test:full` runs the `-all` twins; `test-conformance`, `stock` and `regression:test-device` leave the driver packages out): the patchkit binary and `dist/daemons.json`. A merge key lists a shared file of `SHARED_READS` (`sdk/sdk.toml`, `types.toml`, core `module.toml`) only when a
helper in its closure reads it (`upload-kit.ts`, `synth.ts`, `memaudit-kit.ts`) or the test runs patchkit; the full key lists all of them.
A merge or narrow key leaves out the import-closure files under `DRIVER_ROOTS` (`packages/dk-session`, `packages/test-support`, `packages/control-client`: the code the test drives the device with, covered by its own host tests); the full key keeps them. `tests/regression/runtime` stays an input.
A half (`<target>--dt2`, `--dn2`) of a merge key leaves out the files of the other device (its device file, peer file and ports) unless the test loads the whole registry; a full key keeps them.
The sources, SDK headers and `source_sha256` of the image are not inputs: a comment, an unused constant or a rebuilt object with the same
bytes changes no key. The key also holds the pack outputs (`dist/modules/*/*.dkm`, `*.dks` and the `*.services.json` and `*.addrmap.json` sidecars), the `plusdrive` and `dksample.wasm` outputs, the
files the test reads and lists in `sources` (the module's own sources reach the run through the packs and the
object hashes, not as files), the files in `READ_BY_TESTS` (`tools/nx/device-test-keys.mjs`: `devices/*.toml`, `ports/**/*.toml`, core manifests under `runtime/` and `services/`,
`sdk/sdk.toml`, `types.toml`, fixture files, the Vitest and TypeScript config; tests read these without importing them; the files of a device without an acceptance cell (`devices/syntakt*.toml`, its ports and peer file) are excluded, see "Three keys exist"),
each record's `sources`. A record that runs `patchkit build` or `pack` at run time, or patches with explicit fixture modules (`patchModules`),
also keeps the SDK and the toolchain as inputs (record flag `compiles`; a record that patches fixture modules or packs at run time sets it),
the toolchain, the emulator env switches,
`config/emulator.json`, and the firmware, ready dirs and DT2 sample card (hashed by
`tools/nx/device-inputs.mjs`). That digest also hashes path, contents and file permissions of
`DIGI_EMUD`, device-specific/proof daemon overrides, `DIGI_PATCHKIT`, `DIGI_PATCHKIT_BIN` and
`DIGI_PLUSDRIVE` (following binary symlinks). Nonempty `PATCHKIT_MODULES` or `DIGI_PK_ROOT`, or any
explicit `DIGI_RUNTIME_MODULES` (including empty), deliberately prevent cache reuse with a fresh runtime nonce: arbitrary
sources/includes, variant toolchains and mutable object caches have no proven bounded input closure.

### Check what a change selects


1. What the merge gate would run for your branch, without running it:

   ```text
   $ DIGI_GATE_BASE=origin/main mise run test:plan
   gate: affected over the tier targets
    NX   134 out of 164 lint, fmt-check, rust-fmt, typecheck, test-host, ... affected
   Changing 46 files touches 1 lint task, ... 35 test-device tasks ...
     tests/regression/runtime/needs-packs.json and tools/nx/pack-verdicts.mjs:
       - patchkit:pack-verdicts
   Touching those tasks changes outputs read by 35 test-device tasks, 12 test-conformance tasks, ... and 225 tasks they depend on.
   ```

   "Changing N files touches ..." counts the tasks whose declared inputs contain a changed file, by target. Under each changed file
   or group of files the plan lists the tasks it selects. "Touching those tasks changes outputs read by ..." is the second step: a
   selected build task (here `patchkit:pack-verdicts`) rewrites a file that device tasks hash, so they are selected too. A device
   task whose inputs did not change is not in the list; its last result is replayed.

2. Which byte files key one device test:

   ```text
   $ pnpm nx:pilot show target inputs core/dynfx:test-device --json
   "depOutputs": ["dist/bytes/pack-verdicts.json", "dist/modules/dn2/dyn-fx-gain.dkm", ... , "dist/plusdrive/plusdrive"]
   ```

   Read `depOutputs`. `dist/bytes/images-<device>.sha256` (patched images, plans and address maps of the half's device) and
   `dist/bytes/daemon-test-<device>.key` (the daemon without the patchkit sources) key a merge target; `dist/bytes/pack-verdicts.json`
   keys a test that reads the `needs` of a committed pack; `dist/modules/*/*.dkm` are the packs it loads. `dist/patchkit/patchkit` and
   `dist/daemons.json` appear only on a full key (`test-device-all--*`, `test-device-full`, conformance); a live test holds
   `dist/bytes/patchkit-test-<device>.key` instead. `files` in the
   same output lists the source files in the key, including the test's import closure. `show target inputs --check` lists file
   inputs only: a patchkit source edit selects live tasks through the key file (a dependency output), so check it with a graph diff
   or run `node tools/nx/patchkit-key.mjs` twice.

3. Which tests are live, and why:

   ```text
   $ pnpm nx:pilot show target inputs core/runtime-storage-tests:test-device--store-format --json
   depOutputs: ["dist/bytes/pack-verdicts.json", "dist/bytes/patchkit-test-dn2.key", "dist/plusdrive/plusdrive"]   # the --dn2 half
   files containing tests/regression/runtime/patchkit.ts
   $ pnpm nx:pilot show project core/runtime-storage-tests --json   # targets["test-device--store-format--dn2"].options.env
   {"DIGI_PATCHKIT_LIVE": "1", "DIGI_DEVICE": "dn2"}      # test-device--safe-mode--dn2 has no DIGI_PATCHKIT_LIVE
   ```

   A test is live when its import closure holds `tests/regression/runtime/patchkit.ts` (it runs patchkit itself), or its record patches
   fixture modules. The patchkit key of its device is then in its key (the binary only in a full key) and `DIGI_PATCHKIT_LIVE=1` is set. To find the import that made it live, look
   for `patchkit.ts` in `files` and follow the imports of the test (`runtime-plan-modules.ts`, `dkm.ts`, `synth.ts` and `addrmap.ts`
   each import it).

4. What an undeclared use looks like. A task that is not live cannot start patchkit; the helper throws:

   ```text
   patchkit needs: this task's cache key does not follow patchkit. Import runtime/patchkit.ts in the test (that marks it
   live), or read the result from dist/bytes (images.sha256, pack-verdicts.json).
   ```

   `pnpm lint` also fails on a patchkit spawn or binary lookup outside `tests/regression/runtime/patchkit-exec.ts`
   (`tools/nx/check-patchkit-spawn.mjs`).

Overrides remain supported; these rare runs can write entries that will never be reused in local/remote
caches. Default runs retain a deterministic digest and do not scan override trees.
Specialized catalogue/files/LP0 card, patched-image, plan and boot-fixture overrides also prevent
reuse, because provenance can name further external files. LP0 case-selection flags are in the key.
Ignored/external edits alone do not trigger Git affected selection: invoke the owning Nx target
directly after changing an override binary/root or ignored firmware/ready/card input.
FX targets run the coupled-run binary, not a daemon. `digi-coupled:build` and `build-dt2` write
`dist/coupled-dn2.json` and `dist/coupled-dt2.json` (`coupled build --id-out`): the sha256 of the built binary and the store
key of the core library it loads, without paths. The FX key has this file (as a dependent task output), the named source globs `fxDn2` and `fxDt2` (they also
drive `affected`), and `tools/nx/coupled-inputs.mjs`. That digest covers the firmware and the ready inputs that
`tools/emulator/coupled.py` boots from (`[acceptance] ready_dir` of the device files, and the `DIGI_EMU_SYX*`,
`DIGI_COUPLED_SYX`, `DIGI_EMU_READY_DIR*` and `DIGI_EMU_DSP_*` overrides by file content). These are not the
patchkit ready dirs of `device-inputs.mjs`. Env in the key: `DIGI_FX_SWEEP`, `DIGI_FX_NO_AOT_SUBSET`, `DIGI_EMU_FAST`,
`DIGI_NO_PROGRESS_MS`, `DIGI_DT2_CARD` (its file content too), `DIGI_COUPLED_FEATURES`, `SHARC_AOT`, the `SHARC_FAST_*`
switches, `SHARC_BLOCKTRANS` and `UPDATE_GOLDENS`. The render cache (`out/cache/renders`) is keyed by content and is safe
to share. The tier targets write reports under `out/runs/fx-tier/<step>`; nothing reads them back. Not covered:
`SHARC_GEN_DIR` itself (the generated core enters through the store key of the core library in the identity file) and
`out/sections/*` (derived from the firmware file).
Not in the key on purpose: `dist/session-cache` and `<git dir>/snap-cache` (boot caches, cannot
change a result) and directory scans such as `modules/README.md`. Failed tasks are not cached.

### Digikit boot scenarios

The runtime loads its module set at boot. A TOML file in `tests/digikit/scenarios/` or `modules/*/*/tests/scenarios/` names the device,
the module sets and the expectations. `uv run python -m tools.patchkit.pk_digikit_scen list|run [NAME ...] [--image IMG] [--tier quick|full]`
boots the patched firmware through `pk_dkrt_boot` and checks. A scenario file has `[scenario] name, kind, devices, tier`
and a table named like the kind. Kinds:

| Kind | Checks |
| --- | --- |
| `equiv` | Machine, parameter, page and voice rows of a boot with set S equal those of a boot of no module (apart from S's own machines); the note is audible, and the TypeScript cells pin its PCM with a `<case>.pcm.json` fingerprint pin ("PCM pins"; a change in how the note sounds goes to Em). |
| `upload` | Module upload over USB SysEx: only `rust` is Python. The other parts are in `runtime/core/tests/upload.device.test.ts` and `upload-faults.device.test.ts`. |
| `menu` | Ticking a set in SETTINGS > DIGIKIT > MODULES and restarting loads the set: the machine is in the list, claims its voice row and plays an audible note. |
| `pages` | Parameter pages of a machine on track 1: an added machine has one SRC page (no page dots, no sample view) and the stock sample machines keep two. |

Every scenario ends with the write-watch over `cf.expansion` (0 stray writes), the flush check (0 hits)
and the card-sector check. Tiers: `DIGIKIT_TIER=quick` is the default. The store's cut and tear tests (`runtime/core/tests/transfer/upload-faults.device.test.ts`: upload cuts, torn headers, the CLEAR double fault) are full tier: the default gate skips them. Run them with `pnpm nx:pilot run core/runtime-tests:test-device-all--upload-faults` (sets the tier) or `DIGIKIT_TIER=full pnpm nx:pilot run core/runtime-tests:test-device--upload-faults` (add `--dt2` or `--dn2` for one device). The default `mise run test` gate does not select full-tier tests. `mise run test:full` runs every `test-device-all` twin with `DIGIKIT_TIER=full` (set by the target) and the separate `core/runtime-inferred-needs-tests:test-device-full` target; `mise run module:check` sets it for a module's own scenarios.
Both use the newest image of each device. Cards are cached by content in `out/cache/digikit/cards/`.
Jobs: `DIGI_JOBS`. `DIGIKIT_PROFILE=1` prints the phase table,
`DIGIKIT_RUN_SALT=x` forces cold boots.

The memory audit proves that a loaded module set writes no ColdFire memory outside what each module
owns while the UI is driven. The clean sets and the negative control (`test/dyn-demo-stray` stores one
word in the arena) are `runtime/core/tests/memaudit.device.test.ts`; `mise run module:audit NAME`
runs it for one module as `memaudit-module.manual.device.test.ts` (docs/MODULE-AUTHORING.md). The shared code
is `tests/regression/runtime/memaudit-kit.ts` and `memaudit-suite.ts`. The rig is ColdFire-only. The
first daemon protects the `cpu = "cf"` regions of `ports/<image>.toml` minus each module's
block, allows writes only from placed patch code, drives the UI and expects `guard.report` total 0. A
second daemon boots the same card with the write watch on the same ranges, so the load and `init`
are audited too. DMA and host writes are not seen. The full-tier variant loads the full synthetic set
of the capacity tests behind `test/dyn-demo` (`fullSet` in `synth.ts`) and also expects no free arena byte and
no whole free DSP granule.

The quick TypeScript capacity tests in `runtime/core/tests/capacity/capacity.device.test.ts` fill runtime limits with
synthetic modules (`patchkit synth-dkm`, built by `tests/regression/runtime/synth.ts`; limits in `[limits]` of `devices/*.toml`) and check refusal codes and
the reference module's DSP bytes. The separate full-tier `runtime/core/tests/capacity/inferred/inferred-needs.device.test.ts` checks needs inferred
by `patchkit needs` against real modules and fixtures. The MODULES page (rows, the fit refusal) is a host test of the menu with a fake runtime (`runtime/platform/ui/host/tests/menu.rs`, `menu_mechanism.rs`).
`tests/patchkit/test_limits.py` keeps tools from repeating a limit as a number.

### Machine and FX conformance

`mise run module:check NAME [--device dt2|dn2]` (NAME: `<author/name>` or the name alone) selects the suite by
module type and runs the module's own Python tests. A module with `tests/**/*.device.test.ts` files gets
Nx `test-device` targets (inferred by `tools/nx/modules.mjs`, read from the Nx graph); `module:check` runs all owners serially after Python success, propagates a
failure, and reports size/fit only after both stages pass. With no Python tests, the existing source
check still precedes the owned targets. Frequency shifter owns the exact existing TypeScript shifter
suite on both devices, including when `--device` limits other checks. Its daemons and ready inputs
use the existing harness resolution/overrides; only resolution holds a shared benchlock, released
before the target. The target does not run the broad gate. Its bounded source inputs are conservative affected-selection inputs, not a cache-closure or
fine-routing claim. This does not replace automatic gate routing.
`module:check` runs the conformance suite on the SHARC-only rig (reports in `dist/conformance-rig` of the worktree) ("The Vitest kit") and writes `dsp_cost_*` with `patchkit uses set`.
`pnpm nx:pilot run @digi/sharc-rig-kit:test-dsp-conformance-all` runs it for every module.

Machine rules (`packages/sharc-rig-kit/src/spec.ts`, `cases.ts`, `checks.ts`), checked in every block of every case:

- the call returns within 400,000 instructions;
- M5, M6, M7, M13, M14, M15, L0 to L5 and MODE1 are preserved;
- all 32 output samples are finite and at most 8.0 in magnitude;
- instructions per block stay within the DSP budget headroom: `headroom / 16` per block, `headroom`
  for a note-on block. No manifest replaces either number. After the run `module:check` writes the worst steady block as the module's `dsp_cost_voice` (a machine) or `dsp_cost_fx` (an effect) into `[uses.<device>]`; the boot planner, the menu and `patchkit fit` check a selection with it.

FX rules (the same kit files, `fx.ts` for the master chain): the hook returns
within 200,000 instructions; registers the stock function expects are preserved; the core/fx fault
count stays 0; the module stores only to its output block, data, bss and stack; the master stays finite
within 8.0; the effect stays within `min(headroom) / 4` plus 900 for core/fx. Negative fixtures:
`test/dyn-dsp-hang` (machine), `test/dyn-fx-nan` (effect). `DIGI_JOBS` sets the worker count.
The merge gate gives every manifest exercised by the machine or FX suite its own cached
`test-dsp-conformance` target, including the negative fixtures. The key tracks the DSP
sources and includes, generated gallery, shared rig-kit source, and the adapter and suite for
the module kind. `pnpm nx:pilot run-many -t test-dsp-conformance` runs these cached targets
and checks the shared FX write-set harness once in the kit project. The kit's explicit
`test-dsp-conformance-all` target and the full gate run the aggregate suite.

### SHARC-only rig (`sharc-rig`, T1g slice)

`sharc-rig` (crate `packages/sharc-rig`, outside `packages/emulator` so that an edit is no input of the daemon build key)
is a call harness for module routines on the generated native core. It decodes and runs the loaded code with the engine of
`sharc-native` (`decode_at`, `exec_insn`), like `tools/patchkit/pk_sharc.py` `CallSession` does on the Python core. A JSON
request lists code to load, DM writes and calls; the caller assembles and places the code (`patchkit sharc`) and decides
which words a note or a block sets. The format is in the module comment of `packages/sharc-rig/src/lib.rs`.

```sh
mise run gen:core                                  # the generated core, once
node tools/nx/rust-env.mjs --gen -- cargo build --release -p sharc-rig   # target/release/sharc-rig
sharc-rig request.json > response.json             # or the request on standard input
pnpm nx:pilot run sharc-rig:rust-tests             # unit tests (they need the generated core)
```

A call returns `instructions`, `halt` (`breakpoint`, `max-steps` or `trap: ...`), `final_pc`, the registers and the DM words
asked for. With `watch` ranges (pcs) it also returns the stores of instructions in them as `[pc, address, width]` runs
(the engine's undo log of the instruction; nothing is recorded without `watch`). The op `pokem` changes some bits of a DM word
(a frame halfword, the trig bit, a release byte). `sharc-rig --serve` keeps the core between lines: the first line is a request, every
later line is `{"ops": [...]}` and gets one answer line, so a rig runs a case block by block and stops at its first failure.

The stock DSP image can be the base core. The setup line takes `image`, the loader stream of firmware section 7 as hex. The rig reads it with the sharc-peer crate's own `LoadedMemory::from_stream`
(`packages/emulator/sharc-peer/src/dsp_stream.rs`, compiled into the rig by path: a dependency would change `Cargo.lock`, an input of the device tests) and loads it before `code`, so
linked code and hooks go over the stock bytes. A call takes `stock_reads: [[loader_byte_address, count]]` and answers `stock`, the bytes of the image as loaded (hex, `null` when the stream does not write all of them).
`breaks` on a call replaces the setup's breaks for that call, and `resume: true` continues from the pc the last call stopped at (the stacks and registers stay; `regs` are still set), as `Runner.run` does after a breakpoint. A zero-step
resume reads DM without ending a stopped call. `specials: {"MSF": [mask, bits]}` sets an 80-bit multiplier register (hex strings) before a call and `read_specials` answers it after. An address like 0x2010f000 or 0x80100000 is a byte address: a read count steps by one byte, so a rig that reads words asks for each word.
`packages/sharc-rig-kit/src/stock.ts` finds `out/sections/<id>/section_7_BLOB.bin`, checks its hash and gives the hex; a test that needs it skips with `stockSkipReason` (a describe title) when the firmware was not extracted.
`StereoRig` (`stereo.ts`) is the DN2 1.12 stereo rig on it: `dsp-voice-dn2-stereo.s` linked beside the stock image, the pan hook patched in at 0x1c9af6, `StereoSession` for one core.
The tests are `runtime/platform/dsp-voice/tests/dsp-stereo-*.rig.test.ts` (`pnpm nx:pilot run core/dsp-voice:test-dsp`) and `tests/fixtures/modules/dsp-stereo-proof/tests/*.rig.test.ts` (`core/dsp-stereo-proof:test-dsp`, over `openImageSession`).

A call with `"loads": true` also returns `loads`, the read analogue of `watch`: `[pc, address, width]` for each DM load (1, 2 or 4 bytes; a long-word or SIMD load is one entry per word) of an instruction in the `watch` ranges, in execution order, with the addresses of `stores` (L2 at the loader alias, an image word at its alias). The runtime records the loads (`St::read_log_on`, `packages/emulator/sharc-rt`); while it is on, the call takes the full memory path instead of the plain-RAM fast path. The record is off by default, costs a call without the option nothing and leaves no `loads` key. `StereoSession.raw` and `CallSpec` take `loads: true`. Block code (`_dm_read_b`, the fast tier) is not recorded: the rig runs one instruction at a time.
The check kind is a `LoadRule` (`packages/sharc-rig-kit/src/loads.ts`, host tests `tests/loads.test.ts`): the covered loads (`pcs`, else every load of the call), the byte ranges they must lie in (`within`), and how many bytes they read (`bytes`, `bytesMultipleOf`). `loadFailures(loads, rules)` returns one text per broken rule, so a load of the right instruction at a wrong address fails where a count of executions passes. The shadow replay (`tests/fixtures/modules/dsp-stereo-proof/tests/dsp-stereo-shadow-replay.rig.test.ts`) keeps its read bounds as rule data per group: the table loads of the held and prepared leaves (and the held period `1 < p < 32`), the gain tables, the ring backing of the channel, the source samples and table, the ordered-route phases and the helper resets.

A call with `"profile": true` also returns `profile`, one `[pc, count]` row per pc it executed, in pc order (the counts add up to `instructions`). Without the option
nothing is counted and the answer has no `profile` key. `MachineRig` takes `profile: true` and puts the rows in `MachineBlock.profile`. The rig is a producer for `wb profile report`
([INSTRUMENTATION.md](INSTRUMENTATION.md#profiles)), the profiler of the emulator and the device: `packages/sharc-rig-kit/src/profile.ts` writes the counts as a `dk.profile/1` (`rigProfile`, raw SHARC counters
per pc, metric `sharc.blocks`, clock unit `instructions`, the load of the module's DSP image) and the labels of the placed objects as a `dk-addrmap` (`rigAddrMap`; module labels relative to its DSP image).
`tools/devtools/rig_profile.ts` profiles one conformance case of a machine or effect. It adds the
machine's steady blocks or the effect's master test-chain blocks, writes the profile, address map and
module's `.dkm` to `out/runs/rig-profile/<module>/<device>/`, and runs `wb profile report`. The profile
includes the map and the DO-loop analysis when `dkprof.wasm` is available. Run `mise run wasm:dkprof`
once. `--print` prints the report command for later. SHARC core only, a few seconds:

```sh
node tools/devtools/rig_profile.ts digikit/shot --device dt2 --case sample/stereo-fwd-oneshot/note60 --top 4
# run rig:digikit/shot:dt2:sample/stereo-fwd-oneshot/note60 target dt2 clock sharc-rig.insn (instructions)
# metrics:
#   sharc.blocks                       299 blocks
# pc sharc by function (top 4; 944 rows, total 696969):
#           143819  20.6%      481.0/block  _dk_sample_pcm16s_read_right
#            73853  10.6%      247.0/block  _dk_gain_ramp_stereo
#            63687   9.1%      213.0/block  _dv_gcheck
#            41262   5.9%      138.0/block  _dk_sample_pcm16s_run
# pc sharc by loop (self; an inner loop is not in its outer one):
#           181792  26.1%      608.0/block  digikit/shot _dk_sample_pcm16_stereo_linear depth 0 +0x032b..+0x035b count reg R4
#            66976   9.6%      224.0/block  digikit/shot _dk_gain_ramp_stereo depth 0 +0x001e..+0x0030 count lit 32
# pc sharc by owner:
#           356109  51.1%     1191.0/block  digikit/shot
#           340860  48.9%     1140.0/block  runtime
```

`--blocks 40,41` adds only those blocks. The function rows follow the assembler's labels (a label inside a loop splits its cost); the loop rows come from the DO loops of the module's code, so a loop is one row.
`DIGI_RIG_ROOT=PATH` reads the sources, SDK and device files of another checkout (a lane branch) read only; `DIGI_SHARC_RIG` and `DIGI_PATCHKIT` name the binaries when the worktree has no `dist/`.
The counts are instructions of the generated core: the same numbers as the emulator's `sharc.profile` for the same code, without the firmware around it.

```sh
pnpm nx:pilot run sharc-rig:build        # dist/bin/sharc-rig and dist/bytes/sharc-rig.sha256 (shared generated core)
```

When it was built, measured against the Python rigs (`machine_rig.py`, `fx_rig.py`, which still serve the adapter tests) on `test/sine`, `fadedlimes/analog` (DT2 and DN2), `tom/tb303`,
`digikit/pluck`, the FX `digikit/freqshift` and the negative fixture `test/dyn-guard-nan#overrun` (hostile contexts): bit-identical
output words, identical instruction counts in every block, the same store bytes and the same foreign-store texts. On 300 to
500 block cases the harness (process start and 64k setup pokes included) is 45 to 110 times faster.

#### The Vitest kit (`packages/sharc-rig-kit`)

`@digi/sharc-rig-kit` is the TypeScript side: it assembles and links the sources with `patchkit sharc` (`toolchain.ts`), places them
(`place.ts`), and runs them on `sharc-rig --serve` through `MachineRig` (core/dsp-voice, `machine.ts`) and `FxRig` (core/fx master chain,
`fx.ts`). `spec.ts` reads what a module declares (module.toml, `build/gen/gallery.json`, `devices/*.toml`), `cases.ts` makes the
conformance cases, `checks.ts` holds the rules of a block and `conform.ts` runs a case and writes the report. The random walks and
the hostile contexts come from `PyRandom`, the generator of Python's `random.Random`, so the cases are the ones the Python suite ran
and the recorded costs stay comparable. Host tests (fake transport, made-up build, no toolchain) run with
`pnpm nx:pilot run @digi/sharc-rig-kit:test-host`.

The machine and effect conformance on the rig (about 45 s for all 30 specs with 4 workers, no daemon, firmware or emulator):

```sh
pnpm nx:pilot run-many -t test-dsp-conformance                    # cached contract checks for all modules
pnpm nx:pilot run digikit/pluck:test-dsp-conformance               # fixed machine cases on both devices
pnpm nx:pilot run @digi/sharc-rig-kit:test-dsp-conformance-all     # uncached, accepts selectors and report overrides
DIGI_CONFORM_MODULES=fadedlimes/analog DIGI_CONFORM_DEVICE=dt2 DIGI_JOBS=4 DIGI_CONFORM_REPORTS=/tmp/reports \
  pnpm exec vitest run --config vitest.host.config.ts --project sharc-rig-conformance conformance/machine.test.ts
```

It replaced the Python suites (`tests/modules`, retired in T1g phase 6, [ledger](plans/ledgers/t1g-retirement.md)) with the same case list, the same pass set and the same failure messages for the negative
fixtures. The worst steady block of each (module, device) is the `dsp_cost_voice` or `dsp_cost_fx` of module.toml. Reports are
`dist/conformance-rig/<id>@<device|fx>.json` (`DIGI_CONFORM_REPORTS` moves them). Reports sit in the worktree's own `dist/`, and each records its `worktree`. The fixed merge
targets write and restore their own directories under `dist/conformance-rig/merge/`.
The explicit target and raw Vitest accept `DIGI_CONFORM_MODULES`, `DIGI_CONFORM_DEVICE` and
`DIGI_CONFORM_REPORTS`. The explicit target runs uncached so those invocations write fresh
reports. `module:check` refuses to write a cost from a report of another worktree.

The cost writer, `patchkit uses set --id MODULE --device dt2 --dsp-cost-voice N` (or `--dsp-cost-fx N`), edits `[uses.<device>]` of the
module.toml like `pk_modcheck.write_uses` does: only the lines of the keys given change, comments and other keys stay, and the file
is written only when a count differs. It prints `wrote PATH` or `unchanged PATH`.

#### Sound checks (`tests/rig/*.toml`)

A module keeps the checks of how it sounds in `modules/<kind>/<author>/<name>/tests/rig/*.toml` and one entry file, `tests/rig.rig.test.ts`
(`import { rigSuite } from '@digi/sharc-rig-kit/suite'; rigSuite(import.meta.dirname);`). Nx infers `<module id>:test-dsp` from that file
(cached; inputs: the module, the kit, the two adapters, the SDK, the device profiles and the hashes of `patchkit` and `sharc-rig`; no
daemon, firmware or emulator). `pnpm exec vitest run --config vitest.host.config.ts --project module-rig <module dir>/tests/` runs one.

```toml
[[case]]
name = "c4 sounds"
device = ["dt2", "dn2"]           # default both; an effect has no device
context = "friendly"              # default: a hostile context for every block
[[case.step]]                     # a machine case plays steps; note, vel and parameters stay from step to step
note = 60
vel = 100
params = { decay = 0x4000 }       # parameter words by manifest id
blocks = 200
trigger = true                    # default; release = true sends a note-off in the first block
[[case.check]]
kind = "rms"                      # see below; `blocks = [first, end)` limits a check to some blocks of the case
min = 0.05
max = 0.5
```

An effect case has `[case.input]` (`kind` = `sine` with `freq` and `amp`, `table` with a `file` of float32 words (64 per block: left,
then right), `square-noise`, or a test signal of the conformance cases), `observe = "scratch"` (the effect's own block, the default) or
`"master"`, `mix` (a float32 word) and steps with `params_f32 = [..]` (the effect's float parameters), `blocks` and `mix` (a float32 word written to the row before the step's first block: a MIX change during the run, which the adapter ramps over the next block).

A machine that plays a sample has `[[case.pool]]` tables (`index`, `frames`, `rate`, `channels`, `token`, `planes` = a file of int16 words, left then right;
the kit publishes the planes and the record before the first block), `pool_ctl` (the data symbol of the control sets: the count word must be 0 after every
block), `right = true` (DT2: render the odd voice after each block; checks take `channel = "right"`) and step `pokes = [["rec", word, value]]` (`rec` or
`ctl` of the first entry). `track` plays a track other than 0, `record_word` writes the voice record's word +0x1b8.

Every case also holds the implicit rules of the conformance run (the call returns, the context registers and the runtime guard are
fine, the module writes only its own memory, the output is finite). Check kinds: `rms`, `peak` (`min`, `max`), `dc` (`max`), `no-clip`
(`limit`, default 1), `finite`, `silent-after` (`block`, `max`), `decay-time` (`db`, `min`/`max` seconds), `pitch-hz` (`note` or
`expect`, `cents`, `method = "crossings"` or `"bandpass"`, `skip_periods`), `tone` and `tone-stable` (effects), `sample-at`, `differs`
(`other = [first, end)`), `voice-ends` (`within`, `after`), `instructions` (`max`, `of` = `first`, `steady` or `idle`),
`cursor` (a word of the voice's state block, `signed`, `tol`), `voice-active`, `tone-fit` (least-squares amplitude of a tone), `description`, `state-word`,
`state-untouched-beyond` and `master-equals-input`, `master-equals-scratch`, `scales-input` (effects; `gain`, error within `rel_peak` of the reference peak, default 2**-20), and the references `matches-table` (`file` of float32 words next to the toml,
`atol`, `channel`) and `same-as` (`sha256` of the float32 words). The analysis is `packages/sharc-rig-kit/src/analysis.ts`, tested on
synthetic signals. A reference table is the output of the old float32 model of the module (or of the rig, for a new pin) as float32
words; changing one is a re-pin and needs review like a PCM pin of `tests/scenarios`.

A `same-as` pin comes from the rig itself: a machine case with a `same-as` check names one device (an effect case runs once for the effect and uses the key `fx`), and `DIGI_RIG_PIN=1 pnpm exec vitest run --config vitest.host.config.ts --project module-rig <module dir>/tests/`
runs no checks and writes the words hash of every `same-as` signal into its `sha256` line (`pinRigFile` in `soundcheck.ts`; the n-th `sha256` line belongs to the n-th `same-as` check of the file) and prints `pin CASE [device] HASH`.
Every machine case runs twice with all module state filled differently: the module's `.PKBSS` and the voice state blocks (the state array is a bss run in the rig's data layout) hold NaN words in the first run and a seeded integer pattern (`FILL_SEED`) in the second (`MachineRigOptions.poison` default true, `fillSeed`). The case fails with kind `initial state` when the two outputs differ in any sample: the output does not depend on the initial state, so a read of state the machine never wrote shows, float or integer, and a NaN output fails the finite rule. That is a machine bug to report, not a value to pin. The adapter's `note_on` clears the first `clear` bytes of the voice state, as on the device. `poison = false` on a case zeroes the memory, skips the second run and needs a `poison_reason`. A `MachineRig` refuses to open a machine whose device is missing from `build/gen/gallery.json` or has no parameter there (`galleryGaps`; `module:check` writes the gallery).
The machine note pins of analog, bd, pd and pluck are `tests/rig/equiv-note.toml`: note 60, velocity 100, default parameters, 375 blocks (0.25 s) in a friendly context, one case per device (pluck adds a second note, 67, for 375 blocks).

#### Pluck test coverage

| Assertion | Owner and scope |
| --- | --- |
| Loop period across the note and TUNE grid, bounded model output | `test_pluck_range.py` exercises the float32 model. The same file probes the real SHARC state at the corners. |
| Sound, pitch, retriggers, idle and ending behavior | `digikit/pluck:test-dsp` runs the sound cases in `tests/rig/`. The model tables, pitch, corner and voice lifecycle checks run on DT2. The numeric two-note fingerprints and initial-state check run on DT2 and DN2. |
| DSP contract and budget | `digikit/pluck:test-dsp-conformance` uses the shared machine suite on both devices. Its limits, corners, modulation, long-note, retrigger and write-set cases check returns, context, bounded samples, memory ownership and instruction cost. |
| Controls and registration | The four controls in `src/module.c` are declarative SDK builder calls. The rig reads knob slots, defaults and ranges from the generated gallery. SDK host tests exercise the shared builder contract. Runtime fixture tests check machine rows and parameter state, and the module's device load checks its real pack, published voice row and type. The shipped DT2 pages scenario exercises the actual panel. |
| Runtime loader integration | `core/runtime-fixture-tests:test-device` exercises the common upload and load assertions with `test/sine` on both devices and the machine with its own page, `test/dyn-dn2`, on DN2. `core/runtime:test-assembly` exercises the loader seams on the host. |

A Pluck DSP-source edit selects the pack and both rig checks. An edit to its private
ColdFire source is outside the rig inputs. Device-load conformance follows the same
packed-module policy as other shipped modules. It covers USB upload, persisted bytes,
reboot, registration and runtime faults.
The DT2 menu scenario is available through `module:check`
with the module's host and generic rig conformance checks. The dedicated pages
scenario runs separately and checks the SRC page and encoder interaction.

```sh
pnpm nx:pilot run digikit/pluck:test-dsp
pnpm nx:pilot run digikit/pluck:test-conformance         # shipped load on DT2 and DN2
pnpm nx:pilot run core/runtime-fixture-tests:test-device # representative fixture loads
mise run module:check -- digikit/pluck                  # includes the module's DT2 menu scenario
uv run python -m tools.patchkit.pk_digikit_scen run pages-dyn-pluck --image dt2-1.17
```

#### Classifying a changed pin (fingerprint, report, plots)

A `same-as` check is gated by the pin's fingerprint with its tolerances, or by the `sha256` (bit-exact) when the check has `exact = true` or the pin is a v1
fingerprint (format and tolerances: "PCM pins"). Each pinned case has a fingerprint in `tests/rig/fingerprint/<key>.json`
(plain numbers; v1: peak, onset sample, RMS per 32-sample block, 64 log-spaced band levels from 40 Hz to 22 kHz; no audio; v2 adds more). `<key>` is
`<module>.<device>.<case name>` in lower case (`rigKey` in `packages/sharc-rig-kit/src/fingerprint.ts`). Writing a pin (`DIGI_RIG_PIN=1`, above) writes the
fingerprint and the local cache `out/rig/<key>/ref.f32` (the pinned output; not committed; `out/` is shared by the worktrees, so keys name module, device and case).

When the hash differs, the failure message adds a one-line verdict from `classify`:

```text
[dn2] same-as: the words hash is f2c3d9..., the pin is 00c3d9...
  real change: difference -13.7 dB re the signal, level -2.0 dB, onset 64 -> 64, moved blocks 2-374, moved bands 62-69 Hz -2.0 dB; 84-84 Hz -2.0 dB; 102-20942 Hz -2.0 dB
```

| Verdict | Meaning |
| --- | --- |
| `identical` | the output equals the cached output bit for bit |
| `time-shift` | the output is the cached output shifted by k samples (cross-correlation, -256..256); says whether the overlap is bit-identical |
| `rounding` | the difference is at or below -100 dB re the signal and the band spectrum is unchanged (0.05 dB) |
| `real-change` | the difference, the level change, the onset and the blocks and bands that moved by more than 1 dB |
| `fingerprint-only` | no cached output (a fresh checkout): the new fingerprint against the stored one; it cannot tell rounding from a phase change |

The report writes `out/rig/<key>/out.wav`, `ref.wav` (when the cache exists) and `plot.png` (waveform of the output over the reference, the difference, the band spectra of both) for every pinned case and prints one line per case. It runs the cases and changes nothing in the repository:

```sh
DIGI_RIG_REPORT=1 pnpm exec vitest run --config vitest.host.config.ts --project module-rig modules/machine/fadedlimes/analog/tests/
# DIGI_RIG_REPORT_FILE=PATH appends the lines to PATH (default: the temp directory, rig-report.txt)
```

Real lines of the report (after `DIGI_RIG_PIN=1`, the cache holds the pinned output):

```text
fadedlimes-analog.dn2.note-60-at-the-default-parameters-is-pinned-dn2: pin matches; identical: the output equals the cached output bit for bit
  .../out/rig/fadedlimes-analog.dn2.note-60-at-the-default-parameters-is-pinned-dn2/plot.png
```

On a fresh checkout there is no cache: the verdict is `fingerprint-only` (`envelope, bands and onset equal the pin` when nothing moved). A machine whose device is missing from `gallery.json` prints `not run: ...` (run `mise run module:check ID`).

#### Adapter tests on the rig (`tests/*.rig.test.ts`)

The tests of the SHARC image modules are Vitest files under `runtime/platform/{dsp-voice,fx,sample-pool}/tests/`.
They use the kit classes by `import { ... } from '@digi/sharc-rig-kit'` and run with `pnpm nx:pilot run core/<name>:test-dsp` (same inputs as the sound checks above).
The SDK's own DSP routines are tested the same way in `sdk/tests/dsp/*.rig.test.ts` (`pnpm nx:pilot run sdk:test-dsp`): a small harness object includes the SDK files and calls the routine with `DK_CALL` (the resonator bank, its band and the `DK_EXP2` and `DK_TAN` macros).
The kit has three rigs for them:

- `RawRig` (`raw.ts`): objects placed at bases the test names, data at its alignment, stand-in externs; `call(symbol, regs, { reads, readRegs })`, `poke`, `peek`.
- `FxRig` (`fx.ts`): the effect chain; `block` (master hook) and `sendBlock` (send hook; DT2 track buses with `setTrack`, DN2 slot buses with `setSlot`, `setSlotmap`, `setFade`; `setReturn`, `bus`, `prevGains`).
- `VoiceRig` (`voice.ts`) and `crumbRig()`: the dsp-voice wrapper with one routine; every call runs on a fresh core with the DM in `rig.dm`, so a test carries state by writing words back.

```sh
pnpm exec vitest run --config vitest.host.config.ts --project module-rig runtime/platform/fx/tests/fx-send.rig.test.ts --reporter=verbose   # prints instruction counts
```

### DSP budget gate (patchkit smoke)

Every `pk_harness run` cell ends with a DSP measurement: TRIG 1 held for 1 s of audio, the mean busy
instructions per block from `status.audio.dsp_tiers`, minus the stock baseline of the same device and
OS (cached in `out/runs/patchkit/dsp/`). Projected load = busy x factor / 666,667. The factors are in
`devices/cycle-factors.json`. A cell fails when the projected high load exceeds `--dsp-budget`
(default 85 %); `--no-dsp-budget` skips it. Results go to `out/runs/patchkit/dsp-budget.json`. Unit
test: `tests/patchkit/test_pk_dsp_budget.py`.

### SDK-surface coverage

`patchkit imports` lists the SDK imports each `.dkm` links (its import table; the builder calls of `init`, such as `dk_machine_add_page`, `dk_page_add_param` and `dk_fx_routes`, are imports too, so one table covers them). `patchkit coverage` compares the union over the packed modules with the `[[import]]` list of `sdk/sdk.toml`, by service. An import that no packed module links fails the run, unless `sdk/coverage-allow.toml` lists it with a reason; an allow-list entry that a module links, or that is not in `sdk.toml`, fails too. A linked name that sdk.toml does not list is an error, except in a service file (`.dks`) for its own link names (`dkc_*`, `dk_fw_*`, `dk_exit_*`, `dk_tail_*`, `dk_ret_*`, `dk_stock_*`, `dk_provider_declare_row`; `SERVICE_LINK_PREFIXES` in `coverage_cli.rs`). An `[[import]]` marked `internal = true` is called by the runtime, so it needs an allow-list entry. Not observable from a pack: the values a call passes (glyphs, routes, parameter kinds, the constants of `[machine]`, `[fx]` and `[page]`) and whether a linked call runs. The fixture calls that close a gap are compiled and linked, and some never run (the `probe` flag of `test/entry-demo`).

```sh
pnpm nx:pilot run patchkit:sdk-coverage     # packs every module (cached), then the report; not cached itself
dist/patchkit/patchkit imports [--json] dist/modules/dt2/entry-demo.dkm
dist/patchkit/patchkit coverage --sdk sdk/sdk.toml --allow sdk/coverage-allow.toml [--json] dist/modules/*/*.dkm
```

```text
sdk imports: 116 total, 106 covered, 10 allowed, 0 uncovered (108 modules)
  dsp_link   2/2 covered, 0 allowed, 0 uncovered
  helper     3/6 covered, 3 allowed, 0 uncovered
  ui         18/18 covered, 0 allowed, 0 uncovered
  usb_bulk   8/10 covered, 2 allowed, 0 uncovered
    allowed    dk_usb_bulk_takeover (removed: always returns DK_USB_BULK_E_STATE, kept for the import table)
```

Without `--allow` the same files give 106 of 116 covered. A new `[[import]]` needs a call in a fixture or a module, or an allow-list entry with a reason; the exit code is 1 otherwise. Unit tests: `cargo test -p patchkit --lib coverage_cli`.

### AOT versus interpreter equality

The no-AOT halves of `tier-quick` and `tier-sweep` use `NO_AOT_SUBSET` in `tools/devtools/fx_tier.py`:
17 of the 134 DN2 FX scenarios and 8 of the 60 DT2 ones. `fx-no-aot` runs 17 of the 61 oracle tests
(memberships in `tests/regression/scenarios/dn2-trackfx.test.ts` and `dn2-sendfx.test.ts`).

## CPU budget of the gate

`pnpm nx:pilot` runs `tools/nx/nx.mjs`, the one Nx launcher. It computes a budget from the machine:
B = the performance core count on Apple Silicon, otherwise cores - 2, with a minimum of 2, or `DIGI_BUDGET=N`. A value already set in the environment wins over the derived one.

| Setting | Formula | B=4 | B=8 | B=12 | B=16 |
| --- | --- | --- | --- | --- | --- |
| Nx tasks (`NX_PARALLEL`) and emulator ceiling | max(1, min(B, floor(RAM GB/2))) | 4 | 8 | 12 | 16 |
| Coupled (SHARC) emulators at once | floor(B/3) | 1 | 2 | 4 | 5 |
| `REGRESSION_JOBS` (Vitest workers per task) | 1 | 1 | 1 | 1 | 1 |
| `DIGI_JOBS` (emulator processes) | clamp(floor(B/2), 1, 6) | 2 | 4 | 6 | 6 |
| `CARGO_BUILD_JOBS` | B | 4 | 8 | 12 | 16 |
| `NEXTEST_TEST_THREADS` | 2 | 2 | 2 | 2 | 2 |

The values assume at least 2 GB of RAM per emulator. On Apple Silicon B is the performance core count, so a 12P+4E machine has B 12.
B is also the core budget of the emulators. A task has the cores of its DSP mode: a zero-DSP run (ColdFire only) uses about 1 core, a coupled run (`dsp: 'sharc'`) about 3 (measured on a 16-core M3 Max). The slot ceiling counts zero-DSP runs (12 slots at B 12); the core budget stops coupled runs from filling them: at most 4 coupled emulators, or 12 zero-DSP ones, or a mix with cores 3 x coupled + zero at most 12. (Counting 12 slots of mostly coupled tests put the load at 60 and made each emulator about 3 times slower.)
The mode comes from the `metadata.runtimeAcceptance` record: `"dsp": "sharc"`, no field means zero. The Nx target passes it as `DIGI_EMU_DSP` (a conformance target takes it from the module: a dynamic non-service module is coupled). A test that needs the DSP (PCM, DSP memory or a machine or effect DSP path) and runs in a record without `dsp: 'sharc'` fails at run time: `resolveDsp` (`packages/test-support/src/session.ts`) throws on `pcm.*`, `dsp.*` and `audio.*` calls without it. A record that says zero while a file starts coupled sessions is not detected; the run takes 3 cores while the slot counted 1 (slower, not wrong): set `dsp` when a file passes `dsp: 'sharc'`.
`tools/nx/resources.mjs` holds this policy.

Each launcher run has its own budget, so several lanes could start more emulators than the machine runs well.
Module device targets and `test-conformance` therefore run behind `node tools/devtools/emu-slot.mjs --`. It takes one
of N machine-wide slots (lock files in `<git common dir>/emu-slots/`, N = the ceiling above), runs the command, and
passes on its exit status. A slot is taken only when the task fits the machine. One waiter at a time holds the
`admit` lock and decides: if the slots in use reach the ceiling it waits; the waiter with the longest expected run goes
first (priority = seconds of the task's last passed run, kept per target in `<git common dir>/emu-slots/durations/` and shared by all worktrees, else a guess of 600 s for a coupled and
120 s for a zero-DSP task, so long coupled tasks start first and do not end the gate alone; a head waiter that does not fit by cores yet may be overtaken by a smaller waiter that fits, at most 8 times, then nothing overtakes until it fits); if no emulator runs it admits at once (work
never starves); otherwise it waits while the cores of the running tasks plus its own would exceed B, then samples the idle cores twice, 1 s apart, and admits when at least its own cores plus the reserve are
idle (that guards against load that is no emulator of ours; the CPU times come from the OS, not the load average). The reserve is the efficiency-core count on Apple Silicon, else max(2, floor(cores/4)); `DIGI_EMU_RESERVE=N`
sets it. After an admission the lock is kept for 3 s, so the next decision sees the new emulator's load. The log shows
`emulator slot taken (M of ceiling, idle X.X cores), emulator slot waited N s`. With `DIGI_TRACE_DIR` set, it appends `wait` and `run` spans to `$DIGI_TRACE_DIR/slots-trace.json`. Overrides: `DIGI_BUDGET=N` sets B (cores and slots), `DIGI_EMU_SLOTS=N` is a fixed count with no core, idle or order check, for an A/B.
Order limit: Nx starts twice as many tasks as slots, so the order holds among the tasks that Nx has started; `mise run test:subset` prints how many coupled and zero-DSP tasks start at once.
A waiting task prints one line and waits without a limit; a lock of a dead process is taken over. The admit lock is stale after 30 s (`ADMIT_MAX_MS`) even when its pid lives, so a holder that never releases it cannot stop every waiter. The wait is outside
Vitest, so it does not count against test timeouts. The emulator gate targets are wrapped too (see the job rule below).

Formatting an emulator crate or `dkneeds` changes the daemon key and the PGO key, so the PGO profile needs a retrain.

- For verbs that run tasks the launcher runs at nice 10 and takes the shared benchlock, except when all
  targets are light (`lint`, `fmt-check`, `rust-fmt`, `typecheck`, `test-host`, `check`, `check:ui`,
  `build:ui`, `python-checks`): those take no lock. `show`, `graph`, `--graph` and `--explain` take no lock. Nx has no per-kind limit. No gate task is exclusive: a task with `parallelism: false` scheduled mid-run makes Nx wait for every running task to finish first, so do not add one. The `cli`, `digi-coupled` and `digi-control:daemons` builds run beside other tasks with `CARGO_BUILD_JOBS=DIGI_JOBS`.
- `DIGI_JOBS` is the only job variable. The `fx`, `fx-no-aot` and `fx-dt2` targets and the single-emulator gate targets (`card`, `dt2-tone`, `fast-roots`) run one emulator each (1 job), behind one `emu-slot`, beside device tests. The multi-emulator gate targets (`smoke`, `stock`, `pins`, `tier-quick`, `tier-sweep`, `dt2-tier-quick`, `dt2-tier-sweep`) keep their own `DIGI_JOBS` and take no slot: the idle-core admission of the slot-wrapped device tasks keeps the machine within its budget while they run. `python-tests` runs with 2 jobs (`DIGI_JOBS=2`) beside other tasks, without a slot.
  `DIGIKIT_PART_JOBS` stays. One coupled emulator process keeps 2 cores busy.
- A no-progress watchdog (`tests/regression/watchdog.ts`) is on by default. It starts with the daemon (`Session.start`
  option `onSession`), so it covers `firmware.load`, and it reads `status` over a second connection (`Session.watchStatus`):
  the status call answers while a load runs. It closes a daemon when none of the DSP frames, main loop hits, audio frames,
  `cf.icount`, `boot.icount` (the instruction count of a boot inside `firmware.load`) and the epoch changes for 120 s of wall time
  (`DIGI_NO_PROGRESS_MS`, in ms; 0 turns it off), so the test fails at once. A cold boot, a card-rig load and a
  multi-boot chain move the instruction counts; the counts are not monotonic (a soft reset or a snapshot restore lowers them),
  so a change in either direction is progress. A paused daemon without a job or a boot is idle and not counted. Host work (building a card)
  moves no counter and is not covered: a daemon is not asked to progress then, and only the Vitest limit applies.
- Tests have no wall-time limit of their own for runtime start, baseline reload, boot, restart and safe-mode waits. The
  watchdog catches a stopped emulator; a wait on firmware state uses emulated time (`pollUntil`, `pollEmulated` (thin callers of `poll` in
  `packages/dk-session/src/waits.ts`; clock and probe in `emu-clock.ts`; the harness constructor `emulatedOptions`/`emulatorDk` in
  `tests/regression/runtime/waits.ts`), `rt.ui` waits, `events.until(kind, match, { withinMs })` of `rt.listen()`). A stock MidiRpc request (`DkSession.rpc`, so `RpcFiles`) of a harness session waits `timeoutMs` of emulated time: the harness gives `DkSession` an `emulatedClock` and an `emulatedProbe` (`emulatedOptions`); `dk.waitForBoot()` and `dk.waitForRestart()` run on that clock and every LOG and INFO request of the wait is one 2 s emulated probe, and wall time only guards a dead link. `expectPollEmulated(session, read, assertion, { emulatedMs })` replaces `expect.poll(read, { timeout })` in device tests: a wall `timeout` there is cut by a loaded host. Wall limits that remain are readiness deadlines (`expectRemoteEnabled`, the runtime-menu rows) and polls of host processes. Vitest's `testTimeout` (30 min) and
  `hookTimeout` (10 min) of the `runtime`, `proofs` and `conformance` projects are outer safety nets, not hang detectors: hangs are caught by the progress guard and the emulated-time waits above. `runtimeTests` takes no `timeout` option, so a test cannot set a shorter wall budget that host load then cuts. The emulator-slot wait happens in `emu-slot.mjs` before Vitest starts and never counts against them.
  Hang guards that never fail a healthy slow run: a clock that stands still for 60 s fails a wait (a clock guard per wait, `stuckGuard`). Only the test-support wrappers `pollUntil` and `waitClock` read `DIGI_CLOCK_STUCK_MS` for that limit; dk-session's own `poll`, `waitClock`, `waitForBoot` and `waitForRestart` use `hangMs` (default 60 s) and ignore the variable
  (a paused daemon, a halted DSP); the phases before the daemon has a socket (environment preparation, the startup load of a
  non-reused session) nothing can be asked, so each has a 300 s guard (`PRE_SOCKET_HANG_MS`); `waitReset` stops after 200 steps
  of 400k instructions. Host processes (builders, tools) keep their own `timeout`: the emulator watchdog cannot see them.
- `pnpm nx:pilot run digi-control:daemons` builds the daemons and writes `dist/daemons.json`; helpers
  read it when `DIGI_EMUD_*` is unset. The builds use the daemon store (`<git common dir>/emud-cache`,
  `tools/devtools/verify_build.py`).

Test builds of `digi-emud` and `coupled-run` use the current PGO profile through `native_pgo.profile_for_tests`.
A missing or stale profile prints one warning: a stale profile is still applied to `digi-emud`, and `coupled-run` builds plain.
`DIGI_REQUIRE_PGO=1` makes this an error; the gate does not set it, because every core change makes the profile stale until a retrain.
`mise run test` and `test:plan` (tiers `merge` and `full`) print the DN2 and DT2 profile state before the first task and repeat a stale or missing warning at the end; this never fails the gate.
`uv run python -m tools.emulator.native_pgo clean [--dry-run] [--days N]` removes bench, ab, raw and train output and unreferenced profiles older than N days (default 14) under `out/native/pgo`. It keeps profiles named by `manifest.json` or the newest four `by-key` entries. `train` calls it at its end when `AUTO_CLEAN` is set.
`DIGI_EMU_PGO=0` turns PGO off. Measuring tools (`coupled_ab`, `sharc_matrix`, `perf`, `fastcov`, `fast_roots`) keep their own builds.

`tools/devtools/sharc_cells.py` compares host instruction counts of two built `coupled-run` binaries on runtime fixtures (`uv run python -m tools.devtools.sharc_cells A=BIN B=BIN --state dn2-reset --jobs 2 --cov`). It runs each binary at two lengths, prints steady-state counters per audio second, the B/A instruction ratio and PCM, DSP state and ColdFire state parity (`pcm/dsp/cf` per length; the ColdFire state is `state_sha256` of the `coupled-run` report, taken at the end of the run, see below; non-zero exit on a difference), and with `--cov` the fast-tier root and exit tables. `--env LABEL:NAME=VALUE` (repeatable) sets one variable for the runs of one label only (the cell environment is otherwise clean), for example `--env B:DIGI_SHARC_CORE_LIB_DN2=PATH` to run B with a loaded core library. The environment of a cell includes `DIGI_SHARC_CORE_LIB_<DEV>` from the core store (an explicit variable in the environment or in the label's `--env` wins), so a binary without a static core runs without `--env` (a static binary ignores it). `coupled_ab.py` (`--env LABEL:NAME=VALUE`, next to its `NAME=VALUE`) and `sharc_matrix.py run` (`--env LABEL:NAME=VALUE`, part of the resume identity) take the same per-label option (`tools/devtools/core_choice.py`). All three print which core each label runs, `both labels use core <key>` when it is the same one. A generated-core or transpiler change is compared by giving each label its library: build the B library with `verify_build cores` from the B gen dir (or point at an existing one) and set `B:DIGI_SHARC_CORE_LIB_<DEV>=PATH`; without it a core change is compared with itself. `sharc_matrix` provides libraries only for the devices of its cells and instruments. It does not take the bench lock, because counters are not timing; use `coupled_ab` for the final timing A/B.

```text
== dn2-reset (dn2, trig1-hold), lengths [3.0, 6.0] s, steady state per audio s ==
label    insn G    cyc G   aot M  fast M  interp  idle M   blk M  fcall M   B/A insns   cf G   B/A cf   rtf per length   parity (pcm/dsp/cf per length)
A        35.203    6.541   320.9   140.1    3.09   536.0    6.38     1.29   1.0000     11.530   1.0000    0.52/0.53        3s:=/=/= 6s:=/=/=
B        35.193    6.479   320.9   140.1    3.09   536.0    6.38     1.29   0.9997     11.536   1.0005    0.52/0.54        3s:=/=/= 6s:=/=/=
```

A `DIFF` in `pcm`, `dsp` or `cf` makes the exit code non-zero. In this run A is the plain `coupled-run` and B the same build with `cf-profile` compiled in and switched off: the DSP thread counts (`insn G`) match, and the ColdFire thread counts (`cf G`, host instructions per audio second of the ColdFire thread, from the report field `cpu.cf_thread_insns_per_audio_s`; `B/A cf` is the ratio to the first label) differ by 0.05%, which is the cost of the run-time toggle when off. A binary whose report lacks the field shows `-` in both columns. The ColdFire state digest (`Emulator::cf_state_digest`) covers the `save_state` bytes (CPU, RAM pages, DMA, DSPI and SSI link, eSDHC and card, timers, SD gate, UART bytes, the firmware-event counters, frame and input state) without the three instruction-path counters (interpreted, idle skipped, RAM clear skipped), then the state the snapshot lacks: the flash image and the flash write counts, the MIDI DIN port, the USB port (callbacks, queues with their contents, the active call, counters, the EP2 buffers, the controller error) and the pending and active guest calls. It leaves out instrumentation (spans, probe hits, `pc_probe`, traces, telemetry) and the SHARC side, which the DSP state sha covers. The board snapshot does not carry the USB PHY registers and their log, VBUS, the eDMA channel-37 transmit state, the soft-reset flag and the last write error. When the digest cannot be taken (a faulted emulator, `DIGI_CF_IPS` set, a pending IRQ1, a MIDI DIN transfer on the wire, FPU state) the report has `state_sha256: null` and `state_digest_error`, and `sharc_cells` fails that cell with the reason; so it does for a binary older than the digest (plan item "CF state digest in the A/B tool", PR 0 of docs/plans/emulator-core-split.md).

## Render cache

Scenario renders are cached under `out/cache/renders/` (`tools/devtools/render_cache.py`). The key
hashes the coupled-run binary, the input files, the scenario JSON, the peek specs, the device, the
device TOMLs, and every `SHARC_*`, `DIGI_*` and `DSP_*` variable except a denylist. A hit restores
the report entry and the WAV. Runs with `--diag` bypass the cache.
`--no-cache` or `DIGI_RENDER_CACHE=0` disables it; `uv run python -m tools.emulator.coupled cache stats|clear`
shows or empties it. Only emulator output is cached. Oracles, hash compares and pins run fresh.

Two guards protect against a stale hit:

- Audit: `tests/devtools/test_render_inputs.py` scans the crates linked into coupled-run. Every
  environment read must be in the key, in `DENYLIST` or in `IGNORED_ENV`. Counts of computed-name reads
  and runtime file reads are in `tests/devtools/render_inputs.json`. After review, regenerate with
  `uv run python -m tests.devtools.test_render_inputs --scan`.
- Spot check: `DIGI_RENDER_CACHE_CHECK=p` re-renders each hit with probability p and fails on any
  difference.

Set `DIGI_RENDER_CACHE=0` for release and publication gates.

## Nx cache

Patched-image device and conformance targets first run `build-objects` for the core modules in the
supported profiles' image closure. The plugin derives the `acceptance-image` tag from those profiles;
it is not a manually maintained module list. Stock-only targets and runtime host units have no such prerequisite.
The existing regression device target mixes stock and patched-runtime cases, so its prerequisites prepare the image too.

Each producer checks generated inputs with `patchkit gen --modules ID --check`, then runs the existing
incremental `patchkit build MODULE_ROOT` under shared benchlock. If generation is stale, run
`mise run gen:modules -- --modules ID`, inspect the generated diff, and retry. The module filter scopes
owner-generated files; SDK and firmware-interface generation is shared and is also checked or written.
Core objects are tracked `build/*.dko.json` outputs: a stale build may update them, and their diff needs review.
These producers are uncached; patchkit's source hashes skip current objects. Conservative default inputs
remain in place, so existing graph edges can overselect preparation; narrowing awaits clearer test ownership.
Repeated per-owner generation checks currently repeat shared SDK/interface checks. This is a future optimization.

The packed `.dks` and `.dkm` files, the port files in `dist/ports/` and `dist/ports-test/` and the image info tables are gitignored `dist/`
outputs, and Nx hashes a file input only when git tracks it. So the device tests and `core/runtime:test-assembly` name them as
`dependentTasksOutputFiles` of their `pack`, `ports`, `ports-test` and `device-images` dependencies: a changed ABI word changes the packed
bytes, and the key changes with it. The stock firmware files stay plain file inputs.

Catalog selection has a scoped proof: hypothetical edits to a shipped manifest and a fixture manifest both
select `patchkit:catalog-check`; an edit to Pluck's C source does not. This verifies selection behavior only.

`pnpm nx:pilot` keeps the task cache and the workspace data in `.nx/` of each worktree. The local cache is
per worktree; the "Remote cache" section covers sharing across worktrees. A hit
replays the stored output of a target with `cache: true`; the hash covers only the declared `inputs`.
Digest tools are inputs through their result, not their source text: `tools/nx/device-inputs.mjs` and `tools/nx/coupled-inputs.mjs` run as `runtime` inputs, so a format-only edit selects no device or FX task. Plugin sources (`tools/nx/*.mjs`) are not inputs: Nx hashes each project's resolved configuration, so a plugin
edit invalidates only the targets whose definition changes. `UPDATE_GOLDENS` is a cache input of module device
targets and `gate-stock:stock`.

A package or a harness file that no test imports is not an input: an edit to `packages/dk-session/src/samples.ts`
leaves a target that does not import it cached. Merge and narrow keys also leave out the driver packages (`DRIVER_ROOTS`), even when imported, except the files that hold device facts (protocol, log layout, emulator launch, key codes; `DRIVER_SUBJECT_FILES`). Files read by path are described in "What a device test's cache key contains". `digi-control:daemons` declares the daemon
sources (`SOURCE_GLOBS` of `tools/devtools/emud_key.py`) as inputs. The binary is keyed by content, and these
inputs make a source edit select every task that consumes `dist/daemons.json`.

### What a device test's cache key contains

Where the targets come from:
- `tools/nx/modules.mjs` is the inference plugin that creates the targets.
- `tools/nx/module-build.mjs` defines the build and pack targets.
- `tools/nx/device-tests.mjs` defines the target shape: one target per test file, a half for each device the file runs on, and a parent.
- `tools/nx/device-test-keys.mjs` is the key policy.
- `tools/nx/ts-imports.mjs` and `tools/nx/py-imports.mjs` compute import closures.

Three keys exist:
- Full key: all shared reads and all built outputs, including the patchkit binary. Used by `test-device-all`, `test-conformance-all` and `stock-all` (the full tier); the merge targets `test-conformance` and `stock` leave the driver out.
- Merge key: the key of files listed in `metadata.mergeTests`. It holds the bytes the test loads (its `.dkm` packs, the system package `dist/syspkg/runtime/system.dksp` that every card carries, `images-<device>.sha256`, the daemon key of the device) and the reads its import closure needs, without the driver packages. A half of it leaves out the files of the other device.
- Narrow key and merge key of a device test, and the merge key of `test-conformance`: one rule, one builder (`deviceInputs` in `tools/nx/device-test-keys.mjs`; `conformanceTarget` calls it). Builds key on sources, tests key on what they run. The key holds only the artifacts the test loads (the image hashes and the daemon test key of its half, `MERGE_BYTES`, and its packs: the packs the test loads for a narrow key, every uploadable module's for `test-conformance`, the patchkit key when the test runs patchkit live), its own test files and setup (the import closure of the test files and the Vitest setup, and for a test-only project that is a tests directory the files under it: `ownFiles`), the files its records list in `sources`, and `DRIVER_SUBJECT_FILES`; the other driver files are left out. No other source file is an input: not `src`, `include`, `fit`, `sdk` or the other files of the module (its pack carries them), and not the runtime or an image module (the image hashes carry them; a runtime edit that leaves the image bytes alone, such as a comment or `runtime/core/src/rs/Cargo.lock`, selects no device test). A test that reads a source file at run time, as `reload.device.test.ts` does through `packModuleVariant` (the files of the `sine` fixture), lists it in its record's `sources` (globs are allowed); a Python tool goes there too. For a test-only project the subject module is a fileset, `<subject root>/**/*`, without `src`, `include`, `fit`, `sdk`, `build` and `tests` at its top level, without the directories of nested projects and crates (found when the graph is built), without `*.md`, `Cargo.toml` and `Cargo.lock`, without test sources (`subjectInputs`), plus its pack output when it is packed (a service or content module); the half of a device whose image holds the subject follows the image hashes, and the other halves follow the pack or, when the subject has none, its `production` files. A fixture module that a record names in `patchModules` is built from source at run time and keeps its production files and the patchkit key. The full keys (`test-device-all`, `test-conformance-all`) are unchanged: they hold `production` of the project and the built binaries. Host tests: `tools/nx/tests/device-test-keys.test.ts`.

`affected`, `test:plan` and `test:subset` stay source based and list the coarse set; the cache keys decide what runs.

`mise run test` selects affected targets with merge keys. `mise run test:full` runs every file with full keys.

Files of a device without an acceptance cell (`devices/syntakt*.toml`, its ports and peer file) are no input of a device test key, except of a test that loads the whole registry (its import closure holds `packages/devices/src/node.ts`); the daemon test key leaves them out too (`tools/nx/device-files.mjs`, `emud_key.non_cell_device_prefixes`). A live test follows `dist/bytes/patchkit-test-<device>.key`, which holds only the files of its own device; a merge half leaves out the other device's files (`filesOf` in `tools/nx/device-files.mjs`, `forDevice` in `device-test-keys.mjs`), and takes `images-<device>.sha256` and `daemon-test-<device>.key` of its own device. Full keys keep them all.

Inputs computed from code:
- The TypeScript import closure of the test file. A barrel (a file of only `export ... from`, for example `packages/dk-session/src/index.ts`) is followed per imported name: `import { A } from '@digi/dk-session'` reaches the file that exports A. A namespace, default, side-effect, dynamic or `require` import takes the whole file. A file that a test loads through a barrel without using one of its names is not an input.
- The Python import closure of the `.py` tools that a record lists in `sources`. A test that runs a new Python tool must list it in the `sources` of its record. `tools.devtools.benchlock` is skipped (`NO_RESULT_MODULES` in `tools/nx/py-imports.mjs`): it only serialises runs and changes no result. The gate roots (`tools/gate/*/project.json`) list their Python by hand and leave it out too.

Inputs listed by hand are files that a test reads by path. No import walk sees them. They are in `READ_BY_TESTS` and `TEST_READS` in `tools/nx/device-test-keys.mjs`. Add an entry when a test reads a new file by path.

To inspect a key, use `pnpm nx:pilot show target inputs PROJECT:TARGET --json`, or `--check PATH` for one file. Inspection executes no target. It shows the declared hash plan, not every file a subprocess reads.

Before enabling or changing a cache, review source/dependency reads, outputs, toolchain and
environment identities, including setup commands and runtime digests. Run scoped behavior checks
appropriate to the change: invalidation for relevant inputs and restoration of declared outputs.
The remote `test-conformance` cache proof measured 20.9s on its first run (two cells), 1.8s on an
identical repeat (1/22 cache hit, the test target), and 14.5s after changing declared
`DIGI_NO_PROGRESS_MS=180000`. The module source input and sibling exclusion were inspected against
affected plans. The cached test target declares no outputs; pack and CLI producers remain
`cache: false`, so `dist/` output restoration is not yet applicable or proven.
Verify affected selection separately: a runtime hash does not select a task after an ignored input
changes. The former custom auditor/tracer was removed; its historical passes did not prove
subprocess closure or runtime-digest provenance. Unresolved coverage is not a cache-safety proof.

Cached targets: module `test-device` and `test-conformance`, `gate-stock:stock`, the gate emulator targets
(`gate-fx-dn2` `tier-quick` and `tier-sweep`, `gate-fx-dt2` `dt2-tier-quick` and `dt2-tier-sweep`, `gate-pins:pins`,
`gate-dt2-tone:dt2-tone`, `gate-card:card`, `gate-smoke:smoke`, `gate-fast-roots:fast-roots`; `gate-fast-roots:fixtures`
stays uncached, because it writes a content-keyed store outside Nx), `typecheck` (set in
`nx.json` `targetDefaults`; the `typecheck` of `regression` is not cached because its tsconfig reads every
module's device tests), `@digi/web:check`, `@digi/desktop:build:ui` and `check:ui`, `workspace:lint`, the
host tests, and `rust-tests` of the crates in `HERMETIC` in `tools/nx/cargo.mjs`
(coldfire, periph, emmc-card, dkneeds, dkrt-sdk, dkrt-fit, device-profile, dkupload, dksample, machine,
sharc-rt, dkmidi). Those run
`rustup run <mise rust> cargo nextest run -p NAME --release --locked --offline --target-dir dist/cargo-tests`.
Their runtime key includes the sorted present `CARGO_PROFILE_RELEASE_*` names and values, including
build-override options; changing these environment settings requires a direct target invocation when
there is no tracked change selecting the crate.
The service crates `services/*/dks` are no members of the root workspace (own `[workspace]` and Cargo.lock); `standaloneRustTests` in `tools/nx/cargo.mjs` gives the module project of every such crate an uncached `rust-tests` (`cargo nextest run --manifest-path <crate>/Cargo.toml --release --locked --offline --no-tests=pass`), found by the `[workspace]` table, so a new service crate needs no list entry. Its inputs are the project files, the production files of its path dependencies (transitively) and the toolchain. Run one with `pnpm nx:pilot run core/services/machines:rust-tests`; `core/services/fx` and `core/services/lfo` have no host tests and pass with none.
Other crates run uncached through `node tools/nx/rust-env.mjs --`; only `sharc-native` and the desktop crate
add `--gen`, because only they need the generated core. Crates without tests have no `rust-tests`
target. `workspace:fmt-check` is not cached: prettier also opens files it never formats (`.py`, `.rs`),
and nested `.gitignore` and `.editorconfig` files change its result.

Compiler depfiles provide separate evidence. For the `HERMETIC` `rust-tests`, run
`node tools/nx/cargo-depfiles.mjs` after the targets ran. It checks every repository file in cargo's
dep-info files under `dist/cargo-tests` against the crate, its path dependencies and its extra inputs.
Dep-info does not show run-time reads (`std::fs`), so a crate is `HERMETIC` only if its tests read no
firmware, generated core or `out/` data.

### Owner hashes (`patchkit owners`)

`patchkit owners` writes one hash per owner of the runtime object. The runtime is one object, so `runtime/core/owners.toml` maps the module
path prefixes of its items to tags (`C:storage`, `S:lfo`, ...) and lists the `shared` set (the standard library, `dkrt_abi`,
`dkrt_sdk`, the entry points, the platform and others). The longest prefix at a `::` boundary wins. A C symbol is shared. An anonymous
constant (a string, a table) belongs to the owner of the items that use it when they have one owner, and is shared otherwise. No build
or test selection reads the hashes: the device gate selects by project ownership ("Nx gate"), and the `patchkit:runtime-owners`
target is gone. The command and the map stay for diffing the runtime object by owner (`patchkit objdiff --owners`) and have a patchkit
test (measured: [test-architecture](plans/test-architecture.md#t1e-service-scoped-selection)).

```text
$ patchkit owners runtime/core/build/runtime.dko.json --out out/owners
C:boot            22 items  dde8387366edca03
C:diagnostics     41 items  99fa3153dedc8bf2
C:dsp             25 items  643005221cbfde5b
C:loading         82 items  7f3408b9070fbf87
C:state           23 items  600322bec191c865
C:storage        119 items  825506877527224f
C:tick             1 items  d296515fc179902e
C:transport       37 items  8d22528f8e08036f
S:lfo              0 items  e3b0c44298fc1c14
shared           472 items  a7528f325952d65c
```

The command writes `<tag>.sha256` for every tag and `shared.sha256` into `--out`. `--owners FILE` names another map (default
`runtime/core/owners.toml`). Each hash is over the sorted position-free signatures of the owner's items (the item signature of
`patchkit objdiff`), so moving an item inside the object or inside its owner changes nothing; an inlined callee changes its caller's owner.
A Rust item that no prefix matches is an error that lists the module paths (two components) with their item counts: add them to
`owners.toml`; when unsure, add the path to `shared`.

`objdiff --owners owners.toml OLD NEW` adds the owner of each differing item (`[C:storage]`, `[shared]`), so it shows why an owner hash changed.

### Daemon store

`verify_build daemons` (the `digi-control:daemons` target) keeps the coupled `digi-emud` daemons in
`<git common dir>/emud-cache/<dev>/<key>/` (`digi-emud`, `key.txt`, `depfile.d`). All worktrees of the
clone share it and it is never committed. A worktree with the same key restores the daemon and builds
nothing; a stored file is never rewritten. The last 4 used keys per device stay (a restore refreshes the use time),
and so does every key that `dist/daemons.json` of any worktree of the clone points at: the store holds at most 4 plus one per active lane.
`digi-control:daemons` has `cache: false`, so every run restores or rebuilds the entry. `DIGI_EMUD_STORE` moves the store (tests only).

The key is printed by `uv run python -m tools.devtools.emud_key key --device DEV`: a sha256 over the
content of the repository files in `SOURCE_GLOBS` and `RECIPE_FILES`, every file of the generated core
directory (resolved as `pk_emud_pgo` does), the PGO profile, the `tools.rust` pin from `mise.toml`,
`rustc -vV` of that toolchain, and the build flags. Task definitions in `mise-tasks.toml` are excluded.
Narrowing this input changes the key once, so existing stored daemons miss on the next request.
A device without its stock firmware or ready directory is skipped (`SKIP` on stderr); the command fails when no daemon is built.

Each build audits itself: `verify_build` compares the dependency file cargo wrote (`digi-emud.d`) with
the key. A dependency that the key does not cover (and that is not under the cargo target dir) fails the
build, so nothing is stored with an incomplete key. Add the missing path to `SOURCE_GLOBS` in
`tools/devtools/emud_key.py`. `uv run python -m tools.devtools.emud_key audit --device DEV --depfile FILE`
repeats the check on a kept file.

### Core library store

A device with a fresh PGO profile gets a profile-guided library as a store entry of its own key (the profile content is in the key,
the build is `-Cprofile-use` with `--target`, in the target dir variant `pgo-lib-use`). `native_pgo train` builds the instrumented
libraries (variant `pgo-lib-instrument`, never stored) and loads them in the training run, so the profile covers the host and the DSP
hot loop (`packages/emulator/README.md` "Profile-guided libraries"). `verify_build cores --pgo` provides them by hand; `verify_build
daemons` gives a PGO daemon the library built with its profile and records the plain entry as `DIGI_SHARC_CORE_LIB_<DEV>_PLAIN`
when it is stored, so the eviction keeps both.

`verify_build cores` keeps the generated SHARC core of each device as a library in
`<git common dir>/sharc-core-cache/<image>/<key>/` (the library, `key.txt`, `header.json`; the image is the device id). An
entry is a hit only with all three files. It uses the mechanics of the daemon store (`tools/devtools/store.py`): insert by one
rename, so two processes that insert one key are safe; the use time is the mtime of `key.txt` (a hit refreshes it, never
creates it). Eviction keeps the last 4 keys per image, every key that `dist/daemons.json` of any worktree names, and every entry
used within the last hour; it renames an entry to `.tmp-evict-*` before it removes it and removes `.tmp-*` directories older
than an hour. `DIGI_SHARC_CORE_STORE` moves the store (tests only).

Key and build come from one description, `core_store.build_plan` (the argv and environment of the cargo call). The key follows the
daemon key's scheme (`emud_key.key_of`): the content of the repository files in `CORE_SOURCE_GLOBS` (the `sharc-core-lib`,
`sharc-gen` and `sharc-rt` crates, `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`, `.cargo`), every file of
the gen dir, `rustc -vV`, the target triple, the argv without the target dir (profile, `-p sharc-core-lib`, features) and the rustc flag, `CARGO_PROFILE_*` and `CARGO_TARGET_*` variables of the build environment. The tooling scripts (`core_store.py`, `digi_env.py`, ...) are not in the key: what they decide is in the plan, so an edit that leaves the plan alone keeps every stored library. The key is known before the first build, so
a lookup needs no build. After a build the dependency file of cargo is audited against the key: a file the build read that the key
does not cover fails the build and stores nothing (add it to `CORE_SOURCE_GLOBS`).

```sh
uv run python -m tools.devtools.verify_build cores --device dn2,dt2 [--env-out FILE]
# dn2 core: built 959db07d28b2        (a miss: builds and inserts)
# dn2 core: restored 959db07d28b2     (a hit)
# dn2 core: evicted 91350c44d39f      (a key beyond the last 4 that no worktree names and that was not used within the hour)
```

The build is the documented one, `cargo build --release --locked -p sharc-core-lib` with `SHARC_GEN_DIR` set to the device's core
(`SHARC_GEN_DIR_<DEV>`, for DN2 also `SHARC_GEN_DIR`, else `out/native/<dev>-audio/gen`), in the target dir of that core, so a
compiled `sharc-gen` of that core is reused. A stale generated core is refused as `tools/nx/rust-env.mjs --gen` refuses it. The
library is always a plain build; a profile-guided host accepts it, because the header leaves the PGO flags out.
`header.json` holds the device, the image name (the `IMAGE` const of the gen dir's `image.rs`), the key, the build flags and the
header of the built library (read through ctypes from `sharc_core_header`: ABI version, `sharc-rt` sha256, rustc, target, flags,
layout). The audit of cargo's dependency file runs on every build.

`verify_build daemons` also provides the library of each device (a daemon holds no static core; a library that cannot be built
fails the command with the build command in the message) and writes `DIGI_SHARC_CORE_LIB_<DEV>` (the store path) into the same `--env-out` file
(`dist/daemons.json`); the key of the library is part of `dist/bytes/daemon-test-<device>.key` (and the device's line in `dist/bytes/daemon-test.key`), so a core change
invalidates the merge keys of device tests. The variables reach the daemons as follows: `packages/test-support` (`coreLibEnv`,
used by `Session.start` for a session with DSP) reads them from the environment or `dist/daemons.json` and passes them to the
daemon; `tools/nx/daemon-env.mjs` puts the whole file into the environment of the Python harnesses. Every other launcher calls `core_store.library_env` (the same `ensure_core`): `tools/emulator/native_emu.py` for the cores of a
coupled launch (the desktop app and `--daemon`: the libraries of both devices, so switching device in the app works; one device's failure prints a line and the launch goes on with the other), `tools/emulator/coupled.py`
(`run_env`: `scenario`, `replay`, `bisect`, `coupled_ab.py`, `fastcov.py`; `cmd_gates`; `--id-out` only restores a library and records `core: none` when none is stored; the gate targets see a core change through `tools/nx/coupled-inputs.mjs`, which hashes `core_store key` of each device or an explicit library by content), `tools/devtools/sharc_cells.py`,
`sharc_matrix.py`, `rt_matrix.py`, `dual_core_check.py` and `Emu.spawn_coupled`; a variable already in the environment wins and
nothing is built for that device. A library that cannot be built stops the launch with the build command. The hosts
(`digi-emud`, `coupled-run`, the app) build into the no-core target dir and compile no `sharc-gen`; the core library, the unit
tests of `sharc-peer` and `sharc-native`, `sharc-rig` and the browser core link it. Region lists of a DT2 run use the `_2` names
(`digi_env.resolve_tiers`, `coupledEnv(syx, env, device)`). `tests/devtools/test_core_store.py` covers the key and the store with a
stand-in library.

To clear the Nx cache: `pnpm nx:pilot reset`.

## PCM pins

A PCM pin (`tests/scenarios/*.json`, the device pins `<case>.pcm.json`, the rig pins) is set or changed from a numeric report of the captured signal (pitch against the expected note, crest factor, harmonics, level envelope, onset, stereo match), with the WAV kept for listening; never from a hash alone. Claude approves when the report matches what the fixture should produce; a change in how a module sounds goes to Em. The harness keeps the capture: a pending or changed device pin prints its artifacts directory, which holds `run.wav` (stereo float32, 48 kHz). The report for a pin names the expected values (for `test/sine` at note 60: 261.63 Hz, crest 1.414, no harmonics, a steady level) beside the measured ones. Fingerprint pins (a numeric fingerprint with tolerances compared by a tool, replacing the hashes) are written by the pin CLI of `@digi/sharc-rig-kit` (docs/plans/test-architecture.md T2).

`tests/sharc/integration/test_sharc_golden.py`
hashes six SHARC core outputs: a refactor keeps them; an intended change updates them with
`--update` and says why.

Rig pins and device pins are numeric fingerprints compared with tolerances by one comparer (`packages/sharc-rig-kit/src/pcmpin.ts`,
import `@digi/sharc-rig-kit/pins`). A hash alone never sets a pin.

**Fingerprint v2** (plain numbers, no audio; the reader still accepts v1, comparing its onset, peak, bands and block levels):

| Field | Meaning |
| --- | --- |
| `rate`, `channels`, `frames`, `window_frames` | the format; `window_frames` is the number of frames analysed from the onset (device pins: 12000 = 0.25 s; rig pins: null, the whole output) |
| `onset` | first frame with a sample above 1e-6 |
| `peak_db` | peak, dBFS |
| `stereo` | `identical` (L = R) and `max_diff` (largest abs(L-R)); two channels only |
| `rms_db` | level of each 10 ms window, dBFS |
| `bandHz`, `bandDb` | 64 log-spaced band levels, 40 Hz to 22 kHz, of the channel mix |
| `periodic` | only when the fundamental and harmonics 1..6 hold 90 % of the energy: `pitch` {`hz`, `midi`} from the parabolic peak of the zero-padded Hann spectrum, `crest` (peak over RMS) and `harmonics_db` (2..6, dB re the fundamental) |
| `block`, `block_rms` | rig pins: RMS per 32-sample block (the sample classifier below reads it) |
| `tolerance` | the fields below that differ from the defaults |
| `exact` | `{ sha256 }`: the pin is exact |

**Tolerances** (defaults tight enough that a change you can hear fails): `onset` 0 frames, `level_db` 0.1 (peak and each 10 ms window),
`band_db` 0.5 (bands more than `band_floor_db` = 60 dB under the loudest band are skipped), `pitch_cents` 1, `harmonic_db` 1
(harmonics under `harmonic_floor_db` = -80 dB in both signals are skipped), `crest_pct` 0.5, `rms_floor_db` -90 (a 10 ms window under it in both is skipped).
`compare(pin, capture)` returns the differences in plain words and an empty list when the capture is within tolerance:
`pitch +14.0 cents`, `level -3.1 dB from 200 ms to 250 ms`, `harmonic 3 rose to -38 dB`, `onset moved 32 frames`,
`bands moved: 102-20942 Hz -2.0 dB`, `L and R differ now`. A test fails with that list.

`exact`: the pin also needs the SHA-256 of the analysed float32 words (the fingerprint differences are listed all the same, or
"only bits differ"). Use it only where the test proves identical output: the two `freqshift` state-independence cases
(`exact = true` on the `same-as` check) and the device pin `equiv-dn2-set` (exact RECIPS output, finding 102; written with `--exact`). The machine note pins (`equiv-note.toml`) and `equiv-dt2-sine` are tolerant: a refactor that keeps
the sound within tolerance passes without a re-pin.

**Rig pins**: `tests/rig/fingerprint/<key>.json` (key: `rigKey`). A `same-as` check with a v2 file is compared with its tolerances;
`exact = true` on the check, a v1 file or no file keeps the hash as the gate. The `sha256` line of the toml stays as the record of the last pin.
`DIGI_RIG_PIN=1` (above) writes the v2 file; a tolerance in the old file stays, and an exact check stores the hash in it.

**Device pins**: `runtime/core/tests/machines/pins/<case>.pcm.json` (`pcmPinFile`), checked by `defineEquiv` over 12000 stereo frames
from the onset (`checkCapture`). Cases of a plain sine (`sine: { note }`) also assert properties that hold whatever the pin says:
pitch within 5 cents of the note, crest factor of the second half of the window within 1 % of sqrt(2) (`crest_settled`: the amplitude of the DT2 sine starts 1.7 % high and settles within about 0.15 s, which the whole-window crest includes), harmonics under -60 dB, level steady within 0.5 dB.

**Equiv pins are onset-aligned.** A device pin of `defineEquiv` compares the signal from the onset on (12000 frames, fixed length), so an exact pin's SHA-256 is taken over the onset-aligned signal. The onset stays in the fingerprint and the report, and it may move by whole DSP blocks, at most 2 (`block` of `checkCapture`: `deviceBlockFrames(device)`, the sample rate over `dsp_blocks_per_second` of the device profile, 32 frames on DT2 and DN2). A shift of another size, or of more than 2 blocks, fails. Reason: a change that makes each block cheaper (the fx stubs of R12-8) moves the block in which the key press lands, so the note starts a block earlier or later; the signal from the onset on is unchanged (findings 45, 57). A pin written after such a shift records the new onset.

**Failure and report**: a failed or missing pin prints the differences and the full report, expected beside measured (format, onset, peak,
stereo, level range, loudest bands, pitch, crest factor, harmonics), and writes `<case>.plot.png` (the window and the pin's band curve) and
`<case>.window.wav` beside the capture (`out/runs/...` of the case for a device pin, `out/rig/<key>/plot.png` and `out.wav` for a rig pin) for listening.

**Approving a pin**: read the report; Claude approves a pin that states the fixture's own facts (for the DT2 sine: 261.63 Hz = MIDI 60, crest
1.438 over the window and 1.412 in its second half, harmonics -115 dB, peak -23.9 dBFS, level within 0.4 dB, L = R) and a change that keeps the sound; a change of the sound goes to Em. Then write it from the capture
(the WAV the device test keeps beside the run, `run.wav`, or the window WAV) with the CLI:

```sh
node packages/sharc-rig-kit/src/pin-cli.ts show  --wav CAPTURE.wav --pin PIN.json   # report and differences, exit 1 on a difference
node packages/sharc-rig-kit/src/pin-cli.ts write --wav CAPTURE.wav --pin PIN.json [--window 12000] [--exact] [--tol pitch_cents=3,level_db=0.2]
```

## Desktop UI preview

`mise run wb:preview` simulates the Transfer content packs without a ColdFire compiler. Use `mise run wb:preview:real` to exercise actual compiled packs. Both serve the desktop pages
(`apps/desktop/ui`) at http://127.0.0.1:5199/ over a fake device (state in the address:
`?dev=ok|empty|noreply|error|slow&folder=files|empty|none&speed=MiB/s&mode=modules|samples`).
`node tools/devtools/ui_shots.mjs --out DIR [--prefix before] [scene ...]` writes one PNG per scene at
1440x900 and at 700 px wide (`--list` lists scenes). Look at the pictures before and after a UI change.
`?emudev=<id>` shows another device's toolbar on the Emulator page (default `dn2`).

`mise run readme:shots` regenerates the workbench screenshots in README.md: it runs the `readme-*` scenes at full size and as thumbnails, writes `docs/images/workbench-*.png` and rewrites the table between the `readme-shots` markers in README.md. The Emulator and Remote shots draw `apps/web/public/images/device-*.png` on the faceplate screen. `mise run readme:shots -- --device` first recaptures those device screens from a booted DT2 emulator (`mise run site:shots`, about 2.5 min); the site uses the same images.

The Firmware page (`?page=firmware&fwdev=dt2|dn2&fwstate=stock|runtime|noreply|none`) runs the real patchkit checks (`patchkit.wasm`) over a fake
device that restarts after the last message. `node tools/devtools/firmware_shots.mjs --app desktop|web [--url URL] [--out DIR] [scene ...]` takes its
pictures for the workbench (this preview) and for the site (`astro dev` or `astro preview`, with a fake Web MIDI port); the firmware shots need the stock files in `firmware/`. The Workbench preview prepares `patchkit.wasm` and the module bundle at startup.
The same flash against an emulated device at OS UPGRADE: `os_upgrade_rig.py ... --exec` runs `apps/web/tests/firmware-emulator.wasm.test.ts` (finding 75).

To test pages against a running emulator, `uv run python -m tools.devtools.emu_live --device dn2 --run-dir DIR`
starts a daemon, `apps/desktop/ui/preview/live.vite.config.ts` serves the pages over it
(`DIGI_LIVE_SOCKET=SOCKET`), and `node tools/devtools/memory_live.mjs SOCKET [--check]` measures the
Memory panel.

## Waiting on runtime events and reading the screen

- `rt.listen()` and `events.until(kind, match, { withinMs })` read the runtime log; the log commands exist only while `digikit/debug` is loaded (`installWithDebug`).
- `ringWatch` (`tests/regression/runtime/runtime-menu.ts`) reads the runtime's small ring (32 events) straight from guest memory, so it works without the debug module, for
  example in a card rig whose runtime has none (`CARD_SYMBOLS` of `card-rig.ts` carries the symbols). `next(kind, match, { withinMs })` waits in emulated time; poll every
  100 ms, as the ring is short. Events a test must see are logged with `DK_LOG_F_KEEP`, which the ring keeps.
  Use `rt.listen()` when the debug module is loaded anyway, `ringWatch` otherwise.
- `viewStackFacts(device)` and `readViewStack(read, facts)` (`packages/test-support`, `boot-window.ts`) give the firmware's view stack from guest memory, the top view first (object, class
  pointer, opaque byte): compare the identity of the top view, never count views. `readTopMenuItems(read, facts)` gives the item count of the top view's stock menu (SETTINGS).
  `emulatorRead(session)` is the `read`. A screen is never read from pixels.

## The boot window

On a device whose firmware ends its boot with a ConfirmWindow (the Digitakt II: "FILE SYSTEM OK / PRESS Y/N TO CLOSE", after the filesystem check
of a boot with a card), that window is the top view and takes every panel key until NO or YES answers it. A panel-driven test that starts before
it is answered sends its keys to the window. The daemon's `auto_no` presses NO once at "ready", without looking at the screen: with relaxed
readiness (`relaxFs` card rigs) the press can land while the "VERIFYING FILE SYSTEM..." progress view is on top, and the window opens later.

`settleBootWindow` (`packages/test-support/src/boot-window.ts`) answers it from firmware state. It reads the top of the firmware's view stack
(the facts are the `[ui]` and `[ui.boot_window]` tables of the device file; a device without `[ui.boot_window]` has no window and the step does
nothing). The firmware decides about the check when the main screen opens: `MainScreenView::vfunc_5` starts it only if the filesystem is mounted
and the superblock state word is 3 or its checked word is 0 (the "check pending" test of the facts); a successful check writes state 4 and checked 1 back to the
superblock. Test cards are built verified (`build_superblock` of `plusdrive-format` writes state 4 and checked 1), so on a card built by our tools the check pending test is false at the first main screen and neither the progress view nor the window appears. The step stays as a guard for cards that are not verified (a card image from another source, a superblock the firmware rewrote to checked 0).
So the step ends on firmware state, never after a quiet time. "Main screen reached" means the bottom view of the list is the MainScreenView: an overlay (the
help bubble) sits above it whenever no modal is open, so the main screen is not always the top view. Each poll: the main screen at the bottom with no progress view
or window on top and the check not pending is done; with the check pending it waits (for the progress view and the window, however late they come); a progress view
means the check runs, so it waits; an unanswered window gets NO (again after 1 s if it is still there) and the step waits until the window is gone; the main screen
with the check still pending after the window was answered is also done. No Brain, an empty view list, another bottom view and a read that fails (an unmapped
address, a pointer that is not set yet) are "not yet": the step reads again at the next poll. If the check ends without the window (another ConfirmWindow is on top
after the progress view while the check stays pending), the log and the error name its vtable pointer and title ("the filesystem check ended without FILE SYSTEM OK").
It fails after 60 emulated seconds with the sequence seen, and logs the sequence once
(`boot window: none @0 ms -> main(check pending) @... -> progress @... -> boot-window(unanswered) @... -> tap NO @... -> main(check not pending) @...`).
The step takes a target with `read(addr, len)`, `ui.tap('NO')` and a clock, so a device can use it with `DkSession.peek`; `emulatorBootTarget` is
the daemon's target (`mem.cf.peek`, the panel, the emulated clock).

Runtime sessions of a device that has the facts run it themselves: `Session.start` and `reloadRuntimeBaseline` settle after ready and do not ask the
daemon for `auto_no`, and the card rig's `restart` settles once the new boot has reached its main screen, before the rig's UI checks. `autoNo: false`
turns it off. A device without the facts, a stock session and a paused baseline (`hold`), whose steps a test counts from the ready point (settling
would step the emulator), keep the daemon's press of NO at ready as before. A test that restarts the device in another way calls `session.settleBootWindow()`
after the boot reached the main screen. The rig's restart guard (daemon epoch and `main_ui_reached`) is emulator-only: a device run needs its own "new boot" signal
(for example a changed Brain pointer). The address facts are checked against the port files by `tests/regression/host/device-facts.test.ts`.

## Known issues

- fx-contract: the onset shift varies by +-64 samples between identical runs (free-running coupled
  emulator); the tolerance is 128.
- DN2 chorus-width sweep assertion (full `fx-dn2` tier): an open question whether the oracle or the
  DSP is wrong. See [tests/regression/scenarios/README.md](../tests/regression/scenarios/README.md).
- `tests/sharc/generation/test_sharc_pcode_language.py::test_temporary_artifacts_match_in_tree_when_present`
  fails: the in-tree Ghidra `.slaspec` is stale versus the generator.

## Resumable SHARC matrix

`mise run perf:matrix` runs correctness before paired timings and resumes from
saved artifacts. For an environment-only experiment, reuse the **same frozen
executables** on A and B and put the differing settings in the configuration's
`variants` object. `run` and `status` never build emulator binaries:

```sh
mise run perf:matrix -- run out/matrix/config.json --baseline out/matrix/frozen --candidate out/matrix/frozen --out out/matrix/results
mise run perf:matrix -- status out/matrix/results
```

To freeze existing binaries without Cargo, use `freeze --import-manifest FILE` as
described below. Source changes require an explicit build: freeze A before editing
native sources, then freeze B after the change. Each destination must be new; the
command never switches source revisions.

```sh
mise run perf:matrix -- freeze --out out/matrix/baseline
# Make the candidate source change, then explicitly build and freeze it:
mise run perf:matrix -- freeze --out out/matrix/candidate
mise run perf:matrix -- run out/matrix/config.json --baseline out/matrix/baseline --candidate out/matrix/candidate --out out/matrix/results
```

Freeze builds the existing coupled-run and coupled daemon targets for both devices,
under the shared benchlock and target build locks, then copies the executables into
the destination. Generated cores and current stock firmware must already exist.
For an existing build, `freeze --import-manifest FILE` avoids Cargo. The manifest is:

```json
{
  "dt2": {"coupled": "/absolute/coupled-run-dt2", "daemon": "/absolute/digi-emud-dt2", "profile_firmware": "/absolute/stock-dt2-1.17.syx"},
  "dn2": {"coupled": "/absolute/coupled-run-dn2", "daemon": "/absolute/digi-emud-dn2", "profile_firmware": "/absolute/stock-dn2-1.12.syx"}
}
```

Imported executables have unknown source provenance; current sources are recorded
as the freeze tooling identity, not claimed as their build inputs. Each device's
fast-region defaults are resolved from `profile_firmware` at freeze time and stored
with its hash. For a historical baseline, supply an optional `env` object on each
manifest device to replace those defaults with its previously recorded values.
Runtime replay images use these stock defaults even when their patched hash has no
entry in the current fast-region table.

The run configuration lists prepared replay cells; it creates no replay fixtures:

```json
{
  "pairs": 3,
  "seconds": 3,
  "timeout": 300,
  "instrument_timeout": 1800,
  "instruments": ["dt2", "dn2"],
  "env": {},
  "variants": {"A": {}, "B": {}},
  "cells": [{
    "id": "dt2-stock",
    "device": "dt2",
    "kind": "stock",
    "inputs": {
      "firmware": "/absolute/stock-dt2-1.17.syx",
      "DSP image": "/absolute/dsp.bin",
      "DSP state": "/absolute/dsp.state",
      "CF snapshot": "/absolute/cf.snap"
    },
    "card": "/absolute/card.img",
    "scenarios": "tests/scenarios/dt2.json",
    "names": ["dt2-sample-trig"],
    "env": {},
    "variants": {"A": {}, "B": {}}
  }]
}
```

Add cells for DT2 runtime, DN2 stock and DN2 runtime to complete replay coverage.
`card` is optional; scenario sources may be JSON or executable TypeScript. IDs and
scenario names must contain only letters, digits, underscores, dots and hyphens,
starting with a letter or digit. Environment precedence is frozen device defaults,
run `env`, cell `env`, run label overrides, then cell label overrides; a JSON null
unsets a variable. Only emulator prefixes (`SHARC_`, `DIGI_`, `DSP_`, `DIGIKIT_`) and
OMP/Rayon/OpenBLAS thread counts are accepted. Inherited values in those namespaces
are removed before applying the recorded values. Values beginning with `@` name
files whose full contents are included in the resume identity.

The preflight checks all replay inputs and selected scenarios before starting a
child. Each replay exports the full WAV and SHRD state. Every sample must exit
successfully, pass report gates, produce finite positive audio duration and thread
CPU, and match the report's full PCM hash. A/B must agree on full PCM, complete
SHRD hash, final DSP instruction count and actual audio duration. Requested duration
allows normal native runner frame overshoot. Both labels then run
`equiv-dyn-pluck` on each selected device with their frozen daemon; the existing
scenario gates must pass and both full `run.wav` and `ref.wav` PCM hashes must agree
across labels. The PLUCK helper prepares its normal patch/module/card artifacts;
only emulator binaries are frozen by this command.

Any correctness failure stops before timings. Timing pairs alternate A/B then B/A
under one exclusive benchlock, using the same validated sample helper as
`coupled_ab.py`. Each timing sample must also match its earlier correctness result.
The JSON summary records B/A ratios and tier counts; it makes no performance
acceptance decision or automatic rollback. DSP patch experiments remain available
through `coupled_ab.py --patch LABEL=FILE`. Two patched ColdFire images (for
example `main` against a branch with a changed runtime) run with
`coupled_ab.py --image A=MAIN.syx --image B=BRANCH.syx`: build each with
`pnpm nx:pilot run patchkit:device-images` in its lane (the files are
`dist/bytes/images/<device>/<id>.syx`; check `dist/bytes/images/<device>/` for the id), copy them
away, then

    mkdir -p out/ab    # copy main.syx and branch.syx there
    uv run python -m tools.devtools.coupled_ab A=BIN B=BIN \
        --image A=out/ab/main.syx --image B=out/ab/branch.syx -s trig1-taps --pairs 6

Both labels need an image. Each image boots once with `coupled-run --cold-base`
and keeps its ready pair under `out/runs/coupled_ab/image-*/cold`; the timed
runs restore it. PCM must be identical across all runs and both labels, or the run fails. The matrix requires strict full-state
equivalence and does not support arbitrary firmware patches that intentionally
change the exported state.

Repeating `run` resumes completed steps. Config, consumed inputs (including cards
and environment files), binary hashes, effective environment, device tables,
scenario documents and runner/source hashes must match. Reports, WAVs, SHRD dumps,
argv, filtered environment, logs and kept instrument artifacts are retained;
completed artifact hashes are rechecked, and checkpoints are atomic. A failed or
interrupted pair restarts both sides. One process may write a run directory at a
time. `status` validates the recorded artifacts before printing compact JSON.

Full coverage requires declared stock and runtime cells plus PLUCK for **both**
devices. A bounded pilot may use `--allow-partial` and `"instruments": []`; its
successful status is always `partial-pass` with missing coverage listed. Such a
result cannot stand in for full validation. Firmware and generated artifacts stay
under ignored output directories and must never be committed.
