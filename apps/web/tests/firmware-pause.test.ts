// The sender's short pause (apps/web/src/firmware/pause.ts): it waits at least the time, short or long.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { pause } from '../src/firmware/pause.ts';

test('a pause lasts at least its time', async () => {
  for (const ms of [1, 2, 5, 12]) {
    const t = performance.now();
    await pause(ms);
    assert.ok(performance.now() - t >= ms - 0.5, `${ms} ms`);
  }
});
