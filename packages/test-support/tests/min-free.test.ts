import assert from 'node:assert/strict';
import { test } from 'vitest';
import { minFreeGb } from '../src/session.ts';

test('minFreeGb defaults to 5 and reads the override', () => {
  assert.equal(minFreeGb(undefined), 5);
  assert.equal(minFreeGb(''), 5);
  assert.equal(minFreeGb('0'), 0);
  assert.equal(minFreeGb('12.5'), 12.5);
});

test('minFreeGb rejects garbage and negatives', () => {
  assert.throws(() => minFreeGb('lots'), /DIGI_MIN_FREE_GB/);
  assert.throws(() => minFreeGb('-1'), /DIGI_MIN_FREE_GB/);
});
