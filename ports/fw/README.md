# Firmware declarations

This directory holds the firmware facts of the Digitakt II and Digitone II: one declaration per function, data object,
site or parameter list, with the rule that finds it in a stock image. One file per firmware subsystem.
Design: [docs/design/firmware-decls.md](../../docs/design/firmware-decls.md).

```toml
[[site]]
name = "cf.menu.site.settings_list.tail"
was = ["cf.menu.settings_tail", "cf.menu.site.settings_tail"]
cpu = "cf"
rule = { kind = "cf_func", hash = "8db782f6fd735f89d3b7c856316c0b90", offset = 0x3a8, len = 8 }
mark = "D"
users = ["core/digikit-menu"]
```

`tools/patchkit/port_resolve.py` reads these files (`mise run patch:port`) and writes `ports/`. Names follow
`<cpu>.<subsystem>.<kind>.<name>`: `cpu` is `cf` or `dsp`, `subsystem` is a word from [subsystems.toml](subsystems.toml)
(each has one file: `<subsystem>.toml`, or `dsp/<subsystem>.toml` for the DSP), and `kind` is the table of the entry
(`fn`, `data`, `value`, `sites`, `site`). `port_resolve.py` refuses a name that breaks the rule.

Every entry needs a `doc`: one line on what the object is or does. `patchkit gen` refuses an entry without one.
`mise run fw:reference` writes the searchable reference page.

To declare a function, use `tools/firmware/catalog.py` on a stock image (SHARC if the address is below 0x1000000). Both
commands compute the rule from the function that contains ADDR, check that it resolves back to ADDR, and with `--other`
check the other device against the atlas catalogue pair:

```sh
uv run python -m tools.firmware.catalog emit-rule dt2-1.17 0x40113dc8 --other dn2-1.12
uv run python -m tools.firmware.catalog declare dt2-1.17 0x40113dc8 cf.text.fn.draw_fmt --other dn2-1.12 --doc "..." [--write]
```

A virtual function whose hash is shared by more than four functions gets a `cf_vtable_slot` rule instead
(`{ kind = "cf_vtable_slot", class = "ConfirmWindow", slot = 4, offset = 0, len = 8 }`; `offset_to_top` defaults to 0):
it names the RTTI class and the vtable slot, and a class with another name on the other device gets its own rule.

A raw hook (mode `replace`, no `handler`) may name the places a service leaves it besides the end of its site, and the tail:
`tail = true` (the displaced instructions, then the end of the site) and `exits = ["cf.machine.site.clone.5 + 12", "cf.machine.site.clone.5.alt"]`
(anchor names with an optional offset, evaluated on the port of the bound firmware; the first is exit 1). A service reaches them with the imports
`dk_tail_<hook>` and `dk_exit_<hook>_<n>` (n = 0 is the end of the site); see "Firmware references" in docs/PATCHING.md.

A raw hook (mode `replace`, no handler, no `tail`, `stock` or `exits`) may instead be a patch site: `patch = 2` (1, 2 or 4) with the `site` and `offset` of plain bytes, for example the operand
of an instruction or a table word. The port file lists it as kind `patch` with that length and the check value of the stock bytes; no jump is written there. A service takes it with
`[[patch]]` and gives the bytes in `init` (see "Patches" in [docs/PATCHING.md](../../docs/PATCHING.md)). The anchor of the `site` must cover the bytes (`offset + patch` within its `len`).

`declare` prints a `mark = "O"` entry (a shared rule, or one rule per device) and with `--write` adds it to the
subsystem file in name order. Review the entry and raise the mark after a check against image bytes.

An `fn` may have `sig = "u32 (void *p, const char *fmt, ...)"`: a return type and typed, named arguments, `...` last.
Types are `u8 u16 u32 i8 i16 i32 bool void`, `char` behind a pointer, a struct short name, `const` and `*`. A pointer
return is allowed (the firmware sets d0). A `site` with `bind = "static"` is also written as a static (for a module that
takes only its address); no other `bind` value exists. A `[[struct]]` has `name`, `size`, `mark`, `users` and `fields = [{ name, off, type }]`;
`type` is a scalar, `ptr` or a struct short name. `patchkit gen` writes the `cf` bindings to `ports/bindings/`:
`rs/src/lib.rs` (crate `dkfw`), `include/dk_fw_<subsystem>.h` and `inc/dk_fw_<subsystem>.inc` (GAS).

A `[[hook]]` declares a hook point at a `site` or `fn` (`name` is `<cpu>.<subsystem>.hook.<name>`). `mode` is `before`
(hook, then the displaced instructions), `replace` (displaced instructions dropped) or `retarget` (an existing call is
redirected). `regs` has `in`, `clobbers` and, for SHARC, `modes`; every other register must be kept. A ColdFire hook
may have `handler = { sig, args }`: `patchkit gen` then writes a GAS macro `dk_fw_<sub>_<name>_hook`, a C typedef and a
Rust type. `port_resolve.py` writes a `[[hook]]` record into each port file. A manifest `hook` op names the hook. See
[firmware-decls.md](../../docs/design/firmware-decls.md).

A renamed declaration lists its former names in `was`, for reading older docs and findings. A former name never
resolves: the port file lists each anchor only under its current name. A former name must not be a current name, and
`catalog declare` refuses one. Content modules never use these declarations; only `core/*` adapters do.

## Hook calling contexts

A hook of a service runs in the context of the firmware code it replaces (statically read from the port comments, finding 149 and the service sources; [D] unless marked). The Owner guard
(`dkrt_svc_rt::Owner`) admits a hook that enters the owner only in the UI task and refuses it, counted, inside a part call of the same service. An interrupt hook must never enter the owner.
Host tests fire a hook in a context with `dkrt_test_support::hook_context::HookContext` (`UiTask`, `InPartCall`, `Interrupt`).

A hook declared `context = "interrupt"` (`CTX_IRQ`, flag bits 1..2 = 3) is installed like an `above_ui` one. A live RELOAD of a service that has such a hook is refused with `E_BOOT_ONLY` (the menu shows "RESTART TO LOAD"), because no task can be parked out of an interrupt entry (`Loader::provider_rules`, checked on the loaded and on the new file).

**Open item.** The lfo reload hang: the UNLOAD step of an lfo closure reload never returns on DN2. Diagnosis so far is in `out/runs/hot-reload/`. The guard keeps the hang out of reach until it is understood; remove the `interrupt` context from the lfo tick hooks only together with the fix.

| service | hooks | context | enters the owner |
| --- | --- | --- | --- |
| lfo | `wave_name`, `wave_name3`, `graph_vec` | UI task (MOD page) | no (`an_lfo_page_hook_fired_inside_a_part_call_never_enters_the_owner`) |
| machines | `source_key`, `source_turn` | UI task [O] | yes |
| machines | `frametype` and `type_reader` (DN2) | frame builder, may be an interrupt [O] | no: static table |
| machines | the other type and page helpers | [O] | no |
