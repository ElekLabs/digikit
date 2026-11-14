// The Custom boot screen toggle of the Firmware page (apps/web/src/firmware/default-modules-state.ts): off by default, kept in the app's storage,
// and the patch set the page prepares follows it.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import { memoryKeyValue } from '../src/storage.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';

const env = { log: () => {} } as unknown as FirmwareEnv;

test('off by default: the page patches the device set', () => {
  const s = createDefaultModules(env, memoryKeyValue());
  assert.equal(s.customBootScreen(), false);
  assert.equal(s.patchSet(), 'device');
});

test('on: the introblank set, and the choice is kept', () => {
  const storage = memoryKeyValue();
  const s = createDefaultModules(env, storage);
  s.setCustomBootScreen(true);
  assert.equal(s.patchSet(), 'device-introblank');
  assert.equal(storage.data.get('digikit.customBootScreen'), '1');
  s.setCustomBootScreen(false);
  assert.equal(s.patchSet(), 'device');
  assert.equal(storage.data.get('digikit.customBootScreen'), '0');
});

test('a new app on the same storage starts from the kept choice', () => {
  const storage = memoryKeyValue({ 'digikit.customBootScreen': '1' });
  assert.equal(createDefaultModules(env, storage).patchSet(), 'device-introblank');
});
