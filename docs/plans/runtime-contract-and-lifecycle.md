# Runtime contract and package lifecycle

Date: 2026-11-22. Status: **approved by Em 2026-11-22; RC2 active (RC2b), RC2s designed.** Items: **RC0** to **RC6**, plus the deferred **RC-D** (DSP-side installer).

This plan replaces the open parts of [runtime-and-services.md](runtime-and-services.md) and the "Remaining ownership work" of [ARCHITECTURE.md](../ARCHITECTURE.md). Where it disagrees with older docs, this plan wins and the older text is overwritten when the item lands. The static evidence is [finding 158](../findings/158-service-entry-survey.md) (service entries) and [finding 147](../findings/147-syntakt-flash-image-size.md) (Syntakt image size).

## 1. Goal

A small runtime that names no service and no domain concept. It owns boot and recovery, the package lifecycle, safe calling contexts and shared mechanisms. Services own their domain state, policy, records, resources and versioned operations. Content modules depend on services.

Done means:

- A new service, and the modules that use it, ship as card files. No runtime edit, no runtime rebuild, no reflash. The exception: a service that needs a new SHARC entry site needs a flash until RC-D lands (section 9).
- Loading, unloading and reloading a module or a service from the DIGIKIT menu use one code path. Unloading the last consumer of a service unloads the service, unless the selection pins it.
- `image-modules/` does not exist. Everything resident (flashed) is in `runtime/`. A service has no resident code.
- The flashed runtime is a small resident core: boot, safe mode and recovery, store and Transfer, the platform, and a stage-1 loader. The lifecycle, the full loader and the menu app load from the card as a system package (item RC2s). Every item states its size change and justifies any growth (section 11). The Syntakt image fits its container.
- Every behaviour has its test at the lowest layer that sees it (section 10).

## 2. Glossary

One meaning per word. These replace the older terms listed on the right.

| Term | Meaning | Replaces |
| --- | --- | --- |
| **runtime** | Everything resident: `runtime/core` (the core) and `runtime/platform` (the platform). | "image", "image module", "adapter" for all L1 code |
| **core** | `runtime/core`: boot, recovery, the package manager, installers, link, store, settings, jobs, diagnostics, query broker. Device- and service-neutral. | |
| **platform** | `runtime/platform/*`: resident code tied to the hardware or the firmware: the reset installer, the boot, UI-loop and MIDI hooks, `dsp-link`, UI, input, storage and USB drivers, the SHARC slot catalogue. | "adapter", "plug-in point", "fix" |
| **package** | A card file the core loads: a **module** (`.dkm`) or a **service** (`.dks`). | |
| **service** | A package that provides an API (a **provider**). One service per name and major version. | "provider" as a separate noun; "service" for SDK import groups |
| **module** | A content package (machine, effect, LFO shape, tool). It consumes services. | |
| **consumer** | A package that needs a service: a module or another service. | |
| **need** | A consumer's dependency on a service: name, minimum version, imports, resource uses, records. Derived by the packer. | "uses" and "needs" as two tables |
| **pin** | A selection entry that names a service directly, so it stays loaded without a consumer. | |
| **operation** | A versioned function a service exports. Reached through an SDK import or a service-local import. | |
| **query** | A synchronous request from a resident app to a service, in the UI task (`dkrt_service_query`). | |
| **record** | Data a consumer contributes to a service: kind local to the service, checked and applied by it. | global `[records.kinds]` |
| **resource** | Counted capacity a service (or the core) declares; consumers claim units; the core sums and admits. No meaning in the core. | fixed `[capacity.kinds]` ids for domain rows |
| **object** | A live thing a service gives a consumer through its own API (a pool handle, an effect instance). Not a core concept. | "shared live resource" |
| **part** | An optional part of the service contract (records, DSP, tick, query, describe). A service adopts only the parts it needs. | "facet" |
| **entry** | How stock code reaches our code: a **hook** (a code patch at a site) or a **slot** (a pointer word a flashed stub reads). | |
| **installer** | Core object that writes and restores entries: `ColdFireInstaller`, `SharcInstaller`. | |
| **port file** | `.dkport`: firmware names, sites and stock check values for one firmware version. | "binding" |
| **app** | A resident UI program in `runtime/apps` (`digikit-menu`). | |

## 3. Problems this plan fixes

Read from code on 2026-11-22 (lane `lane/fx-route-policy`).

1. **Two lifecycles.** Providers load only at boot through `Slot::Provider`; modules load at boot and at run time. Provider unload exists only as failure handling (`take_down`, `unload_rec_inner` `!module` branch). The reload job refuses `.dks` paths (`reload.rs`).
2. **Two dependency mechanisms.** The planner orders by NEEDS/PROVIDES tables (`plan.rs`); the linker resolves imports separately (`loader/mod.rs` `check_file`). Nothing records which loaded package uses which service. `take_down` finds dependents by scanning boot-time header tables (`needs_hash`); modules loaded at run time may be missed [O].
3. **Domain knowledge in the core.**
   - `handlers.rs` bridges `dkrt_fx` to `dk_fx_routes_entry` and the MODULES reply to `dk_machines_module_info`.
   - `svc_rows.rs` holds a callback-order table naming six services.
   - `[records.kinds]` carries `service = "machines"` and is generated into the core's `gen_sdk.rs`.
   - `[capacity.kinds]` has domain ids (`MACHINE_ROWS`, `VOICE_ROWS`, `FX_SLOTS`, `SEND_SLOTS`, `MAP_ENTRIES`).
   - `DspState` (voice and effect rows) is part of the ABI, and 8 of the 25 `ServiceRow` callbacks are voice or effect row steps.
4. **A fixed service row.** `ServiceRow` is 25 function pointers. Adding one changes `SVC_ABI` for every service.
5. **Service state in statics.** `Service<C>` methods are associated functions; state is crate statics. Host tests cannot make two instances or reset one.
6. **Resident code in the wrong place.**
   - `runtime/platform/dyndsp` requires the `dsp-voice` and `fx` points, and `dsp-link` names `_dl_vrows`, `_dl_frows` and `_sp_tail`: the runtime depends on service points.
   - `platform.rs` calls `dkfw::samples::worker_*`.
   - `image-modules/` mixes plug-in points, drivers, fixes and test proofs.
7. **Size.** The Syntakt `device` set is 5,888 bytes over `container_max` (section 14).

### Relation to other plans

- **E, extension model** ([extension-model.md](extension-model.md), active in lane `ext-model`): E gives content-module hot reload, derived keys with one planner rule and one contract suite per service. It leaves service replacement out of scope. RC1 adds new files only and can run beside E. RC2 and RC3 start after E's open branches merge; they reuse E's contract suite and key rule rather than adding a second one.
- **H, hot reload** ([digikit-hot-reload.md](digikit-hot-reload.md), todo): RC2 replaces it. Its findings on hook gates and task-reachable code are inputs to RC2's `can_stop` and refusal rules.
- **RS-S and RS-C, runtime slimming**: done. RC3 finishes the deferred `fx_entry` bridge.
- **R12-8, SHARC stubs**: the slot catalogue in section 9 is its stub design, owned by the platform instead of the points.

## 4. Target layout

```text
runtime/
  core/            the core (Rust): boot, packages/, install/, link, store, settings, jobs, diag, query
  platform/        resident platform code: expansion, platform, dsp-link (+ SHARC slot catalogue),
                   ui, input, health, storage and USB drivers, sample transport (DN2), peer-image (ST)
  apps/            digikit-menu
services/<name>/
  src/             the service crate: owner objects, host-tested over FakeCore
  dks/             module.toml, service!(...), hooks.rs (entries), fw.rs
  tests/           unit, assembly and device tests of the service
modules/<kind>/<author>/<name>/
sdk/               shared public contract: headers, sdk.toml, templates
ports/             firmware declarations and port files
tests/fixtures/modules/local-ping*   the reference contract fixtures (RC1, extended in RC3)
```

`image-modules/` is removed in RC5. Section 9 lists where each item goes.

### Code layout

Every RC change follows the "Code layout" rule of [AGENTS.md](../../AGENTS.md): one file per concept named after its noun, no logic in `lib.rs` or `mod.rs`, behaviour of a large type split into a folder of `impl` blocks, private state, tests beside the code. The RC crates:

```text
runtime/core/abi/src/package.rs      PackageId (shared by dkrt-fit and dkrt-lifecycle)
runtime/core/abi/src/version.rs      version_satisfies
runtime/core/lifecycle/src/           dkrt-lifecycle
  lib.rs                             crate doc, mod lines, pub use
  graph.rs                           PackageGraph: construction, the acyclic rule, lookups
  graph/bring_up.rs                  bring_up_order
  graph/take_down.rs                 take_down_order, the orphan rule
  node.rs, live.rs, steps.rs, refusal.rs
runtime/core/fit/src/ledger.rs       ResourceLedger
runtime/core/lifecycle/src/manager.rs  RC2: PackageManager, generic over the Placer, Linker, Installer and ProviderHost seams
runtime/core/src/rs/src/lifecycle/   RC2: the runtime's real Placer, Linker and ProviderHost
runtime/core/src/rs/src/install/     RC4: mod.rs (Installer, Entry, Installers), coldfire.rs, sharc.rs
```

