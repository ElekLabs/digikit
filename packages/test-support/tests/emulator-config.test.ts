import assert from 'node:assert/strict';
import { test } from 'vitest';
import { snapshotCapBytes } from '../src/emulator-config.ts';

const GIB = 1024 ** 3;

test('snapshotCapBytes: default, env value, invalid value', () => {
  assert.equal(snapshotCapBytes({}), 20 * GIB);
  assert.equal(snapshotCapBytes({ DIGI_SNAP_CAP_GIB: '5' }), 5 * GIB);
  for (const bad of ['abc', '', '-3', '0']) assert.equal(snapshotCapBytes({ DIGI_SNAP_CAP_GIB: bad }), 20 * GIB);
});
