import { expect } from 'vitest';
import type { PatchedSession } from '@digi/regression/support.ts';
import { waitFrames } from '@digi/regression/runtime/debug-kit.ts';
import { installWithDebug } from '@digi/regression/runtime/install.ts';
import { expectRuntimeReady, runtimeTests } from '@digi/regression/runtime/ready.ts';

/** Spans: reset and start, a short wait, then the table has the debug module's own tick row (calls > 0, max <= sum) at the span clock rate (132 MHz, the bus).
 * After stop the counts stay the same. */
async function expectProfileSpans(runtime: PatchedSession): Promise<void> {
  const { session, dk } = runtime;
  await expectRuntimeReady(session);
  await installWithDebug(runtime);
  const debug = (await dk.modules()).find((m) => m.name === 'digikit/debug')!;
  await dk.profileState('reset');
  expect((await dk.profileState('startTrace')).running).toBe(true);
  await waitFrames(runtime, 600);
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
  console.log('poll owners', JSON.stringify(used.map((p) => ({ ...p, fn: p.fn.toString(16) }))));

  expect((await dk.profileState('stop')).running).toBe(false);
  const trace = await dk.profileTrace();
  expect(trace.events.some((event) => event.owner === debug.index && event.durationTicks > 0), 'captured the debug tick on the firmware clock').toBe(true);
  const first = await dk.profileAll();
  await waitFrames(runtime, 600);
  const second = await dk.profileAll();
  expect(second.running).toBe(false);
  expect(
    second.spans.map((s) => [s.calls, s.sumTicks, s.maxTicks]),
    'stopped: the counts do not change',
  ).toEqual(first.spans.map((s) => [s.calls, s.sumTicks, s.maxTicks]));
  await session.expectNoFault('profile spans');
}

runtimeTests('the span table counts the debug module tick and holds still when stopped', expectProfileSpans, { dsp: 'sharc' });
