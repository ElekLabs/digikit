# Two-core PGO profile for the desktop build

**Status:** plan, not started (2026-10-31). Estimated 1 to 1.5 days of agent time, plus
about 1 hour of quiet-machine time for builds and training. Outcome risk: medium (see
Risks). Not contained enough to do as a side change.

## Problem

The workbench builds both devices' generated cores into one binary (feature `second-core`,
`SHARC_GEN_DIR` and `SHARC_GEN_DIR_2`). `pgo_plan` in `tools/emulator/native_emu.py` returns no
profile plan when `SHARC_GEN_DIR_2` is set, so the desktop build runs without PGO. Single-core
profiles give 19% (DT2) and 3% (DN2) on the DSP thread; finding 68 measured a plain two-core
build against non-PGO single-core binaries only.

## Design

1. Profile identity (`tools/emulator/native_pgo.py`)
   - New profile state `out/native/pgo/both/<triple>/` (device name `both`).
   - `compute_components` adds `generated2` (tree digest of `SHARC_GEN_DIR_2`) and the
     feature `second-core`; the workload name is `both-v1`.
   - `GUARDED_CRATES` gets `sharc_native_2` and `sharc_gen_2`, so `crate_mismatches` checks the
     second engine instance. `CRATE_NAME_RE` already builds its pattern from the list.
   - `check_state`, `pgo_state`, `plan_for` and `pgo_binary` take the second core into account;
     the target dir comes from `native_target.target_dir(gen, variant, second)`, which already
     keys on both cores.
2. Training binary: an instrumented `coupled-run` built with `--features second-core` (the
   coupled crate supports a second core), not the desktop app: the desktop app has no DT2
   workload, and a profile only applies when the engine crate hashes match. `DT2_APP_UNPROFILED`
   shows that the engine crates of `coupled-run` and the app already match; the check in step 5
   proves it for the two-core pair.
3. Workload (`DT2_WORKLOAD` style list, one process per item, firmware picks the core):
   - DN2: the existing `coupled_ready_exactness` scenario set (stock) and one DN2 runtime replay.
   - DT2: `dt2-sample-trig` and the FX scenarios of `DT2_WORKLOAD`, plus one DT2 runtime replay
     with sound (patched image, the dsp-voice and converter patches, one voice held).
   - Runtime replays need a ready dir with a patched snapshot pair and the matching patched
     `.syx`. `tools/devtools/sharc_matrix.py` already prepares and records such cells; reuse its
     replay fixture preparation. The fixture must be regenerated when the patch set changes (the
     snapshot is tied to the patched image hash), so the training step takes a fixture path and
     records its hashes in the manifest.
   - Set the fast-tier defaults of each device through `digi_env.fast_env` for the stock
     firmware, as the workbench does, so training sees the same tiers as use.
4. `native_emu.pgo_plan`: remove the early return; for a two-core launch use device `both`.
   Missing or stale profile keeps the current fallback to the plain two-core build and its
   notice. `perf:pgo` gains the third training (`--device both`).
5. Verify: `native_pgo check --device both`; after the profile-use build run
   `verify_binary` (crate hashes) on `digiemu-desktop`. A mismatch on `sharc_native_2` means the
   profile did nothing for core 2.

## Cost

| Step | Cost |
| --- | --- |
| Code and unit tests (steps 1, 2, 4; manifests, key, notices) | 3 to 4 h |
| Workload list and replay fixture hook (step 3) | 2 to 3 h |
| Cold two-core instrumented build, plain lane target | 25 to 40 min once, 5 to 15 min later |
| Training (3 runs of both devices) | about 15 min, quiet machine |
| Profile-use desktop build and crate check | 10 to 15 min |

## Risks

- A lane's target dir is cold: the first two-core build compiles both generated crates
  (about 25 minutes, 15 cores). Build in the main tree's warm key dir only with Em's go-ahead.
- The patched-runtime replay changes with each patch change; the profile then trains on a
  stale distribution. Profile data of changed functions is skipped (partial match), so the harm
  is a lower gain, not wrong code.
- Instrumented counters make runs about 2 to 3 times slower; the training scenarios use short
  bounded `--seconds`.

## Commands for the timing step (run only when Em allows timing)

Timing of the DT2 root change (commit `45c4811f`), same frozen binaries, only the environment
differs (`docs/TESTING.md` "Resumable SHARC matrix"): in the matrix configuration set

```json
"variants": {
  "A": {"SHARC_FAST_FALLBACK_REGIONS": "0x1c2607"},
  "B": {"SHARC_FAST_REGIONS": "0x1c4ed2", "SHARC_FAST_FALLBACK_REGIONS": "0x1c2607,0x1c1fe7,0x1c204b"}
}
```

with cells `dt2-stock`, a DT2 runtime idle replay and a DT2 runtime sample replay, then:

```sh
mise run perf:matrix -- run PERF/config.json --baseline PERF/frozen --candidate PERF/frozen --out PERF/results
```

Training for the existing single-device profiles, to compare against the two-core build later:

```sh
mise run perf:pgo:dn2
mise run perf:pgo:dt2
```

Two-core training (after the implementation above):

```sh
uv run python -m tools.emulator.native_pgo train --device both
uv run python -m tools.emulator.native_pgo check --device both
uv run python -m tools.emulator.native_emu --build-only
```
