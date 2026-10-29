# Firmware declarations

Status: agreed with Em 2026-10-27. Plan: [firmware-decls-and-sdk.md](../plans/firmware-decls-and-sdk.md), step 3.
Inputs: the 201 `[[anchor_rule]]` entries in 15 `module.toml` files, the anchor catalogue
(`tools/coldfire/atlas.py catalog`, `tools/sharc_dev/sharc_catalog.py`) and
[finding 71](../findings/71-ui-components-1-17.md).

## Problem

Today one firmware fact is written in up to four places: a rule in a module manifest, a name mapping in
`[object.externs]`, an `extern` in the module source with the signature in a comment, and struct offsets as bare
numbers in code. Rules for the same function repeat across modules (`dsp.master.*` in two effects). Nothing checks a
signature or an offset. Anchor names follow the module that first used them (`dkrt.*`, `health.*`, `input.*`), not the
firmware.

## Decision summary

1. Firmware facts live in one place: `devices/fw/<subsystem>.toml`, one file per firmware subsystem. Module manifests
   no longer hold `[[anchor_rule]]`.
2. Each entry is one declaration: a function, a data object, a value, a site list, a hook site or a struct. It has a
   name, the devices it exists on, a rule per device (or one shared rule), a type, a mark and a short description (`doc`, required).
   `users` lists the modules that use it (provenance only; the linker does not read it).
3. Names follow `<cpu>.<subsystem>.<kind>.<name>` with a fixed subsystem list. A renamed declaration lists its former
   names in `was`: they are for reading older docs and findings, never resolve, and must not be a current name.
4. `port_resolve.py` reads `devices/fw/` and writes `devices/ports/<dev>-<os>.toml` as today. Rule kinds and matching do
   not change in this step.
5. Step 5 generates bindings (Rust, C, assembly) from the same files. Content modules never see these declarations;
   only `core/*` adapters do.

## File layout

```text
devices/fw/
  subsystems.toml     the fixed subsystem list, with one line of meaning each
  view.toml           ViewController, View, push, close, invalidate
  draw.toml           text, fill, blit, scroll bar, screen bitmap
  menu.toml           Menu, MenuItem, VerticalMenuView, item builder
  window.toml         ConfirmWindow, MultiChoiceWindow, PopupWindow, ...
  string.toml         COW std::string, std::function helpers, shared_ptr helpers
  rtos.toml           reset path, tasks, mutexes, operator new
  params.toml         parameter tables, references, bounds
  machines.toml       machine dispatch, name tables, per-type sites
  midi.toml, fs.toml, project.toml, ...
  dsp/voice.toml      SHARC: frame, voice render
  dsp/fx.toml         SHARC: send, return, buses
  dsp/master.toml     SHARC: master buffer, q31 converter
```

A subsystem file holds entries for every device. A per-device difference sits inside the entry, never in a separate
file per device.

## Entry kinds

| Kind | Names | Today's rule kinds | Port anchor kind |
|---|---|---|---|
| `fn` | `cf.view.fn.view_controller_push` | `cf_func`, `cf_vtable_slot`, `sharc_func` | `code` |
| `data` | `cf.machine.data.name.table` | `cf_ptr`, `sharc_table`, `sharc_mem_addr` | `data` |
| `value` | `dsp.voice.value.frame_base` | `sharc_literal` | `value` |
| `sites` | `cf.param.sites.descriptor_table.ref` | `cf_params_refs`, `cf_params_bounds` | `sites` |
| `site` | `cf.menu.site.settings_list.tail` | `cf_func` / `cf_words` with an offset into a function | `code` |
| `hook` | `cf.menu.hook.settings_list.tail` | none; declared here, a manifest `hook` op names it | `[[hook]]` record per device (name, site, offset, mode) |
| `struct` | `cf.menu.struct.MenuItem` | none | none; generated types only |

An `fn` may use a `cf_ptr` rule: a pointer literal to a function (the port anchor kind is then `data`).
`site` is a place inside a function that a hook or patch targets. `fn` is a callable entry point. The split matters for
generation: an `fn` becomes a callable binding, a `site` never does.

## Examples

### A function shared by both devices

```toml
# the view subsystem file (written by the rename commit)
[[fn]]
name = "cf.view.fn.view_controller_push"
sig = "bool (ViewController *mgr, shared_ptr_View *sp, bool replace_same_class)"
doc = "Push a view on the view stack; replaces a view of the same class when asked (finding 71)."
devices = ["dt2", "dn2"]
rule = { kind = "cf_func", hash = "…" }     # same bytes on both devices
mark = "D"
```

### A function with a rule per device

```toml
# devices/fw/dsp/fx.toml
[[fn]]
name = "dsp.fx.fn.entry"
doc = "Effect chain entry in the frame orchestrator."
devices = ["dt2", "dn2"]
rule.dt2 = { kind = "sharc_func", reloc_hash = "…", offset = 0x0, len = 6 }
rule.dn2 = { kind = "sharc_func", reloc_hash = "…", offset = 0x0, len = 6 }
mark = "D"
```

