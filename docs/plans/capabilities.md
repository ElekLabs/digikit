# Capabilities: minimal and on demand (plan, 2026-10-31)

Goal: the runtime stays a small shim that loads modules. A module asks for **capabilities** (device-neutral features of the module contract, as
`DK_UI_CAP_*` already are for the UI); an **adapter** (`modules/core/adapters`) is the device- and firmware-specific code that implements one or
more of them. A capability costs nothing when no loaded module asks for it. Today adapters are static: the device file's `[patch_sets]` puts the
same eight (`dynreg`, `dyndsp`, `dynfx`, `fxpages`, `ui`, `digikit-menu`, `health`, `input`) into every image, used or not.
Limits: [finding 100](../findings/100-module-limits.md); the limits plan: [module limits](module-limits.md).

## Rule

Data before hooks. Implement a capability by extending what an adapter already passes (a field in a row, a word in the voice call) before adding a
firmware hook. A capability that needs a hook costs nothing when no loaded module asks for it: at most one flag check on a hot path, or the hook
is installed only when needed.

## 1. Capabilities from PR #209 (Tom's patched-in machines)

PR #209 built TB303, Para, ParaWT and eight overdrive flavours into a DT2 1.17 image with these core changes. They are unreviewed (**[D]**).
As capabilities, they let such machines load as `.dkm` modules.

| Capability (what a module asks for) | Implemented by (adapter) | Kind | DT2 | DN2 |
| --- | --- | --- | --- | --- |
| Value text per parameter (bipolar, names) | `params`: a code pointer in the copied parameter row (`+0x34`) | data | yes | yes |
| Track sample (address and length of the sample the stock engine loaded) | `dsp-voice`: a field of the voice call | data | yes | no (no stock sample engine; samples come from the sampler work) |
| Stereo voice | `dsp-voice`: a flag in the voice row | data | yes (the track's two voices) | not yet (voices are mono slots chosen per note) |
| All 8 knobs on the source page | a hook before the stock per-voice unpack (track level on knob H) | hook | yes | already true: no adapter |
| Chords on audio tracks (up to 4 notes per trig) | a hook on the ColdFire note writers | hook | yes | already true: no adapter |

Track flags (16 saved bits per track, PR #209) are not planned: a parameter does the same job.

- [ ] Capability declaration: a module lists the capabilities it needs in `module.toml`; the device reports the ones it has; the fit check
      (`dkrt-fit`) reports a missing one like any other shortfall ("needs stereo voice: not on this device").
- [ ] Value text per parameter (both devices).
- [ ] Track sample (DT2).
- [ ] Stereo voice (DT2).
- [ ] All 8 knobs on the source page (DT2), at most one flag check per voice when no module asks for it.
- [ ] Chords on audio tracks (DT2), same cost rule.
- [ ] First proof: TB303 as a `.dkm` on DT2 (needs plan module-limits step 4 for the knob count). Then Para and ParaWT.

These change the module contract, which the sampler branch also changes: start after it is merged.

## 2. Hooks installed on demand

- [ ] Find out whether the runtime can install a ColdFire hook at boot (the OS runs from RAM; instruction cache) and a SHARC hook (L2/L1 code,
      after the boot stream) **[O]**.
- [ ] If yes: the runtime installs only the hooks of the capabilities the selected modules ask for.

## 3. Runtime review (later)

- [ ] Measure what each of the eight static adapters costs when no module uses it (image bytes, RAM, cycles per block).
- [ ] Move hot-path hooks to the on-demand install, and drop what nothing needs.
