# Architecture

This file is the reference for the layers (L0 to L4), what each layer may depend on, the interfaces between the runtime, services and modules, and how a new device or firmware version fits. `tools/devtools/layercheck.py` enforces the rules it can check today (`mise run check:layers`).

## Layers

Five layers, L0 to L4. Calls go down. The diagram is the target; "today and target" below says which boxes exist now.

```
 L4  Content modules (.dkm, loaded from the card)          apps
     machines: pluck, shot, analog, bd …                   digikit-menu (runtime UI)
     fx: freqshift …   tools: bootscreen, debug
        │ A: sdk.toml imports by service group,                │ dkrt_* runtime API
        │    dk_<service>_open handles                         │
 ───────┼──────────────────────────────────────────────────────┼──────────
 L3  Services (in the runtime crate now; .dks at R12)          │
     src/services/machines   machine/dp/page/voice rows, keys, │
                             native pages, page key/turn slot  │
     src/services/fx         FX pages, sends, FX param/send/   │
                             effect rows, FX DSP parts         │
     src/services/samples    tracks, peak windows, sample-     │
                             pool exports, track-table DSP     │
     each: mod.rs · records.rs · slots.rs · dsp/ · tests/      │
        │ C: sdk.toml imports        ▲ B: only declared:       │
        │    + service-only set      │    lifecycle, record-kind handlers,
        ▼                            │    slots/hooks, resources, sinks
 ───────────────────────────────────────────────────────────────┴──────────
 L2  Runtime core (kind-free: never names a machine, effect or service)
     state      State, statics, imports, C API (api.rs), mailbox
     boot       selection, safe mode, boot plan (dependencies, fit, refusals)
     loading    loader, collect (relocate, commit records),
                registry walker + record-kind handler table, params (param-table writer)
     dsp core   DSP area allocator, dsp-link + windows, row table,
                DSP load state machine   (→ R6 transfer queue)
     storage    store, files, card, scan, arena, worker, settings (broker)
     transport  SysEx/USB: transport, store/admin/module commands
     diagnostics dlog, sinks log/span (R2d), fault (crash), watch, invariants,
                health, report (status, capacity, diag)
     tick       scheduler: service ticks, poll table
        │ platform calls only
 ───────┼─────────────────────────────────────────────────────────────────
 L1  Platform and firmware adapters (in the patched image)
     platform glue     st, flush, block timing, MIDI send, raw memory
                       (platform.rs in R2b → platform adapter at R3)
     patched adapters  ColdFire hooks and *_info tables: dynreg, dyndsp,
                       dynfx, fxpages, ui, input, health, dsp-link …
                       SHARC hook stubs with pointer slots (finding 128)
     device profiles / anchors  per-device and per-firmware addresses
 ───────┼─────────────────────────────────────────────────────────────────
 L0  Stock firmware and hardware: ColdFire OS, SHARC DSP image, card, USB
```

Rules between layers:

- Calls go down only.
- The one upward path is L2 to L3, through what services declared (interface B). The runtime resolves it at boot planning from a static table that `module:check` writes. It is derived and declarative, the same as S3 for modules: the service's `init` declares through builder calls, and the table is written from them.
- L3 never calls L3 directly. If a service needs another, it imports the other's exports through `sdk.toml` (interface A) and declares the dependency. The boot plan orders them and refuses the first if the second is missing.
- L4 modules reach services only through `sdk.toml` imports. The digikit menu is an app on the runtime's C API.
- Everything that depends on the device or the firmware version sits in L1: adapters, device profiles and anchors. It is not in the SDK, in L2, L3 or in modules.

Three interfaces (details in [runtime and services](plans/runtime-and-services.md) section 5.12):

- **A, module to service:** `sdk.toml` imports by service group and `dk_<service>_open` handles.
- **B, runtime to service:** only what a service declared: lifecycle, record-kind handlers, slots and hooks, resources, and the log and span sinks.
- **C, service to runtime:** the same `sdk.toml` imports as modules, plus a small service-only set.

Service-to-service crossings in today's code, and where they belong:

| Today's crossing | Where it belongs |
| --- | --- |
| `tracks` checks `fxpage` and `fxsend` busy | The runtime link (R6 transfer queue) |
| `fx` needs `machines` rows | The kind-free core registry |
| `pageclick` reads registry rows | A core registry accessor |
| `fxsend` uses `settings` | A core broker resource (allowed) |

### Today and target

Today the runtime is one crate in which L2 and L3 code is mixed, and the services have no folder of their own. The adapters, the app and the installer are separate patched-in modules. The target is reached in steps:

- **R2b** moves files into L2 and L3 folders inside the runtime crate, with shims at the old paths.
- **R2c** turns the crossings above into interfaces. `check:layers` and the `dkrt-abi` crate split then enforce the arrows.
- **R3** moves the platform glue into the L1 platform adapter.
- **R7 and R9 to R12** make the L3 services real modules (`.dks`). The movable ColdFire hooks go from L1 into their services, and the runtime shrinks to L2.

