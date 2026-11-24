// The Firmware page's state (packages/workbench-core/src/firmware/firmware-state.ts): a file of a device that has no hardware image is
// refused before patchkit is asked.
import assert from 'node:assert/strict';
import { expect, test, vi } from 'vitest';
import { createFirmwareState } from '../src/firmware/firmware-state.ts';
import { memoryKeyValue } from '../src/storage.ts';
import { createDefaultModules } from '../src/firmware/default-modules-state.ts';
import type { FirmwareEnv } from '../src/firmware/env.ts';
import type { TransferApp } from '../src/transfer/app.ts';
import { createJobs } from '../src/jobs/jobs.ts';
import type { SystemPackage } from '../src/runtime/activate-system-package.ts';

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
function page(over: Partial<FirmwareEnv>, target?: TransferApp['target']) {
  const prepare = vi.fn(
    async () =>
      ({ ok: true, restore: false, device: 'locked', imageId: '0123456789abcdef', image: new Uint8Array(0) }) as never,
  );
  const env = { log: () => {}, prepare, ...over } as unknown as FirmwareEnv;
  const transfer = {
    target: target ?? { target: () => ({ kind: 'emulator' }) },
    jobs: createJobs(),
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

test.each(['firmware', 'route'] as const)(
  'changing the %s while reading the package stops the flash before transport',
  async (changed) => {
    fake.chosen = 'locked';
    let finishPackage!: (pkg: SystemPackage) => void;
    const pending = new Promise<SystemPackage>((resolve) => {
      finishPackage = resolve;
    });
    const systemPackage = vi.fn(() => pending);
    const portTransport = vi.fn();
    const rpc = vi.fn();
    const sender = vi.fn();
    const target = {
      target: () => ({ kind: 'midi', port: 'Test MIDI' }),
      bindSession: () => ({ check: () => {} }),
      portTransport,
    } as unknown as TransferApp['target'];
    const { state } = page({ systemPackage, rpc, sender }, target);

    await pick(state);
    const flashing = state.startFlash();
    expect(systemPackage).toHaveBeenCalledOnce();

    if (changed === 'firmware') await pick(state);
    else state.setRoute('menu');
    finishPackage({ runtime: 0, bytes: new Uint8Array(), plan: async () => [] });
    await flashing;

    expect(state.run()).toMatchObject({ phase: 'failed', message: expect.stringMatching(/flash route changed/) });
    expect(rpc).not.toHaveBeenCalled();
    expect(sender).not.toHaveBeenCalled();
    expect(portTransport).not.toHaveBeenCalled();
  },
);
