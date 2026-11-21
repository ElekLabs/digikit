# Native crates

The Rust side of the emulator: a ColdFire core, board and peripheral models, the
SHARC+ DSP engine, the coupled run loop, and the desktop and browser hosts.

## Build and run

Use mise (`mise tasks` lists every task). Cargo needs the generated SHARC+ core;
the tools set `SHARC_GEN_DIR` and the target dir for you (see "Target dirs").

```
mise run workbench -- --build-only   # build the desktop emulator without launching it
mise run workbench         # build and run it (DN2 audio when the ready inputs exist)
mise run scenario:build    # build the headless runner coupled-run
mise run scenario:all      # run every DN2 scenario against its pinned PCM hash
mise run test:quick        # fast checks for the tooling and the coupled runtime
node tools/nx/rust-env.mjs --gen -- cargo check --workspace --exclude digiemu-desktop --all-targets --locked
mise run web:wasm          # browser cores
```

The ready inputs come from `mise run emu:ready -- dn2|dt2` (see the top README).

## Crate map

One cargo workspace (root `Cargo.toml`, one `Cargo.lock`, profiles defined at the
root). The directory name is not always the package name; use the package name with
`cargo -p`.

| directory | package | role |
|---|---|---|
| `packages/emulator/coldfire` | `coldfire` | ColdFire V4e core (ISA_C, EMAC); decoder generated from `tools/coldfire/cfisa/coldfire.json`. Pure, builds for wasm32. |
| `packages/emulator/periph` | `periph` | MCF5441x timer, interrupt controller, eDMA, DSPI and SSI models. |
| `packages/emulator/card` | `emmc-card` | Minimal eMMC card identity and media model (sparse write overlay). |
| `packages/emulator/machine` | `machine` | The board seam: guest RAM, MMIO, eMMC DMA, timers. |
| `packages/emulator/loader` | `dt2-firmware-loader` | Decoder for the `.syx` container (ELE3 sections, no I/O). |
| `packages/emulator/device-profile` | `device-profile` | Hash-bound device and firmware identities, embedded from `devices/*.toml`. |
| `packages/emulator/plusdrive` | `plusdrive-format` | +Drive image format encoders; the `plusdrive` CLI. |
| `packages/emulator/boot` | `elektron-native-boot` | Bounded cold boot, the ColdFire runtime (no SHARC dependency: the DSP side is the `periph::dspi::Peer` trait), and the examples (`card_boot`, `turn_probe`; the headless tools `boot_gate`, `panel_drive` and `sysex_bridge`, see their headers). |
| `packages/emulator/sharc-peer` | `sharc-peer` | The SHARC+ side of a coupled run: the DSPI2 peer and worker thread (`peer.rs`), engine glue, DSP patches and streams, DDR monitor; with feature `host` the core selection (`dsp_core`), the cold boot (`cold`) and the examples `dsp_cold`, `sharc_live`, `regions_probe`. |
| `packages/emulator/wasm-core` | `digi-wasm-core` | The browser core: the raw wasm ABI (`abi.rs`) over `boot`, with features `sharc` and `fast-wasm` for the coupled core. Built by `tools/nx/wasm.mjs` into `emulator-core*.wasm`. |
| `packages/emulator/sharc-rt` | `sharc-rt` | Runtime under the generated core: state, memory, decoder, instruction table, and `GenCore`, the table of one generated core. |
| `packages/emulator/sharc-gen` | `sharc-gen` | The generated SHARC+ core (from `SHARC_GEN_DIR`), compiled apart from the engine; `sharc_gen::CORE` is its `GenCore`. Only the core library, `sharc-rig`, the browser core and the tests and examples of the engine link it; the hosts do not. |
| `packages/emulator/sharc-stats` | `sharc-stats` | The counters and profiles the engine reports (shared by every engine). |
| `packages/emulator/sharc` | `sharc-native` | The DSP engine: interpreter, AOT block dispatch and the fast tier (`src/fast/`). It runs the `GenCore` it is opened with (`Engine::new(mem, core)`). |
| `packages/emulator/sharc-fast-cl` | `sharc-fast-cl` | Cranelift backend for the fast-tier kernel IR. |
| `packages/emulator/sharc-core-lib` | `sharc-core-lib` | One generated core as a shared library (cdylib over `sharc-gen`, `SHARC_GEN_DIR` picks the device): `sharc_core_header` and `sharc_core`. Every host loads its cores from these. See "Core libraries". |
| `packages/emulator/dynlib` | `dynlib` | `dlopen`/`LoadLibraryW` and symbol lookup, no dependencies (used by `live` and the core loader). |
| `packages/emulator/sharc-fast-wasm` | `sharc-fast-wasm` | WebAssembly backend for the same IR, plus the V8 gate tools. |
| `packages/emulator/live` | `live-audio` | Live audio output: SPSC ring, frame repeater, cpal callback. |
| `packages/emulator/coupled` | `digi-coupled` | The coupled run loop shared by the app and `coupled-run`: `CoupledRuntime` over a `Companion` (the SHARC+ session `SharcCompanion`, the zero answerer or none) and one `AudioOut` (player, tap, capture, pacing); the fixture gates. Features `sharc` (the DSP engine; its core comes from a core library) and `play` (output device). |
| `packages/emulator/control` | `digi-control` | Controller and control protocol (JSON-RPC over NDJSON); the `digi-emud` daemon. Design: `docs/design/emulator-control.md`. |
| `apps/desktop/src-tauri` | `digiemu-desktop` | The desktop app (Tauri), a thin shell over `digi-control` and `digi-coupled`. |