`core/expansion` is the reset-time installer: it runs at reset and copies all patched-in code to its place. `modules/core/fw` holds the generated firmware bindings (a library, not a module).

## Boundaries and tests

This section is the target. The interfaces and fakes below are not built yet: B and C (with `FakeService` and `FakeCore`) arrive with R2c, and the Platform interface (with `FakePlatform`) with R3. After R3, test-speed item 7 moves logic tests from the device to the host. The rule for review of deleted tests applies to each move: a host test replaces a device test only when it covers the same path.

Each layer has entry points (what the layer above calls) and exit points (what the layer calls), and is tested at those points.

| Layer | Entry points | Exit points | Tested how |
| --- | --- | --- | --- |
| L1 platform | The Platform interface: time and blocks, flush, MIDI send, raw memory, the `*_info` tables | Stock firmware | A contract suite, run once per device and firmware in the emulator. Everything above uses a `FakePlatform` |
| L2 core | The C API `dkrt_*`, the boot and tick entries, the static service tables | Platform, the interface B handler tables, the DSP link | Host unit tests with `FakePlatform` and `FakeService` (it records handler calls): loader, boot plan, fit, refusals, registry walker, later the R6 queue |
| L3 services | The interface B handlers | Interface C, a `CoreApi`: params, rows, link, settings, log | Host unit tests of each service against a `FakeCore` that records row writes and link submissions |
| L4 modules | The `dk_module` lifecycle and the kind contract | `sdk.toml` imports | Conformance tests, and module host tests against fake services |

Rules:

- Parts inside a layer depend on small interfaces, not on each other's internals, so each part has its own unit tests.
- Each interface has one contract test suite. It runs against the fake and against the real implementation, so the fake cannot drift.
- Target pyramid: many host tests per layer, one contract suite per boundary (the only emulator part, once per device), and a small set of end-to-end device scenarios.

## Roles

| Layer | Folder | `role` | Runs | May depend on |
| --- | --- | --- | --- | --- |
| L2 Runtime core | `modules/core/runtime` | `runtime` | At boot and in the UI and block loops | Firmware bindings, SDK crate `dkrt-sdk`, adapters through optional symbols |
| L3 Service | `src/services/NAME` in the runtime crate (later `modules/core/services/NAME`) | `service` (planned) | Loaded when a selected module needs it | SDK imports (interface C), other services through imports (interface A) |
| L1 Installer | `modules/core/expansion` | `installer` | At reset, once | Firmware bindings |
| L1 Firmware bindings | `modules/core/fw` | none (not a module) | Never; generated by `patchkit gen` from `devices/fw/` | Anchors |
| L1 Adapter | `modules/core/adapters/NAME` | `adapter` | Inside firmware hooks, in the firmware's contexts | Firmware bindings, runtime exports, other adapters' headers |
| L4 App | `modules/core/apps/NAME` | `app` | In the task context, called by an adapter | SDK, `dkrt.h`, `dk_ui_core.h` |
| L4 Content module | `modules/machines`, `fx`, `tools`, `tests/fixtures/modules` | none | Loaded at run time from the card (a `.dkm`) | SDK only |
| SDK contract | `sdk/` | none | Not code that runs; the contract between L4 and L2 and L3 | Nothing |

## Rules

`layercheck` reads manifests, sources and built objects. It builds nothing; for the objects of content modules it asks patchkit where the objects of this tree's sources are stored (`patchkit build --paths`, about 10 s), so a cache entry another worktree built is never read.

1. **Roles.** Every module under `modules/core` has `role` in `[module]`, and the role fits the folder: `adapters/` is `adapter`, `apps/` is `app`, `runtime/` is `runtime`, `expansion/` is `installer`. Roles map to layers as in the table above.
2. **Sources.** Content modules and apps include only their own files and the SDK (`<dk/...>`, `sdk/dsp/contract`). An app may also include `dkrt.h` and `dk_ui_core.h`. Only runtime, installer and adapters include `dk_fw_*` headers or depend on the `dkfw` crate. C, ColdFire and SHARC assembly (`#include`, `.include`) and Rust (`dkfw`, path dependencies) are checked.
3. **Objects.** The undefined symbols of the built objects of apps and content modules are SDK imports (`sdk/sdk.toml` `[[import]]`), `dkrt_*`, `dk_ui_*`, `dk_app_*` or the SHARC externs the SDK documents. They never name a firmware anchor (`cf.*`, `dsp.*`, `hook.*`, `pk.*`), and `[object.externs]` never maps a symbol to one.
4. **Apps do not patch.** An app has no `[[op]]` and no `[[claim]]`. If an app needs a firmware hook, the adapter that owns that plug-in point installs the hook and calls the app through an optional symbol (`dk_app_*`).
5. **Capabilities cost nothing unused.** A module asks for capabilities; adapters implement them per device. Prefer data (a field an adapter already passes) to a new firmware hook. A hook must cost at most one flag check on a hot path when no loaded module asks for its capability ([capabilities plan](plans/capabilities.md)).

