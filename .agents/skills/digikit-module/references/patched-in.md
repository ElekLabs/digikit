# Patched-in modules

A module without `dynamic = true` is linked into a stock firmware image by patchkit (`packages/patchkit`). Use this route for hooks in
firmware code (master-bus effects, ColdFire UI changes). The project's PLUCK machine (`digikit/pluck`) is a run-time module; its patched-in twin `test/patched-pluck` is a test fixture.
The browser patch page lists patched-in modules only; a `.dkm` goes to the device through `/upload`.

- **Look up declared anchors** in `modules/core/fw/reference.json` or with `mise run fw:reference`.
- **Anchors, not addresses.** Every firmware site is a named anchor resolved by a rule declared in `devices/fw/` (masked signature, `cfmatch`
  offset, or SHARC `reloc_hash` plus offset) into `devices/ports/<dev>-<ver>.toml` by `mise run patch:port`. A rule that matches nothing or
  twice is an error: fix the rule, never type an address.
- **Claims.** `[[claim]] resource = "..."` declares a scarce resource (for example `dsp.fx.hook`); two modules claiming one conflict.
- **Ops.** `write` (`expect_sha256` of the bytes replaced, never the bytes),
  `hook` (names a hook declared in `devices/fw/`; the declaration gives site, mode and registers). `[[section]]` copies a stock table from the user's image at patch time.
- Never patch sections 2, 4 or 8 (patchkit refuses). Always patch from the stock image.

| step | command |
| --- | --- |
| new | Copy an existing patched-in module (`tests/fixtures/modules/patched-pluck`, `modules/core/adapters/fx`). `module:new` makes run-time content modules only. |
| toolchains | `mise run patch:toolchains` once |
| patch | `mise run patch -- --firmware firmware/Digitakt_II_OS1.17.syx --modules fx/NAME -o out/patch/x.syx` |
| explain | `mise run patch:explain -- --firmware ... --modules ...` |
| inject | `mise run patch:inject -- out/patch/x.syx` into a running emulator |
| edit loop | `mise run module:dev -- --modules fx/NAME [--device dt2\|dn2]` |
| test | `mise run patch:test -- --cell "dt2-1.17:fx/NAME"` (a new module set needs `-- --update` once for frame goldens) |

A patched-in machine has a permanent type number in `modules/core/adapters/machines/types.toml`. Documents: `docs/PATCHING.md`, `modules/README.md`,
`docs/findings/21-patchkit-core.md`.
