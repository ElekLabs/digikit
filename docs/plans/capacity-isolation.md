# Capacity and isolation of run-time modules: limits in the profile, tests that fill every limit (plan, 2026-10-21)

Status: plan only, no code. Lane `tasks-images`, base main 3ff156f9. Facts below were read from the code on 2026-10-21 and are **[D]** (read once, nothing was run).
Paths of files that do not exist yet are written without their directory prefix on purpose (pathcheck).

Status 2026-10-21: PR 1 (steps 1 to 5) built on lane/tasks-images; results and corrections in docs/findings/57-capacity-isolation.md (the DSP area's first range starts at offset 4 and 60 bytes stay unusable, so it cannot be filled byte-exactly; the full set has 5 loaded modules; `SEL_MAX` and `MAP_N` are tied to `[limits.max]` by a cargo test, generation moves to PR 2). PR 2 (step 6) is not started.

## Goal (agreed with Em)

Prove, on both devices (DT2 1.17, DN2 1.12), that:

1. each run-time module type loads up to each capacity limit;
2. each limit is refused with its own code when it is exceeded;
3. modules that are already loaded are not clobbered by a later module, loaded or refused (same PCM, clean memory audit, same tables);
4. all limits come from the device profile (`[limits]`), checked against a contract maximum in `sdk/sdk.toml`;
5. MODULES (or a sibling command) reports rows used and free per type next to the arena and DSP area totals.

Module types that load at run time today: machine (DSP), machine (data only) and raw ColdFire code. Run-time effects do not exist (finding 56).

## What the code does today

### Limits, where they live (**[D]**)

| Limit | DT2 | DN2 | Source | Copies |
| --- | --- | --- | --- | --- |
| module records | 8 | 8 | `[limits] modules` -> `LIMIT_MODULES` (generated `device.rs`) | `MAX_MODULES` in tools/patchkit/pk_dkrt.py:59 |
| selection paths | 8 | 8 | `SEL_MAX` in modules/core/runtime/sdk/src/lib.rs (record format) | `SEL_MAX` in pk_dkrt.py:54 and in pk_dkrt_boot.py |
| sticky map entries | 8 | 8 | `MAP_N` in the same file, line 319 (record format: bytes 28-31 and 480-507) | none |
| ColdFire arena | 512 KiB | 512 KiB | `[limits] arena_bytes` | `ARENA_BYTES` in pk_dkrt.py:58 |
| staging buffer, file size | 64 KiB | 64 KiB | `[limits] stage_bytes` | none |
| machine types, machine rows, voice rows | 8 | 8 | `[limits] machine_rows`, tied to types.toml by an assert (dsp.rs ~50) | none |
| parameter rows, page rows | 16, 8 (DN2 only) | same | count of reserved `[[entry]]` rows in modules/core/rows/module.toml; read by the runtime from the device object (`P_DYN_N`, `PG_DYN_N`, registry.rs ~388-407) | none; not in `[limits]` |
| DSP run-time area | 512 KiB | 320 KiB | `[digikit] dsp_area_bytes` (PerDevice in devgen.rs) | `0x80000` and `0x20000` in modules/core/dsp-link/src/cf/dsp-link.c:37,135 |
| effects in the chain | 4 | 4 | `DK_FX_MAX` | sdk/sdk.toml:47, sdk/include/dk/gen/fx.h:11, sdk/asm/dk_fx.inc, `max = 4` in modules/core/fx/module.toml:55, `FX_MAX` in tools/patchkit/pk_conform.py:417 |

All `[limits]` are `Shared` in packages/patchkit/src/devgen.rs: the runtime is one image for both devices, so `gen` refuses a value that differs (docs/design/device-data.md). Only `dsp_area_bytes` is `PerDevice` (the runtime reads it from the `core/dyndsp` info table, dsp.rs `iw(I_AREA_BYTES)`).

### Refusal paths, in the order the loader meets them (**[D]**, loader.rs:104-300, dsp.rs:350-480 and 520-535)

1. `store::load_to_stage`: file larger than the staging buffer -> `E_TOO_BIG` (-13).
2. Header checks: `E_FORMAT`, `E_SHA`, `E_CONTRACT`. `bss > ARENA_BYTES` is `E_FORMAT` (-5), not `E_NOMEM`.
3. `registry::check`: no free machine number for a hashed machine -> `E_TYPE_RANGE` (-22); `E_TYPE_USED` (-23); not enough free parameter rows -> `E_PARAM_FULL` (-24); not enough free page rows -> `E_PAGE_FULL` (-25).
4. `dsp::check`: more than 8 voices in the file or an extent above the area -> `E_DSP_FORMAT` (-31) / `E_DSP_NOSPACE` (-33); `nvoice > free rows` -> `E_DSP_ROWS` (-34), checked before the types; a voice type outside 16..23, or used by another module, or twice in the file -> `E_DSP_TYPE` (-37).
5. Same name already loaded -> `E_LOADED`; no free module record -> `E_TABLE_FULL` (-14).
6. `dsp::reserve`: first-fit range in the area fails -> `E_DSP_NOSPACE`; the arena block for the relocated DSP image fails -> `E_NOMEM`.
7. `arena::alloc(image + bss)` fails -> `E_NOMEM` (-8); the DSP reservation is cancelled (`dsp::cancel` frees the image block and the rows).
8. `table::check` runs after every attempt; a failed invariant sets `DKRT_TABLE_BAD` and every later load answers `E_TABLE` (-49).

### Three facts that change the test design

- **E_TABLE_FULL cannot be reached today.** The boot loads the selection, and a selection holds at most `SEL_MAX` = 8 paths; there are 8 module records. A ninth path is refused earlier: `SELECT` answers `E_SEL_COUNT` (-41, upload.rs:969); the mailbox `CMD_SELECT` answers `E_SEL_PATH` (-40, lib.rs:864, a different code for the same case); a forged record with count 9 is invalid (boot.rs:163). `E_TABLE_FULL` is defence in depth while `modules == SEL_MAX`. It becomes reachable only when a device sets `modules` below 8.
- **A hashed machine is refused with E_TYPE_RANGE, not E_DSP_ROWS.** Eight types, eight machine rows and eight voice rows are the same number, and the type assignment (assign.rs) gives out numbers before `dsp::check` runs. `E_DSP_ROWS` is reached by a voice that is not tied to a machine record (a pinned type), or when one module takes several rows (one module with 7 voices, one with 1, and a third with 1 is refused with -34: three modules, eight rows). `E_DSP_TYPE` is reached with a pinned type that is out of range or already used while rows are still free.
- **A file can be large without being big.** `bss` is not stored: a `.dkm` with a 64-byte image and a 400 KiB `bss` is a small file. The same holds for the DSP section (`bss`, and `voices x state_bytes` of a version-3 voice). So every ColdFire and DSP limit can be filled with files of a few hundred bytes.

### Tests today (finding 56 section 6): none fills a limit

The ColdFire-only rig exists and is cheap: `Ctx.rig(..., cf_only=True)` boots the patched firmware on a card in about 6 s cold (5.7 s measured in finding 39; 1 s when the boot cache hits), runs the UI at real time, reads the boot report (`rig.boot_info()` with one result per selection path), and speaks the SysEx protocol (`uploader(rig).modules()`, `set_selection`). `sticky-types` (nine boots) and `part_modules` use it. A module with a DSP section loads its ColdFire part and reserves its DSP range synchronously; without a DSP the result stays `LOADING` (1), which the existing scenarios accept as loaded. Every refusal above is decided before the DSP transfer, so a ColdFire-only boot sees all of them.

## Design

### 1. Limits in the profile

Rule: the contract maximum lives in `sdk/sdk.toml`, the device count in `[limits]`; `patchkit gen` refuses a device value above the maximum (message names both).

New table in sdk/sdk.toml, `[limits.max]` (names = `[limits]` keys): `modules` = 8 (the selection record holds 8 paths), `map_entries` = 8, `fx_slots` = 4 (= `DK_FX_MAX`, one source: the header copies stay and `sdkgen.rs` keeps checking them), `dsp_area_bytes` = 524288 (the 17-bit offset field of a link chunk), `param_rows` = 16 and `page_rows` = 8 (what core/rows reserves; a test ties them to the manifest), `machine_rows` = 8 (stays tied to types.toml by the existing assert).

New keys in devices/digitakt-ii.toml and devices/digitone-ii.toml `[limits]`: `fx_slots` = 4, `param_rows` = 16, `page_rows` = 0 on the DT2 (it has no page rows) and 8 on the DN2. Existing keys stay.

`SEL_MAX` and `MAP_N` are record format, not device values: sdk.toml `[limits.max]` generates them into gen_sdk.rs (one source for Rust and Python) and the format test keeps the byte layout fixed. They cap `modules` and `map_entries`.

Scope of each value (the runtime is one image): `modules`, `arena_bytes`, `stage_bytes`, `machine_rows`, `param_rows`, `page_rows` stay `Shared` (gen refuses a difference). `dsp_area_bytes` and `fx_slots` are `PerDevice`. `fx_slots` is read by patchkit at build time: `core/fx` keeps its registry cap from the contract (`max = 4`), and a new check in prim.rs refuses more effects than the device's `fx_slots`.

Python side: `pk_device.limits(short)` returns the `[limits]` of a device (and the contract maxima); pk_dkrt.py (`MAX_MODULES`, `ARENA_BYTES`, `SEL_MAX`) and pk_conform.py (`FX_MAX`) read it. A test fails when a copy reappears (grep of the numeric constants in tools/patchkit).

Emission into the runtime (`device.rs`, `.inc` files) changes only when the runtime consumes a new constant (the reporting step). The validation lands first so that the first PR does not touch a framework module.

`dsp-link.c`: the two numbers are the **protocol width** (a 17-bit word offset: 0x80000 bytes, 0x20000 words), not the device area, and the ColdFire object is shared by both devices. They become one generated shared constant from sdk.toml (`dsp_area_bytes` maximum, new gen kind `c-shared` writing a header with `#define`s). The real area is still checked by the runtime (`alloc_range`, `table::check` invariant 1) and by the DSP (status 3). Gap noted: on DN2 the sender accepts offsets up to 0x80000 while the area is 0x50000.

### 2. Fixtures: generated, no new module folders

A new `patchkit` subcommand `synth-dkm SPEC.toml -o FILE` (library function next to `pack_full` in packages/patchkit/src/dkm.rs; it uses `Register::encode` and the DSP section encoder, so the format has one source). Spec keys, all optional except `name`:

| Key | Meaning |
| --- | --- |
| `name` | module name (31 characters) |
| `image` | bytes of ColdFire image (default 4: `moveq #0,d0; rts`) |
| `bss` | ColdFire `bss` bytes (not stored in the file) |
| `pad` | extra file bytes (for `E_TOO_BIG` and the stage limit) |
| `dsp.image`, `dsp.bss` | DSP section image (zero words: SHARC NOPs) and `bss` |
| `dsp.voices` | list of voices: `type` (pinned number or `machine = "id"`), `state_bytes`, `voices` |
| `machines`, `params`, `pages` | counts: the generator makes records with hashed keys, one parameter each |
| `fill_dsp_byte`, `fill_cf_byte` | non-zero fill of the images (the isolation test looks for a changed neighbour) |

The specs are not files in the repository. A new `pk_capacity.py` in tools/patchkit computes the module set of each case **from the profile and the plan** (`pk_device.limits(short)`; `DKRT_ARENA` address from the plan for the exact arena size; the DSP area size from the profile), writes the spec TOMLs into the scenario's work directory and packs them. Nothing is hard coded: a device that sets `modules = 6` or another `dsp_area_bytes` gets its own sets.

Exact fill: arena block = `ceil16(n) + 16`; a first module takes the arena minus its own block minus the filler's header, so that the free list is empty (`arena_free == 0`, `largest == 0` in MODULES) and a 4-byte module is refused. The DSP area is filled the same way (`len` is rounded to 64, first fit from offset 0; the test checks that the first range starts at 0 and reads `dsp_free == 0`). The assumption (area base aligned) is asserted from MODULES, so a wrong assumption fails loudly and does not pass by accident.

### 3. Test levels, cases and cost

Levels (docs/plans/test-architecture.md): the loader needs the firmware's store, the real linked runtime and the DSP link table, so a host unit test of loader.rs would need a shim for the store, the link and the registry (about a day, and it tests a copy of the situation). It is not worth it. The cheapest level that tests the real code is **device-cf** (ColdFire emulator, no DSP). Coupled runs (**device-coupled**) are used only where PCM matters.

| Level | What | Why this level |
| --- | --- | --- |
| unit (cargo, packages/patchkit) | synth-dkm: output parses with `Dkm::parse`, sizes are exact, the generated spec is deterministic; devgen: a device value above the maximum is refused, a missing key is refused; sdk.toml `[limits.max]` matches `SEL_MAX`, `MAP_N`, dynreg rows, `DK_FX_MAX` | ms; no emulator |
| unit (pytest, tests/patchkit) | `pk_capacity` sets: for both devices the sizes add up to the profile (arena, DSP area, rows, modules); no numeric copy of a limit in tools/patchkit | seconds |
| device-cf | new `[debug] part = "capacity"` in tools/patchkit/pk_digikit_debug.py, scenario files `capacity-dt2.toml` and `capacity-dn2.toml` (kind `debug`, tier quick) | every refusal is decided on the ColdFire; about 6 boots per device |
| device-cf | `memaudit-dt2-full.toml` and `memaudit-dn2-full.toml`: the existing `memaudit` part with the full set | clean audit with every limit used |
| device-coupled | `isolation-dt2.toml`, `isolation-dn2.toml`: reference sine alone against reference sine in the full set; PCM equal | PCM needs the DSP |

Cases of the `capacity` part (one card and one cf-only boot each; independent boots run in threads with `Ctx.parallel`). `R` is a small reference module (a raw module with a registry-free image; the first of every set). Expected results are boot results unless noted; "loaded" = 0 or `LOADING`.

| Case | Set | Expected | Also checked |
| --- | --- | --- | --- |
| modules | 8 raw modules, distinct names | 8 x loaded; MODULES total 8 | `SELECT` of 9 paths answers `E_SEL_COUNT`; mailbox select of 9 answers `E_SEL_PATH`; a file with a repeated name -> `E_LOADED`; `modules <= SEL_MAX` (so `E_TABLE_FULL` is unreachable; documented, not tested) |
| arena | R, F (exact fill), X (4 bytes), Y (DSP image, tiny), Z (`bss` = arena), W (`bss` > arena) | loaded, loaded, -8, -8 (DSP reserve site), -8, -5 | MODULES after: `arena_free == 0`; Y left no DSP range (`dsp_free` same as before Y); R and F records unchanged |
| stage | R, S (file = `stage_bytes` + 1) | loaded, -13 | file = `stage_bytes` exactly loads (if the store allows it) |
| dsp area | R(DSP), A (extent = rest of the area), B (64 bytes), C (extent = area + 8), D (extent = area) | loaded, loaded, -33 (reserve), -33 (check), -33 | `dsp_free == 0` after A; ranges disjoint; B, C, D left nothing |
| voice rows | A (8 pinned voices, types 16..23), B (1 voice, pinned 16), C (1 voice, pinned 40) | loaded, -34, -34 | rows free 0; a second boot: A (7 voices) + B (1) loaded, C (1) -> -34; D with pinned 16 while rows are free -> -37; 9 voices in one file -> -31 |
| machine rows | A (8 machines, hashed), B (1 machine) | loaded, -22 | MODULES `types` of A: 16..23; the sticky map has 8 entries |
| parameter rows | A (16 parameters on 8 machines), B (1 parameter) | loaded, -24 | |
| page rows (DN2) | A (8 pages), B (1 page) | loaded, -25 | DT2: skipped (no page rows) |

After every case: `DKRT_TABLE_BAD == 0` (no `E_TABLE`), no `TABLE_BAD` event, the log has one `MOD_LOAD` per path with the same result, the write-watch over `cf.expansion` has 0 stray writes and the flush check 0 hits (the common checks of the runner already do this), and the report from the new command (step 5) equals the profile and MODULES.

Refusal leaves nothing behind: for every refused module the test reads MODULES before the module would have loaded (from the same boot's earlier records) and after; arena figures, DSP figures, row masks, `types` of earlier records are identical. Because the set is fixed per boot, "before" is a second boot of the same card without the refused path (its results must equal the first boot's results for the other paths: same arena ranges, same DSP ranges). That doubles the boots for the arena and DSP-area cases only; the others compare the records inside one boot.

Isolation (coupled): two boots in parallel. Boot 1: R alone (`test/sine`: runs on both devices, 64 B images, about 2 KiB of DSP). Boot 2: R first, then the full set of the `capacity` cases that load (fillers with non-zero fill bytes in ColdFire and DSP images, an exact arena fill, an exact DSP area fill, all rows used, then refused modules behind them). Both select the machine, play a note and capture PCM (the equivalence code). Checks: results of R equal; R's MODULES record (range, rows, type) equal; PCM of R equal from the onset (`exact`, 0 samples differ); common checks. On DN2 `digikit/pluck` does not fit next to anything (256 KiB of 320 KiB), so `test/sine` is the reference on both devices; a pluck variant for the DT2 may run in the full tier later.

Memory audit: the same set under `memaudit` (guard on all free ColdFire regions outside each module's block). Modules in the set that never run cannot store; `test/dyn-demo` is `must_run`. The audit proves that the loader's own copy and relocation stayed inside each block when the arena is full.

Expected cost per new test (cold boots, idle 8-core machine; to be measured with `DIGIKIT_PROFILE=1`, one paired run each; the estimates come from finding 39 and the memaudit times):

| Test | Boots | Wall alone | CPU |
| --- | --- | --- | --- |
| cargo units (synth, devgen) | 0 | under 5 s on top of the existing patchkit tests | small |
| pytest units | 0 | 2 s | small |
| `capacity-dt2` or `-dn2` | 6 to 8 cf-only | 20 to 30 s (parallel boots), 60 s CPU | 1 to 4 cores |
| `memaudit-*-full` | 2 (cf-only) | 15 to 25 s | 2 cores |
| `isolation-*` | 2 coupled | 40 to 60 s | 4 cores |

The quick tier is 38 to 44 s wall today (three scenarios at once). Adding the capacity and memaudit files in parallel keeps the wall time near 45 to 60 s; the isolation scenarios add about 4 cores for 40 to 60 s. If the paired measurement shows more than 60 s added to the quick tier, `isolation-dt2` moves to the full tier and `isolation-dn2` stays quick.

### 4. Reporting rows used and free per type

`MODULES` (command 0x03) has a fixed head (4 x u32, `total`, `n`) and variable records that the host pages through; appending fields to the head or the record breaks old hosts (a 0.x MINOR change). Instead:

- New command `CAPACITY` = 0x04 and capability bit 6, in sdk.toml (`[protocol.commands]`, `[protocol.caps]`), generated into dkproto.py and gen_proto.rs. An old runtime does not set the bit; hosts ask only when it is set.
- Reply: `u8 version` (1), `u8 n`, then `n` entries of `u8 kind, u32 limit, u32 used, u32 largest` (largest free: 0 where it has no meaning). Kinds (a table in sdk.toml, additive): modules, selection paths, arena bytes, stage bytes, DSP area bytes, machine rows, voice rows, parameter rows, page rows, fx slots (limit only; run-time effects do not exist: used 0), map entries. New limits add a kind and no layout change.
- The runtime fills it from the same tables as MODULES and from the generated `LIMIT_*` and the device object (`dsp::area_bytes`, dynreg info). `DkrtHealth` and `DkrtStatus` stay as they are; the on-device MODULES page may show one line from them later (the menu is a framework module and would pull in its smoke cell; not in this plan).
- `Uploader.capacity()` in tools/device/dkupload.py; `dk:store info` prints one line per kind (`used/limit`) and says "not reported" for a runtime without the bit; `dk:store modules` unchanged. The Rust client (packages/dkupload) has no MODULES today and gets no CAPACITY; its generated constants update by `gen`.
- Test: the existing `part_modules` (debug-log, quick) checks CAPACITY against MODULES after its two-module boot (modules 2/8, voice rows 1/8, machine rows 2/8, arena and DSP figures equal to MODULES); the capacity cases check `used == limit` where a limit is full. Cost: no extra boot.

### 5. Version, compatibility, images

- The runtime changes only in the reporting step (a new command). Under docs/design/module-sdk.md D9 and the sdk.toml note ("0.x: MINOR may break, PATCH = features and fixes") an additive command with a capability bit is a PATCH: **0.2.1 -> 0.2.2**. No contract version changes (`api` stays [1, 1]); `.dkm` files need no rebuild (`runtime_min` is informational).
- New flash images are needed only to get the new command. A device on 0.2.1 keeps working with every host (the capability bit is absent). Per the standing rule there is no flashing before everything works in the emulator; this PR is emulator-only.
- Committed objects (modules/core/runtime/build/*.dko.json, dsp-link-cf) are rebuilt by `pk_build` with the Rust-for-ColdFire and C toolchains; `test_patchkit_dkrt` checks they are current. The toolchain must be present on the machine before the runtime step starts.
- Host compatibility: Python `dkproto` is generated; `Uploader.capacity()` checks the cap bit. The Rust `packages/dkupload` only gets new generated constants. No change to the store format, the selection record or the log kinds.
- PCM pins in tests/scenarios do not change; the isolation scenarios compare two runs of the same image against each other (no new pin, no approval needed).

## PR steps (each step is testable alone)

The work splits into two PRs so that the tests land without a runtime rebuild: **PR 1** (steps 1 to 5, no framework module touched) and **PR 2** (step 6, runtime and dsp-link). Check each step's selection with `mise run test:changed -- --explain` before the first edit and record it in the log (devices/*.toml and sdk/sdk.toml may route broadly; if they do, step 1 is split into the gen rule and the profile keys).

| Step | Change | Iterate with | Gate adds |
| --- | --- | --- | --- |
| 1. limits | sdk.toml `[limits.max]`; `[limits]` keys in both device files; devgen.rs parse and refusal (no new emission); `pk_device.limits()`; pk_dkrt.py, pk_dkrt_boot.py and pk_conform.py read it; guard test for numeric copies; prim.rs check of `fx_slots` against effects | `test:changed` (patchkit cargo tests, test_pk_current, tests/patchkit unit files); `patchkit gen --check` | patchkit suite, build-daemons |
| 2. synth-dkm | dkm.rs library function, CLI subcommand, unit tests (parse back, exact sizes, determinism, refusals) | `cargo test -p patchkit` through `test:changed` | patchkit suite |
| 3. capacity sets and part | pk_capacity.py (sets from the profile), `part = "capacity"` in pk_digikit_debug.py, `capacity-dt2.toml`, `capacity-dn2.toml`, pytest units for the sets, docs/TESTING.md | `test:changed`; `pk_digikit_scen run capacity-dt2 --image dt2-1.17` then DN2, bounded, `DIGIKIT_PROFILE=1` | digikit-boot suite |
| 4. audit and isolation | the full set under `memaudit` (`memaudit-*-full.toml`), `isolation-*.toml` and its part in pk_digikit_debug.py (or pk_digikit_scen.py, whichever holds the PCM code); control: the DSP byte check of the reference's range (open point 8); the existing `stray` modules stay the ColdFire control | `test:changed`; the two scenarios once per device | digikit-boot suite |
| 5. docs | finding 57 (marks **[D]**; **[V]** only after a second agent checks the sets against the image bytes), update finding 56 sections 6 and 9, TESTING.md, devices.md reference (limits table for authors), design note in module-sdk.md 6.2 | none | none |
| 6. reporting (PR 2) | `CAPACITY` in sdk.toml, runtime (upload.rs and lib.rs), `RUNTIME_VERSION` 0.2.2, generated files, dkupload.py `capacity()` and `dk:store info`, dsp-link.c generated constant (`c-shared` kind), emission of the new `LIMIT_*` constants, `part_modules` check | `test:changed`; `verify:pre` once | `mise run verify` (framework: all tests/patchkit and tests/digikit, smoke) once, by the coordinator |

Order inside PR 1: steps 1 and 2 are independent and can run in parallel lanes; step 3 needs both; step 4 needs 3. Every step ends with `mise run verify:pre` only at the end of PR 1 (once) and PR 2 (once).

Commit points: after each step with its tests green; docs last.

## Risks

1. **Exact fill depends on allocator internals** (16-byte header, 16-byte alignment, the `reg` copy and the DSP image copy that `reserve` takes from the arena). Mitigation: sizes from the plan symbol and the profile, verified by MODULES figures in the same boot; a wrong assumption fails the test and does not pass it.
2. **A ColdFire-only rig cannot see a DSP transfer fail** (`E_DSP_XFER`, timeouts). Not in scope; the isolation scenarios run the transfers coupled.
3. **The isolation control**: PCM equality cannot show that it would fail when something is clobbered, and the runtime cannot be made to overlap two ranges without editing it. The memaudit has a negative control for ColdFire writes (the `stray` modules). For the DSP the plan adds a byte-level check in the coupled boot (the reference's range read back from L2 equals its packed image); see open point 8.
4. **`E_TABLE_FULL` stays untested** while `modules == SEL_MAX`; the plan tests what is reachable and states the gap.
5. **Quick tier cost**. Measured pairs decide the tier of each scenario (open point 5).
6. **Runtime step needs the ColdFire Rust toolchain** and rebuilds committed objects; a missing toolchain blocks PR 2 only.
7. **Per-device limits against a one-image runtime**: a limit that differs per device (other than the DSP area) needs the runtime to read it from a device object. This plan does not add one; `gen` keeps refusing the difference.
8. **Bugs found by the tests** (the aim). A runtime fix moves the work into the framework gate (PR 2 scope). Budget one fix per two limits in the estimate.

## Estimate (agent time) and payoff

Payoff: users will upload their own modules to a device. The loader is the only guard between a bad or merely large module and the sound engine. Today nothing proves that a full loader refuses cleanly or that earlier modules survive. Confidence that the work finds at least one real defect or documentation error: medium (the `E_TABLE_FULL` and `E_DSP_ROWS` findings above came from reading alone).

| Step | Hours | Confidence |
| --- | --- | --- |
| 1 limits | 2 to 3 | high |
| 2 synth-dkm | 3 to 4 | medium-high |
| 3 capacity part | 5 to 7 | medium (rig quirks, exact arithmetic) |
| 4 audit and isolation | 3 to 4 | medium |
| 5 docs | 1 to 2 | high |
| PR 1 total | 14 to 20 (about 2 days) | medium |
| 6 reporting and dsp-link constant (PR 2) | 6 to 9 | medium (toolchain, full gate) |

Outcome risk, separate from time: the tests may expose a loader defect (probability medium-low); fixing it adds 2 to 6 hours and a runtime rebuild. Recommendation: do it. The stopping rule: stop and report after step 3 if the capacity scenario costs more than 90 s wall in the quick tier or needs a runtime fix of more than a day.

## Open points (for the coordinator)

1. **E_TABLE_FULL is unreachable** (modules == SEL_MAX == 8). Options: (a) test `E_SEL_COUNT` and the invariant `modules <= SEL_MAX`, keep `E_TABLE_FULL` as documented defence; (b) build a test runtime with a lower `modules`; (c) change the selection record format. Recommendation: (a).
2. **Which code a full machine table gives.** The brief expects `E_DSP_ROWS`; a hashed machine gets `E_TYPE_RANGE` (-22) and only pinned or multi-voice modules reach `E_DSP_ROWS` (-34) or `E_DSP_TYPE` (-37). Codes are never renumbered. Recommendation: test the codes as they are and document the path of each; do not unify.
3. **Extra limits** beyond the brief: parameter rows (16, `E_PARAM_FULL`), page rows (8 on DN2, `E_PAGE_FULL`), staging buffer (`E_TOO_BIG`). Recommendation: include all three (cheap, same fixtures) and add `param_rows` and `page_rows` to `[limits]`.
4. **Two PRs or one.** PR 1 (profile, fixtures, capacity, isolation) touches no framework module; PR 2 (reporting) rebuilds the runtime and routes the whole digikit and patchkit suites. Recommendation: two PRs.
5. **Tier of the new scenarios.** Recommendation: `capacity-*` and `memaudit-*-full` quick on both devices; `isolation-dn2` quick, `isolation-dt2` quick if the measured addition to the quick tier is under 60 s, else full.
6. **Fixture generator language.** Rust `patchkit synth-dkm` (one format source, cargo-testable) against a Python writer (faster to write, a second copy of the format). Recommendation: Rust.
7. **Reporting channel.** New command `CAPACITY` 0x04 with capability bit 6 and runtime 0.2.2 (additive, old hosts unaffected) against a change of the MODULES layout (a 0.3.0 break). Recommendation: new command; no change to `DkrtHealth`, `DkrtStatus` or the on-device MODULES page in this work.
8. **Isolation control.** Without a way to force an overlap, PCM equality cannot show it would fail. Recommendation: add a byte-level check in the coupled boot (read the reference's DSP range back from L2 and compare with its packed image) and keep the memaudit `stray` modules as the negative control for ColdFire writes; do not edit the runtime for a control.
9. **dsp-link.c constants.** They are the protocol offset width, not the device area, and the object is shared. Recommendation: one generated shared constant (new `c-shared` gen kind from sdk.toml); the device size stays checked by the runtime and the DSP. The DN2 gap (sender accepts 0x80000 while the area is 0x50000) is recorded as **[O]**, not closed here.
10. **Per-device limit scopes.** Keep `modules`, `arena_bytes`, `stage_bytes`, `machine_rows`, `param_rows`, `page_rows` Shared; `dsp_area_bytes` and `fx_slots` PerDevice. Recommendation: keep `fx_slots` = 4 on both devices (the DN2 chain cost is x3.09 in the emulator, finding 55; lowering it is a product choice for Em, the field makes it a one-line change).
11. **Mailbox select code.** `CMD_SELECT` through the mailbox answers `E_SEL_PATH` for a ninth path, `SELECT` over SysEx answers `E_SEL_COUNT`. Recommendation: record it; fix to `E_SEL_COUNT` in PR 2 only if the change is a one-liner (test hook only).
12. **Reference module of the isolation test.** `test/sine` on both devices (small DSP, runs on both, fits next to the fillers) against `digikit/pluck` (not on DN2). Recommendation: sine; a pluck variant on DT2 in the full tier later.
13. **Toolchain.** PR 2 needs the Rust-for-ColdFire and C toolchains for the committed objects. Recommendation: the runner checks `pk_toolchains.codegen_gcc_ready()` at the start of step 6 and reports before any edit.
