import assert from 'node:assert/strict';
import { afterEach, beforeEach, test, vi } from 'vitest';
import { poll, stuckGuard, waitClock, type Clock } from '@digi/dk-session';

beforeEach(() => void vi.useFakeTimers());
afterEach(() => void vi.useRealTimers());

function manualClock(): Clock & { at: number; pauses: number[] } {
  const clock = {
    at: 0,
    pauses: [] as number[],
    async now() {
      return clock.at;
    },
    async pause(ms: number) {
      clock.pauses.push(ms);
      clock.at += ms;
    },
  };
  return clock;
}

/** Run `p` while fake time passes in 500 ms steps (5 s in all). */
const settle = async (p: Promise<unknown>) => {
  const out = p.then(
    () => 'ok',
    (e: Error) => e,
  );
  for (let i = 0; i < 10; i++) await vi.advanceTimersByTimeAsync(500);
  return out;
};

test('poll returns at once without reading the clock', async () => {
  const clock = manualClock();
  clock.now = async () => {
    throw new Error('clock read');
  };
  assert.equal(
    await poll(
      clock,
      async () => 7,
      (v) => v === 7,
      { withinMs: 100, message: 'm' },
    ),
    7,
  );
});

test('poll retries a failing read and calls onWait', async () => {
  const clock = manualClock();
  let calls = 0;
  const waits: boolean[] = [];
  const value = await poll(
    clock,
    async () => {
      if (++calls < 3) throw new Error('lost');
      return calls;
    },
    (v) => v === 3,
    { withinMs: 1000, message: 'm', retry: true, onWait: (r) => waits.push(r.ok) },
  );
  assert.equal(value, 3);
  assert.deepEqual(waits, [false, false, true]);
  assert.deepEqual(clock.pauses, [200, 200]);
});

test('poll fails after withinMs with the lazy message and the last value', async () => {
  const clock = manualClock();
  let resolved = 0;
  await assert.rejects(
    poll(
      clock,
      async () => 'x',
      () => false,
      { withinMs: 500, intervalMs: 100, message: () => `lazy ${++resolved}` },
    ),
    /lazy 1: not seen within 500 ms of emulated time \(last: "x"\)$/,
  );
  assert.equal(resolved, 1);
});

test('poll without retry rethrows a failing read', async () => {
  await assert.rejects(
    poll(
      manualClock(),
      async () => Promise.reject(new Error('boom')),
      () => true,
      { withinMs: 10, message: 'm' },
    ),
    /boom/,
  );
});

test('poll retry function: false rethrows that error, true retries', async () => {
  let calls = 0;
  const read = async () => {
    calls++;
    throw new Error(calls === 1 ? 'transient' : 'fatal');
  };
  await assert.rejects(
    poll(manualClock(), read, () => true, {
      withinMs: 1000,
      message: 'm',
      retry: (e) => String(e) !== 'Error: fatal',
    }),
    /fatal/,
  );
  assert.equal(calls, 2);
});

test('waitClock lets the time pass in steps of at most 50 ms', async () => {
  const clock = manualClock();
  await waitClock(clock, 120);
  assert.deepEqual(clock.pauses, [50, 50, 20]);
  await waitClock(clock, 0);
  assert.equal(clock.pauses.length, 3);
});

test('waitClock and poll fail with a clear message when the clock never moves', async () => {
  const slowFrozen: Clock = { now: async () => 7, pause: (ms) => new Promise((r) => setTimeout(r, ms)) };
  const waited = await settle(waitClock(slowFrozen, 1000, { hangMs: 2000 }));
  assert.ok(
    waited instanceof Error && /wait of 1000 ms: emulated clock stuck at 7 ms/.test(waited.message),
    String(waited),
  );
  const polled = await settle(
    poll(slowFrozen, async () => 0, Boolean, { withinMs: 10_000, message: 'ready', hangMs: 2000, intervalMs: 100 }),
  );
  assert.ok(polled instanceof Error && /ready: emulated clock stuck at 7 ms/.test(polled.message), String(polled));
});

test('a changed progress token is movement', async () => {
  let token = 0;
  const clock: Clock = {
    now: async () => 7,
    pause: (ms) => new Promise((r) => setTimeout(r, ms)),
    progress: () => String(token++),
  };
  // `now` is frozen but the token moves: after 5 s of wall time, more than twice the 2 s hang limit, the wait has not failed as stuck
  let ended: unknown;
  void poll(clock, async () => 0, Boolean, { withinMs: 10_000, message: 'm', hangMs: 2000, intervalMs: 100 }).then(
    (v) => (ended = v),
    (e: unknown) => (ended = e),
  );
  for (let i = 0; i < 10; i++) await vi.advanceTimersByTimeAsync(500);
  assert.equal(ended, undefined, String(ended));
});

test('stuckGuard names the clock detail', async () => {
  const clock: Clock = { now: async () => 1, pause: async () => {}, describe: async () => '[mode paused]' };
  const guard = stuckGuard('w', 0, clock);
  await guard(1);
  await assert.rejects(guard(1), /w: emulated clock stuck at 1 ms for 0 ms.* \[mode paused\]$/);
});
