# project service

A service of the digikit runtime that keeps the digikit data of one Elektron project in a `.dkp` file of the store (`projects/<number>-<name>.dkp`), reads it when a project becomes
current and writes it at the next successful stock save. The runtime's settings overlay makes module settings per project through it (`docs/design/digikit-project.md`).
It owns no record kind and takes five hooks on the stock project paths. The layout follows `services/shortcuts` (the template); this README lists the differences.

```text
src/project.rs   the owner `Project<H>`: the engine and the host state, `Provider` (start_in sets the overlay, stop takes it away), and the calls the file's entries make
src/engine/      `ProjectService`: the state and the work after the hooks (bound project, queue, scan, plan, task run, write job, module restore); reaches the outside through `Env` and `Files`
src/dkp/         the file format over the record codec, the RAM image and the project scope's get, put and remove
src/name.rs      the file names of the store from the stock project names
src/lib.rs       docs, `mod` only
dks/             the service file: `lib.rs` (the `service!` call), `hooks.rs` and `hooks.s` (the five hooks), `glue/` (`host.rs` the `DkHost` state, `env.rs` and `files.rs` its
                 `Env` and `Files` over the SDK, `scope.rs` the overlay table, `exports.rs`, `tick.rs`, `sdk_imports.rs`), `gen_exports.rs` (generated: never edit)
tests/           `service/` (the engine over fakes, by behaviour), `contract.rs` (the shared suite through `provider_contract` on every device with a DSP), `state.rs` (start and stop
                 over a `FakeCore`), `file.rs` (the file's entries over the owner cell; it compiles `dks/src/lib.rs` by path), `host/` and `stubs/`; device test in `tests/project.device.test.ts`
```

Differences from the shortcuts template:

- The owner is generic: `Project<H: Host>`. `Host` is the engine's `Env` plus what the file must give (`open`, `files`, `begin_pass`, `overlay`); the file's `DkHost` makes the SDK calls, a host
  test uses a fake. A neutral crate cannot make those calls, and `impl Provider for Project<DkHost>` in the file would break the orphan rule.
- `start_in` fills the cell with zero bytes, then `open`s the host (a failure is the status of `init`) and sets the overlay (`dkc_settings_overlay_set`); `stop` sets it to null.
  Zero is the initial state of the engine and of `DkHost` (`tests/state.rs` drives a zeroed and a new engine through the same project change). No part but the lifecycle is adopted:
  `parts = []`. The module-record `tick = glue::tick` stays: it runs once per UI pass, in the place the old file ran it.
- The statics that stay, each read by name or by an interrupt: `DKRT_PROJECT_PENDING` (`dks/src/hooks.rs`; written by `hooks.s`, taken by the tick) and `SCOPE` (`glue/scope.rs`; the
  table the runtime forwards the settings calls to). Nothing else is static.
- The write job is not in the owner. Its data (the writer's steps and the file buffer) is `DK_JOB`, a static of the file (`engine/job.rs`, `StoreJob`), and the listing client's handle for
  the I/O task is a word beside it. The entries `io_step` (I/O task) and `io_done` (UI task) work on those two statics and never reach the owner. A phase word hands the data over:
  `Idle`, `Posted`, `Running`, `Done`; the UI task touches it only in `Idle` and `Done`, the I/O task only in `Posted` and `Running`, and each phase is one aligned word store.
  `tick` gets `DK_JOB` as an argument. Everything else uses `with` and answers "no service" on `None` (`E_SET_NOKEY` for a get, `E_SET_OFF` for a put or remove, `E_NOT_FOUND` for `dk_project_current`).
- Nesting: every call into the owner runs under its busy state (`Owner::run`, see "re-entry" in `services/shortcuts/README.md`): the exports and the overlay table answer "no service"
  inside another call, the refusal is counted (`DK_SVC_OWNER.refused()`), and a second `&mut` is never formed. The tick is the only code that runs inside `with` for long, and
  nothing in it calls `dk_project_status`, `dk_project_current` or the overlay table synchronously: a module's settings change only queues a reload (`dkc_settings_changed`),
  which the runtime runs outside the tick. `tests/file.rs` pins the refusal. The service takes no events (no `Reentry` part).
- Re-entry and the I/O task are two cases: re-entry (a call that arrives while the owner is in a call) is refused by the busy state; the I/O task never needs the owner, because the
  state it works on is not in it. Every host write test drives the job cell with a real posted job (`tests/job.rs` pins the phases).
- `service-check` still reports `src/records.rs` missing: the service has no record kind, and the check requires the file.
