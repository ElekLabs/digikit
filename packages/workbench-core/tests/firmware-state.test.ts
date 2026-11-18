// The Firmware page's state (packages/workbench-core/src/firmware/firmware-state.ts): a file of a device that has no hardware image is
// refused before patchkit is asked.
import assert from 'node:assert/strict';
import { expect, test, vi } from 'vitest';
import { createFirmwareState } from '../src/firmware/firmware-state.ts';
import { memoryKeyValue } from '../src/storage.ts';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';
import type { TransferApp } from '../src/transfer/app.ts';

// a device that has no `device` patch set (the registry's own devices all have one now): the registry and the file identity answer for it
const noHardware = vi.hoisted(() => ({
  id: 'nohw',
  record: { id: 'nohw', name: 'Nohardware', patchSets: {} as Record<string, string[]>, firmware: [] },
}));
vi.mock('../src/patch/identify.ts', () => ({ identify: () => ({ device: noHardware.id }) }));
vi.mock('../src/data/device-profiles.ts', async (importOriginal) => {
  const m = await importOriginal<typeof import('../src/data/device-profiles.ts')>();
  const registry = new Proxy(m.registry, {
    get(target, key) {
      if (key === 'get') return (id: string) => (id === noHardware.id ? noHardware.record : target.get(id));
      const v = Reflect.get(target, key) as unknown;
      return typeof v === 'function' ? (v as (...a: unknown[]) => unknown).bind(target) : v;
    },
  });
  return { ...m, registry };
});
vi.mock('../src/runtime/local-inputs.ts', () => ({ sha256Hex: async () => '0'.repeat(64) }));
const emulatorOnly = noHardware.record;

test('a file of a device without a hardware image gives the error and patchkit is not asked', async () => {
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
