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
([digikit-hot-reload.md](digikit-hot-reload.md) decision 4). Unload and load of a content module while the device runs, offered to the user in the MODULES list, are E5c.

## 2. State before this plan

Facts from the code on `int/shared-gate`. Paths are relative to the repository root.

**What is already general**

- The `Service<C: CoreApi>` trait (`modules/core/runtime/abi/src/service.rs:436`). The four services implement it (machines, fx, lfo,
  samples: `modules/core/services/*/src/records.rs`): `declare`, `check`, `apply`, `apply_record`, `remove(owner)`, `cap`, `attribute`,
  `on_loaded`, `tick`, `module_begin`, `module_commit`, `settle` (E3 removed `guard_state`, `fx_entry` and `owned_ids`: see 4.3).
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
  - LFO shape ids were a global u32 the module chose (`[[record]] key`). E3: `[[record]] id` and a derived key, like machines.
- Before E3 the planner refused a duplicate key only for generic kinds (`is_generic`); now `keys_clash` and `key_refusal` check every kind, over the tables captured at boot
  (`plan.rs:367`, `loader/mod.rs:770-787`). It refuses two owners of the same kind, slot or hook site (`owner_clash`, `plan.rs:299`) and
  checks capacity (`take_fit`, `plan.rs:263`).
- The loader refuses a content module with HOOKS, PROVIDES, KINDS or SLOTS (`loader/role.rs`, `role_ok`; E3 added KINDS and SLOTS).
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
- Kind-specific methods were on the generic `Service` trait: `fx_entry` (effects for DIGIKIT > FX ROUTES), `guard_state` (machine types),
  `owned_ids` (machine type numbers for the MODULES reply). E3 made them `internal` exports found by name (`dk_fx_routes_entry`, `dk_machines_guard_state`, `dk_machines_owned_ids`).
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

The details, addresses and marks are in the findings [filter machines](../findings/148-filter-machines.md), [LFO waveforms](../findings/149-lfo-waveforms.md),
[transport state](../findings/147-transport-state.md) and the corrected [finding 135](../findings/135-hot-reload-firmware-facts.md) Q6. A second agent
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
- Hook budget: with the port file, `BIND` items leave the services, so no per-file limit applies. `install_sites` 48 bounds the installed hook sites
  overall. `machines.dks` declares 13 hooks (DT2 binds 7, DN2 10); six filter hooks give 19.

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
| role | in the file header: 0 module, 1 service, 2 port. In `module.toml`: runtime, installer, platform, image-half, adapter, fix, proof, app or service (the repo class of a unit; the taxonomy lists them). Different words for different things: say "header role" or "manifest role" where it matters. |
| record kind | a record type in `[records.kinds]`. A content module has a module kind (machine, fx, filter, lfo-shape, tool). |
| service | owns a set of kinds that write to the same family of firmware tables, plus the hooks on them. |
| image half | the part of a service that must sit in the patched image: reserved rows, stubs, slots, info tables, interrupt-site stubs (class `image-half`). Today an "adapter". |
| platform | the runtime's own image code: expansion, platform, dsp-link, dyndsp, ui, health, input, and the pieces of E9. Today an "adapter". |
| adapter | a code adapter: image code for one device function that is no service and no runtime part (usb-bulk, plusdrive-write, sdhc-large). |
| fix | an optional firmware fix: changes stock behaviour, needed by no service (overdrive, introblank). |
| proof | a firmware experiment behind a finding, in no patch set. |
| content module | a `.dkm` with role module: machines, filters, effects, tools, apps. Owns records and keys, never hooks. |
| init window | the builder check that accepts builder calls only from the module whose `init` runs. Today `Gate` in `params/src/builder.rs`. |
| hook gate | the planned counter around a service hook (hot reload S3). Not built, not in this plan. |

"Adapter" (as a name for all L1 image code) and the bare word "gate" leave the docs in E8 and the code in the Rust rename PR.

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
3. **Content modules own no hooks.** `role_ok` refuses HOOKS, PROVIDES, KINDS and SLOTS in a content module.
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
    pub kinds: &'static [(u32, u32)], // [records.kinds] of the service
    pub image: fn(&mut FakeCore),  // the image tables of the profile
    pub state: fn(&FakeCore) -> Vec<u32>, // the service's own view (owners, ids)
    pub a: Module,                 // module A: keys, a call list that stands for the fixture's module.c, its DSP rows
    pub b: Module,                 // module B: different keys
    pub a2: Module,                // the keys of A, other content in every table the service fills (names, defaults, DSP fields)
    pub enable: &'static [(u32, u32, u32)], // link words that turn a DSP row off: (window, word, off value)
    pub kept: fn(&FakeCore, u32, u32) -> bool, // (address, value): a memory word that may differ from the start after a release, with the value it may hold
    pub kept_dsp: fn(usize, u8) -> bool,       // the same for a DSP state byte (offset, value)
    pub live: Option<fn(&FakeCore)>,           // queues live values for the loaded module's rows, as a knob turn does
    pub salt_same: &'static [(&'static str, &'static str)], // areas where A and a2 may be equal, each with its reason
    pub bad: Module,               // a module the service must refuse at the commit
    pub bad_late: Option<Late>,    // a module refused during apply, after rows were written, and its reference that loads
}

