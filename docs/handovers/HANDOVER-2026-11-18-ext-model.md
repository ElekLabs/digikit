# Handover 2026-11-18: extension model (lane ext-model)

Start from `docs/plans/extension-model.md` (section 6 is the status table), then this file. Priority (Em, 2026-11-18): hot reload for machines and
filters first, then digikit-project, then reload for other services.

## Where the work is

Lane `/Users/em/src/digi/lanes/ext-model`, stacked branches rebased onto origin/main `7c1e3e7fb` (main had not moved at handover). Nothing is
pushed or merged.

| Branch | Head | Content | State |
| --- | --- | --- | --- |
| `plan/extension-model` | d0ccb4164 | the plan, roadmap row E | reviewed |
| `e0b/findings` | 5edaf3232 | findings transport state, filter machines, LFO waveforms; 02, 06, 53, 135 corrected | second-agent checked |
| `e0a/filter-dsp` | 46e1bcb58 | E0a parked, E1 outcome, E2/E3 designs in the plan | docs |
| `e2/exports-codegen` | 1d35b254c | builder exports generated from sdk.toml | built, reviewed, host green |
| `e3/keys` | f8a3ae5ea | one key rule, keys_of, kind-free trait, role check, LFO keys, sticky Range | built, reviewed, host green |
| `e4/contract-suite` | e8f39ef44 | Service contract suite, fx/machines rows through FwMem, fx leak fix | built, reviewed (3 rounds), host green |
| `e6/lfo-shapes` | 4962a6072 + uncommitted rig work | LFO shapes: image stubs, lfo.dks hooks, names, panel range, graph, device tests | built, reviewed; device iteration in progress |

Backups: `<name>-pre-rebase`, `-rebase-1`, `-rebase-2`, `e6/lfo-shapes-backup-*`. `e6/lfo-shapes-backup-5` holds an old commit with committed
firmware tile bytes: never push it; Em removes it (`git -C /Users/em/src/digi/lanes/ext-model branch -D e6/lfo-shapes-backup-5`). A throwaway
worktree `/Users/em/src/digi/lanes/tmp-main-check` may still exist (remove with `git -C ~/src/digi/digitakt2 worktree remove --force` on it).

## Next, in order

1. **Land docs + E2** (agreed with Em): on `e2/exports-codegen`, renumber our findings 141, 142, 143 to **142, 143, 144** (main already has
   `141-syntakt-analog-and-nor-paths.md`): files `docs/findings/141-transport-state.md`, `142-filter-machines.md`, `143-lfo-waveforms.md`, the
   three rows in `docs/FINDINGS.md`, links in findings 06 (line ~1068), 135 (~56), 53 (~72) and the plan (lines ~106-107, ~665). Rename in
   descending order (143 to 144 first). Later branches (e3..e6) carry more references (lfo.toml, hooks-*.s comments, plan); fix them when each
   branch is rebased onto the merged E2. Then run only `gate-smoke` and `gate-pins` (E2's objects are byte-identical, objdiff), push, open the PR
   (base main), merge after green, and `git pull --ff-only` in `~/src/digi/digitakt2`.
2. **E6 device iteration in the main context, not through agents** (lesson below). The rig bug found today: on DN2 turning LFO DEST opens a
   destination list ("MOD1 DEST", Confirm Yes/No); nothing is stored until YES. The uncommitted rig rework (`lfo-rig.ts` chooseDest: scan the list,
   tap YES) handles it; device facts `lfo_live`, `lfo_phase` were added to `devices/digitakt-ii.toml` and `digitone-ii.toml` and typed in
   `tests/regression/runtime/device-data.ts`. A run of `core/runtime-lfo-tests:test-device--lfo-shape-output--dn2` was in flight at handover; its
   log is `out/runs/e6-phase-a/out2.log` (phase A logs: `run.log`). Also fix `lfo.device.test.ts` case "kind table that init does not declare":
   it now gets E_PLAN_KIND (kind 8 unowned) because the bound variant's KINDS lists only 91; patch the table to kind 8 plus an undeclared 91 so
   the planner accepts the fixture and the provider's E_DECL_KIND check fires (the uncommitted edits may already do this: check the diff). Goldens:
   run with `UPDATE_GOLDENS=1`, show Em the PNGs, accept only after Em looks. `dk:dev` for screenshots:
   `mise run dk:dev -- emulator --device dn2 --keys 'tap:MOD,wait:400,shot:mod,turn:ENC D:8,shot:dest'` (frames in `out/runs/devreload/dn2/`).
3. **Land E3 + E4** after rebasing onto merged E2: device halves on DT2 and DN2: `core/runtime-refusal-tests:test-device`,
   `core/runtime-capacity-tests:test-device--commit`, `core/runtime-machines-tests:--machine-rows-dt2/--machine-rows-dn2`,
   `core/fxpages:--fx-pages`, `core/fxpages:--fx-track-route`, `core/dynreg:--roundtrip`, plus `gate-smoke`, `gate-pins`.
4. **Land E6** with the four `core/runtime-lfo-tests` files on both devices.
5. **E5 machine reload** (R12-8 merged on main; step 7 still open). Design: `out/runs/ext-model-design/e5-archive-map.md` and the plan's E5
   (smaller scope, 2-3 days; E5b deferred items). Iterate against the emulator in the main context from day one.
6. Then E0a rerun (daemon with the generated core: on main hosts load the core from the store, `verify_build cores`), E6c (design
   `out/runs/ext-model-design/e6c-design.md`: a tiny `ranges` service; first check the 1 h spike whether core/params' RAM table copy makes the
   hook unnecessary), E7 filters (ColdFire design `out/runs/ext-model-design/e7-coldfire-design.md`, with reload built in), then digikit-project.

## Decisions taken today

- Reload scope: E5 small (machines first), E5b later; soak fixture = `test/sine` built twice with a pitch define.
- Filter choices persist through digikit-project (`.dkp` project scope); session-only until then. LFO extended waves stay in the sound for now;
  move to `.dkp` with digikit-project.
- Proof adapters stay; they move in E8 (dsp-stereo-proof into dsp-voice tests, others to `modules/core/proofs/`).
- Refactors (E6e: shared Bitmap crate, unified per-device LFO routines from devices/fw facts) and SDK custom graphics (E6d) come after E6's device
  check. E6c is needed before E7.
- Device proofs are narrow: named tests that observe each change, not area tags.

## Open for Em

- **O8**: with a shape loaded, MIDI CC >= 7 (NRPN >= 0x380) on WAVE reaches shape numbers instead of RAND. Recommended: accept as a feature.
- The patchkit test `devdata::a_device_without_a_dsp_patches_the_runtime_and_what_it_stands_on_alone` fails on main (Syntakt emulator set):
  fix in a separate small PR or leave to the Syntakt owner. `patch:port` fails on Syntakt 1.42 (`cf.usb.site.init`, usb.toml site without a
  `devices` filter) on main too.

## Lessons

- Device tests written and reviewed without running them cost several rounds; the first real run failed on a basic rig assumption. Do a short
  `dk:dev` look at the page before writing a device test, and do device iteration in the main context.
- A profiling daemon needs the generated core; check `counted` is non-zero before an experiment.
- Never commit firmware-derived bytes, even a 28-word extract; read them from the image at test time and skip when absent.
- Check machine load before heavy steps; other sessions' gates can hold it at 25-60 for hours.
