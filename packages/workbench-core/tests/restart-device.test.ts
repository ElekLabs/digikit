import { afterEach, expect, test, vi } from 'vitest';
import type { DkSession } from '@digi/dk-session';
import { restartDevice } from '../src/modules/dktransfer';

afterEach(() => void vi.useRealTimers());

const fakeSession = (waitForRestart: (o: unknown) => Promise<{ name: string }[]>) => {
  const calls: string[] = [];
  const s = {
    info: async () => (calls.push('info'), { runtime: 0x205, card_read_only: false }),
    restart: async () => void calls.push('restart'),
    waitForRestart: async (o: unknown) => (calls.push('waitForRestart'), waitForRestart(o)),
  } as unknown as DkSession;
  return { s, calls };
};

test('restartDevice: RESTART, then one waitForRestart on the session clock, no timer of its own', async () => {
  vi.useFakeTimers(); // a setTimeout in restartDevice would hang the test: nothing advances the timers
  const seen: unknown[] = [];
  const { s, calls } = fakeSession(async (o) => (seen.push(o), [{ name: 'a' }, { name: 'b' }]));
  const events: { event: string }[] = [];
  const stillBooting = () => false;
  const out = await restartDevice(s, (e) => events.push(e as { event: string }), {
    bootTimeoutMs: 123,
    loadedTimeoutMs: 456,
    stillBooting,
  });
  expect(out).toEqual({ loaded: ['a', 'b'], missing: [] });
  expect(calls).toEqual(['info', 'restart', 'waitForRestart']);
  expect(events.map((e) => e.event)).toEqual(['restart', 'wait', 'done']);
  expect(seen).toEqual([{ timeoutMs: 123, loadedTimeoutMs: 456, stillBooting }]);
  expect(vi.getTimerCount()).toBe(0);
});

test('restartDevice: a refused or failed wait passes the error on without a done event', async () => {
  const { s } = fakeSession(async () => {
    throw new Error('no new boot');
  });
  const events: { event: string }[] = [];
  await expect(restartDevice(s, (e) => events.push(e as { event: string }))).rejects.toThrow('no new boot');
  expect(events.map((e) => e.event)).toEqual(['restart', 'wait']);
});
