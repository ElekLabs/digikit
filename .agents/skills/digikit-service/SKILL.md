---
name: digikit-service
description: >-
  Build or change a digikit ColdFire service (a .dks loaded from the card): the layer choice (platform table, service hook, runtime core), the service! macro, the service module.toml ([[hook]], [[fill]], [[service.resource]], [[service.record]]), exports, versions, the lifecycle (start_in, attach, detach, can_stop, State carry, reload and its refusals), and the test layers (unit over a fake, assembly, device). Use when a module needs a resource or API that no loaded service provides, when a firmware site must be hooked from the card, or when you copy services/shortcuts or services/lfo. Content modules (machine, fx, tool, lfo-shape) use digikit-module; firmware analysis uses firmware-query.
---

# digikit services

Draft (2026-11-28): written from the sources named below. Where a fact is open, the text says so. Run every command from the repository root.

A service is a ColdFire Rust file (`.dks`) that owns domain state, policy and exports for modules. The runtime loads it from the card, so it can be replaced while the device runs.

## When to use this skill

The handoff from `digikit-module`: ask "does a loaded service provide the resource or API my module needs?" Look in `sdk/sdk.toml` (imports with `provided = true`) and `services/*`. If one does, stay in `digikit-module` and declare `[uses]` / `[[service_import]]`. If none does, you need a new service or a change to one: use this skill.

Reasons to build one: a shared table or pool that several modules fill, a policy (who wins, ordering, conflicts), a firmware site that must be hooked, a record kind with its own rule.

## Pick the layer

| Layer | Holds | Changes by |
| --- | --- | --- |
| Firmware facts | `ports/fw/*.toml` declarations and `docs/findings/` | analysis ([firmware-query](../firmware-query/SKILL.md)) |
| Platform | `runtime/platform/*`: sites patched once at boot, resident stubs, tables published through `[[info]]` | a flashed change; Em decides |
| Runtime core | `runtime/core/*`: lifecycle, loader, resource ledger. It names no service | should not change for a new feature |
| Service | `services/*` (`.dks`): domain state, policy, exports, resources, records, settings. Reloadable | card upload |
| Module | `modules/*` (`.dkm`): content that declares `[uses]` | card upload |

The rule for a firmware site: record its execution context first (task or interrupt). Then:

- **Task-context site with one consumer:** the service hooks it (`[[hook]]`, installed through the `Installer`, reloaded with a guard). A pure service change.
- **Interrupt-level site, a shared site, or a site whose owner must reload live:** a platform table (a flashed change, so a core change, not a service change).
- **A second consumer appears for a hook:** promote the hook to a table then. Do not build tables ahead of need.

Policy (who wins, ordering, conflicts) lives in the service. The platform only gives the site and the slots. The runtime and platform never name a service; device facts come from `devices/*.toml` and `ports/fw`, and code branches on `[capabilities]`, never on a device name.

## Find the firmware place

1. Use the firmware-query skill: `fw.py atlas TERM`, then `dossier ADDR --other IMAGE`. Check both devices (DT2 1.17, DN2 1.12) and confirm callers with `tools/coldfire/refscan.py`.
2. Write the result in `docs/findings/NN-topic.md` with the marks [V], [D], [O], and index it in `docs/FINDINGS.md`. A [V] needs a second agent's check against image bytes.
3. Declare the site in `ports/fw/<subsystem>.toml` (`uv run python -m tools.firmware.catalog declare IMAGE ADDR NAME --other IMAGE2 --write`). Add `users = ["core/services/NAME"]`; it is provenance only. Rules and fields: `docs/design/firmware-decls.md`.

## The provider contract

Copy `services/shortcuts` (its README says it is the template) or, for a service that reaches device-only calls through a `Host` trait, `services/project`. The layout:

- `services/NAME/src/` is a host-testable crate. One owner object holds private state. Its parts sit beside it, one file per concept.
- `services/NAME/dks/` is the service file: `lib.rs` (the `service!` call), hooks, builder exports, generated `gen_exports.rs` (never edit), and `module.toml`.

**The `service!` macro** (`runtime/core/svc-rt/src/service.rs`, doc comment on top) emits the module record, the ABI word, the owner cell `DK_SVC_OWNER`, the part tables and `dk_svc_declare`. Its form:

    dkrt_svc_rt::service!(Shortcuts, "shortcuts", &KINDS_SHORTCUTS,
        parts = [Records, Tick], hooks = [...], fills = [...],
        patches = [...], exports = gen_exports::exports(), tick = f);

