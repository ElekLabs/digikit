# Runtime contract and package lifecycle

Date: 2026-11-22. Status: **approved by Em 2026-11-22; RC1 active.** Items: **RC0** to **RC6**, plus the deferred **RC-D** (DSP-side installer).

This plan replaces the open parts of [runtime-and-services.md](runtime-and-services.md) and the "Remaining ownership work" of [ARCHITECTURE.md](../ARCHITECTURE.md). Where it disagrees with older docs, this plan wins and the older text is overwritten when the item lands. The static evidence is [finding 158](../findings/158-service-entry-survey.md) (service entries) and [finding 147](../findings/147-syntakt-flash-image-size.md) (Syntakt image size).

## 1. Goal

A small runtime that names no service and no domain concept. It owns boot and recovery, the package lifecycle, safe calling contexts and shared mechanisms. Services own their domain state, policy, records, resources and versioned operations. Content modules depend on services.

Done means:

- A new service, and the modules that use it, ship as card files. No runtime edit, no runtime rebuild, no reflash. The exception: a service that needs a new SHARC entry site needs a flash until RC-D lands (section 9).
- Loading, unloading and reloading a module or a service from the DIGIKIT menu use one code path. Unloading the last consumer of a service unloads the service, unless the selection pins it.
- `image-modules/` does not exist. Everything resident (flashed) is in `runtime/`. A service has no resident code.
- The runtime is smaller after every item (section 11). The Syntakt image fits its container.
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

[[entry]]                          # resolved through the port file at load
hook = "cf.page.hook.page_desc"
[[entry]]
slot = "dsp.fx.master"             # SHARC slot catalogue (section 9)
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