Left out of the workspace: `.claude/` (agent worktrees live inside the tree and must
not become members). A crate is never built with its own manifest's profile: cargo
reads profiles only from the root.
built with its own manifest's profile: cargo reads profiles only from the root.

## Profiles

- `release`: timing and shipping profile (opt-level 3, cargo's default codegen units,
  unwinding panics: what the desktop app has always used; PGO is trained on it).

There is no cheaper profile for correctness-only runs. Measured on `sharc-gen`
(80 MB generated core): an opt-level 1 override costs about 2.5x the CPU of
release (5.5 min wall vs 2.9 min), and opt-level 0 compiles in about 1.4 min but
runs `coupled_ready_exactness` in 150 s instead of a few seconds (and overflows
the default thread stack). Use `release`.

## Target dirs

`sharc-gen` (the generated SHARC+ core, 80 MB of Rust) takes minutes to compile
and cargo recompiles it for each target dir and each `SHARC_GEN_DIR`. The hosts (the app, `digi-emud`,
`coupled-run`) do not link it: they build into the `nogen` dir and load each core from a library (see "Core libraries").
The core libraries, the engine's tests and the examples that link a core build into
`out/native/target/<gen-key>` (`tools/emulator/native_target.py`):

| key        | generated core                                         |
|------------|--------------------------------------------------------|
| `prod`     | `out/native/dn2-audio/gen` (its library, tests, examples) |
| `nogen`    | none (`SHARC_GEN_DIR` unset): the hosts, CF-only builds, wasm CF |
| `generic`  | any other transpile-only gen (no `image.rs`)           |
| `g-<hash>` | any other gen with an image: hash of its resolved path |

Everything for one gen goes into its key dir, built with plain `--release`
(`-p sharc-core-lib`, `-p sharc-fast-cl --examples`, `packages/emulator/sharc` tests), so `sharc-gen` compiles once. Builds with different RUSTFLAGS
(PGO) get a subdir of the same key: `pgo-instrument/`, `pgo-use/`. Switching gen
never touches another gen's build.

```
export SHARC_GEN_DIR=$PWD/out/native/dn2-audio/gen
T=$(uv run python -m tools.emulator.native_target dir)
cargo build --release --locked --target-dir "$T" -p sharc-fast-cl --examples
```

In a git worktree the root is `dist/native-target/` inside the worktree instead
(git-ignored; it is deleted with the lane). A worktree cannot reuse the main
tree's compiled `sharc-gen` (different package paths and fresh file mtimes make
cargo rebuild it), so it pays one compile per gen, then reuses its own dir.
A live lane that built into the old `out/native/target-wt/<name>/` pays one cold
build the next time it builds; that old dir stays in `out/` until removed by hand.

A plain `cargo build` without `--target-dir` uses `./target` (ignored by git).
Wrap heavy builds in `uv run python -m tools.devtools.benchlock run --shared -- ...`.

## Core libraries

A host holds no generated core. The DSP of each device comes from a core library, one per device, built from the
device's generated directory (`packages/emulator/sharc-core-lib`, a cdylib over `sharc-gen`). `digi-emud`, `coupled-run`
and the desktop app load it when a firmware starts coupled: `sharc-peer/src/dsp_core.rs` (`core_for_device(device,
dsp_image)`) reads `DIGI_SHARC_CORE_LIB_DN2` or `DIGI_SHARC_CORE_LIB_DT2`. A missing variable, a refused library or a core
made for another device is an error that names the variable and the command that provides the library:

