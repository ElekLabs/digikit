# __NAME__ service

A service of the digikit runtime, made by `mise run module:new -- service __NAME__`. Every service has this shape:

```text
services/__NAME__/
  Cargo.toml        package dkrt-svc-__KEBAB__: the neutral crate, host-tested over FakeCore
  src/lib.rs        pub mod records, slots (and dsp when there is a src/dsp/ folder)
  src/records.rs    the Service handlers (declare, validate_records, check, apply, remove, tick ...)
  src/slots.rs      what the service fills in the firmware (rows, slots, hooks)
  src/dsp/          the service's parts of the DSP section (only when it has any)
  tests/            host tests over FakeCore
  dks/module.toml   id __ID__, role = "service"; one [[hook]] for each hook of service!
  dks/src/lib.rs    the service!(...) call and nothing else
  dks/src/hooks.rs  every hook as assembler (only when the service takes hooks)
  dks/src/fw.rs     firmware calls (only when the service needs them)
```

The neutral crate exports only `dk_module` and `DKRT_*` state symbols, has no assembler and names no firmware (no `cf.` hook or anchor names, no `dkfw`). What the service
needs from the image comes from the image adapter `image-modules/points/X`: reserved rows, hook stubs and `[[slot]]` words, and `[[info]]` data tables, found by name hash
with `dkc_info` (`CoreApi::info`). The image half is the adapter's; the runtime names neither the service nor the adapter's table.

Every service implements `validate_records` (no default): the rule for the records of a kind it owns, as far as the record bodies alone decide it. `check` runs it at load and
`patchkit records --check` runs it on a built module, so the load and `module:check` cannot differ on these rules. What needs the device, the module's image or another kind stays in `check`. A service that owns no kind, or needs no rule, writes `dkrt_abi::accepts_records!();`. A service with a rule also has an inherent `validate` that the trait method calls; patchkit calls it through the owner map in `packages/patchkit/src/record_rules.rs` (add one line there).

Make it a real service:

1. Add a `[[service]]` row (no `rust`: the service is the file `__NAME__.dks`) and its `[records.kinds]` entries (`service = "__NAME__"`) to `sdk/sdk.toml`, then `patchkit gen`.
2. Add the workspace member in `Cargo.toml`. A service file needs no entry in the runtime's dependencies or `owners.toml`.
3. A service that takes hooks: one `[[hook]]` in `dks/module.toml` for each, the `service!(..., setup = f, hooks = [("cf.x.hook.y", hooks::entry), ("cf.x.hook.z", hooks::z_dn2, dn2)])` list (an entry with `dt2` or `dn2` is for that device),
   and the entry as assembler in `dks/src/hooks.rs`. A hook that takes the whole control flow of its site (no handler in `ports/fw/`) may continue in the stock code of the site with the exit `tail = true`;
   its assembler refers to `dk_tail_<8 hex digits of name_hash(hook)>`, which the port file resolves to code generated from the user's firmware (docs/PATCHING.md "Raw hooks").
   Firmware references (resolved by the port file; one example each, aliases in `[object.externs]` of `dks/module.toml`, H = hash of the hook):
   `_blit = "cf.draw.fn.blit"` then `jsr _blit` (`dk_fw_`, a firmware address; Rust takes `dkfw::draw::blit` and needs no alias);
   `_x = "hook.cf.x.hook.y.exit.1"` then `jmp _x` (`dk_exit_`, leave at the declared exit 1 of the hook's `exits`; `.exit` is the end of the site);
   `_t = "hook.cf.x.hook.y.tail"` then `jmp _t` (`dk_tail_`, the displaced instructions, then on);
   `_s = "hook.cf.x.hook.y.stock"` then `jsr _s` (`dk_stock_`, the stock path called from inside the hook; it returns);
   `_r = "hook.cf.x.hook.y.ret"` then `jmp _r` (`dk_ret_`, leave as `rts` does). See docs/PATCHING.md "Firmware references".
   A fill (the pointer slot of a fixed firmware site) is one `[[fill]]` (`slot`, `entry`, optional `devices`) in `dks/module.toml` and one `fills = [("slot", hooks::fill_fn, dn2)]` entry;
   as for hooks, an entry with `dt2` or `dn2` is for that device. The long forms of `service!` need `setup = f` (the image half), and `fills` must match the `[[fill]]` entries (`exports` comes from `sdk/sdk.toml`).
   `dks/module.toml` and `dks/src/lib.rs` hold commented examples of each.
   A service that needs an image table (like `machines_info`) requires the adapter that provides it, as `core/services/machines` requires `core/machines`: put the adapter in `requires` of `dks/module.toml`.
   A service that offers functions to other modules: each is an `[[import]]` of the service in `sdk/sdk.toml` with `provided = true` (a function that is not a thin wrapper over the crate's `build::*` names its module in `wrapper = "fw"`). `patchkit gen` writes `dks/src/gen_exports.rs` (the pure wrappers and `exports()`); `lib.rs` declares `#[rustfmt::skip] mod gen_exports;` and passes `exports = gen_exports::exports()` after `fills =` (`hooks =` and `fills =` come first; pass `fills = []` when there is none). `service!` declares each export in `init`:
   a header entry `init` does not declare refuses the file with `E_DECL_KIND`. `dks/module.toml` has no `[[export]]`: the header lists the provided imports.
4. `mise run module:check -- service/__NAME__` runs `patchkit service-check` (the layout above, the `service!` hooks against `[[hook]]` per device, assembler only in `dks/src/hooks.rs`, firmware names only under `dks/`,
   the one exported entry symbol `dk_module`, the generated export list against the provided imports, the declarations against `sdk.toml`) and the crate's tests.
