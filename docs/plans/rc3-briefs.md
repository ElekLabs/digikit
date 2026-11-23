# RC3 worker briefs: lean scope (architect pass, 2026-11-23; read-only, nothing built or run)

Plan: [runtime-contract-and-lifecycle.md](runtime-contract-and-lifecycle.md), rows "RC3 briefs" and "Canonical service layout". Code read on `rc/rc2s-2c` (lane fx-route-policy). Re-find every path with `git grep` at the base; RC2s moves files.

Marks: [V] read in the tree in this pass; [D] taken from the scout draft, not re-read.

## 1. Scope (decided by Em 2026-11-23)

- RC3 ports the six services (shortcuts, samples, lfo, project, fx, machines) to the Provider contract on one branch. No LegacyService adapter. One device gate, one PR.
- Commit order: contract path; shortcuts (template); samples, lfo, project, fx in parallel worktrees branched from the template commit, cherry-picked after review; machines last; delete the old `Service` trait and macro.
- Not in RC3: header table v4, the typed-address cleanup. Machines stays separate from the RC4 machines point.
- Tests: the contract is tested once on the fixture; each service has unit tests with fakes for its own logic; device tests only for firmware facts; the existing device tests are the proof and stay unchanged.

RC3 is behaviour-neutral. The proof is: the boot-effects snapshots do not change, the contract suite passes per service, and the existing device tests pass unchanged.

## 2. State of the code (verified)

- Contract types exist and nothing uses them: `runtime/core/abi/src/provider/{contract,parts,consumer,declarations,row}.rs`. `PROVIDER_ABI = 1`, tags 1-6 (`row.rs:7-15`), `check` and `CheckedRow::part` with `TooNew` (`row.rs:100-177`). [V]
- The old trait has 25 callbacks plus `validate_records` (`abi/src/service.rs:556-662`); `ServiceRow` has 24 function pointers and no `validate` pointer (`service.rs:706-738`); `ServiceTable` holds the walks (`service.rs:999-1345`). [V]
- The Provider traits are much narrower: lifecycle 6 methods, `Records::validate`, `DspPart` 2, `Ticker`, `Query`, `Describe` (`provider/contract.rs:6-28`, `provider/parts.rs`). They do not cover the check steps, the per-record apply, the commit phases, `settle`, `live_done`, `cap`, `attribute`, `diag`, `table_check`, `on_loaded` or 6 of the 8 DSP steps. [V]
- What each service implements [V]:
  - shortcuts: validate, declare, tick, module_begin, module_commit, check, apply_record, remove (`services/shortcuts/src/records.rs:77-214`).
  - samples: on_loaded, tick, dsp_check_sections, dsp_admit, dsp_claim, dsp_release (`services/samples/src/records.rs:11-41`).
  - lfo: validate, declare, settle, check, apply_record, remove (`services/lfo/src/records.rs:427-594`).
  - project: nothing; its work is in `dks/src/glue.rs` behind `setup`, five hooks and the module-record `tick` (`services/project/src/lib.rs:22-25`, `dks/src/lib.rs:13-29`).
  - fx: 16 callbacks (`services/fx/src/records.rs:25-159`) and `query = rt::dk_service_query_v1` (`dks/src/lib.rs:34`).
  - machines: validate, declare, settle, check (7 steps), apply, apply_record, live_done, remove, module_begin, module_commit, tick, cap and the DSP set (`services/machines/src/records.rs:134-273` and on).
  - All six use `accepts_teardown!` (shortcuts `records.rs:82`, samples `:14`, fx `:33`, lfo `:432`, machines `:139`, project `lib.rs:24`).
