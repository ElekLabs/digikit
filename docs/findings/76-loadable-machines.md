# 76. bd, analog and pd as run-time machines (DT2 1.17, DN2 1.12)

`fadedlimes/bd`, `fadedlimes/analog` and `fadedlimes/pd` were patched-in machines with fixed types (15, 13, 14). They are now run-time modules (`dynamic = true`,
both ports, DN2 "experimental") like `digikit/pluck` (finding 32). The DSP sources are unchanged: one manifest edit each. Marks: **[D]** measured here once, no second agent.

## Result

| Module | DT2 1.17 note against the patched-in machine | Peak at defaults, 24 000 samples from the onset (DT2 / DN2) |
| --- | --- | --- |
| bd | **[D]** exact, 96 000 samples, 0 differ | 0.044 / 0.024 |
| analog | **[D]** 0 differ after aligning the onset (the run-time onset is one block, 64 samples, later) | 0.037 / 0.019 |
| pd | **[D]** exact, 96 000 samples, 0 differ | 0.032 / 0.018 |

The comparison ran once, before the patched-in twins were removed: the old manifests copied to scratch modules `test/patched-NAME` (same DSP source), `equiv`
scenarios with `twin = ...`. Machine, parameter and page rows also equal the twin's. No write into `cf.expansion`, no flush hit, only the two selection sectors written.
DN2 has no twin: its note is pinned (see below).

## One thing the emulator needed: uninitialised voice state

`analog` and `pd` seed their state at the first call when a magic word in the state differs from the expected value. A run-time module's state is uninitialised L2 (the
runtime clears only `state_clear_bytes`, the module contract), so that read returns an unknown value in the emulator and the DSP halts at the conditional jump
("fork at ..."). The patched-in twin does the same on its state. `SHARC_UNINIT=<seed>` (finding 49) fills unwritten bytes with a non-zero pattern, which is what a device holds:
with it both modules start, and the pattern never equals the magic. The `equiv` kind got `env_run` and `env_ref` for this (`tools/patchkit/pk_digikit_scen.py`), and `play_module`
and `play_machine` to play a machine other than pluck. **[O]** On a device the state of a module that never ran holds whatever the SRAM had; a value equal to the
magic (probability about 2^-32) would skip the seed. A contract-clean fix would keep the phases outside the cleared bytes and drop the magic, which changes the sound (phases
free-run on purpose), so it was not done.

## Scenarios and pins

`tests/digikit/scenarios/equiv-dyn-{bd,analog,pd}.toml` (tier `quick`) load one module at boot, check the voice row and play a note. Their `pcm_sha256` pins (approved by Em, 2026-10-29) are the note 60 as in `equiv-dn2-set`, from the onset, 24 000 samples:

| Scenario | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| bd | `2cb0259aeb2626862b0b5fdbc4e3073ea5c3a33698dfaf20afcf730d027ebb0d` (onset 258) | `3da404b56faf9b7a38b4fc50470de7496ef8e32ce0b6832dd84a31562b2725f7` (onset 206) |
| analog | `e9fb4d69393815a3b512cf5b5db11284992f66199f493472fff3d3299aba78e6` (onset 320) | `9e0c24e964e7a480416b73228bd606a36eaef2bdfb15792607ae90a9cc7a8821` (onset 264) |
| pd | `0e0c6778ad7684ae018ecc87755aeb8bf5ec8b3237121f015f95713fdfe3dd33` (onset 258) | `1c5cc2f1712f861e989cd0f335cbfb6e6c094183145e13634b4f20bdd2a8a5bc` (onset 208) |

## What was removed

The permanent types 13, 14 and 15 of both devices in `modules/core/adapters/machines/types.toml` (now retired gap rows) and the patched-in references in the docs. `test/patched-pluck`
stays the patched-in fixture. The site and the workbench list the three modules in Transfer (they ship because `dynamic = true`).
