import assert from 'node:assert/strict';
import { afterEach, beforeEach, test, vi } from 'vitest';
import { emulatorClock, pollUntil, type Clock } from '../src/emulated-clock.ts';
import { MS } from '../src/session.ts';

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

test('emulatorClock steps a paused daemon and waits on a free one', async () => {
  let mode = 'paused';
  const steps: number[] = [];
  const clock = emulatorClock({
    status: async () => ({ mode, elapsed: 0 }),
    client: { step: async (n: number) => void steps.push(n) },
  } as never);
  await clock.pause(2);
  assert.deepEqual(steps, [2 * MS]);
  mode = 'free';
  const free = clock.pause(100);
  await vi.advanceTimersByTimeAsync(100);
  await free;
  assert.deepEqual(steps, [2 * MS]);
});

test('pollUntil applies DIGI_CLOCK_STUCK_MS', async () => {
  const before = process.env.DIGI_CLOCK_STUCK_MS;
  process.env.DIGI_CLOCK_STUCK_MS = '2000';
  try {
    const slowFrozen: Clock = { now: async () => 7, pause: (ms) => new Promise((r) => setTimeout(r, ms)) };
    const out = pollUntil(slowFrozen, async () => 0, Boolean, {
      withinMs: 100_000,
      message: 'ready',
      intervalMs: 100,
    }).then(
      () => 'ok',
      (e: Error) => e,
    );
    for (let i = 0; i < 10; i++) await vi.advanceTimersByTimeAsync(500);
    const error = await out;
    assert.ok(error instanceof Error && /emulated clock stuck/.test(error.message), String(error));
  } finally {
    if (before === undefined) delete process.env.DIGI_CLOCK_STUCK_MS;
    else process.env.DIGI_CLOCK_STUCK_MS = before;
  }
});
