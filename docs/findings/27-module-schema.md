# 27. Module schema: typed machine modules, device knob models, generated constants

Plan: `docs/plans/digikit-runtime.md`, Phase 3 ("Module interface, schema and Rust SDK"; this is the schema part, not the
runtime interface). Code: `packages/patchkit/src/{typed,modgen}.rs`, `devices/*.toml` `[modules]`, `modules/README.md`
("Typed sections"). Marks as in AGENTS.md; nothing here has had a second check, so **[D]**.

## What was decided and built

- Modules are authored in typed TOML (Em, 2026-10-16): `[[machine]]`, `[[param]]`, `[[page]]`, `[[voice]]`. They are
  lowered per device into the `[[entry]]` rows that `core/machines`, `core/params` and `core/dsp-voice` already take,
  before patchkit's apply. The frameworks, anchors and the registry code are unchanged.
- The device decides what a knob is (plan decision 8): `[modules.knobs]` and `[modules.frame]` in `devices/<device>.toml`.
  Digitakt II: knobs are descriptor fields `f0..f7` of the machine row (engine slots 0 to 7), level `f8`. Digitone II: knobs
  are the columns `c0..c7` of a page row, sound-array slots 25 to 32, level `c8`, `own_dsp` allowed. The code names no
  device; a device without `[modules]` takes no typed sections.
- The lowering is Rust in patchkit and runs in `patchkit.wasm`. It does not use the `device-profile` crate: that crate
  parses the card and firmware identity with `deny_unknown_fields` types and pulls in more than a wasm patch tool needs.
  patchkit reads the same TOML text itself (`typed::Models`), and the bundle carries the device files
  (`bundle.devices`, like `ports`), so the browser and the command line lower from the same source.
- `patchkit gen` writes `build/gen/{consts.rs,<device>.inc,gallery.json}` per module (committed; `gen --check` and
  `packages/patchkit/tests/typed.rs` catch a stale file). `pk_build` expands `#include "x.inc"` from `build/gen/`.

## Oracle **[D]**

`digikit/pluck` and `test/xslice` were re-expressed in the typed form. Patched images, hand-written rows (8606b7f) against typed
form, SHA-256 and plan.json equal on every port the modules support:

| Image | Modules | SHA-256 of the patched image |
| --- | --- | --- |
| DT2 1.16 | pluck | `bb551424be229e7eb654687b956d26a75d65420f211ed9a907d235c73c0fefef` |
| DT2 1.16 | xslice | `5c0268833c64ac10abb6227ee268be97fdca9712dc6dad5269eb3c711694e0bc` |
| DT2 1.16 | pluck + xslice | `0788007e39a6ca1cd23e8e305c95205eeba15d07bd71c83419f5781a7fa8ead8` |
| DT2 1.17 | pluck | `4b29dfd6a3c76da34efdb5cdbae3c619b9ac33a6ee040b08dea23634574390c1` |
| DT2 1.17 | xslice | `c0948bb2ece9df0d6bf15fbb7eb97bfa5635cad3346803688ac05c48a1775a07` |
| DT2 1.17 | pluck + xslice | `5885b5f0395c6741b992b5f85536f11f7644eeba47b900562bb1741fb84ab4d2` |
| DN2 1.12 | pluck | `ce4b082dfce3385bbfa0a6a4b43645d9803d964bd5b51b38409325ff4153ffc6` |

DN2 1.11 and DN2 1.12 + xslice are refused as before (pluck lists no 1.11 port; xslice now says `ports = ["dt2-1.16", "dt2-1.17"]`,
where it used to fail on a missing type number). `patchkit.wasm` and the command line give byte-identical images for pluck (apps/web
`patchkit.wasm.test.ts`).

## The generated frame numbers match the DSP sources **[D]**

pluck's DSP sources hard-code the frame layout: Digitakt II `0x6d + 48 t`, knob offsets 1, 2, 6, 7 (pluck.s header; core/params builds the same mirror index: the
machine slots skip two halfwords after every four); Digitone II `0x6d + 0x49 v`, offsets 0 to 3. The device models give
exactly these: `frame_offset(j) = j + (j / group) * gap` with group 4, gap 2 (DT2) and none (DN2). Replacing the literals
in a copy of `pluck.s` and `pluck_dn2.s` by the generated names assembles to the same code, data, symbols and relocations
as the committed objects (`tests/patchkit/test_pk_gen_include.py`). The committed sources keep their literals so that the
objects (which hold the source hash) do not change.

The assembler upper-cases macro text, so a `0x` literal after `+` fails (`R0 = R13 + 0X1`); the `.inc` files use decimal.

## Open **[O]**

- The runtime registry interface (a loaded module registering what it adds at run time) is not designed here; the typed
  sections are the source it can be generated from. Machine rows are still built at patch time.
- A stock parameter code as a knob of a Digitone II page is refused: how the slot-to-code mapping treats a stock code is
  not known (no module needs it yet).
- Voice entries carry only the symbol; the DSP code size and the frame-tail placement (S4b) are not part of the schema.
- `core/params` still holds the mirror-index formula in its section rows; `[modules.frame] mirror_base/group/gap` repeats
  it for the generators. `packages/patchkit/tests/typed.rs` checks the numbers pluck uses, not the formula itself.
