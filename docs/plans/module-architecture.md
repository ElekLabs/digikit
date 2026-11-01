# Module architecture: categories, contracts, capabilities and enforced layers (plan, 2026-11-01)

Goal: users write modules against a stable contract, and can submit a module before the SDK has an API for it. The runtime core keeps only
what loading modules needs. Tools check the layer boundaries. Inputs: issue #240 (the store upload for patch-based images), PR #209
(patched-in machines), [finding 73](../findings/73-dnfw-coexistence.md), the [capabilities plan](capabilities.md) and the
[module limits plan](module-limits.md). Steps 1 and 2 start after the sampler branch is merged.
Code references and sizes are from the sampler branch (`sdk/dsp-sampler-discovery` at 94c8ee1a): `tracks.rs`, `sample.rs`,
`sample_source.rs`, `files.rs` and `core/sample-pool` reach `main` with it.

## Terms

| Term | Meaning |
| --- | --- |
| Category | What a module is to a user: `machines`, `fx`, `tools`, `test` (`core` for `core/*`). Every UI shows it. It does not change when the module moves to another contract. |
| Contract | The SDK API a module conforms to: `machine`, `fx`, `service` or `raw`. `raw` has no contract: only the generic load checks. |
| Capability | A device-neutral feature a module can use: FX pages, track samples, value text per parameter, and so on. Its logic is device neutral. |
| Adapter | The patch-time code of one firmware plug-in point (hook sites, tables, reserved rows). It implements capabilities on one firmware. |
| Device data | Values in `devices/*.toml`, firmware addresses in `devices/ports/*.toml` (anchors by hash). `patchkit gen` is the only writer of `build/gen/`. |

## Layers

```
content module (.dkm)       C and SHARC assembly against the SDK only
SDK contract                sdk/: headers, sdk.toml imports, contract versions
runtime: capabilities       device-neutral logic, src/cap/* of the runtime crate, registered in the core's table
runtime: core               load and unload, record dispatch, arena, boot, selection, safe mode, crash and fault, log,
                            settings, store reader and card gate, DSP link transport, basic stats (CAPACITY, MODULES, what fits)
adapters                    patch-time firmware glue per firmware; reached through optional symbols (0 when absent)
firmware bindings, anchors  modules/core/fw (generated), devices/ports
```

Rules:

1. The core names no capability. It calls the registration table.
2. A capability uses the core only through the core API, and another capability only through that capability's declared API, never its state.
3. An adapter uses firmware bindings and other adapters' headers. It calls the runtime only through declared callbacks (today
   `dkrt_pool_done`, `dkrt_pool_lost` and `dkrt_project_changed` of `core/sample-pool`).
4. `requires` in `module.toml` is a code dependency. The image's contents come from `[patch_sets]`. Today most adapters list `core/runtime`
   only to pull it into the image: of all adapter objects, only `core/sample-pool` imports runtime symbols. The patch sets name
   `core/runtime` themselves.

## Module manifest

- Content modules keep: `id`, `version`, `name`, `category`, `contract`, `author`, `summary`, `[targets]`.
- `contract` replaces `type`. It defaults from the category (`machines` → `machine`, `fx` → `fx`, `tools` → `service`); `raw` is written out.
- Removed from content modules: `dynamic` (always true), `requires` and `conflicts` (a content module gets what it needs through
  capabilities), `ports` (the keys of `[targets]`), `status` if it always equals the `[targets]` values (checked in step 1). `[[object]]`
  follows from the source layout: `src/*.c` → `cf.expansion`, `src/dsp/*.s` → `dsp.l2`.
- `core/*` modules keep `category = "core"` and `role`. `framework` is deleted.
- The `.dkm` header carries the category. The loader still decides compatibility by the contract bytes (`docs/design/digikit-dkm.md`, 0x38).
- The packer checks the declared contract against the content (a machine needs a machine declaration, and so on) instead of inferring it.
- The Transfer tab and the desktop group by the header's category. A `raw` module shows as unchecked.
- A module's needs are derived from the built `.dkm`, never declared: sizes and rows as `packages/dkneeds` does now, capabilities from its
  imports (`[[import]] requires`) and record kinds.

## Raw modules and experimental adapters

| Step | Shape | Example |
| --- | --- | --- |
| A. Experimental adapter | Patch-time module that owns a new hook and exposes an experimental capability as data rows or optional symbols. The content is a `raw` `.dkm` where possible, else inside the adapter. | PR #209 |
| B. Raw `.dkm` | Uses the capability. Generic load checks only. | TB303 once it loads from the card |
| C. Contract module | The capability is stable in the SDK; the module moves to the `machine` or `fx` contract. | spark, pluck |

The category stays the same in every step. To move from B to C: change `contract`, replace calls that are not SDK calls, pass the
conformance suite.

A `raw` module has no contract checks. It has the same link surface as every content module (SDK imports, `dkrt_*`, `dk_ui_*`, `input_*`)
and uses no absolute firmware or I/O address. Checks: a static absolute-address check on content objects (new, step 3), the loader's import
checks (`E_IMPORT`, `E_DSP_IMPORT`), and at test time `module:audit` and the conformance write-set checks.

