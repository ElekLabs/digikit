# Architecture

This file is the reference for the layers (L0 to L4), what each layer may depend on, the interfaces between the runtime, services and modules, and how a new device or firmware version fits. `tools/devtools/layercheck.py` enforces the rules it can check today (`mise run check:layers`).

## Layers

Five layers, L0 to L4, and the port file beside them. The [taxonomy](design/taxonomy.md) defines the terms and card layout. The [repository layout](LAYOUT.md) shows where the layers live.

```
 L4  Content modules (.dkm, loaded from the card)       Apps (digikit-menu)
        │ A: SDK or service-local imports                  │ dkrt_* and UI APIs
 ───────┼──────────────────────────────────────────────────┼─────────
 L3  Versioned services (.dks, loaded at boot)             │
     samples, machines, fx, lfo, shortcuts, project        │
        │ C: SDK and service-only imports                  │
        ▼                         ▲ B: declared callbacks │
 ──────────────────────────────────────────────────────────┴─────────
 L2  Runtime core (patched in)
     boot, selection, safe mode, planning, loading, registry, DSP
     loading and link, store, settings broker, transport, diagnostics, tick
        │ platform interface
 ───────┼────────────────────────────────────────────────────────────
 L1  Installer, platform adapters and firmware plug-in points (patched in)
     reset and boot entry, UI/MIDI/exception hooks, shared tables,
     firmware-time stubs, DSP link and stock fallbacks
        │ bindings and device facts             Port file (.dkport)
 ───────┼────────────────────────────────────────────────────────────
 L0  Stock firmware and hardware (ColdFire, SHARC, card, USB)
```

- **A, consumer to service.** A content module or another service calls an SDK import or an explicitly declared service-local import. An SDK import can resolve to the core, an image capability or a service export. A local import declares the provider, minimum API version and function names. Services use these declared imports for cross-service calls.
- **B, runtime to service.** The loader checks declarations from `init` against the packed metadata before accepting a provider. A service can register a `ServiceRow` for lifecycle, record handling and DSP work. Other optional facets include exports, hooks, slot fills, resources and diagnostic sinks. Firmware-facing callers enter loaded services through a validated installed hook or a filled slot.
- **C, service to runtime.** The service implementation uses `CoreApi` in `dkrt-abi`. The card wrapper implements it with service-only `dkc_*` imports, while host tests can supply `FakeCore`. `Service` and the C-ABI `ServiceRow` in `dkrt-abi` define the callback boundary. `gen_services.rs` reserves a prefix for in-image rows, currently empty, and `svc_rows.rs` registers the loaded rows.

The core owns shared mechanisms and safety policy, including admission, hook installation, scheduling, storage, diagnostics and generic settings and reload operations. A service owns its domain policy and state, including the meaning of any records it declares. A static library such as `services/rows` supplies code at build time. It creates no provider identity or managed lifetime. Shared live state, such as reserved descriptor rows, needs an owner even when multiple services use the same library to write it. The menu renders state and sends requests through runtime and UI APIs. It should leave domain decisions and persistence formats with their owners.

A service needs an image capability only where its contract calls for one. `project.dks` and `shortcuts.dks` install hooks from the port file and have no separate image module. Image capabilities can support more than one service. The port file matches the stock firmware hash and supplies firmware names, hook sites and check values. The installer, platform entry points, firmware-time fallback paths and shared tables that stock code needs remain image-owned. A service wrapper can still contain device-specific hook or DSP routines that bind through the port or the image.

### Implementation history

These stages record the architecture at the time they landed. The current ownership and packaging are described above and in the inventories below.

R2b crossings and their replacements:

