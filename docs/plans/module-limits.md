# Module limits (plan, 2026-10-31)

Goal: modules are limited only by what the firmware allows; each limit has one source; every usage number we show is measured correctly.
Facts: [finding 100](../findings/100-module-limits.md). Branch: limits/single-source. It lands before the sampler branch; that branch then merges
`main`. Step 4 starts after the sampler branch is merged.

## 1. Findings

- [ ] Finding 100, stale passages replaced with pointers.

## 2. One source for every limit

Sources: `sdk/sdk.toml` (contract and format limits, protocol limits) and `devices/*.toml` `[limits]` (per device). Everything else is generated, imported or
read from CAPACITY.

- [ ] `sdk.toml [limits.max]`: add `machine_rows` and `stage_bytes`; sdkgen emits `PARAM_ROWS`, `PAGE_ROWS`, `PAGE_KNOBS` into `gen_sdk.rs` `limits`.
- [ ] Runtime `registry.rs` and patchkit `register.rs`: use the generated and `sdkgen` values, not literals.
- [ ] `dkneeds` holds the loader format limits (records, relocs, imports, state voices, granule, arena header, DSP slack, ColdFire imports); the runtime
      `dsp.rs`, `loader.rs`, `arena.rs` and patchkit `dkmdsp.rs` import them.
- [ ] `dkm.rs MAX_FILE` comes from `stage_bytes`.
- [ ] `sdk.toml [protocol.limits]` (chunk, read, peek, sector, wire sizes); the generated `gen_proto.*` files replace the copies in TypeScript, Rust and Python.
- [x] The fit rule's arena header and DSP slack come from `dkneeds`, not from literals in host copies: the host copies are gone, the rule is only `dkrt_fit` (called through `pk_fit` and `patchkit fit`).
- [ ] `pk_capacity.py` and `pk_conform.py` read the device files; the stock DSP busy counts move into `devices/cycle-factors.json`.
- [ ] Tests: the dyndsp voice rows against `machine_rows`; every CAPACITY kind is judged or listed as not judged. Fix the stale count comments in the dynreg
      and dyndsp manifests.
- [ ] DN2 `dsp_l2_excludes` gets `0x200fa000-0x200fe000` (same cache range as DT2).

## 3. Usage numbers that are measured correctly

- [ ] DSP load: label it as an estimate at 1 GHz on the device, and use one figure in the emulator (the HEALTH page and the Console show different numbers today).
- [ ] CPU %: show that it is relative to the quietest window since boot, or measure it absolutely.
- [ ] Heap LARGEST: walk the high orders first, or mark it when the walk stops at `HF_CUT`.
- [ ] Menu and Memory panel: show the largest free block next to the total free (DSP area and arena), as the fit check uses it.
- [ ] Fit check: model the loader's separate first-fit blocks, so a module that "fits" also loads; add a multi-fragment case to `sdk/vectors/fit-vectors.json`.
- [ ] Emulator load meter: report a peak per window, not only the smoothed average.
- [ ] Re-measure stock DSP busy on 1.17 / 1.12 and record the free cycles per block in finding 100.

## 4. Raise the limits that are only our numbers

Each with one proof run on DN2, then DT2.

- [ ] DN2 DSP area to 448 KiB (below `0x200fa000`).
- [ ] Parameters: DT2 `param_slots` `[1,2,4,5,6]`; DN2 `[0..7]`. Check that kit and project save and load keep the new knobs.
- [ ] ColdFire arena and staging: size from the measured runtime footprint inside the expansion region (DT2 1.33 MiB in all, DN2 much more).
- [ ] `param_rows`, `page_rows`, `machine_rows`: raise with the dynreg and dyndsp entries; size from RAM cost.

## 5. Contract changes (after the sampler branch is merged)

- [ ] DN2: several pages per machine and more than 8 parameters (per-machine slot maps; `DK_MAX_PARAMS`).
- [ ] A formatter per parameter in the parameter record (value text: bipolar, names).
- [ ] DT2 knob H as a parameter (fixed track level before the stock unpack; idea from PR #209).
- [ ] DT2 stereo output (the track's two voices).
- [ ] L1 placement for hot code.
- [ ] DT2 DSP area above 512 KiB (wider offset field or a second window).
- [ ] DDR for modules: through the sampler branch's work.