Each optional list comes in that order. Parts are traits the owner implements: `Records`, `Dsp`, `Tick`, `Query`, `Describe`, `State`, `Payloads`, and `Reentry` (an event queue for a callback that arrives while the owner is busy). The lifecycle part (`Provider`) is always there. `Provider` is in `runtime/core/abi/src/provider/contract.rs`: `declare`, `start_in`, `attach`, `detach`, `can_stop`, `stop`, `settle`, `reload_done`.

Rules from `services/shortcuts/README.md`:

- State is private. A hook reaches the owner with `DK_SVC_OWNER.with(|s| ...)`; before `start` or after `stop` it gets `None` and answers "no service".
- A failed `start_in` leaves nothing to drop.
- A nested call into the owner is refused (busy state). Use `Reentry` for callbacks.

**The service `module.toml`** (read `services/shortcuts/dks/module.toml` and `services/lfo/dks/module.toml`; checked by `patchkit service-check`):

- `[module]` has `role = "service"`, `service = "NAME"`, `callback_order` and `requires`.
- `[[hook]] name, entry, devices` names a site of `ports/fw`. Each `[[hook]]` is also an entry of `hooks = [...]` in `service!` with the same device filter. A `[[fill]]` fills a pointer slot of an adapter. `[[patch]]` entries pair with `patches = [...]`.
- `[[service.resource]]` (`name`, `limit`, `aggregate`, `scale`, `draws_from`) declares a capacity the service owns. `[[service.record]]` (`name`, `kind`, `len`) declares a record kind local to it. Parsing is in `packages/patchkit/src/service_resource.rs` and `service_record.rs`. **Open:** those sources say the packer does not write them yet (RC3); check the current state before you rely on them. `tests/fixtures/modules/local-ping/src/lib.rs` says the same of its record, resource and settings fields.
- `[[object]]` names the built `.dko.json`, placed in `cf.expansion`. `[object.externs]` aliases hook exits into imports.

**Exports and lookups.** The functions a service exports are `provided = true` imports in `sdk/sdk.toml`; `patchkit gen` writes `gen_exports.rs`. A service with its own API outside the SDK lists `[[export]]` names (fixture `test/local-ping`: `local_ping_value`, `dk_service_query_v1`). A consumer declares `[[service_import]] service, min_version, names` (fixture `local-ping-user`).

**Versions.** Packed `major << 16 | minor << 8 | patch`. `version_satisfies(provided, minimum)` in `runtime/core/abi/src/version.rs` is the one rule: same major, not older. Fixtures: `local-ping` (1.2.0), `-user` (needs 1.2.0), `-user-b`, `-old-user` (a need the provider no longer meets, refused).

**Claims and resources.** The core's `ResourceLedger` (`runtime/core/fit/src/ledger.rs`) counts pools `(owner, name)` with a limit and sums consumers' claims. **Open:** its header says nothing in the runtime uses it yet; confirm before you depend on it. Today the services own the claim policy in the `Dsp` part (`DspPart` in `runtime/core/abi/src/provider/parts.rs`: `admit`, `claim`, `release`, `clear`); read `services/machines/src/dsp/claim.rs` and `services/fx/src/dsp/`. The core never decides who wins.

## Lifecycle

- **start:** `start_in` writes the state into the cell, in the file's `init`, before the declarations. Subscribe to tables and install hooks here. A refusal fails the start, so a module that needs the service is refused instead of binding things nobody delivers (shortcuts does this when the image has no key table).
- **attach / detach:** `attach` runs for each consumer that comes up; `detach` frees what that consumer owns and must be idempotent (it also runs for a consumer attached only in part).
- **can_stop:** answer `Err` while a take-down is unsafe (for example the transport plays). It names no consumer.
- **take down / bring up:** consumers go down first, services up first (`runtime/core/lifecycle/src/manager/`: `take_down.rs`, `bring_up.rs`). A hook or `[[patch]]` has its stock bytes restored before `stop`.
- **reload:** `PackageManager::reload` (`manager/reload.rs`) takes the service and its live consumers down and up again. Reloading a service reloads its consumers.
- **reload refusals:** the generic ones are in `runtime/core/src/rs/src/package/reload/refusal.rs`, in order: safe mode, transport not stopped, open upload, card busy, a menu built for the module. Then each service's `can_stop`. Graph refusals (unmet, incompatible, cycle) are in `runtime/core/lifecycle/src/refusal.rs`.
- **carried state:** the `State` part (`export_max`, `export`, `import`, `VERSION`, `ACCEPTS_FROM`) keeps state across a reload of the service. Examples: `services/project/src/project.rs`, fixture `local-ping`. `import(None)` must work: it starts clean.
- **entry context:** a UI-task entry may touch UI-task tables. An interrupt-level entry may not take the owner's busy state or call task services. Subscribe and unsubscribe run in the UI task only (`key_subscribers.h`).