| R2b crossing | Replacement |
| --- | --- |
| `tracks` checks `fxpage` and `fxsend` busy; `fx_live` checks `tracks::busy` | The DSP link queue of the core (R6a, `link.rs`): live values with `link_set`, owner requests with `link_submit`. No writer asks whether the link is busy |
| `fx` needs `machines` rows | Both write parameter rows with the shared crate `dkrt-svc-params` and find `dynreg_info` through `CoreApi::info`; neither calls a function of the other (RS-C: no registry state or table in `CoreApi`) |
| `pageclick` reads registry rows | `dkrt_svc_machines::records::param_first`, `param_owners`: the registry state `DKRT_REG` is a static of `machines.dks` |
| `fxsend` uses `settings` | `CoreApi::settings_get` and `settings_put` (a broker resource) |
| `assign` (sticky type numbers) in `machines` | In `machines` again (RS-C, `assign.rs` of the crate): `Service::settle` plans the numbers once per boot from the machine keys of the selection, and the map is the settings key `machines.types` (the selection record's old map is read once: `CoreApi::legacy_type_map`) |
| core calls of `assign`, `fxpage::tick`, `pageclick::tick`, `tracks::tick`, the `dk_fx_routes_entry` export, the nativepage kinds | `Service::tick(phase)`, `handlers::exports`, `dkrt_abi::registry::KIND_NATIVE*` |

- **R2b** moved files into L2 and L3 folders inside the runtime crate.
- **R2c** is done: the interfaces exist with fakes (`FakeCore`, `FakeService`), the shims of R2b are gone, and `check:layers` enforces the arrows (rule 6). What Cargo enforces: `dkrt-abi` cannot see the runtime. What only `check:layers` enforces: that services see only `core_api`. A separate service crate takes the core by reference (`core: &C`, `C: CoreApi`; R9). Exceptions left for R3b and later: the optional `fxpages_info`, `dkpool_*` and `dkpw_*` symbols of the adapters are still declared where they are used; the DSP row table (`DspState`: voice, effect and track rows) is service data inside a core-owned object, because `DKRT_DSP` is read by host tools (`DKRT_REG` likewise); the walkers of `registry.rs` and `dsp/mod.rs` still hold the machine-only tables (`counts`, `types_of`, `free_rows`, `profile`) that R9 and R10 move.
- **R3a** is done: the `Platform` trait (`dkrt-abi`: device profile, block clock, interrupt mask, flush, card block calls, MIDI, OS upgrade flag, soft reset, with `Tasking`, `SamplePool` and `StagedWriter`) and `RealPlatform`, a zero-sized type in `platform.rs`. `platform.rs` is the only runtime file that names `dkfw`, besides `fw.rs` (R10: the implementation of `dkrt_abi::fw::Fw`). The runtime is one image for all devices and holds no device value. The device files give the facts (`[board]`, `[digikit]`, `[capabilities]`), and `patchkit gen` writes them as the data object `dkrt_dev` (struct `DkrtDevice` in `runtime/core/abi.toml`), which patchkit places in `cf.expansion` with the runtime. `rs/dev.rs` is the one private reader of the object; `RealPlatform` fills its `DeviceProfile` from it (cached DDR window, keep RAM, SD host, DSP block cycles, run-time machine types, safe key). A service gets its facts through `DeviceProfile::by_id(core.device_id())`, from the table `abi/src/profiles.rs` that `patchkit gen` writes from the same files. A device without a profile has no DDR window and no run-time types. `FakePlatform` drives the host tests, and the contract table `runtime/core/tests/platform/contract.toml` is checked against it and by `platform-contract.device.test.ts` on the device (rule 7).
- **R3b** is done: the hooks `cf.view.hook.ui_loop`, `cf.rtos.hook.main_os_task` and `cf.midi.hook.rx`, their trampolines and the exception hook install (`dkplat_exc_install`, which `Platform::install_exception_hook` calls every UI pass) are the L1 adapter `core/platform` (`runtime/platform/platform`). Same hook sites, the same stock-byte checks and the same trampoline code as before; the runtime requires the adapter, and the two link by name (the adapter calls `dkrt_tick`, `dkrt_boot`, `dkrt_midi_rx` and `dkrt_exc`), so every image with the runtime has it and `requires` has no cycle. At this stage the runtime still installed the machine page hooks (finding 128). R12 moved them to `machines.dks`.
- **R7** is done: the record kinds, resources and the in-crate services are one table generated from `sdk/sdk.toml` (`gen_services.rs`, `records::`); the core walks a `ServiceTable` of `ServiceRow` function pointers, not a tuple of types; a module emits a record of a generic kind with `dk_record_add`; the `.dkm` header carries a tagged table (needs, uses, keys, and the entries R12 needs); a pure boot planner (`plan.rs`) returns one `Verdict` for each module before anything loads; a service declares its kinds and the runtime compares them with its table. The `lfo` service (`services/lfo`) is the proof: a new kind with no edit to the registry, the collector, the loader or dkneeds. Patchkit needed one line later, when `patchkit records --check` came: the owner map in `packages/patchkit/src/record_rules.rs` names the crate of each service that owns a kind (written by hand, a test fails for an owner without a line). R11 added module states and their reasons (LOADS v2), shown in Transfer and the DIGIKIT menu. R12 later moved service loading to `.dks` files.
- **R9** is built: the `fx` service moved into the crate `dkrt-svc-fx` (`services/fx`), initially linked into the runtime object. Its functions take the core by reference (`core: &C`, `C: CoreApi`: static dispatch, the runtime passes its zero-sized `RealCore`); `Service<C>` methods take it as their first argument, and the service table (`ServiceTable<RealCore>`) holds `&REAL_CORE` and passes it to each call, so `ServiceRow::of::<Fx>` instantiates the fx code once for `RealCore`. `handlers::exports::fx_entry` calls the export `dk_fx_routes_entry` of the fx file, found by name in the loader's export table (an `internal` import of `sdk/sdk.toml`: the runtime calls it, no module can); the guard counts and the machine types of the MODULES reply are the same kind of export of the machines file, and the `Service` trait names no kind. The fx crate has host tests over `FakeCore`. `CoreApi` gained `device_id`. The link queue is the crate `dkrt-link` and `FakeCore` is in `dkrt-test-support` (`runtime/core/{link,test-support}`), so the service crate tests without the runtime. The code is the same up to inlining of the generic functions into the runtime crate (`lto = false`): text 216,286 to 214,994 bytes (-0.6%).
- **R10** is built: the `machines` service moved into the crate `dkrt-svc-machines` (`services/machines`), initially linked into the runtime object, and `layercheck` asserted the layout. The same pattern as R9: `Machines` implements `Service<C>`, its statics (the click state in `pageclick.rs`, `APPLIED` in `records.rs`) are statics of the crate, and no `REAL_CORE` is named in it. Two interfaces carry what the service needed from the runtime. The firmware's parameter page is the trait `dkrt_abi::fw::Fw` (`mode_key_held`, `page_value`, `object`, `object_value`, `store`, `view_track`, `param_code`): the runtime implements it once as `RuntimeFw` (`fw.rs`: the page's vtable slots, the key state, the stock setter), a function that needs it takes `fw: &impl Fw`, and `FakeFw` (`dkrt-test-support`) fakes it. The native page registration of the firmware adapter (`mf_native_*`) is `CoreApi::native_available`, `native_register`, `native_remove` and `native_turn` (`native.rs` of the runtime), because the `Service` methods have a core argument and no `Fw`. The hooks stay in the runtime: `dkrt_page_key` and `dkrt_page_turn` (`handlers.rs`, called from `tramp.s`) are `#[no_mangle]` wrappers that call `pageclick::page_key` and `page_turn` with `&REAL_CORE` and `&RuntimeFw`, and `handlers::exports` has `param_get`, `param_set` and `machine_types_of` (the module list asks it; the old `registry::types_of` and the unused `registry::counts` are gone). The crate has host tests over `FakeCore` and `FakeFw`: the click detector, the parameter rules, the native page checks, and the record checks, `apply`, `remove`, `cap` and the native page record of the service (a machine or page row is written at absolute firmware addresses, which a host cannot reach: the device tests cover it). The code is the same up to inlining of the generic functions into the runtime crate (`objdiff --erase RealCore` reports 34 differences, all in the moved machines code, its service entry points and the hook wrappers); text 215,268 to 216,132 bytes (+0.4%).
- **R12-1** is built: `samples` and `lfo` are crates too (`dkrt-svc-samples`, `dkrt-svc-lfo`), no service is a folder of the runtime crate any more (`layercheck`), and all four crates have one layout (`src/lib.rs`, `records.rs`, `slots.rs`, `dsp/` when needed, `tests/`). `module:new -- service NAME` writes it from `sdk/templates/service`; `module:check -- service/NAME` runs `patchkit service-check` (layout, at most one exported symbol `dk_module`, declarations equal to `sdk.toml`) and the crate's tests. `CoreApi` gained `host()` (`dkrt_abi::platform::SampleHost`: pool present, files in the store, interrupt mask, the sample storage) for the platform facts `samples` reads; `FakeCore` has a `FakeHost`, a job queue (`job_pump`) and the `dkpool_*` stubs live in `dkrt-test-support`. The page key and turn slot code of `machines` is `slots.rs`.
- **R12** moved services into `.dks` packages. Service-owned ColdFire hooks moved with them, and the SHARC hooks kept resident stubs with service-owned dispatch behind slots. Six services now ship as card packages.