- Record order across services matters: the host runs every row's `apply`, then each record in file order to the row that owns its kind (`runtime/core/lifecycle/src/seams/host.rs:28-31`, `package/registry.rs:45-60`).
- The runtime gets a row through `dk_service_declare_row` (`abi/src/core_imports.rs:80`), `Registry::add` (`package/loader/seams.rs:50`, `:142-147`) and `svc_rows::add`; the table is sorted by callback order (`package/svc_rows.rs:51-85`). `gen_services::STATIC = 0`: there is no in-crate service (`package/gen_services.rs:11`).
- `svc_rows.rs:13-23` holds the callback-order fallback that names the six services. `callback_order` is in each manifest (`services/shortcuts/dks/module.toml:8`).
- `service!` is in `runtime/core/svc-rt/src/lib.rs:382-466`; `dk_module.fini` is `None` (`:441`); `setup` runs before the declarations (`:425-436`).
- patchkit depends on the service code in two places: `packages/patchkit/src/record_rules.rs:16-19` names `dkrt_svc_<name>::records::<Type>::validate` for lfo, shortcuts, fx, machines; `packages/patchkit/src/svccheck.rs:10-24` reads the `service!` call and the `d.kind(...)` lines of `Service::declare` with regular expressions.
- Statics named `DKRT_*` in a service are read by device tests by name (`svccheck.rs:21` names `DKRT_FXP`, `DKRT_FXS`).
- The shared contract suite already exists: `runtime/core/test-support/src/contract.rs` (`contract::<S>(&Fixture)`, `S: Service<FakeCore>`), and every service runs it in `tests/contract.rs`. The plan says RC3 reuses it (plan section 3, "Relation to other plans").
- `tests/fixtures/modules/local-ping/src/lib.rs` is a Rust service on the old trait with one export.
- `test/svc-test` and `test/svc-echo` are C service files with no row. A service file without a row stays valid (`seams.rs:143-144`).
- Correction of the draft: `DKRT_PROVIDERS` holds the module records of the provider files, not service rows (`package/loader/records.rs:2`, `package/host_dir.rs:17-18`). RC3 does not change its layout and adds no host-directory id.
- Correction of the draft: `services/shortcuts/README.md` and `src/shortcuts.rs` (the `Shortcuts` state object) exist already.
- ColdFire rule seen in the code: use an if chain, not a `match` on dense integers; a jump table is not valid ColdFire code here (`services/machines/src/records.rs:262`, `package/host_dir.rs:14`).
- [D] 85 `static mut` in `services/*/src` and `dks/src`. [D] Sizes: shortcuts 2.3k lines, samples 2.3k, lfo 2.9k, fx 5.6k, project 8.3k, machines 12.4k. [D] Oversized files: fx `fxpage.rs` 1030, `fxsend.rs` 676; machines `build.rs` 1007, `records.rs` 1629, `slots.rs` 539; project `dks/src/glue.rs` 621.

## 3. Design decisions for the branch

### D1 Part version 1 carries today's callbacks, regrouped

Decision: every old callback maps to exactly one method of one part. No callback changes its meaning or its place in a walk. The part traits in `provider/` gain the methods the walks still make, each with a default.

| Part (tag) | Methods in version 1 | Old callbacks |
| --- | --- | --- |
| Lifecycle (1), `Provider<C>` | `start` (runs in the file's `init`), `attach`, `detach`, `can_stop`, `stop` (runs in the file's `fini`), `settle`, `reload_done` | `setup`, `apply`, `remove`, -, -, `settle`, `live_done` |
| Records (2) | `validate` (host only, not in the device table), `begin`, `commit`, `check`, `apply` per record; the table also holds the kinds list | `validate_records`, `module_begin`, `module_commit`, `check`, `apply_record`, `ServiceRow.kinds` |
| Dsp (3) | `check_sections`, `check_rows`, `check_types`, `admit`, `claim`, `release`, `clear`, `row_step`, `loaded`, `attribute`, `diag` | the eight `dsp_*`, `on_loaded`, `attribute`, `diag` |
| Tick (4) | `tick(phase)` | `tick` |
| Query (5) | `query(op, req, reply)` | the `query =` entry |
| Describe (6) | `cap`, `table_check`, `describe` | `cap`, `table_check`, - |

- `Service::declare` is dropped: the kinds come from the `service!` kinds argument, as `svc-rt` `init` does today (`svc-rt/src/lib.rs:261-269`).
- `teardown_refusal` is dropped: no service uses it. `can_stop` keeps its default.
- `Consumer` gains the module base and reads records from the register section, so `attach` gets what `ApplyCtx` gives today. `consumer.rs` is outside the ABI hash.

Reason: header table v4 is out of RC3, so record kinds stay global, records are still written by the services at the commit, and capacity is still `cap(kind)`. The plan's narrow signatures (section 6.1) need packed records and resources.

Rejected:
- Narrow to section 6.1 now: needs v4 and changes the record order across services (`seams/host.rs:28-31`).
- One part with all callbacks: every later change then bumps one part for all services.

Changes the decision: v4 lands first, or a service needs a narrower part before v4. Then that part gets version 2.

### D2 Lifecycle uses the module record's entry points

Decision: `start` runs inside the file's `init`, before the declarations, in the place of `setup`. `stop` runs in `fini`, which the macro now emits. The instance lives in one cell that the macro declares (`Owner<T>` in `svc-rt`). The file passes the row and the instance pointer with a new import `dk_provider_declare_row(ctx, row, instance)`. Part thunks are generic functions that take the instance pointer, as `ServiceRow::of` builds its thunks today.

