# Adapters: minimal and on demand (plan, 2026-10-31)

Goal: the runtime stays a small shim that loads modules. A firmware feature a module needs comes from an adapter (`modules/core/adapters`), and an
adapter costs nothing when no loaded module uses it. Today adapters are static: the device file's `[patch_sets]` puts the same eight
(`dynreg`, `dyndsp`, `dynfx`, `fxpages`, `ui`, `digikit-menu`, `health`, `input`) into every image, used or not. Limits: [finding 100](../findings/100-module-limits.md);
the limits plan: [module limits](module-limits.md).

## Rule

Data before hooks. Extend what an existing adapter already passes (a field in a row, a word in the voice call) before adding a firmware hook. A
hook adapter must cost nothing when no loaded module needs it: at most one flag check on a hot path, or installed only when needed.

## 1. Features from PR #209 (Tom's patched-in machines)

PR #209 built TB303, Para, ParaWT and eight overdrive flavours into a DT2 1.17 image with these core changes. They are unreviewed (**[D]**).
Ported as adapters, they let such machines load as `.dkm` modules.

| Feature | What a module gets | Kind | DT2 | DN2 |
| --- | --- | --- | --- | --- |
| Value formatter | its own value text per parameter (bipolar, names) | data: a code pointer in the copied parameter row (`+0x34`) | yes | yes |
| Sample pointer | address and length of the sample the stock engine loaded for the track | data: a field of the voice call | yes | no stock sample engine; samples come from the sampler work |
| Stereo voice | left and right output | data: a flag in the voice row, handled in `dsp-voice` | yes (the track's two voices) | needs another design (voices are mono slots chosen per note) |
| Knob H as a parameter | all 8 knobs on the SRC page | hook: before the stock per-voice unpack (track level) | yes | not needed (separate level field) |
| Polynotes | up to 4 notes per trig on an audio track | hook: the ColdFire note writers | yes | not needed (a note takes its own voice slot) |
| Track flags | 16 saved bits per track | hook | not planned: use a parameter | not planned |

- [ ] Value formatter (both devices).
- [ ] Sample pointer (DT2).
- [ ] Stereo voice (DT2).
- [ ] Knob H as a parameter (DT2), at most one flag check per voice when no module uses it.
- [ ] Polynotes (DT2), same cost rule.
- [ ] First proof: TB303 as a `.dkm` on DT2 (needs plan module-limits step 4 for the knob count). Then Para and ParaWT.

These change the module contract, which the sampler branch also changes: start after it is merged.

## 2. Hooks installed on demand

- [ ] Find out whether the runtime can install a ColdFire hook at boot (the OS runs from RAM; instruction cache) and a SHARC hook (L2/L1 code,
      after the boot stream) **[O]**.
- [ ] If yes: a module declares the hook features it needs; the runtime installs only those at boot.

## 3. Runtime review (later)

- [ ] Measure what each of the eight static adapters costs when no module uses it (image bytes, RAM, cycles per block).
- [ ] Move hot-path hooks to the on-demand install, and drop what nothing needs.
