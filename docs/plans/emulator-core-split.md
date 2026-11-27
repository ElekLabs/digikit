# Emulator core split

Goal: ordinary builds (workbench, daemons, tests, lanes) stop compiling the generated SHARC core, and the emulator gets one
boundary for its second processor, so the Syntakt (ColdFire MCU + a second ColdFire) fits beside DT2/DN2 (ColdFire + SHARC+).
This stack lands on main before the Syntakt branch; the Syntakt branch then rebases and adds its peer CPU as a companion.

## Problem

- `sharc-gen` holds the transpiled SHARC firmware: DN2 117 MB (about 3.1M lines), DT2 43 MB, in one crate each. rustc's front
  end runs mostly on one thread for a crate, so a cold compile takes 3 to 4 minutes (10 or more under load), and the two-core
  workbench compiles two of them (`sharc-gen`, `sharc-gen-2`).
- Every target dir (`native_target.py`: per core key, per PGO variant, per worktree) compiles its own copy. A `sharc-rt` edit,
  a toolchain change or a regeneration recompiles it everywhere.
- A second core is three duplicate crates (`sharc-2`, `sharc-fast-cl-2`, `sharc-gen-2`), a double `include!` of
  `sharc_engine.rs` in `boot/src/sharc_peer.rs` and a `Core { First, Second }` match in `coupled/src/dsp_core.rs`.
- The Syntakt branch adds its peer CPU beside the SHARC path (`cf_audio.rs`, feature `cf-audio`, `companion()` strings).

Rejected before: lower opt-level (opt 1: 2.5x the build CPU; opt 0: 100x slower runs), sccache (0 hits), nightly `-Z threads`.

## Design

### P1: one engine, the core as a value (PR 1, PR 2)

- `sharc_rt::GenCore`: a plain Rust struct of `&'static` tables and `fn` pointers: `BLOCKS`, `CODE_RANGES`, `REGION_ENTRIES`,
  `MODEL_SAFE`, `LOOP_ENDS`, `INSN_BLOB`, `insn_at`, image name and hashes, `GEN_EXPLICIT_MEMORY_MODEL`, `TRAP_SITES`,
  `SYM_NAMES`, `sym_of`, `write_ureg`, the interrupt and sync entry points, and `exec_insn`. `GenCore::NONE` replaces the
  `sharc_gen`, `sharc_image`, `sharc_region_entries`, `sharc_gen_version` and `sharc_second` cfgs.
- `St.core: &'static GenCore`: the fast tier's helpers reach the interpreter with only `St`.
- `exec_insn` moves from `sharc/src/lib.rs` into the hand-written glue of `sharc-gen/src/lib.rs` and is exported whole. The
  generated `_execute` and `_core_timer_tick` are `#[inline]`; keeping them in the core crate keeps them inlinable, and the
  boundary stays one non-inlined call per interpreted instruction, as today. No generator change, no regeneration.
- `Engine::new(mem, core)`. Delete `sharc-2`, `sharc-fast-cl-2`, the double include and `Core { First, Second }`.
  `sharc-gen` and `sharc-gen-2` stay as data crates until P2.

### P2: the core as a loaded library from a shared store (PR 3 to PR 5)

- `sharc-core-lib` (cdylib over `sharc-gen`) exports `sharc_core_header()` (a `#[repr(C)]` header: ABI version, `sharc-rt`
  source sha256, rustc version, target, layout fingerprint of `St`, `Mem`, `Insn`, `V`, `Cfg`) and `sharc_core()`
  (`&'static GenCore`). The host reads `sharc_core()` only when the header equals its own; otherwise the load is refused.
- Only `&mut St` and `fn` pointers cross, with the Rust ABI. The library is never unloaded. The loader reuses the `dl` module
  of `live/src/sharc_lib.rs`; the copy in `sharc/src/fast/plugin.rs` is deleted.
