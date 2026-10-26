import { afterEach, expect, test, vi } from 'vitest';
import { timePhase } from '../timing.ts';

afterEach(() => vi.restoreAllMocks());

function capture() {
  const output = vi.spyOn(console, 'info').mockImplementation(() => {});
  return () => output.mock.calls.map(([line]) => JSON.parse(line as string));
}

test('timing preserves the returned value and attributes nested inclusive phases', async () => {
  const records = capture();
  const value = {};
  expect(
    await timePhase('body', () => timePhase('upload', async () => value), {
      scope: 'scenario',
      device: 'dt2',
      session: 'session-1',
    }),
  ).toBe(value);
  const [child, parent] = records();
  expect(child).toMatchObject({
    event: 'runtime-phase',
    phase: 'upload',
    scope: 'scenario',
    device: 'dt2',
    session: 'session-1',
    parentSpan: parent.span,
    durationKind: 'inclusive',
    outcome: 'success',
  });
  expect(parent.parentSpan).toBeNull();
  expect(child.span).not.toBe(parent.span);
  expect(child.durationMs).toBeGreaterThanOrEqual(0);
  expect(parent.durationMs).toBeGreaterThanOrEqual(child.durationMs);
});

test.each(['sync', 'async'] as const)('timing preserves the original %s error and reports failure', async (kind) => {
  const records = capture();
  const error = new Error('original');
  await expect(
    timePhase('failure', () => {
      if (kind === 'sync') throw error;
      return Promise.reject(error);
    }),
  ).rejects.toBe(error);
  expect(records()).toEqual([expect.objectContaining({ phase: 'failure', outcome: 'failure' })]);
});

test('concurrent phases retain their own parent context', async () => {
  const records = capture();
  await Promise.all(
    ['dt2', 'dn2'].map((device) =>
      timePhase(
        'body',
        async () => {
          await Promise.resolve();
          await timePhase('settle', async () => {});
        },
        { device },
      ),
    ),
  );
  const all = records();
  for (const child of all.filter((record) => record.phase === 'settle')) {
    expect(all.find((record) => record.span === child.parentSpan)).toMatchObject({
      device: child.device,
      phase: 'body',
    });
  }
});

test('diagnostic output failure preserves results and errors', async () => {
  vi.spyOn(console, 'info').mockImplementation(() => {
    throw new Error('output failed');
  });
  expect(await timePhase('success', async () => 42)).toBe(42);
  const error = new Error('operation failed');
  await expect(timePhase('failure', () => Promise.reject(error))).rejects.toBe(error);
});
