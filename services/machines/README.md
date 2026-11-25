# machines service

A service of the digikit runtime that adds machine types at run time: the machine, parameter, page, click-action and native-page records of a module become rows of the
firmware's registry, the machine's voice rows are published to the DSP, and the encoder click on a machine page runs a module's callback. It also plans the sticky type numbers.
Layout and rules follow `services/shortcuts/README.md` (the template); this file lists what is specific here.

```text
src/machines.rs   the owner `Machines`: private state (`Builder`, `Slots`, `Parking`, `Applied`), `Provider` (start_in, attach, detach, settle, reload_done), `Ticker`, `Describe`
src/records.rs    the `Records` part (begin, commit, check, apply) and the free `validate` that `patchkit records --check` calls
src/dsp/part.rs   the `DspPart` over the voice-row steps of `dsp/*`; the guard's diagnostics and the voice site's attribution
src/apply.rs      `attach` (the rows of a module) and the native page record: `Applied`, the walk's state
src/build/        the builder calls (`Builder`: the collection and its gate) and the commit
src/slots.rs      the page's click state (`Slots`), with `slots/` the detector and the action lookup
src/parking.rs    the parking passes of absent numbers (`Parking`)
src/info.rs       the registry words of the image's `rows_info` (`core/rows`): binding into `DKRT_MF_TAB`
src/pagerows.rs   the reserved page rows (a device whose machines keep their knobs in page rows) as free rows: `init` at `start`, `free_row` for an unload
src/native_page/  the native callback pages: `PageTable` (the owner holds it), the frame, the clipped canvas and its overlay mask, the dots, `draw_page`, `overlay_page`, `turn_page`;
                  firmware only through the traits `View`, `Surface`, `Stock` and `Content`, so every rule has a host test (`fake_firmware.rs`)
src/list.rs       the machine list the firmware's list builder copies and the rank pairs of the sort comparator, built in `start` from the stock order and the rows' positions; the owner
                  gives the four list patches their bytes (`list_bytes`, `list_end`, `list_start`)
dks/              the service file: `lib.rs` (the `service!` call), `hooks.rs` and `hooks-*.s` (the machine framework's hooks), `fw.rs` (the firmware calls, the page's key and turn
                  entries, `dk_param_get`, `dk_param_set`), `native.rs` (the native pages over the firmware: the view, the stock draw routines, the content's callbacks, and the entries
                  `dks_native_*` that the page hooks call), `builder.rs` (the builder exports, which reach the owner), `gen_exports.rs` (generated)
tests/            host tests over `FakeCore`: `state.rs` (the owner), `records.rs`, `build.rs`, `assign.rs`, `guard.rs`, `file.rs` (the file's code over the cell), `contract.rs` (the
                  shared suite on both profiles through `provider_contract`, and the placeholder and parking cases over its `Lifecycle`)
```

State: the owner holds the collection of the builder calls, the click state, the parking passes, the state between `attach` and the per-record `apply`, and the registry state
(`RegState`: the owners of the parameter and page rows, and the strings that the firmware's registry rows point at; they are valid while the service is loaded, which is as long as the owner
lives). Four tables stay named statics of the crate (D7 of the RC3 briefs), each with its reader, and one cell is shared:

| Static | Reader |
| --- | --- |
| `DKRT_ASSIGN` (`assign.rs`) | device tests by symbol (`machines-kit`, `fx-live`); the sticky map |
| `DKRT_VROWS` (`rows.rs`) | the runtime's DSP code (`abi/dsp.rs`); the voice rows; `Describe` on the MODULES task |
| `DKRT_GUARD` (`guard.rs`) | the MODULES reply (`Describe`), on another task than the poll that writes it |
| `DKRT_MF_TAB` (`info.rs`) | the assembler hooks (`T_*` offsets of `hooks-*.s`) |
| `MACH_OWNER` (`registry.rs`, a `Shared<[u8; MAX_MACH]>`) | `Describe` on the MODULES task, and the owner (`attach`, `detach`, the parking pass, the load check) |
| `DKRT_MF_LIST`, `DKRT_MF_RANK`, `DKRT_MF_RANKN` (`list.rs`) | the list builder's patched operands (addresses), and the rank hook `mf_rank` (the ColdFire hook file of the device that has the sort) |

`Describe::describe` (`modules::describe`) gives the five words of the MODULES reply for a module record: the voice rows it owns (mask); the guard's fault count; `nt` with the guard-off flag in bit 8;
the types 0 to 3 and 4 to 7 (one byte each, type `i` in byte `i % 4`). It has no object and never enters the owner (`Describe::describe` has no `&self`): the command handler may ask while the UI task is
inside a call of the owner (a tick, a hook), and the owner's guard would refuse it. It reads `MACH_OWNER` (`Shared`: one byte copied per row, eight rows at most: the longest section is a copy of 8 bytes
under the interrupt mask), `DKRT_VROWS` and `DKRT_GUARD` (read by words: a task switch cannot tear a word). A module with no voice row, no type and no fault gets no word, so no section.
The registry state moved into the owner in E1 step 4 (it was the static `DKRT_REG`; no tool reads it by symbol any more). The free functions of `unload.rs`, `parking.rs`, `check.rs`, `apply.rs` and
`build/` take the registry state from the owner's methods.

Without an owner (before `init` made it, after `fini`) the file's calls answer the neutral way: a builder call returns null or does nothing, `dk_param_get` and
`dk_param_set` answer `E_STATE` (no flow), the key hook passes the event and the turn hook consumes nothing. The turn hook ends the click flow in the owner, then runs the page's
native handler outside it (a handler may call back into the file).

A failed `start` (no usable `rows_info` for the device) leaves nothing: the file is refused with `E_NO_REGISTRY` and the cell stays empty. ColdFire rule: an if chain, not
a `match` on dense integers, in code that runs on the device (`Describe::cap`).

Native pages: a module's page records are registered in the owner's `PageTable` (`native_page::record::apply`) and taken out again by `unload::remove`. The hooks of the machine page call
`dks_native_draw`, `dks_native_overlay`, `dks_native_code`, `dks_native_page_count` and `dks_native_descriptor`, which look the page up in the owner and clone it out before its
callbacks run (a callback may call back into the file). A page's descriptor holds the address of its own name (a `std::string` representation), so the table stays where the owner
is made and is never moved once it has pages. The clipped bitmap is a static (6.8 KiB, too big for a UI stack). The frame layout is asserted against the struct by the fixture
`tests/fixtures/modules/native-page` (`frame-layout.h`), which the ColdFire compiler checks, and against the service's constants by `native_page/frame/tests.rs`. The machine type of the
selected sound comes from the copy of the stock type reader when the image has one (`rows_info` word `READER_REAL`), else from the firmware's current-track type function.

What the image keeps (`core/rows`, `core/ui`, `core/dyndsp`) and what the service does at run time (RC4-4):

- The image has the type reservation and the dispatch of a type; a row without descriptor strings gets the empty ones there. The service writes the strings with a row, and `stop` takes them out of the
  reserved rows again (they were buffers of the file).
- The machine list getter's copy (`list.rs` `copy_listed`, entered by `dks_list_copy` of `dks/src/fw.rs`) leaves out the reserved rows that have no name. A device with a slot map points its `jsr` at it with a patch
  (`cf.machine.hook.patch_list_copy`, the value is the entry's address), a device with page rows replaces the call and the store after it with the hook `mf_copy`.
- The machine selection view (the firmware makes it before any file loads) is remembered by a resident hook of `core/ui`; `machine_view` gives its variable and the builder. `selection_view::refresh_view`
  builds the list again at `reload_done` and at `stop`, after the loader restored the sites.
- The `service!` call names every hook of the file with no device: the loader takes the manifest entry of its own device (docs/MODULE-AUTHORING.md).
- The native pages are kept for the reserved types only (`[limits] machine_rows`), numbered from the first reserved type.
