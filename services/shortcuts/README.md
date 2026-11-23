# shortcuts service

A service of the digikit runtime that lets a module bind a key combination to a function. A combination is two logical keys: the first held, then the second pressed
(`dk_shortcut_add`, `sdk/include/dk/platform/shortcuts.h`). Key names are the SDK's (`DK_KEY_*`, `DK_KEY_TRIG(n)`), not panel codes.

This crate is the template for the other services: one owner object, its parts beside it, no statics. A new service copies the layout and replaces the concept files.

```text
src/shortcuts.rs  the owner `Shortcuts`: private state, the state machine (key event in, verdict out), `Provider` (start_in, detach) and `Ticker` in one block each
src/records.rs    the `Records` part of the owner (begin, commit, check, apply) and the free `validate`, the entry that `patchkit records --check` calls (it picks the kind and
                  hands the records to `rule::check_set`)
src/builder.rs    the builder call `dk_shortcut_add`: `Builder` holds the collection of one module's `init` and writes the records at the commit; the owner gives it out
                  with `Shortcuts::builder()`
src/bindings.rs   the binding table and the callbacks
src/keys.rs       which keys are held
src/rule.rs       the record layout (`at`, `BODY_LEN`) and the rule for the records (`check_set`), shared by `Records::check`, the commit and `validate`
src/lib.rs        docs, `mod` and `pub use` only
dks/              the service file: `lib.rs` (the `service!` call), `hooks.rs` and `hooks.s` (the key hook), `builder.rs` (the builder call, which reaches the owner),
                  `gen_exports.rs` (generated: never edit)
tests/            host tests over `FakeCore`: `state.rs` (owner logic), `records.rs` (commit, check, apply, detach), `contract.rs` (the shared suite on every profile), `file.rs` (the
                  service file's own code: the builder export and the key hook over the cell), `stubs/` (the `dkc_*` imports as host functions); device test in `tests/device`.
                  `contract.rs` and `file.rs` compile `dks/src/lib.rs` into the test by path (the file has its own cargo workspace, so it cannot be a dependency)
```

What goes where (copy this split):

- The owner holds all state as private fields: no `static mut`, no lock. The file's cell (`DK_SVC_OWNER`, made by `service!`) holds the one instance: `Provider::start` makes it in
  the file's `init`, `Provider::stop` ends it in `fini`. The cell is `.bss` (zeroed bytes plus a live flag), and the state is made and dropped where it lives: a service with
  a large state overrides `Provider::start_in` to write it into the slot (here one zero fill; a copy of the value `start` returns would pass through the stack), and `stop`
  takes `&mut self` and the cell drops the value in place. A hook or a builder call in `dks/` reaches it with `DK_SVC_OWNER.with(|s| ...)`; a call before `start` or after
  `stop` gets `None` and answers as "no service" (the hook passes the key, the builder call returns `E_CONTEXT`).
- `service!(Shortcuts, "shortcuts", &KINDS_SHORTCUTS, parts = [Records, Tick], ...)` lists the parts the owner adopts, in one place: the contract recipe reads them from the row
  (`file::dk_svc_row()`), it does not list them again. `Provider` is always there. A part is a trait the owner
  implements (`Records`, `Ticker`); `attach` has no work here, so the default stays, and `detach` is what the old `remove` did.
- `Provider::can_stop(&self, core)` names no consumer: a service refuses a take-down for every module or for none (no shipped service refused per owner before the provider contract).
- No device facts in the crate: the key codes and flag bits come from the device profile (`DeviceProfile`) that `core.device_id()` names.
- A builder call that is a thin wrapper over a free function is generated; one that must reach the owner is hand-written in `dks/src/builder.rs` and named with
  `wrapper = "builder"` in `sdk/sdk.toml`.
- A service marks its own SDK calls: a `wrapper = "builder"` entry in `sdk/sdk.toml` is allowed in the service's own section (the service writes its export in `dks/src/builder.rs`); no
  other service or the runtime names it.
- An owner that needs device-only SDK functions (calls that exist only on the ColdFire) is `Owner<H: Host>` in the service crate: `Host` is a trait of the crate that names those
  calls, the file in `dks/` implements it with the real calls and the crate's tests implement it with a fake. The trait impl for a foreign type has to sit where the type or the trait
  is local (Rust orphan rule), so the real host lives in `dks/` and the fake in `tests/`; `services/project` is the example (`Project<H>`, `DkHost` in `dks/src/glue/host.rs`).
- A provider with no record kinds has no `src/records.rs` and no `pub mod records`/`slots` (`patchkit service-check` accepts it when the manifest gives it no kind and `service!` uses
  `parts = [...]`): its parts (`Dsp`, `Tick`, ...) sit beside the owner, and `service!` takes `&[]` for the kinds. A provider with kinds keeps `records.rs`, `pub mod records` and
  `&KINDS_<NAME>`. A provider with no `provided` imports leaves out `exports`: `service!(Owner, "name", &[], parts = [...], hooks = [...], fills = [...])`.