An experimental adapter:
- has `stability = "experimental"` in `[module]` and its own patch set, never the `emulator` or `device` set;
- takes values only through `patchkit gen`, and has per-device code only where the firmware differs;
- has its own tests, and its README names the capability it prototypes;
- leaves the default images unchanged (the smoke matrix shows no change).

## Runtime: core and capabilities

Measured from `modules/core/runtime/build/runtime.dko.json`: text 144,532 B, data 73,689 B, bss 666,916 B (arena 524,288, staging 65,536,
upload buffer 36,896, settings 8,192). `cf.expansion` is 1,396,736 B on DT2 1.17 (`devices/ports/dt2-1.17.toml`) and about 25 MiB on DN2.
The runtime's hooks are the UI loop, the boot task and MIDI RX (`modules/core/runtime/module.toml`); no runtime code runs in the audio
block. Modules load at boot only (`loader.rs:109-112`). Hot reload is deferred (`docs/plans/digikit-hot-reload.md`); the table keeps
`quiesce` and `remove` for it.

| Capability (`src/cap/*`) | Code today (`modules/core/runtime/src/rs/src/`) | Adapters |
| --- | --- | --- |
| Rows: machines, parameters, pages | `registry.rs`, `assign.rs` | dynreg, machines, params |
| Voices | voice rows in `dsp.rs` | dyndsp, dsp-voice |
| FX: rows, pages, sends | fx rows in `dsp.rs`, `fxpage.rs`, `fxsend.rs` | dynfx, fxpages, fx |
| Sample bank | `sample.rs`, `sample_source.rs` | none |
| Track samples | `tracks.rs` | sample-pool |
| Files | `files.rs` | none |
| Store upload | the store commands of `upload.rs`, the store writer and figures of `store.rs` | none |
| Health figures | `dkrt_health*` in `lib.rs`, the snapshot in `watch.rs` | health |

The registration table has the shape of a C interface, so that one capability can later be built as its own object without a new interface:

```rust
#[repr(C)]
pub struct DkCap {
    kinds: u32,                                                          // record kinds it handles
    phase: u32,                                                          // order of apply (PARAM, PAGE, MACHINE, EFFECT, ...)
    check:  extern "C" fn(rec: *const u8, len: u32) -> i32,              // before any write
    apply:  extern "C" fn(owner: u32, rec: *const u8, len: u32) -> i32,  // 0 or E_*
    ready:  Option<extern "C" fn(owner: u32)>,                           // after the DSP rows are published
    quiesce: Option<extern "C" fn(owner: u32) -> i32>,
    remove: extern "C" fn(owner: u32),
    tick:   Option<extern "C" fn()>,
    boot:   Option<extern "C" fn()>,
    counts: Option<extern "C" fn(out: *mut DkCapCounts)>,                // its share of CAPACITY
    owner_of_row: Option<extern "C" fn(row: u32) -> u32>,                // fault attribution
    invariants: Option<extern "C" fn() -> i32>,                          // table::check
}
static CAPS: [&DkCap; N] = [&ROWS, &VOICES, &FX, &SAMPLES, &TRACKS, &FILES, &STORE_UPLOAD, &HEALTH];
```

Only the seam uses `extern "C"`, `repr(C)`, pointer and length, and error codes. Code inside a capability is normal Rust.

The DSP link carries one transfer at a time, one chunk per audio block. Today each user checks the others' `busy()` before it starts
(`sample.rs:133-135`, `tracks.rs:190`, `dsp.rs:361`, `fxpage.rs:585`, `fxsend.rs:417`). The transport gets one claim flag: a user claims,
sends and releases, and no user knows another. The tick order keeps today's priority. Step 6 lists the current rules before the change.

Cost: the table is about 256 B, and a UI pass makes about 10 calls through it. Proof: the runtime object's sizes before and after, the
regression suites, one health CPU reading.

A capability becomes its own object only for a reason. The first is the store upload (#240), as a 5th `[[object]]` of `core/runtime`.

## Device data

Values are in TOML. Code is per device only where the firmware differs. These five adapters have DT2 and DN2 sources that differ only in
the `.include` line and comments: dynfx (22 lines), dyndsp (40), fxpages (318), input `device.s` (18), dsp-link (14 and 11). They become
one source each:

```toml
[[gen]]
kind = "gas"
path = "build/gen/{dev}/device.inc"     # the source has .include "device.inc"

[[object]]
path = "build/dynfx-{dev}.dko.json"
sources = ["src/cf/dynfx.s"]
devices = ["dt2", "dn2"]
```

Real differences stay per device: machines (DT2 clone tests and sort rank, DN2 type maps, frame type and type reader), the DN2 list hook of
dynreg, params and sample-pool (DT2 only), the SHARC wrappers of dsp-voice and the fx send, and the runtime's `device.s` (DT2 has the
firmware file-system pointers `cf.storage.fn.path.*`). Proof: the objects are byte-identical before and after.

