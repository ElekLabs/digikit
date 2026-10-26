# Runtime acceptance phase timing, 2026-10-27

**[D] [C]** The October 26 gate took 1,531 seconds; runtime workflows took
1,244 seconds. Its file timings identify remote-input (560 seconds),
input-adapter (396 seconds), and transfer-live (122 seconds) as the largest
runtime files. Those old logs do not isolate boot time. The earlier claim
that repeated boots account for the long runtime suite was a hypothesis,
not measured attribution.

## Instrumentation

Runtime tests now emit `runtime-phase` JSON records through
`tests/regression/timing.ts`. Records carry a scenario scope, device, unique
session and span IDs, parent span, monotonic wall duration, and success or
failure. Output errors do not replace operation results or errors.

Phases cover aggregate session startup, boot validation, scenario body,
settling/input waits, module upload/install, protocol restart and boot wait,
CLI commands, and shutdown. Module acceptance uses the same scope context.
Durations are inclusive: do not add a parent body/install duration to its
children. CLI install/restart remains an aggregate operation. Startup includes
input resolution, daemon launch, card/firmware preparation and loading; it
is not a pure firmware-boot or DSP measurement.

No waits, timeouts, restart counts, session reuse, test assertions, emulator
defaults, or Rust code were changed. Fresh boots and persistence checks
remain independent.

## Bounded pilot

One DT2 OS 1.17 `service-commands` case passed with the previously verified
daemon and prepared `svc-echo` module. The DN2 case was filtered out. The
run held the exclusive benchmark lock, used normal fast-region configuration,
and performed no Rust build. It is one observation, not a speedup comparison
or an estimate of the whole suite's distribution.

| Phase | Seconds | Relationship |
| --- | ---: | --- |
| Session startup | 30.219 | Separate setup phase |
| Boot validation | 0.040 | Separate validation phase |
| Scenario body | 25.363 | Includes the following nested phases |
| Fixed settle | 12.001 | Child of body |
| Module install | 13.256 | Child of body |
| Module upload | 0.052 | Child of install |
| Restart command | 0.009 | Child of install |
| Wait for boot | 13.157 | Child of install |
| Shutdown | 0.252 | Separate teardown phase |

The whole invocation took 56.519 seconds. Records correlate to one session;
all phases succeeded. Source artifacts are local under
`out/native/runs/20261027-runtime-phase-timing/`: `invocation.json`, `pilot.log`,
`phases.json`, and `validated-summary.json`. The invocation pins the daemon
and prepared module directory from the prior gate.

## Validation and next decision

The changed-file preflight passed, including 38 related host tests, formatting,
lint and workspace types. Five timing-wrapper tests cover nested/concurrent
attribution, returned values, synchronous/asynchronous errors and logging
failure. Independent review found a module-context gap, which was fixed and
re-reviewed; regression types and scoped lint/format passed afterward.
The live pilot checks actual phase output. The full routed gate has not been
rerun for this instrumentation-only change; it is still required before merge.

**[O]** Profile remote-input and input-adapter with these records next. If their
fixed waits dominate, investigate readiness predicates before shortening
them. If startup dominates, split `Session.start` into measured preparation,
launch and firmware-load phases before considering snapshot reuse. Preserve
the identical fresh-boot comparison and restart/persistence coverage. This
change adds evidence; it does not yet reduce test execution time.