pub fn contract<S: Service<FakeCore>>(f: &Fixture) { /* cases below */ }
```

A shared lifecycle driver in test-support (`Lifecycle`) runs a module the way the loader does: `settle`, DSP admit and claim, `module_begin`, the
init calls, `module_commit` (PREPARE, WRITE), the registry check (the layout by `dkrt_abi::regcheck::check_layout`, the function the runtime's walker
calls, then the service's check steps), DECLARE, apply, row publication, and on unload `dsp_clear`, `remove` and DSP release. Its state snapshot is
`FakeCore.mem` (non-zero words), the whole DSP state (`DspState` bytes), the last non-zero word on each link slot, the link queue (live values, queued
requests), the last word at each `Fixture::enable` word, the settings store, `cap` counts and the service's own view (`Fixture::state`:
`fxpage::page_owner`, `owned_ids`, `tracks::has`). A module gets DSP rows whose fields follow `Dsp::salt`, so two modules with the same keys and other
salts claim the same rows with other contents.

Cases:

1. `declare` gives exactly the kinds that `[records.kinds]` assigns to this service (`Declare::matches_kinds`).
2. `check` writes nothing: during the load of A and of B (the state in which the loader checks, after the commit and before the apply) the snapshot
   before and after the registry check is the same, the check makes no core call and accepts the section; a second run over the section of a loaded
   module is compared the same way.
3. Re-commit equivalence: the snapshot after `A` equals the snapshot after `A, unload A, A`. What `B` alone writes is the same after
   `A, unload A, B`: a row that `A` held and released keeps the service's free values, which a never used row does not have, so the words of `B`
   are compared, not the whole memory. `cap` counts and the service's view return to their starting values after `unload A`. This is the
   reload property.
4. Load B, load A, unload A: B's words, DSP state and ids are those of B alone, and another cycle of A changes nothing. Load A, load B, unload
   both: `cap` counts and the view are at their start.
5. A refused commit (`bad`), then the loader's `remove`: the snapshot equals the one before.
6. Case 3 ten times: no growth in any table, no drift in sticky maps after the first assignment.
7. An unload leaves nothing. Two comparisons. (a) From the same fresh start, the state after "load `A`, unload `A`" equals the state after "load `a2`, unload `a2`"
   in memory words, DSP state bytes, link queue and requests, enable words, settings, `cap` counts and the view (the last words sent for a row's content
   differ with the module and stay in the DSP memory until the next row overwrites them: they are not compared). Loaded `A` and loaded `a2` must differ in
   each area (memory, DSP, settings, view), or the fixture lists the area in `salt_same` with a reason; a listed area that differs fails too. (b) The state after the
   unload is compared with the state before the load: every memory word, DSP state byte and the settings that differ from the start fail, unless `Fixture::kept`
   (address and value) or `kept_dsp` (byte offset and value) names the word with the value it may hold. A value that the module wrote, or that the unload wrote
   back from the module, is not a free value, so an unload that writes `A`'s own value back, or leaves a constant such as an owner or active byte, fails. The
   harness starts every voice row free, as the runtime's `init_once` does. Besides: no queued link value or request is left (`Fixture::live` queues live values
   while the module is loaded, so a release that does not forget them shows), and every enable word is 0 or its off value after the unload.
8. A commit refused during apply (`bad_late`: rows are written, then a record is refused), then the loader's undo, leaves what the unload of
   `same_but_ok` leaves (a module that claims the same DSP rows and writes the same rows and loads), as in case 7 but without the enable words (a row
   that was never published sends none); also with B loaded when the module fits beside it. The failure must come after at least one record applied.
   Lfo and samples have no such path: their `check` covers every limit of their `apply`.

Fixtures are the shared ones in `tests/fixtures/modules` (`lfo-shape`, `dyn-fx-gain`, `dyn-fx-gain-b`, `sine`). Each module of the suite is a Rust
call list that makes the builder calls of the fixture's `module.c` through the service's `build::*` (or `Init::record` for `dk_record_add`) and mirrors
the fixture's calls exactly; it sits in the service's `tests/contract.rs`, because it names `build::*` and test-support is below the services. `a2`,
`bad_late` and its reference have no shared fixture. The fixtures' `module.toml`, `module_ids.h` and sources are inputs of the crate's `rust-tests`
(`FIXTURE_SOURCE` in `tools/nx/cargo.mjs`). Host compilation of the `module.c` files is not done. The samples service has no kind and no builder
call: its module is a track-table record in the DSP section (`Dsp::bank`; `a2` has another local id), and its `bad` module is a stray `dk_record_add`
that the commit refuses after the claim.
No hand-built record bytes where a builder call exists.

Service statics are reset by `module_begin` and `remove`; the suite takes the shared test lock.

This is the "contract suites for `Service`" item of [test-architecture.md](test-architecture.md) T1 and the follow-up named in the 2026-11-17
handovers. Key refusal is a planner test (E3), not a contract case. There is no `in_use` scan: the refusals of an unload are the menu check and `teardown_refusal` (E5c).

### 3.6 Reload of content modules

Reload replaces a loaded content module with a new file of the same module id. It is the common case: `dk:dev` and the workbench.

Order:

1. **Validate the new file first**, before anything is unloaded: role, ABI, firmware identity, and the planner (keys, owners, fit, needs)
   for the candidate against the live set minus the old module.
2. **Keys check**: the new file's `KEYS` must contain every key of the old file that kit data can name. If not, refuse (this replaces an
   `in_use` scan for reload; a key the new file drops needs an unload and a load, E5c).
3. **Refuse** while the transport plays or records, during card operations, in safe mode, when the module built its own menus (until
   `core/ui` can tear down a module's menus, E5b), and for an fx module that has a published effect row (E5b).
4. **Unload the old module**: stop its voices, wait for in-flight SysEx commands and the span clock, run `unload_rec`. `unload_rec` resumes
   after `E_BUSY` from the step that returned it. DSP code is freed after two confirmed blocks.
5. **Placeholder window**: from unload to the new module's commit, a type number the kit names resolves to the MISSING placeholder row, so no
   firmware path dispatches into an empty row. A DSP load spans UI passes (`loader/mod.rs:693-717`), so the window is real.
6. **Load the new module** into the old record with run-time numbering: `settle` runs again over the live set plus the candidate, which gives
   kept keys their old numbers and new keys free numbers.
7. **Rebuild the selection views** and bump the listing generation counters, so open lists re-read.
8. **On a failed load**: the placeholder stays, the old file is not restored (the replaced design of hot reload decision 3, load beside and
   roll back, is out of scope: it needs twice the memory). The workbench reports the failure; reloading a good file recovers.
9. `invariants::check` after unload and after load (exists).

The planner check of step 1 is the only duplicate check on a run-time load, so E3 must not delete the per-service checks until E5 runs the
planner against the live set (E3, E5 and E5b land in that order, the per-service checks go in E5b).

## 4. Code structure

### 4.1 Folders

The target tree, the mapping from today's folders and what does not move are in [taxonomy](../design/taxonomy.md) "Repo layout". E8 does the move. No module id changes (rule 11).

Class and home of every image module (inventory of the 25 `adapters/` folders and `expansion`):

| Class | Modules (id) | Home |
| --- | --- | --- |
| service image half | machines, params, dynreg, dsp-voice | `services/machines/image-halves/` |
| service image half | fx, dynfx, fxpages | `services/fx/image-halves/` |
| service image half | sample-pool | `services/samples/image-halves/` |
| service image half | lfo | `services/lfo/image-halves/` |
| runtime platform | platform, ui, health, input, dsp-link, expansion, dyndsp | `runtime/platform/` |
| adapter | usb-bulk (usb-bulk-audio merged in a separate PR), plusdrive-write, sdhc-large | `image-modules/adapters/` |
| fix | overdrive, introblank | `image-modules/fixes/` |
| proof | dsp-stereo-proof | tests of `dsp-voice` |
| proof | storage-fault-proof, file-catalog-proof | `image-modules/proofs/` |
| not image | runtime, digikit-menu | `runtime/core`, `runtime/apps/digikit-menu` |

`dsp-stereo-proof` becomes test assets of the call sites it proves; its id stays. `file-catalog-proof` stays a proof (E1: the gate covers directory open and next through `plusdrive-write`'s lookups, but nothing else covers `path.size`). Proofs are in no patch set. `overdrive` may later move to a service or become a content module (O3).

### 4.2 Names

| Today | After | Where |
| --- | --- | --- |
| `role = "adapter"`, `type = "framework"` in every image module | one role per module: `platform`, `image-half`, `adapter`, `fix`, `proof` (table in 4.1; the full list is in the taxonomy) | `module.toml` of each module; layercheck `roles` |
| `Gate` (builder check) | `InitWindow` | `params/src/builder.rs` and users |
| `gate()` calls | `init_window()` | machines and fx `build.rs` |
| `services/params` | `services/rows` | crate rename; users in machines and fx |
| "adapter" in docs | "image half" or "platform" | ARCHITECTURE.md, LAYOUT.md, runtime-and-services.md, AGENTS.md |

Module ids keep the word they have (`core/dynreg` and so on); only folders, roles and docs change.

### 4.3 Service trait

- `fx_entry`, `guard_state` and `owned_ids` leave the trait (rule 12). They become service exports found by name hash
  (the handover's deferred item): `dk_fx_routes_entry`, `dk_machines_guard_state`, `dk_machines_owned_ids`. They are `[[import]]` entries of
  `sdk/sdk.toml` with `provided`, `wrapper = "rt"` and `internal = true` (in the export list and `gen_exports.rs`, in no header, document or module import
  table). `handlers::exports` looks them up with `find_export(name)` over the loader's export table; the menu and the MODULES reply keep their
  behaviour when the file is not loaded. The runtime's `kind-free` rule then also holds for the trait.
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
- Parked after a first attempt (finding 148 "The decisive check still open"): the run read the DSP table (types 0-5 map to S3 1, 3, 2, 4, 5,
  6), but a raw byte write did not reach the DSP, and the profile counted nothing because the daemon was built without a generated DSP core
  (a plain `cargo build` without `SHARC_GEN_DIR`: the stub traps on every instruction, `native-trap: no decoded instruction`).
  `sharc.profile` counts the native tier. The retry needs: the daemon built with
  `SHARC_GEN_DIR=out/native/dt2-audio/gen node tools/nx/rust-env.mjs --gen --variant sharc-profile -- cargo build --release -p digi-control --bin digi-emud --features sharc-profile`;
  a check that the core is present; a proof that `counted` is non-zero on a busy routine; `SHARC_AOT=0` for exact per-PC counts; and the
  filter type changed through the UI chooser. `tools/devtools/param_sweep.py` stays out of the repo until it has run once end to end.

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

E0c Daemon hardening (host only, no generated core needed; done on branch e0c/daemon-hardening). The E0a failure cost a session because nothing said that the daemon had no
DSP core.
1. The host holds no core any more (a core library named by `DIGI_SHARC_CORE_LIB_<DEV>`): a missing variable already failed the session. A library
   built without `SHARC_GEN_DIR` loaded and trapped at the first instruction; `core_for_device` (`packages/emulator/sharc-peer/src/dsp_core.rs`) now
   refuses it with "built without a generated core".
2. `sharc.profile` answers `engine_tiers` (fast, block, interpreted, idle skipped, traps since the DSP worker started), so a zero in `counted` is
   explained (docs/CONTROL.md).
Depends on: nothing. Do it before the E0a retry.

### E1 Proof adapters: coverage check

The proofs stay (runtime-and-services.md keeps them: no shipped test covers their paths). E1 only checks one of them; the moves are in E8.

What:
- `file-catalog-proof`: find whether a shipped device test lists a DT2 +Drive folder through the runtime's file calls (candidates:
  `transfer-live`, `samples-store-live`, the Files page tests). If one does, propose deleting the proof and its opt-in test, with finding 93
  pointing at the shipped test; an independent reviewer checks it is not the only proof. If none does, it moves to `proofs/` in E8.
- `storage-fault-proof` and `dsp-stereo-proof` stay; nothing to check.

Gate: static checks; no device run.

Done when: the decision on `file-catalog-proof` is written into this plan (4.1) and, if it goes, the deletion PR is merged.

Outcome: keep. `plusdrive-write-files` (gate, DT2) reaches directory open and next through `pdw.c` `lookup()`; no shipped test or caller
uses `path.size`. To delete the proof later, first add gate checks that list a populated +Drive folder and read a size. The `users` lists of
`cf.storage.fn.directory.open` and `.next` in `devices/fw/storage.toml` name `core/runtime`, which does not call them; E8 removes it.

Estimate: 1-2 h. Payoff: small. Confidence: high. Depends on: nothing.

### E2 Builder boilerplate from sdk.toml

Facts (read on `int/shared-gate`): `sdk/sdk.toml` has 26 `provided = true` imports (machines 12, fx 5, samples 9). 13 have pure forwarding
wrappers (`build::<rule(name)>(&ImportCore, args...)`, rule: drop `dk_`, then a leading `module_ctx_`; it fits all 13). The other 13 are
hand-written: machines `dk_param_get` and `dk_param_set` (`fw.rs`), `dk_param_on_click` and `dk_page_canvas` (`builder.rs`, they convert
callbacks), and all 9 samples wrappers (`glue.rs`). `[[export]]` is read by service-check (`svccheck.rs:426-451`) and by
`Module::provided_service` (`module.rs:188-198`, the `.dks` header's export list).

What:
1. `sdk.toml`: an optional import field `wrapper = "<dks module>"` (the file of a hand-written function, e.g. `fw`, `builder`, `glue`); allowed
   only with `provided`; a pure wrapper with a callback argument is a generator error. 13 imports carry it (or a `[[service]]` default for
   samples).
2. `patchkit gen` writes `modules/core/services/<s>/dks/src/gen_exports.rs` for each service with a `dks/` folder, in `sdk.toml` order: the pure
   `extern "C"` wrappers (`#[cfg_attr(target_arch = "m68k", no_mangle)]`, as today), a `const _: api::Dk…Fn` check for every export, and
   `pub fn exports() -> [(u32, *const u8); N]` (a function, because a function address cannot become a pointer in a `const`). Crate path by
   the convention `dkrt_svc_<name>`. `gen --check` covers the files.
3. `service!` (`svc-rt/src/lib.rs`) gains an arm `exports = $e:expr`; the literal list form stays (the service-check tests use it). The three
   `dks/src/lib.rs` call `exports = gen_exports::exports()`. fx `builder.rs` goes; machines `builder.rs` keeps its two callback wrappers.
4. `module.rs`: a service's exports are its `provided` imports from `sdk.toml` (`import_infos()` gains `provided`); the `[[export]]` entries
   leave the three `dks/module.toml`. Service-check refuses a literal `exports = [` and requires `gen_exports::exports()` when the service has
   provided imports.
5. `rust_api` writes `pub const PROVIDED: [(&str, &str); 26]` (service, import); `MACHINE_BUILDERS` and `FX_BUILDERS` in `assembly_tests.rs`
   filter it. `layercheck.py` builds the builder part of `RT_KIND_FREE` from `sdk.toml`; the kind names stay literal until a generated set is
   proven a superset.
6. Nx: the dks projects' build inputs must include `sdk/sdk.toml` (check with `pnpm nx:pilot show target inputs ... --check sdk/sdk.toml`).

Before building: confirm the loader and `bind.rs` do not depend on the order of the `.dks` export table; the generated order is `sdk.toml`
order, not today's `lib.rs` order.

Tests: `patchkit gen --check`; sdkgen host tests for pure and hand-written output and a bad `wrapper`; service-check tests rewritten (a missing
`gen_exports` call fails, a literal list fails, fills unchanged). Acceptance: copy the three committed `.dko.json` first, then
`patchkit objdiff OLD NEW --strict` for machines, fx and samples (code identical, names and placement ignored), equal export sets,
`pnpm nx:pilot run core/runtime:test-assembly` and `mise run objects-check`.

Done when: adding a builder call touches `sdk.toml`, the service's `build.rs` and its tests, plus the generated files.

Estimate: 4-6 h. Payoff: every later API. Confidence: high. Depends on: nothing (after the gate merge, to avoid conflicts in the service
crates).

### E3 Keys, the trait, and the role check

Facts (read on `int/shared-gate`): `GENERIC_KINDS` is only `[KIND_LFO_SHAPE]`, so `keys_clash` (`plan.rs:367-434`) checks duplicates only
for LFO shapes; for MACHINE and EFFECT it checks only that the kind has an owner. `keys_clash` never compares a table with itself (a repeated
`[[record]]` key is refused by patchkit, two machine or effect ids that hash alike are not). Assign rule 4 lives in `sticky.rs:147-152` and
`:200-205`. No packer writes KINDS or SLOTS for a content module. There is no `dkrt_export`: the loader's `Exports` table
(`loader/exports.rs`, `resolve(hash)`) is the lookup.

