# fx service

A service of the digikit runtime for run-time effects: a module's effect gets a page after CHO in the FX view, its knobs and MIX are written to its row of the effect chain, its route
(master or send bus) and send amounts are kept in the settings, and the kit values of the effect are saved with the kit. The service owns the effect record kind and takes nine hooks
of the effect pages. The design is in `docs/design/digikit-registry.md` ("Effect pages") and `docs/findings/61-fx-pages.md`, `64-fx-send-page.md`, `65-fx-routes.md`.

Layout (the template is `services/shortcuts`, see its README for the pattern):

```text
src/fx.rs         the owner `Fx`: the builder collection and the guard memory (private), `Provider` (start, detach), `Ticker` and `Describe` (cap, table_check, describe) in one block each
src/records.rs    the `Records` part of the owner (begin, commit, check, apply) and the free `validate`, the entry that `patchkit records --check` calls
src/builder.rs    the builder calls (`dk_module_ctx_add_fx`, `dk_fx_add_page`, `dk_page_add_fx_param`, `dk_fx_routes`, `dk_fx_mix`): `Builder` holds the collection of one module's
                  `init` and the gate, lowers it to the effect records and declares the route flags and defaults at the commit; the owner gives it out with `Fx::builder()`
src/rule.rs       the rule for the effect records (`effects`), shared by `Records::check` and `validate`
src/dsp/          the effect rows of the DSP loader (`mod.rs`) and the `DspPart` of the owner (`part.rs`: check_sections, check_rows, claim, release, clear, row_step, attribute, diag)
src/routing.rs    FX ROUTING: the SETTINGS list of the effects, the routes offered and what a pick does, over `CoreApi` and the `Ui` trait (`routing/tests.rs`: FakeCore and a fake UI)
src/fxpage/       the effect pages: page rows, parameter rows, the kit's values, the page list and the tick that sends changed values (`DKRT_FXP`, `DKRT_FXP_*`)
src/fxsend/       the send route and send amounts: cells, settings, the tick over the 16 Sound objects (`DKRT_FXS`), and `describe`: the MODULES words of an effect module
src/rows.rs       the effect rows of the chain (`DKRT_FROWS`)
src/slots.rs      docs only: the file fills no pointer slot (`patchkit service-check` wants the file because the manifest has DSP `[[fill]]`s)
src/lib.rs        docs, `mod` only
dks/              the service file: `lib.rs` (the `service!` call), `hooks.rs` (the nine hooks, assembler), `builder.rs` (the builder exports, which reach the owner), `ui.rs` (the FX ROUTING row; its three callbacks hand an `Event` to the owner's `Reentry` queue, so a pick that arrives while the service is busy runs after it, and a full queue shows `BUSY: TRY AGAIN`). A pick saves the route, queues the reload and shows its toast when the job ends (`Routing::poll_job` from the tick): `NAME: ROUTE`, or `NAME: ROUTE ON STOP` when the transport plays (the runtime then reloads the module at stop: `dkc_reload_when_possible`; no toast follows that reload), or `NAME: RESTART TO APPLY`; the job is matched by its serial (`DkrtLive::serial`), and a wait ends when the effect is torn down by anything but that job (`Routing::effect_gone` from `detach`). Picking the saved route while its reload is queued shows `NAME: ROUTE PENDING`. A pick while the reload is deferred to the stop saves a changed route, calls `dkc_reload_when_possible` again (the runtime ignores it while the module is queued and marks it again when its queue gave up, so no second reload job starts) and shows `NAME: ROUTE ON STOP`. `gen_exports.rs` (generated: never edit); `dsp/*.s` is the effect dispatch of the DSP
tests/            host tests over `FakeCore`: `contract.rs` (the shared suite through `provider_contract`, and the cases of reload and park), `records.rs` (apply, tick, detach, check,
                  DSP steps), `builder.rs` (the collection, the lowering and the commit), `table.rs` and `fxpage_checks.rs` (the image table, the record
                  rule), `guard.rs` (diagnostics, table check, attribute, section check), `file.rs` (the file's code over the owner cell); `stubs/` (the `dkc_*` imports as host functions)
```

State: the owner holds what is private to it (the builder's collection and gate, what the guard has logged). Four tables stay named statics in the concept file that owns them, written only
through that concept's functions, because something outside the service reads them by symbol:

| Static | File | Reader |
| --- | --- | --- |
| `DKRT_FROWS` | `src/rows.rs` | `core/dsp-link` (`_dl_win2_base = "window2.rows"` in its module.toml), the contract suite (`rows.kept`), the runtime's host tests of cleanup and invariants |
| `DKRT_FXP` | `src/fxpage/state.rs` | the device tests `fx-kit-persist` and the shot module's link test (`sent`, `errors`, `seeds`) |
| `DKRT_FXS` | `src/fxsend/state.rs` | the device test `fx-track-route` (`allow`, `dflt`) |
| `DKRT_FXP_TAB`, `DKRT_FXP_ROWS`, `DKRT_FXP_LIST_BUF`, `DKRT_FXP_COUNT`, `DKRT_FXP_IDS`, `DKRT_FXP_DEFS`, `DKRT_FXP_*` counters and cells | `src/fxpage/symbols.rs` | the assembler of the nine hooks (`dks/src/hooks.rs`) |

`DKRT_FXP` and `DKRT_FXS` hold more than the counters the device tests read: the strings (`tab`, `title`, `name`, `short`, the `rep_*` placeholders) that the page and parameter rows
of the firmware's tables point at, and the values the tick and the nine assembler hooks reach by symbol with no owner object in hand. The firmware, the DSP and the hooks read them by
address, so the address must be one fixed, named object (not an instance block that `start` could place elsewhere). `DKRT_FROWS` is read by the DSP code through `core/dsp-link`.
Only the owner's private data (the builder's collection and gate, the guard's log) is in the owner object.

`DKRT_FXP_FW` is the other way: two words of the hooks' assembler (the stock page list of the FX view and the `moveq` of its builder, resolved by the port file) that `Fx::start` copies into
`DKRT_FXP_TAB` (`fxpage::firmware`). A host build has none.

How it works:

- `Fx::start` is the file's `init` step before the hooks are installed: it binds the table of the effect pages (`fxpage::bind`: the generated device facts of the core's device, `facts.rs` and `tables.rs` written by `patchkit gen` from the device files' `[fx]`, and the two blocks of `rows_info` that `core/rows` reserved) or refuses the file with `E_NO_REGISTRY` (no `rows_info`, no block, a block with too few rows, a device without facts). A file that
  starts has a table, so the resident query reads it without a core (`fxsend::bound_slots`).
- A module's `init` calls the builder exports; they reach the owner through `DK_SVC_OWNER.with` and fill its `Builder`. With no owner (before `init` or after `fini`) an export that
  opens a handle returns null and a setter does nothing. `Records::commit` checks and lowers the collection to `EFFECT` records and declares the route flags and defaults of the effect's
  DSP row; `check` runs the rule (`rule::effects`) over the block; `apply` writes the page, parameter rows and list entry of one record.
- `Provider::detach` is the teardown of an effect module, whatever route it runs on: page, list entry, send cell and defaults go back (the `DspPart::clear` before it frees the DSP row).
  `can_stop` keeps its default: the service never holds up a reload. The route and the send amounts are settings keyed by the module id, so the next load of the module gets them back; the
  knob values stay in the kit's array.
- The service has no Query part and no `dk_service_query_v1` export: the menu no longer asks which routes fit, because the service shows the choices itself (`routing.rs`).

Describe (the MODULES reply): `Describe::describe` (`fxsend::describe`) gives one word for each effect row of module record `who` - 1 that has a page, in row order: the row (bits 0 to 7), then four bits
each of the route it runs on, the route the settings give it now, the routes its record allows and its default (1 master, 2 send). It has no object and runs on the MODULES command handler's task outside
the owner: it reads the tables of the service by name (`DKRT_FXP` page owner, `DKRT_FXS` route, allowed and default: one byte each, so a task switch cannot tear a word) and one settings value (`fx/route/<id>`, a
byte of the settings record, which the UI task writes on a pick); the longest read is the four rows and four settings reads of the table. A module with no effect row gives no word, so no section. This replaces the
`dkrt_fx` bridge (`fx_entry`; the export `dk_fx_routes_entry` is gone, and `dkrt_fx` answers `E_NOT_FOUND`).
