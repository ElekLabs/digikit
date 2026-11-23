import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, expect, test, vi } from 'vitest';
import type { Device } from '@digi/test-support';
import { FIRMWARE } from '../runtime/devices.ts';
import { liveOptions } from '../runtime/live-options.ts';

const device = Object.keys(FIRMWARE)[0] as Device;
let root: string;
beforeAll(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-live-options-'));
  const binary = path.join(root, 'git', 'emud-cache', device, 'testkey', 'digi-emud');
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  vi.stubEnv(`DIGI_EMUD_${device.toUpperCase()}`, binary);
  vi.stubEnv('DIGI_SNAP_STORE', path.join(root, 'snaps'));
});
afterAll(() => {
  vi.unstubAllEnvs();
  fs.rmSync(root, { recursive: true, force: true });
});

test('every live session shares the snapshot store; only a mirror or fresh one gets its own run dir', () => {
  const plain = liveOptions(device);
  const mirror = liveOptions(device, { mirror: true });
  const fresh = liveOptions(device, { fresh: true });
  expect(plain.snapshotDir).toBe(path.join(root, 'snaps', 'testkey', device));
  expect(mirror.snapshotDir).toBe(plain.snapshotDir);
  expect(fresh.snapshotDir).toBe(plain.snapshotDir);
  expect(plain.runDir).toBeDefined();
  expect(mirror.runDir).toBeUndefined();
  expect(fresh.runDir).toBeUndefined();
});

test('a mirror or fresh session does not claim the cached run dir', async () => {
  vi.resetModules();
  const claim = vi.fn();
  vi.doMock('../runtime/devices.ts', async (original) => {
    const real = await original<typeof import('../runtime/devices.ts')>();
    return { ...real, cachedRunDir: claim };
  });
  const { liveOptions: fresh } = await import('../runtime/live-options.ts');
  fresh(device, { mirror: true });
  fresh(device, { fresh: true });
  expect(claim).not.toHaveBeenCalled();
  vi.doUnmock('../runtime/devices.ts');
});