## 5. Package manifests

The packer derives every table from these manifests and from the module's `init` calls. The core reads only the packed header tables.

```toml
# services/fx/dks/module.toml
[module]
id = "core/services/fx"            # ids do not change
role = "service"
service = "fx"
version = "3.0.0"                  # major = API compatibility

[[service.resource]]
name = "effect_slots"              # consumers write fx.effect_slots
limit = "profile:fx_slots"         # a number, "profile:<key>" (devices/*.toml), or "init" (declared in start)

[[service.resource]]
name = "fx_cost"
draws_from = "core.dsp_cost"       # units also count against the core's DSP budget
aggregate = "sum"

[[service.record]]
name = "effect"                    # local kind; the packed kind is (service hash, 1)
kind = 1
len = 0x1a4

[[export]]
name = "dk_fx_routes"              # an SDK import implemented here, or a service-local operation

[[hook]]                           # existing syntax: a ColdFire site resolved through the port file at load
name = "cf.page.hook.page_desc"
[[fill]]                           # existing syntax: a slot of the SHARC slot catalogue (section 9)
name = "dsp.fx.master"
```

```toml
# modules/fx/acme/chorus/module.toml: written by the packer from imports and init, not by hand
[[need]]
service = "fx"
min_version = "3.0.0"
imports = ["dk_module_ctx_add_fx", "dk_fx_routes"]
records = ["effect"]
resources = { "fx.effect_slots" = 1, "fx.fx_cost" = 1800 }
```

