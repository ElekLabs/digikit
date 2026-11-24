import { registry } from '@digi/workbench-core/data/device-profiles.ts';
import { createFakeHost } from '@digi/workbench-core/host/fake-host.ts';
import { memoryKeyValue } from '@digi/workbench-core/storage.ts';
import { createFakeTransferApp } from '@digi/workbench-core/transfer/fake-app.ts';
import { createRoot } from 'solid-js';
import { expect, test, vi } from 'vitest';
import { createEmulatorActions } from '../src/emulator-actions';
import { createWorkbenchEmulator } from '../src/emulator/emulator';
import { createLogDrawer } from '../src/log-state';

test('Reload with runtime patches the running stock firmware and makes Transfer available', async () => {
  const file = { name: 'stock.syx', path: '/tmp/stock.syx', modified_ms: 1, bytes: 1024 };
  const device = registry.runtimeIds[0]!;
  const snapshot = { status: { device, version: '1.0', ready: true, error: null } };
  let patched = false;
  const host = createFakeHost({
    'emu.startup': snapshot,
    'emu.step': snapshot,
    'emu.launch': { loaded: true, syx: file.path, card: null, runtime: false },
    'emu.sourceInfo': file,
    'emu.setSource': file,
    'emu.buildInfo': undefined,
    'rpc.call': (method: string, params: { runtime?: boolean }) => {
      if (method === 'status') return { firmware: { runtime: patched ? { patched: true } : null } };
      if (method === 'firmware.load') {
        patched = params.runtime === true;
        return {};
      }
      throw new Error(`Unexpected emulator method: ${method}`);
    },
  });
  const loads = () => host.argsOf('rpc.call').filter(([method]) => method === 'firmware.load');
  const storage = memoryKeyValue({ 'digikit.bootRuntime': 'off' });
  const actions = createEmulatorActions();
  const app = createFakeTransferApp({ storage, env: { extras: { reloadWithRuntime: actions.reloadWithRuntime } } });
  const { log, logWriter } = createLogDrawer(storage);
  const noop = () => {};

  vi.useFakeTimers();
  const { dispose, firmware } = createRoot((dispose) => ({
    dispose,
    ...createWorkbenchEmulator({
      host,
      actions,
      storage,
      jobs: app.jobs,
      target: app.target,
      connection: app.connection,
      defaults: { tickedFor: () => [], install: async () => [] },
      logWriter,
      page: () => 'run',
      pollAudio: async () => {},
      pollHealth: async () => {},
      showSnapshot: noop,
      releaseHeldKeys: noop,
      setError: noop,
      setReloadNote: noop,
      watchSnapshotError: noop,
      resetHealth: noop,
      controllerBusy: () => false,
      setBrowseDevice: app.state.setBrowseDevice,
      clearInventory: app.inventory.clear,
      bumpCardRefresh: noop,
    }),
  }));
  try {
    await vi.advanceTimersByTimeAsync(0);
    expect(firmware.source()?.path).toBe(file.path);
    expect(app.target.emulatorRuntime()).toBe('no');
    expect(app.target.libraryOnly()).toBe(true);
    expect(loads()).toHaveLength(0);

    app.env.extras!.reloadWithRuntime!();
    expect(app.jobs.busy('emulator')).toBe(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(loads().map(([, params]) => params)).toEqual([expect.objectContaining({ syx: file.path, runtime: true })]);
    expect(app.target.emulatorRuntime()).toBe('yes');
    expect(app.target.libraryOnly()).toBe(false);
    expect(app.jobs.busy('emulator')).toBe(false);
    expect(storage.get('digikit.bootRuntime')).toBe('on');
    expect(log.entries()).toContainEqual(
      expect.objectContaining({ text: expect.stringContaining('loaded stock.syx') }),
    );

    dispose();
    app.env.extras!.reloadWithRuntime!();
    await vi.advanceTimersByTimeAsync(2500);
    expect(loads()).toHaveLength(1);
    expect(host.argsOf('emu.stop')).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    dispose();
    vi.useRealTimers();
  }
});