### A site and the hook that uses it

```toml
# devices/fw/menu.toml
[[site]]
name = "cf.menu.site.settings_list.tail"
doc = "End of the SETTINGS item list rebuild."
devices = ["dt2", "dn2"]
rule = { kind = "cf_func", hash = "8db782f6fd735f89d3b7c856316c0b90", offset = 0x3a8, len = 8 }
mark = "D"

[[hook]]
name = "cf.menu.hook.settings_list.tail"
site = "cf.menu.site.settings_list.tail"
cpu = "cf"
mode = "before"                     # the hook runs, then the displaced instructions
regs = { in = ["a2"], clobbers = ["d0-d1", "a0-a1", "ccr"] }
handler = { sig = "void (u8 *menu)", args = ["a2"] }
devices = ["dt2", "dn2"]
mark = "D"
users = ["core/digikit-menu"]
doc = "Last step of the SETTINGS item list rebuild, before the selection is restored (a2 = the settings menu); the hook point for adding an item."
```

A SHARC hook carries its DSP mode as well:

```toml
[[hook]]
name = "dsp.master.hook.q31_converter.call"
site = "dsp.master.site.q31_converter.call"
cpu = "sharc"
mode = "retarget"
regs = { in = [], clobbers = ["r0-r3", "i4", "astat"], modes = { m5 = 0, m6 = 1, simd = false } }   # no DO loop is active
devices = ["dt2", "dn2"]
mark = "D"
doc = "The master block handler's one CALL of the Q31 converter (64 planar floats in the master buffer; arguments in r4/r8); the hook point for processing the master block in place."
```

## Hooks

A `[[hook]]` declares a hook point. Its fields are `name`, `site`, `cpu`, `mode`, `regs`, and optional `offset`,
`handler`, `devices`, `mark`, `users`, `doc` and `was`. The name is `<cpu>.<subsystem>.hook.<name>` and has the same cpu
and subsystem as its site. `site` is a declared `fn` or `site`, or `<sites name>.<i>`.

`mode`:

- `before`: the hook runs, then the displaced instructions, then the rest of the function. Patchkit copies the displaced
  instructions into a tail `hook.<site>[.<offset>].tail`.
- `replace`: the displaced instructions are dropped. The stub decides where to continue.
- `retarget`: the target of an existing call at the site is replaced.

`regs`:

- `in`: registers at the site that the hook reads.
- `clobbers`: registers the hook may leave changed when it continues into firmware code (dead there, or changed on
  purpose). Every other register must be kept.
- `modes` (SHARC only): state on entry that the hook relies on (`m0`-`m15`, `l0`-`l15` as integers, `simd` as bool).

Ranges like `d0-d1` are allowed. There is no `preserves` key.

`handler = { sig, args }` is for ColdFire hooks in mode `before` or `retarget`. `sig` uses the sig grammar above. Each
arg is a register from `regs.in` or `sp+N` (the entry stack; N is a multiple of 4 and at least 4). `patchkit gen` writes:

- a GAS macro `dk_fw_<sub>_<name>_hook handler, next` in `modules/core/fw/inc/dk_fw_<sub>.inc`. It saves the
  caller-saved d0-d1/a0-a1 that are not in `clobbers`, pushes the args, calls the handler, pops, restores and jumps to
  `next`;
- a C typedef `dk_fw_<sub>_<name>_hook` in the header;
- a Rust type `<Name>Hook` in the `dkfw` crate.

Five stubs use the macros: `dk_hook` (core/ui), `dkrt_loop_hook` and `dkrt_boot_hook` (core/runtime),
`health_block_hook` (core/health) and `dl_frame_hook` (core/dsp-link). `pk_build` adds `modules/core/fw/inc` to the
ColdFire include path.

`port_resolve.py` writes a `[[hook]]` record (name, site, offset when non-zero, mode) into each port file for every
hook whose site resolves on that device. A hook whose site does not resolve on a listed device is an error. `fwgen`
checks the schema: names, site, mode, register names, modes and handler.

A manifest op attaches to a hook by name: `[[op]] op = "hook"`, `hook = "<hook name>"`, `to = "<symbol>"`, with optional
`devices`, `place` and `expect_sha256`. `anchor`, `offset`, `displaced` and `cpu` are errors on a hook op, because they
come from the declaration. The tail and return symbols keep their names: `hook.<site>[.<offset>].tail` and `.return`.

Contracts are mark D, read from the stubs. SHARC device values stay [O] (findings 45 and 53).

### A struct with one per-device difference

```toml
# devices/fw/menu.toml
[[struct]]
name = "cf.menu.struct.MenuItem"
size = 0x54
fields = [
  { name = "id",      off = 0x04, type = "u32" },
  { name = "label",   off = 0x0c, type = "std_function" },
  { name = "action",  off = 0x1c, type = "std_function" },
  { name = "draw",    off = 0x2c, type = "std_function" },
  { name = "adjust",  off = 0x3c, type = "std_function" },
  { name = "enc_div", off = 0x4c, type = "u32" },
]
# size.dn2 = 0x58      # only where a device differs (example; DN2 is not known to differ)
mark = "D"
```

