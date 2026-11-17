# Extension model: how modules extend the firmware, one contract per service, hot reload of content modules

Status: draft for review. Roadmap row: "E Extension model" in [sdk-runtime-roadmap.md](sdk-runtime-roadmap.md).

Base: branch `int/shared-gate` (lane sdk-design: R12 batch, B1, RS-S, RS-C merged). Rebase this plan's branch on main when that lands.

Inputs: [runtime-and-services.md](runtime-and-services.md), [runtime-slimming.md](runtime-slimming.md), [digikit-hot-reload.md](digikit-hot-reload.md),
[r12-8-sharc-stubs.md](r12-8-sharc-stubs.md), [sdk.md](sdk.md), [sdk-api.md](sdk-api.md), [test-architecture.md](test-architecture.md),
[docs/ARCHITECTURE.md](../ARCHITECTURE.md), findings 02, 04, 06, 32, 33, 53, 61, 132, 135.

Firmware facts in this plan come from one static read of DT2 1.17 (atlas `out/atlas/dt2-1.17.sqlite`, MAIN_OS bytes). They are **[D]** until
item E0 has a second agent check them against the image bytes. DN2 1.12 facts are **[O]** unless marked.

## 1. Goal

Make it quick and safe to add new kinds of module (filter machines, LFO shapes, new pages, new effects) without new runtime machinery,
and make every content module hot-reloadable. In detail:

1. One written model of the ways a module can extend the firmware, with one rule for which service owns what.
2. Adding a builder call costs one `sdk.toml` entry, the service logic and its unit tests. Today it costs about 11 to 14 files.
3. Every key is derived from the module, and one planner rule refuses a duplicate or a hash collision, at boot and at a run-time load.
4. One contract suite proves every service. The same suite is the base of hot reload of content modules.
5. Hot reload of content modules (machine, effect, code-only) on the device, through the runtime's own unload path.
6. Two features prove the model: LFO shapes (a new value for an existing knob) and filter machines (a new row in a table).
7. The folders and names show the layers: runtime, platform, services with their image halves, apps, content modules.

Not in scope: replacing a service while the device runs (hot reload S3, hook gates). A changed service still needs a restart
([digikit-hot-reload.md](digikit-hot-reload.md) decision 4). Unload without a replacement, offered to the user, waits for a later decision (O5).

## 2. State before this plan

Facts from the code on `int/shared-gate`. Paths are relative to the repository root.

**What is already general**

- The `Service<C: CoreApi>` trait (`modules/core/runtime/abi/src/service.rs:436`). The four services implement it (machines, fx, lfo,
  samples: `modules/core/services/*/src/records.rs`): `declare`, `check`, `apply`, `apply_record`, `remove(owner)`, `cap`, `attribute`,
  `guard_state`, `on_loaded`, `tick`, `fx_entry`, `owned_ids`, `module_begin`, `module_commit`, `settle`.
- `[records.kinds]` in `sdk/sdk.toml` maps each record kind to its owning service. machines owns PARAM, MACHINE, PAGE, ACTION,
  NATIVE_PAGE and NATIVE_PAGE_V2; fx owns EFFECT; lfo owns LFO_SHAPE (`generic = true`). A kind is added there and in its service only.
- The runtime names no kind: registry walker (`registry.rs`), collector (`collect.rs`), boot planner (`plan.rs`). Layercheck `kind-free`
  enforces this (`tools/devtools/layercheck.py`, `RT_KIND_FREE`).
- The header table of a `.dkm` (`packages/dkneeds/src/keys.rs`) has a `KEYS` entry: per record kind, the keys the module provides, "one
  owner per (kind, key)".
- Keys today:
  - Machine keys are `hash24(<module id>:<machine id>) << 8` (`packages/patchkit/src/register.rs`).
  - Effect ids are FNV-1a 32 of `<module id>:<effect id>` folded to 16 bits, 0 turned into 1 (`register.rs:74-84`). The kit stores the id as
    a halfword beside the effect's values (`modules/core/services/fx/src/fxpage.rs:15-17`). It is module-derived already, and its width is
    part of the kit format.
  - LFO shape ids are a global u32 the module chooses (`[[record]] key`, `packages/patchkit/src/module.rs:59`). Nothing stores them yet.
- The planner refuses a duplicate key, but only for generic kinds (`plan.rs:403`, `is_generic`), and only over the tables captured at boot
  (`plan.rs:367`, `loader/mod.rs:770-787`). It refuses two owners of the same kind, slot or hook site (`owner_clash`, `plan.rs:299`) and
  checks capacity (`take_fit`, `plan.rs:263`).
- The loader refuses a content module with HOOKS or PROVIDES (`loader/role.rs`, `role_ok`). It does not refuse KINDS or SLOTS in a content
  module (`role.rs:259`).
- Sticky numbers: `dkrt_sdk::sticky` (`modules/core/runtime/sdk/src/sticky.rs`) assigns machine type numbers from machine keys and keeps them
  in the settings key `machines.types` (8 words) across boots (`modules/core/services/machines/src/assign.rs`). It is written for machine
  types only (`in_range`). `resolve` answers only from the boot's `settle` table (`assign.rs:151-165`); outside a load `boot_index` is
  `u32::MAX` (`runtime/abi/src/core_api.rs:26-28`). A full map evicts an entry and reuses its number (`sticky.rs:168-187`).
- Undo by owner already exists on both sides:
  - Runtime: `Loader::unload_rec` (`loader/mod.rs:724-767`) releases files, worker jobs, sinks, SysEx commands, tick callbacks, poll
    ranges, collect state, DSP ranges and memory, and runs `invariants::check`. It can return `E_BUSY` after some steps ran (`:732-741`).
  - Services: each `Service::remove(core, owner)` gives back its rows (machines `records.rs:849-936`, fx `records.rs:68`,
    lfo `records.rs:153`). machines `remove` writes gap-row values, not zeros (`:864-887`). DSP claim, clear and release are not part of
    `remove` (`service.rs:508-515`).
- `FakeCore` (`modules/core/runtime/test-support/src/lib.rs:101`) models firmware memory as `mem: BTreeMap<u32, u32>` and the DSP link with
  the real queue. Every service crate tests against it.

**What is missing or uneven**

- No run-time unload. `unload_rec` runs only to roll back a failed load (`undo_early`). Hot reload was archived at tag
  `digikit-hot-reload-v1` (`loader::unload`, `registry::in_use`, `registry::view_now`, `refresh_view`, `CMD_UNLOAD`). Its anchors predate
  1.17 and 1.12, and the `dynreg_info` words it used are zeros today (`modules/core/services/params/src/dynreg.rs:12-13`).
- No transport check exists in the runtime. E0b found the firmware's transport function (section "Firmware facts" below).
- ARCHITECTURE.md:198: menus are never freed and no module unloads.
- The `dk:dev` swap (about 4 s) replaces module bytes inside the emulator and restores the screen (`tools/patchkit/pk_devreload.py`). It never
  runs the device unload path. Machine and fx modules still boot.
