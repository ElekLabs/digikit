// The Firmware page's state (packages/workbench-core/src/firmware/firmware-state.ts): a file of a device that has no hardware image is
// refused before patchkit is asked.
import assert from 'node:assert/strict';
import { expect, test, vi } from 'vitest';
import { registry } from '../src/data/device-profiles.ts';
import { createFirmwareState } from '../src/firmware/firmware-state.ts';
import { memoryKeyValue } from '../src/storage.ts';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';
import type { TransferApp } from '../src/transfer/app.ts';

const emulatorOnly = registry.devices.find((d) => !d.patchSets.device && d.firmware.length > 0);

// the bytes are not a real image: the hash stands for the registry's firmware row of the device the test sets
const hashed = vi.hoisted(() => ({ sha: undefined as string | undefined }));
vi.mock('../src/runtime/local-inputs.ts', () => ({ sha256Hex: async () => hashed.sha ?? '' }));
hashed.sha = emulatorOnly?.firmware[0]?.sha256;

test('a file of a device without a hardware image gives the error and patchkit is not asked', async () => {
  assert.ok(emulatorOnly, 'the registry has a device without a device patch set');
  const prepare = vi.fn();
  const env = { log: () => {}, prepare } as unknown as FirmwareEnv;
  const transfer = {
    target: { target: () => ({ kind: 'emulator' }) },
    connection: { asked: async () => ({}), noteConnection: () => {} },
    stock: { remember: async () => {} },
  } as unknown as TransferApp;
  const defaults = createDefaultModules(env, memoryKeyValue());
  const state = createFirmwareState({ env, transfer, defaults });
  await state.chooseFile(new File([new Uint8Array([1, 2, 3])], 'stock.syx'));
  const p = state.prepared();
  assert.equal(p.kind, 'error');
  assert.match((p as { message: string }).message, new RegExp(`no hardware image for the ${emulatorOnly.name} yet`));
  expect(prepare).not.toHaveBeenCalled();
  assert.equal(state.fileDevice()?.id, emulatorOnly.id);
});