Today `check:layers` enforces these five rules: roles, which sources and objects each role may use (so L4 never reaches firmware names and L1 is the only layer with `dk_fw_*`), no patching by apps, and the capability cost. It does not yet check the arrows inside the runtime crate between L2 and L3. R2c adds that: the rest of the runtime reaches a service only through the interfaces, and the `dkrt-abi` split turns the cycles into build errors.

Exceptions are listed in `layercheck.py` with a reason: `test/registry-demo` patches the firmware itself (it is patchkit's example of a hook), and `test/ui-demo` stands in for an app.

Run it:

```sh
mise run check:layers          # or: uv run python -m tools.devtools.layercheck
```

Each violation prints `file:line: rule: what is wrong. Instead: what to do.` The gate runs it in `python-checks` (with ruff and mypy) whenever modules, the SDK or the firmware declarations change; `mise run test:changed` runs it too.

## How a new device or firmware version fits

A new firmware version of a known device:

1. Add the anchors to `devices/ports/` (they resolve by hash) and check the declarations in `devices/fw/`.
2. Run `patchkit gen`. The bindings in `modules/core/fw` follow.
3. Fix what changed in the adapters. The adapters hide the difference; no SDK header, no content module and no app changes.

A new device:

1. Add the device profile (`devices/NAME.toml`): limits, patch sets, panel, storage and the values the adapters read (`docs/design/device-data.md`).
2. Add its ports and, where its firmware differs, device-specific adapter sources (`src/cf/NAME/`, `src/dsp/NAME.s`) behind the same adapter API.
3. Keep the SDK contract. A content module that builds against the SDK loads on the new device when the runtime reports the contract version.

## Where the device specifics live

| What | Where |
| --- | --- |
| Firmware addresses and hook sites | `devices/ports/` (anchors by hash) |
| Firmware function and structure declarations | `devices/fw/`, generated into `modules/core/fw` |
| Limits, patch sets, panel, storage | `devices/*.toml` (device profiles) |
| Code that hooks or wraps firmware | `modules/core/adapters/*` |
| The module contract | `sdk/` (device-neutral) |

## Worked example: the UI path

A menu or screen of an app or module goes through these steps:

1. **Module.** A module calls `dk_ui_menu_new`, `dk_ui_text`, `dk_ui_open` and the other calls of `<dk/platform/ui.h>` (`sdk/include/dk/platform/ui.h`). It knows no firmware.
2. **Runtime.** The runtime links the module's imports when it loads the `.dkm`. The `dk_ui_*` calls are `[[import]]` entries of `sdk/sdk.toml` with `requires = "core/ui"`: the runtime's object names them as `optional` symbols, and patchkit resolves them to the functions of `core/ui`, or to 0 when `core/ui` is not in the image. For 0 the runtime hands out a stub that returns the API's failure value (the entry's `absent`: null, -1 or 0, or nothing for a void call), so a module that uses the UI still loads. An app (a core module) links the same symbols directly through patchkit.
3. **Adapter `core/ui`.** `modules/core/adapters/ui` implements the calls. It builds the firmware's own views (`VerticalMenuView`, list and text views) through the generated bindings (`dk_fw_view.h`, `dk_fw_menu.h`) and keeps their state.
4. **Firmware views.** The firmware draws and handles the keys.

The SETTINGS entry shows the rule for hooks. `core/ui` owns the hook at the end of the SETTINGS list rebuild (`cf.menu.hook.settings_list.tail`). It calls the optional symbol `dk_app_settings_rows(settings_menu)` (declared in `dk_ui_core.h`). The app `core/digikit-menu` implements it with `dk_ui_stock_row` and adds the DIGIKIT row. The app has no hook, no claim and no firmware include. When no app is in the image, patchkit resolves the symbol to 0 and the hook does nothing.

A module with settings sets the `menu` field of its `dk_module` (`void menu(dk_module_ctx *ctx, dk_ui_menu *menu)`; the `.dkm` header carries its address and `DkrtModule.menu` and `DkrtModule.ctx` give it to the app). The MODULES list of `core/digikit-menu` shows SETTINGS... in the side panel of a loaded module that has one, builds a menu once for it (titled with the module's name) and calls `menu` to fill it only when the user opens it. The check mark of the row stays the only on/off of a module; the runtime owns it, and the module owns only its own settings.

**Lifetime rule.** No module is unloaded at run time, and menus are never freed. So every `ctx`, callback and descriptor that a module hands to `<dk/platform/ui.h>` must point at the module's own static data or code, never at the stack, and menus are built when the user opens them, after all modules are loaded. Then no UI pointer outlives the module that owns it.

The same holds for card reads: the INFO page of the menu gets the module header from `dkrt_module_header` of the runtime, which does the card read in its own buffer.

## See also

- [Repository layout](LAYOUT.md), [testing](TESTING.md)
- [Modules: what they are and why](MODULES.md), [module reference](../modules/README.md), [SDK](../sdk/README.md)
- [Firmware declarations](design/firmware-decls.md), [device data](design/device-data.md), [runtime](design/digikit-runtime.md), [boot screen format](design/dkbs.md)
