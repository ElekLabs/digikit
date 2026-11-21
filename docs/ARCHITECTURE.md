# Architecture

This file is the reference for the layers (L0 to L4), what each layer may depend on, the interfaces between the runtime, services and modules, and how a new device or firmware version fits. `tools/devtools/layercheck.py` enforces the rules it can check today (`mise run check:layers`).

## Layers

Five layers, L0 to L4, and the port file beside them. The [taxonomy](design/taxonomy.md) defines the terms and card layout. The [repository layout](LAYOUT.md) shows where the layers live. Calls go down. The diagram shows the target architecture; "today and target" below gives its implementation history.

```
 L4  Content modules (.dkm, loaded from the card)          apps
     machines: pluck, shot, analog, bd …                   digikit-menu (runtime UI)
     fx: freqshift …   tools: bootscreen, debug
        │ A: sdk.toml imports by service group,                │ dkrt_* runtime API
        │    dk_<service>_open handles                         │
 ───────┼──────────────────────────────────────────────────────┼──────────
 L3  Services (crates linked into the runtime; .dks at R12)   │
     services/machines       machine/dp/page/voice rows, keys, │
                             native pages, page key/turn slot  │
     services/fx             FX pages, sends, FX param/send/   │
                             effect rows, FX DSP parts         │
     services/samples        tracks, peak windows, sample-     │
                             pool calls, track-table DSP       │
     services/lfo            lfo_shape records                 │
     each: records.rs · slots.rs · dsp/ · tests/               │
        │ C: sdk.toml imports        ▲ B: only declared:       │
        │    + service-only set      │    lifecycle, record-kind handlers,
        ▼                            │    slots/hooks, resources, sinks
 ───────────────────────────────────────────────────────────────┴──────────
 L2  Runtime core (kind-free: never names a machine, effect or service; `layercheck` rule `kind-free`)
     state      State, statics, imports, C API (api.rs), mailbox
     boot       selection, safe mode, boot plan (dependencies, fit, refusals)
     loading    loader, collect (relocate, commit records),
                registry walker + record-kind handler table, params (param-table writer)
     dsp core   DSP area allocator, dsp-link + windows, row table,
                DSP load state machine   (R6a: its transfers go through the link queue)
     storage    store, files, card, scan, arena, worker, settings (broker)
     transport  SysEx/USB: transport, store/admin/module commands
     diagnostics dlog, sinks log/span (R2d), fault (crash), watch, invariants,
                health, report (status, capacity, diag)
     tick       scheduler: service ticks, poll table
        │ platform calls only
 ───────┼─────────────────────────────────────────────────────────────────
 L1  Platform and other image modules (in the patched image)
     platform glue     st, flush, block timing, MIDI send, raw memory
                       (platform.rs: RealPlatform behind the Platform trait; the
                       hooks are in the platform adapter, core/platform)
     image modules     ColdFire hooks and *_info tables: platform (UI loop,
                       main-OS entry, MIDI receive, exception hook), dynreg, dyndsp,
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
- The port file is the port layer: the facts of one device and one firmware, loaded at run time. It is a provider (the boot plan orders it first and refuses its dependents when it is missing), not a service. After its step 3, a call from one of its routines into a service goes through a pointer slot that the service fills, the same pattern as the image interrupt slots today (for example the slot `frametype` of `image-modules/points/machines/module.toml`, which `services/machines/dks/module.toml` fills with `mf_frametype`). L1 never calls a service by address.
- Everything that depends on the device or the firmware version sits in L1: adapters, device profiles and anchors, and the port file ([port file plan](plans/port-file.md); a `.dkport` file on the card, one per device and firmware, picked by the `FW` hash), which holds the firmware names, the hook sites and, after its step 3, the device-specific hook routines of all services. It is not in the SDK, in L2 or in modules. Services are neutral after step 3; until then they keep hook routines per device, and machines and fx keep per-device SHARC dispatch until the SHARC section of the port file is decided.

Three interfaces (details in [runtime and services](plans/runtime-and-services.md) section 5.12):

- **A, module to service:** `sdk.toml` imports by service group and `dk_<service>_open` handles.
- **B, runtime to service:** only what a service declared: lifecycle, record-kind handlers, slots and hooks, resources, and the log and span sinks.
- **C, service to runtime:** the same `sdk.toml` imports as modules, plus a small service-only set.

The runtime owns selection membership. Services read the selected module’s header keys by kind through `CoreApi::keys_of`. Each service owns the numbers it gives those keys. Boot assigns the accepted selection once. Live enable and replacement preview the proposed selection before validating the module. A check-only or refused load restores the previous assignments. An accepted load keeps the new assignments and saves any changed settings. DSP transfer completion is tracked separately. When a live job finishes, the machines service rebuilds the instrument and filter lists in the firmware’s existing selection view. The view caches both lists at startup.

The interfaces are Rust traits in the crate `dkrt-abi` (`runtime/core/abi`): `CoreApi` (C), `Service` and `ServiceList` (B), with the pure types and constants they share (`DspState`, `RegState`, the DSP section types, the row and word constants, memory helpers). The core implements `CoreApi` as `RealCore` and services as `Services`, the static list in `handlers.rs`. A service never calls another service and never names a core module: it calls the core through `crate::core_api` (`REAL_CORE`, or a generic `C: CoreApi` where a host test drives it), and the core calls a service only through `Services` or the C entry points listed in `handlers::exports`.

The crossings of the R2b code, and what R2c made of them:

| R2b crossing | Now |
| --- | --- |
| `tracks` checks `fxpage` and `fxsend` busy; `fx_live` checks `tracks::busy` | The DSP link queue of the core (R6a, `link.rs`): live values with `link_set`, owner requests with `link_submit`. No writer asks whether the link is busy |
| `fx` needs `machines` rows | Both write parameter rows with the shared crate `dkrt-svc-params` and find `dynreg_info` through `CoreApi::info`; neither calls a function of the other (RS-C: no registry state or table in `CoreApi`) |
| `pageclick` reads registry rows | `dkrt_svc_machines::records::param_first`, `param_owners`: the registry state `DKRT_REG` is a static of `machines.dks` |
| `fxsend` uses `settings` | `CoreApi::settings_get` and `settings_put` (a broker resource) |
| `assign` (sticky type numbers) in `machines` | In `machines` again (RS-C, `assign.rs` of the crate): `Service::settle` assigns numbers from the machine keys of the boot or live selection, and the map is the settings key `machines.types` (the selection record's old map is read once: `CoreApi::legacy_type_map`) |
| core calls of `assign`, `fxpage::tick`, `pageclick::tick`, `tracks::tick`, the `dk_fx_routes_entry` export, the nativepage kinds | `Service::tick(phase)`, `handlers::exports`, `dkrt_abi::registry::KIND_NATIVE*` |

### Today and target

The runtime core is in `runtime/core`. The service crates in `services/` use the interfaces of `dkrt-abi` and package their own `.dks` files. The platform modules, app and installer are patched into the image, along with the modules in `image-modules/`. The runtime reached this layout in stages.

- **R2b** moved files into L2 and L3 folders inside the runtime crate.
- **R2c** is done: the interfaces exist with fakes (`FakeCore`, `FakeService`), the shims of R2b are gone, and `check:layers` enforces the arrows (rule 6). What Cargo enforces: `dkrt-abi` cannot see the runtime. What only `check:layers` enforces: that services see only `core_api`. A separate service crate takes the core by reference (`core: &C`, `C: CoreApi`; R9). Exceptions left for R3b and later: the optional `fxpages_info`, `dkpool_*` and `dkpw_*` symbols of the adapters are still declared where they are used; the DSP row table (`DspState`: voice, effect and track rows) is service data inside a core-owned object, because `DKRT_DSP` is read by host tools (`DKRT_REG` likewise); the walkers of `registry.rs` and `dsp/mod.rs` still hold the machine-only tables (`counts`, `types_of`, `free_rows`, `profile`) that R9 and R10 move.
- **R3a** is done: the `Platform` trait (`dkrt-abi`: device profile, block clock, interrupt mask, flush, card block calls, MIDI, OS upgrade flag, soft reset, with `Tasking`, `SamplePool` and `StagedWriter`) and `RealPlatform`, a zero-sized type in `platform.rs`. `platform.rs` is the only runtime file that names `dkfw`, besides `fw.rs` (R10: the implementation of `dkrt_abi::fw::Fw`). The runtime is one image for all devices and holds no device value. The device files give the facts (`[board]`, `[digikit]`, `[capabilities]`), and `patchkit gen` writes them as the data object `dkrt_dev` (struct `DkrtDevice` in `runtime/core/abi.toml`), which patchkit places in `cf.expansion` with the runtime. `rs/dev.rs` is the one private reader of the object; `RealPlatform` fills its `DeviceProfile` from it (cached DDR window, keep RAM, SD host, DSP block cycles, run-time machine types, safe key). A service gets its facts through `DeviceProfile::by_id(core.device_id())`, from the table `abi/src/profiles.rs` that `patchkit gen` writes from the same files. A device without a profile has no DDR window and no run-time types. `FakePlatform` drives the host tests, and the contract table `runtime/core/tests/platform/contract.toml` is checked against it and by `platform-contract.device.test.ts` on the device (rule 7).
- **R3b** is done: the hooks `cf.view.hook.ui_loop`, `cf.rtos.hook.main_os_task` and `cf.midi.hook.rx`, their trampolines and the exception hook install (`dkplat_exc_install`, which `Platform::install_exception_hook` calls every UI pass) are the L1 adapter `core/platform` (`runtime/platform/platform`). Same hook sites, the same stock-byte checks and the same trampoline code as before; the runtime requires the adapter, and the two link by name (the adapter calls `dkrt_tick`, `dkrt_boot`, `dkrt_midi_rx` and `dkrt_exc`), so every image with the runtime has it and `requires` has no cycle. The runtime module keeps only the machine page hooks (finding 128: runtime-owned, with slots); the hooks that must exist before the main OS starts stay patched in the image. Rule 7 checks it.
- **R7** is done: the record kinds, resources and the in-crate services are one table generated from `sdk/sdk.toml` (`gen_services.rs`, `records::`); the core walks a `ServiceTable` of `ServiceRow` function pointers, not a tuple of types; a module emits a record of a generic kind with `dk_record_add`; the `.dkm` header carries a tagged table (needs, uses, keys, and the entries R12 needs); a pure boot planner (`plan.rs`) returns one `Verdict` for each module before anything loads; a service declares its kinds and the runtime compares them with its table. The `lfo` service (`services/lfo`) is the proof: a new kind with no edit to the registry, the collector, the loader or dkneeds. Patchkit needed one line later, when `patchkit records --check` came: the owner map in `packages/patchkit/src/record_rules.rs` names the crate of each service that owns a kind (written by hand, a test fails for an owner without a line). R11 added module states and their reasons (LOADS v2), shown in Transfer and the DIGIKIT menu. What is still target: services loaded from `.dks` files, and a kind or hook that a module provides (R12).
- **R9** is built: the `fx` service is the crate `dkrt-svc-fx` (`services/fx`), linked into the runtime object. Its functions take the core by reference (`core: &C`, `C: CoreApi`: static dispatch, the runtime passes its zero-sized `RealCore`); `Service<C>` methods take it as their first argument, and the service table (`ServiceTable<RealCore>`) holds `&REAL_CORE` and passes it to each call, so `ServiceRow::of::<Fx>` instantiates the fx code once for `RealCore`. `handlers::exports::fx_entry` calls the export `dk_fx_routes_entry` of the fx file, found by name in the loader's export table (an `internal` import of `sdk/sdk.toml`: the runtime calls it, no module can); the guard counts and the machine types of the MODULES reply are the same kind of export of the machines file, and the `Service` trait names no kind. The fx crate has host tests over `FakeCore`. `CoreApi` gained `device_id`. The link queue is the crate `dkrt-link` and `FakeCore` is in `dkrt-test-support` (`runtime/core/{link,test-support}`), so the service crate tests without the runtime. The code is the same up to inlining of the generic functions into the runtime crate (`lto = false`): text 216,286 to 214,994 bytes (-0.6%).
- **R10** is built: the `machines` service is the crate `dkrt-svc-machines` (`services/machines`), linked into the runtime object, and `layercheck` asserts the layout. The same pattern as R9: `Machines` implements `Service<C>`, its statics (the click state in `pageclick.rs`, `APPLIED` in `records.rs`) are statics of the crate, and no `REAL_CORE` is named in it. Two interfaces carry what the service needed from the runtime. The firmware's parameter page is the trait `dkrt_abi::fw::Fw` (`mode_key_held`, `page_value`, `object`, `object_value`, `store`, `view_track`, `param_code`): the runtime implements it once as `RuntimeFw` (`fw.rs`: the page's vtable slots, the key state, the stock setter), a function that needs it takes `fw: &impl Fw`, and `FakeFw` (`dkrt-test-support`) fakes it. The native page registration of the firmware adapter (`mf_native_*`) is `CoreApi::native_available`, `native_register`, `native_remove` and `native_turn` (`native.rs` of the runtime), because the `Service` methods have a core argument and no `Fw`. The hooks stay in the runtime: `dkrt_page_key` and `dkrt_page_turn` (`handlers.rs`, called from `tramp.s`) are `#[no_mangle]` wrappers that call `pageclick::page_key` and `page_turn` with `&REAL_CORE` and `&RuntimeFw`, and `handlers::exports` has `param_get`, `param_set` and `machine_types_of` (the module list asks it; the old `registry::types_of` and the unused `registry::counts` are gone). The crate has host tests over `FakeCore` and `FakeFw`: the click detector, the parameter rules, the native page checks, and the record checks, `apply`, `remove`, `cap` and the native page record of the service (a machine or page row is written at absolute firmware addresses, which a host cannot reach: the device tests cover it). The code is the same up to inlining of the generic functions into the runtime crate (`objdiff --erase RealCore` reports 34 differences, all in the moved machines code, its service entry points and the hook wrappers); text 215,268 to 216,132 bytes (+0.4%).
- **R12-1** is built: `samples` and `lfo` are crates too (`dkrt-svc-samples`, `dkrt-svc-lfo`), no service is a folder of the runtime crate any more (`layercheck`), and all four crates have one layout (`src/lib.rs`, `records.rs`, `slots.rs`, `dsp/` when needed, `tests/`). `module:new -- service NAME` writes it from `sdk/templates/service`; `module:check -- service/NAME` runs `patchkit service-check` (layout, at most one exported symbol `dk_module`, declarations equal to `sdk.toml`) and the crate's tests. `CoreApi` gained `host()` (`dkrt_abi::platform::SampleHost`: pool present, files in the store, interrupt mask, the sample storage) for the platform facts `samples` reads; `FakeCore` has a `FakeHost`, a job queue (`job_pump`) and the `dkpool_*` stubs live in `dkrt-test-support`. The page key and turn slot code of `machines` is `slots.rs`.
- **R12** makes the L3 services real modules (`.dks`). The movable ColdFire hooks go from L1 into their services, and the runtime shrinks to L2.