- The library carries its own `sharc-rt`. Its globals are safe per copy: `POOL` (no pointer equality), `TABLES` (keyed by blob
  address; the host warms the core's `insn_at`), `NEXT_LAYOUT_GEN` (only `Mem::new`, which generated code never calls),
  `bnd.rs` (environment values, log counters).
- Store: `<git common dir>/sharc-core-cache/<image>/<key>/` (library, `key.txt`, header JSON). Insert by one rename; keep the
  last 4 keys per image plus every key a worktree's `dist/daemons.json` names. The helper is extracted from
  `tools/devtools/verify_build.py`, not copied.
- Key: sha256 of the gen dir files, the `sharc-rt` source, `rustc -vV`, target and CPU flags, profile, PGO profile hash or
  `plain`.
- A miss is built by `verify_build cores`: one Nx target that `digi-control:daemons` depends on; `native_emu.py` calls the
  same function. If the build fails, the caller stops with the build command in the message (no silent zero DSP).
- Lookup: `DIGI_SHARC_CORE_<DEV>` in `dist/daemons.json`. The loaded `GenCore.image` must equal `DspProfile.image`; this
  replaces the image-name prefix match in `coupled/src/dsp_core.rs`.
- Wasm and the C-ABI harness link `sharc-gen` statically and use the same `GenCore` value.
- PGO: one training run with both images, one merged profile. If PGO parity fails for the library, PGO builds stay static
  and only plain builds use the store.

### P3: the companion processor (PR 6 to PR 8)

- PR 6, pure move: `sharc_peer.rs`, `sharc_engine.rs`, `dsp_patch`, `ddr_monitor`, `dsp_core.rs`, `cold.rs` into a new crate
  `sharc-peer`. `boot` has no SHARC dependency after it.
- PR 7: `AudioOut` (player, tap, capture, pacing; now in `coupled/src/lib.rs` 581-805) is the one audio output path.
- PR 8: `coupled::Companion`; `CoupledRuntime` holds `Box<dyn Companion>` and one `AudioOut`. Methods: `kind() -> Kind`
  (`Kind::as_str` keeps "peer", "zero", "sharc", "responder", "none"), `sync()`, `poll(&mut Emulator, &mut AudioOut)`,
  `halted()`, `export_state(&mut Emulator)`, `report(&Emulator)`, `as_any()` and `as_any_mut()` for SHARC-only data, and
  `step_begin()` (a timing mark before the ColdFire step, empty by default). `sync()` is the barrier only: the runtime
  calls `poll` after it to push the PCM it completed. `SharcCompanion` lives in `coupled` (the trait takes `AudioOut`,
  which is there; `sharc-peer` cannot depend on `coupled`). `Kind` has "sharc", "zero" and "none"; "peer" and "responder"
  join it with their companions.
- Scheduling stays outside the trait: the SHARC runs on a worker thread behind `periph::dspi::Peer` and `sync()` is its
  barrier; the Syntakt peer runs in lock step inside the board and `sync()` does nothing. No lock-step methods on main
  until the Syntakt rebase shows a second user.
- The device profile picks the companion (`FirmwareProfile.dsp`: SHARC, image, idle loop); options (zero, responder)
  override it. The `DT2_/DN2_IDLE_*` constants in `boot/src/sharc_peer.rs` are deleted.
- Cargo features: `coupled-audio` becomes `sharc` + `play` in PR 8, with the documented commands. `second-core` is deleted in
  PR 5. `cf-audio` never lands on main.
- `DspEngine` stays as the SHARC crate's internal test seam.

### P4 (deferred)

The generator could emit N crates for a parallel cold compile. That changes `CORE_FINGERPRINT` and needs a regeneration.
Only if cold builds still hurt during generator work; N crates would feed one `GenCore`.

## PR stack

| PR | Content | Lane | Depends on |
|---|---|---|---|
| 0 | CF `state_sha256` in the `coupled-run` report and a column in `tools/devtools/sharc_cells.py` (documented in its doc) | B | none |
| 1 | `GenCore`, `St.core`, `exec_insn` into the core glue; the engine reads only the table (default `&sharc_gen::CORE`) | A | none |
| 2 | `Engine::new(mem, core)`; delete `sharc-2`, `sharc-fast-cl-2`, the double include, `Core { First, Second }` | A | 1 |
| 3 | Slice: `sharc-core-lib` and the loader, DN2, opt in by env | A | 2 |
| 4a | Store tooling: extracted store helper, key, `verify_build cores`, eviction (Python, Nx) | B | none |
| 4b | Store wiring: Nx target, `daemons.json` entries, DT2, profile image check | A + B | 3, 4a, 6 |
| 5 | Loading by default; delete `sharc-gen-2`, `second-core`, `SHARC_GEN_DIR_2`, gen keys of `native_target.py`; PGO retrain | A | 4b |
| 6 | Pure move to `sharc-peer` | C | 2 |
| 7 | `AudioOut` | C | none |
| 8 | `Companion`, `Kind`, profile selection, feature split | C | 6, 7 |

PR 6 lands before PR 4b, so the store wiring edits the moved `dsp_core.rs`. If the slice (PR 3) fails, PR 6 to 8 follow PR 2.

Lanes: A `emu-core` (branch `emu/core-split`), B `emu-store`, C `emu-companion`. Each lane caps cargo at 4 jobs; at most two
release builds at once (`benchlock`); A/B runs one after another. Lane A settles `sharc-rt` and `sharc-gen` glue edits in its
first commit, because every edit there recompiles both generated cores. The merge-base A/B binaries are built once and kept.

## Proof

- Every PR: host tests of the touched crates, the static checks (`lint,fmt-check,rust-fmt,typecheck,python-checks`), and
  `sharc_cells.py A=<merge base> B=<branch> --state dn2-reset --state dt2`: PCM, DSP state sha and (after PR 0) CF state sha
  identical; host instruction ratio within 0.5 %. No device tests per PR.
- PR 3 and PR 5 also: the conformance rig on the loaded core (module DSP code loaded at run time) and the slice pass marks:
  1. PCM, DSP state and CF state equal to the static build.
  2. Host instruction ratio within 1 %, plain and with PGO.
  3. A library built from an edited `sharc-rt` is refused.
  4. DN2 and DT2 libraries in one process pass their cells.
  5. The conformance rig passes on the loaded core.
  6. A cold lane build of the daemon compiles no `sharc-gen`; record the time.
- PR 4a/4b: store miss, hit and eviction runs.
- PR 7 and PR 8: host tests with a fake player and a fake companion.
- Once for the stack: `mise run test:subset -- S:fx --run` (adds `gate-smoke` and `gate-pins`) on DT2 and DN2. It covers the
  daemon control path and core loading from the store, which the A/B does not.
- `mise run test:full` once in the background batch after the merge.

## Estimates (agent hours)

PR 0: 0.5. PR 1 + 2: 2-3. PR 3: 2-3. PR 4: 2-3. PR 5: 1-2. PR 6: 1. PR 7: 1-2. PR 8: 2-3. Critical path (lane A) 8-10 hours.
Risks: the slice fails its pass marks (known after PR 3); PGO parity for the library; machine load slows builds 2-3x.

## Status

All items are done; P4 stays deferred.

- [x] P0 cold build, measured instead of profiled: a cold two-core `coupled-run` build took 22 min 29 s (load 30-45); after PR 5a a cold `coupled-run` build takes 1-1.5 min and compiles no `sharc-gen`. A store hit for both core libraries takes 0.2 s.
- [x] PR 0 CF state digest in the A/B tool (`state_sha256`; `sharc_cells` fails on a CF difference)
- [x] PR 1 GenCore table
- [x] PR 2 one engine
- [x] PR 3 core library slice
- [x] PR 4a store tooling
- [x] PR 4b store wiring
- [x] PR 5a loading by default: hosts hold no generated core; `sharc-gen-2`, `second-core` and `SHARC_GEN_DIR_2` deleted; the header flags hash leaves out the PGO flags
- [x] PR 5b PGO libraries: training builds and loads instrumented libraries, one profile per target covers host and libraries; profile-guided libraries are store entries keyed by the profile content
- [x] PR 6 move to sharc-peer
- [x] PR 7 AudioOut
- [x] PR 8 Companion

Proof of the stack: every PR's `sharc_cells` A/B against the static baseline is identical in PCM, DSP state and CF state on dn2-reset and dt2; the final host instruction ratio is 1.0010-1.0015 (plain host and libraries). PGO host and PGO libraries run at the speed of the old static PGO build (`coupled_ab`: dn2 1.002, dt2 0.994 DSP CPU per audio second; PCM equal) with a 8.6 MB host binary instead of 70.9 MB. Device subset `C:dsp` (11 targets on DT2 and DN2, with gate-smoke and gate-pins) passes.

After the merge: run `mise run perf:pgo` once (the profiles are stale after the sharc-rt change).