## Tests, by layer

Design bar: most proof is unit tests over fakes, some assembly tests, few device tests. When a test is hard to write low, change the code's shape (inject a fake, split the pure part).

1. **Unit:** a trait seam with a fake, tests beside the code (`services/NAME/tests/`: `state.rs`, `records.rs`, `contract.rs` for the shared provider suite, `file.rs` for the file's own code). `pnpm nx:pilot run dkrt-svc-NAME:rust-tests`.
2. **Assembly:** one case that fits and one typical refusal on the canonical fixtures: `pnpm nx:pilot run core/runtime:test-assembly`. Fixtures in `tests/fixtures/modules` (DRY: reuse `local-ping*`, add a variant only for a distinct identity).
3. **Device:** one firmware fact per test, waits on events or state (`rt.listen()`, `events.until`), never fixed time; the test record names the fact. On card rigs run inside `h.usb`. Example: `services/shortcuts/tests/device/shortcuts.device.test.ts`. Run `pnpm nx:pilot run PROJECT:test-device--FILE--dn2` first, then `--dt2`. One device workload at a time.

Iteration and generated files:

    pnpm nx:pilot run core/runtime:rust-tests
    pnpm nx:pilot run core/runtime:build-objects
    dist/patchkit/patchkit --root . build --tree runtime
    dist/patchkit/patchkit --root . build --tree services
    pnpm nx:pilot run patchkit:objects-check
    mise run dk:dev -- ID --device dn2 --keys 'tap:...,shot:NAME'
    mise run module:check -- AUTHOR/NAME
    DIGI_GATE_BASE=origin/main mise run test:changed

On a merge conflict in `*.dko.json`, take main's version and regenerate. `dk:dev` is not a proof; finish with the owner's checks.

## Worked examples

**Shortcuts on `core/input`'s key table** (PR #789). The key walker is shared and runs in the UI task before any view, so the platform owns it:

- Platform: `runtime/platform/input/src/cf/key_subscribers.c` and `.h`, `key-walk.s`, and the image table `input_keys_info` (`'KEYS'`, version, slots, subscribe, unsubscribe). The first subscriber that returns nonzero consumes the event; else the stock walker runs. Four slots.
- Service: `services/shortcuts` reads the table (`key_table.rs`), subscribes on `start`, unsubscribes on `detach`/`stop`. No `[[hook]]` in its `module.toml`. The policy (two-key combinations, bindings) stays in the service.
- Fake: `UnwiredKeys` (`services/shortcuts/src/unwired_keys.rs`) counts calls; `UnwiredKeys<REFUSAL>` gives the refusal of a full or missing table, so the start-refusal path is a unit test.
- Gate command: `workspace:key-abi` (`node tools/nx/key-abi.mjs`) checks the ABI words against their source.

**LFO on `core/lfo-table`.** The tick reads the wave tables in the engine task, so the platform owns them: `runtime/platform/lfo-table` (README: what the module changes in the image). `services/lfo` binds the image's table in `dks/src/image_table.rs` and fills it (`wave_table.rs`, 32 samples per wave, UI task). It owns the `lfo_shape` record kind and the rule for it. Its UI hooks (task context, one consumer) are `[[hook]]` entries in `services/lfo/dks/module.toml`.

The contrast is the rule above: lfo's UI sites are hooks; the tick and walker sites are tables.

## Checklist before a PR

- [ ] Layer choice stated: each firmware site has its context recorded, and the hook or table decision is explained.
- [ ] Finding written and indexed; `ports/fw` declaration with `users`.
- [ ] No device ids in hand-written code: `node tools/nx/device-literals.mjs` (use `--write` only after you removed literals).
- [ ] Generated objects rebuilt (`build-objects`, `build --tree runtime`, `--tree services`), `patchkit:objects-check` passes. `patchkit gen --check` is clean.
- [ ] Size before and after: `dist/patchkit/patchkit --root . size --device dt2,dn2` (see `patchkit --help`; `--baseline FILE` compares).
- [ ] `core/runtime:test-assembly` passes.
- [ ] Merge gate run once: `DIGI_GATE_BASE=origin/main mise run test`, plus lint, fmt-check, typecheck.
- [ ] PR text lists tests per layer (unit / assembly / device) and quotes the gate's selected and skipped lines.
- [ ] No firmware bytes, no `out/` content committed.

## References

`services/shortcuts/README.md` (the template and its rules), `docs/plans/runtime-contract-and-lifecycle.md` (the contract design), `docs/design/firmware-decls.md`, `docs/TESTING.md`, `docs/MODULES.md`.