A service path in the selection is a pin (no new field; the selection's paths are shared by modules and pinned services).

## 6. Service contract

### 6.1 Rust side

A service is one owner object. The `service!` macro holds the one instance in a static cell and passes `&mut self`. Host tests construct it directly.

```rust
// runtime/core/abi/src/provider.rs
pub trait Provider<C: CoreApi>: Sized {
    /// Declarations the core checks against the packed header: resources with an "init" limit, entries.
    fn declare(&self, d: &mut Declarations) {}
    /// After placement and linking. Allocate state; no consumer is attached yet.
    fn start(core: &C) -> Result<Self, Status>;
    /// A consumer came up. Its records for this service arrive here.
    fn attach(&mut self, core: &C, consumer: &Consumer<'_>) -> Result<(), Status> { Ok(()) }
    /// A consumer goes down. Free everything it owns here: rows, objects, routes.
    fn detach(&mut self, core: &C, consumer: ConsumerId) {}
    /// Whether this service and its consumers may go down now (e.g. not while the transport plays).
    fn can_stop(&self, core: &C) -> Result<(), Status> { Ok(()) }
    /// Last call before the core restores entries and frees the package.
    fn stop(self, core: &C) {}
}

/// Optional parts. Each one is a separate tagged table in the packed row.
pub trait Records { fn validate(kind: u16, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)); }
pub trait DspPart<C: CoreApi> { fn admit(&mut self, core: &C, c: &Consumer<'_>) -> Result<(), Status>; fn release(&mut self, core: &C, who: ConsumerId); }
pub trait Ticker<C: CoreApi>  { fn tick(&mut self, core: &C, phase: Phase); }
pub trait Query               { fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> Status; }
pub trait Describe            { fn describe(&self, who: ConsumerId, out: &mut WordSink); }
```

`Records::validate` stays callable without a device, so `patchkit records --check` runs the same rule before load.

### 6.2 C ABI: tagged parts

```rust
#[repr(C)]
pub struct ProviderRow {
    pub abi: u32,                 // PROVIDER_ABI: changes only when this struct or PartRef changes
    pub name: u32,                // service name hash
    pub parts: *const PartRef,
    pub nparts: u16,
}

#[repr(C)]
pub struct PartRef {
    pub tag: u32,                 // PART_LIFECYCLE (required), PART_RECORDS, PART_DSP, PART_TICK, PART_QUERY, PART_DESCRIBE
    pub version: u16,             // per-part version: a new callback is a new version of one part
    pub table: *const (),         // the part's function table
}
```

The core finds a part by tag. A missing part means "not adopted". A part with a newer version than the core knows is refused at admission with a clear status. Adding a part or a part version changes no other service.

### 6.3 Service example

```rust
// services/fx/src/lib.rs: owner objects by concept
pub struct Fx { effects: EffectTable, routes: RouteRules }

impl<C: CoreApi> Provider<C> for Fx {
    fn start(core: &C) -> Result<Self, Status> {
        Ok(Fx { effects: EffectTable::new(core.profile().fx_slots), routes: RouteRules::default() })
    }
    fn attach(&mut self, core: &C, c: &Consumer<'_>) -> Result<(), Status> {
        for body in c.records(KIND_EFFECT) {
            self.effects.admit(core, c.id(), EffectRecord::parse(body)?)?;
        }
        Ok(())
    }
    fn detach(&mut self, core: &C, who: ConsumerId) {
        self.routes.forget(who);
        self.effects.remove_owner(core, who);
    }
    fn can_stop(&self, core: &C) -> Result<(), Status> { self.effects.refuse_while_routed(core) }
}

impl Query for Fx {
    fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> Status {
        match op {
            OP_ROUTE_CHOICES => self.routes.choices(&self.effects, req, reply),
            OP_LIST_EFFECTS  => self.effects.list(reply),      // replaces the dkrt_fx bridge
            _ => DKRT_E_MISSING,
        }
    }
}

service!(Fx, parts = [Records, DspPart, Query, Describe]);
```

### 6.4 Choosing an API

- **SDK import** (`sdk.toml`): a public contract the project keeps stable for all modules, e.g. `dk_module_ctx_add_fx`. The service implements it; the header ships in the SDK.
- **Service-local import** (`[[service_import]]`): an API defined in the service's own header, versioned with the service. No SDK edit.
- **Query**: a resident app asks a service in the UI task. The service owns operation numbers and layouts.

The core resolves all three by name hash against live services. None needs a core rebuild.

### 6.5 Records and resources

- A packed record carries `(service hash, local kind, length)`. The core routes each record to its service's `attach`; it holds no kind table. `sdk.toml` keeps only the typed builder declarations of SDK imports.
- A resource is `(owner, name hash, limit, aggregate, draws_from)`. The core's own resources are `core.modules`, `core.arena`, `core.stage`, `core.dsp_area`, `core.dsp_cost`, `core.store`, `core.settings`. Every other capacity is declared by its service.
- The planner checks fit from header tables only, before any code runs. A resource with a limit of `"init"` is checked at `start`.
- The DSP voice cost rule (`max(per-voice cost) * voices + sum(effect costs)`) becomes an aggregate: `machines.voice_cost` with `aggregate = "max"`, `scale = "profile:dsp_voices"` and `draws_from = "core.dsp_cost"`.
- The CAPACITY reply lists `(owner, name hash, limit, used, largest)`. Hosts take labels from the manifests.

### 6.6 Settings

A package can have settings: a module, and from RC3 a service. Settings are data, not menus: the owner describes its fields, the menu app renders them with the firmware's views, the core's settings broker stores the values (keyed by owner, with the project overlay), and the owner checks choices and values. No owner builds UI, so a package with settings stays reloadable.

```rust
// optional part of the provider contract (RC3)
pub trait Settings {
    /// The fields of the service itself, or the rows it adds to one consumer's page.
    fn fields(&self, scope: Scope, out: &mut FieldSink);   // Scope::Service | Scope::Consumer(id)
    /// Choices that depend on live state (e.g. which routes still have a free send slot).
    fn choices(&self, field: FieldId, scope: Scope, out: &mut ChoiceSink) -> Result<(), Status>;
    /// Whether a value may be set now; the broker stores it only when this says yes.
    fn validate(&self, field: FieldId, scope: Scope, value: u32) -> Result<(), Status>;
}
```

- **A service's own settings** (`Scope::Service`) show on the service's page in MODULES.
- **Settings a service owns about a consumer** (`Scope::Consumer`) show as a section on that module's page: the FX route of an effect is a row the FX service contributes and validates. The menu app's FX route panel and its old-picker fallback are deleted.
- **A module's own settings stay its own:** keys under its owner, today through its `menu` callback and the SDK get/put calls (unchanged). A module influences a service's rows through its records (e.g. the routes an effect supports), never by writing the service's keys, and reads a service's value through the service's API.
- **Module fields as data (RC3m):** SDK builder calls in `init` (e.g. `dk_setting_choice(ctx, "curve", "Depth curve", choices)`) that the packer writes as records, rendered by the same renderer. A module that declares fields instead of a `menu` is no longer held by the menu lifetime rule. `menu` stays supported.

## 7. Core owners

Each owner holds its own state, has a narrow interface and has a fake.

```rust
// runtime/core/lifecycle/src/graph.rs: pure, no I/O
pub struct PackageGraph { nodes: ArrayVec<Node, MAX_PACKAGES> }
pub struct Node { pub id: PackageId, pub provides: Option<Api>, pub needs: ArrayVec<Need, MAX_NEEDS>, pub pinned: bool }

impl PackageGraph {
    pub fn from_tables(tables: &[HeaderTable]) -> Result<Self, Refusal>;
    /// Services before their consumers; refuses an unmet or cyclic need.
    pub fn bring_up_order(&self, roots: &[PackageId], live: &LiveSet) -> Result<Steps, Refusal>;
    /// Consumers first, then the target, then services left with no consumer and no pin.
    pub fn take_down_order(&self, target: PackageId, live: &LiveSet) -> Steps;
    pub fn services_of(&self, p: PackageId) -> impl Iterator<Item = PackageId> + '_;
}
```

```rust
// runtime/core/fit/src/ledger.rs: pure
pub struct ResourceLedger { pools: ArrayVec<Pool, MAX_POOLS> }

impl ResourceLedger {
    pub fn declare(&mut self, owner: PackageId, r: &ResourceDecl) -> Result<(), Status>;
    pub fn fits(&self, wants: &[Want]) -> Result<(), Short>;          // the planner, before load
    pub fn claim(&mut self, consumer: PackageId, wants: &[Want]) -> Result<(), Short>;
    pub fn release(&mut self, consumer: PackageId);
    pub fn withdraw(&mut self, owner: PackageId) -> Result<(), Status>; // refused while claims remain
    pub fn report(&self, out: &mut CapacitySink);
}
```

```rust
// runtime/core/src/rs/src/install/mod.rs
pub trait Installer {
    fn install(&mut self, owner: PackageId, entries: &[Entry]) -> Result<(), Status>; // all or nothing
    fn restore(&mut self, owner: PackageId) -> Result<(), Status>;                     // E_BUSY: retry next pass
}
pub enum Entry { Hook { site: SiteRef, target: u32 }, Patch { site: SiteRef, bytes: PatchBytes }, Slot { slot: SlotRef, target: u32 } }

/// Routes each entry by the CPU the port data gives for its site.
pub struct Installers<CF: Installer, DSP: Installer> { pub coldfire: CF, pub sharc: DSP }

// install/coldfire.rs: check stock bytes, mask interrupts, write, flush. Over a CodeMemory trait.
pub struct ColdFireInstaller<M: CodeMemory> { mem: M, owned: EntryLog }
// install/sharc.rs: v1 fills catalogue slots through the link queue; RC-D v2 patches sites through the DSP-side installer.
pub struct SharcInstaller<L: LinkQueue> { link: L, owned: EntryLog }
```

Every firmware write a service makes goes through the installer, including table pointers and bounds (`Entry::Patch`), so `restore` returns the firmware to stock.

```rust
// runtime/core/lifecycle/src/manager.rs: the one lifecycle, generic over its seams
pub struct PackageManager<S: Store, P: Placer, L: Linker, I: Installer, H: ProviderHost> {
    graph: PackageGraph, ledger: ResourceLedger, live: LiveSet,
    store: S, placer: P, linker: L, installer: I, host: H,
}

impl<S: Store, P: Placer, L: Linker, I: Installer, H: ProviderHost> PackageManager<S, P, L, I, H> {
    pub fn boot(&mut self, selection: &Selection) -> BootReport;
    pub fn load(&mut self, id: PackageId) -> Result<(), Refusal>;
    pub fn unload(&mut self, id: PackageId) -> Result<(), Refusal>;
    pub fn reload(&mut self, id: PackageId) -> Result<(), Refusal>;   // a module or a service

    fn bring_up(&mut self, p: PackageId) -> Result<(), Refusal> {
        let at = self.placer.place(p)?;
        self.linker.link(p, at, &self.graph, &self.live)?;   // an import must resolve to a live service in p's needs
        let decl = self.host.start(p)?;                      // services only; checked against the header
        self.ledger.declare_all(p, &decl.resources)?;
        self.ledger.claim(p, &self.graph.wants_of(p))?;
        self.installer.install(p, &decl.entries)?;
        self.host.attach(p, &self.graph.services_of(p))?;    // one call: the host walks the records over all the services
        self.live.insert(p);
        Ok(())
    }
    // A failure at any step runs take_down for the steps already done.
    fn take_down(&mut self, p: PackageId) {
        for s in self.graph.services_of(p) { self.host.detach(s, p); }
        self.ledger.release(p);
        self.installer.restore(p).ok();                      // E_BUSY: the job retries next pass
        self.host.stop(p);
        self.linker.unbind(p);
        self.placer.free(p);
        self.live.remove(p);
    }
}
```

`ProviderHost` calls the parts of a service row. `FakeHost` records the calls, so a test asserts the order (detach before restore, restore before stop).

The link-time import list and the graph edges are the same data: the packer writes needs from the imports, and the linker refuses an import whose service is not in the package's needs. That removes the second dependency mechanism.

## 8. Lifecycle

### 8.1 States

```text
Absent -> Placed -> Linked -> Started -> Claimed -> Installed -> Activated -> Active
Active -> Detached -> Deactivated -> Released -> Restored -> Stopped -> Freed (Absent)
```

`Activated` makes the package public (a service publishes its exports and resident query entry; the tick and the record go live): it follows the install and precedes every attach, so nothing is visible before the package's entries are in firmware. `Deactivated` withdraws it before the entries are restored. A failure unwinds exactly the steps done. The DSP part of a package comes up over UI passes after `Installed`; a DSP failure takes down the closure of that package through the same path.

### 8.2 Boot

1. The platform calls `dkrt_boot` at the main-OS task entry (before the MainScreenView constructor; finding 158).
2. Read the selection; decide safe mode. In safe mode no card package loads and both installers stay disarmed; stock paths and the recovery UI run.
3. Read the header tables of the selected packages and of the services they need; build `PackageGraph`.
4. `fits` for all claims; refuse what does not fit (with the reason shown in Transfer and the menu).
5. `bring_up` in `bring_up_order`. Services and modules follow the same steps.

### 8.3 Run-time load, unload, reload

All in the UI task, queued as one job (`reload.rs` job steps CHECK, QUIESCE, UNLOAD, LOAD, DSP, DONE).

- **Load**: `bring_up_order(target)`: brings up any service it needs that is not live.
- **Unload**: `take_down_order(target)`: consumers first, then the target, then orphans. `can_stop` of every service in the closure is asked first.
- **Reload**: take down the closure, bring it up again (orphans excluded). Reloading a service reloads its consumers.
- **Refusals** (generic, in the core): safe mode, transport playing, upload open, card busy, a package in the closure has built menus (the menu lifetime rule; section 12), and each service's `can_stop`.

### 8.4 Calling contexts

| Context | What runs there |
| --- | --- |
| UI task | Lifecycle, `attach`/`detach`, `tick`, queries, settings changes |
| MIDI task | Protocol commands. Reads the module list through a snapshot the UI task publishes after each lifecycle step, never live tables |
| Interrupts, SHARC | Entry targets only. Read state that the UI task publishes; never call the core's lifecycle |

## 9. Entries into firmware and what stays resident

Finding 158: no firmware dispatcher can be reused, so every entry is a code patch or a flashed slot.

| Today | Entry after this plan | Owner after this plan |
| --- | --- | --- |
| `points/lfo` (12 ColdFire stubs) | Hooks installed at load | `services/lfo` |
| `points/machines`, `dynreg` (hooks, bounds, pointers, copied tables) | Hooks and patches at load; tables in arena buffers owned by the package | `services/machines`, `services/rows` |
| `points/params` | Row content written at load; the enlarged table stays one flashed reservation (110 absolute readers per device) | Reservation in `runtime/platform/rows`; claim policy in `services/rows` |
| `points/fxpages` | Already data plus service hooks | `services/fx` |
| `points/fx`, `dsp-voice`, `dynfx`, `runtime/platform/dyndsp` (SHARC) | Flashed numbered slots ("call slot N or run stock"), generated from port data, no service code; row capacity a platform reservation offered as resources | `runtime/platform/dsp-link` (slot catalogue); dispatch stays in `services/fx` and `services/machines` |
| `points/sample-pool` | DN2 boot path stays flashed | Transport in `runtime/platform/sample-transport`; policy in `services/samples` |
| `adapters/*` (USB, SD, card guard, +Drive) | Unchanged | `runtime/platform/*` |
| `fixes/introblank` | Unchanged | `runtime/platform/introblank` |
| `fixes/overdrive` | Deleted | |
| `proofs/*` | Test fixtures | The test project that uses each one |
| `adapters/peer-image` | Unchanged | `runtime/platform/peer-image` |

The platform stops naming services: `dsp-link` registers windows by number instead of naming `_dl_vrows`, `_dl_frows` and `_sp_tail`, `dyndsp` loses its `requires` on the points, and `platform.rs` gets the sample worker through the sample transport's interface.

A service needs a flash only for a new SHARC site that no slot covers. RC-D removes that case.

## 10. Tests

| Layer | Owns | Example |
| --- | --- | --- |
| Unit (host, most tests) | `PackageGraph`, `ResourceLedger`, `PackageManager` over fakes, each installer over a fake memory, each service over `FakeCore` | `unloading_last_consumer_unloads_unpinned_service`, `take_down_detaches_before_restore`, `claim_over_limit_is_refused` |
| Assembly (host, real packed files) | The seams: header tables to graph, import binding, part lookup, record routing, entry declarations against the port file. One fit and one refusal per seam | `ref_reload_rebinds_consumer_imports`, `part_with_newer_version_is_refused` |
| Device (DT2 and DN2) | Only what the firmware shows: entries back to stock after unload, audio and pages after a service reload | `service-reload.device.test.ts` (RC2), the existing tests of each moved service (RC3, RC4) |

```rust
#[test]
fn unloading_last_consumer_unloads_unpinned_service() {
    let g = graph![service("ref"), module("a", needs = "ref"), module("b", needs = "ref")];
    assert_eq!(g.take_down_order(id("a"), &live!["ref", "a", "b"]), steps!["a"]);
    assert_eq!(g.take_down_order(id("b"), &live!["ref", "b"]), steps!["b", "ref"]);
}

#[test]
fn take_down_detaches_before_restore() {
    let mut pm = manager_with_fakes(&[service("ref"), module("a", needs = "ref")]);
    pm.load(id("a")).unwrap();
    pm.unload(id("a")).unwrap();
    assert_eq!(pm.host().calls(), [Start("ref"), Attach("ref", "a"), Detach("ref", "a"), Stop("ref")]);
    assert!(pm.installer().log().restored_before(Stop("ref")));
}
```

**Reference fixtures.** One canonical fixture per contract feature (`tests/fixtures/modules`). RC1d extends the existing `local-ping` pair with what today's header table can express (finding: every header parser refuses an unknown tag, so a new tag needs a table version and a runtime minor; RC3 brings that version):

| Fixture | Exercises | Item |
| --- | --- | --- |
| `test/local-ping` | Service `local-ping`: one export, PROVIDES with its version | RC1d |
| `test/local-ping-user` | A consumer of the service (`min_version` 1.2.0) | RC1d |
| `test/local-ping-user-b` | A second consumer of the service: shared service, orphan unload | RC1d |
| `test/local-ping-old-user` | Needs a major version the provider does not have: the incompatible-need refusal | RC1d |
| the same fixtures | One resource `local-ping.tokens` (limit 2), one record kind owned by the service, attach/detach counters, a query, one ColdFire hook entry; a greedy consumer for the fit refusal | RC3 |

A newer minor version satisfying a need is a unit test of `from_tables`, because a second provider of one service cannot be packed beside the first (service files are named by service).

They replace `svc-test`, `svc-test-user` and `svc-echo` where coverage overlaps (RC3). Each deletion is mapped to the test that now covers it and reviewed before merge.

Development style: write the unit test first for each owner method; then the assembly case for the seam; device tests last and only for the rows above.

## 11. Size gate

Every item records, before and after:

- the stored container size against `container_max` for the `device` set of DT2 1.17, DN2 1.12 and Syntakt 1.42, and for `core/runtime` alone;
- the runtime object's code and data sizes, grouped by Rust module.

The tool is `patchkit size` (built first in RC1; section 14 has the baseline). "The runtime did not change" is shown with `patchkit objdiff` against the committed object, not with the text size: a change to the dependencies or files of a crate the runtime links (`dkrt-abi`, `dkrt-fit`) changes the crate hash in symbol names, relocations and rodata layout, so the committed objects are rebuilt and the image hash changes even when the code bytes do not (RC1d). An item that grows the core states why in its PR. Measured in RC2b-2: one lifecycle costs about 15-20 KB more than the old planner and loader paths it replaces, so the flashed runtime shrinks only when the lifecycle moves to the card (RC2s). The Syntakt fit has its own track (the 70 KB packing loss, finding 147).

## 12. Decisions

Taken (Em, 2026-11-22):

- `fixes/overdrive` is deleted.
- The selection may pin a service: a service path in the selection is a pin; the record gains no field (decided 2026-11-22).
- A `rows` service owns the claim policy for the reserved parameter, machine and page rows.
- The enlarged parameter table stays one flashed reservation, owned by the platform. It replaces today's copy in the `params` point and adds no bytes.
- Two installers behind one `Installer` interface: `ColdFireInstaller` in the core, `SharcInstaller` beside `dsp-link`.
- The DSP-side installer (RC-D) is deferred, and planned.
- One service ABI change per item, with all six services adapted in the same stack. No dual old and new path in the core.

Open:

- The menu lifetime rule (menus are never freed) blocks reload of a module with menus. `core/ui` needs a menu owner and a free path. Separate item after RC2.
- The interrupt level of vector 191 (LFO). If it is 7, masking does not keep it out during an install (RC4) [O].
- Whether `permit` (DT2 `0x400da514`) is read before the main-OS task entry (RC4) [O].
- `project.dks` has no consumer module: confirm it is pinned in every selection that uses projects (RC2).

## 13. Items

Each item is one PR or a small stack, lands green and merges before the next starts. Estimates are agent time.

| Item | What | Exit check | Size | Estimate, confidence |
| --- | --- | --- | --- | --- |
| **RC0 Plan and baseline** | This plan; size baseline and gate commands | Em's review | baseline | 0.5 day, high |
| **RC1 Contract and fixtures** | `patchkit size` (section 14); `Provider`, parts, `ProviderRow`, `Installer`, manifest schema (`[[service.resource]]`, `[[service.record]]`; entries keep the existing `[[hook]]` and `[[fill]]`) parsed and validated in patchkit, not packed; `PackageGraph` and `ResourceLedger` test-first, not wired; `PackageGraph::from_tables` over the packed `local-ping` fixtures | `core/runtime:rust-tests`, patchkit tests, fixture pack checks | no change in the image | 1-2 days, high |
| **RC1g Device test diet** | Inventory of every device test (what it proves, firmware-only or logic, lowest layer that sees it, minutes); a smoke set per device approved by Em, run only for changes to inputs every device test reads (runtime image, SDK, patchkit, platform), in place of every selected device test; a change local to one owner runs only that owner's device tests; from RC2 on, each item moves the device assertions of the area it touches to unit or assembly tests and deletes the redundant device tests (adversarial review, Em decides); Nx keys simplified once device tests are few. The gate target `gate-device-smoke` runs the 8 smoke tests on DT2 and DN2; AGENTS.md states the rule. | Inventory reviewed; smoke set approved | none | inventory 0.5 day, then per item |
| **RC2 One lifecycle** | `PackageManager` replaces the provider and module paths, `take_down`, `needs_hash` and `plan.rs` provider handling; replaces the loader's two inline version checks (loader/mod.rs, `[[service_import]]` and live plan) with `dkrt_abi::version::version_satisfies`; reload of services from the menu; services listed in MODULES; orphans; pins (decided 2026-11-22: no new field; a service path in the selection is a pin, sharing the selection's paths with modules); MIDI snapshot. Splits the device tests of ledger item RC2 (loading and lifecycle; 17 files, see [ledgers/device-test-inventory.md](ledgers/device-test-inventory.md)). | Unit and `core/runtime:test-assembly`; one device test per device: `core/runtime-tests:test-device--service-reload` | must shrink | 3-5 days, medium |
| **RC2s Resident core and system package** (design: [resident-core-and-system-package.md](resident-core-and-system-package.md); then RC2s-1 boundary in one binary, RC2s-2 package and stage 1, RC2s-3 menu) | What stays resident (reset installer, platform hooks, boot counter, safe mode and recovery screen, store and Transfer, a stage-1 loader) and what moves to a card system package (package manager, graph, ledger, the runtime's seams, planner, reload job, service registration, settings overlay, full loader, the menu app); the stage-1 format, the resident function table and its versioning, the recovery path when the system package is missing or broken, how apps load; measured sizes per part. Re-cuts RC2c-RC6 | Em approves the design | measured | 1 day design, medium |
| **RC-I Lifecycle instrumentation** (after RC2s) | A trace seam in the package manager (state changes, refusals with reasons, attach and restore, DSP phases, time per step), free when nobody listens (one flag check, rule 5); events through the existing log and span sink channels; the debug module subscribes when loaded or when profiling is on; dkprof and the Profile pane show a lifecycle timeline; folds in the consistent emit points of the instrumentation audit | unit tests over a fake sink; one device test with the debug module | small | 1-2 days, medium |
| **RC2t Test dedupe** (after RC2s) | Where one behaviour is asserted at several layers (lifecycle unit tests, runtime seam tests, assembly tests; the RC2b and RC4-lfo assembly refusal cases), keep the lowest owning test and one seam case per seam; each deletion through the adversarial review | test counts and runtimes before and after | none | 0.5 day |
| **RC3 Service contract v2** | Owner-object services; tagged parts; provider-local record kinds and resources; `Describe` and query replace both bridges; `svc_rows` fallback, `[records.kinds]` owners and domain capacity ids deleted; header table version 4 with the new schema packed (provider-qualified resources with aggregate and parent, provider-local record kinds, entries); `.dkm` record format version bump; the fixtures gain resources, records, a query, a hook and one settings field of each scope; the `Settings` part (6.6) with FX's route choice moved to it and the menu's FX route panel deleted; all 6 services and content modules repacked. Splits the device tests of ledger item RC3 (FX, services, settings; 16 files, see the ledger). The hook table allows the same hook name once per disjoint device set (packer, loader lookup, service check, `service!` macro), and the device-suffixed lfo hook names of RC4-lfo (`cf.lfo.hook.*_dt2`, `*_dn2`) return to plain names. Format proposal: [rc3-header-table-v4.md](rc3-header-table-v4.md). | Service unit tests, assembly, the device tests `test:plan` selects for the services | must shrink | 4-6 days, medium |
| **RC3m Module settings schema** | SDK builder calls for module settings fields, packed as records and rendered by the RC3 renderer; values keep the SDK get/put calls; `menu` stays supported | SDK header tests, assembly (packed fields reach the renderer), one menu device test per device | small | 1-2 days, medium |
| **RC4 Installers and ColdFire points** | `Installers` with both backends; `lfo` first (deletes `points/lfo`), then `machines`, `dynreg`, `params` content, `fxpages` into their services and `services/rows`. Splits the LFO, machine and row device tests (ledger item RC4). | Backend unit tests; the existing device tests of each service | the image shrinks | 3-5 days, medium (lfo 1 day, high) |
| **RC5 Remove image-modules and tidy runtime/core** | SHARC slot catalogue and drivers to `runtime/platform`; platform stops naming services; proofs to tests; `overdrive` deleted; the crates of `runtime/core` move to `runtime/core/crates/<name>` (the runtime crate from `src/rs` to `crates/dkrt`, assembly from `src/cf` to `asm/`); the runtime crate's remaining flat files grouped into concept folders. Splits the platform, storage, +Drive, upload and fault device tests (ledger item RC5). The remaining large resident files are split by concept (`storage/store_writer.rs` 1,577 lines, `io.rs`, `files.rs`, `settings.rs`, `boot.rs`). | Byte manifest for pure moves; named tests for the de-naming | neutral or smaller | 1-2 days, high |
| **RC6 Runtime size** (folded into RC2s) | The menu app and the lifecycle move to the card system package (RC2s). Transfer and recovery stay resident | Per moved part | Syntakt fits | sized after RC5 |
| **RC-D DSP-side installer** (deferred) | A resident DSP installer that patches sites at block tail; `SharcInstaller` v2; SHARC slots leave the image | Static design, emulator, one hardware test | the image shrinks | 2-4 days, low-medium |

Before RC2 and RC3 start: state payoff and confidence again with the RC1 results, and stop if they are weak.

### Status (for a restart)

Update this block whenever the state changes; keep it a current summary, not a log. Plan file: `/Users/em/src/digi/lanes/fx-route-policy/docs/plans/runtime-contract-and-lifecycle.md` (lane A worktree).

- **RESTART HERE (2026-11-23, evening; read this first after a context reset):**
  - **Lanes and branches (all local, nothing of RC2s pushed):** lane A `/Users/em/src/digi/lanes/fx-route-policy`: `rc/rc2s-1` (RC2s-0 + RC2s-1 complete, tip `5969fade2`) and `rc/rc2s-2c` (step c, tip with plan commits after `1a999c433`; plan file lives here). Lane `lfo-shapes`: `rc/rc2s-2` (join a `ff96a5535` + step b `3e1af5da5`, `c23f7d2f4` + objects; b fix round in progress). Lane `nx-test-keys`: RC2t stack `rc/rc2t-refusal` (`8be64e883`) → `rc/rc2t-store` (`4f1217f96`) → `rc/rc2t-settings` (`91edc8968`) → `rc/rc2t-load` (area 4 in progress), all based on rc/rc2s-1. Main is `0172a84a2` (#699-#708 merged).
  - **In flight:** RC2s-2 (b) fix round (lfo-shapes): core-only installs no package hooks (check init_once order vs boot::run; set_safe must not go through the package), core-only stops counting (saturating counter), the package-write clear moves to a level-owned hook called BEFORE the commit (store reports "package path written", level decides; keep LATE_FLOOR), one-shot flag consumed when core-only wins, tests for Rename/InsertMany/RETRY through boot::run, full-tier safe-mode and crash-capture runs (expectations edited unrun). RC2t area 4 (module-load and commit) on nx-test-keys. After each: a fresh reviewer sub-agent (adversarial when tests are deleted), then commit.
  - **Next (in order):** (1) combine `rc/rc2s-2` (after the b fix round's review) with `rc/rc2s-2c` into one branch. (2) RC2s-2 step (d): stage 1 loads `digikit/system.dksp` into `core.package` at the arena top (Placer over the arena; capacity reports it) and replaces `hooks::attach_package` in `init_once`; remove the `link-package` switch (keep the test-only attach for host tests); stage 1 runs `dkrt_abi::resident::check` before calling `dkpkg_entry` and logs the refusal reason, null-table check; `SAFE_CORE_ONLY` reason plus a cause field (crashes, missing, incompatible, damaged, unreadable) in abi.toml; an append-safety test for the new LinkRows group (1.0 package on a 1.1 table accepted, 1.1 package on a 1.0 core refused); a symbol-size listing to explain the 193 KB package vs the 112 KB image saving (duplicated SDK/abi/builtin code?); a relocation scan test (no absolute in-image address without a relocation entry); `patchkit size` for DT2, DN2, Syntakt (expect Syntakt to fit); boot-restart and safe-mode device checks. (3) step (e): card installation of system.dksp in tests/regression/runtime/card-kit.ts buildCard (also the folder branch) and card-cache.ts cardKey; device tests: extend safe-mode, a new core-only recovery test (package removed → core-only screen and status → Transfer upload → restart → normal). (4) RC2s-3: `ui.settings_rows` hook + shim.s calls it; the C menu into the package artifact; menu device tests; Syntakt size. (5) RC2t area 4 review and done. (6) RC2s milestone gate: `DIGI_GATE_BASE=origin/main mise run test:changed` first, then one device gate (smoke set + boot, safe-mode, menu, refusal, store, settings, lfo tests on DT2 and DN2, halves in parallel), post-landing reviewer on the combined diff, then ask Em to push/PR/merge the stack (RC2s-0/1/2/3 + RC2t) and do the one ABI bump.
  - **Open follow-ups:** other devices' port files are still inputs of a device half (merge and narrow keys); the record `dsp` field (DIGI_EMU_DSP budget) is not a key input; the test-source guard does not look inside nested tests/ dirs; cfg(test) modules with other names over-select; stale PGO profiles; dkpkg_entry returns null without a reason (fixed in d).
- **Merged on main (`b348fd19b`):** #692-#697 (RC1, RC1g, RC2a, RC2b), #699 fixture pack targets, #702 live sessions start on a card with a short stable period (`stable_blocks` 1500; opt-outs `stableBlocks: 0`, `blankCard: true`), #703 RC4-lfo (lfo hooks installed at load, `points/lfo` deleted), #704 three FX device tests merged into fx-contract, #700 Rust test sources out of selection and build keys (one definition `config/test-source-patterns.json`, guard `workspace:test-source`). The stack #702-#704 passed one device gate (smoke 16 halves, lfo files, fx-contract, fx-routes-panel, card-safety; 41 min, halves in parallel).
- **Working rules (Em):** each step updates ARCHITECTURE.md, keeps one owning test per behaviour at the lowest layer, runs the static checks and `test:plan`; a reviewer sub-agent after every step, fix round and PR (adversarial when tests are deleted), and a post-landing reviewer on the combined diff after each chunk merges; a step is committed after its review findings are fixed; Em reviews per milestone and decides open questions; pushes, PRs and merges need Em's OK; pass or fail only from test results. Device tests once per milestone plus a quick device check per step.
- **Speed-ups (Em):** stacked device gates for queued branches, with the affected host targets (`mise run test:changed` from the stack base) run first; DT2 and DN2 halves in parallel; behaviour-neutral steps proven by the boot-effects snapshot and `patchkit objdiff`; a scout before a worker whose brief rests on an unverified assumption; `dk:dev` for device iteration; at most three lanes, 2 builds at a time.
- **Load discipline (2026-11-23):** RC2t area 4's device run took 54 min at load 70 (cap: load under 12): parallel builds, two or three device runs and a profiler at once slowed every emulator several times over. From now on: one Nx device invocation at a time (two in parallel also collide on `patchkit:device-images`), at most two cargo builds, check `uptime` before starting a device run; read-only scouts and reviewers may overlap.
- **First-pass checklist (Em, 2026-11-23), in every worker brief; the worker checks it before handing back, the reviewer still reviews every step and fix round:** (1) every new or moved test has been seen to fail: by writing it first (TDD), or, for code that already exists, by temporarily breaking the code (report each break); (2) no device literals, devices from the registry or capabilities, the device-literals baseline only goes down; (3) tests drive the real entry points (`boot::run`, the real reload `Job`, production initial values), fakes only for hardware, no hand-copied production sequences; (4) objdiff and size: every differing function explained, size before and after; (5) after any move, search the tree for the old path and update docs, tables and comments; (6) for each rule one accept and one refuse case plus the design's edge cases; (7) a change to build inputs runs that build (`verify_build daemons`, patchkit), and `DIGI_GATE_BASE=<base> mise run test:changed` before a merge; (8) globals in tests go through one owner with a guard that restores on unwind (TestArena, scoped `with_*`). Fix rounds address every finding at once; the delta review is full when fixes touch production logic, focused when they touch only tests or docs.
- **Design checklist (Em), in every worker and reviewer brief:** separation of concerns (device facts only from device data behind capabilities); DRY (one definition; other copies derived or checked by a test); owner objects with private state and lifecycle; files named after the noun they own, methods after the verb; no utils/helpers files; language conventions.
- **Decisions:** pins are service paths in the selection; a module's record follows its selection index; lfo hooks carry `_dt2`/`_dn2` until RC3 (RC3 removes them and lowers the device-literals baseline); RC2s design approved (normal; safe = system package and menu, no modules; core-only fallback; DSP watch resident; package at the top of the arena); RC3 header table v4 decisions taken; RC2s-1 adds the typed address seam (`DevAddr`), RC3 moves the services' raw reads behind it; RC2t moves tests down assertion by assertion per area (bulk deletion list refuted); RC-K device and conformance test keys hold only the artifacts they load.
- **Active now (2026-11-23, later):**
  - RC2s-1 complete at `5969fade2` (S7c versioned resident table: header {magic, major, minor, size} plus one pointer per group, each group with its size in bytes; `dkrt_abi::resident::check` refuses and logs, never panics; `PackageState` view with the arena words opaque; append-safety and per-group parity tests; all steps reviewed).
  - RC2s-2: (a) join on `rc/rc2s-2` (`ff96a5535`) done; (b) counter rules and `level::decide` in boot (`3e1af5da5`, `c23f7d2f4`; RETRY, package-write clear) reviewed, fix round running (core-only installs no package hooks and stops counting; the package-write clear moves to a level-owned hook called before the commit; one-shot flag consumed; rename/InsertMany/RETRY tests; full-tier safe-mode and crash-capture runs); (c) the package as its own artifact on `rc/rc2s-2c` (`9fe4635ae`..`1a999c433`): a second object of the runtime crate (`[system_package]`, `--features package-image`) packed to `dist/syspkg/runtime/system.dksp` (192,856 B, resident table 1.1 with a new LinkRows group for 45 optional dk_*/dl_* symbols), entry `dkpkg_entry`; a temporary `link-package` switch keeps it linked until (d). With the switch off: resident text 260,756 → 76,168 B; stored image DT2 1,493,552 and DN2 1,898,976 (−108 KB vs main), Syntakt 2,048,448 (fits container_max 2,096,640 with 48 KB spare). Review running. Then (d) stage 1 loads system.dksp at the arena top (`core.package`), the switch goes, `dkpkg_entry` refusals get a logged reason, SAFE_CORE_ONLY with its cause field; (e) card installation and device tests (core-only recovery).
  - RC2s-3 scouted (about 1 day of planner time, less at the measured pace): `ui.settings_rows` hook with a null default and the UI shim (runtime/platform/ui/src/shim.s) calling it instead of the link-time `dk_app_settings_rows`; the C menu (runtime/apps/digikit-menu/src/menu.c) compiled into the package artifact (step c must link a C object), its remaining `dkrt_*` calls through table entries; menu device tests and core-only recovery on DT2/DN2. Size: the Syntakt image is now 2,164,416 B stored vs 2,096,640 (~67.8 KB over); the menu alone (19 KB stored) does not fit it; moving package code out in step (c) must; re-measure each step. DN2 headroom ~39.7 KB.
  - RC2t (moves down) on `nx-test-keys`: area 1 refusal (`8be64e883`), area 2 card-safety and store-format (`ff0111864`, `4f1217f96`), area 3 settings (`rc/rc2t-settings` tip `91edc8968`) done and adversarially reviewed; area 4 module-load and commit running on `rc/rc2t-load`. Each move: lower test first with a break-it proof; device tests keep only firmware and real-card facts.
- **Decided by Em (2026-11-23, later):** core-only by crashes needs no stored hash: a core-only boot does not clear the boot counter (it runs no package code), so every boot stays core-only while the counter is at or above CORE_ONLY_AFTER; writing `digikit/system.dksp` through the resident store clears it, and a RETRY command clears it without an upload; re-uploading the same broken package gives one more bounded round (4 boots). The resident store and transport stay usable in core-only (host sees status and boot info, can list, upload or remove the package); the device shows the safe-screen reason frame, then stock firmware without the DIGIKIT menu. No selection-record format change. (Replaces "until the package hash changes".) Also decided (Em, 2026-11-23): RETRY is transport command 0x0b (fixture reservation narrowed to 0x0c-0x0f in sdk/sdk.toml; documented in docs/design/digikit-upload.md); core-only reports a proper `SAFE_CORE_ONLY` reason plus a cause field (crashes, missing, incompatible, damaged, unreadable) in abi.toml, shipped in the one RC2s ABI bump (implemented in RC2s-2 step d).
- **Decided by Em (2026-11-23):** (1) service ABI word: S3's type-only edit of ABI-hashed files accepted inside RC2s; one deliberate ABI bump when RC2s merges; later steps avoid ABI files without reason. (2) Boot wiring rules: a safe boot caused by the counter counts itself, key/flag safe boots do not; a safe boot clears the counter only after the stable period; core-only by crashes lasts until the package hash changes (Transfer upload); the safe key cannot override core-only by crashes; header errors: bad magic/format → incompatible, bad hash → damaged, both core-only without a crash count; package file `digikit/system.dksp`; an unreadable card gives core-only for that boot, no crash count, retried next boot (`PackageStatus::Unreadable`). (3) RC2t starts right after S7b's review, beside the RC2s-2 join: refusal codes → assembly/unit; card-safety and store-format → store and fake-platform tests; settings copies/cards → settings.rs units; module-load and commit → the snapshot; each move writes the lower test first, shows it fails when the behaviour breaks, then removes the device assertion, adversarial review. (4) Design checklist gains: a behaviour is tested at the lowest layer that sees it (unit with fakes for logic, assembly for a seam with one fit and one refusal, device only for firmware or hardware facts); reviewers flag device tests that check logic and repeated assertions.
- **Merged since:** #705 fix-forward (daemon key, stale patchkit tests), #706 RC-K test keys from artifacts, #707 key gaps (pack sidecars, device/port data, patched-dsp-startup sources), #708 reload-menus waits and three `dsp` records. Follow-ups: other devices' port files are still inputs of a device half; the record `dsp` field (emulator budget) is not a key input.
- **Next:** RC2s-1 S1 (`DevAddr`) in lane A and S4 (loader split) in lane `nx-test-keys`, then S2, S3, S5-S7, S8 in its own lane; RC2s-2 host parts A-D in a third lane (A `level.rs`, B package header, C `stage1.rs`, D patchkit `syspkg`), joined after S7; then RC2s-3 and one RC2s milestone device gate. Host-only infra when a lane is free: RC-K, then the device-test speed PR (`fault-trace-dt2` timing, `reload-menus` drained taps, four `dsp` records; needs device runs). Target: about 3-4 days to the RC2s gate.
- **Known failures on main (`6458690f8`):** the Syntakt container-overflow patchkit tests (`card_guard`, `repack`; wait on RC2s). #705 fixed the daemon key (#700 made patchkit compile in `config/test-source-patterns.json`) and three patchkit tests #703 left stale. Lesson: a change to build inputs runs the build itself (`verify_build daemons`) in its proof.
- **Open follow-ups:** the test-source guard does not look inside nested `tests/` dirs; `#[cfg(test)]` modules with other names (`test_lock.rs`, `host_io.rs`) still over-select; stale PGO profiles (warning); the check that fails on a `dependsOn` naming a missing target.
- **Size:** runtime text after #697 about 252,900 (+41 KB), accepted until RC2s moves the lifecycle to the card; the Syntakt fit waits for RC2s (finding 159). `patchkit size` compiles device data in: measure a branch with a binary built from it.

### Progress

Status: `todo`, `active`, `review`, `done`. Each item and step ends the same way:

1. The worker updates docs/ARCHITECTURE.md (and its inventories) for what the step changes. Device tests run once per milestone, not per step: a milestone (RC2s-0 to RC2s-3; RC2c to RC2e; RC3 and RC3m; the rest of RC4; RC5) is a stack of reviewed commits that merges together after one device gate (the smoke set when a shared input changed, plus the device tests of the firmware paths it changed). Each step adds one quick device check of about 5-10 minutes (one `boot-restart` run on DN2, or `dk:dev` with one module). Small independent PRs keep their own narrow device proof. Where possible a step is either a byte-identical move (proved with `patchkit objdiff`; the image hash and so the device-test keys stay the same, and Nx replays device results from cache) or a behaviour change (proved by the boot-effects snapshot and, for firmware paths only, device tests). Each behaviour has one owning test at the lowest layer that sees it; an assembly test proves that the real built files reach the logic (one case that fits and one refusal per seam), not every refusal code. The worker runs the static checks (`pnpm nx:pilot run-many -t lint,fmt-check,rust-fmt,typecheck,python-checks`), leaves its changes uncommitted and marks the row `review`.
2. The worker runs `mise run test:plan` with `DIGI_GATE_BASE` at the start of the phase and states whether the selection matches what the change can reach; a selection that is wider is fixed in the inputs and edges, not run. An independent reviewer sub-agent reviews the diff and the selection; it flags a test that repeats an assertion a lower layer already owns, and lost coverage only where no lower layer covers it (ranked findings with `file:line` and a failure scenario; boundaries, separation of concerns and device specifics behind capabilities included). Findings are fixed or answered.
3. Em reviews the phase or deliverable. Only then is it committed and the row set to `done` with the commit.

| Item or step | Status | Commits, PR |
| --- | --- | --- |
| RC0 Plan and baseline | done | `57b861aaa` (finding 158), `3d6a6bc49` (plan, baseline); approved by Em 2026-11-22 |
| RC1 Contract and fixtures | done, merged in PR #692 (`82e5f39ee`) | lane `fx-route-policy`, `dd211d711`..`a4d2ecfcb` |
| RC1a `patchkit size`: stored container per device, runtime with its `requires`, drop-one per module, runtime groups, compare with a saved baseline | done | `dd211d711` |
| RC1b crate `dkrt-lifecycle` (`runtime/core/lifecycle`): `PackageGraph` and `ResourceLedger`, test-first, pure, not a runtime dependency yet | done | `65bb121db` |
| RC1c contract types: `Provider`, parts, `ProviderRow`, `PartRef` (a new `dkrt-abi` file outside `SVC_ABI_FILES`); `Installer`, `Entry` (in `dkrt-lifecycle`) | done | `4b48e0de9` |
| RC1f rename `dkrt-packages` to `dkrt-lifecycle` (`runtime/core/lifecycle`): the crate holds the lifecycle logic, RC2's `PackageManager` included | done | `1066f6351` |
| RC1d `PackageGraph::from_tables` over the existing header table (NEEDS, PROVIDES; shared `dkneeds` parser); `[[service.resource]]` and `[[service.record]]` parsed and validated in patchkit, not packed (packing is RC3; entries keep `[[hook]]` and `[[fill]]`); the `local-ping` fixtures extended (second consumer, incompatible consumer); one host test that packs them and checks bring-up and take-down orders (a newer provider minor is a `from_tables` unit test) | done | `a4d2ecfcb` (objects refreshed in `890092c7d`) |
| RC1e selection check: RC1's host-only changes select device tests (runtime `Cargo.lock`), every Rust target (workspace `Cargo.lock`) and every service (any `dkrt-abi` file); narrow the edges that are wrong so `test:plan` shows what the change can reach | done | `b62328998` |
| RC1g Device test diet: inventory and smoke set | done | PR #693 (`125419d11`) |
| RC2 One lifecycle | active: RC2a and RC2b merged; RC2s next | lane `fx-route-policy` |
| RC2a `PackageManager` in `dkrt-lifecycle` over seam traits, test-first with fakes; ledger bound check against `REC_MAX` | done | PR #695 (`132db2ece`), merged as `3a27bd7dc` |
| RC2b-1 the runtime's real seams (placer, linker, host, installer, manifest) over the existing loader code, unit-tested over FakePlatform/FakeStorage and assembly-tested with packed fixtures; boot unchanged | done | committed on `rc/rc2b-runtime-seams` (merged with RC2b-3) |
| RC2b-2 boot and the reload job go through `PackageManager`; the DSP part as a seam step | done | committed on `rc/rc2b-runtime-seams` (merged with RC2b-3) |
| RC2b-2s stack and regressions: build `Rt` in place (boot frame 60 KB to about 7 KB), attach in callback order, a busy module keeps its tick on unload, `live_plan` and a refused-then-unloaded service | done | committed on `rc/rc2b-runtime-seams` (merged with RC2b-3) |
| RC2b-3a the graph replaces `plan.rs` in boot and the live-load checks; old loader paths deleted; a record whose owner is not among the module's needs is refused at load; `last_name` cleared; the attach test against the full walk table | done | committed on `rc/rc2b-runtime-seams` (merged with RC2b-3b) |
| RC2b-3b RC2 device tests split (ledger item RC2); main merged in; device proof on DT2 and DN2 (smoke set, remaining loading tests, the provider-event tests); the RC2b PR | done | PR #697, merged as `fc23903c5` |
| RC2s Resident core and system package: design | done (approved by Em 2026-11-22) | [resident-core-and-system-package.md](resident-core-and-system-package.md) |
| RC2s-0 boot-effects snapshot: one host test per device runs a standard selection through boot, a live load, a reload and an unload over FakePlatform and the real packed files, and records the effects (installer writes and restores, registry rows in order, arena layout, module states and reasons, selection and settings records) into a committed text snapshot; a behaviour-neutral step must leave it unchanged, a behaviour change shows its difference for Em to approve | active: scout | lane `fx-route-policy` |
| RC2s-1 boundary in one binary (resident table, hooks, layer rule; plus the typed address seam decided 2026-11-22: `DevAddr(u32)`, one owner of device memory access, arena hands out device addresses, device build unchanged code, host build translates, then the boot-effects snapshot drops its child process); `loader/mod.rs` (1,781 lines), `reload.rs` (1,372) and their test files split by concept as they move behind the boundary | todo | |
| RC2s-1 sub-steps (planned 2026-11-22; 5-6.5 agent-days, about 4 days wall with parallel lanes; confidence ~80% S1-S5, ~65% S6-S7). S1 `DevAddr(u32)` in `runtime/core/abi/src/dev_addr.rs` (`#[repr(transparent)]`; `ptr::<T>()` so callers keep their access kind), `mem::read32/write32/call` as the one access path that `rd/wr/rd16/wr16/call` and the `FwMem` defaults delegate to; host address map in `fake_platform/address_map.rs`; objdiff identical for the runtime and every service object. S2 arena hands out `DevAddr`, host-arena workaround deleted; every host test (or FakePlatform::new) installs an address map, so S3 can turn the host's "no map installed → plain cast" arms in dev_addr.rs into panics (objdiff + snapshot). S3 `ctx_addr` and `fn as u32` sites through `DevAddr`, callers move from `mem::call(u32)` to `call_at(DevAddr)` and `call` goes, the no-map fallback arms panic; the snapshot drops its child process only if 60 in-process runs match, else it stays with both causes written (narrowing; service statics not reset, RC3) (objdiff + snapshot). S4 split `loader/mod.rs`, `reload.rs` and tests by concept (objdiff; parallel with S1-S2). S5 split mixed files, pure helpers to abi, package files into `package/` (snapshot: call order moves). S6 typed `PackageHooks`, 28 call sites (objdiff shows only hook calls; snapshot). S7 `ResidentTable` by owner, `package/resident.rs`, `PackagePlatform` as a narrow trait of the 17 methods the package uses, import rows (snapshot, `patchkit size`, boot stack frame 6,740 B). S8 enforced `check:layers` boundary rule, ARCHITECTURE.md (own lane after S5). Placement decided by Em 2026-11-22: io, worker, aux_task, intro, speed and sha256 resident (sha256 offered to the package through the table; stage 1 needs it); dsp_stats resident if the DSP hang watch reads it, else package. Each sub-step: reviewer sub-agent, commit; Em per milestone | todo: S1 and S4 start when RC2s-0 is committed | |
| RC2s-2 system package artifact, stage 1, the three levels. Decided 2026-11-22 (Em): its host parts start in a third lane in parallel with RC2s-1 once RC2s-0 is committed (stage-1 loader over FakeStorage/FakePlatform with unit tests for good package, bad hash, incompatible version, missing file; the level decision with both counter thresholds, the safe key and a DSP hang; the package as its own artifact; card preparation installing it); linking joins after RC2s-1 S7. Lanes: A = RC2s-1 S1-S3, S5-S7; lane 2 = S4 then S8; lane 3 = RC2s-2 host parts; at most three lanes (2 builds at a time). Target: about 3-4 days to the RC2s milestone gate | todo: host parts sized (scout 2026-11-22, ~10-14 agent hours): A `level.rs`, a pure `decide(inputs) -> Level{Normal, Safe(reason), CoreOnly(reason)}` (today an if-chain in `boot.rs:149-165`; `SAFE_AFTER` from abi.toml), plus the second counter threshold as an abi.toml constant; B the package header and version check as one shared type (sdk or dkrt_abi) used by runtime and patchkit; C resident `stage1.rs` over `store::find`, streaming `sha256` and FakeStorage, placement behind a `Placer` trait (good, bad hash, incompatible, missing without crash count); D patchkit `syspkg.rs` pack plus a fixture package and an assembly case. Join after S7: Placer to the arena top (`core.package`), the package entry gets the resident table, A wired into `boot.rs`, card preparation (`card-kit.ts buildCard`, `card-rig.ts`, `cardKey`; changes every device key, last), device tests. Lane 3 avoids loader/mod.rs, reload.rs, arena.rs, mem.rs, entry.rs and the check:layers rules | |
| RC2s-3 the menu moves into the system package | todo | |
| Syntakt fit: deferred to RC2s. Finding 159: the stored size of MAIN is bounded by its unpacked size (in-place unpack), so each flashed byte removed saves one stored byte; RC2s moves about 167 KB to the card. Fallback: lower `main_margin_min` (about 7.9 KB) after the depacker's read loop is checked a second time and `depack_check` passes | deferred to RC2s | finding 159 |
| RC2t Test dedupe | re-scoped 2026-11-22: moves down by assertion per area after RC2s-0; device-test speed PR first | |
| RC-K Test keys from artifacts (decided by Em 2026-11-22; next infra item after the selection fix `fix/device-keys-skip-host-tests`): builds key on sources, tests key on what they run. Every device and conformance test target keys only on the artifacts it loads (patched image hashes `dist/bytes/images-<device>.sha256`, the `.dkm`/`.dks` packs it loads), its own test files and setup, and the driver subject files (`DRIVER_SUBJECT_FILES`): the `mergeTests` form for all; no source-file inputs. `affected`, `test:plan` and `test:subset` stay the coarse source-based list; a selected test whose artifacts are byte-identical replays from cache. Then the device-test part of the selection fix (`production`/`subjectInputs` test-source exclusions) is deleted; the test-source patterns stay for build keys (daemon key, cargo globs, patchkit `source_sha256`). Scout (2026-11-22): both non-full keys still add `production`, imports and `sources` (`tools/nx/device-test-keys.mjs:348-351`); the merge key (13 `mergeTests` files) subtracts built dirs, the narrow key (~85 files) keeps all of `production` and holds no image hash or daemon key except for image-module subjects; conformance keys hold the module's own non-built sources. Smallest change: the narrow key takes `MERGE_BYTES` (image hashes, daemon key via `forDevice`) and `builtSourceExclusions` in place of `production`; tests that read source files at run time list them in `sources`. Proof: a host-test edit and a comment edit in a runtime source both replay every device test from cache, a code change in the runtime does not | todo | |
| RC-I Lifecycle instrumentation | todo (after RC2s) | |
| Canonical service layout (decided by Em 2026-11-23; every service PR must meet it): `services/<name>/` with `Cargo.toml` (crate `dkrt-svc-<name>`, matching the folder; `rows` is renamed from `dkrt-svc-params`), `README.md`; `src/lib.rs` module docs, mod lines, pub use only; `src/<name>.rs` the owner object (Fx, Lfo, Machines, Project, Samples, Shortcuts) with all state private in it and its `Provider` impl (start/attach/detach/can_stop/stop together in one impl); `src/<concept>.rs` one noun per file, each concept owning its logic; `src/records.rs` the record kinds and `Records::validate` (split to `records/<kind>.rs` only if it grows); a part's impl in its own file only when substantial and its own concept (`queries.rs`, `settings.rs`, `dsp.rs`), a small part beside the Provider impl; host tests beside code (`<file>/tests.rs`, `tests/`) over a fake CoreApi; firmware crate `dks/` with `module.toml`, `src/{lib.rs,hooks.rs,hooks_<dev>.s,gen_exports.rs}`; few device tests in `tests/device/`. The `service!` macro holds the one owner instance; hooks reach state through it, no loose `static mut`. Split a file by behaviour into a folder of impl blocks only past ~400 lines or two concepts (AGENTS.md). Scout (2026-11-23) found: owner in `records.rs` for fx/lfo/samples/shortcuts, `ProjectSvc` in project lib.rs, mixed owner names, `static mut` state almost everywhere (hence test locks), lfo `hooks-ui.s` vs `hooks.rs`, project `engine/` and in-crate `dkp/tests.rs`, fx `fxpage.rs`/`fxsend.rs` and machines `slots.rs`/`build.rs` oversized | decided | |
| RC3 order (decided by Em 2026-11-23): (1) shared work once: header table v4 packed by patchkit and read by the runtime ([[service.resource]]/[[service.record]]); PackageManager part lookup and admission by tag and version; `service!` emits ProviderRow and PartRef; the services' raw device-memory reads (~64 sites, 17 files) through FwMem/DevAddr; the fake CoreApi and the contract harness (test-support contract.rs, `Service`-typed today) take a Provider. (2) Template service: shortcuts (smallest, has a device test): canonical layout + contract v2 in one PR. (3) Parallel lanes, one PR per service, layout + contract together: samples; lfo (drops `_dt2`/`_dn2`, hooks chosen by device capability, device-literals baseline back down); project (Settings candidate); fx (`dkrt_fx` bridge → Query). (4) machines last (slot fills, voice-cost aggregate), together with RC4's machines point. Logic to host tests over the fake CoreApi (record validation, admit/detach ownership, Query ops, resource limits); device tests only for page hooks, DSP fills, lfo tick, key events. Estimate at the measured pace: ~0.5 day shared, ~1 day lanes | todo | |
| RC4 order (decided by Em 2026-11-23): fxpages → fx service (data table only; its hooks already in fx.dks), first, parallel with machines; machines point → machines service as hooks installed at load like lfo, with machines' RC3 lane (same files); dynreg → rows service, hooks at load, after machines; params split: the flashed table reservation stays resident (runtime/platform/rows), the rows service writes the content at load, after dynreg (hardest; check whether params can leave flash at all). SHARC points (fx, dynfx, dsp-voice, dyndsp) and sample-pool stay resident: they become the platform DSP-slot catalogue and sample transport in RC5, policy stays in the services. Python `test_pk_*_fw` tests become Rust host tests in the owning service; patchkit side: remove each point's ops and anchors and its smoke/fast-roots/project entries; sites declared in ports/fw/*.toml. Estimate ~1 day, overlapping RC3's machines lane | todo | |
| RC3 Service contract v2 (must also drop the `_dt2`/`_dn2` lfo hook-name suffixes and lower the device-literals baseline they raised: lfo `module.toml` 28→76, `lfo.dko.json` 24→84; plus, decided 2026-11-22: the services' raw device-memory reads go through the typed address seam from RC2s-1; then RC2s-0b, a real machines commit in the host snapshot) | todo | |
| RC3m Module settings schema | todo | |
| RC4-lfo the lfo plug-in point becomes lfo.dks hooks installed at load; `image-modules/points/lfo` deleted. Decided 2026-11-22: device-suffixed hook names now (one hook name may appear once per service file); RC3 restores plain names | active (lane B) | branch `rc/rc4-lfo` |
| RC4 Installers and ColdFire points | todo | |
| RC5 Remove image-modules | todo | |
| RC6 Runtime size | todo | |
| RC-D DSP-side installer | todo (deferred) | |

## 14. Size baseline (RC0)

Measured 2026-11-22 on the lane state at `57b861aaa` with the lane's patchkit build and `runtime/core/build/runtime.dko.json` (newer than every runtime, service and SDK source). Static: `patchkit apply` to scratch files with a scratch copy of `ports/` that raises `container_max` so that apply completes; nothing built, flashed or emulated. **[D]**

Stored container against `container_max` (2,096,640 bytes on all three ports):

| Device | `device` set | Headroom | Runtime and its `requires` only |
| --- | --- | --- | --- |
| DT2 1.17 | 1,616,928 | +479,712 | 1,577,872 |
| DN2 1.12 | 2,028,608 | +68,032 | 1,983,248 |
| Syntakt 1.42 | 2,102,528 | **-5,888** | 2,185,808 with `card-guard`; 2,067,856 with `peer-image` added |

- `core/peer-image` is in the Syntakt `device` set and already saves 118,048 bytes (finding 147's first route).
- The Syntakt image pays a literal-tail penalty of about 69.8 KB: when the unpack margin is small (8.0 KB), patchkit stores the tail of MAIN unpacked. A shrink that restores the margin may remove the penalty [O].
- The largest resident module after the runtime is `core/digikit-menu`: 19,104 bytes stored on the Syntakt (9.3 KB on DT2 and DN2). Every other module saves 4.3 KB or less when dropped. Loading the menu app from the card (the recovery screen stays in the core) would bring the Syntakt under the limit by itself; it is the first RC6 candidate.
- On DT2 and DN2, `core/runtime` `requires` the `machines`, `params`, `dsp-voice` and `fx` points: the runtime depends on service points (section 3, problem 6). RC4 and RC5 remove these edges.

Runtime object: text 211,942 bytes, data and rodata 20,817, bss 791,234 (arena 524,288, stage 131,072). Largest groups by text and data (Rust v0 names demangled, grouped by crate and first module):

| Group | Bytes | Share |
| --- | --- | --- |
| `dkrt::loader` | 35,504 | 15.3% |
| `dkrt::storage` | 20,641 | 8.9% |
| `dkrt::files` | 19,440 | 8.4% |
| `dkrt_sdk::store` | 16,426 | 7.1% |
| `dkrt::sha256` | 7,660 | 3.3% |
| `dkrt::imports` | 6,828 | 2.9% |
| `dkrt::store_cmds` | 6,550 | 2.8% |
| `dkrt_scan` (export) | 6,092 | 2.6% |
| `dkrt::svc_imports` | 5,744 | 2.5% |
| `dkrt::plan` | 5,224 | 2.2% |
| `dkrt::reload` | 4,948 | 2.1% |

RC2 replaces most of `loader`, `plan`, `reload`, `imports` and `svc_imports` (58 KB together): the target is that `packages/` and `install/` together are smaller.

**Gate tool.** No existing tool reports these numbers; the baseline used two scratch scripts. RC1 starts by adding `patchkit size` (Rust, documented in [PATCHING.md](../PATCHING.md) and linked from DEVELOPMENT.md). It prints the stored container per device for a patch set and for the runtime with its `requires`, the drop-one saving per module, and the runtime object grouped by module, and it compares against a saved baseline. Every later item runs it before and after.
