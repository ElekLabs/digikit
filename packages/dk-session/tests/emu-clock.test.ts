import assert from 'node:assert/strict';
import { afterEach, beforeEach, test, vi } from 'vitest';
import { emulatedClock, emulatedProbe, poll, INSNS_PER_MS, type EmuStatus } from '@digi/dk-session';

beforeEach(() => void vi.useFakeTimers());
afterEach(() => void vi.useRealTimers());

test('now is the emulated ms of elapsed, falls back to DSP frames', async () => {
  let status: EmuStatus = { elapsed: 5 * INSNS_PER_MS };
  const clock = emulatedClock(async () => status);
  assert.equal(await clock.now(), 5);
  status = { audio: { frames: 300 } };
  assert.equal(await clock.now(), 200);
});

test('pause waits ms of wall time and reads nothing: the next now() reads the status', async () => {
  let reads = 0;
  const clock = emulatedClock(async () => ({ elapsed: ++reads }));
  const paused = clock.pause(1000);
  await vi.advanceTimersByTimeAsync(999);
  assert.equal(reads, 0);
  await vi.advanceTimersByTimeAsync(1);
  await paused;
  assert.equal(reads, 0);
  await clock.now();
  assert.equal(reads, 1);
});

test('pauseEmulated returns when emulated time has advanced by ms, whatever the wall time', async () => {
  let elapsed = 0;
  const clock = emulatedClock(async () => ({ elapsed }));
  let done = false;
  const paused = clock.pauseEmulated!(1000).then(() => (done = true));
  for (let i = 0; i < 9; i++) {
    elapsed += 100 * INSNS_PER_MS;
    await vi.advanceTimersByTimeAsync(30);
    assert.equal(done, false);
  }
  elapsed += 100 * INSNS_PER_MS;
  await vi.advanceTimersByTimeAsync(30);
  await paused;
  assert.equal(done, true);
});

test('pauseEmulated counts only forward movement of the clock', async () => {
  let elapsed = 500 * INSNS_PER_MS;
  const clock = emulatedClock(async () => ({ elapsed }));
  let done = false;
  const paused = clock.pauseEmulated!(100).then(() => (done = true));
  await vi.advanceTimersByTimeAsync(30);
  elapsed = 0; // a restore drops the clock
  await vi.advanceTimersByTimeAsync(30);
  elapsed = 60 * INSNS_PER_MS;
  await vi.advanceTimersByTimeAsync(30);
  assert.equal(done, false);
  elapsed = 160 * INSNS_PER_MS;
  await vi.advanceTimersByTimeAsync(30);
  await paused;
});

test('pauseEmulated fails with the emulator state when emulated time stands still', async () => {
  const clock = emulatedClock(async () => ({ mode: 'paused', elapsed: 7 * INSNS_PER_MS }));
  const paused = clock.pauseEmulated!(1000).catch((e: Error) => e);
  await vi.advanceTimersByTimeAsync(20 * 1000 + 60_000 + 100);
  const err = await paused;
  assert.match((err as Error).message, /pauseEmulated\(1000 ms\): emulated clock stuck at 7\.0 ms.*mode paused/);
});

test('a poll on emulatedClock trips when the clock stands still and no job runs', async () => {
  let n = 0;
  const clock = emulatedClock(async () => ({ mode: 'paused', elapsed: 5000, cf: { icount: n++ } }) as EmuStatus);
  const out = poll(clock, async () => 0, Boolean, { withinMs: 1000, intervalMs: 5, hangMs: 40, message: 'm' }).then(
    () => 'ok',
    (e: Error) => e,
  );
  for (let i = 0; i < 20; i++) await vi.advanceTimersByTimeAsync(10);
  const error = await out;
  assert.ok(error instanceof Error && /emulated clock stuck/.test(error.message), String(error));
  assert.ok(/mode paused/.test((error as Error).message), String(error));
});

test("a job's icount counts as movement while the clock stands still", async () => {
  let icount = 0;
  const clock = emulatedClock(
    async () => ({ audio: { frames: 1000 }, job: { kind: 'boot', icount: (icount += 1000) } }) as EmuStatus,
  );
  let reads = 0;
  const out = poll(clock, async () => ++reads > 30, Boolean, {
    withinMs: 1000,
    intervalMs: 5,
    hangMs: 40,
    message: 'm',
  });
  for (let i = 0; i < 40; i++) await vi.advanceTimersByTimeAsync(10);
  assert.equal(await out, true);
});

test("a dropping clock restarts the poll's count", async () => {
  let elapsed = 100 * INSNS_PER_MS;
  const clock = emulatedClock(async () => ({ elapsed: (elapsed -= INSNS_PER_MS) }));
  let reads = 0;
  const out = poll(clock, async () => ++reads > 5, Boolean, {
    withinMs: 100_000,
    intervalMs: 3000,
    hangMs: 5000,
    message: 'm',
  });
  for (let i = 0; i < 8; i++) await vi.advanceTimersByTimeAsync(3000);
  assert.equal(await out, true);
});

test('a halted DSP does not stop the clock', async () => {
  let elapsed = 0;
  const clock = emulatedClock(async () => ({ mode: 'free', audio: { frames: 1000 }, elapsed: (elapsed += 1_320_000) }));
  const out = poll(clock, async () => false, Boolean, { withinMs: 50, intervalMs: 1, hangMs: 40, message: 'm' }).then(
    () => 'ok',
    (e: Error) => e,
  );
  for (let i = 0; i < 20; i++) await vi.advanceTimersByTimeAsync(5);
  const error = await out;
  assert.ok(error instanceof Error && /m: not seen within 50 ms/.test(error.message), String(error));
});

test('emulatedProbe ends a lost probe after its emulated bound, however slow the wall clock', async () => {
  vi.useRealTimers();
  let t = 0;
  const probe = emulatedProbe({ now: async () => (t += 10) });
  const out = await probe(
    ({ signal }) =>
      new Promise<string>((resolve) => signal.addEventListener('abort', () => resolve('aborted'), { once: true })),
    200,
  );
  assert.equal(out, 'aborted');
});

test('a probe that answers is not cut', async () => {
  vi.useRealTimers();
  let t = 0;
  const probe = emulatedProbe({ now: async () => (t += 10) });
  const out = await probe(async ({ signal }) => (signal.aborted ? 'aborted' : 'answer'), 200);
  assert.equal(out, 'answer');
});