## Versioning and module settings

- The runtime has a semantic version (`sdk/sdk.toml` `version`, `docs/design/module-sdk.md` D9). Contracts carry [major, minor] and
  decide compatibility (D3). The manifest `version` of a module is the module's own; the runtime does not read it.
- Step 1 changes the `.dkm` format once. Each new capability is a minor of the `api` or `machine` contract: it only adds.
- The registration table is internal, built with the runtime, and has no version. A capability built as its own object gets a version in
  its descriptor (the store upload first).
- Module settings: keys `<module id>/...` in the settings record (`docs/design/digikit-store.md`, "Keys by owner"). The module owns its
  values and their version: it writes a version field and reads older values or resets them. The runtime never interprets them. Values
  saved with the project are the firmware's, not part of this.

## Enforcement

- One table of layer rules, read by both checks below.
- A local Nx plugin (`createNodesV2`, `createDependencies`; check the field names against the installed `@nx/devkit`): one project for
  each `module.toml`, tags `role:`, `category:`, `contract:`, `stability:`, edges from `requires`, C `#include`, assembly `.include` and
  `#include`, Rust `dkfw::` and Cargo path dependencies.
- A boundary check over `nx graph --file` against the rules table. The TS packages get tags and the Oxlint boundary rule.
- `layercheck` keeps the rules a graph cannot express: undefined symbols of built objects, apps without `[[op]]` and `[[claim]]`,
  self-contained, SDK imports. It adds: no absolute firmware or I/O address in content objects (ColdFire through the `isacheck` decoder,
  SHARC through `tools/sharc_dev/sharc_core/`), no device-named include (`device-dt2.inc`) in a source, capability folders do not use core
  internals.
- Generated files: an AGENTS.md rule (never edit `build/gen/`; change the TOML and run `mise run gen:modules`); `test:changed` runs
  `gen-check` when a generated file or its source changes; the gate keeps `gen-check`.
- Not used: Nx Conformance (needs a paid licence), GritQL (no C, Rust or assembly), the Nx language plugins.

## Steps

- [ ] 0. This plan; draft replies to #240 and #209.
- [ ] 1. Contract change, once, after the sampler branch is merged: category in the `.dkm` header, `type` → `contract`, `framework`
      deleted, raw shown as unchecked, the minimal content manifest, the Transfer tab and desktop grouped by category, the packer checks the
      contract. All content modules and fixtures in one change. About 1 day.
- [ ] 2. Store upload (#240): `src/cap/store_upload` with the C-interface table, the commands split (store: INFO, LIST, CAPACITY, BEGIN,
      DATA, END, ABORT, READ, VERIFY, DELETE, CLEAR, MKDIR; runtime: MODULES, SETTINGS, FAULT, SELECT, RESTART, plus a callback for unknown
      commands), a store bit in INFO, then a 5th `[[object]]` of `core/runtime`, committed. 1 to 2 days.
- [ ] 3. Experimental tier: `stability`, its own patch set, the merge rules above, the layercheck rules (values through `patchkit gen`, no
      device-named include, no absolute address in content objects). Then PR #209 in parts (overdrive, Open303, the paraphonic machines).
      About 1 day plus review.
- [ ] 4. Enforcement: the rules table, the Nx plugin, the boundary check, `requires` as code dependencies, AGENTS.md, ARCHITECTURE.md,
      MODULE-AUTHORING.md, a README link. About 1 day.
- [ ] 5. The PR #209 capabilities in the order of the [capabilities plan](capabilities.md): value text, all 8 knobs (module limits step 4),
      track sample, stereo voice, chords. Then TB303 as a `raw` `.dkm`, then on the machine contract.
- [ ] 6. Runtime split into core and `src/cap/*`, with the DSP link claim flag. 3 to 5 days.
- [ ] 7. One source for each of the five adapters above. 2 to 3 hours.
- [ ] 8. Follow-ups: hooks installed at boot (classify every hook site: region, first run against the install point, branch or table,
      context; then one emulator run that compares the bytes the runtime writes with patchkit's), the cost of each adapter when no module
      uses it, the C compiler of the Selache toolchain against hand-written assembly. If hooks can be installed at boot: the runtime installs
      only the hooks of the capabilities the selected modules use, hot-path hooks first, and adapters that no module uses leave the default
      images.

Later, not ordered: the module settings API (a module reaches only `<module id>/` keys).

## Open

- **[O]** A free byte in the `.dkm` header for the category, or a format version bump (step 1).
- **[O]** `{dev}` in `[[object]] path`, and a per-device include path in the build (step 7).
- **[O]** The store-upload object: its own copy of the store reader and card gate, or the core's through the table (step 2). How dnfw links
  a `.dko` (asked in the reply to #240).
- **[O]** Hooks at boot: whether the MMU is on; whether the firmware sends the DSP boot stream again (step 8).
- **[O]** The cost of each adapter when unused (step 8).