Reason: no new loader step; the loader already runs `init` and `fini` in the right lifecycle states. A host test builds the same row over `FakeCore` or `RealCore`.

Rejected: a `start` pointer in the lifecycle table that the loader calls (a new loader step and a second place that owns the instance).

Changes the decision: a service that must start without `init` (none today).

### D3 Mid-branch, the row table accepts both row kinds

Decision: commit 1 makes the runtime's one row table (`svc_rows.rs`, the type `services()` returns) hold a row of either kind, in callback order. The old arm delegates to the existing single-row `ServiceTable` walk (`svc_rows.rs:137-140` already builds a one-row table). The last commit deletes the old arm with `Service`, `ServiceRow` and `ServiceTable`. Walk method names stay, so call sites do not change.

Reason: Em's order ends with "delete the old trait and macro", so both exist during the branch. The runtime's host tests link the service crates (`package/handlers.rs:92-101`); without this, `core/runtime:rust-tests` and `test-assembly` do not compile between commit 1 and the machines commit, and commit 1 cannot prove itself. The merged result has one path (plan section 12: no dual path in the core).

Rejected:
- Adapter that wraps `Service` as a Provider: decided against by Em.
- Runtime drives only Provider from commit 1: the branch is not testable until all six are ported.
- Two tables: breaks callback order between ported and unported services.

Changes the decision: Em wants no old arm in the runtime even inside the branch. Then all service commits are verified only at the end.

### D4 One `SVC_ABI` change; `PROVIDER_ABI` stays 1

Decision: RC3 changes `SVC_ABI` once from main's view. `core_imports.rs` and `service.rs` are hashed boundary files (`core_imports.rs:11`, `service.rs:22-29`). Part tables are versioned by `PartRef.version`, not by the files hash. `service.rs` stays as the file of the boundary types (`CheckCtx`, `ApplyCtx`, `Claim`, `RowWrite`, `ModuleCommitCtx`, `RecordSink`, `SettleCtx`, `Declare`, `Problem`, `SettingsOverlay`); the trait, the row, the thunks and the table leave it.

Reason: plan section 12, "one service ABI change per item, with all six services adapted in the same stack". No shipped file carries a `ProviderRow`, so its ABI number does not move.

Consequence: every `.dks` and `system.dksp` is rebuilt. An older `.dks` on a card is refused (`seams.rs:139-141`). Content modules (`.dkm`) are not affected. No runtime MINOR bump: the header table does not change.

Changes the decision: nothing in RC3.

### D5 The contract suite

Decision: `test-support/src/contract.rs` becomes generic over a small driving trait with two wrappers: one for a Provider owner object (new, drives the real part thunks) and one for `S: Service<FakeCore>` (old, deleted in the last commit). `contract::<S>` keeps its name for unported services; ported services call `provider_contract`. No second copy of the suite.

Reason: plan section 3 (reuse the suite of plan E); first-pass point 3 (tests drive the real entry points).

