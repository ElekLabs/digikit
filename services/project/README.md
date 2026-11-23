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
- The write job's entries `io_step` (I/O task) and `io_done` reach the owner with `DK_SVC_OWNER.instance()`, not `with`: the I/O task must not find the cell busy while the UI task is in
  `tick`. Everything else uses `with` and answers "no service" on `None` (`E_SET_NOKEY` for a get, `E_SET_OFF` for a put or remove, `E_NOT_FOUND` for `dk_project_current`).
- Nesting: every call into the owner runs under its busy state (`Owner::run`, see "re-entry" in `services/shortcuts/README.md`): the exports and the overlay table answer "no service"
  inside another call, the refusal is counted (`DK_SVC_OWNER.refused()`), and a second `&mut` is never formed. The tick is the only code that runs inside `with` for long, and
  nothing in it calls `dk_project_status`, `dk_project_current` or the overlay table synchronously: a module's settings change only queues a reload (`dkc_settings_changed`),
  which the runtime runs outside the tick. `tests/file.rs` pins the refusal. The service takes no events (no `Reentry` part).
- The I/O task is not re-entry: it is another task, and the busy flag is the UI task's. The step forms references to the engine's writer and file buffer and to the host's file
  client only (`Project::io_step`, `ProjectService::io_step_at`, `Host::files` take raw pointers), so the I/O task forms no `&mut` to the whole owner and does not go through
  the owner. It still overlaps, in the abstract aliasing model, the `&mut` the UI task holds in `tick`; soundness rests on the UI task leaving the writer and the buffer alone while
  a job runs, which nothing enforces. Removing the overlap means moving the writer's job and the file buffer out of the owner into memory the UI task reaches by raw pointer only;
  that is a change of the engine's layout, not of the contract. Every host write test drives this path with a real posted job.
- `service-check` still reports `src/records.rs` missing: the service has no record kind, and the check requires the file.
