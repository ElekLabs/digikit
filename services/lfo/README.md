# lfo service

A service of the digikit runtime that lets a module add LFO waveforms. A module's `lfo_shape` records (32 samples and a 4-character name) become values of the LFO WAVE parameter:
the service gives each shape key a wave number (7 up, at most 8, always below 0x80), the interrupt hooks of the LFO tick read the shape from the table, and the MOD page shows
the name and a picture of it. It follows the layout of `services/shortcuts` (read its README for the pattern).

```text
src/lfo.rs      the owner `Lfo`: the settled keys and their wave numbers; `Provider` (`start_in`, `settle`, `detach`) in one block
src/records.rs  the `Records` part of the owner (check, apply) and the free `validate` that `patchkit records --check` calls; the record constants (`SAMPLES`, `RANGE`, ...)
src/shapes.rs   the shape table (`SHAPES`), its lookups, the interpolation and `eval`, the lookup the interrupt of the LFO tick calls; the writes (`store`, `free`) are under the interrupt mask
src/wave.rs     the maximum of the three WAVE rows of the parameter table (`WAVE`, `sync_range`)
src/graph.rs    the tiles the MOD page draws for a shape
src/route.rs    the table index of a wave number (tied to the bounds in the hook assembler)
src/validate.rs the rule every shape module meets
src/lib.rs      docs and `mod` lines
dks/            the service file: `lib.rs` (the `service!` call), `hooks.rs` and `hooks-*.s` (the interrupt and UI hooks)
tests/          host tests over `FakeCore`: `state.rs`, `records.rs` (settle, check, apply, detach, the lookups), `graph.rs`, `route.rs`, `validate.rs`, `contract.rs` (the shared suite on both
                profiles; it compiles `dks/src/lib.rs` by path), `stubs/` (the `dkc_*` imports as host functions)
```

State that is not in the owner (the rule: only what an interrupt, an assembler hook or a device test reads by symbol stays a named static, written through the concept's own functions):

| Static | Reader | Writer |
| --- | --- | --- |
| `shapes::SHAPES` | the interrupt of the LFO tick (`dk_lfo_eval` of the hooks), the UI lookups | `shapes::store`, `shapes::free`, under the interrupt mask |
| `graph::GRAPHS`, `graph::MASK` | the graph hook of the MOD page | `graph::publish`, `graph::retract` |
| `wave::WAVE` | `sync_range` and `Records::check` | `wave::set_firmware` (at `start_in`), `sync_range` |

The tests of the crate share these statics, so each takes a lock; the owner itself is one per test.