- Duplicate checks that see the live set are per service: `E_FX_ID_DUP` (fx `build.rs:504-509`), `E_TYPE_USED` (lfo `records.rs:109-123`).
  A duplicate machine key is not refused: it gets a fresh number and is not recorded (`assign.rs`, rule 4).
- Machine and page rows are written with the raw `wr`/`rd` at absolute addresses (machines `records.rs:8`), which a host test cannot reach
  (machines `tests/records.rs:1-3`). Service state sits in statics outside `FakeCore.mem` (`DKRT_REG`, fx `IDS`, `DKRT_FXP_*`, lfo `SHAPES`).
  Service unit tests hand-build record bytes (machines `tests/records.rs:89-108`).
- Boilerplate repeated by hand for each builder call: the `exports = [...]` list of `service!` (`services/<s>/dks/src/lib.rs`), the thin
  `extern "C"` wrappers (`dks/src/builder.rs`: 10 in machines, 5 in fx; two machines wrappers convert callbacks, `builder.rs:32-38`),
  `[[export]]` in `dks/module.toml` (12 in machines, with `dk_param_get` and `dk_param_set`), `MACHINE_BUILDERS` and `FX_BUILDERS` in
  `loader/tests/assembly_tests.rs:768,780`, and the builder names in `RT_KIND_FREE` (`layercheck.py:965`).
- Kind-specific methods on the generic `Service` trait: `fx_entry` (effects for DIGIKIT > FX ROUTES), `guard_state` (machine types),
  `owned_ids` (machine type numbers for the MODULES reply). The handover of 2026-11-17 already deferred `fx_entry` to `dkrt_export(name_hash)`.
- "Adapter" covers two things: the firmware half of a service (reserved rows, stubs, slots, info tables) and the runtime's own platform code.
  `modules/core/adapters/` holds 25 folders of both.
- Proof adapters: three firmware experiments that back findings, in no patch set and required by no module. An image holds one only when a test
  names it (`apply --modules ...`). runtime-and-services.md 5.x keeps them because no shipped test covers their paths.
  - `dsp-stereo-proof` (DN2 1.12): observer stubs on the stock SHARC stages after a machine; shadow-replay of the right channel. Three rig tests,
    run in the normal rig target; `dsp-voice` hooks the same call sites; T1i extends the shadow-replay test (branch `test/t1i-rig-read-hook`).
  - `storage-fault-proof` (DT2 1.17): latches the eMMC error bits in the storage interrupt; proves the emulator's storage model (finding 107,
    patchkit `storage_fault_isr.rs`).
  - `file-catalog-proof` (DT2 1.17): directory open, next and size through a mailbox in the UI task (finding 93); opt-in device test that needs a
    prebuilt card. The runtime and `plusdrive-write` now use the same anchors.
  - `dsp-ddr-proof` and `dsp-lp0-proof` are not tracked any more.
- "Gate" has four meanings: the planned hot reload hook gate, the builder `Gate` (`params/src/builder.rs`), test gates, size limits.
- Module ids are written in each `module.toml` (`id = "core/dynreg"` in `modules/core/adapters/dynreg/`); patchkit finds manifests anywhere
  under `modules/` (`packages/patchkit/src/module.rs:1`). A folder move inside `modules/` keeps ids.

**Firmware facts (E0b, DT2 1.17 and DN2 1.12)**

The details, addresses and marks are in the findings [filter machines](../findings/142-filter-machines.md), [LFO waveforms](../findings/143-lfo-waveforms.md),
[transport state](../findings/141-transport-state.md) and the corrected [finding 135](../findings/135-hot-reload-firmware-facts.md) Q6. A second agent
checked the claims the plan depends on against the image bytes. What the items below rely on:

Filter machines:

- One chooser view (`MachineSelectionView`) holds a source list and a filter list. Filter rows are text only (no glyph). FLTR1 and FLTR2 are a
  fixed global page set: a filter type has three own knobs on FLTR1 (descriptor fields 4-6) and cannot have another page.
- The filter type is a byte in the sound: DT2 +0xa3, DN2 +0xdf. On DT2 filter types and machines share one descriptor table; on DN2 they are
  separate arrays that share one fallback row.
- Six sites interpret the value and need a hook: the accessor, the graph switch (its default branch draws something; the hook must replace it),
  the list builder, the name function ("ERROR" for 6 and up), the loader bound, and the per-type knob slot function that `core/machines` already
  hooks (`cf.machine.hook.param_code.args`). About ten other functions only copy or compare the byte.
- The stock loader keeps 0..5 (and 0xff) and resets any other value to 0. A stored type of 6 or more on stock code shows the last machine's
  knobs, the name "ERROR" and no knob slots, with no fault.
- New filter types can use 16..31: positive as a signed byte, never 0xff, above the stock range, inside the MIDI value and the 32-word SHARC table.
- DSP: the filter type goes to the SHARC as one frame word per track; one converter function indexes a 32-word table with it and keeps only
  the result. What the result selects is open (E0a). The "five filter routines" of findings 06 and 53 are stages of one function.
- Hook budget: `bind_max` is 24 per service file, `install_sites` 48 overall. `machines.dks` declares 13 hooks (DT2 binds 7, DN2 10); six
  filter hooks give 19.

LFO waveforms:

- WAVE is parameter codes 79, 89, 99, range 0..0x600 (`value >> 8`, 7 values). DT2 has one name formatter, DN2 two (4- and 3-character names).
- The tick runs in the vector_191 interrupt at 1,500 Hz and evaluates 48 LFOs (16 tracks x 3). It clamps the wave to 0..6 before it calls the
  6-entry function table; wave 6 (RAND) is a special case. A second function reads the same table and clamps the same way. The graph clamps
  to 0..6. Waveforms return signed Q31 from a 32-bit phase.
- No loader clamps the wave value, but every reader does. On stock code a wave of 7 or more shows "ERR" and behaves like RAND, with no
  fault, and survives save and load. Which field the tick clamp reads is not traced (so the exact behaviour per value is [O]).

Machine rows and transport (for E5):

- Stock dispatch sends an out-of-range machine type to the last stock machine (DT2 type 6, DN2 type 4), not to type 0. In a patched image
  a gap row is dispatched without a fault but plays type 0's sound (DT2 sample renderer, DN2 FM TONE).
- The firmware's transport function returns 0 stopped, 1 playing, 2 paused; panel keys and MIDI clock set the same words. It resolves by
  hash on both images. A record function combines playing with a UI flag.
- The machine selection list is built once per view by one builder function from a list that holds every reserved type; only the final copy
  is filtered by name. A reload needs one rebuild call on the stored view. The archive's project, kit and track facts all re-resolve on
  1.17 and 1.12.

## 3. The model

### 3.1 Vocabulary