What, as separate commits:
1. `dkneeds` `Keys::first_shared_key(&self, other) -> Option<(kind, key)>` and `Keys::first_repeat(&self) -> Option<(kind, key)>` next to
   `has_key` (`keys.rs:534`), with host tests. The planner and E5's live-set check use them. (The workbench fit check calls `pk_fit` for capacity only; the key check there is E5b item 4.)
2. `plan.rs`: `keys_clash` keeps the owner check and uses the two functions for every kind (the `is_generic` condition goes; `is_generic`
   is deleted if nothing else uses it). `settle` hands the services the keys of the modules the plan accepted only (`settle_ctx`), so a module refused with `E_PLAN_KEY` takes no number. Expose `key_refusal(t, others) -> Option<Reason>` for E5. Plan tests: duplicate machine key, duplicate
   effect id, the same module twice, the same local id in two modules (accepted), a forced hash collision, a key repeated inside one table.
3. `dkrt_sdk::sticky`: `Range { lo, hi }` and map slices instead of `DYN_TYPE_*` and `[u32; MAP_N]`; machines passes its range and map, and
   `machines.types` stays byte-identical (the existing format test guards it). New case with another range. Then delete rule 4 (the `first`
   loop, the fresh-number branch and its test `a_repeated_key_takes_a_fresh_number_and_is_not_recorded`) and document that `assign` takes
   unique keys, with a debug assertion.
4. `role_ok` (`role.rs:256-271`): role 0 also refuses KINDS and SLOTS; unit cases next to the existing ones.
5. LFO shape keys: `[[record]]` takes a local `id` string; patchkit puts `hash24(<module id>:<id>) << 8` (low byte 0) into KEYS and generates
   a per-module constant `DK_KEY_<ID>` (as `module_ids.h` gives machine ids) that `module.c` writes into the record body, so the key has one
   source. The lfo service keeps `id != 0` and `E_TYPE_USED` until E5b. The fixture `lfo-shape-dup` is deleted (it only repeated shape id 7); the lfo
   and refusal device tests make the second owner of a key by renaming a copy of the `lfo-shape` file.
6. Trait (4.3), last and on its own: `fx_entry`, `guard_state`, `owned_ids` leave `Service`, `ServiceRow` and the thunks. Each service
   declares an export (`dk_fx_routes_entry`, `dk_machines_guard_state`, `dk_machines_owned_ids`, in `dks/src/rt.rs`): a plain `extern "C"` function with no core
   argument (the `.dks` code reaches the core through `ImportCore`, as the builder exports do). The runtime adds `find_export(name)` over
   `Loader.exports.resolve`; `dkrt_fx` (menu FX ROUTES) and the MODULES reply call it and keep their behaviour when the export is absent
   (`E_NOT_FOUND`, 0, none). `SVC_ABI` changes once; every `.dks` and the runtime object rebuild (`objects-check`).
7. One assembly case: two packed fixtures with the same machine key refused by the boot plan. Doc updates (service.rs and handlers.rs doc
   comments that name the three methods).
8. The per-service duplicate checks (`E_FX_ID_DUP`, the lfo `E_TYPE_USED` path) stay until E5b; E5b deletes them with their refusal-table rows. At boot the plan now
   refuses the repeated key first (`E_PLAN_KEY`), so the commit test of a repeated effect id expects `E_PLAN_KEY` and the `E_FX_ID_DUP` row of the refusal table is a host
   row (the fx unit test of `check_commit`).

Scan before landing (done, from the manifests of 57 modules, with the hash of `register.rs`): no two shipped modules or fixtures share a machine key or an
effect id, and no module repeats a key. Two test paths relied on the old gap and changed: the commit test that repeats an effect id (now `E_PLAN_KEY`), and the
capacity set `fullSet` (`tests/regression/runtime/synth.ts`), whose refused voice module `cap/full-y` pinned the same type as `cap/full-v` and now pins `typeFirst - 1`, a type outside the run-time range that no reference module or V holds, so it still reaches `E_DSP_ROWS`.

Tests: as listed per commit. Device subset (DT2 and DN2): `C:loading`, `S:machines`, `S:lfo`, the runtime lfo and refusal tests, the tests
that read FX ROUTES (fixtures `dyn-fx-gain-send`, `dyn-fx-gain-sendonly`) and the MODULES reply, plus `gate-smoke` and `gate-pins`.

Done when: every kind's keys are checked by the one rule; the trait names no kind; layercheck `kind-free` covers the trait.

Estimate: 1.5 days. Payoff: one conflict rule for all kinds, a kind-free trait. Confidence: high. Depends on: E2 (both touch `module.rs`
and the service crates).

### E4 Service contract suite

What:
1. Route the row writers of machines and fx through `FwMem` (`core.wr`/`core.rd`, new `rd8`, `wr8`, `rd16`, `wr16`, `wr_half`) instead of raw
   addresses (machines `records.rs`; fx `fxpage.rs` and `fxsend.rs`), so a host test reaches machine, page, effect page and send rows. Done;
   objdiff of `machines.dko.json` shows 5 changed functions, of `fx.dko.json` 9 changed and one new (`ImportCore::rd16`).
2. The shared lifecycle driver and `contract.rs` in `modules/core/runtime/test-support/src/` (3.5). Done.
3. A Rust call list per fixture, in the service's `tests/contract.rs`. Done; host compilation of the `module.c` files is not planned.
4. `tests/contract.rs` in machines, fx, lfo and samples with DT2 and DN2 fixtures. Done.
5. Fix what the suite finds. Found: the fx and machines rows were unreachable on the host (item 1); the machines `rust-tests` did not compile
   on e3/keys and two of its tests shared a static without the lock (fixed). The first version of the suite compared only what a module wrote
   itself, so it could not see a release that left words behind; case 7 and 8 and the `a2` and `bad_late` modules were added for that. Found with
   them: the fx `remove` published the kit table (`DKRT_FXP_DEFS`) before it cleared the row's defaults, so the defaults of an unloaded effect stayed in
   the table (fixed in `fxpage::remove`, unit test `an_unloaded_effect_leaves_no_defaults_in_the_table_of_the_kit_hooks`). Left on purpose, named in
   the fixtures' `kept` with the value each word may hold: the machines and fx `remove` give a row back as a free row (the empty string or empty representation for
   names, the track level code, the reserve template and slot of a parameter row, `PG_FREE` of a page row; the machines `O_POS` of a hidden row is not written
   and is no order; the fx list buffer entries at or past the count; the MIX word of a released DSP effect row is `MIX_ONE`, in `kept_dsp`). The fx `remove` sets the name and title pointers of its page row to the service's empty string
   (`DKRT_FXP_EMPTY`, 4-byte aligned, the layout `make_rep` writes for "": length 0, capacity 0, refcount -1, NUL; found by the suite: they pointed to the strings of the effect that left). Mutations of each service's `remove`, `dsp_clear` or
   `dsp_release` (one item of each kind left behind: row words, a page row, a send cell, an IDS halfword, a DEFS halfword, a DSP row field, an owner, a
   DSP row's off word, a link value, an unload that writes the module's own value back into a row word, a DSP release that leaves an owner byte that has the same value for `A` and `a2`) fail the suite.

Tests: the suite itself (unit). Check with `pnpm nx:pilot show target inputs <service>:rust-tests` that each service's target hashes the
test-support crate and the fixtures it reads.

Done when: all four services pass all eight cases on both profiles.

Estimate: 2-3 days. Payoff: the reload proof for every service, and the T1 `Service` contract. Confidence: medium-high (step 1 touches the
machines writers on the device path; the device subset of E3 or E5 covers it). Depends on: E3; branch `nx/dks-tests` (the service
`rust-tests` targets).

### E5 Reload of content modules

This is hot reload S1 (code-only modules) and S4 (machine and fx modules) of [digikit-hot-reload.md](digikit-hot-reload.md), for reload
(3.6), scoped to day-to-day machine work: the module's code, DSP and keys change; its menus and its own effect routes do not. It starts
after R12-8 merges: R12-8 builds the shared unload lifecycle and late teardown in the same `Loader` code (r12-8 sections 2.6, 2.7) and moves
DSP row state into the services (step 7). The two-block DSP wait is new work, done here. Menus, fx reload, the workbench key check and the
cleanup of the old duplicate checks wait in E5b.

Before E5 starts (about 2 hours, no code): map which code of the archive tag `digikit-hot-reload-v1` carries over to the R12-8 layout
(`loader::unload`, `registry::in_use`, `refresh_view`, `CMD_UNLOAD`; findings 32 and 33 proved machine unload on the emulator).

What:
1. Async job. A SysEx command runs on the MIDI task, but the loader, registry, DSP state and services are UI-task state. `CMD_RELOAD`
   only queues a job; `reload_poll()` in `dkrt_tick` runs it, one step per pass when it must wait; `CMD_RELOAD_STATE` polls the result. This
   is the RESTART pattern (`admin_cmds.rs`). One job at a time; a second `CMD_RELOAD` while one is queued answers `E_BUSY`.