```text
uv run python -m tools.devtools.verify_build cores [--device dn2,dt2] [--env-out FILE]
```

That command builds the library into the store, `<git common dir>/sharc-core-cache/<device>/<key>/` (shared by every
worktree; tools/devtools/core_store.py), or restores it, and writes the two variables. The launchers call the same code:
`tools/emulator/native_emu.py` (workbench and `emu:serve`), `tools/emulator/coupled.py` (`coupled-run`: `build`, `scenario`,
`replay`, `tools/devtools/sharc_cells.py`, `coupled_ab.py`), `verify_build daemons` (writes them into `dist/daemons.json` and
adds the library key to `dist/bytes/daemon-test.key`; Vitest reads them with `coreLibEnv`) and
`Emu.spawn_coupled` of `tools/control/digi_client.py`. A variable already set in the environment wins and nothing is
built for that device. Build one library by hand (the device's generated directory in `SHARC_GEN_DIR`):

```text
SHARC_GEN_DIR=out/native/dn2-audio/gen CARGO_BUILD_JOBS=4 nice -n 10 \
  node tools/nx/rust-env.mjs --gen -- cargo build --release --locked -p sharc-core-lib
```

A library built without `SHARC_GEN_DIR` (or without `image.rs`) loads but holds no core: `core_for_device` refuses it with "built without a generated core" when the session opens.

The load is refused, with the first differing field named, unless the library's header equals the host's: header version,
SHA-256 of the `sharc-rt` sources (`sharc-rt/build.rs`), rustc version, target, the code generation flags and the size,
alignment and selected field offsets of `St`, `Mem`, `Insn`, `V`, `Cfg` and `GenCore` (`sharc-rt/src/header.rs`). The
flags hash leaves out `-Cprofile-use=`, `-Cprofile-generate=` and `-Cprofile-sample-use=`: PGO changes optimisation, not
layout or ABI, so a PGO host accepts a plain library and the other way round (see "Profile-guided libraries"). The core must belong to the firmware's
device (its image name, `dn2-1.12`, `dt2-1.17`, starts with the device id); a core made for another version of the same
device is accepted, and the engine interprets what its blocks do not cover. Example message after an edit of `sharc-rt` that
the host does not have: `DIGI_SHARC_CORE_LIB_DT2: /path/dt2.dylib: core library refused: sharc-rt source sha256: library
..., host ...`. The loader is `sharc_native::core_lib::load`; the library is never unloaded.

### Profile-guided libraries

The DSP hot loop (the generated blocks and the interpreter) runs in the library, so PGO covers it only when the library
is profile-guided. `uv run python -m tools.emulator.native_pgo train [--device dn2|dt2|both]` (`mise run perf:pgo`, or
`perf:pgo:dn2`, `perf:pgo:dt2`, `perf:pgo:both`) builds the host and the library of each device it runs with
`-Cprofile-generate`, runs the workload with `DIGI_SHARC_CORE_LIB_<DEV>` naming the instrumented libraries and merges the
host's and the libraries' `.profraw` files into one profile per training target. `mise run perf:pgo` trains `dn2`, `dt2` and then `both`
(the two-core profile of the desktop build; the libraries of the first two trainings are restored from the store, so
`both` costs a host build and its runs). Run `mise run workbench` and choose a firmware file. Workbench uses the DSP library for that device.
A current `both` profile covers both libraries. When it is unavailable, Workbench uses each device’s existing
training independently. A device without usable training gets a plain library. The desktop host can run plain
while the active firmware uses a profile-guided DSP library. Training in a linked worktree is found by key. A training whose profile for the current key exists is skipped (`train --force`, or
`mise run perf:pgo:dn2 -- --force`, trains again; a workload edit that keeps the workload name needs `--force`). The
instrumented libraries are built once per key (the `-Cprofile-generate` directory is fixed per target dir and not in the
key) and kept in `<git common dir>/sharc-core-pgo-instrument`, shared by all worktrees (the last 4 keys per device stay;
`DIGI_SHARC_PGO_INSTRUMENT_STORE` moves it, tests only); a training restores them. Each training prints `phase <name>: N s`
lines (`key`, `host-build`, `library-*`, `runs`, `merge`) and records them as `phase_seconds` in its manifest. A launch
with a fresh profile uses it for the host (`-Cprofile-use`) and for the library of every device the profile covers (`dn2`:
the DN2 library, `dt2`: the DT2 one, `both`: both): the library is a store entry whose key holds the profile content, so
it is built once per profile. The launchers (`native_emu.py`, `verify_build daemons`) pick it when the profile is fresh and
fall back to the plain library otherwise; `verify_build cores --pgo` builds or restores them by hand; an explicit
`DIGI_SHARC_CORE_LIB_<DEV>` wins. Because the header leaves the PGO flags out, any host works with any library.
`verify_build daemons` also writes `DIGI_SHARC_CORE_LIB_<DEV>_PLAIN` (when the plain entry is stored) so that the eviction
keeps both entries. Compare plain and PGO builds with `sharc_cells.py` / `coupled_ab.py` and `--env B:DIGI_SHARC_CORE_LIB_<DEV>=PATH`.

One process can run both devices: the other device's ready inputs go to `digi-emud` and the app as
`--also-coupled DSP_IMAGE DSP_STATE CF_SNAPSHOT SYX` (repeatable), and a firmware load for that device then boots coupled
(audio, DSP, restart) instead of ColdFire only. The workbench launcher passes them when both devices' inputs and generated
cores exist (`DIGI_EMU_SECOND_CORE=0` selects one device; daemon launches (`mise run emu:serve`) use one device unless
`DIGI_EMU_SECOND_CORE=1`). Proof: `uv run python -m tools.devtools.dual_core_check` (`DIGI_EMUD` names a `digi-emud`).

- The fast-tier lists are addresses in one device's DSP image, so a core reads the lists with the suffix of its device
  (`GenCore::env_suffix`, set by the host at load): DN2 `SHARC_FAST_REGIONS`, `SHARC_BLOCKTRANS` and
  `SHARC_FAST_FALLBACK_REGIONS`; DT2 the same names with `_2`. This holds for a host that runs one device and for one that
  runs both. `digi_env.resolve_tiers` and `fast_env` (Python) and `coupledEnv(syx, env, device)` (Vitest) apply it. Builds
  that link `sharc-gen` (`sharc-rig`, the browser core, the engine's tests and examples) read the unsuffixed names.
- The patched images (runtime, core/dynfx, core/fxpages, core/fx) differ from the image the generated blocks came from. Each
  engine checks its own loaded code against its own generated core (`Engine::verify_code`, `code_diff.rs`) and bars only
  the blocks with a changed instruction (`SHARC_CODE_LOG=1` prints the check; the audio report's `dsp_tiers` has
  `code_mismatch`).
- Process-wide state is keyed per core in `sharc-rt`: the instruction table (`insn_table`) is parsed once per generated
  blob (one slot each). Any new process-wide cache in `sharc-rt` needs the same key.
- `dual_core_check` modes (a daemon each, bounded): the default (open the other device and back), `--module digikit/pluck
  --play` (`dk run` and a played note), `--menu digikit/pluck --menu digikit/freqshift` (the on-device path SETTINGS > DIGIKIT >
  MODULES > tick > APPLY & RESTART, a note, the effect's FX page; screenshots to `out/runs/lanes/dualcore-shots/` or
  `--shots DIR`) and `--bench dt2 --binary LABEL=PATH ... --pairs 3` (DSP CPU per audio second, AOT share and real-time factor
  of a device with the runtime patched in, alternating the `digi-emud` builds given).
- Compare two builds with `sharc_cells` ([testing](../../docs/TESTING.md)); `--env B:DIGI_SHARC_CORE_LIB_DN2=PATH` runs one
  label on another library.

## Headless coupled runner (`coupled-run`) and the matrix

`packages/emulator/coupled` (crate `digi-coupled`) holds the coupled ColdFire + SHARC+ run
loop that the desktop app used to own: the DSP worker thread behind the DSPI2
link, the 250k-instruction step (`STEP_CHUNK`), PCM handling and link timing.
The app (`apps/desktop/src-tauri`) is a thin shell over it, and
`coupled-run` drives the same `CoupledRuntime::step_chunk` loop with no Tauri, no
webview and no audio device, so the numbers it reports are the app's.

The fixture gates (`coupled_ready_exactness` and the Trig 1 tests) live in
`digi_coupled::gates`; the desktop crate wraps them as ignored tests under their
old names (`desktop_runtime::coupled::tests::...`), with the same asserted hashes.

```
export SHARC_GEN_DIR=$PWD/out/native/dn2-audio/gen
T=$(uv run python -m tools.emulator.native_target dir)
cargo build --release --locked --target-dir "$T" -p digi-coupled --features sharc,play --bin coupled-run
SHARC_FAST_REGIONS=0x1c399a,0x1c3862,0x1c364f,0x1c5765,0xb823db,0x1c1884,0x1cdcef,0x1c3030,0xb85347,0x1c1da8,0xb80509 "$T"/release/coupled-run \
  --syx firmware/Digitone_II_OS1.11.syx \
  --dsp-image out/snapshots/dn2-audio-ready-cold-2026-10-07/digi-audio-dn2-image.bin \
  --dsp-state out/snapshots/dn2-audio-ready-cold-2026-10-07/digi-audio-m5.snap.dsp \
  --cf-snapshot out/snapshots/dn2-audio-ready-cold-2026-10-07/digi-audio-m5.snap \
  --scenario idle --scenario trig1-hold --out out/runs/x/report.json
```

The launcher also exports `SHARC_BLOCKTRANS` for DN2 OS 1.11: 12 block-translator
cluster roots (`0xb8276c,0x1c369b,...`, `BLOCKTRANS_ROOTS` in `tools/emulator/native_emu.py`,
keyed by the .syx SHA-256 like `FAST_REGIONS`). Measured A/B on DSP CPU per audio
second without PGO: 0.958 idle, 0.963 chord16, PCM identical. A user-set
`SHARC_BLOCKTRANS` wins, `DIGI_EMU_FAST=0` turns it off with the fast regions, and
other firmware gets neither. The PGO key (`tools/emulator/native_pgo.py`) includes the roots.
Without the launcher, add `SHARC_BLOCKTRANS=0xb8276c,0x1c369b,0xb809f2,0x1c5796,0xb84a8c,0xb8212b,0xb827b7,0x1c5110,0xb82568,0xb813cf,0xb8040f,0xb826a5`
to the command above.

`--list` shows the scenarios, `--seconds S` overrides the length, `--no-pcm-tap`
skips the PCM hash and statistics and `--no-link-timing` the per-period and link
timers (to A/B their cost). Exit code 2 means a gate
failed (missing blocks, halt, step error, non-finite PCM, wrong expected hash).

### Scenarios (`tests/scenarios/dn2.json`)

A scenario is a length (`seconds` of audio, or `instructions` of ColdFire) and a
list of panel events. Times count from the restored ready point: `at_instr` in
ColdFire instructions, `at_s` in audio seconds (DSPI2 frames sent / 1500). Events
fire at chunk boundaries before the next chunk, so a run is a function of its
inputs. `qa_script` also plays the shared QA sequence of
`packages/emulator/boot/examples/common` (NO at ready and +20M, encoder A at +40M).

| scenario | what it does |
|---|---|
| `idle` | 10 s of audio with no input. |
| `trig1-hold` | QA prelude, then Audition Trig 1 (button 25) pressed at +60M and held to the end. One voice. |
| `kb-chord16` | QA prelude, keyboard mode on (button 18 tapped at +50M), then all 16 trig keys (25..40) pressed at +60M and held: a chromatic chord of several voices. |
| `exactness-100m` | Validation: the `coupled_ready_exactness` workload (QA, Trig 1 down at +60M, up at +100M, 100M instructions). Its PCM hash must be `b3f5230f...4d03f` (cold-start reference state, 2026-10-07; was `3b600f6d...7f267` on the old state and `38d2a322...3ff1f8` before the Type 17 SIMD fix). |
| `trig1-noqa-40m` | Validation: `desktop_trig1_capture_without_qa_script` (Trig 1 from ready, 40M instructions, no prelude; it sounds, rms 0.032). Hash `2861ce03...c6f21` (cold-start reference state; `a50586e8...b75a` on the pre-2026-10-06 state). |

The QA prelude is no longer needed: on the current ready state Trig 1 sounds
with no prelude (the second validation scenario). It was needed only with the
2026-10-03 snapshot, whose card opened an SLC dialog that swallowed the key. `kb-chord16` is the heavy best effort. It is
heavier in audio (distinct pitches 277, 311, 370, 415, 466 Hz ... against one
262 Hz tone; PCM RMS 1.3-1.5x `trig1-hold`) but only slightly heavier in DSP
CPU: the DSP cost is dominated by a constant background (the idle-skipped RTOS
loop, the fast-tier region and the generated blocks that run with no voice at
all), so a voice adds a few percent. Pressing the 8 trig keys without keyboard
mode retriggers one pitch and is not heavier. Recording a pattern by button
script and playing it was tried and does not sound reliably (the first notes are
accepted, later button events are not), so it is not a scenario.

### Report

`report.json` has one object per scenario run: `audio_seconds`, `wall_seconds`,
`real_time_factor`; `cpu` (exact thread CPU time of the DSP worker and of the
ColdFire step thread from `CLOCK_THREAD_CPUTIME_ID`, total and per audio second);
`link_timing` (the `DN2_PROFILE_LINK` counters, as deltas over the run);
`dsp_instructions` (idle-skip / interpreter / AOT block / fast tier counts and
percentages, plus fast-tier builds and compile time); `cf_instructions`
(interpreted / idle fast-forward / ram-clear); `dsp_period_wall` (per-period DSP
wall time against the 0.667 ms deadline: count over, mean, p50/p90/p99/max);
`pcm` (SHA-256, RMS, peak, 100 ms window RMS series); `gates`.

Measurement overhead is a handful of timer reads per 0.667 ms period (the link
timing the app already has behind `DN2_PROFILE_LINK`, plus one per-period wall
time) and one hash/sum pass over the PCM on the ColdFire thread. Nothing runs
per instruction. The engine's tier counters and the worker's CPU time are read
once at the start and once at the end of a run. Measured A/B on `prod` (6 s
`trig1-hold`, three pairs, exclusive): the PCM tap changes ColdFire CPU per audio
second by under 0.01 (0.64 against 0.64); the link timers cost 0-3% of DSP CPU
(1.98 against 1.94 on average), inside the 1.84-2.07 run-to-run spread.

### Matrix (`tools/emulator/coupled_matrix.py`)

```
uv run python -m tools.emulator.coupled_matrix                       # full default matrix
uv run python -m tools.emulator.coupled_matrix --configs prod --scenarios idle --repeats 1
uv run python -m tools.emulator.coupled_matrix --pgo out/pgo/x.profdata
```

It builds `coupled-run` once per configuration (the cargo target dir from
`tools/emulator/native_target.py`, under a shared benchlock), runs every scenario under the
exclusive benchlock, one process per run, repeats outermost so drift spreads over
all configurations, and writes `out/native/runs/<timestamp>/{report.json,
summary.md,raw/}`. Configurations: `prod` (`out/native/dn2-audio/gen`,
`SHARC_FAST_REGIONS` from the launcher's table, 10 s, 3 repeats), `generic`
(`gen-generic`, interpreter only, 1.5 s, 1 repeat) and `generic-auto` (generic plus
`SHARC_FAST_AUTO=256,64`, 1.5 s, 1 repeat). The interpreter-only configurations run at
about 0.02-0.04 of real time, so their defaults are short; the full default
matrix takes about 10.5 minutes (621 s measured). The validation scenarios run once per
configuration; all configurations must produce the same hash. `--pgo PROFDATA`
builds every configuration with that profile (`pgo-use` subdir, `--target`).
Each run records the 1-minute load average; the summary flags runs that started
under load, because benchlock only excludes work that also takes it.

### Browser core tiers

The coupled browser core (wasm32) has a fast tier too (feature `fast-wasm`): its
kernels are wasm modules the page compiles (`packages/emulator/sharc-fast-wasm`, `packages/workbench-core/src/core-host.ts`).
`SHARC_FAST_REGIONS` and `SHARC_BLOCKTRANS` are environment variables, which
wasm32-unknown-unknown does not have, so the page passes the regions in
(`digi_sharc_fast_regions`). With no `?fast=` query it uses the `desktop` regions
of `config/emulator.json` for a listed firmware (AOT blocks present), or
run-time selection `4,2,1` on the generic core. `node tools/devtools/wasm_parity.mjs --scenario NAME` (or
`mise run web:parity NAME`) checks that the browser PCM is byte-identical to
the native `expect_sha256`.

The coupled core (`emulator-core-sharc.wasm`) contains firmware-derived DSP code.
`tools/nx/wasm.mjs` writes it to `apps/web/.local/`, which is gitignored
and served only by the dev server. It is never in `dist/`: a build guard fails
the build if it appears in the output. The CF-only core stays in
`apps/web/public/`.
