import { withViewObserver } from '@digi/dk-session';
import { installWithDebug } from '@digi/regression/runtime/debug-install.ts';
import { expectRuntimeReady } from '@digi/regression/runtime/ready.ts';
import { runtimeTests } from '@digi/regression/runtime/runtime-tests.ts';
import { emulatorRead, readViewStack, viewStackFacts } from '@digi/test-support';
import { expect } from 'vitest';

runtimeTests('debug observes SETTINGS open and close from the stock view stack', async (rt) => {
  const { dk, session } = rt;
  await expectRuntimeReady(session);
  await installWithDebug(rt);
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