`core/expansion` is the reset-time installer: it runs at reset and copies all patched-in code to its place. `ports/bindings` holds the generated firmware bindings (a library, not a module).

A service file (`.dks`) holds no firmware address and no firmware bytes. It names firmware through `dk_fw_` (an anchor), `dk_exit_` (the end of a hook's site or an `exits` entry of the hook in `ports/fw`), `dk_ret_` (an `rts`), `dk_stock_` (the stock path, called from the hook) and `dk_tail_` imports ([PATCHING.md](PATCHING.md) "Firmware references"). The port file for the device's firmware (the card file `ports/<device>-<version>.dkport` whose `FW` equals the image `fw_id`) resolves them at load: it carries the name table, the hook sites with a stock check value, and the table item `FW` (the first 8 bytes of the stock image's SHA-256). The loader compares `FW` with the `fw_id` entry that `patchkit apply` puts in the image's info table and refuses a port file for another firmware with `E_HOOK_STOCK`. The port file is built from committed data and carries no firmware bytes. The image info tables hold only what the patched image owns: `machines_info` (version 2, 16 words: registry rows and counts, `mf_ensure`, the native page functions, the slot map, the page rows) and `fxpages_info` (version 4: device numbers, value indices and send cells; its former firmware words are 0).

## Service contract and boot

The target minimum core reads card and selection data needed for bootstrap, manages the generic package lifecycle, and maintains health, crash capture and boot-loop recovery. It can boot and recover without any card service. Directory clients and their listing buffers, along with project-specific policy, are optional users of shared primitives. Today the core also implements store scanning, settings brokering and the live reload queue.

The reset installer and platform hooks bring up the runtime before it can read the card. The core reads the selection and decides safe mode before loading any provider or content module. In safe mode the provider installer stays disarmed and resident service entry points retain their stock fallback. The platform and recovery UI still run. In normal mode the planner follows hard needs from the selection, orders the matching port and providers before their consumers, and refuses a consumer whose required provider cannot load. Placing a `.dks` on the card alone does not select it.

Callback order governs the walk of registered service rows. It is independent of the dependency order used for package initialization. Runtime 0.11.1 adds optional `callback_order` and service-local API metadata. Loaded rows follow any in-image rows, with ascending explicit order and ties broken by service name. Unordered services follow them. Metadata-less copies of the six shipped packages keep their positions through the name fallback in `svc_rows.rs`. Legacy SDK imports keep their existing lookup and inference. Explicit local imports bind to the named provider and require a compatible API version. Transfer checks the runtime minimum for the new metadata.

A local API can add a provider and ColdFire function exports without a central SDK entry. SDK-wide imports and record kinds still use the SDK contract. A provider that registers a row must match the runtime's service ABI. Discovery and admission use bounded tables, with eight provider slots in the boot plan, including the port when needed. Service and port replacement take effect on restart. For the manifest and working provider/consumer example, see [card services with a local API](MODULE-AUTHORING.md#card-services-with-a-local-api).

### Choosing an API

The SDK defines shared public contracts and their generated C and Rust bindings and packaging metadata. For example, a module's `dk_fx_routes` builder call comes from `sdk/sdk.toml`. Its implementation and route policy run in `fx.dks`. Add a public SDK import or record definition when the project will maintain its compatibility, bindings and conventions as part of the shared SDK. A provider can support several consumers through its own versioned API.

A service can define a narrower API in its own headers and export it from its `.dks`. The `local-ping` fixture and its consumer illustrate this path. The consumer declares `[[service_import]]` with the provider name, minimum compatible version and symbols. The packer records the hard need, and the loader links only to the matching provider. Card modules use these declared imports or SDK imports.

A patched-in resident app can query an optional card provider through `dkrt_service_query` (runtime 0.11.2). The provider declares one `[[export]]` named `dk_service_query_v1`, and passes its implementation as `query = path::dk_service_query_v1` to `service!`. The glue checks the entry against the fixed C signature `ResidentQueryV1`. Patchkit marks the export as a resident query and adds the 0.11.2 runtime minimum. The loader activates it with its owning provider and withdraws it when the provider is retired. Each call checks the provider identity, compatible version and live entry. The entry stays private to the broker.

The provider owns operation numbers, typed request and reply layouts, validation and status codes. Publish the wrapper with the provider contract and use a new operation number for a changed layout. Operations run synchronously in the UI task, read state without blocking, and must return before any UI or loader work resumes. They cannot recurse, retain the buffers or publish service addresses. `DKRT_E_MISSING` reports an absent provider or facet. The caller chooses the fallback. A runtime change is needed for a new shared mechanism or lifecycle rule. New operations within this query contract add code to their provider and caller.

The menu keeps pending FX route choices and asks the FX provider which routes fit. The typed wrapper is [dk/fx/route-query.h](../sdk/include/dk/fx/route-query.h). The FX provider reads its effect rows and send-slot capacity. Existing `dkrt_fx` enumeration keeps the same layout and returns an empty list when the provider is absent. The menu uses the previous picker rule when an older provider can enumerate effects but lacks the query facet.

### Shipped services

| Service | Callback order | Owns | Optional facets used |
| --- | --- | --- | --- |
| [samples](../services/samples/dks/module.toml) | 100 | Per-module sample pools, track state and peak windows | Sample-pool API, pool completion/loss fills, DSP track-table parts |
| [machines](../services/machines/dks/module.toml) | 200 | Machine types, parameters and pages, native pages, sticky type assignment | Record handlers and builders, module diagnostics, ColdFire hooks, DN2 frame-type fill and DSP voice dispatch |
| [fx](../services/fx/dks/module.toml) | 300 | Effect records, pages, parameters, kit values and routes | Record handler and builders, route enumeration and choices, ColdFire page hooks and DSP dispatch fills |
| [lfo](../services/lfo/dks/module.toml) | 400 | LFO shape records, numbering and wave bounds | Record handler, six interrupt fills per device and MOD UI hooks |
| [shortcuts](../services/shortcuts/dks/module.toml) | 500 | Shortcut records and dispatch | Record handler, shortcut builder and a port-resolved key-walker hook |
| [project](../services/project/dks/module.toml) | 600 | `.dkp` files and project-scoped module settings | Project API, settings overlay, tick and port-resolved project hooks |

### Firmware plug-in points

The inventory covers DT2 1.17 and DN2 1.12. Residency is a property of the executing code, data and fallback path. Moving a source file or a manifest does not change it.

| Point | Resident responsibility | Service ownership and extraction boundary |
| --- | --- | --- |
| [machines](../image-modules/points/machines/module.toml) | Stock-compatible table construction, dispatch, chooser/list adaptation and DN2 frame-type stub | `machines.dks` owns type assignment, record/row lifecycle, page hooks and the frame-type slot target. Retain the chooser and stock table path for boots without the service. |
| [params](../image-modules/points/params/module.toml) | Patch-time descriptor-table copy with device-specific slot mappings and reader/reference patches | `machines.dks` and `fx.dks` build and manage disjoint rows. Keep the shared descriptor layout and reader bounds together. |
| [dynreg](../image-modules/points/dynreg/module.toml) | Machine, parameter and DN2 page reservations, `dynreg_info`, and stock chooser/rebuild integration | `machines.dks` populates and clears its records and rows. `fx.dks` reads the parameter-row layout from `dynreg_info` and writes only the parameter rows reserved by `core/fxpages`. Reservations and empty-row handling are part of the image contract. |
| [dsp-voice](../image-modules/points/dsp-voice/module.toml) | SHARC hook sites, stock fallback, heartbeat, adapter data and DN2 stereo context | `machines.dks` fills render and DT2 unpack slots with dispatch and guard code. Any further split must preserve timing and the live DSP state shared with the resident paths. |
| [fx](../image-modules/points/fx/module.toml) | Post-master and send entry stubs, stock fallback, chain data and registry | `fx.dks` supplies master and send dispatch and output guards. Keep the stubs and check which data has stock or shared callers before moving it. |
| [dynfx](../image-modules/points/dynfx/module.toml) | Empty DSP effect rows and `dynfx_info` over the shared DSP area | `fx.dks` admits effects and publishes rows into the reservation. Moving their policy requires no change to the image allocation. |
| [fxpages](../image-modules/points/fxpages/README.md) | Device page/send tables and reserved parameter rows | `fx.dks` owns nine page and kit hooks and writes effect data. Route-choice policy can move independently of the resident reservation. |
| [lfo](../image-modules/points/lfo/module.toml) | Six interrupt stubs per device with stock behavior when slots are zero | `lfo.dks` fills the slots, evaluates shapes and installs the MOD name and graph hooks. The firmware-time entry stays resident. |
| [sample-pool](../image-modules/points/sample-pool/module.toml) | DT2 stock pool/loader or DN2 DDR and LP0/DSP transfer, with a shared `sample_pool_info` interface | `samples.dks` owns the user API and fills completion/loss slots. Keep the device-specific backing behind the adapter interface. |

### Remaining ownership work

Keep the runtime small and service-agnostic. The core owns boot and recovery, the standard provider contract for admission, version compatibility, task rules and lifetime, and shared mechanisms such as jobs and settings. Providers own domain state and policy and can extend their APIs with optional facets within that contract. In a normal boot, an absent provider leaves its optional interfaces unavailable and the planner refuses consumers that require it. Safe mode leaves all card providers disarmed and keeps the stock entry paths and recovery UI available.

**Progress.** Independent service registration and versioned local imports merged in PR #690. The pushed FX route-policy change (`edf0559bf`) puts route eligibility and send-slot limits in `fx.dks`. Its verification covered FX host rules and the FX route panel and persistence on DT2 and DN2 (`core/fx:test-device--fx-routes-panel` and `core/fx:test-device--fx-routes-persist`). The menu still keeps pending edits, presents choices and writes the selected `fx/route/` keys. The runtime still bridges `dkrt_fx` enumeration to `dk_fx_routes_entry`. Cleanup of the old menu picker fallback is deferred. The steps below target current provider packages. [Runtime export bridge](../runtime/core/src/rs/src/handlers.rs)

1. **Keep the existing machines query.** The internal `dk_machines_module_info` export gathers voice rows, machine types and guard counts for a module owner. The debug subscriber checks types on the UI task, serialized with loader work. MODULES uses the same export from the MIDI task. Static inspection shows a possible overlap with row clearing and reuse. Revisit the query if that overlap produces an inconsistent reply or a consumer needs an exact answer during reload. If a change is needed, each successful MODULES record should carry the identity, machine types and voice rows of one committed module incarnation, with live fault counts and guard flags sampled for those types. Distinguish actual absence from an update in progress. Keep MIDI reads bounded independently of writer progress and preserve the successful wire layout. The guarantee covers one reply record. Pages may observe different commits. Test the existing guards before choosing a mechanism. On the host, unload module A, reuse its slot for B and read midway through a write to catch mixed records and false absence. Cover teardown and provider absence, then reserve a focused MODULES wire check for device-only behavior. The SDK client retries `E_BUSY` for store commands, so check client behavior before introducing a busy reply for MODULES. [MODULES reporter](../runtime/core/src/rs/src/module_cmds.rs), [debug setup](../runtime/core/src/rs/src/report.rs), [SDK contract](../sdk/sdk.toml), [client retry policy](../packages/dk-session/src/dkupload-core.ts)
2. **Keep the project reload boundary.** The project provider compares effective settings, including values inherited from the settings record, and notifies the core once per changed owner. The core's serialized UI-task queue owns reload eligibility, coalescing and retry because it tracks module records, which modules opened settings, and live job state. The existing notification contract covers this split. Host tests cover effective changes and queue behavior. Review existing project device coverage before adding a case for a device-only gap. [Project host tests](../services/project/tests/service.rs), [settings reload queue and host tests](../runtime/core/src/rs/src/settings_reload.rs), [project device coverage](../services/project/tests/project.device.test.ts)

Evaluate subsequent changes at each caller and ownership boundary.

When a service dependency, runtime guarantee or firmware-resident responsibility changes, update this contract or inventory in the same change and link its lowest-layer proof.

## Boundaries and tests

| Layer | Entry points | Exit points | Tested how |
| --- | --- | --- | --- |
| L1 platform | `Platform`, resident hooks and image info tables | Stock firmware | Platform contract on the emulator for each supported device, with `FakePlatform` for host callers |
| L2 core | `dkrt_*`, boot and tick, provider declarations | Platform, storage, registered rows, DSP link | Host tests for planning, fit, refusal, registry and link; assembly tests with real packed providers and image tables for declaration and installation seams |
| L3 services | Declared row callbacks, exports, hook and slot targets | `CoreApi`, SDK imports and declared firmware bindings | Host tests against `FakeCore`; device tests for effects on firmware and DSP paths |
| L4 modules | `dk_module` lifecycle and module-kind contract | SDK and declared service-local imports | Conformance and module tests |

Use the lowest layer that sees the behavior. A host test replaces device coverage only when it exercises the same path. See [test levels and targets](TESTING.md).

## Roles

| Layer | Folder | `role` | Runs | May depend on |
| --- | --- | --- | --- | --- |
| L2 Runtime core | `runtime/core` | `runtime` | At boot and in the UI and block loops | Firmware bindings, SDK crate `dkrt-sdk`, interface crate `dkrt-abi`, adapters through optional symbols |
| L3 Service | `services/NAME` (module manifest in `dks/`) | `service` (checked by `patchkit service-check`) | Loaded from a `.dks` file | The core through `CoreApi` (interface C) and `dkrt-abi`, other services through declared imports (interface A) |
| L1 Installer | `runtime/platform/expansion` | `installer` | At reset, once | Firmware bindings |
| L1 Firmware bindings | `ports/bindings` | none (not a module) | Never; generated by `patchkit gen` from `ports/fw/` | Anchors |
| L1 Platform or other image module | `runtime/platform/NAME` (except `expansion`), `image-modules/{points,adapters,fixes,proofs}/NAME` | `adapter` | Inside firmware hooks, in the firmware's contexts | Firmware bindings, runtime exports, other adapters' headers |
| L4 App | `runtime/apps/NAME` | `app` | In the task context, called by an adapter | SDK, `dkrt.h`, `dk_ui_core.h` |
| L4 Content module | `modules/<kind>/<author>/<name>`, `tests/fixtures/modules` | none | Loaded at run time from the card (a `.dkm`) | SDK and declared service-local imports |
| SDK contract | `sdk/` | none | Not code that runs; the contract between L4 and L2 and L3 | Nothing |

## Rules

`layercheck` reads manifests, sources and built objects. It builds nothing; for the objects of content modules it asks patchkit where the objects of this tree's sources are stored (`patchkit build --paths`, about 10 s), so a cache entry another worktree built is never read.

1. **Roles.** Manifests under `runtime/`, `services/` and `image-modules/` set `[module].role` according to the table above.
2. **Sources.** Content modules and apps include only their own files and the SDK (`<dk/...>`, `sdk/dsp/contract`). An app may also include `dkrt.h` and `dk_ui_core.h`. Only the runtime (its `platform.rs`, rule 7), the installer and adapters include `dk_fw_*` headers or depend on the `dkfw` crate. C, ColdFire and SHARC assembly (`#include`, `.include`) and Rust (`dkfw`, path dependencies) are checked.
3. **Objects.** The undefined symbols of the built objects of apps and content modules are SDK imports (`sdk/sdk.toml` `[[import]]`), declared service-local imports (`[[service_import]]`), `dkrt_*`, `dk_ui_*`, `dk_app_*` or the SHARC externs the SDK documents. They never name a firmware anchor (`cf.*`, `dsp.*`, `hook.*`, `pk.*`), and `[object.externs]` never maps a symbol to one.
4. **Apps do not patch.** An app has no `[[op]]` and no `[[claim]]`. If an app needs a firmware hook, the adapter that owns that plug-in point installs the hook and calls the app through an optional symbol (`dk_app_*`).
5. **Capabilities cost nothing unused.** A module asks for capabilities; adapters implement them per device. Prefer data (a field an adapter already passes) to a new firmware hook. A hook must cost at most one flag check on a hot path when no loaded module asks for its capability ([capabilities plan](plans/capabilities.md)).

6. **Arrows inside the runtime.** Service implementations live in their own crates and take the core through `CoreApi`. Card wrappers bind service-only imports and declare their callbacks to the loader. `check:layers` checks allowed crate dependencies and test-only fakes, including renamed and target-specific dependencies. Service-specific runtime bridges are confined to `handlers.rs` and the generated registration code.

7. **Platform.** The runtime manifest installs no firmware hooks. The platform adapter installs boot, UI-loop and MIDI hooks, and the machines service owns the source-page key and turn hooks. The runtime's `platform.rs` and `fw.rs` adapt firmware through the platform and firmware traits. Only `rs/dev.rs` reads `dkrt_dev`. Runtime and service code use profiles and capabilities for device values, which come from `devices/*.toml`.

8. **Kind-free core.** Builders, record semantics and machine or effect state belong to services. The core walks generic records and calls the owning row for checks, application and teardown. `patchkit records --check` and the service share the rules for records a module can validate before it loads. Device- and registry-dependent checks run at load. The core owns generic DSP allocation and load state, while services own voice and effect rows, guard counts and diagnostics. See [remaining ownership work](#remaining-ownership-work) for the current bridges and policy seams.

`check:layers` enforces the source and object boundaries it can inspect. The assembly and device checks cover declaration and execution behavior.

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