- Tests: the file's wrapper code in `file.rs` (with no owner it answers `E_CONTEXT` and passes the key); logic in `state.rs` over `Shortcuts::new()` or `Provider::start`; the loader's walk (commit, check, apply, detach) in `records.rs` over one owner per test; the shared
  suite in `contract.rs` through `provider_contract` (its fixture functions reach the started owner with `dkrt_test_support::subject`).

- A failed start leaves nothing to drop: write the state fully before returning `Ok`, or return `Err` without partial state (the cell stays empty and `stop` never runs on it).

Re-entry (every service):

- Every call into the owner runs under its busy state: the part functions of the tables (through the `Enter` guard that `service!` gives them, `Owner::enter` and `Owner::leave`),
  `DK_SVC_OWNER.with` and `DK_SVC_OWNER.post`. While one runs, a second `&mut` to the object is never formed: a nested `with` or part call is refused
  (`None`, or the part's refusal such as `E_BUSY`; counted in `DK_SVC_OWNER.refused()`). The test of the flag and every change of the queue happen with interrupts masked
  (`dkc_irq_mask`, the import `lfo` uses for its shape table), so a task switch or an interrupt cannot fall between the test and the set.
- A callback that must still reach the object (a pool event fired by a foreign call) is an event. The service adopts the part
  `Reentry` (`parts = [.., Reentry]`): `type Event: Copy`, `const QUEUE: usize` (how many wait) and `fn apply(&mut self, core, event)`. The callback calls
  `DK_SVC_OWNER.post(event)`: idle, it runs at once; busy, it is queued and `Owner` runs it after the outer call returns, in order, in the same task, before the owner is idle.
  A full queue is `Post::Full`, counted in `refused()`, never silent. No heap: the queue is `N` slots in the cell. A service with no such callback adopts nothing and pays one flag.
  The shortcuts key hook is not one: it calls `DK_SVC_OWNER.with`, so a key that reaches it while a call of the service runs (only by re-entry in the UI task: a module's callback
  that reaches the key walker; interrupts and the job task never call it) is refused and counted in `refused()`. The stock walker gets the key and the service never sees it, as
  when the nested access answered `None` before the guard existed; shortcuts adopts no `Reentry`.
- A foreign call that fires callbacks into the owner is made before the owner is entered: `Ticker::pre_tick(core, phase)` (no `&mut self`) runs first in the table's tick function, so
  the events it posts run at once, in order, ahead of the tick that depends on them. The step runs also when the owner is not live (before `start`, after `stop`): its events then
  meet `Post::NoOwner` and are dropped, and the tick is refused (`svc-rt` test `the_pre_tick_step_runs_before_...`).
- The busy flag is for calls into the object. Another task's work (the I/O or job task) never goes through the owner: a job works on state of its own in a static of the file
  (a cell outside the owner) with a one-word phase handoff (`Idle`, `Posted`, `Running`, `Done`: the UI task touches it in `Idle` and `Done`, the job task in `Posted` and `Running`),
  and its end reaches the owner as an event or not at all (`services/project`: `StoreJob`; `services/samples`: `PeakJob`).
- A service file's wiring is checked by `dkrt_test_support::contract::row_is_guarded`: a part call through the file's row while a hook holds `DK_SVC_OWNER.with` is refused and counted.

Steps for a new service:

1. The owner file `src/<name>.rs`: private state, `Provider` (`start_in`, `detach`) and the other parts in one `impl` block each.
2. The parts the owner adopts (`Records`, `Ticker`, ...), listed once in the `service!` call.
3. `src/records.rs` (and `src/rule.rs`) when the service owns record kinds: begin, commit, check, apply, and the free `validate` for `patchkit records --check`.
4. `src/builder.rs` when modules call a builder function that must reach the owner: `wrapper = "builder"` in `sdk/sdk.toml` and a hand-written export in `dks/src/builder.rs`.
5. The service file in `dks/`: `service!`, hooks and exports reach the owner through `DK_SVC_OWNER.with(...)`; answer "no service" on `None`.
6. The contract test through `provider_contract`, with fixture functions reaching the owner via `dkrt_test_support::subject`; state and records tests beside it.
7. `patchkit service-check` on the file, then `patchkit objdiff` against the old object (explain every differing function; compare section sizes).

How it works:

- A module's `init` adds `SHORTCUT` records (id, label, two keys, `on`, `off`). The first owner of a pair of keys wins: a second module that asks for the same pair gets a
  clash log (`SHORTCUT_CLASH`) and its binding is not active.
- A binding whose key the device has no panel code for is kept but inactive (`SHORTCUT_INACTIVE` is logged); it works on a device that has the key.
- The hook sees every key event before the stock firmware. The first key is never swallowed (the firmware needs it). When the second key completes a binding, that press,
  its repeats and its release are swallowed, so the stock screen never sees them; every other event passes, so [TRK]+[TRIG] still selects a track.
- The hook does not call modules. It queues `on` (when the binding completes) and `off` (when either key is released); the service's tick runs the queue in the UI task,
  in the early phase only.
- When a module unloads (`detach`), its bindings and its queued calls are removed; an active binding of it ends without `off` (its code is going away).

The neutral crate exports only `dk_module` and names no firmware; the hook and the site come from `ports/fw/input.toml` and the port files.
