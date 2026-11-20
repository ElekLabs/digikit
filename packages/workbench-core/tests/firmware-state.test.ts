// The Firmware page's state (packages/workbench-core/src/firmware/firmware-state.ts): a file of a device that has no hardware image is
// refused before patchkit is asked.
import assert from 'node:assert/strict';
import { expect, test, vi } from 'vitest';
import { createFirmwareState } from '../src/firmware/firmware-state.ts';
import { memoryKeyValue } from '../src/storage.ts';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';
import type { TransferApp } from '../src/transfer/app.ts';

// devices the registry does not have: one with no `device` patch set, one with a set that the site's policy does not list; the registry and the file identity answer for them
const fake = vi.hoisted(() => {
  const record = (id: string, name: string, patchSets: Record<string, string[]>) => ({
    id,
    name,
    patchSets,
    firmware: [],
  });
  return {
    chosen: 'nohw',
    records: {
      nohw: record('nohw', 'Nohardware', {}),
      locked: record('locked', 'Sitelocked', { device: ['core/x'] }),
    } as Record<string, ReturnType<typeof record>>,
  };
});
vi.mock('../src/patch/identify.ts', () => ({ identify: () => ({ device: fake.chosen }) }));
vi.mock('../src/data/device-profiles.ts', async (importOriginal) => {
  const m = await importOriginal<typeof import('../src/data/device-profiles.ts')>();
  const registry = new Proxy(m.registry, {
    get(target, key) {
      if (key === 'get') return (id: string) => fake.records[id] ?? target.get(id);
      const v = Reflect.get(target, key) as unknown;
      return typeof v === 'function' ? (v as (...a: unknown[]) => unknown).bind(target) : v;
    },
  });
  return { ...m, registry };
});
vi.mock('../src/runtime/local-inputs.ts', () => ({ sha256Hex: async () => '0'.repeat(64) }));

/** The Firmware page state over a fake env; `prepare` answers like patchkit would. */
function page(over: Partial<FirmwareEnv>) {
  const prepare = vi.fn(
    async () => ({ ok: true, restore: false, device: 'locked', image: new Uint8Array(0) }) as never,
  );
  const env = { log: () => {}, prepare, ...over } as unknown as FirmwareEnv;
  const transfer = {
    target: { target: () => ({ kind: 'emulator' }) },
    connection: { asked: async () => ({}), noteConnection: () => {} },
    stock: { remember: async () => {} },
  } as unknown as TransferApp;
  const defaults = createDefaultModules(env, memoryKeyValue());
  return { prepare, state: createFirmwareState({ env, transfer, defaults }) };
}
const pick = (state: ReturnType<typeof page>['state']) =>
  state.chooseFile(new File([new Uint8Array([1, 2, 3])], 'stock.syx'));

test('a file of a device without a hardware image gives the error and patchkit is not asked', async () => {
  fake.chosen = 'nohw';
  const { prepare, state } = page({});
  await pick(state);
  const p = state.prepared();
  assert.equal(p.kind, 'error');
  assert.match((p as { message: string }).message, /no hardware image for the Nohardware yet/);
  expect(prepare).not.toHaveBeenCalled();
  assert.equal(state.fileDevice()?.id, 'nohw');
});

test('the site prepares no patched image of a device outside its list; the workbench offers every device with a set', async () => {
  fake.chosen = 'locked';
  const site = page({ offersPatched: (id) => id === 'listed' });
  await pick(site.state);
  const p = site.state.prepared();
  assert.equal(p.kind, 'error');
  assert.match((p as { message: string }).message, /does not offer a patched image for the Sitelocked/);
  expect(site.prepare).not.toHaveBeenCalled();

  // restoring stock stays available on the site
  site.state.setRestore(true);
  await vi.waitFor(() => expect(site.prepare).toHaveBeenCalledTimes(1));

  // a device the site lists is prepared
  const listed = page({ offersPatched: (id) => id === 'locked' });
  await pick(listed.state);
  await vi.waitFor(() => expect(listed.prepare).toHaveBeenCalledTimes(1));
  assert.notEqual(listed.state.prepared().kind, 'error');

  const workbench = page({});
  await pick(workbench.state);
  await vi.waitFor(() => expect(workbench.prepare).toHaveBeenCalledTimes(1));
  assert.notEqual(workbench.state.prepared().kind, 'error');
});