| Word | Meaning |
| --- | --- |
| role | module or service: the header role word. Nothing else is a role. |
| kind | a record type in `[records.kinds]`. |
| service | owns a set of kinds that write to the same family of firmware tables, plus the hooks on them. |
| image half | the part of a service that must sit in the patched image: reserved rows, stubs, slots, info tables, interrupt-site stubs. Today an "adapter". |
| platform | the runtime's own image code: expansion, platform, dsp-link, ui, usb-bulk, introblank, health, and the pieces of E9. Today an "adapter". |
| content module | a `.dkm` with role module: machines, filters, effects, tools, apps. Owns records and keys, never hooks. |
| init window | the builder check that accepts builder calls only from the module whose `init` runs. Today `Gate` in `params/src/builder.rs`. |
| hook gate | the planned counter around a service hook (hot reload S3). Not built, not in this plan. |

"Adapter" and the bare word "gate" leave the code and the docs in E1 and E8.

### 3.2 Three ways to extend the firmware

| Shape | Firmware form | Examples | API pattern | Owner |
| --- | --- | --- | --- | --- |
| Table row: an entity with pages and params | a list the firmware builds from a table, plus a per-type descriptor reached through an accessor | source machine, filter machine, effect | `dk_module_ctx_add_<entity>` then `dk_<entity>_add_page` then `dk_page_add_param` | the service of that table family |
| Value of an existing knob | a closed list: name table, compute table, bound | LFO shape, overdrive curve | one call or one generic record: id, name, data | the service that owns the list |
| Overlay | draw over a stock view | shot's canvas page, a custom filter graph | `dk_page_canvas(p, DK_PAGE_STOCK_OVERLAY, ...)` | the service of the page |

A new feature request is first placed in one of these shapes. A request that fits none is a design question for this model, not a new
service by default.

**How a closed table is extended.** The firmware's bounds (`type < 6`, `moveq #6`) are not raised in place. The service takes the points
where the firmware turns a stored number into a row: the list builder (appends the new ids), the accessor (returns the service's own row for
a number in the service's range), and each switch or bound that reads the number directly (default case to the service). Source machines
work this way today (`core/machines`: lookup at run time, hooks at the machine sites, stock tables copied with rows appended). The cost of a
new table family is the number of direct readers of its stored number; E0b counts them before a feature starts.

### 3.3 Rules

1. **One service per family of firmware tables, not one per kind.** A new service only when a new table family or closed list appears.
   - Filters share the machine descriptor table, the chooser and the page view. They go into the machines service.
   - LFO shapes go into the lfo service.
2. **The API follows the domain; what the device can do grows inside the service.** A call the firmware cannot support yet is refused
   with a status code (for example `DK_E_PAGE_FULL`). When a hook later makes it possible, the same call succeeds. No API is designed ahead
   of a firmware capability.
3. **Content modules own no hooks.** `role_ok` refuses HOOKS and PROVIDES in a content module. E3 adds KINDS and SLOTS to that refusal.
   Content modules can then only clash on keys and capacity.
4. **Module code runs only through its service, in the UI task.** Module data that the firmware reads in an interrupt (an LFO shape) is
   data: a sample table, not a callback. Module callbacks (click actions, canvas draw) run only in the UI task through the service, and
   `remove` drops them before the module's code is freed (precedent: machines `records.rs:862`).
5. **Every key is derived from `<module id>:<local id>`, with a width per kind.** Machine keys: 24-bit hash. Effect ids: 16-bit folded hash
   (stored in kits; the width stays). LFO shape keys: 24-bit hash (E3). A collision between two modules is unlikely, not impossible; the
   planner refuses it.
6. **Global firmware numbers are assigned at load and kept across boots.** Machine type, filter type, LFO wave value: each comes from the
   sticky allocator with the service's range and settings key. Extended numbers are stored only where stock firmware cannot misread them
   (rule 7).
7. **Elektron data never holds a value stock code cannot handle.** An extended value goes into Elektron sound data only if every stock reader
   handles it without a fault and the value survives a stock save and load. Otherwise the value lives in digikit's store, as the fx send
   amounts do (`fxsend.rs:11-12`), and the service writes a stock value into the sound. Outcome of E0b:
   - LFO wave 7 and up: every stock reader clamps it (shows "ERR", behaves like RAND, survives save and load). It goes into the sound.
   - Filter type 16 and up: the stock loader resets it to 0. It goes into digikit's store, and the sound holds the stock filter the module names
     as its fallback (O6).
8. **One planner rule checks keys for every kind, against the boot set and the live set.** patchkit and dkneeds read the same `KEYS` table,
   so the workbench refuses a clash at upload.
9. **Services keep only domain checks**: ranges, row capacity, valid glyphs, page limits.
10. **Every service passes the contract suite** (3.5). A service that passes it supports reload of the modules that use it.
11. **Module ids do not change.** They feed the keys (rule 5), the sticky maps and kit data. Folder moves (E8) keep every id.
12. **The `Service` trait names no kind.** Service-specific queries (effects for FX ROUTES, machine guard counts, machine type numbers) are
    service exports looked up by name, not trait methods.

### 3.4 Conflicts

| Conflict | Handled by |
| --- | --- |
| Two modules derive the same key (hash collision) or the same module is loaded twice | the planner key rule, at boot and before a run-time load (E3, E5) |
| Two modules want the same firmware number | assigned by the sticky allocator (rule 6) |
| Two services hook the same site, or own the same kind or slot | `owner_clash` (exists) |
| Not enough rows or DSP time | `take_fit` and the menu's `dkrt_fit` (exist) |
| A content module hooks the firmware or declares kinds or slots | refused by `role_ok` (exists for hooks; E3 for kinds and slots) |

No per-feature conflict registry and no module marshaller are needed.

### 3.5 The service contract suite

One generic function in the test-support crate, run by every service crate in its own `rust-tests` target (unit layer, host, seconds):

```rust
// modules/core/runtime/test-support/src/contract.rs
pub struct Fixture {
    pub device: u32,               // DT2 or DN2 profile
    pub a: fn(&Builder),           // module A's init: the fixture's module.c compiled for the host, or a Rust call list
    pub b: fn(&Builder),           // module B: different keys
    pub bad: fn(&Builder),         // an init the service must refuse at the commit
}

pub fn contract<S: Service<FakeCore>>(f: &Fixture) { /* cases below */ }
```

A shared lifecycle driver in test-support runs a module the way the loader does: `module_begin`, the init calls, `module_commit` (PREPARE,
WRITE), the registry check and apply, DSP claim, and on unload `remove`, DSP clear and release. Its state snapshot is `FakeCore.mem`, the
recorded calls, the link wire, the settings store, `cap` counts and `owned_ids`.

Cases:

1. `declare` gives exactly the kinds that `[records.kinds]` assigns to this service (`Declare::matches_kinds`).
2. `check` writes nothing: the snapshot is unchanged.
3. Re-commit equivalence: the snapshot after `A` equals the snapshot after `A, unload A, A`; the snapshot after `B` equals the snapshot after
   `A, unload A, B`. `cap` counts and `owned_ids` return to their starting values after `unload A`. This is the reload property.
4. Commit A, commit B, unload A: B's rows, B's DSP rows and B's `owned_ids` are unchanged.
5. A refused commit (`bad`), then the loader's `remove`: the snapshot equals the one before.
6. Case 3 ten times: no growth in any table, no drift in sticky maps after the first assignment.

Fixtures are the shared ones in `tests/fixtures/modules`. Their `module.c` is compiled for the host against the services' `build::*`
functions (the pattern of `modules/core/adapters/ui/host`); a fixture whose init cannot compile on the host gets a Rust call list in the same
fixture folder. No hand-built record bytes.

Service statics are reset by `module_begin` and `remove`; the suite takes the shared test lock.

This is the "contract suites for `Service`" item of [test-architecture.md](test-architecture.md) T1 and the follow-up named in the 2026-11-17
handovers. Key refusal is a planner test (E3), not a contract case. `in_use` and its case wait for O5.

### 3.6 Reload of content modules

Reload replaces a loaded content module with a new file of the same module id. It is the common case: `dk:dev` and the workbench.

Order:

1. **Validate the new file first**, before anything is unloaded: role, ABI, firmware identity, and the planner (keys, owners, fit, needs)
   for the candidate against the live set minus the old module.
2. **Keys check**: the new file's `KEYS` must contain every key of the old file that kit data can name. If not, refuse (this replaces an
   `in_use` scan for reload; a key the new file drops needs unload, which waits for O5).