2. Validation before unload: the new file is staged and checked (role, ABI, DSP checks; split out of `load_inner` as
   `check_file`), then the planner runs over the live set with the candidate in the old module's slot (`key_refusal`, E3). Keys check:
   `Keys::first_missing_key(old, new)` (`packages/dkneeds/src/keys.rs`) refuses a new file that lacks a key of the old file (`E_RELOAD_KEYS`). The new
   file must have the old file's module name (`E_FORMAT` otherwise: another name is another module). The planner is `plan::plan_live`: the loaded providers
   are not judged again (their rows are in the service table, which the boot's judgment refuses as a second owner) and the capacity counts that the live
   modules own start at 0 (`live_caps`), because the plan counts every live module again. The checks of the new file run as a load runs them: `reload_check` sets `CoreApi::boot_index` to the module's selection index and numbers the file's keys first (`settle` over the live set with the file in the old slot; the old keys keep their numbers), because the services' DSP checks resolve a voice's key to its type by that index and the numbering; and `CoreApi::live_op` is the owner of the module being reloaded for the whole job, so the machines service leaves the module's own voice rows out of "used by another module" in `check_types` and counts them as takeable in `check_rows`. (Without this every reload of a machine with DSP was refused with `E_DSP_TYPE`.) Nothing is unloaded before both pass.
3. Transport and state refusal: one function, in this order: safe mode; the firmware transport function returns non-zero (`cf.project.fn.transport_state`, declared in `devices/fw/project.toml`; `Platform::play_state`; a platform that cannot read it refuses);
   card operations; the module built its own menus; then each service's `Service::teardown_refusal(core, owner)` (required, like `validate_records`; the fx service answers `E_RELOAD_ROUTED` for any module with a published effect row, because E5 has no fx teardown (route 0 is the master chain; E5b relaxes it), machines and lfo never refuse, samples never refuses: whether a pool entry is in use is not cheap to know, a follow-up). Refusing while the machine selection view
   is open is not needed: the list rebuild (item 9) is unconditional.
4. Fixed record index. A reload loads into the old record (`Slot::Module(rec)`), so owner ids (`rec + 1` in machine rows, DSP rows, menus,
   `sel_of_rec`) do not move.
5. Progress mask in `unload_rec` (`torn` per record: fini done, undo done), so a second call after `E_BUSY` does not run `fini` or the
   service `undo` twice. The steps before `fini` already repeat safely. On a timeout the old module stays live, so nothing is released: the reload restores the old tick and leaves the DSP rows as they were. `release_range` needs a confirmed quiesce (`modules/core/runtime/src/rs/src/dsp/mod.rs:536`), and `dsp::release` latches `PH_RETAINED` for the session when `quiesce` fails (`dsp/mod.rs:540-541`), after which every later load sees `retained()` and returns `E_BUSY`. The reload timeout path never calls `release`.
6. Drain wait, before any step that frees or clears something (`Loader::reload_quiesce`): the tick stops (address kept), the module's SysEx commands answer
   `E_BUSY` (`module_cmds::set_draining`), its sinks and the span clock are cleared under the interrupt mask, and the block count is noted. The in-flight counter
   (`module_cmds::begin_cmd` / `end_cmd`, counted in the same masked step as the handler lookup) is the number of its commands that run on the MIDI task.
   The step returns `E_BUSY` until that counter is 0 and `reload::DRAIN_BLOCKS` (2) blocks passed (`Platform::blocks`, no spinning; a platform with no block
   clock skips the wait). R12-8 gives no voice stop and no block wait; unloading then clears the voice rows, which cuts the voices. Because nothing is freed
   or cleared before the wait ends, `reload_abort` (a timeout) gives the old module its tick and commands back and leaves its DSP rows as they were. Its channel sinks stay released after an abort and are not set again (the debug module loses its logging until the next boot); this
   is kept on purpose, because the module's `init` is the only place that sets them. The legacy single sink and tick of `dlog` stay set: they point into code that stays
   mapped until the free, and `sink_drop` clears them just before it. DSP instruction cache on range reuse stays open (hardware only); the device test with a changed constant proves the new code runs.
7. MISSING placeholder, written only while a reload runs (`CoreApi::live_op`): a plain unload and the boot's failure path keep their behaviour (a free voice
   row, a gap machine row). A kit names a type number, not a key. A gap row plays type 0's sound today, so the placeholder must be silent. The machines
   service does two things, on both devices (no capability separates them; the dispatch of both devices reads word 10 of a voice row as "the disabled
   type", `dispatch-dt2.s` `_dv_gdis`, `dispatch-dn2.s` `_dv_contract`):
   - `remove` writes the machine row as a placeholder: the list name stays empty (the selection list leaves it out), the long and short names (descriptor and
     names table) are "MISSING" and "MISS" in the row's own text buffers, `own_dsp = 1` (DN2: finding 33 saw silence without fault).
   - `dsp_clear` (`dsp::clear_rows`) parks each published voice row instead of freeing it: word 10 takes the row's type, the pc becomes
     `PLACEHOLDER_PC`, the type word stays, and the row's owner becomes `dkrt_abi::dsp::PLACEHOLDER_OWNER` (0xfe, no record: the table check accepts it, and the
     unload does not free it). The new version's claim of a voice of the same type takes the placeholder row itself (`dsp::claim`, `row_pub` 2), and the row
     publication clears word 10 and writes the new words; that step carries the mark and the undo (a failed publication disables the row again), so the window
     is silent from the unload to the publication. `check_rows` counts such a row as free for that voice and `check_types` does not refuse its number. When the
     reload ends, with the new version loaded or not, the job calls `Service::live_done` (machines: `free_placeholders`) and every placeholder row nobody took
     over is freed, so no row of owner 0xfe outlives its reload. A placeholder row uses one of the 16 type rows for the length of the reload.
   One rig case on each device proves it: a row with word 10 pre-set to its type renders silence from a routine that sounds, without a fault
   (`modules/core/adapters/dsp-voice/tests/dsp-guard.rig.test.ts` for the Digitakt II dispatch, `dsp-voice-dn2.rig.test.ts` for the Digitone II one). The machines
   contract lists the placeholder values in `kept`, `kept_dsp` and `kept_caps`, and the `disabled` words of the rows that count as off.
8. Numbering: re-run `settle` over the live set plus the candidate, after the old module is torn down and the new file's header table is kept in its slot
   (`Loader::reload_load`; the keys of every loaded module and of the new file, in selection order). Live modules keep their numbers (rule 1) and new keys
   take free numbers; `dkrt_sdk::sticky` does not change. A service `settle` must be idempotent: contract case 9 (`settle_over_the_live_set_changes_nothing`).