## Types and signatures

`sig` uses a small C-like grammar that the generator can parse:

- scalar types `u8 u16 u32 i8 i16 i32 bool void`, pointers `T *`, `const`, and `...` for printf-style functions;
- struct names declared in any `devices/fw/` file (`MenuItem`, `ViewController`), written without the prefix;
- opaque firmware library types: `std_string`, `std_function`, `shared_ptr_View` (layouts in `string.toml`);
- one calling convention per CPU: ColdFire is gcc m68k cdecl (arguments on the stack, result in d0, d0-d1/a0-a1
  clobbered); SHARC entries without `sig` are not callable from C and only have a register contract in their hook.

A `sig` may return a pointer: our m68k-elf gcc and the Rust backend read it from d0, and the firmware's operator new
(0x4011e954), `cf.view.fn.brain.instance` (0x40198c24) and `cf.view.fn.brain.view_manager` (0x40030a42) on DT2 1.17, and operator new on DN2
1.12, set d0 **[D]** (static check, 2026-10-27).

A `site` with `bind = "static"` also becomes a static (like a `data` entry), for a module that takes only its address.
No other `bind` value exists, and `bind` is allowed on a `site` only.

A `[[struct]]` field has `name`, `off` and a `type`: a scalar, `ptr` (a 32-bit pointer on the device) or the short name
of another struct (its size). The generator sorts fields by `off`, fills gaps with padding and refuses overlaps and
fields past `size`. `patchkit gen` writes the ColdFire bindings under `modules/core/fw/`: the Rust crate
`dkfw` (`rs/src/lib.rs`), C headers (`include/dk_fw_<subsystem>.h`) and GAS includes (`inc/dk_fw_<subsystem>.inc`). The
bindings are their own core piece, not part of `core/runtime`: the runtime, `core/health` and `core/dsp-link` call
firmware functions too, and the planned `adapters/*` modules will depend on them in the same way.

## Names

`<cpu>.<subsystem>.<kind>.<name>`:

- `cpu`: `cf` or `dsp`.
- `subsystem`: one word from `subsystems.toml` (draft: `view draw text string menu list window rtos fs project kit
  sound pattern param machine midi led audio input sample sequencer util`, and for the DSP `voice fx master frame link`).
- `kind`: `fn data value sites site hook struct`.
- `name`: snake case, named after what the firmware does.

The rename moved `dkrt.fw.*`, `dkrt.hook.*`, `dkrt.ui`, `health.*`, `input.*`, `dl.*` and the kind-less names to this
form. Each renamed declaration lists its former names in `was`. The port file lists each anchor only under its current
name.

## Resolution and proof

- `port_resolve.py` reads `devices/fw/**/*.toml` instead of the manifests' `[[anchor_rule]]`. A declaration without a
  rule for a device does not exist on that device, as with today's `devices` filter.
- The `rule` field in a port anchor (today `"<module>:<kind>"`) becomes `"<subsystem file>:<kind>"`.
- Proof for the move, in two commits:
  1. Move only (old names kept as the names): for every port file, the set of (name, cpu, kind, addr, pm, len, sha256,
     mark) is identical before and after. Only the `rule` provenance text changes.
  2. Rename: every old tuple is still present unchanged (as an alias), and each new name has exactly the values of
     its alias. The rename merged three pairs that were the same firmware object (operator new, the block read and
     the link record literal) into one declaration each, with the larger `len`. Step 5 then moved every user to the
     new names, renamed `aliases` to `was` and removed the alias anchors from the port files; the patched images
     stayed byte-identical.
- `cf.params.fmt` and `cf.params.slotbuild` (generator only, never used) are dropped. The `pk_*_rules*.py` generators
  are deleted; new rules come from `catalog emit-rule ADDR` on the current image.

## Reference

`patchkit gen` also writes `modules/core/fw/reference.json`. It holds every declaration with its doc, the rule per
device, the resolved address per port, the binding names, the hook contract and the struct layout. `patchkit gen --check`
fails when the file is out of date. `mise run fw:reference` renders it as a searchable page, by default
`out/reference/fw.html`. Open an entry with `#<entry name>` or a subsystem with `#<cpu>.<subsystem>`.

## What modules declare

- `core/*` adapters: the anchors they need, by name, through undefined symbols. Until step 5 the existing
  `[object.externs]` mapping stays; step 5 replaces it with generated bindings whose symbol name is the anchor name.
- Content modules: nothing. They use the runtime API and the SDK headers only.

## Decisions (Em, 2026-10-27)

1. One file per subsystem; SHARC subsystems in `devices/fw/dsp/`.
2. Former names are kept in `was`; step 5 moved the users to the new names when generated bindings replaced
   `[object.externs]`.
3. `catalog declare ADDR NAME` writes a declaration stub from the review decision and the suggested rule.