3. **Refuse** while the transport plays or records, during card operations, in safe mode, or while one of the module's views is open
   (until `core/ui` can tear down a module's menus).
4. **Unload the old module**: stop its voices, wait for in-flight SysEx commands and the span clock, run `unload_rec`. `unload_rec` resumes
   after `E_BUSY` from the step that returned it. DSP code is freed after two confirmed blocks.
5. **Placeholder window**: from unload to the new module's commit, a key the kit names resolves to the MISSING placeholder row, so no
   firmware path dispatches into an empty row. A DSP load spans UI passes (`loader/mod.rs:693-717`), so the window is real.
6. **Load the new module** with run-time numbering: the sticky allocator gives kept keys their old numbers and new keys free numbers outside
   the boot's `settle`.
7. **Rebuild the selection views** and bump the listing generation counters, so open lists re-read.
8. **On a failed load**: the placeholder stays, the old file is not restored (the replaced design of hot reload decision 3, load beside and
   roll back, is out of scope: it needs twice the memory). The workbench reports the failure; reloading a good file recovers.
9. `invariants::check` after unload and after load (exists).

The planner check of step 1 is the only duplicate check on a run-time load, so E3 must not delete the per-service checks until E5 runs the
planner against the live set (E3 and E5 land in that order, the per-service checks go in E5).

## 4. Code structure

### 4.1 Folders

The regroup stays inside `modules/core`, because patchkit finds manifests under `modules/` and every module id is written in its
`module.toml`. No module id changes (rule 11).

```
modules/core/
  runtime/                         unchanged (L2)
  platform/                        the runtime's own image code (L1), from adapters/:
    expansion/                       from modules/core/expansion
    platform/  dsp-link/  ui/  introblank/  health/
    usb-bulk/                        usb-bulk-audio merged in (one module, one id kept: core/usb-bulk; core/usb-bulk-audio becomes an alias or goes, see E8)
    input/  plusdrive-write/  sdhc-large/   until E9 decides on-demand loading
    fw/                              from modules/core/fw (firmware bindings, a build library)
  services/                        (L3) each service in one folder with its image half:
    machines/{src,dks,image/{machines,dynreg,params,dsp-voice,dyndsp}}
    fx/{src,dks,image/{fx,dynfx,fxpages}}
    samples/{src,dks,image/sample-pool}
    lfo/{src,dks,image/}             image/ gains the LFO tick stub in E6
    params/                          shared library crate (row writers, init window), not a service: renamed rows/ (crate dkrt-svc-rows)
  apps/digikit-menu/               unchanged (L4)
  proofs/storage-fault/            firmware experiment behind finding 107 (from adapters/storage-fault-proof)
  adapters/                        gone
```

- `dsp-stereo-proof` moves into the tests of `dsp-voice` (`services/machines/image/dsp-voice/tests/stereo-proof/`): its sources become test
  assets of the call sites it proves. Its id stays.
- `file-catalog-proof` moves to `proofs/file-catalog/`, or goes if E1 finds a shipped test that covers its path.
- Proofs are in no patch set; moving them changes no shipped image.
- `overdrive` stays as an adapter folder until E9 decides its owner; it moves to `services/<owner>/image/` or becomes a content module.

### 4.2 Names

| Today | After | Where |
| --- | --- | --- |
| `role = "adapter"`, `type = "framework"` in an image half | `role = "image"` | `module.toml` of each `services/*/image/*`; layercheck `roles` |
| `role = "adapter"` in platform code | `role = "platform"` | `module.toml` of each `platform/*`; layercheck `roles` |
| `Gate` (builder check) | `InitWindow` | `params/src/builder.rs` and users |
| `gate()` calls | `init_window()` | machines and fx `build.rs` |
| `services/params` | `services/rows` | crate rename; users in machines and fx |
| "adapter" in docs | "image half" or "platform" | ARCHITECTURE.md, LAYOUT.md, runtime-and-services.md, AGENTS.md |

Module ids keep the word they have (`core/dynreg` and so on); only folders, roles and docs change.

### 4.3 Service trait

- `fx_entry`, `guard_state` and `owned_ids` leave the trait (rule 12). They become service exports found by name hash
  (`dkrt_export(name_hash)`, the handover's deferred item): `fx.routes_entry`, `machines.guard_state`, `machines.owned_ids`. The menu and the
  MODULES reply look them up. The runtime's `kind-free` rule then also holds for the trait.
- The trait keeps the generic lifecycle: `declare`, `check`, `apply`, `apply_record`, `remove`, `cap`, `attribute`, `on_loaded`, `tick`,
  `module_begin`, `module_commit`, `settle`.
- `ServiceRow` and `SVC_ABI` change once for this (E3), together with any other trait change, so every `.dks` rebuilds once.

### 4.4 Other structure

- Thin forwarding goes: machines and fx `dks/src/builder.rs` become generated (E2); `samples/dks/src/glue.rs` (92 lines of forwarding) is
  checked for the same treatment in E2.
- `lfo` stops being a template service: it gets its hooks and image half in E6.
- `owners.toml` test tags follow the services (`S:lfo` exists; machines and fx tags are added where tests move into service crates).

## 5. Items

Each item lists what, where, steps, tests by layer, done when, estimate (agent time), payoff and confidence, and dependencies. Each item is
one PR unless it says otherwise. Every PR gets an independent review before merge.

### E0 Spikes: filter DSP, readers of stored numbers, transport state, open firmware facts

Payoff: decides E5 details, E6 and E7. Confidence of an answer: high.

E0a Filter DSP experiment (emulator, one run, about 10 minutes; needs Em's go before it runs).
- The static half is done (E0b): the ColdFire frame builder sends the filter type as TX word `0xb4 + 2 * track`; on the SHARC one converter
  function indexes the 32-word table at DM `0x2567a0` with it and stores only the result in the track record.
- On DT2 1.17: dump DM `0x2567a0..0x2567df` after boot. Then one track, steady noise input, the existing DSP PC histogram; write sound+0xa3 to
  0..7, one segment each. Record which SHARC routines change with the filter type and what the table result selects.
- Answer: whether the filter stage is a per-voice dispatch (a stub plus a slot as in R12-8) or inline stock code, and the site to hook.

E0b Static, read-only (firmware-query), in parallel. Done: results in the findings named in section 2; checked by a second agent (18 of 22
claims confirmed, one refuted and corrected, the rest open and marked).
1. Transport state: the variable that says the sequencer plays or records, on DT2 1.17 and DN2 1.12, and a find-rule for `devices/fw/`.
2. Empty machine rows: what the firmware does with a row of type 0 (finding 135 Q6, `mf_dispatch` `0x400c89a4`). Decides the MISSING
   placeholder row.
3. Readers of the filter type: every reader of sound+0xa3 and of `FUN_400506ac` (the accessor `FUN_400c89c6`, the graph switch, the DN2
   slot table `0x42c65cd0`, the DSP copy, anything else). This counts the hooks E7 needs (3.2 "How a closed table is extended") and fixes the
   numbering of new filter types (a range above the machine rows that the accessor hook maps, since rows 6 and up are machines). Also: what
   the accessor's `else` row `0x4293ba68` is.
4. Filter glyphs: where the chooser's filter glyph comes from. Filter page count: is the FLTR1/FLTR2 page list fixed or per type.
5. DN2 1.12: filter list, filter descriptor table, filter type byte, LFO tick, function table, formatter, graph clamp.
6. **Stop rule for E6 and E7**: do the project, kit and sound loaders clamp the LFO wave value (codes 79, 89, 99) and the filter type byte? If
   not, rule 7 puts extended values in digikit's store.
7. Re-resolve on 1.17 and 1.12 the project, kit, track and view-hook facts of the archive (`registry::in_use`, `view_now`, `refresh_view`,
   the core/dynreg view hook), as `devices/fw` declarations for B1 `dk_fw_` imports.
8. Count imports, binds and exports of `machines.dks` against the limit of 64, with the filter hooks of item 3 added.
9. Second agent: check every DT2 address in section 2 against the image bytes.

Output: one finding file per topic in `docs/findings/`, indexed from `docs/FINDINGS.md` (filter machines, LFO waveforms, transport state,
stored-number readers). Anchors go into `devices/fw/*.toml` with `tools.firmware.catalog declare ... --write`, then `mise run patch:port`.

Estimate: E0a 1-2 h including the report; E0b 1 day. Depends on: nothing.

### E1 Proof adapters: coverage check

The proofs stay (runtime-and-services.md keeps them: no shipped test covers their paths). E1 only checks one of them; the moves are in E8.

What:
- `file-catalog-proof`: find whether a shipped device test lists a DT2 +Drive folder through the runtime's file calls (candidates:
  `transfer-live`, `samples-store-live`, the Files page tests). If one does, propose deleting the proof and its opt-in test, with finding 93
  pointing at the shipped test; an independent reviewer checks it is not the only proof. If none does, it moves to `proofs/` in E8.
- `storage-fault-proof` and `dsp-stereo-proof` stay; nothing to check.

Gate: static checks; no device run.

Done when: the decision on `file-catalog-proof` is written into this plan (4.1) and, if it goes, the deletion PR is merged.

Estimate: 1-2 h. Payoff: small. Confidence: high. Depends on: nothing.

### E2 Builder boilerplate from sdk.toml

What: `patchkit gen` writes, per service, from the `provided = true` imports of that service in `sdk/sdk.toml`:
1. `services/<s>/dks/src/gen_exports.rs`: the `extern "C"` wrappers and an `EXPORTS` list. Signatures come from the existing `api::Dk…Fn`
   types (`sdkgen.rs` already writes them); the body calls `build::<fn>(&ImportCore, ...)` by a fixed name rule
   (`dk_module_ctx_add_fx` calls `build::add_fx`).
2. Wrappers that convert arguments (the two callback wrappers of machines `builder.rs:32-38`) stay hand-written; `sdk.toml` marks them
   `wrapper = "manual"` and the generated list includes them by name.
3. `service!` takes `exports = gen_exports::EXPORTS`. The hand-written `builder.rs` files keep only the manual wrappers.
4. `[[export]]` entries leave `dks/module.toml`; patchkit and `service-check` read the exports from `sdk.toml`.
5. `MACHINE_BUILDERS` and `FX_BUILDERS` in `assembly_tests.rs` come from the generated `gen_sdk.rs` (a per-service list).
6. `layercheck.py` builds `RT_KIND_FREE` from `sdk.toml` (builder names and the kind names of `[records.kinds]`).
7. Check `samples/dks/src/glue.rs` for the same treatment.

Tests: `patchkit gen --check` covers the generated files (host consistency). `service-check` host tests cover a missing or extra export.
Acceptance of the refactor: `patchkit objdiff` of `machines.dks` and `fx.dks` before and after is identical or differs only in symbol order,
and the `EXPORTS` lists are equal. The committed `.dko.json` files change.

Done when: adding a builder call touches `sdk.toml`, the service's `build.rs` and its tests, plus the generated files.

Estimate: 4-6 h. Payoff: every later API. Confidence: high. Depends on: nothing.

### E3 Keys, the trait, and the role check

What:
1. The planner key rule (`plan.rs` `keys_clash`) checks every kind: drop the `is_generic` condition at `plan.rs:403`. `is_generic` stays for
   `dk_record_add` only. The rule is a function over two key tables, so E5 can call it for a candidate against the live set.
2. LFO shape keys become `hash24(<module id>:<shape id>) << 8`, computed by patchkit at pack time (`module.rs`, `major3.rs`). Machine keys
   and effect ids stay as they are (rule 5).
3. A duplicate machine key in one boot (`assign.rs` rule 4) becomes a planner refusal.
4. Generalise `dkrt_sdk::sticky`: the number range and the map size become parameters (`Range { lo, hi }`). machines keeps its map format
   and its settings key `machines.types` unchanged.
5. `role_ok` refuses KINDS and SLOTS in a content module (rule 3).
6. Trait (4.3): `fx_entry`, `guard_state`, `owned_ids` become exports found by name hash; one `SVC_ABI` change; all `.dks` rebuild.
7. The per-service duplicate checks (`E_FX_ID_DUP`, the lfo `E_TYPE_USED` path) stay until E5; E5 deletes them with their refusal-table rows.

Tests:
- Unit: planner cases in `plan.rs` for a duplicate key of a typed kind, the same module twice, two modules with the same local id
  (accepted), and a forced hash collision (two key tables with the same key from different ids). `sticky` cases with two ranges. `role_ok`
  cases. dkneeds and patchkit host tests: a clash refused at pack and at the workbench fit check.
- Assembly: one case, two packed fixtures with the same key, refused by the boot plan.
- Device: the subset `C:loading` plus the fx and machines device tests that read FX ROUTES and MODULES (the trait change reaches them).

Done when: every kind's keys are checked by the one rule; the trait names no kind; layercheck `kind-free` covers the trait.

Estimate: 1.5 days. Payoff: one conflict rule for all kinds, a kind-free trait. Confidence: high. Depends on: nothing.

### E4 Service contract suite

What:
1. Route the machines row writers through `FwMem` (`core.wr`/`core.rd`) instead of the raw `wr`/`rd` (machines `records.rs:8`), so a host
   test reaches machine and page rows. Same for any other raw writer the suite finds.
2. The shared lifecycle driver and `contract.rs` in `modules/core/runtime/test-support/src/` (3.5).
3. Host compilation of fixture `module.c` files against the services' `build::*` (pattern of `modules/core/adapters/ui/host`), or a Rust call
   list per fixture.
4. `tests/contract.rs` in machines, fx, lfo and samples with DT2 and DN2 fixtures.
5. Fix what the suite finds. Expected: residue after `remove` in tables that `remove` does not reset, statics not cleared, sticky map drift.

Tests: the suite itself (unit). Check with `pnpm nx:pilot show target inputs <service>:rust-tests` that each service's target hashes the
test-support crate and the fixtures it reads.

Done when: all four services pass all six cases on both profiles.

Estimate: 2-3 days. Payoff: the reload proof for every service, and the T1 `Service` contract. Confidence: medium-high (step 1 touches the
machines writers on the device path; the device subset of E3 or E5 covers it). Depends on: E3; branch `nx/dks-tests` (the service
`rust-tests` targets).

### E5 Reload of content modules

This is hot reload S1 (code-only modules) and S4 (machine and fx modules) of [digikit-hot-reload.md](digikit-hot-reload.md), for reload
(3.6). It starts after R12-8 merges: R12-8 builds the shared unload lifecycle and late teardown in the same `Loader` code (r12-8 sections 2.6,
2.7) and moves DSP row state into the services (step 7). The two-block DSP wait for modules is S4's own work, done here.

What:
1. Runtime: `dkrt_reload(rec, path)` and `CMD_RELOAD` in `module_cmds.rs`, in the order of 3.6. One refusal function: the firmware's
   transport function returns non-zero (playing or paused; declared in `devices/fw` as `cf.project.fn.transport_state`, found by hash on
   both images), card operations, safe mode, an open module view or the machine selection view.
2. Validation before unload: the planner over the live set (E3 rule as a function), keys check of 3.6 step 2.
3. Run-time numbering: `sticky` assigns for a load outside `settle` (kept keys keep numbers; new keys take free ones); `resolve` answers from
   the live table, not only the boot table.
4. In-flight counter for module SysEx commands; clear the span clock pointer under a mask; `unload_rec` resumes after `E_BUSY`; retry
   `worker::retire` when it returns `E_BUSY`.
5. DSP: stop the owner's voices, two-block wait before `dsp::release_range`; DSP instruction-cache invalidate on range reuse (hardware-only:
   close statically first, as R12-8 lists it open).
6. MISSING placeholder row, written by machines (and filters after E7) for a key whose module is absent. A gap row plays type 0's sound
   today, so the placeholder must be made silent: DN2 sets `own_dsp = 1` (finding 33 saw silence without fault); DT2 needs a silent routine
   registered in `dsp-voice` for the placeholder type, or a selector word of 6 or more if the rig shows it gives a zero block (try on the
   rig first). The row keeps an empty list name, so the selection list leaves it out, and shows "MISSING" / "MISS" as its name.
7. Selection views: port `refresh_view` (archive `registry.rs:479`) into the machines service. Declare the list builder hook and the view
   word in `devices/fw` and fill the `dynreg_info` words (view word, builder, project singleton, active kit, track type, track count 16);
   E0b re-resolved all of them on 1.17 and 1.12. One builder call on the stored view rebuilds the list. Listing generation counters for the
   runtime menu's own lists.
8. Event tables published by pointer swap (`svc_rows.rs:73` shifts the table today), so a reader never sees a half-moved table.
9. `core/ui`: refuse reload while one of the module's views or menus is open (ARCHITECTURE.md:198); menu teardown is later work.
10. Delete the per-service duplicate checks (`E_FX_ID_DUP`, lfo `E_TYPE_USED` path) and their refusal-table rows; the planner covers them.
11. `dk:dev` and the workbench use `CMD_RELOAD` for any content module; the emulator byte swap remains only for the runtime image and the UI
    layer (`core/ui`, `core/digikit-menu`). Document the command in `docs/TESTING.md` "Fast iteration".

Tests:
- Unit: runtime host tests on `FakePlatform` for the refusal order, validation before unload, the in-flight counter, resume after `E_BUSY`,
  run-time numbering, unload then load leaving `invariants` clean.
- Assembly: one case, a real packed fixture loaded, reloaded and reloaded again through the plan.
- Device (both devices, one file in `core/runtime-tests`): (a) a code-only fixture reloaded; (b) a machine fixture with DSP reloaded with a
  key held, and reloaded during voice release: silence in the window, the same PCM pin after reload; (c) an fx fixture reloaded; (d) reload
  refused while the transport plays; (e) project round trip: save a project that uses the module, reload, load the project, same sound.
- Soak: 20 reload cycles with `DIGI_FLUSH_CHECK=1` on the emulator, `invariants` clean, no memory growth.

Done when: `dk:dev` reloads a machine or effect module with no boot, on both devices, and the soak passes.

Estimate: 6-7 days (the hot reload plan's S1 plus S4). Payoff: removes the 5-10 s boot from every machine and effect iteration, and makes
module updates live for users. Confidence: medium-high; the archive proved the machine path on the emulator (findings 32, 33).
Depends on: E3, E4, R12-8 merged. (E0b is done.)

### E6 LFO shapes

What:
1. Hook contexts (sites in the LFO waveforms finding, both devices):
   - Interrupt sites, as an image half (a stub with a slot in `services/lfo/image/`; new image hashes, patch sets updated): the tick's wave
     clamp (DT2 `0x40139698`, DN2 `0x40137b56`) and its function-table call and RAND compares (DT2 `0x40139804`, `0x4013986c`), and the second
     function-table reader with its own clamp (DT2 `FUN_401392d4`, DN2 `FUN_401376bc`). Without the clamp hook a wave of 7 or more becomes RAND
     before the table call.
   - UI-task sites, as `lfo.dks` hooks (B1 bind resolves the addresses): the name formatter (DT2 one; DN2 two, 4- and 3-character names), the
     graph clamp and the unipolar table, and the T1 maximum of codes 79, 89, 99 so the panel can reach the new values.
2. The slot code: for an extended wave value, compute signed Q31 from the 32-bit phase with the module's 32-sample table and integer linear
   interpolation; mark unipolar shapes in the service's own table. Cost ceiling: the tick runs at 1,500 Hz and evaluates 48 LFOs; the estimate
   is 20-30 instructions per extended LFO, at most 1.5-2 M instructions per second with all 48 extended. Measure with the instruction counts
   before the PR; stop if above the ceiling.
3. `SHAPES` writes happen under the interrupt mask (lfo `records.rs:153-161`), so the tick never reads a half-written shape.
4. Stored value (rule 7): the extended wave value goes into the sound. Every stock reader clamps it, so without the module it shows "ERR" and
   behaves like RAND, with no fault, and it survives save and load.
5. The LFO_SHAPE record gains a name (4 characters, as the stock names). The kind has no consumer on the device, so its layout changes in place;
   this is a contract MINOR (`since` bumped).
6. Wave value numbers from the sticky allocator (E3), range above 6, settings key `lfo.shapes`.
7. With `lfo.dks` absent or in safe mode, stock code handles the value as in item 4. With the service loaded but the shape's module absent,
   the slot falls back to TRI.
8. Builder: `dk_record_add` with the generic LFO_SHAPE record; a typed `dk_lfo_add_shape` only if writing the fixture shows `dk_record_add`
   is awkward in C.
9. Template `sdk/templates/lfo/` and a fixture `tests/fixtures/modules/test/lfo-shape`.

Tests: unit (lfo service: lookup, interpolation, fallback, contract suite); assembly (one case, the lfo fixture binds its hooks and the image
stub slot is filled); device (one test per device: choose the new wave, read the firmware's LFO output in the parameter value array, not the
screen; and one stock-firmware or safe-mode check that the saved project loads without a fault).

Done when: a module adds a waveform that shows its name on the MOD page, draws, and modulates, on DT2 and DN2, and a project saved with it
loads safely without the module.

Estimate: 2-3 days. Payoff: proves the "value of an existing knob" shape. Confidence: medium-high (all sites found on both devices; the
interrupt cost is the open risk). Depends on: E3; E4. Open first: which field the tick clamp reads (finding [O]).

### E7 Filter machines

Only if E0a finds a usable DSP site. E0b bounded the ColdFire side: six hook sites on DT2, the same shape on DN2.

What:
1. `dk_module_ctx_add_filter(ctx, index, name, short)` returns a typed `dk_filter *`. Page and param calls take a `dk_filter *` through
   their own typed entry points (`dk_filter_add_page`), implemented by the machines service with the same code as machines. The typed handle
   costs nothing and stops a track's machine and filter from being mixed up in C.
2. The machines service gains the filter table family (3.2), six sites per device (addresses in the filter machines finding): the list
   builder (append the new ids), the accessor (return the service's row for 16..31), the graph switch (replace the default branch for 16..31;
   the service draws through an overlay canvas), the name function, the loader bound, and an extension of the existing
   `cf.machine.hook.param_code.args` hook for the per-type knob slots. New filter type numbers are 16..31, assigned by the sticky allocator
   (E3); they never index a stock table. Chooser rows are text only, so a filter has a name and no glyph.
3. A filter page has at most three own knobs (descriptor fields 4-6); a fourth is refused with `DK_E_PAGE_FULL`. FLTR1 and FLTR2 are a fixed
   global page set, so a second filter page is refused the same way (rule 2).
4. Stored value (rule 7): the stock loader resets 16..31 to 0, so the choice lives in digikit's store per sound and the sound holds the stock
   filter the module names as its fallback. The loader-bound hook restores the choice from the store when a kit loads (O6).
5. DSP: a stub at the filter-type site with a slot, dispatch in the `machines.dks` DSP section (R12-8 pattern). Filter DSP is a new contract
   in `sdk/dsp/contract/` (input and output buffers per voice, state per voice).
6. Hook budget: `machines.dks` declares 13 hooks; the six filter sites give 19, under `bind_max` 24 per service file and `install_sites` 48
   overall. Five hooks remain after E7; the next table family may need a second service file.
7. Template `sdk/templates/filter/` and a fixture `tests/fixtures/modules/test/filter-gain`.

Tests: unit (machines service filter lowering, contract suite with a filter fixture); rig (filter DSP conformance in `@digi/sharc-rig-kit`);
assembly (filter fixture loads); device (one test per device: select the filter, render, PCM report against the fixture's expected response).

Done when: a module adds a filter that appears in FUNC+SYN on the FLTR page, shows its knobs, draws its graph and filters audio, on DT2
(and DN2 if E0b item 5 shows the same shape).

Estimate: 3-5 days. Payoff: proves the "table row" shape for a second table family. Confidence: medium until E0a (ColdFire side known).
Depends on: E0a; E3; E4; E5 (MISSING placeholder for filters); R12-8.

### E8 Folder regroup and names

What: section 4.1 and 4.2, as one mechanical PR after the open branches merge.
- `git mv` into `modules/core/platform/` and `modules/core/services/<s>/image/`; `services/params` to `services/rows`.
- `module.toml` roles (`image`, `platform`); layercheck `roles` table and folder rules; `project.json` paths and Nx project roots;
  `tools/nx/*.mjs` inputs; patch-set comments in `devices/*.toml` (ids unchanged).
- Merge `usb-bulk-audio` into `usb-bulk`: one module; the patch sets name `core/usb-bulk`; the `core/usb-bulk-audio` id goes (it is a
  platform module with no stored identity).
- `InitWindow` rename (4.2).
- Proofs (4.1): `dsp-stereo-proof` into the tests of `dsp-voice`, `storage-fault-proof` (and `file-catalog-proof` if E1 keeps it) to `proofs/`.
- Docs: ARCHITECTURE.md (layers, folder map), LAYOUT.md, AGENTS.md directory map, runtime-and-services.md wording.

Tests: `git diff -M --stat` shows renames plus path edits only; image and `.dks` hashes unchanged except the usb-bulk merge (its image
diff is checked with `patchkit objdiff`). This selects the whole gate, so it runs with the full test run of its batch (`mise run test:full`).

Estimate: 4-6 h mechanical plus the full run. Payoff: the folders show the layers; new code lands in the right place. Confidence: high.
Depends on: E1; open branches merged (including `test/t1i-rig-read-hook`, which edits `dsp-stereo-proof`). Order: last, or before E6 if the
branches have merged by then.

### E9 Later: on-demand platform pieces and overdrive

Not started by this plan. Candidates that only some modules need, so they could load with a service instead of sitting in every image:
`input` (used by remote), `plusdrive-write` (usb-fast), `sdhc-large` (large transfers), and possibly `health`. USB descriptor code stays in
the image (descriptors cannot change after enumeration). Decide per piece with the "harmless with no service loaded" test of
runtime-and-services.md.

`overdrive` as a "value of an existing knob" feature (OVR TYPE): its owner service is open (O3).

## 6. Order and dependencies

| # | Item | Depends on | Device runs | Status |
| --- | --- | --- | --- | --- |
| 1 | E0b static spikes (extended) | - | none | done: findings 141, 142, 143; 135, 02, 06, 53 corrected (branch e0b/findings) |
| 1 | E0a filter DSP experiment | Em's go | one, about 10 min | todo |
| 2 | E1 proof coverage check | - | none | todo |
| 2 | E2 boilerplate codegen | - | none; objdiff | todo |
| 3 | E3 keys, trait, role check | - | subset, shared with E1 and E2 in one gate | todo |
| 4 | E4 contract suite | E3, `nx/dks-tests` | none | todo |
| 5 | R12-8 merged | (roadmap) | (its own gate) | roadmap row R12-8 |
| 6 | E5 reload of content modules | E3, E4, E0b, R12-8 | reload test, soak, subset | todo |
| 7 | E6 LFO shapes | E0b, E3, E4 | lfo test plus subset | todo |
| 8 | E7 filter machines | E0a, E0b, E3, E4, E5, R12-8 | filter test plus subset | todo |
| 9 | E8 regroup and names | E1, open branches merged | full run of its batch | todo |

E1, E2, E3 share one subset gate. E0 runs in parallel with E1-E3. When an item starts, its status becomes `active` with the branch; when it lands, `done` with the PR, set in that PR.

## 7. Test pyramid and Nx

| Layer | What this plan adds | Target |
| --- | --- | --- |
| Unit | contract suite per service (6 cases x 2 devices x 4 services); planner key cases including a collision; sticky ranges; `role_ok`; reload refusals and order; run-time numbering; lfo lookup; filter lowering | `<service>:rust-tests` (from `nx/dks-tests`), `core/runtime:rust-tests` |
| Generated consistency | `patchkit gen --check` for exports and builder lists; `patchkit objdiff` for the E2 refactor | `mise run gen:modules -- --check` |
| Assembly | one case per new seam: key clash in the boot plan; load-reload-reload of a packed fixture; lfo hooks and image slot bind; filter fixture loads | `core/runtime:test-assembly` |
| Rig | filter DSP conformance | `@digi/sharc-rig-kit:test-conformance-rig` |
| Device | reload (code-only, machine with DSP, fx, during voice release, refused during transport, project round trip); LFO shape output and safe load without the module; filter select and render | `core/runtime-tests:test-device--<file>`, `<module>:test-device` |
| Soak | 20 reloads with `DIGI_FLUSH_CHECK=1` | emulator, once per E5 PR |

Rules kept: each assertion at the lowest layer that can see it; fixtures from `tests/fixtures/modules`; device tests read firmware output,
not the screen; PCM pins set from a numeric report. Before a device run, list the tests and the time and get Em's go.

## 8. Risks and stop rules

| Risk | Effect | Stop rule or fallback |
| --- | --- | --- |
| A stock reader of the LFO wave that E0b missed does not clamp | a fault on stock code with a saved extended wave | E6 device test loads a saved project without the module and in safe mode |
| E0a finds the filter stage inline in stock code with no per-type dispatch | E7 needs a new DSP hook design | stop E7 after E0a and report payoff and cost; E1-E6 are unaffected |
| A filter-type reader E0b missed indexes a table by the raw value | wrong data for 16..31 | E7 device test walks every filter page and the chooser with a custom filter selected |
| A gap row plays type 0's sound (DT2 sample renderer, DN2 FM TONE) | sound in the reload window | the MISSING placeholder is made silent (E5 item 6); on DT2 the silent routine is checked on the rig first |
| The contract suite finds residue in many tables | E4 grows | fix in the owning service; each fix is a unit test |
| Routing machines row writers through `FwMem` changes timing or code size | device behaviour changes | E3/E5 device subset covers the machines path; size limit of runtime-and-services.md applies |
| LFO slot cost in the interrupt | audio or UI timing | state the ceiling first; integer interpolation; stop if above the ceiling |
| DN2 LFO or filter shape differs | DN2 support later than DT2 | ship DT2 first only with Em's OK; otherwise wait for E0b item 5 |
| Regroup breaks Nx inputs | wrong test selection | `mise run test:plan` and the full run in the same batch |

## 9. Decisions

Taken:
- Extended values (rule 7): LFO waves in the sound (stock readers clamp them); filter types in digikit's store (the stock loader resets them).
- Proof adapters stay; they move next to what they prove (4.1); E1 checks whether `file-catalog-proof` is still needed.
- O1: filters get a typed `dk_filter *` handle; the machines service implements them.
- O5: reload first; user-facing unload without a replacement later.
- O2: regroup inside `modules/core` (section 4.1), after the open branches merge.
- Effect id format stays 16-bit.

Open:
- O3: owner of the overdrive curve list if overdrive becomes a feature: a small `drive` service, or the fx service.
- O4: E6 builder: `dk_record_add` with the generic LFO_SHAPE record, or a typed `dk_lfo_add_shape` (decided while writing the fixture).
- O6: E7 stored filter choice: digikit's store with a stock fallback in the sound (recommended, rule 7), or patch the stock loader bound
  (7 to 32) so the extended type survives in kit files; with the patch, a kit opened on stock firmware falls back to filter 0.

## 10. Changes to other documents when items land

- [digikit-hot-reload.md](digikit-hot-reload.md): S1 and S4 are done by E5 for reload; decision 3 (load beside, roll back) is replaced by
  validate-then-reload with a placeholder (3.6); S3 stays deferred; "gate" becomes "hook gate".
- [sdk-runtime-roadmap.md](sdk-runtime-roadmap.md): row E; row H points to E5 for S1 and S4.
- [test-architecture.md](test-architecture.md): T1 "contract suites for `Service`" is E4.
- `docs/ARCHITECTURE.md`, `docs/LAYOUT.md`, `AGENTS.md` directory map: E8. ARCHITECTURE.md:198 (no unload) changes with E5.
- `docs/MODULE-AUTHORING.md` and `sdk/README.md`: the three shapes of 3.2 and the rules of 3.3 (E2), filters and LFO shapes (E6, E7).
- `.agents/skills/digikit-module/SKILL.md`: the filter and LFO builder calls (E6, E7).