```toml
# the selection may pin a service
[[selection]]
path = "services/project.dks"
pin = true
```

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
        for s in self.graph.services_of(p) { self.host.attach(s, p)?; }
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
Absent -> Placed -> Linked -> Started -> Claimed -> Installed -> Active
Active -> Detached -> Released -> Restored -> Stopped -> Freed (Absent)
```

A failure unwinds exactly the steps done. The DSP part of a package comes up over UI passes after `Installed`; a DSP failure takes down the closure of that package through the same path.

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
| `test/local-ping-1-3` | The same service at a newer minor version (a thin variant): reload with a new version | RC1d |
| `test/local-ping-user`, `test/local-ping-user-b` | Two consumers of one service: shared service, orphan unload | RC1d |
| `test/local-ping-old-user` | Needs a major version the provider does not have: the incompatible-need refusal | RC1d |
| the same fixtures | One resource `local-ping.tokens` (limit 2), one record kind owned by the service, attach/detach counters, a query, one ColdFire hook entry; a greedy consumer for the fit refusal | RC3 |

They replace `svc-test`, `svc-test-user` and `svc-echo` where coverage overlaps (RC3). Each deletion is mapped to the test that now covers it and reviewed before merge.

Development style: write the unit test first for each owner method; then the assembly case for the seam; device tests last and only for the rows above.

## 11. Size gate

Every item records, before and after:

- the stored container size against `container_max` for the `device` set of DT2 1.17, DN2 1.12 and Syntakt 1.42, and for `core/runtime` alone;
- the runtime object's code and data sizes, grouped by Rust module.

The tool is `patchkit size` (built first in RC1; section 14 has the baseline). An item that grows the core states why in its PR. RC2 and RC3 must shrink it. RC6 runs only if the Syntakt still does not fit after RC5.

## 12. Decisions

Taken (Em, 2026-11-22):

- `fixes/overdrive` is deleted.
- The selection may pin a service.
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
| **RC1 Contract and fixtures** | `patchkit size` (section 14); `Provider`, parts, `ProviderRow`, `Installer`, manifest schema (`[[service.resource]]`, `[[service.record]]`, `[[entry]]`, derived needs) parsed and validated in patchkit, not packed; `PackageGraph` and `ResourceLedger` test-first, not wired; `PackageGraph::from_tables` over the packed `local-ping` fixtures | `core/runtime:rust-tests`, patchkit tests, fixture pack checks | no change in the image | 1-2 days, high |
| **RC2 One lifecycle** | `PackageManager` replaces the provider and module paths, `take_down`, `needs_hash` and `plan.rs` provider handling; replaces the loader's two inline version checks (loader/mod.rs, `[[service_import]]` and live plan) with `dkrt_abi::version::version_satisfies`; reload of services from the menu; orphans; pins (the selection record has no free field: a record version 2 or a settings key); MIDI snapshot | Unit and `core/runtime:test-assembly`; one device test per device: `core/runtime-tests:test-device--service-reload` | must shrink | 3-5 days, medium |
| **RC3 Service contract v2** | Owner-object services; tagged parts; provider-local record kinds and resources; `Describe` and query replace both bridges; `svc_rows` fallback, `[records.kinds]` owners and domain capacity ids deleted; header table version 4 with the new schema packed (provider-qualified resources with aggregate and parent, provider-local record kinds, entries); `.dkm` record format version bump; the fixtures gain resources, records, a query and a hook; all 6 services and content modules repacked | Service unit tests, assembly, the device tests `test:plan` selects for the services | must shrink | 4-6 days, medium |
| **RC4 Installers and ColdFire points** | `Installers` with both backends; `lfo` first (deletes `points/lfo`), then `machines`, `dynreg`, `params` content, `fxpages` into their services and `services/rows` | Backend unit tests; the existing device tests of each service | the image shrinks | 3-5 days, medium (lfo 1 day, high) |
| **RC5 Remove image-modules and tidy runtime/core** | SHARC slot catalogue and drivers to `runtime/platform`; platform stops naming services; proofs to tests; `overdrive` deleted; the crates of `runtime/core` move to `runtime/core/crates/<name>` (the runtime crate from `src/rs` to `crates/dkrt`, assembly from `src/cf` to `asm/`); the runtime crate's remaining flat files grouped into concept folders | Byte manifest for pure moves; named tests for the de-naming | neutral or smaller | 1-2 days, high |
| **RC6 Runtime size** (only if the Syntakt still does not fit) | First candidate: load `digikit-menu` from the card (19.1 KB stored on the Syntakt). Then move optional core parts to services, ranked by measured bytes; Transfer and recovery stay in the core | Per moved part | Syntakt fits | sized after RC5 |
| **RC-D DSP-side installer** (deferred) | A resident DSP installer that patches sites at block tail; `SharcInstaller` v2; SHARC slots leave the image | Static design, emulator, one hardware test | the image shrinks | 2-4 days, low-medium |

Before RC2 and RC3 start: state payoff and confidence again with the RC1 results, and stop if they are weak.

### Progress

Status: `todo`, `active`, `review`, `done`. Each item and step ends the same way:

1. The worker leaves its changes uncommitted and marks the row `review`.
2. The worker runs `mise run test:plan` with `DIGI_GATE_BASE` at the start of the phase and states whether the selection matches what the change can reach; a selection that is wider is fixed in the inputs and edges, not run. An independent reviewer sub-agent reviews the diff and the selection (ranked findings with `file:line` and a failure scenario; boundaries, separation of concerns and device specifics behind capabilities included). Findings are fixed or answered.
3. Em reviews the phase or deliverable. Only then is it committed and the row set to `done` with the commit.

| Item or step | Status | Commits, PR |
| --- | --- | --- |
| RC0 Plan and baseline | done | `57b861aaa` (finding 158), `3d6a6bc49` (plan, baseline); approved by Em 2026-11-22 |
| RC1 Contract and fixtures | active | lane `fx-route-policy` |
| RC1a `patchkit size`: stored container per device, runtime with its `requires`, drop-one per module, runtime groups, compare with a saved baseline | done | `dd211d711` |
| RC1b crate `dkrt-lifecycle` (`runtime/core/lifecycle`): `PackageGraph` and `ResourceLedger`, test-first, pure, not a runtime dependency yet | done | `65bb121db` |
| RC1c contract types: `Provider`, parts, `ProviderRow`, `PartRef` (a new `dkrt-abi` file outside `SVC_ABI_FILES`); `Installer`, `Entry` (in `dkrt-lifecycle`) | done | `4b48e0de9` |
| RC1f rename `dkrt-packages` to `dkrt-lifecycle` (`runtime/core/lifecycle`): the crate holds the lifecycle logic, RC2's `PackageManager` included | done | commit "Rename dkrt-packages to dkrt-lifecycle" (hash in the next progress update) |
| RC1d `PackageGraph::from_tables` over the existing header table (NEEDS, PROVIDES; shared `dkneeds` parser); the manifest schema parsed and validated in patchkit, not packed (packing is RC3); the `local-ping` fixtures extended (second consumer, incompatible consumer, newer provider version); one host test that packs them and checks bring-up and take-down orders | todo | |
| RC1e selection check: RC1's host-only changes select device tests (runtime `Cargo.lock`), every Rust target (workspace `Cargo.lock`) and every service (any `dkrt-abi` file); narrow the edges that are wrong so `test:plan` shows what the change can reach | done | `b62328998` |
| RC2 One lifecycle | todo | |
| RC3 Service contract v2 | todo | |
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