Rejected: a copied suite (two definitions); converting the suite in one step (unported services' tests do not compile).

### D6 The `service!` grammar

Decision: only the head changes: `service!(Shortcuts, "shortcuts", &KINDS_SHORTCUTS, parts = [Records, Tick], hooks = [...], fills = [...], exports = ...)`. The `hooks`, `fills`, `exports` lists keep their text form. `setup =` goes (it is `start`). `query =` goes (it is the Query part); the macro still declares the resident query export and points it at the Query thunk, so `hooks::provider_query` (`resident_query.rs:48`), the manifest and patchkit do not change. `tick =` (module-record tick, project) stays allowed.

Reason: `patchkit service-check` reads the call as text (`svccheck.rs:10-24`).

### D7 Service state

Decision: state is private fields of the owner object. Three kinds of table stay named statics in their concept file, written only through that concept's type: a table a device test or tool reads by symbol (`DKRT_*`), a table an assembler hook reads by symbol, a table an interrupt reads. Each one is listed in the service README with its reader. Every other `static mut` goes. Hooks reach state through `Owner::with`.

Reason: canonical layout row ("no loose static mut"); assembler and device tests stay unchanged, so the device tests remain a valid proof.

Rejected: moving every table into the owner with offset constants for the assembler (changes hook code and device-test symbols for no gain in RC3).

Changes the decision: a later item that gives tools a symbol map for service files.

### D8 Fixtures

Decision: `local-ping` is ported in commit 1 and gains only what today's header can express: attach and detach counters, a Tick, a Query that returns the counters. Its record kind, resource, hook and settings fields wait for v4 and the Settings item. `svc-test`, `svc-test-user`, `svc-echo` stay.

Reason: a fixture record kind needs an entry in `sdk/sdk.toml [records.kinds]` until v4. Deleting fixtures changes device tests that are the proof.

### D9 No device-test moves in RC3

Decision: the ledger moves of item RC3 (FX, services, settings) are a later item.

Reason: Em's rule (d): existing device tests are the proof. A moved test proves less about a contract change.

## 4. RC2s dependencies

1. Base: `main` after the RC2s stack merges (RC2s-0 to RC2s-3 and RC2t) and its one ABI bump. Do not start commit 1 before that.
2. After RC2s-2 the loader and the row table are in `system.dksp`. Measure the package size and the flashed image.
3. `DKRT_PROVIDERS` is read through the host directory (`HOST_PROVIDERS`); RC3 does not change it. `boot-restart` must read it through the helper that RC2s-2 step (e) adds.
4. New imports for service files go in `core_imports!` (the package's import table). Nothing is added to the resident table. If a brief finds it needs a resident entry, stop and ask.
5. The Settings part and the FX route panel depend on RC2s-3 (the menu in the package, `ui.settings_rows`). See question E1.
6. Paths: `package/` moved in RC2s-1 S5. Re-find files at the base.

## 5. Order, lanes, ownership

```
commit 1  contract path                         lane A (rc3)
commit 2  shortcuts (template; its review gates the rest)   lane A
parallel, branched from commit 2, cherry-picked in this order after review:
          samples | lfo | project | fx
          (machines pure-move commit may run beside them)
then      machines
then      delete the old path
then      one device gate, post-landing review, one PR
```

Lanes: `tools/devtools/lane.sh rc3 origin/main` for lane A. Reuse warm lanes for the parallel worktrees (fx in lane A; lfo, project, samples in three warm lanes). At most two cold cargo builds at once. Emulators only through Nx or `tools/devtools/emu-slot.mjs`.

| Unit | Owns | Must not edit |
| --- | --- | --- |
| contract path | `runtime/core/abi/src/provider/**`, `abi/src/{core_imports,service}.rs`, `runtime/core/svc-rt/**`, `runtime/core/test-support/**`, `runtime/core/src/rs/src/package/{svc_rows,svc_imports,handlers,registry}.rs`, `package/loader/{seams,commit}.rs`, `package/lifecycle/host.rs`, `packages/patchkit/src/{svccheck,record_rules,sdkgen}.rs`, `tests/fixtures/modules/local-ping/**`, one free `validate` in the `records.rs` of shortcuts, lfo, fx, machines | service logic |
| each service | `services/<name>/**`, its fixture call lists, its own lines in the runtime's test files (`package/handlers.rs:92-101`, `handlers/{fx_fixture,machines_fixture,lfo_tests,dks_decls}.rs`), its section of `docs/ARCHITECTURE.md` | patchkit, `svc-rt`, `abi`, `test-support`, `sdk/sdk.toml`, `ports/`, device tests |
| delete | the old halves in the files of commit 1 | services |

Shared runtime test files: each worktree edits only its own lines; the orchestrator resolves text conflicts at the cherry-pick. Committed objects (`*.dko.json`) are never merged by hand: regenerate.

## 6. Rules for every brief (STD)

- Read before Edit. Leave changes uncommitted and mark the plan row `review`. Report in at most 200 words: Result, Commits, Checks, Device runs, Open items.
- Run `DIGI_GATE_BASE=<base> mise run test:plan` first and say whether the selection matches what the change reaches.
- Static set: `pnpm nx:pilot run-many -t lint,fmt-check,rust-fmt,typecheck,python-checks`. Then `DIGI_GATE_BASE=<base> mise run test:changed`.
- Update `docs/ARCHITECTURE.md` for the step. No `pkill`. `dk:dev` for device iteration.
- If chains, not `match` on dense integers, in code that runs on the ColdFire.
- First-pass checklist, all eight points: (1) every new or moved test seen to fail, by TDD or by a reported temporary break; (2) no device literals, the baseline only goes down; (3) tests drive real entry points, fakes only for hardware; (4) `patchkit objdiff` and `patchkit size` (DT2, DN2, Syntakt): every differing function explained, sizes before and after; (5) after a move, search the tree for the old path; (6) one accept and one refuse case per rule plus the edge cases; (7) a change to build inputs runs that build (`uv run python -m tools.devtools.verify_build daemons`, patchkit, `patchkit:objects-check`); (8) globals in tests through one owner with a guard that restores on unwind.
- Design checklist: separation of concerns; device facts only from device data behind capabilities; DRY; owner objects with private state; files named after the noun; no utils or helpers files.
- Canonical service layout (plan row): `src/<name>.rs` owner with the `Provider` impl in one block; `src/records.rs` kinds and `Records`; a part in its own file only when substantial (`dsp.rs`, `queries.rs`); `src/lib.rs` docs, mod, pub use only; README; tests beside code; `dks/` with `module.toml`, `src/{lib,hooks,gen_exports}.rs`.
- A service unit with file moves is two commits: pure moves (objdiff identical), then the port.
- Test layers: logic in unit tests over `FakeCore`; one fit and one refusal per seam in `core/runtime:test-assembly`; no new device test.

## 7. Briefs

### RC3-1 Contract path (commit 1)

- **Goal:** a service file can export a `ProviderRow` and the runtime drives it. Decisions D1 to D6 and D8 are implemented. The six services do not change and still load.
- **Files:** see section 5. Also: split `svc-rt/src/lib.rs` (568 lines) by concept first as a pure move (`import_core.rs`, `module_def.rs`, `declare.rs`, `service.rs` for the macro, new `owner.rs`); regenerate `abi/src/gen_svc_abi.rs` with `patchkit gen`; regenerate the committed objects of the runtime, the package, the six services and the fixtures; `docs/ARCHITECTURE.md` service contract section, `modules/README.md` "Services", the fixture table in `docs/TESTING.md`.
- **Tests first:**
  - Unit, `dkrt-abi`: each part table's thunk calls the owner method with the same arguments, reached through `check` and `CheckedRow::part`; `Consumer` gives the base and the records of a kind in file order.
  - Unit, `svc-rt`: `Owner::with` gives nothing before `start` and after `stop`; a second `start` is refused; the macro emits exactly the adopted parts; a hook reaches the instance that `start` made.
  - Unit, runtime row table: old and new rows walk in callback order; the first refusal stops a walk; a missing part is skipped; a part version newer than the core knows is refused at `add`.
  - Contract suite: `provider_contract` over the ported `local-ping` on `FakeCore` (start, attach and detach counters per consumer, query, tick, stop order).
  - Assembly: fit: the packed `local-ping.dks` loads through the real loader, its two consumers attach and detach, the query answers. Refusal: a thin-wrapper variant of the fixture with a newer part version is refused with a clear status.
- **Checks:** `pnpm nx:pilot run core/runtime:rust-tests`; `core/runtime:test-assembly`; `dkrt-abi:rust-tests`, `dkrt-svc-rt:rust-tests`, `dkrt-test-support:rust-tests` and the six `dkrt-svc-<name>:rust-tests` (Cargo projects, `tools/nx/cargo.mjs:1-4`; confirm names with `pnpm nx:pilot show project <name>`); patchkit tests; `patchkit service-check` for the six services and local-ping; `patchkit:objects-check`; `mise run check:layers`; STD set. Boot-effects snapshots unchanged.
- **Quick device check:** `core/runtime-provider-tests:test-device--dn2` (loads `test/local-ping` and its user in the real image).
- **Break proofs:** ignore the part version in `add` (refusal case red); hand a fresh instance to each thunk (hook-reaches-state red); sort the table by load order (callback-order test red); skip `detach` in the fixture walk (counter case red).
- **Size:** 10 h. Risk medium: walk order with mixed rows; `svccheck` text rules; object regeneration for every service.

### RC3-2 shortcuts (template)

- **Goal:** the reference port that the others copy: canonical layout, D7 state rule, README.
- **Files:** `services/shortcuts/**`. `Shortcuts` (`src/shortcuts.rs:53`) becomes the owner with the `Provider` impl (`attach` none, `detach` = old `remove`) and `Ticker` beside it; `src/records.rs` keeps kinds, `Records` (`begin`, `commit`, `check`, `apply`) and the free `validate`; `STATE` (`records.rs:28`) and `state()` go; `ShortcutsService` goes; `src/slots.rs` deleted; `dks/src/image.rs` setup moves into `start`; `dks/src/hooks.rs` reaches state through the cell; `tests/{contract,state,records}.rs` lose the `LOCK` mutex.
- **Tests first:** unit: records become bindings in file order; the first owner of a key pair keeps it and the second is disabled with the log event; `detach` drops bindings, queued calls and the active binding without calling `off`; `check` refuses a full table and a callback outside the image; `tick` runs pending calls in order only in the early phase. Contract suite: `provider_contract` on every profile. Assembly: the existing shortcut-demo case.
- **Checks:** `dkrt-svc-shortcuts:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; `patchkit service-check`; `patchkit objdiff` of `shortcuts.dks` with every difference explained; `patchkit:objects-check`; STD set.
- **Quick device check:** `core/services/shortcuts-tests:test-device--dn2`.
- **Break proofs:** `detach` leaves bindings; drop the first-owner rule; run pending calls in every phase. Each turns a unit test red.
- **Size:** 3 h. Risk low to medium: the template is copied five times, so its review is adversarial and includes the README and the ARCHITECTURE text.

### RC3-3 samples

- **Goal:** owner `Samples` in `src/samples.rs` with `Provider`, `DspPart` (`check_sections`, `admit`, `claim`, `release`, `loaded`) and `Ticker`.
- **Files:** `services/samples/**` (`src/records.rs` 41 lines becomes the owner file; `src/slots.rs` deleted; `tracks.rs`, `pool.rs`, `peakwin.rs` stay concepts; `dks/src/{lib,glue}.rs`).
- **Tests first:** unit: track state per owner; `release` gives back every track of the consumer; `loaded` marks tracks ready; the tick works only in the early phase; `admit` accept and refuse. Contract suite. Assembly: existing cases.
- **Checks:** `dkrt-svc-samples:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; objdiff; `patchkit:objects-check`; STD set.
- **Device targets (gate):** `core/runtime-tests:test-device--samples-live` (DT2), `core/runtime-tests:test-device--samples-store-live` (DN2).
- **Break proofs:** skip the release of tracks; mark ready before `loaded`.
- **Size:** 3 h. Risk low to medium.

### RC3-4 lfo

- **Goal:** owner `Lfo` in `src/lfo.rs` with `Provider` (`settle`, `detach`) and `Records`. Hook names and port files do not change (question E2).
- **Files:** `services/lfo/**` (`src/records.rs` 594 lines split into owner and records; `graph.rs`, `route.rs`, `validate.rs` stay; `dks/src/hooks*.s` untouched). The shape table that the vector 191 tick reads (`SHAPES`) and `WAVE` stay named statics under D7.
- **Tests first:** unit: `settle` gives each key one number and saves only when the map changed; `check` refuses a key the module does not provide and a full table; `apply` writes the shape under the interrupt mask and does not overwrite another owner; `detach` retracts the picture first and syncs the range. Contract suite. Assembly: existing lfo-shape cases.
- **Checks:** `dkrt-svc-lfo:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; `patchkit service-check`; objdiff (hooks identical); `node tools/nx/device-literals.mjs` (no rise); STD set.
- **Device targets (gate):** `core/runtime-lfo-tests:test-device` (lfo, lfo-shape-output, lfo-shape-project, lfo-shape-panel).
- **Break proofs:** write a shape without the mask order (unit asserts the fake's mask calls); let `apply` overwrite another owner; skip `sync_range` in `detach`.
- **Size:** 4 h. Risk medium: tables read in an interrupt.

### RC3-5 project

- **Goal:** owner `Project` in `src/project.rs`; the state that is in `dks/src/glue.rs` statics moves into it; `start` does what `glue::setup` does (settings overlay); `stop` drops the overlay.
- **Files:** `services/project/**` (`ProjectSvc` goes; `engine/` and `dkp.rs` stay concepts; `glue.rs` 621 lines split by concept; tests in `tests/service.rs` split by behaviour).
- **Tick:** keep the module-record `tick =` unless the worker shows that a `Ticker` phase gives the same place in the UI pass; report which.
- **Tests first:** unit: existing engine tests stay; new: `start` sets the overlay once, `stop` removes it, the hook entries reach the one instance. Contract suite. Assembly: existing project-user case.
- **Checks:** `dkrt-svc-project:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; objdiff identical for the move commit; STD set.
- **Device targets (gate):** `core/services/project-tests:test-device`; `core/runtime-tests:test-device--reload-project` (DT2).
- **Break proofs:** leave the overlay set after `stop`; give the hooks a second instance.
- **Size:** 4 h. Risk medium: the largest moves outside machines.

### RC3-6 fx

- **Goal:** owner `Fx` in `src/fx.rs` with `Provider`, `Records`, `DspPart`, `Ticker`, `Query`, `Describe` (`cap`, `table_check`). The route panel, `dkrt_fx` and the Settings part do not change (question E1).
- **Files:** `services/fx/**` (move commit: `fxpage.rs` 1030 and `fxsend.rs` 676 split by concept, objdiff identical; port commit: `src/records.rs`, `build.rs`, `dsp/mod.rs`, `dks/src/{lib,hooks,rt}.rs`). `DKRT_FXP` and `DKRT_FXS` stay named statics (device tests read them). `dks/dsp/*.s` untouched.
- **Tests first:** unit: check steps (count over `MAX_FX`, effect rules, missing registry); `apply` claims a row and writes the page; `detach` gives back page, list entry, send cell and defaults; `cap` for effect and send slots; `query` accept, unknown op, short reply buffer; `row_step` order. Contract suite. Assembly: existing dyn-fx-gain cases.
- **Checks:** `dkrt-svc-fx:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; `patchkit service-check`; objdiff; STD set.
- **Device targets (gate):** `core/dynfx:test-device` (fx-contract); `core/fx:test-device--fx-routes-panel`, `--fx-routes-persist`, `--fx-send`, `--fx-send-dn2`; `core/fxpages:test-device--fx-pages`, `--fx-bipolar`, `--fx-kit-persist`, `--fx-track-route`; `core/runtime-tests:test-device--reload-fx`, `--fault-trace`.
- **Break proofs:** `detach` leaks a send cell; `query` answers OK for an unknown op; `cap` counts a freed row.
- **Size:** 6 h. Risk medium to high: most callbacks after machines, nine assembler hooks, most device tests.

### RC3-7 machines (last)

- **Goal:** owner `Machines` in `src/machines.rs` with all parts except Query. The RC4 machines point is not touched.
- **Files:** `services/machines/**`; `services/rows` crate renamed from `dkrt-svc-params` to `dkrt-svc-rows` (canonical layout row; also `runtime/core/project.json:4,16,58`). Move commit: `records.rs` 1629, `build.rs` 1007, `slots.rs` 539, `dsp/mod.rs` split by behaviour, objdiff identical. This commit may run beside the four parallel worktrees. Port commit: state into the owner; `PARK_TRIES`, `BUSY_PASSES`, `LOGGED`, `MISSING_ROWS` become fields.
- **Tests first:** unit: the seven check steps, each accept and refuse; `attach` and per-record `apply`; `settle` plans numbers and resets the parking state; `reload_done` frees placeholders and resets missing rows; `cap` for the five capacity kinds; `detach`; DSP claim, release, clear and row steps. Contract suite. Assembly: existing sine and dyn-dn2 cases; boot-effects snapshots unchanged.
- **Checks:** `dkrt-svc-machines:rust-tests`; `dkrt-svc-rows:rust-tests`; `core/runtime:rust-tests`; `core/runtime:test-assembly`; `pnpm nx:pilot run-many -t test-dsp-conformance`; `patchkit service-check`; objdiff; `patchkit size`; STD set.
- **Device targets (gate):** `core/runtime-machines-tests:test-device` (frametype-slot, machine-rows-dn2, machine-rows-dt2); `core/runtime-tests:test-device--reload`, `--live`, `--missing`, `--boot-restart`; the machine module device targets that `test:plan` lists (tb303, shot).
- **Break proofs:** keep the parking state across `settle`; skip a row release in `detach`; return the wrong pair for one capacity kind.
- **Size:** 9 h. Risk high: the largest service and DSP-coupled.

### RC3-8 Delete the old path

- **Goal:** one path. Delete `Service`, `ServiceRow`, the thunks, `ServiceTable`, `RowPtrs`, `accepts_records!`, `accepts_teardown!`, `Declare::ROW`, `dk_service_declare_row`, the old macro arms, the old arm of the row table, the `Service` wrapper of the contract suite, the old form in `svccheck.rs`, and the callback-order fallback (`svc_rows.rs:13-23`) with its test reworked to explicit orders. Rename `svc_rows.rs` to the noun it now owns. Regenerate `gen_services.rs`, `gen_svc_abi.rs` and all objects.
- **Tests first:** none new. The runtime tests that used a fake row for `teardown_refusal` become a `can_stop` case of a fixture provider, with an adversarial review of each change.
- **Checks:** all host targets of RC3-1; search the tree for the deleted names; `patchkit size` for DT2, DN2, Syntakt and the package against the base (plan: must shrink); `mise run check:layers`; STD set.
- **Break proofs:** not applicable (deletion); the search for old names is the check.
- **Size:** 2 h. Risk low.

### RC3-9 Gate and PR

- `DIGI_GATE_BASE=origin/main mise run test:changed`, then `mise run test:plan` and show the plan and its cost. One device gate on DT2 and DN2, halves in parallel: the device targets named in RC3-1 to RC3-7 plus `gate-device-smoke`, and `gate-smoke` and `gate-pins` when `affected` selects them. Post-landing reviewer on the combined diff. The PR text names the subset and what it skips. Push, PR and merge need Em's OK.
- **Size:** 3 h agent time; the last stack gate took 41 minutes.

**Total:** 44 agent-hours. Wall: about 2.5 days; about 2 days when the machines move commit runs beside the four worktrees and reviews need one round.

## 8. Questions of the draft

| Q | Decision | Reason |
| --- | --- | --- |
| Q1 adapter | Decided by Em: none. Detail in D3. | |
| Q2 size of the shared work | Closed. v4 is out; commit 1 is 10 h. | |
| Q3 callback order | Stays in the manifest. The six-name fallback is deleted in RC3-8. | The `SVC_ABI` change refuses every older `.dks`, so no file without a packed order can load. |
| Q4 `Needs::fields` | Deferred with header table v4. | RC3 does not touch the table. |
| Q5 `BOOT_TABLE_MAX` | Deferred with v4. | Same. |
| Q6 `can_stop` for shortcuts | Default `Ok` for all six. The per-consumer refusal walk is deleted. | All six use `accepts_teardown!`. A service that must refuse one consumer later adds `can_detach` as lifecycle version 2. |
| Q7 state cell | `Owner<T>` in `svc-rt`; D2 and D7. | |
| Q8 samples DSP pool | Deferred with v4. | Resources are not packed in RC3; samples keeps its `admit` check. |
| Q9 dn2-1.11 port | No port file changes in RC3. With the hook-name item: rename in all three port files, device proof on 1.12 and 1.17, 1.11 by patchkit host tests. | AGENTS.md: 1.11 is a patch port only. |
| Q10 project Settings | Not in RC3; project keeps `SettingsOverlay` (`service.rs:675-695`). | Follows E1. |
| Q11 `dkrt_fx` bridge | Question E1. | |
| Q12 route UI and RC2s-3 | Closed: the base is after the RC2s merge. Scope is E1. | |
| Q13 machines and RC4 | Decided by Em: separate. | |

## 9. For Em (2)

**E1 Settings part, FX route panel and the two bridges.** The plan's RC3 row includes the `Settings` part, the FX route row moved to it, the menu's route panel deleted, and `Describe` and Query in place of the `dkrt_fx` and `dk_machines_module_info` bridges (`package/handlers.rs:49-50, 74-89`). The lean decision does not name them.
- Recommendation: their own item after RC3, together with RC3m, which needs the same renderer.
- If in RC3: about 10 to 14 h more; a UI change that needs an agreed design first; `fx-routes-panel` is rewritten and is no longer an unchanged proof; menu edits after RC2s-3; `abi.toml` and `dkrt.h` change.
- If out: RC3 stays behaviour-neutral with unchanged snapshots and device tests; the runtime names `fx` and `machines` in two export lookups for one more item.

**E2 lfo hook names.** The plan says RC3 returns `cf.lfo.hook.*_dt2` and `*_dn2` to plain names and lowers the device-literals baseline. The device mask is already in the HOOKS item (`rc3-header-table-v4.md:19`), so this needs no v4, but it edits patchkit (duplicate-name checks, `svccheck`), the loader (`role.rs`, `admission.rs`) and the three port files.
- Recommendation: its own small item right after RC3 (about 4 h, proof: the four lfo device tests, `gate-smoke`, `gate-pins`).
- If in RC3: the lfo worktree is no longer disjoint and lands serially; port files change in the branch.
- If out: the suffixes and the raised baseline stay for one more item.

## 10. Later items (not in RC3)

| Item | Payoff |
| --- | --- |
| Header table v4 (resources and records packed, DKRG 2, decoders, runtime MINOR) | A new service ships its record kinds and capacity as card files with no `sdk.toml` or runtime edit; `[records.kinds]`, `[records.resources]`, the domain capacity ids and the v1-v3 parser branches are deleted; the Records part narrows to routed records. |
| Typed-address cleanup (about 64 raw reads in 17 service files, [D]) | Service host tests run on the address map; the boot-effects snapshot can hold a real machines commit and run in-process. |
| Settings part, FX route rows, bridges to Query and Describe (if E1 is "out") | No service name in the runtime; the route panel leaves the menu; RC3m is unblocked. |
| Hook names per device set (if E2 is "out") | Plain lfo hook names; device-literals baseline back down. |
| Ledger RC3 device-test moves; `svc-test` and `svc-echo` replaced by `local-ping` | Fewer emulator minutes per gate; one fixture per contract feature. |

## 11. Conflicts and risks across items

- `packages/patchkit` changes only in RC3-1 and RC3-8. A service unit that finds it must edit patchkit stops and reports.
- The runtime's test files that name services are shared; expect small text conflicts at the cherry-picks.
- Every unit regenerates its own committed object; RC3-1 and RC3-8 regenerate all. Run `patchkit:objects-check` after each cherry-pick.
- RC3 changes the image hashes and every `.dks`, so no device result replays from cache at the gate.
- Size: the old arm of the row table costs text only inside the branch. Report `patchkit size` at RC3-1 and RC3-8.
- If the RC2s merge slips, only reading and the fixture design can start early.
