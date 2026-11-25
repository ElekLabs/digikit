# One image with dnfw's mods and digikit's runtime: where both write (DN2 1.11), 2026-10-28

**[D]** Measured once, statically, on Digitone II 1.11. Nothing here ran on a device.

dnfw (angellinares/dn2_firmware_explore) patches the same firmware with its own mods. Its mods declare every range they write
(section 3 offsets, RAM addresses, the DSP boot stream). patchkit's plan (`patchkit patch ... --plan`) lists every write, hook
site and region in use. `dnfw mods overlap` (dn2_firmware_explore PR #303) compares the two. This finding is its result for one
mod set against the runtime's set.

- digikit: `core/runtime`, `core/digikit-menu`, `core/health`, `core/rows`, `core/dyndsp` (with their requirements), from main
  `359a229a`. This is the image `digikit-health1` (sha256 `968463e487ab581d`).
- dnfw: waverider, layermidi, arpmodes, lfowaves, lfo4 and bootscreen, the set one user runs.

## Result

| Kind | Count |
| --- | --- |
| Both write the same bytes | 361 |
| A patchkit hook site that a dnfw mod also changes | 8 |
| dnfw RAM inside the used part of `cf.expansion` | 10 |
| The DSP boot stream | 1 |

dnfw's `usbprobe` alone has none.

## The two machine frameworks patch the same sites

**[D]** Most of the collisions are one fact. dnfw's platform has a hook bus that raises stock's bounds and counts for new
parameters and machine types. `core/rows` and `core/rows` raise the same ones.

- 255 parameter-record bounds: dnfw's "the parameter records count" against `cf.params.bound.*` (for example `0x400366ac`,
  `cf.params.bound.2+2`). lfo4's parameter edits meet `cf.params.ref.*` 58 times.
- The machine-type tables: `cf.machines.permit`, `cf.machines.name.0..2` and `cf.machines.persist` (for example `0x400dc19d`).
- The machine hooks: Waverider changes the bytes at `cf.machines.dispatch` (`0x400c248e`), `cf.machines.typemap.0..2`,
  `cf.machines.reader` (`0x4004b816`), `cf.machines.owner` (`0x40036c24`), `cf.machines.group` (`0x4005927a`) and the MACHINE SEL list
  (`cf.machines.list`, `cf.machines.list_alloc`). lfo4 changes the bytes at `cf.params.t2` (`0x400c241c`).
- The start-up: dnfw's platform loader and `core/expansion` both take `cf.reset.init_calls` (`0x4000053e`).

So the two cannot share an image by keeping apart: the sites are the same because the job is the same. One framework must own
them, and the other must add its machines and parameters through it.

**Decision (2026-10-28, the dnfw owner and Em):** `core/rows` and `core/rows` own these sites. dnfw's mods move to digikit
modules and add their machines and parameters through them. A machine that does not use the SDK goes in as a `raw` module
(`docs/design/module-sdk.md` D2: no contract, only the generic load checks). Em's proposal on top: make the module's type and its
category two fields, so a module can be a raw machine, a machine that bypasses the SDK. **[O]** That split is not designed yet.
Until the port, dnfw keeps its own framework, so the two still do not go in one image.

## RAM

**[D]** dnfw's mods put their chunks at fixed addresses. Five chunks (ten collisions: each meets a placed object and the region) fall in the part of `cf.expansion` that patchkit fills first fit
(`0x46712000..0x467dd2b0` for this set): lfowaves at `0x46740000`, `0x46750000` and `0x46780000`, arpmodes at `0x467a0000`, layermidi
at `0x467d8000`. The port file's `excludes` could keep patchkit out of them, so these alone are not a blocker.

## The DSP boot stream

**[D]** Waverider rebuilds section 7, and patchkit writes 8 DSP ranges, the first the hook `dl.dsp.block_tail`. dnfw declares the
stream as one range today, so the tool cannot tell whether the ranges meet. Waverider would have to declare its blocks one by one.

## Open

- **[O]** The type and category split for raw machines (above).
- **[O]** The same comparison on DN2 1.12, and with `core/fx` and `core/fxpages`.
