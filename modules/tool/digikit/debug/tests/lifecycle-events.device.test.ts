import { LIFECYCLE_STEPS } from '@digi/dk-session';
import { expect } from 'vitest';
import { installWithDebug, moduleStorePath } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';

/**
 * The runtime logs the steps of a live module load as LIFECYCLE events (docs/INSTRUMENTATION.md "Lifecycle events"): with the debug module loaded, the load job's check and load
 * steps and each step the package manager takes for the module (place, link, start, install, activate, attach) have a begin and an end that names the module's file. The module is
 * unloaded first and loaded again, so the events are the live job's, not the boot's. What is a host test: the pairs and their order over fakes (`dkrt-lifecycle` `traced`), the
 * boot's events over packed files (assembly `lifecycle_events`) and the job's steps (`reload/tests/trace.rs`); this test shows only that the firmware build emits them to a listening host.
 */
const PROBE = 'digikit/probe';
const MODULE_STEPS = ['place', 'link', 'start', 'install', 'activate', 'attach'];

runtimeTests('a live module load logs a begin and an end for each step, named by the module file', async (rt) => {
  const { dk, session } = rt;
  await expectRuntimeReady(session);
  await installWithDebug(rt, PROBE);
  const path = moduleStorePath(session.options.device, PROBE);
  expect(await dk.unloadAndWait(path), 'the module is unloaded').toEqual({ result: 0, unloaded: true });
  const events = await rt.listen();
  expect(await dk.loadAndWait(path), 'the module loads again').toEqual({ result: 0, unloaded: false });

  for (const step of ['check', ...MODULE_STEPS]) {
    expect(Object.values(LIFECYCLE_STEPS)).toContain(step);
    const begin = await events.untilLifecycle(path, step, 'begin', { withinMs: 5000 });
    const end = await events.untilLifecycle(path, step, 'end', { withinMs: 5000 });
    expect({ step, status: end.fields.status, op: end.fields.op }, `the end of ${step}`).toEqual({
      step,
      status: 0,
      op: begin.fields.op,
    });
  }
  await session.expectNoFault('live load with lifecycle events');
});
