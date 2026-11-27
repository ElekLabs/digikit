import { LIFECYCLE_STEPS, withViewObserver } from '@digi/dk-session';
import { emulatorRead, readViewStack, viewStackFacts } from '@digi/test-support';
import { expect, test } from 'vitest';
import { waitFrames } from '@digi/regression/runtime/debug-kit.ts';
import { installWithDebug } from '@digi/regression/runtime/debug-install.ts';
import { expectRuntimeReady } from '@digi/regression/runtime/ready.ts';
import { moduleStorePath } from '@digi/regression/runtime/module-artifact.ts';
import { runtimeSuite } from '@digi/regression/runtime/runtime-tests.ts';

/**
 * What the debug module observes on the firmware: one boot, the debug module and a probe module installed once, then three facts in order, each leaving the firmware as it found it.
 * - lifecycle: a live load of the probe logs a begin and an end for each step, named by the module file (what is a host test: the pairs and their order over fakes, the boot's
 *   events over packed files and the job's steps, `dkrt-lifecycle` `traced`, assembly `lifecycle_events`, `reload/tests/trace.rs`; here only that the firmware build emits them);
 * - view: SETTINGS opens and closes on the stock view stack and the observer's snapshot equals an independent read of that stack;
 * - spans: the span table counts the debug module's tick, the poll slots name their owners, and the counts hold still when stopped (the fold arithmetic is `spans_tests.rs`).
 * The parameter observer is in parameter-observer.device.test.ts (DT2 only).
 */
const PROBE = 'digikit/probe';
const MODULE_STEPS = ['place', 'link', 'start', 'install', 'activate', 'attach'];

runtimeSuite('debug observers', {
    dsp: 'sharc',
    setup: async (rt) => {
      await expectRuntimeReady(rt.session);
      await installWithDebug(rt, PROBE);
    },
  }, (runtime) => {
  test('a live module load logs a begin and an end for each step, named by the module file', async () => {
    const rt = await runtime();
    const { dk, session } = rt;
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

  test('debug observes SETTINGS open and close from the stock view stack', async () => {
    const rt = await runtime();
    const { dk, session } = rt;
    const facts = viewStackFacts(session.options.device);
    if (!facts) throw new Error('The device test needs an independent view-stack read');
    const read = emulatorRead(session);
    await withViewObserver(dk, { listen: async () => { await rt.listen(); } }, async (initial, generation) => {
      expect(initial.object).not.toBe(0);
      const until = (match: (args: number[]) => boolean) => ({
        event: 'VIEW_CHANGED' as const,
        match: (event: { args: number[] }) => event.args[4] === generation && match(event.args),
        withinMs: 10_000,
      });
      await rt.ui.tap('SETTINGS', { until: until(([old, object]) => old === initial.object && object !== old) });
      const open = (await dk.viewObserver())?.snapshot;
      expect(open).toBeDefined();
      expect(open?.classId).not.toBe(initial.classId);
      const [actual] = await readViewStack(read, facts);
      expect(open).toEqual({ object: actual?.view, classId: actual?.vptr });
      await rt.ui.tap('NO', { until: until(([old, object]) => old === open?.object && object === initial.object) });
      expect((await dk.viewObserver())?.snapshot).toEqual(initial);
    });
    await session.expectNoFault('observe SETTINGS');
  });

  test('the span table counts the debug module tick and holds still when stopped', async () => {
    const rt = await runtime();
    const { session, dk } = rt;
    const debug = (await dk.modules()).find((m) => m.name === 'digikit/debug')!;
    await dk.profileState('reset');
    expect((await dk.profileState('startTrace')).running).toBe(true);
    await waitFrames(rt, 600);
    const running = await dk.profileAll();
    expect(running.running).toBe(true);
    expect(running.hz, 'the span clock is the bus clock').toBe(132_000_000);
    const row = running.spans.find((s) => s.kind === 'module' && s.index === debug.index)!;
    expect(row.calls, "the debug module's tick ran").toBeGreaterThan(0);
    expect(BigInt(row.maxTicks), 'max is at most the sum').toBeLessThanOrEqual(row.sumTicks);
    // the poll slots name their owner: a loaded module record, or the firmware image (a patched-in core module)
    const records = (await dk.modules()).map((m) => m.index);
    expect(running.polls, 'one entry per poll slot').toHaveLength(4);
    const used = running.polls.flatMap((p, slot) => (p ? [{ slot, ...p }] : []));
    for (const p of used) {
      expect(p.owner === 'runtime' || records.includes(p.owner), `poll ${p.slot} owner ${p.owner}`).toBe(true);
      expect(p.fn, `poll ${p.slot} function`).toBeGreaterThan(0);
    }
    expect((await dk.profileState('stop')).running).toBe(false);
    const trace = await dk.profileTrace();
    expect(trace.events.some((event) => event.owner === debug.index && event.durationTicks > 0), 'captured the debug tick on the firmware clock').toBe(true);
    const first = await dk.profileAll();
    await waitFrames(rt, 600);
    const second = await dk.profileAll();
    expect(second.running).toBe(false);
    expect(
      second.spans.map((s) => [s.calls, s.sumTicks, s.maxTicks]),
      'stopped: the counts do not change',
    ).toEqual(first.spans.map((s) => [s.calls, s.sumTicks, s.maxTicks]));
    await session.expectNoFault('profile spans');
  });
});