`core/expansion` is the reset-time installer: it runs at reset and copies all patched-in code to its place. `ports/bindings` holds the generated firmware bindings (a library, not a module).

A service file (`.dks`) holds no firmware address and no firmware bytes. It names firmware through `dk_fw_` (an anchor), `dk_exit_` (the end of a hook's site or an `exits` entry of the hook in `ports/fw`), `dk_ret_` (an `rts`), `dk_stock_` (the stock path, called from the hook) and `dk_tail_` imports ([PATCHING.md](PATCHING.md) "Firmware references"). The port file for the device's firmware (the card file `ports/<device>-<version>.dkport` whose `FW` equals the image `fw_id`) resolves them at load: it carries the name table, the hook sites with a stock check value, and the table item `FW` (the first 8 bytes of the stock image's SHA-256). The loader compares `FW` with the `fw_id` entry that `patchkit apply` puts in the image's info table and refuses a port file for another firmware with `E_HOOK_STOCK`. The port file is built from committed data and carries no firmware bytes. The image info tables hold only what the patched image owns: `machines_info` (version 2, 16 words: registry rows and counts, `mf_ensure`, the native page functions, the slot map, the page rows) and `fxpages_info` (version 4: device numbers, value indices and send cells; its former firmware words are 0).

## Boundaries and tests

This section is the target. Interface B exists as the `Service` trait and its static table (`ServiceTable`, generated into `gen_services.rs`; `handlers.rs` hands it out), with `FakeService` in `handlers/tests.rs`. C is `CoreApi` in `dkrt-abi`, with `RealCore` and `FakeCore` in `core_api.rs`, and the Platform interface is `Platform` in `dkrt-abi`, with `RealPlatform` and `FakePlatform` (`fake_platform.rs`). After R3, test-speed item 7 moves logic tests from the device to the host. The rule for review of deleted tests applies to each move: a host test replaces a device test only when it covers the same path.

Each layer has entry points (what the layer above calls) and exit points (what the layer calls), and is tested at those points.

| Layer | Entry points | Exit points | Tested how |
| --- | --- | --- | --- |
| L1 platform | The Platform interface: time and blocks, flush, MIDI send, raw memory, the `*_info` tables | Stock firmware | A contract suite, run once per device and firmware in the emulator. Everything above uses a `FakePlatform` |
| L2 core | The C API `dkrt_*`, the boot and tick entries, the static service tables | Platform, Storage, the interface B handler tables, the DSP link | Host unit tests with `FakePlatform`, `FakeStorage` and `FakeService` (it records handler calls): loader, boot plan, fit, refusals, registry walker, later the R6 queue |
| L3 services | The interface B handlers | Interface C, a `CoreApi`: params, rows, link, settings, log | Host unit tests of each service against a `FakeCore` (crate `dkrt-test-support`: it records row writes and runs the real link queue of `dkrt-link` over a model of the link) |
| L4 modules | The `dk_module` lifecycle and the kind contract | `sdk.toml` imports | Conformance tests, and module host tests against fake services |

Rules:

- Parts inside a layer depend on small interfaces, not on each other's internals, so each part has its own unit tests.
- Each interface has one contract test suite. It runs against the fake and against the real implementation, so the fake cannot drift.
- Target pyramid: many host tests per layer, one contract suite per boundary (the only emulator part, once per device), and a small set of end-to-end device scenarios.

## Roles

| Layer | Folder | `role` | Runs | May depend on |
| --- | --- | --- | --- | --- |
| L2 Runtime core | `runtime/core` | `runtime` | At boot and in the UI and block loops | Firmware bindings, SDK crate `dkrt-sdk`, interface crate `dkrt-abi`, adapters through optional symbols |
| L3 Service | `services/NAME` (module manifest in `dks/`) | `service` (checked by `patchkit service-check`) | Loaded from a `.dks` file | The core through `CoreApi` (interface C) and `dkrt-abi`, other services through imports (interface A) |
| L1 Installer | `runtime/platform/expansion` | `installer` | At reset, once | Firmware bindings |
| L1 Firmware bindings | `ports/bindings` | none (not a module) | Never; generated by `patchkit gen` from `ports/fw/` | Anchors |
| L1 Platform or other image module | `runtime/platform/NAME` (except `expansion`), `image-modules/{points,adapters,fixes,proofs}/NAME` | `adapter` | Inside firmware hooks, in the firmware's contexts | Firmware bindings, runtime exports, other adapters' headers |
| L4 App | `runtime/apps/NAME` | `app` | In the task context, called by an adapter | SDK, `dkrt.h`, `dk_ui_core.h` |
| L4 Content module | `modules/<kind>/<author>/<name>`, `tests/fixtures/modules` | none | Loaded at run time from the card (a `.dkm`) | SDK only |
| SDK contract | `sdk/` | none | Not code that runs; the contract between L4 and L2 and L3 | Nothing |

## Rules

`layercheck` reads manifests, sources and built objects. It builds nothing; for the objects of content modules it asks patchkit where the objects of this tree's sources are stored (`patchkit build --paths`, about 10 s), so a cache entry another worktree built is never read.

1. **Roles.** Manifests under `runtime/`, `services/` and `image-modules/` set `[module].role` according to the table above.
2. **Sources.** Content modules and apps include only their own files and the SDK (`<dk/...>`, `sdk/dsp/contract`). An app may also include `dkrt.h` and `dk_ui_core.h`. Only the runtime (its `platform.rs`, rule 7), the installer and adapters include `dk_fw_*` headers or depend on the `dkfw` crate. C, ColdFire and SHARC assembly (`#include`, `.include`) and Rust (`dkfw`, path dependencies) are checked.
3. **Objects.** The undefined symbols of the built objects of apps and content modules are SDK imports (`sdk/sdk.toml` `[[import]]`), `dkrt_*`, `dk_ui_*`, `dk_app_*` or the SHARC externs the SDK documents. They never name a firmware anchor (`cf.*`, `dsp.*`, `hook.*`, `pk.*`), and `[object.externs]` never maps a symbol to one.
4. **Apps do not patch.** An app has no `[[op]]` and no `[[claim]]`. If an app needs a firmware hook, the adapter that owns that plug-in point installs the hook and calls the app through an optional symbol (`dk_app_*`).
5. **Capabilities cost nothing unused.** A module asks for capabilities; adapters implement them per device. Prefer data (a field an adapter already passes) to a new firmware hook. A hook must cost at most one flag check on a hot path when no loaded module asks for its capability ([capabilities plan](plans/capabilities.md)).

6. **Arrows inside the runtime.** Every service is a crate (`services/NAME`); no service is a folder of the runtime crate. A service crate (`services/NAME`) depends only on `dkrt-abi`, `dkrt-sdk` and `dkneeds`, and on `dkrt-test-support` as a dev-dependency; it takes the core as a `&C` argument with `C: CoreApi`. Test fakes (`dkrt-test-support`, the `fake` feature of `dkrt-link`) are dev-dependencies of the runtime crate and of services only; `check:layers` reads renamed and `[target.*]` dependencies too. The core names a service only in `handlers.rs` and the generated `gen_services.rs`.

7. **Platform.** The runtime module installs no firmware hook except the machine page hooks (`cf.page.hook.source_key`, `source_turn`); the rest are in the platform adapter. Only `platform.rs` and `fw.rs` (at the source root of the runtime crate; it must implement `dkrt_abi::fw::Fw`) name the firmware crate `dkfw`; the rest of the core and the services use the `Platform` and `Fw` traits. Only `rs/dev.rs` reads `dkrt_dev`. No runtime or service file compares the device id with a number: the runtime asks the device profile (`crate::profile()`, `Platform::has_dsp`, `samples_in_store`, `sample_path_max`), and a service asks `DeviceProfile::by_id(core.device_id())`. The values come from the device files.

8. **Kind-free core.** The runtime's own code (the runtime crate's sources without its host tests, `handlers.rs` and the generated `gen_services.rs`) names no machine or effect: no builder call, no record kind of a machine or an effect, no table of an image adapter that only a service reads (`dynreg_info`, `machines_info`, `fxpages_info`, `mf_native_*`) and no machines registry state. The machines and fx service files export the builder calls (`provided = true` in `sdk/sdk.toml`), keep the state of a module's `init` and write their records in `Service::module_commit`: two passes (`PREPARE` checks and measures, `WRITE` writes and declares the DSP rows) over the one record block of the module; the loader appends the generic records and runs the registry walker (`check`, `apply`, `apply_record`, `remove` of the services that own the kinds). Every service also has `validate_records` (required, no default): the rule for the records of its kinds as far as the bodies of one module's records of a kind decide it; what needs the device, the module's image, another kind or the registry state stays in `check`. `check` calls the same code at load, and `patchkit records --check` calls the service's inherent `validate` (which the trait method calls) on a built module, so one rule serves both. A service without a rule writes `dkrt_abi::accepts_records!();`. The DSP row tables are the services' own: the voice rows and the guard's counts are `DKRT_VROWS` and `DKRT_GUARD` of `machines`, the effect rows `DKRT_FROWS` and `DKRT_FXGUARD` of `fx`; each service reads its adapter's table (`dyndsp_info`, `dynfx_info`) through `CoreApi::info`, logs the DSP words it owns in `Service::diag`, and checks its tables against the module records in `Service::table_check` (invariant 3). `DspState` keeps only the loader's state (the module ranges, the link request, the loads in flight). What is left in the core of this kind is the area allocator (it reads `dyndsp_info` itself), the effect list of the FX ROUTES menu (the export `dk_fx_routes_entry`) and the sample worker (`job_post`, until `samples.dks`).

Today `check:layers` enforces these eight rules: roles, which sources and objects each role may use (so L4 never reaches firmware names and L1 is the only layer with `dk_fw_*`), no patching by apps, the capability cost, the arrows between L2 and L3 (including the dependencies and sources of the service crates), the platform boundary of the runtime, and a core that names no machine or effect (`kind-free`).

Exceptions are listed in `layercheck.py` with a reason: `test/registry-demo` patches the firmware itself (it is patchkit's example of a hook), and `test/ui-demo` stands in for an app.

Run it:

```sh
mise run check:layers          # or: uv run python -m tools.devtools.layercheck
```

Each violation prints `file:line: rule: what is wrong. Instead: what to do.` The gate runs it in `python-checks` (with ruff and mypy) whenever modules, the SDK or the firmware declarations change; `mise run test:changed` runs it too.

## Profiling

Three sources feed one recorder. The Profile pane and the `wb` CLI show the result as one `dk.profile/1` artifact.

```text
 runtime spans (device and emulator)      debug module, SysEx PROFILE_STATE / PROFILE_READ
 cf.profile    (emulator only)            feature cf-profile, run-time toggle, in every workbench build
 sharc.profile (emulator only)            needs a DSP and a separate build (feature sharc-profile)
          |
          v
 recorder (apps/desktop/ui/src/profile-backend.ts)
   one start / stop / reset / read interface per source;
   a source that is missing or fails is reported with its reason
          |
          v
 dkprof (packages/dkprof, dkprof.wasm)
   the dk.profile/1 artifact, naming through the image address maps, the budget;
   shared by the workbench and the `wb` CLI
```

The spans run on hardware and in the emulator through the same debug module and session layer. The two instruction profiles are `cf.profile {enable?}` and `sharc.profile {enable?}` of the control daemon ([CONTROL.md](CONTROL.md)): `enable: true` starts and clears, `false` stops and clears, no argument reads. A firmware reload starts a new emulator; the daemon starts a profile that was on again on the new one and counts the restarts in its answer, so the recorder can say the recording has a gap. The span clock is device data: `span_clock` and `span_clock_hz` in `[digikit]` of each device file name the timer the spans read. Details: [INSTRUMENTATION.md](INSTRUMENTATION.md#profiles).

## Emulator

The emulator ([packages/emulator/README.md](../packages/emulator/README.md)) runs the ColdFire in `elektron-native-boot` and puts everything the main CPU talks to as a second processor behind one boundary, the `Companion` trait of `digi-coupled`. A device file chooses the companion: the SHARC+ session on DT2 and DN2, the peer ColdFire on the Syntakt, and an inert responder (mailbox answers, no audio) when the DSP or the peer is off or absent. `AudioOut` is the one audio path for all of them: the companion pushes its PCM into it, and the player, the tap, the capture and the pacing read from it. The instruction rate is a property of each emulator (`DIGI_CF_IPS`, else the device's rate), not a global. The generated SHARC core is a library that the host loads, so hosts compile without it. Plan and reasons: [emulator-core-split.md](plans/emulator-core-split.md).

## How a new device or firmware version fits

A new firmware version of a known device:

1. Add the anchors to `ports/` (they resolve by hash) and check the declarations in `ports/fw/`.
2. Run `patchkit gen`. The bindings in `ports/bindings` follow.
3. Fix what changed in the adapters. The adapters hide the difference; no SDK header, no content module and no app changes.

A new device:

1. Add the device profile (`devices/NAME.toml`): limits, patch sets, panel, storage and the values the adapters read (`docs/design/device-data.md`).
2. Add its ports and, where its firmware differs, device-specific adapter sources (`src/cf/NAME/`, `src/dsp/NAME.s`) behind the same adapter API.
3. Keep the SDK contract. A content module that builds against the SDK loads on the new device when the runtime reports the contract version.

## Where the device specifics live

| What | Where |
| --- | --- |
| Firmware addresses and hook sites | `ports/` (anchors by hash) |
| Firmware function and structure declarations | `ports/fw/`, generated into `ports/bindings` |
| Limits, patch sets, panel, storage, span clock | `devices/*.toml` (device profiles) |
| Facts the runtime and services read | `dkrt_dev` object (`DkrtDevice`), `DeviceProfile` |
| Code that hooks or wraps firmware | `runtime/platform/*`, `image-modules/*/*`, and the movable hooks in `services/NAME/dks/` |
| The module contract | `sdk/` (device-neutral) |

## Worked example: the UI path

A menu or screen of an app or module goes through these steps:

1. **Module.** A module calls `dk_ui_menu_new`, `dk_ui_text`, `dk_ui_open` and the other calls of `<dk/platform/ui.h>` (`sdk/include/dk/platform/ui.h`). It knows no firmware.
2. **Runtime.** The runtime links the module's imports when it loads the `.dkm`. The `dk_ui_*` calls are `[[import]]` entries of `sdk/sdk.toml` with `requires = "core/ui"`: the runtime's object names them as `optional` symbols, and patchkit resolves them to the functions of `core/ui`, or to 0 when `core/ui` is not in the image. For 0 the runtime hands out a stub that returns the API's failure value (the entry's `absent`: null, -1 or 0, or nothing for a void call), so a module that uses the UI still loads. An app (a core module) links the same symbols directly through patchkit.
3. **Adapter `core/ui`.** `runtime/platform/ui` implements the calls. It builds the firmware's own views (`VerticalMenuView`, list and text views) through the generated bindings (`dk_fw_view.h`, `dk_fw_menu.h`) and keeps their state.
4. **Firmware views.** The firmware draws and handles the keys.

The SETTINGS entry shows the rule for hooks. `core/ui` owns the hook at the end of the SETTINGS list rebuild (`cf.menu.hook.settings_list.tail`). It calls the optional symbol `dk_app_settings_rows(settings_menu)` (declared in `dk_ui_core.h`). The app `core/digikit-menu` implements it with `dk_ui_stock_row` and adds the DIGIKIT row. The app has no hook, no claim and no firmware include. When no app is in the image, patchkit resolves the symbol to 0 and the hook does nothing.

A module with settings sets the `menu` field of its `dk_module` (`void menu(dk_module_ctx *ctx, dk_ui_menu *menu)`; the `.dkm` header carries its address and `DkrtModule.menu` and `DkrtModule.ctx` give it to the app). The MODULES list of `core/digikit-menu` shows SETTINGS... in the side panel of a loaded module that has one, builds a menu once for it (titled with the module's name) and calls `menu` to fill it only when the user opens it. The check mark of the row stays the only on/off of a module; the runtime owns it, and the module owns only its own settings.

**Lifetime rule.** Menus are never freed, so a module that builds menus of its own is never unloaded or reloaded at run time; a module without menus can be loaded, unloaded and reloaded (`runtime/core/src/rs/src/reload.rs`). So every `ctx`, callback and descriptor that a module hands to `<dk/platform/ui.h>` must point at the module's own static data or code, never at the stack, and menus are built when the user opens them, after all modules are loaded. Then no UI pointer outlives the module that owns it.

The same holds for card reads: the INFO page of the menu gets the module header from `dkrt_module_header` of the runtime, which does the card read in its own buffer.

## See also

- [Repository layout](LAYOUT.md), [testing](TESTING.md)
- [Modules: what they are and why](MODULES.md), [module reference](../modules/README.md), [SDK](../sdk/README.md)
- [Firmware declarations](design/firmware-decls.md), [device data](design/device-data.md), [runtime](design/digikit-runtime.md), [boot screen format](design/dkbs.md)