9. Machine list rebuild after every reload, unconditional (the archive's behaviour, proven on the emulator only). The firmware builds the machine selection
   view's item list once and keeps it. `core/dynreg` has the view hook again (`dynreg_view_hook`, hook `cf.machine.hook.view.build` on the function
   `cf.machine.fn.view.build`, declared in `devices/fw/machine.toml` with the archived hash on DT2 1.17 and DN2 1.12): it stores the view in `dynreg_view`, and
   words 48 and 49 of `dynreg_info` (`dynreg::w::VIEW_VAR`, `VIEW_BUILD`) hold that variable and the builder. The machines service rebuilds the list in
   `Service::live_done`, which the job calls at the end of every reload that got past the checks, success or failure (after a failed load the cached list would keep
   an entry the rows no longer have): it calls the builder with the stored view (`refresh_view`; nothing while the view does not exist). The machines service writes the machine
   rows in `apply_record`, as at the boot: the placeholder voice row of item 7 keeps the type silent until the module's row is published, so the window needs no later row write.
   A module with no DSP gets `on_loaded` from the job. The stored view pointer is the risk of this step; the device gate proves it (archive: emulator only). Counters
   `unloads` and `reloads` at the end of `DkrtBootInfo` make open lists re-read (`unloads` counts every module teardown; the job counts `reloads` for every reload that loaded the
   new file or tore the old one down).
   End state of a reload that fails after the old module was torn down (the load fails, the DSP part fails, or it stalls and is cancelled):
   the module is not loaded, as after a plain unload. The boot record says FAILED with the code and `loaded` drops; the machine rows that read MISSING are written as gap rows
   (a MISSING row with no voice row would play the first stock machine's sound); the placeholders are freed; the list is rebuilt; `RELOAD_STATE` carries `unloaded = 1`. A
   DSP part that does not end in 4 s is cancelled through the DSP loader's own failure path (`Loader::cancel_dsp`), so no transfer outlives the job. Every wait also ends after
   1800 UI passes, whatever the block clock does. `unload_rec` answers `E_BUSY` only before `fini` (a file or I/O step of the module still runs, or the DSP rows cannot be cleared), so a teardown that does not end gives the old module back; a DSP loader that cannot clear the rows is retained for good, and from then on every reload is refused at the queue with `E_RELOAD_RETAINED` (restart the device). The DSP wait has one budget for a stalled load: it starts again whenever the loader moves (an image or a row goes out, a load starts), so a healthy slow load of many rows is not cancelled.
10. `dk:dev` and the workbench use `CMD_RELOAD` for any content module; the emulator byte swap remains only for the runtime image and the UI
    layer (`core/ui`, `core/digikit-menu`). Document the command and its refusals in `docs/TESTING.md` "Fast iteration".

Design requirement "Reusable reload": the runtime holds the generic reload work once: the job, validation before unload, the refusal order, the in-flight counter, the two-block DSP wait, the progress mask and numbering over the live set. Per-kind work sits behind required methods of the `Service` trait (for example unload records, install records, refresh view, `teardown_refusal`; `on_loaded` exists already), the same pattern as `validate_records` (PR #614). The reload job has no code for one service. Machines implements the methods in E5, fx in E5b, lfo shapes and filters later. Boot loads modules and the services they need; reload swaps content modules only, and services stay loaded (O5). A new module version that needs a service that is not loaded makes the reload load that service first (a plain load, not a replace). A service that no module needs any more stays loaded until the next boot. The port file ([port-file.md](port-file.md)) is a service too: reload never replaces it.

Plan corrections the E5 design found (the text above already reflects them):
- SysEx runs on the MIDI task, hence the job.
- R12-8 gives no voice stop and no block wait. Unloading clears the voice rows; the wait is new.
- Event tables by pointer swap (`svc_rows`) is hot reload S3 (service files), not E5.
- Kit data names type numbers, not keys; the placeholder is a row for the numbers a module held.
- Menus keep pointers into module code (`desc`, `fill`, `arg`) even when closed, so refusing "while a view is open" is not enough.
- No firmware predicate says which view is on top. The rebuild is unconditional until E5b answers it.
- The DSP placeholder window lasts about 200 ms past commit, because machines publish rows at commit and the voice rows follow; the placeholder voice row (item 7) keeps the type silent until the module's own row is published, so the machine rows are written at commit as at the boot.
- E3/E4 and R12-8 both rewrite five files (`loader/mod.rs`, `abi/src/service.rs`, `handlers.rs`, `svc-rt`, machines `records.rs` and `assign.rs`). The second to merge pays one rebase and an object rebuild.
- `PH_RETAINED` latches for the session after one failed `quiesce`, so a reload timeout must not call `release` (item 5).
- `clear_rows` frees every row of the record on an unload; during a reload it parks each published row as a placeholder instead, and the reload's end frees the rows nobody took over (item 7).
- E5 changes the image (dynreg words) and `SVC_ABI` (`live_op`), so its PRs are shared inputs of every device test: the merge proof is a named subset on both devices. The port file stack ([port-file.md](port-file.md) PR2) changes `SVC_ABI` again and merges after E5. The port file is a provider, not a service; it is the one provider that is no service file. Each change makes the runtime refuse old service files with `E_SVC_ABI`, and Transfer replaces them.

Commit split (host checks per commit; device runs only at the end, one gate):

| # | Commit | Size |
| --- | --- | --- |
| 0 | Spike: archive map | 1 h |
| 1 | `Keys::first_missing_key`, tests | 2 h |
| 2 | Transport predicate (anchor search on both images, `devices/fw`, `Platform::play_state`, fakes) and `reload::refusal` | 4 h; the anchor search may add 2-4 h |
| 3 | Loader: `Slot`, `Phase::Reload`, progress mask, `check_file`, plan over the live set, keys check | 8 h |
| 4 | In-flight counter, masked span clock, two-block DSP wait | 4 h |
| 5 | Run-time numbering over the live set | 3 h |
| 6 | MISSING placeholder (DT2 rig case, DSP row lifecycle), contract `kept` update | 8 h |
| 7 | Machine list rebuild: dynreg words, `devices/fw`, `refresh_view`, `on_loaded` rows, `live_op`, counters | 6 h |
| 8 | Protocol: `CMD_RELOAD`, `CMD_RELOAD_STATE`, `reload_poll`, client | 4 h |
| 9 | `dk:dev`, `wb transfer`, docs, device test file, soak | 6 h |

Tests:
- Unit: runtime host tests on `FakePlatform` for the refusal order, validation before unload (a refused candidate leaves the old module live),
  the in-flight counter, resume after `E_BUSY`, run-time numbering, placeholder words, unload then load leaving `invariants` clean, the job
  state machine.
- Assembly: one case, a real packed fixture loaded, reloaded and reloaded again through the plan, plus a refusal (dropped key).
- Device (both devices, one file in `core/runtime-tests`), one test per kind: (a) a code-only fixture reloaded; (b) a machine fixture with
  DSP reloaded with a key held: silence in the window, new pitch after reload; (c) reload refused while the transport plays. An fx reload
  test comes with E5b.
- Soak: the existing `test/sine`, built twice from the same source with a define that changes its pitch (same module id and keys, no new
  fixture identity). The soak reloads between the two builds with a note held and checks each cycle that the pitch switches, `invariants`
  stay clean, memory and DSP rows do not grow, and no fault is logged. About 5 cycles in the gate and 20 cycles in the full tier, with
  `DIGI_FLUSH_CHECK=1`, bounded with `--seconds`.

Done when: `dk:dev` reloads a machine module with no boot, on both devices, and the soak passes.

Estimate: 2-3 days, upper end, at the measured agent pace (about 50 agent hours of edits and host checks in 10 commits, with reviews and one E3/E4 plus
R12-8 rebase); device wall time about 40 minutes for the named subset on both devices plus the soak. Payoff: removes the 5-10 s boot from every machine
iteration. Confidence: medium-high; the archive proved the machine path on the emulator (findings 32, 33); the list rebuild call on a
stored view pointer is the main risk (emulator proof only).
Depends on: E3, E4, R12-8 merged. (E0b is done.)

### E5b Reload follow-ups (deferred from E5)

What:
1. `core/ui` menu owner count: record the owner of each menu at `dk_ui_menu_new`, export `dk_ui_owner_menus(owner)`; then allow reload of
   modules that built menus (the refusal of E5 item 3 goes). Menu teardown stays later work.
2. Open-view predicate spike: find the function that says which view is on top (neighbours of `cf.view.fn.view_controller_push` in
   `devices/fw/view.toml`). Then skip the unconditional list rebuild when the selection view is not open.
3. Fx placeholder for an absent effect id: what the FX page shows for a track that names an effect id with no row (one static read), then
   allow fx reload while the effect is on a route, and add the fx reload device test.
4. Workbench key check: a `pk_keys_check` wasm export next to `pk_fit` (`Keys::first_shared_key`, `first_repeat`, `first_missing_key`), called by
   `fit.ts`, shown by `transfer-backend.ts` before the upload; one wasm test and one Vitest case.
5a. The fx reload must leave the module's own effect rows out of the fx service's `dsp_check_rows` (`fx_owner`, as the machines service does for voice rows with `live_op`).
5. Delete `E_FX_ID_DUP` and the lfo `E_TYPE_USED` path with their refusal-table rows, after the reload planner case of E5 is green.
6. The 20-cycle soak and the full project round trip (save a project that uses the module, reload, load the project, same sound), if E5 did
   not carry them.

7. Reload that needs a service that is not loaded (the "Reusable reload" rule says the reload loads it first): E5 refuses with `E_MISSING` (`plan::plan_live`). Loading it
   at run time is the boot's provider stage run again: `discover::find` for the new file's unmet hard needs, a plan over the loaded providers, a free provider record, the
   installer's hook and slot writes (firmware code is patched while the system runs, under one mask) and the provider's DSP part after the boot, then `settle`. About 8 h of
   edits and host tests, and its own device proof (a hook installed at run time on both devices); reload never replaces a service.
8. The soak's pitch switch: two builds of `test/sine` that differ in one constant (same module id and keys), reloaded alternately with a note held, each cycle checking the pitch;
   E5's soak reloads the same file (rows, records, bytes and faults) and a build helper with a changed constant is still to be written.

Estimate: 1-2 days. Depends on: E5.

### E5c Live enable and disable of content modules

E5 reloads a module that is loaded. E5c adds load and unload of a content module at run time, so the user can tick and untick a module in the
MODULES list and see the change without a restart. Both are built on E5's job: reload is already "the unload half, then the load half"
(`reload.rs`), so one job with three operations replaces three mechanisms.

Decisions (taken with Em):
1. Untick while a track uses the machine: a plain unload. The live state equals the state after "untick and restart" today: a gap row, which plays the
   first stock machine's sound. "MISSING and silent for an absent module everywhere" is a separate later item.
2. A needed service is not loaded: the plan answers `E_MISSING` and the menu offers "restart to apply". Loading a service at run time waits for port file
   step 3 (slot fill). E5b item 7 as written (writing hook jumps while the device runs) is not built.
3. The menu applies a change as one batch when the user leaves the MODULES list: the unloads first, then the loads.
4. A load is refused while the transport plays, like an unload and a reload (one rule).

What:
1. One job, three operations (`reload::Job.op`): load, unload, reload. The states keep their numbers (IDLE, CHECK, QUIESCE, UNLOAD, LOAD, DSP, DONE), so
   `RELOAD_STATE` keeps its bytes. Load: CHECK, LOAD, DSP (if the module has a DSP part), DONE. Unload: CHECK, QUIESCE, UNLOAD, DONE. Reload: as E5.
   Every job that passes CHECK ends with `finish` (`Service::live_done`: the machine list is rebuilt, placeholders freed, counters).
2. No new two-phase protocol across services. The loader already sends each lifecycle step to every loaded service by owner id, and its undo is the
   rollback: a load that fails ends as `fail` leaves it, a failed DSP part ends through `take_down`.
3. Loader: `Phase::Reload` becomes `Phase::Live` and accepts `Slot::Free` as well as `Slot::Module`. `live_check_load` runs `check_file(path, Slot::Free)` and the planner
   over the live set with the file at its selection index (`live_caps(own = (0,0))`); `live_load` keeps the file's header table at that index, runs `settle` over the
   live set plus the file and loads into the first free record. `pick_record` answers `E_LOADED` for a name that is loaded.
4. Selection index of a load (`boot::pick_index`): the index the full path had at this boot, if no module lives there (the boot's load loop writes a 32-bit hash of each selection path once, `BOOT_PATH`; a live load never writes it, so a module gets its index back after another module used a new one); else the first index below the count that no module lives at and no boot path owns; else a new
   index (`count + 1`); `E_SEL_COUNT` at `SEL_MAX`. `BOOT_NAME` stays the display name. The boot record (`states`, `results`, `needs`), `BOOT_NAME`, `tables`, `accepted` and `sel_of_rec` use that index, as at the boot. A load that fails puts the boot record row and the count back as they were (UNLOADED stays UNLOADED; an unused index is SELECTED again).
5. Unload end state: the record is free, `accepted[k]` and `sel_of_rec[rec]` are cleared, `loaded` drops, the state is the new `STATE_UNLOADED` (8, "taken down at run time"), the services' `live_done`
   ran. The sticky numbers stay, so a later load of the same file gets the same numbers. Unload runs no settle. A load that fails ends as it began: the index is free again.
6. Refusals, one order (`reload::refusal`):

   | Rule | load | unload | reload |
   | --- | --- | --- | --- |
   | safe mode, DSP retained, transport not stopped, upload open, card busy | yes | yes | yes |
   | the module built menus (`E_RELOAD_MENUS`); `Service::teardown_refusal` (an fx on a route: `E_RELOAD_ROUTED`) | - | yes | yes |
   | file checks and the plan over the live set (`E_MISSING` for a service that is not loaded) | yes | - | yes |
   | same name, keys superset (`E_RELOAD_KEYS`) | - | - | yes |

   The `E_RELOAD_*` codes keep their numbers and also answer an unload and a load.
7. Numbering: load and reload run `settle` over the live set plus the candidate; unload runs none.
8. Services: no new method. `CoreApi::live_op` stays the owner of a module that is being reloaded (a plain unload and a failed load must write gap rows, not placeholders, so they leave it 0).
   `live_done` runs at the end of every job that passed its checks. A multi-service module (for example a machine with sample pool entries) needs no service list: each
   lifecycle step goes to all service rows in table order, and the teardown runs in phases (`dsp_clear` of all, `fini`, `remove` of all in reverse, `dsp_release` of all).
   One host case covers the edge: `dsp_clear` runs on every service even when one fails, and the abort that follows leaves every service's rows as they were.
9. Protocol: `CMD_LOAD` (0x08) and `CMD_UNLOAD` (0x09) take the body and give the reply of `CMD_RELOAD`; `RELOAD_STATE` serves all three (capability bit `LIVE`, 19) and ends with a byte `op` (1 load, 2 unload, 3 reload; an old runtime sends 7 bytes). C calls for the
   menu: `dkrt_live(op, path)` (queue a job on the same `Job::queue`) and `dkrt_live_state(out)`.
10. Clients: `DkSession.load`, `unload`, `loadAndWait`, `unloadAndWait` (TypeScript, with `liveModules` in `dktransfer.ts`), `Uploader.load`, `unload`, `load_and_wait`, `unload_and_wait` (`tools/device/dkupload.py`), `wb transfer load|unload PATH`. The Rust `dkupload` crate has no uploader method for reload either: it carries the generated command and capability constants only.

PR 1 (E5c-1) is items 1 to 10 without the menu. PR 2 (E5c-2): `menu.c` `apply()` keeps the save, queues the changed rows through `dkrt_live` (unloads first), polls with the per-pass
callback and restarts directly only for a route change or to leave safe mode. The user sees "APPLY n CHANGES?"; rows read LOADING; a refusal or failure shows "<NAME> NOT APPLIED" and
the status text, then "RESTART TO APPLY?" ("STOP PLAYBACK" for the transport refusal). New `status_text` rows -83 to -87. The same PR maps the boot results by the saved selection index
(`g_sel_of`; read results by path) and drops the `g_set[k]` settings-menu cache when the `unloads` or `reloads` counters change. The menu is an image change: named-subset gate.

Tests: job unit tests over the scripted `Ops` for each operation and each refusal; loader tests (load into a free record with index allocation: a path seen at boot, a new path, `SEL_MAX`;
the unload end state; a load after an unload keeps the sticky number; the `dsp_clear` abort case); assembly cases in `core/runtime:test-assembly` (a packed fixture loaded at run time, unloaded,
loaded again). Device proof (file `modules/core/runtime/tests/live.device.test.ts`): a code-only fixture loaded outside the boot selection; `test/sine` loaded, then a note with a pitch report;
unload with a note held; load and unload soak (5 cycles, 20 in the full tier); `E_MISSING` with no machines service; a sample-pool fixture loaded, a sample bound, unloaded with the voice
sounding, loaded again. Plus C:loading, gate-smoke and gate-pins.

Estimate: PR 1 12 to 16 h of agent editing time, PR 2 6 to 8 h. Depends on: E5.

### E6 LFO shapes

What:
1. Hook contexts (sites in the LFO waveforms finding, both devices):
   - Interrupt sites, as an image half (a stub with a slot in `services/lfo/image-halves/`; new image hashes, patch sets updated): the tick's wave
     clamp (DT2 `0x40139698`, DN2 `0x40137b56`) and its function-table call and RAND compares (DT2 `0x40139804`, `0x4013986c`), and the second
     function-table reader with its own clamp (DT2 `FUN_401392d4`, DN2 `FUN_401376bc`). Without the clamp hook a wave of 7 or more becomes RAND
     before the table call.
   - UI-task sites, as `lfo.dks` hooks (B1 bind resolves the addresses; routines in `services/lfo/dks/src/hooks-ui.s`, sites in `devices/fw/lfo.toml`,
     finding 149 "The panel hooks"): the name formatter (`wave_name`: DT2 0x400e1b68 and DN2 0x400e3610, four characters; `wave_name3`: DN2
     0x4000766c, the first three characters). The maximum of the WAVE parameter is no hook: the service writes the max word of the three WAVE rows (codes 79, 89 and 99) in the
     RAM copy of the parameter table that `core/params` makes, to the highest loaded wave number << 8, and puts the stock pair back when the last shape is gone (E6c), so
     with the service absent the maximum stays 0x600. The firmware's range accessor (DT2 0x400da368, DN2 0x400dc154, 172 callers on DT2 and 191 on DN2, among them the live-set
     clamp the encoder reaches) reads that copy. No caller scales a value onto 0..max (finding 149, "Callers of the T1 range accessor"); the MIDI CC and NRPN clamps see the new maximum, so with a shape loaded
     a CC value above 6 selects a shape number up to the highest one instead of clamping to RAND. Gaps: the maximum follows the highest loaded number, so a number below it
     whose module is not loaded is reachable; the page shows "----" ("---" on the three-character formatter) for it and the tick plays TRI. DT2 binds two sites, DN2 three (installed sites DT2 18, DN2 22 of 48 before the graph site below; table in [port-file.md](port-file.md) section 5).
   - Graph (decided, O7): see item 1b.
1b. Graph (option A of the static design; finding 149 "Graph"). Built. The 28-byte elements of the seven RAM vectors are `Bitmap` objects (1-bit,
   column-major tiles of 28 x 15 pixels, both devices), and the cell blits them. A shape graph is a vector of four tiles.
   - One UI-task hook site per device, the `addi.l #BASE,D6` before the vector accessor call: DT2 0x4010c83c, DN2 0x4010e1b0 (a `cf_func` rule in
     `devices/fw/lfo.toml`: `cf.lfo.site.graph_vec`, offset 0x24e of the cell, the hash is the same on both images and on DN2 1.11 and DT2 1.16; hook
     `cf.lfo.hook.graph_vec`, `tail = true`, routine `lfo_ui_graph` in `hooks-ui.s`). The hook reads the unclamped wave at 0x18(A6) (the Ghidra label
     `Stack[0x14]` is 4 less than the displacement), and for a wave 7..14 calls `dk_lfo_graph(wave, vtable word of A2, &flag)`; a non-null vector becomes D6
     and the unipolar byte goes to -0x11(A6). Every other case, and an interrupt context, runs the displaced `addi` from the bind tail. The
     shape path clears the condition code register, which is what the stock `addi` leaves.
   - The hook saves and restores D0, D1, A0 and A1 (strict liveness: only N, Z, V, C are dead after the site; the gcc model is not used).
   - The service builds, at `apply_record`, one vector header and four Bitmaps (vtable word copied from the cell's surface Bitmap by the first lookup,
     width 28, height 15, one word per column, pixel pointer, mask pointer, owner byte 0) per shape, from the 32 samples (`graph.rs`): per column the
     shape at phase `c / 28` (linear interpolation of the samples), `row = 7 - round(7 v)`, vertical fill to the middle of a step, a full line at a jump.
     Element 2 is `v(t)`, element 3 `v(-t)`, element 0 `-v(t)`, element 1 `-v(-t)` (the stock pairing; the cell chooses by the signs of the LFO speed and
     depth), so a shape that is not odd gets four pictures. The mask is 28 words of 0xfffe0000, shared. 4 tiles of 112 bytes and the objects are about
     570 bytes per shape in `.bss`, 4.5 KB for 8 slots.
   - Publish: `valid` is the last word stored; the hook reads it first. One buffer set per slot: the fields the cell reads never change after the first
     publish, only tile pixels do, so a draw that races a reload shows one torn picture. `remove` clears `valid` first; the memory stays (nothing frees it).
   - Budget: one more site per device (DT2 three binds, DN2 four; installed sites DT2 19, DN2 23 of 48). Cost: a status register read, one load and one
     compare per cell draw; the tile build is a few hundred instructions once.
   - Known gaps: the picture ignores the start phase (the cell zeroes it for the clamped wave 6 before the site), and a wave number whose shape is not
     loaded draws the RAND tiles. The element choice by speed and depth sign, and how a stock-like unipolar flag looks (three tiles drawn), are checked in the
     `dk:dev` screenshot at the device check.
2. The slot code: for an extended wave value, compute signed Q31 from the 32-bit phase with the module's 32-sample table and integer linear
   interpolation (the service keeps a delta table of 32 cells `{ v = s[i] << 16, d = s[i+1] - s[i] }`, built when the shape loads, so the tick makes
   `v + ((d * f) << 1)`); mark unipolar shapes in the service's own table. Cost budget, from the CPU and the tick period: the core runs at 264 MHz
   (the bus constant 132 MHz of the firmware, core = 2 x bus: findings 07, "ColdFire instruction rate", and 08), at 1.5 to 3 cycles per
   instruction (finding 07). The tick runs at 1,500 Hz (finding 04), so one tick is 667 us = 176,000 cycles = 59,000 to 117,000 instructions.
   The stock handler already uses 56.7% of a frame when stopped and 91 to 100% on step frames (DN2 1.11 unit, finding 04 "the stock handler
   on a unit"), so the budget is small: **all 48 LFOs extended may cost at most 3% of the tick period over stock** (5,280 cycles), and a few
   shape LFOs well under 0.5%. Static counts from the built object (finding 149, "Interrupt context and rate"): a wave of 0 to 5 costs 2 `jmp.l`
   more than stock per LFO (96 per tick); an extended LFO costs 54 instructions (stub 8, routine 18, `dk_lfo_eval` 28) against 22 (TRI) to 42
   (SINE) for the stock table wave, so +12 to +32 each. Worst case, 48 extended LFOs at 1,500 Hz: 2,592 instructions per tick (3.9 M per
   second), 576 to 1,536 over stock, which is 0.5% to 2.6% of the period at 1.5 to 3 cycles per instruction: inside the budget, but a step frame
   that already reaches 100% has no room for it, so the all-48 case is not a goal. One extended LFO is 0.05% to 0.09% of the period. These are
   static counts. `coupled_ab.py --image` measures only the stub and routine cost with empty slots: it cannot put `lfo.dks` and a shape module on the card, so it does not
   run the extended path. Use the ColdFire thread instruction counts (`tools/devtools/sharc_cells.py`, the ColdFire thread metric it produces), not `cpu.dsp_thread_insns_per_audio_s`, which counts the DSP thread. The 3% budget is not claimed until
   the extended path is measured (item 2b). Stop if the measurement shows more than the budget. Step frames on a busy pattern already
   run at 91-100% of a frame (finding 04, DN2 1.11), so the measurement must include them. Hardware headroom is confirmed with roadmap row P9 (device calibration) when it exists. The stack of the extended path is 28 bytes at most (24 on the DN2 tick).
2b. Measure the extended path before the 3% budget is claimed: either a card-file option for `coupled_ab.py` (the two labels boot the same image with `lfo.dks` and a shape module on the card), or an
   instruction-count run on a busy pattern with 48 shaped LFOs (the scenario `dn2-lfo-busy.ts`), compared with the stock wave on the same pattern, by the ColdFire thread counts.
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
8b. The lfo service checks a record's body key against the module's KEYS entry. Today nothing ties them: a literal body id bypasses the planner's key rule, so the check refuses a record whose key is not in the module's table. The service reads the keys with the kind-free `CoreApi::keys_of(at, kind, out)` (the keys of record kind `kind` in the header table of the accepted module at selection index `at`); the `Service` ABI names no LFO field (rule 12).
9. Template `sdk/templates/lfo/` and a fixture `tests/fixtures/modules/test/lfo-shape`.

Follow-ups after the device check: E6c (WAVE maximum as a row write), E6d (custom LFO graphics through the SDK) and E6e (clean-up), below.

Tests: unit (lfo service: lookup, interpolation, the delta table against the plain formula, the tile builder against the stock TRI tiles, fallback, contract suite); assembly (one case, the lfo fixture binds its hooks and the image
stub slot is filled); device (written, not yet run; project `core/runtime-lfo-tests`, `modules/core/runtime/tests/lfo/`, tag `S:lfo`, both devices, no SHARC; the fixtures are `test/lfo-shape` (RAMP) and `test/lfo-shape-b` (STEP, a second shape key)).
The output is read from the Sound array of track 1 in firmware memory (the LFO runs into one destination word, found by sampling), at fixed emulated intervals, and fitted to a shape with unknown rate and start phase (`lfo-rig.ts`, `fitShape`); the fit error is below 0.1 for the shape that made the samples and above 0.14 for another:
- `lfo-shape-output`: stock (`core/lfo` in the image, no service: WAVE stops at RAND, waves TRI, SINE, SQR, SAW and RAMP fit their stock functions); table (WAVE reaches 7, the output fits the RAMP table and not TRI, SQR or RAND); start phase (mode TRG, speed 0, SPH at 0, 1/4, 1/2, 3/4 gives equal rising steps for RAMP, TRI's pattern otherwise); number kept (the `lfo.shapes` setting holds 7 before and after a restart and the shape fits again).
- `lfo-shape-project`: with no `lfo.dks` a saved project with wave 7 loads without a fault, plays as RAND, shows the ERR page, and the value survives a second save and load; with `lfo.dks` and only STEP loaded (RAMP's number 7 is a gap below the highest number 8) the same project plays TRI and the page differs from the ERR page and from RAND's, pinned in `goldens.json`. Safe mode loads no module and no service, so it is the same path as "no `lfo.dks`" and has no case of its own.
- `lfo-shape-panel`: with RAMP (7) and STEP (8) loaded WAVE goes past RAND to 7 and 8 and stops at 8; the pages of RND, RAMP and STEP differ, and the RAMP and STEP pages are pinned in `goldens.json`.
- MIDI CC (O8): `lfo-shape-output` sends CC 106 = 7 without a shape (WAVE stays at RAND) and with a shape (WAVE 0x0700). NRPN has no case. The pinned pages are in `goldens.json` for both devices.
Run: `mise run test:subset -- S:lfo --run`. A `dk:dev` screenshot of the MOD page with a loaded shape (DN2 first, DT2 at the end).
A/B (not run): the image half alone is `uv run python -m tools.devtools.coupled_ab A=BIN B=BIN --image A=<image without core/lfo>.syx --image B=<image with core/lfo>.syx -s trig1-taps --scenarios tests/regression/scenarios/dn2-trackfx.ts --pairs 6 --out out/lfo-ab-image.json` (the images from `nx run patchkit:device-images`; one binary, two images; report: the paired ratio B/A of the DSP and CF thread CPU per audio second and the instruction ratio `cpu.dsp_thread_insns_per_audio_s`). The extended path needs `lfo.dks` on the card and all 48 LFOs on a shape on a busy pattern with step frames; no scenario source does that yet (`--image` covers firmware images, not card files), so the device check first writes `tests/regression/scenarios/dn2-lfo-busy.ts` (a pattern with 16 tracks of trigs, every LFO on wave 7) and runs the same command with that `-s` and `--scenarios`, `A` the image without `core/lfo`, `B` the image with `core/lfo` and the card with `lfo.dks` and the shape module. Stop if the ratio of the tick exceeds the 3% budget.

`sdk/sdk.toml` is a shared input of every device test (the `[runtime]` table and version feed the runtime and patchkit), so the E6 PR does not run
the whole device set: it names a targeted subset (`mise run test:subset -- ...` with the lfo and loading tags, gate-smoke and gate-pins, on DT2
and DN2) and says what it skips; `mise run test:full` covers the batch.

Done when: a module adds a waveform that shows its name on the MOD page, draws, and modulates, on DT2 and DN2, and a project saved with it
loads safely without the module.

Estimate: 2-3 days. Payoff: proves the "value of an existing knob" shape. Confidence: medium-high (all sites found on both devices; the
interrupt cost is the open risk). Depends on: E3; E4. Open first: which field the tick clamp reads (finding [O]).

### E6c WAVE maximum as a row write

What: the maximum of the LFO WAVE parameter is a word of the firmware's parameter table, so the lfo service writes it and hooks nothing. The service writes
the max word (+0xc) of the three WAVE rows (codes 79, 89 and 99; row layout: owner +0, mirror +4, min +8, max +0xc, default +0x10, stride 0x3c) in the RAM copy of
the table that `core/params` makes (every reference of the stock table points into the copy). Only the range accessor reads min, max and default of those rows,
apart from the default read in `ParameterPageView::vfunc_2`, which the write does not touch (finding 149, "Direct readers of the parameter table"). The write
is made at run time, never at patch time, so with no shape loaded the maximum stays 0x600 (RAND).

- No new `Service` method, record kind, `CoreApi` call or `SVC_ABI` change. The shared library crate `dkrt-svc-params` has the helper: `set_stock_range` sets the
  (min, max) of a stock row of the copy and returns the pair it held, `stock_range` reads it; both refuse a row at or above `P_FIRST` (not a stock row), a row off the
  row grid and a null table. The copy's base is `P_TABLE` of `dynreg_info`; the row's place is `row - table` of the stock addresses.
- The rows are named by anchors, not by numbers in the service: `cf.lfo.data.wave_row.1` to `.3` in `devices/fw/lfo.toml` (a `cf_words` signature of each row) and
  `cf.param.data.descriptor_table`; `patchkit bind` resolves the four words (`DKRT_LFO_FW` in `hooks-ui.s`) and `setup` gives them to the crate.
  `patchkit service-check` fails a row anchor that lists two services in `users`.
- `apply_record` and `remove` call `sync_range`: max = the highest loaded shape number << 8 while a shape is loaded; the stock pair goes back with the last
  shape (contract cases 3 and 7 compare the word with its start value). `lfo.dks` requires `core/dynreg` (it provides `dynreg_info` and the table copy), so a patch set without it is refused at plan time; an image without `dynreg_info` still refuses a shape with `E_NO_REGISTRY`.

Tests: unit (the helper over `FakeCore` memory: set, restore, refusals; lfo: the maximum follows the highest loaded shape over apply, remove and reload; the contract
suite with the rows in the fixture image), assembly (`lfo.dks` binds two hooks on DT2 and three on DN2, the four words of `DKRT_LFO_FW` resolve to the table and its rows), device (existing: lfo-shape-panel,
lfo-shape-output, lfo-shape-project, lfo.device; goldens unchanged). Depends on: E6, E5.

### E6d Custom LFO graphics through the SDK

What: an optional module draw function on the shape, for example `static void draw(const dk_page_canvas_v1 *c, int mirrored)`. The lfo service calls
it once in the UI task when the shape loads, with a canvas sized to the device's tile (28 x 15). The canvas writes into the service's tiles; the
service keeps the pixels and no pointer into module code (rule 4). Without the function the tiles come from the samples (E6 item 1b). The firmware
`Bitmap` layout stays out of the SDK (`sdk/include/dk/page/widgets.h` gives no firmware Bitmap access).

This changes the SDK contract, so Em agrees the concrete API before it is built. Build it after E6's device check.

### E6e Clean-up after E6's device check

Refactors with no behaviour change, proven by objdiff of the built objects:
1. Move the firmware `Bitmap`, vector header and tile helpers from the lfo service into a shared no_std library crate that follows the
   `modules/core/services/params` pattern (a library crate used by more than one service, not a service). lfo is the first user and E7's filter graph
   the second. Check the layercheck service-crates rule allows it as it allows params.
2. Unify `hooks-dt2.s` and `hooks-dn2.s` into one routine source. The per-device facts are declared in `devices/fw/lfo.toml` (a `[[struct]]` for the offset of
   the wave field of the LFO state, the base register, the scratch register of each site, the D1 save) and reach the routines through the generated
   `dk_fw_lfo.inc` symbols and macros of `patchkit gen`, so a new firmware version is a TOML change plus `patch:port`. This reworks reviewed interrupt
   code, so it gets a new review.

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
6. Hook budget: the six new hooks go through the port file ([port-file.md](port-file.md)). `machines.dks` declares 13 hooks; `BIND` items leave the services with the port file, so no per-file limit applies. The runtime's `install_sites` 48 still bounds the
   installed hook sites: 18 are taken on DT2 and 22 on DN2 today (table in port-file.md section 5), and E7 adds at most 6 per device, which gives 24 and 28.
7. Template `sdk/templates/filter/` and a fixture `tests/fixtures/modules/test/filter-gain`.
8. The filter response graph reuses the Bitmap and tile helpers of the shared crate (E6e), and the filter knob ranges are rows of the machines service (the rows its builder calls write), so they need no range call.

Tests: unit (machines service filter lowering, contract suite with a filter fixture); rig (filter DSP conformance in `@digi/sharc-rig-kit`);
assembly (filter fixture loads); device (one test per device: select the filter, render, PCM report against the fixture's expected response).

Done when: a module adds a filter that appears in FUNC+SYN on the FLTR page, shows its knobs, draws its graph and filters audio, on DT2
(and DN2 if E0b item 5 shows the same shape).

Estimate: 3-5 days. Payoff: proves the "table row" shape for a second table family. Confidence: medium until E0a (ColdFire side known).
Depends on: E0a; E3; E4; E5 (MISSING placeholder for filters); R12-8; the port file (its six new hooks go through it).

### E8 Top-level lift and names

What: the target tree of [taxonomy](../design/taxonomy.md) (layers, `runtime/`, `services/`, `ports/`, `image-modules/`, content modules under `modules/<kind>/<module id>`), as one mechanical PR in two commits.

Scope:
- Commit 1: `git mv` of every folder in the mapping table of the taxonomy, with the path edits in `tools/nx/modules.mjs`, `tools/devtools/layercheck.py`, `project.json` files (Nx project roots and inputs; Nx project names stay), Cargo path dependencies, patchkit manifest roots and `tools/nx/*.mjs` inputs. `services/params` becomes `services/rows` as a folder; the crate keeps its name.
- Commit 2: roles in `module.toml` (`runtime`, `installer`, `platform`, `image-half` with `service = "<x>"`, `adapter`, `fix`, `proof`, `app`, `service`; they replace `role = adapter` and `type = framework`), the layercheck `roles` table and folder rules, the requires edges (`plusdrive-write` and `sdhc-large` get an edge from the tool that needs them; every patch-set list that names `core/ui` or `core/health` also names `core/runtime`, as ST does), patch-set comments in `devices/*.toml` (ids unchanged), `dsp-stereo-proof` into the tests of `dsp-voice`, and the `users` of `cf.storage.fn.directory.open` and `.next` in `devices/fw/storage.toml` (E1: drop `core/runtime`). The `category` values change from the plural (`machines`, `tools`, `lfo`) to the template names (`machine`, `tool`, `lfo-shape`) only if the check below shows that `category` reaches no `.dkm` byte: today `pack.rs` only compares it with `core` and `digikit-dkm.md` lists no category field; the workbench (`FilesPage.tsx`), the tests and the templates still read it. If it reaches bytes, keep the values and rename folders only.
- Docs: ARCHITECTURE.md (layers, folder map), LAYOUT.md, AGENTS.md directory map, runtime-and-services.md wording.
- Not in E8, each its own small PR with its own proof: the merge of `usb-bulk-audio` into `usb-bulk` (changes bytes on purpose; named device tests usb-bulk and usb-bulk-audio), and the renames of Rust names (`Gate` to `InitWindow`, `gate()` to `init_window()`, crate `dkrt-svc-params` to `dkrt-svc-rows`). Rust symbol names can reach `.dko.json`, so those PRs compare objects with `patchkit objdiff` and, if bytes change, show why. Module ids do not change.
- Check first: the Rust objects and `.dko.json` files embed no source path (a moved source would change their bytes), and `category` as above.

Proof:
- The byte manifest proves no behaviour change. Tool, part of E8: `node tools/nx/byte-manifest.mjs write|compare` (it reuses `tools/nx/device-bytes.mjs` and `object-hash.mjs`). Manifest: `out/e8/manifest-<device>.json`, one line per file with its SHA-256. Artifacts: the patched image per device (`dist/bytes/images.sha256`), `dist/modules/<device>/*.dkm` and `*.dks`, every committed `**/build/*.dko.json` (whole-file hash, so an embedded path shows), `dist/bytes/pack-verdicts.json`, and the generated files that `patchkit gen --check` covers (for example `gen_sdk.rs`, `gen_abi.rs`, `gen_limits.rs` and the generated headers).
- Write the manifest before commit 1 (a clean build of everything). Compare after commit 2. Compare after commit 1 too, to find which commit changed a hash.
- Merge gate (about an hour of checking): the manifest compare, the host stage (`typecheck`, `lint`, `fmt-check`, `test-host`, `python-checks`), `mise run test:plan` (the same targets as before, modulo renamed paths), `gate-smoke`, one named device test per device (for example `module-load` on DT2 and DN2) to show that the harness finds the moved files, and `git diff -M` showing renames plus path edits only.
- The merge-test files key on image hashes (AGENTS.md), but other device tests key on the production files of their project, their imports and packs, so a move changes their keys and they would run. The PR does not run them: it names what it skips and argues from the bytes (the targeted route; Em decides per PR). The full device set reruns in the next batch's full run in the background lane, as for any batch of merges.
- Stop rule: any hash change stops the PR.

Estimate: 10-14 h of agent editing time (the move, the path users, roles, requires edges and the manifest tool). It is not test time. Payoff: the folders show the layers; new code lands in the right place. Confidence: high.
Depends on: E1; the port-file stack (PR2, PR3) and E5c/E5b merged; open branches merged (including `test/t1i-rig-read-hook`, which edits `dsp-stereo-proof`). Order: before port step 3, the project service and E7.

### E9 Later: on-demand platform pieces and overdrive

Not started by this plan. Candidates that only some modules need, so they could load with a service instead of sitting in every image:
`input` (used by remote), `plusdrive-write` (usb-fast), `sdhc-large` (large transfers), `introblank` (optional), and possibly `health`. USB descriptor code stays in
the image (descriptors cannot change after enumeration). Decide per piece with the "harmless with no service loaded" test of
runtime-and-services.md.

`overdrive` as a "value of an existing knob" feature (OVR TYPE): its owner service is open (O3).

## 6. Order and dependencies

| # | Item | Depends on | Device runs | Status |
| --- | --- | --- | --- | --- |
| 1 | E0b static spikes (extended) | - | none | done: findings 147, 148, 149; 135, 02, 06, 53 corrected (branch e0b/findings) |
| 1 | E0a filter DSP experiment | daemon with a generated core and `sharc-profile`; UI chooser | one, about 10 min | parked: the first attempt ran on a daemon without a generated core (finding 148); retry before E7 |
| 1 | E0c daemon hardening | - | none | done: PR #615 |
| 2 | E1 proof coverage check | - | none | done: keep `file-catalog-proof`, move in E8 |
| 2 | E2 boilerplate codegen | - | none; objdiff | done: PR #608 (objects byte-identical by objdiff) |
| 3 | E3 keys, trait, role check | - | subset, shared with E1 and E2 in one gate | done: PR #609 (device: runtime-refusal-tests on DT2 and DN2) |
| 4 | E4 contract suite | E3, `nx/dks-tests` | none | done: PR #610 (device: runtime-machines-tests machine-rows on DT2 and DN2) |
| 5 | R12-8 merged | (roadmap) | (its own gate) | roadmap row R12-8 |
| 6 | E5 reload of content modules (machine work) | E3, E4, E0b, R12-8 | reload test, soak, subset | done: PR #623 (device: reload test on DT2 and DN2 incl. PCM swap check and soak; named subset 35 halves, gate-smoke, gate-pins) |
| 7 | E5b reload follow-ups | E5 | fx reload test, soak | todo |
| 7 | E5c-1 live job: load, unload and reload (runtime, protocol, clients) | E5 | live test (list in E5c), subset | done: PR (device: live, shot-live, reload, C:loading subset, gate-smoke, gate-pins on DT2 and DN2) |
| 7 | E5c-2 MODULES applies live (menu) | E5c-1 | menu device test, subset | todo |
| 8 | E6 LFO shapes | E0b, E3, E4 | lfo test plus subset | done: PR #612 (test image fix for the refusal test: PR #613) |
| 8 | Service record validators | E3 | none | done: PR #614. `validate_records` is a required method of the `Service` trait (ABI word changed); every service implements it; `patchkit records --check` runs the owning service's validator (patchkit feature `record-rules`, CLI only) |
| 8 | LFO shape template and first module | E6 | lfo test | done: PR #618: module kind `lfo-shape` (`mise run module:new -- lfo-shape AUTHOR/NAME`, `sdk/templates/lfo-shape`; `module:check` runs the service validator), module `digikit/lfo_shapes_1` with 7 shapes (STP4 PULS HSIN ABSN SOFT DUCK WOBL), Transfer group "LFO shapes"; `mise run lfo:report -- <author/name>` measures each shape's output on the emulator (DN2 report: all seven shapes OK) |
| 8 | Boot window | none | none | done: PR #611, #613 |
| 8 | Taxonomy and card layout | - | none | done: docs/design/taxonomy.md |
| 8 | Test-speed follow-up | wait audit | none | active (branch test/waits-on-state): built, device proof pending. boot-restart waits on `ui_reached` and the record sequence; the LFO rig turns the WAVE burst into a poll and freezes on the phase standing still. Kept with a reason: fx-routes-persist 44,000 is ColdFire instructions (a press offset inside a DSP block), project-kit save 3.5 s and load 6 s (no firmware state marks the end), machine-rows `stableFrameHash` (the frame is the observed value), the LFO destination glide (the LFO swings the word) |
| 8 | Flash waits for the device write | none | host tests of the flash flow | open: after the last SysEx block the device still writes the image to flash while the workbench can already report done (`packages/workbench-core/src/firmware/firmware-flow.ts` `afterSend`: if the port does not leave within `leaveMs` 10 s it asks INFO, and the old image can answer). Show "the device is writing, keep it powered" and accept an answer only after the port left and came back, or after a minimum write time; size it from a device log of the "rebooting: <stage> · ports:" lines (real write time unknown, finding 75) |
| 8 | Python tests red on main | none | python-tests | done: PR #622 (test_dktiming fake INFO layout, test_core_fx section list, menu-dkrun-pluck binds its services) |
| 8 | test_pk_current fails on main | none | python-tests | done: PR #621 (container_max hex case) |
| 8 | E6c WAVE maximum as a row write | E6, E5 | lfo device tests (below) | done: branch e6c/row-write (host checks; the lfo device tests below are not run yet) |
| 8 | E6d custom LFO graphics through the SDK | E6 device check, Em agrees the API | none | todo |
| 8 | E6e clean-up after E6 | E6 device check | objdiff, host tests | todo |
| 9 | E7 filter machines | E0a, E0b, E3, E4, E5, R12-8, port file | filter test plus subset | todo |
| 10 | E8 top-level lift and names (taxonomy tree, roles, requires edges, patch-set lists, byte-manifest tool; 10-14 h agent editing time) | E1, port-file stack, E5c/E5b, open branches merged | byte manifest, host stage, `test:plan`, gate-smoke, one device test per device; the rest in the next batch's full run | todo |
| 6 | Port file stack, PR1-PR3 and PR4 ([port-file.md](port-file.md)) | PR D, E5 | the device subset in port-file.md section 12, DT2 and DN2 | active: PR1 done (PR #621); PR2 and PR3 on branches pf/pr2-runtime and pf/pr3-workbench (done on the host; the device proof is next, one gate for both) |
| 11 | Port file step 3: adapter routines in the port file (lfo, then fx, then machines) | E8, port file stack | the device subset in port-file.md section 12 | todo |

E1, E2, E3 share one subset gate. E0 runs in parallel with E1-E3. When an item starts, its status becomes `active` with the branch; when it lands, `done` with the PR, set in that PR.

## 7. Test pyramid and Nx

| Layer | What this plan adds | Target |
| --- | --- | --- |
| Unit | contract suite per service (6 cases x 2 devices x 4 services); planner key cases including a collision; sticky ranges; `role_ok`; reload refusals and order; run-time numbering; lfo lookup; filter lowering | `<service>:rust-tests` (from `nx/dks-tests`), `core/runtime:rust-tests` |
| Generated consistency | `patchkit gen --check` for exports and builder lists; `patchkit objdiff` for the E2 refactor | `mise run gen:modules -- --check` |
| Assembly | one case per new seam: key clash in the boot plan; load-reload-reload of a packed fixture; lfo hooks and image slot bind; filter fixture loads | `core/runtime:test-assembly` |
| Rig | filter DSP conformance | `@digi/sharc-rig-kit:test-conformance-rig` |
| Device | reload (code-only, machine with DSP, fx, during voice release, refused during transport, project round trip); LFO shape output and safe load without the module; filter select and render | `core/runtime-tests:test-device--<file>`, `<module>:test-device` |
| Soak | about 5 reload cycles in the gate, 20 in the full tier, with `DIGI_FLUSH_CHECK=1` | emulator, once per E5 PR |

Rules kept: each assertion at the lowest layer that can see it; fixtures from `tests/fixtures/modules`; device tests read firmware output,
not the screen; PCM pins set from a numeric report. Before a device run, list the tests and the time and get Em's go.

## 8. Risks and stop rules

| Risk | Effect | Stop rule or fallback |
| --- | --- | --- |
| A stock reader of the LFO wave that E0b missed does not clamp | a fault on stock code with a saved extended wave | E6 device test loads a saved project without the module and in safe mode |
| E0a finds the filter stage inline in stock code with no per-type dispatch | E7 needs a new DSP hook design | stop E7 after E0a and report payoff and cost; E1-E6 are unaffected |
| A filter-type reader E0b missed indexes a table by the raw value | wrong data for 16..31 | E7 device test walks every filter page and the chooser with a custom filter selected |
| A gap row plays type 0's sound (DT2 sample renderer, DN2 FM TONE) | sound in the reload window | the MISSING placeholder is made silent (E5 item 7); on DT2 the silent routine is checked on the rig first |
| `machines.dks` is near the 65536-byte staging limit (the file was about 65.5 KB before E5 and is about 61 KB on DT2 after BUILD is zero-initialised) | E7's filter code may not fit | measure the file before E7 starts; move code out of the service or raise the staging limit (a runtime change) before the filter code lands; stop E7 if neither is possible |
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
- O5 (decided with E5c): reload first (E5), then one live job for load, unload and reload (E5c); the user applies a MODULES change as one batch, unloads first.
- O2: the top-level lift of the taxonomy (E8), after the port-file stack and E5c/E5b, before port step 3, the project service and E7.
- Effect id format stays 16-bit.

Open:
- O3: owner of the overdrive curve list if overdrive becomes a feature: a small `drive` service, or the fx service.
- O4 (decided in E6): the LFO_SHAPE record stays a generic `dk_record_add`. Writing the fixture and the template showed only byte packing as the awkward part (key, name and 32 samples as big-endian bytes, 80 bytes); the service check now refuses a body key that the module's KEYS entry does not list, so a literal key is caught. A typed `dk_lfo_add_shape` would add an import, a `since` and collector plumbing for no new capability; if packing proves error-prone for authors, add a header helper that fills the body in `sdk/include/dk`, with no runtime change.
- O6: E7 stored filter choice: digikit's store with a stock fallback in the sound (recommended, rule 7), or patch the stock loader bound
  (7 to 32) so the extended type survives in kit files; with the patch, a kit opened on stock firmware falls back to filter 0.
- O7 (decided in E6): the shape graph is option A of item 1b: one hook at the vector pick, four 28 x 15 tiles per shape built by the service. Replacing
  the accessor (wider, generic) and writing into the firmware's RAM table (needs a second site and the firmware would free our memory) are rejected.
- O8 (decided in E6): MIDI behaviour with a shape loaded is accepted as a feature. A CC value of 7 or more, or an NRPN of 0x380 or more, reaches shape numbers
  instead of clamping to RAND; stock waves 0-6 are unchanged (finding 149 "Callers of the T1 range accessor"). The panel, CC and NRPN share the T1 maximum and the clamp.
- O10: after a MIDI CC change of WAVE, the shape name shows only after the next full page redraw.

## 10. Changes to other documents when items land

- [digikit-hot-reload.md](digikit-hot-reload.md): S1 and S4 are done by E5 for reload; decision 3 (load beside, roll back) is replaced by
  validate-then-reload with a placeholder (3.6); S3 stays deferred; "gate" becomes "hook gate".
- [sdk-runtime-roadmap.md](sdk-runtime-roadmap.md): row E; row H points to E5 for S1 and S4.
- [test-architecture.md](test-architecture.md): T1 "contract suites for `Service`" is E4.
- `docs/ARCHITECTURE.md`, `docs/LAYOUT.md`, `AGENTS.md` directory map: E8. The lifetime rule of ARCHITECTURE.md (no module unloads) changes with E5c.
- `docs/MODULE-AUTHORING.md` and `sdk/README.md`: the three shapes of 3.2 and the rules of 3.3 (E2), filters and LFO shapes (E6, E7).
- `.agents/skills/digikit-module/SKILL.md`: the filter and LFO builder calls (E6, E7).
